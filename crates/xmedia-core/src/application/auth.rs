use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

use crate::application::activity::ActivityService;
use crate::application::dto::{
    AuthSessionDto, ChangePasswordInput, CreateUserInput, SetupAdminInput, SetupStatusDto,
    SubsonicCredentials, UpdateUserInput, UserDto,
};
use crate::application::ports::{
    AuthClaims, MediaDomainLifecycle, PasswordHasherPort, TokenServicePort,
};
use crate::domain::error::{DomainError, DomainResult};
use crate::domain::library::MediaKind;
use crate::domain::repository::UserRepository;
use crate::domain::user::{User, UserId, UserPreferences, UserRole, require_admin};
use crate::infra::stream::sha256_hex;

/// Computes a deterministic credential version fingerprint from a user's password hash, API key,
/// and role so password changes, admin password resets, API key rotations, and role changes
/// immediately invalidate all previously issued JWTs even across server restarts.
fn compute_user_cred_ver(user: &User) -> String {
    let material = format!(
        "{}\0{}\0{}",
        user.password_hash,
        user.api_key,
        user.role.as_str()
    );
    let full = sha256_hex(material.as_bytes());
    full[..16].to_string()
}

/// Constant-time byte-slice comparison using XOR accumulation to avoid timing side-channels
/// when comparing API keys or secrets.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (&x, &y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    std::hint::black_box(diff) == 0
}

const MAX_REVOKED_JTIS: usize = 8192;
const AUTH_RATE_LIMIT_WINDOW: Duration = Duration::from_secs(60);
const AUTH_RATE_LIMIT_MAX_FAILURES: usize = 20;
const AUTH_RATE_LIMIT_MAX_KEYS: usize = 2048;

struct RateLimiterState {
    entries: HashMap<String, VecDeque<Instant>>,
    overflow: VecDeque<Instant>,
}

struct AuthRateLimiter {
    window: Duration,
    max_failures: usize,
    state: std::sync::Mutex<RateLimiterState>,
}

impl AuthRateLimiter {
    fn new(window: Duration, max_failures: usize) -> Self {
        Self {
            window,
            max_failures: max_failures.max(1),
            state: std::sync::Mutex::new(RateLimiterState {
                entries: HashMap::new(),
                overflow: VecDeque::new(),
            }),
        }
    }

    fn normalize_key(key: &str) -> String {
        let trimmed = key.trim();
        if trimmed.is_empty() {
            "<anonymous>".to_string()
        } else {
            trimmed.to_ascii_lowercase()
        }
    }

    fn prune_hits(hits: &mut VecDeque<Instant>, now: Instant, window: Duration) {
        while hits
            .front()
            .is_some_and(|ts| now.saturating_duration_since(*ts) >= window)
        {
            hits.pop_front();
        }
    }

    fn is_limited(&self, key: &str) -> bool {
        let norm = Self::normalize_key(key);
        let mut guard = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        let window = self.window;
        if let Some(hits) = guard.entries.get_mut(&norm) {
            Self::prune_hits(hits, now, window);
            let limited = hits.len() >= self.max_failures;
            if hits.is_empty() {
                guard.entries.remove(&norm);
            }
            return limited;
        }
        if guard.entries.len() >= AUTH_RATE_LIMIT_MAX_KEYS {
            Self::prune_hits(&mut guard.overflow, now, window);
            return guard.overflow.len() >= self.max_failures;
        }
        false
    }

    /// Records a failed authentication attempt and returns `Err(TooManyRequests)` once the
    /// active failure window for `key` (or the global overflow bucket when saturated) has already
    /// reached `max_failures`. Never evicts active rate-limit windows.
    fn record_failure_and_check(&self, key: &str) -> DomainResult<()> {
        let norm = Self::normalize_key(key);
        let mut guard = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        let window = self.window;

        if let Some(hits) = guard.entries.get_mut(&norm) {
            Self::prune_hits(hits, now, window);
            if hits.len() >= self.max_failures {
                return Err(DomainError::TooManyRequests(
                    "too many failed authentication attempts; please try again later".into(),
                ));
            }
            hits.push_back(now);
            return Ok(());
        }

        if guard.entries.len() >= AUTH_RATE_LIMIT_MAX_KEYS {
            guard.entries.retain(|_, hits| {
                Self::prune_hits(hits, now, window);
                !hits.is_empty()
            });
        }

        if guard.entries.len() < AUTH_RATE_LIMIT_MAX_KEYS {
            let mut hits = VecDeque::with_capacity(4);
            hits.push_back(now);
            guard.entries.insert(norm, hits);
            return Ok(());
        }

        Self::prune_hits(&mut guard.overflow, now, window);
        if guard.overflow.len() >= self.max_failures {
            return Err(DomainError::TooManyRequests(
                "too many failed authentication attempts; please try again later".into(),
            ));
        }
        guard.overflow.push_back(now);
        Ok(())
    }

    /// Clears minor pre-lockout typo counters upon valid authentication while preserving an
    /// active brute-force lockout window so concurrent attackers remain rate-limited.
    fn clear_below_limit(&self, key: &str) {
        let norm = Self::normalize_key(key);
        let mut guard = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        let window = self.window;
        if let Some(hits) = guard.entries.get_mut(&norm) {
            Self::prune_hits(hits, now, window);
            if hits.len() < self.max_failures {
                guard.entries.remove(&norm);
            }
        }
    }
}

#[derive(Clone)]
pub struct AuthService {
    users: Arc<dyn UserRepository>,
    hasher: Arc<dyn PasswordHasherPort>,
    tokens: Arc<dyn TokenServicePort>,
    lifecycles: Arc<RwLock<HashMap<MediaKind, Arc<dyn MediaDomainLifecycle>>>>,
    activity: Option<Arc<ActivityService>>,
    setup_lock: Arc<Mutex<()>>,
    throttled_verify_lock: Arc<Mutex<()>>,
    revoked_jtis: Arc<RwLock<HashMap<String, u64>>>,
    rate_limiter: Arc<AuthRateLimiter>,
}

impl AuthService {
    pub fn new(
        users: Arc<dyn UserRepository>,
        hasher: Arc<dyn PasswordHasherPort>,
        tokens: Arc<dyn TokenServicePort>,
        lifecycles: Arc<RwLock<HashMap<MediaKind, Arc<dyn MediaDomainLifecycle>>>>,
    ) -> Self {
        Self {
            users,
            hasher,
            tokens,
            lifecycles,
            activity: None,
            setup_lock: Arc::new(Mutex::new(())),
            throttled_verify_lock: Arc::new(Mutex::new(())),
            revoked_jtis: Arc::new(RwLock::new(HashMap::new())),
            rate_limiter: Arc::new(AuthRateLimiter::new(
                AUTH_RATE_LIMIT_WINDOW,
                AUTH_RATE_LIMIT_MAX_FAILURES,
            )),
        }
    }

    pub fn with_activity(mut self, activity: Arc<ActivityService>) -> Self {
        self.activity = Some(activity);
        self
    }

    pub fn has_activity_cleanup(&self) -> bool {
        self.activity.is_some()
    }

    /// Revokes a single JWT (by its `jti`) until its natural expiration timestamp.
    pub fn revoke_jwt_token(&self, raw_token: &str) {
        let Ok(claims) = self.tokens.verify_token(raw_token) else {
            return;
        };
        if claims.jti.is_empty() {
            return;
        }
        let now = jsonwebtoken::get_current_timestamp();
        let mut guard = self.revoked_jtis.write().unwrap_or_else(|e| e.into_inner());
        guard.retain(|_, exp| *exp > now);
        if guard.len() >= MAX_REVOKED_JTIS
            && let Some(oldest_key) = guard
                .iter()
                .min_by_key(|(_, exp)| **exp)
                .map(|(k, _)| k.clone())
        {
            guard.remove(&oldest_key);
        }
        guard.insert(claims.jti, claims.exp);
    }

    fn is_jti_revoked(&self, jti: &str) -> bool {
        if jti.is_empty() {
            return false;
        }
        let now = jsonwebtoken::get_current_timestamp();
        let guard = self.revoked_jtis.read().unwrap_or_else(|e| e.into_inner());
        guard.get(jti).is_some_and(|exp| *exp > now)
    }

    pub fn token_ttl_seconds(&self) -> u64 {
        self.tokens.ttl_seconds()
    }

    pub async fn setup_status(&self) -> DomainResult<SetupStatusDto> {
        let has_user = self.users.has_any_user().await?;
        Ok(SetupStatusDto {
            needs_setup: !has_user,
        })
    }

    pub async fn setup_initial_admin(&self, req: SetupAdminInput) -> DomainResult<AuthSessionDto> {
        let _guard = self.setup_lock.lock().await;
        if self.users.has_any_user().await? {
            return Err(DomainError::Conflict(
                "initial administrator has already been configured".into(),
            ));
        }
        Self::validate_password(&req.password)?;
        let hash = self.hasher.hash_password(&req.password).await?;
        let enc_secret = self.hasher.encrypt_subsonic_secret(&req.password)?;
        let api_key = self.hasher.generate_api_key();

        let user = User::new(
            req.username,
            req.display_name.unwrap_or_default(),
            hash,
            enc_secret,
            api_key,
            UserRole::Admin,
        )?;
        self.users.save(&user).await?;
        self.issue_session(user)
    }

    pub async fn login(&self, username: &str, password: &str) -> DomainResult<AuthSessionDto> {
        let Some(user) = self.users.find_by_username(username.trim()).await? else {
            self.rate_limiter
                .record_failure_and_check("login:<unknown>")?;
            return Err(DomainError::Unauthorized(
                "invalid username or password".into(),
            ));
        };

        let rate_key = format!("login:{}", user.username);
        let verified = if self.rate_limiter.is_limited(&rate_key) {
            let _throttle = self.throttled_verify_lock.lock().await;
            self.hasher
                .verify_password(password, &user.password_hash)
                .await?
        } else {
            self.hasher
                .verify_password(password, &user.password_hash)
                .await?
        };
        if !verified {
            self.rate_limiter.record_failure_and_check(&rate_key)?;
            return Err(DomainError::Unauthorized(
                "invalid username or password".into(),
            ));
        }

        self.rate_limiter.clear_below_limit(&rate_key);
        if !user.is_active {
            return Err(DomainError::Forbidden("account has been disabled".into()));
        }

        self.issue_session(user)
    }

    /// Issues a session directly for an active local administrator without password verification.
    /// Intended strictly for the in-process standalone loopback client gateway so local
    /// password changes or identity file recovery never break auto-login and repeated
    /// imports avoid redundant Argon2 hashing.
    pub async fn issue_local_admin_session(
        &self,
        preferred_username: Option<&str>,
    ) -> DomainResult<AuthSessionDto> {
        if let Some(username) = preferred_username.map(str::trim).filter(|u| !u.is_empty())
            && let Some(user) = self.users.find_by_username(username).await?
            && user.is_active
            && user.role.is_admin()
        {
            return self.issue_session(user);
        }

        let admin = self
            .users
            .list_all()
            .await?
            .into_iter()
            .find(|u| u.is_active && u.role.is_admin())
            .ok_or_else(|| {
                DomainError::NotFound("no active local administrator account found".into())
            })?;

        self.issue_session(admin)
    }

    pub async fn authenticate_jwt(&self, raw_token: &str) -> DomainResult<User> {
        let claims = self.tokens.verify_token(raw_token)?;
        if self.is_jti_revoked(&claims.jti) {
            return Err(DomainError::Unauthorized("token has been revoked".into()));
        }
        let user = self
            .users
            .find_by_id(claims.user_id)
            .await?
            .ok_or_else(|| DomainError::Unauthorized("user no longer exists".into()))?;
        if !user.is_active {
            return Err(DomainError::Forbidden("account has been disabled".into()));
        }
        let expected_ver = compute_user_cred_ver(&user);
        if claims.cred_ver.is_empty()
            || !constant_time_eq(claims.cred_ver.as_bytes(), expected_ver.as_bytes())
        {
            return Err(DomainError::Unauthorized(
                "session has been invalidated by a credential change".into(),
            ));
        }
        Ok(user)
    }

    pub async fn authenticate_api_key(&self, key: &str) -> DomainResult<User> {
        let trimmed = key.trim();
        if trimmed.is_empty() {
            self.rate_limiter
                .record_failure_and_check("api_key:<invalid>")?;
            return Err(DomainError::Unauthorized("empty api key".into()));
        }
        let Some(user) = self.users.find_by_api_key(trimmed).await? else {
            self.rate_limiter
                .record_failure_and_check("api_key:<invalid>")?;
            return Err(DomainError::Unauthorized("invalid api key".into()));
        };
        if !constant_time_eq(trimmed.as_bytes(), user.api_key.as_bytes()) {
            self.rate_limiter
                .record_failure_and_check("api_key:<invalid>")?;
            return Err(DomainError::Unauthorized("invalid api key".into()));
        }
        if !user.is_active {
            return Err(DomainError::Forbidden("account has been disabled".into()));
        }
        Ok(user)
    }

    pub async fn verify_basic_credentials(
        &self,
        username: &str,
        password: &str,
    ) -> DomainResult<User> {
        let Some(user) = self.users.find_by_username(username.trim()).await? else {
            self.rate_limiter
                .record_failure_and_check("basic:<unknown>")?;
            return Err(DomainError::Unauthorized("invalid credentials".into()));
        };
        let rate_key = format!("basic:{}", user.username);
        let ok = if !user.api_key.is_empty()
            && constant_time_eq(password.as_bytes(), user.api_key.as_bytes())
        {
            true
        } else if self.rate_limiter.is_limited(&rate_key) {
            let _throttle = self.throttled_verify_lock.lock().await;
            self.hasher
                .verify_password(password, &user.password_hash)
                .await?
        } else {
            self.hasher
                .verify_password(password, &user.password_hash)
                .await?
        };
        if !ok {
            self.rate_limiter.record_failure_and_check(&rate_key)?;
            return Err(DomainError::Unauthorized("invalid credentials".into()));
        }
        self.rate_limiter.clear_below_limit(&rate_key);
        if !user.is_active {
            return Err(DomainError::Forbidden("account has been disabled".into()));
        }
        Ok(user)
    }

    pub async fn authenticate_subsonic(&self, creds: SubsonicCredentials) -> DomainResult<User> {
        match creds {
            SubsonicCredentials::ApiKey(key) => self.authenticate_api_key(&key).await,
            SubsonicCredentials::Password { username, password } => {
                let plain = decode_subsonic_password(&password)?;
                self.verify_basic_credentials(&username, &plain).await
            }
            SubsonicCredentials::TokenSalt {
                username,
                token,
                salt,
            } => {
                let Some(user) = self.users.find_by_username(username.trim()).await? else {
                    self.rate_limiter
                        .record_failure_and_check("subsonic:<unknown>")?;
                    return Err(DomainError::Unauthorized(
                        "wrong username or password".into(),
                    ));
                };
                let rate_key = format!("subsonic:{}", user.username);
                let ok = self.hasher.verify_subsonic_token(
                    &user.subsonic_secret_enc,
                    &user.api_key,
                    &salt,
                    &token,
                )?;
                if !ok {
                    self.rate_limiter.record_failure_and_check(&rate_key)?;
                    return Err(DomainError::Unauthorized(
                        "wrong username or password".into(),
                    ));
                }
                self.rate_limiter.clear_below_limit(&rate_key);
                if !user.is_active {
                    return Err(DomainError::Forbidden("account has been disabled".into()));
                }
                Ok(user)
            }
        }
    }

    pub async fn get_me(&self, actor: &User) -> DomainResult<UserDto> {
        let fresh = self
            .users
            .find_by_id(actor.id)
            .await?
            .ok_or_else(|| DomainError::NotFound("user not found".into()))?;
        Ok(UserDto::from(fresh))
    }

    pub async fn update_preferences(
        &self,
        actor: &User,
        preferences: UserPreferences,
    ) -> DomainResult<UserDto> {
        let mut user = self
            .users
            .find_by_id(actor.id)
            .await?
            .ok_or_else(|| DomainError::NotFound("user not found".into()))?;
        user.update_preferences(preferences);
        if !self
            .users
            .update_preferences(user.id, &user.preferences, user.updated_at)
            .await?
        {
            return Err(DomainError::NotFound("user not found".into()));
        }
        Ok(UserDto::from(user))
    }

    pub async fn change_own_password(
        &self,
        actor: &User,
        input: ChangePasswordInput,
    ) -> DomainResult<AuthSessionDto> {
        let mut user = self
            .users
            .find_by_id(actor.id)
            .await?
            .ok_or_else(|| DomainError::NotFound("user not found".into()))?;
        if !self
            .hasher
            .verify_password(&input.current_password, &user.password_hash)
            .await?
        {
            return Err(DomainError::Unauthorized(
                "current password is incorrect".into(),
            ));
        }
        Self::validate_password(&input.new_password)?;
        let hash = self.hasher.hash_password(&input.new_password).await?;
        let enc_secret = self.hasher.encrypt_subsonic_secret(&input.new_password)?;
        user.update_credentials(hash, enc_secret)?;
        if !self
            .users
            .update_credentials(
                user.id,
                &user.password_hash,
                &user.subsonic_secret_enc,
                user.updated_at,
            )
            .await?
        {
            return Err(DomainError::NotFound("user not found".into()));
        }
        self.issue_session(user)
    }

    pub async fn rotate_api_key(&self, actor: &User) -> DomainResult<UserDto> {
        let mut user = self
            .users
            .find_by_id(actor.id)
            .await?
            .ok_or_else(|| DomainError::NotFound("user not found".into()))?;
        let new_key = self.hasher.generate_api_key();
        user.rotate_api_key(new_key)?;
        if !self
            .users
            .update_api_key(user.id, &user.api_key, user.updated_at)
            .await?
        {
            return Err(DomainError::NotFound("user not found".into()));
        }
        Ok(UserDto::from(user))
    }

    pub async fn list_users(&self, actor: &User) -> DomainResult<Vec<UserDto>> {
        require_admin(actor)?;
        let all = self.users.list_all().await?;
        Ok(all.into_iter().map(UserDto::from_redacted).collect())
    }

    pub async fn create_user(&self, actor: &User, input: CreateUserInput) -> DomainResult<UserDto> {
        require_admin(actor)?;
        Self::validate_password(&input.password)?;
        if self
            .users
            .find_by_username(input.username.trim())
            .await?
            .is_some()
        {
            return Err(DomainError::Conflict("username already exists".into()));
        }
        let hash = self.hasher.hash_password(&input.password).await?;
        let enc_secret = self.hasher.encrypt_subsonic_secret(&input.password)?;
        let api_key = self.hasher.generate_api_key();
        let user = User::new(
            input.username,
            input.display_name.unwrap_or_default(),
            hash,
            enc_secret,
            api_key,
            input.role,
        )?;
        self.users.save(&user).await?;
        Ok(UserDto::from_redacted(user))
    }

    pub async fn update_user(
        &self,
        actor: &User,
        target_id: UserId,
        input: UpdateUserInput,
    ) -> DomainResult<UserDto> {
        require_admin(actor)?;
        let mut target = self
            .users
            .find_by_id(target_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("target user not found".into()))?;

        if target.id == actor.id {
            if matches!(input.is_active, Some(false)) {
                return Err(DomainError::Validation(
                    "cannot deactivate your own account".into(),
                ));
            }
            if let Some(role) = input.role
                && !role.is_admin()
            {
                return Err(DomainError::Validation(
                    "cannot demote your own admin role".into(),
                ));
            }
        }

        target.update_profile(input.display_name, input.role, input.is_active)?;
        if let Some(ref new_pwd) = input.password
            && !new_pwd.trim().is_empty()
        {
            Self::validate_password(new_pwd)?;
            let hash = self.hasher.hash_password(new_pwd).await?;
            let enc = self.hasher.encrypt_subsonic_secret(new_pwd)?;
            target.update_credentials(hash, enc)?;
        }
        self.users.save(&target).await?;
        if target.id == actor.id {
            Ok(UserDto::from(target))
        } else {
            Ok(UserDto::from_redacted(target))
        }
    }

    pub async fn delete_user(&self, actor: &User, target_id: UserId) -> DomainResult<()> {
        require_admin(actor)?;
        if actor.id == target_id {
            return Err(DomainError::Validation(
                "cannot delete your own account".into(),
            ));
        }
        if self.users.find_by_id(target_id).await?.is_none() {
            return Err(DomainError::NotFound("target user not found".into()));
        }

        let handlers: Vec<Arc<dyn MediaDomainLifecycle>> = self
            .lifecycles
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .cloned()
            .collect();
        for handler in handlers {
            handler.on_user_deleted(target_id).await?;
        }
        if let Some(ref activity) = self.activity {
            activity.purge_user(target_id).await?;
        }

        self.users.delete(target_id).await
    }

    fn validate_password(password: &str) -> DomainResult<()> {
        let char_len = password.chars().count();
        if !(6..=128).contains(&char_len) {
            return Err(DomainError::Validation(
                "password length must be between 6 and 128 characters".into(),
            ));
        }
        if password.trim().is_empty() {
            return Err(DomainError::Validation(
                "password cannot consist solely of whitespace".into(),
            ));
        }
        Ok(())
    }

    fn issue_session(&self, user: User) -> DomainResult<AuthSessionDto> {
        let cred_ver = compute_user_cred_ver(&user);
        let token = self.tokens.issue_token(&AuthClaims {
            user_id: user.id,
            username: user.username.clone(),
            role: user.role,
            cred_ver,
            jti: uuid::Uuid::new_v4().to_string(),
            exp: 0,
        })?;
        Ok(AuthSessionDto {
            token,
            user: UserDto::from(user),
        })
    }
}

fn decode_subsonic_password(raw: &str) -> DomainResult<String> {
    if let Some(hex_str) = raw.strip_prefix("enc:") {
        let bytes = hex::decode(hex_str)
            .map_err(|_| DomainError::Unauthorized("invalid hex-encoded password".into()))?;
        String::from_utf8(bytes)
            .map_err(|_| DomainError::Unauthorized("invalid UTF-8 password".into()))
    } else {
        Ok(raw.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_time_eq_compares_bytes_accurately() {
        assert!(constant_time_eq(b"xmk_secret_123", b"xmk_secret_123"));
        assert!(!constant_time_eq(b"xmk_secret_123", b"xmk_secret_124"));
        assert!(!constant_time_eq(b"short", b"longer"));
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn rate_limiter_never_evicts_active_buckets_under_global_key_flood() {
        let limiter = AuthRateLimiter::new(Duration::from_secs(60), 3);

        for _ in 0..3 {
            assert!(limiter.record_failure_and_check("login:admin").is_ok());
        }
        assert!(limiter.is_limited("login:admin"));
        assert!(matches!(
            limiter.record_failure_and_check("login:admin"),
            Err(DomainError::TooManyRequests(_))
        ));

        // Flood past AUTH_RATE_LIMIT_MAX_KEYS with distinct unauthenticated keys.
        for i in 0..(AUTH_RATE_LIMIT_MAX_KEYS + 10) {
            let _ = limiter.record_failure_and_check(&format!("flood:{i}"));
        }

        // Active `login:admin` bucket must NOT be evicted or reset.
        assert!(limiter.is_limited("login:admin"));
        assert!(matches!(
            limiter.record_failure_and_check("login:admin"),
            Err(DomainError::TooManyRequests(_))
        ));
        // Overflow bucket must also reject additional untracked failures.
        assert!(matches!(
            limiter.record_failure_and_check("flood:extra"),
            Err(DomainError::TooManyRequests(_))
        ));
    }

    #[tokio::test]
    async fn valid_admin_credentials_and_api_key_are_not_locked_out_by_failed_attempts() {
        use async_trait::async_trait;
        use jiff::Timestamp;

        #[derive(Default)]
        struct MemUserRepo {
            users: std::sync::Mutex<Vec<User>>,
        }

        #[async_trait]
        impl UserRepository for MemUserRepo {
            async fn count(&self) -> DomainResult<u64> {
                Ok(self.users.lock().unwrap().len() as u64)
            }
            async fn has_any_user(&self) -> DomainResult<bool> {
                Ok(!self.users.lock().unwrap().is_empty())
            }
            async fn list_all(&self) -> DomainResult<Vec<User>> {
                Ok(self.users.lock().unwrap().clone())
            }
            async fn find_by_id(&self, id: UserId) -> DomainResult<Option<User>> {
                Ok(self
                    .users
                    .lock()
                    .unwrap()
                    .iter()
                    .find(|u| u.id == id)
                    .cloned())
            }
            async fn find_by_username(&self, username: &str) -> DomainResult<Option<User>> {
                let norm = username.trim().to_ascii_lowercase();
                Ok(self
                    .users
                    .lock()
                    .unwrap()
                    .iter()
                    .find(|u| u.username == norm)
                    .cloned())
            }
            async fn find_by_api_key(&self, api_key: &str) -> DomainResult<Option<User>> {
                Ok(self
                    .users
                    .lock()
                    .unwrap()
                    .iter()
                    .find(|u| u.api_key == api_key)
                    .cloned())
            }
            async fn save(&self, user: &User) -> DomainResult<()> {
                let mut g = self.users.lock().unwrap();
                if let Some(slot) = g.iter_mut().find(|u| u.id == user.id) {
                    *slot = user.clone();
                } else {
                    g.push(user.clone());
                }
                Ok(())
            }
            async fn update_preferences(
                &self,
                _id: UserId,
                _preferences: &UserPreferences,
                _updated_at: Timestamp,
            ) -> DomainResult<bool> {
                Ok(true)
            }
            async fn update_credentials(
                &self,
                _id: UserId,
                _password_hash: &str,
                _subsonic_secret_enc: &str,
                _updated_at: Timestamp,
            ) -> DomainResult<bool> {
                Ok(true)
            }
            async fn update_api_key(
                &self,
                _id: UserId,
                _api_key: &str,
                _updated_at: Timestamp,
            ) -> DomainResult<bool> {
                Ok(true)
            }
            async fn delete(&self, id: UserId) -> DomainResult<()> {
                self.users.lock().unwrap().retain(|u| u.id != id);
                Ok(())
            }
        }

        struct FastTestHasher;

        #[async_trait]
        impl PasswordHasherPort for FastTestHasher {
            async fn hash_password(&self, plain: &str) -> DomainResult<String> {
                Ok(format!("hash:{plain}"))
            }
            async fn verify_password(&self, plain: &str, hash: &str) -> DomainResult<bool> {
                Ok(hash == format!("hash:{plain}"))
            }
            fn encrypt_subsonic_secret(&self, plain: &str) -> DomainResult<String> {
                Ok(format!("enc:{plain}"))
            }
            fn verify_subsonic_token(
                &self,
                encrypted_secret: &str,
                _fallback_api_key: &str,
                salt: &str,
                expected_token_hex: &str,
            ) -> DomainResult<bool> {
                Ok(expected_token_hex == format!("{encrypted_secret}:{salt}"))
            }
            fn generate_api_key(&self) -> String {
                "xmk_valid_admin_key_123456".into()
            }
        }

        struct DummyTokenSvc;

        impl TokenServicePort for DummyTokenSvc {
            fn issue_token(&self, claims: &AuthClaims) -> DomainResult<String> {
                Ok(format!("tok:{}:{}", claims.user_id, claims.jti))
            }
            fn verify_token(&self, _token: &str) -> DomainResult<AuthClaims> {
                Err(DomainError::Unauthorized("unused".into()))
            }
            fn ttl_seconds(&self) -> u64 {
                3600
            }
        }

        let auth = AuthService::new(
            Arc::new(MemUserRepo::default()),
            Arc::new(FastTestHasher),
            Arc::new(DummyTokenSvc),
            Arc::new(RwLock::new(HashMap::new())),
        );
        let setup = auth
            .setup_initial_admin(SetupAdminInput {
                username: "admin".into(),
                display_name: Some("Admin".into()),
                password: "admin-good-pass".into(),
            })
            .await
            .unwrap();

        // 1. Brute-force wrong passwords against "admin" triggers TooManyRequests on 21st failure
        for i in 0..AUTH_RATE_LIMIT_MAX_FAILURES {
            assert!(matches!(
                auth.login("admin", &format!("wrong-{i}")).await,
                Err(DomainError::Unauthorized(_))
            ));
        }
        assert!(matches!(
            auth.login("admin", "wrong-overflow").await,
            Err(DomainError::TooManyRequests(_))
        ));

        // 2. Real "admin" presenting valid credentials succeeds without lockout, while keeping
        // the attacker's active rate-limit window enforced on subsequent wrong guesses!
        let valid_sess = auth.login("admin", "admin-good-pass").await.unwrap();
        assert_eq!(valid_sess.user.username, "admin");
        assert!(matches!(
            auth.login("admin", "still-wrong-after-valid").await,
            Err(DomainError::TooManyRequests(_))
        ));

        // 3. Invalid API keys are rate-limited, while the valid API key continues to work
        for i in 0..AUTH_RATE_LIMIT_MAX_FAILURES {
            assert!(matches!(
                auth.authenticate_api_key(&format!("xmk_bad_{i}")).await,
                Err(DomainError::Unauthorized(_))
            ));
        }
        assert!(matches!(
            auth.authenticate_api_key("xmk_bad_overflow").await,
            Err(DomainError::TooManyRequests(_))
        ));
        let api_user = auth
            .authenticate_api_key(&setup.user.api_key)
            .await
            .unwrap();
        assert_eq!(api_user.username, "admin");

        // 4. Deleting a user via AuthService purges that user's activity records consistently
        use crate::application::activity::{ActivityHeatmapQuery, RecordActivityInput};
        use crate::domain::activity::{UserActivityDaily, UserActivityRepository};

        #[derive(Default)]
        struct MemActivityRepo {
            rows: tokio::sync::Mutex<Vec<UserActivityDaily>>,
        }

        #[async_trait]
        impl UserActivityRepository for MemActivityRepo {
            async fn list_by_user_and_domain(
                &self,
                user_id: UserId,
                media_kind: MediaKind,
            ) -> DomainResult<Vec<UserActivityDaily>> {
                let g = self.rows.lock().await;
                Ok(g.iter()
                    .filter(|r| r.user_id == user_id && r.media_kind == media_kind)
                    .cloned()
                    .collect())
            }
            async fn find_by_key(
                &self,
                user_id: UserId,
                media_kind: MediaKind,
                activity_date: &str,
            ) -> DomainResult<Option<UserActivityDaily>> {
                let g = self.rows.lock().await;
                Ok(g.iter()
                    .find(|r| {
                        r.user_id == user_id
                            && r.media_kind == media_kind
                            && r.activity_date == activity_date
                    })
                    .cloned())
            }
            async fn save(&self, record: &UserActivityDaily) -> DomainResult<()> {
                let mut g = self.rows.lock().await;
                if let Some(slot) = g.iter_mut().find(|r| r.id == record.id) {
                    *slot = record.clone();
                } else {
                    g.push(record.clone());
                }
                Ok(())
            }
            async fn delete_by_user(&self, user_id: UserId) -> DomainResult<()> {
                self.rows.lock().await.retain(|r| r.user_id != user_id);
                Ok(())
            }
        }

        let activity = Arc::new(ActivityService::new(Arc::new(MemActivityRepo::default())));
        let auth_with_act = auth.with_activity(activity.clone());
        let member = auth_with_act
            .create_user(
                &api_user,
                CreateUserInput {
                    username: "member_to_delete".into(),
                    display_name: None,
                    password: "member-pass-123".into(),
                    role: UserRole::Member,
                },
            )
            .await
            .unwrap();
        let today = Timestamp::now()
            .to_zoned(jiff::tz::TimeZone::UTC)
            .date()
            .to_string();
        activity
            .record_heartbeat(
                member.id,
                RecordActivityInput {
                    media_kind: MediaKind::Book,
                    date: today.clone(),
                    delta_secs: 90,
                    new_session: true,
                    item_title: Some("Sample".into()),
                },
            )
            .await
            .unwrap();
        activity
            .record_heartbeat(
                api_user.id,
                RecordActivityInput {
                    media_kind: MediaKind::Book,
                    date: today.clone(),
                    delta_secs: 120,
                    new_session: true,
                    item_title: Some("AdminBook".into()),
                },
            )
            .await
            .unwrap();

        auth_with_act
            .delete_user(&api_user, member.id)
            .await
            .unwrap();

        let deleted_hm = activity
            .get_heatmap(
                member.id,
                ActivityHeatmapQuery {
                    media_kind: MediaKind::Book,
                    year: None,
                    today: Some(today.clone()),
                },
            )
            .await
            .unwrap();
        assert_eq!(deleted_hm.summary.total_secs, 0);

        let admin_hm = activity
            .get_heatmap(
                api_user.id,
                ActivityHeatmapQuery {
                    media_kind: MediaKind::Book,
                    year: None,
                    today: Some(today),
                },
            )
            .await
            .unwrap();
        assert_eq!(admin_hm.summary.total_secs, 120);
    }
}

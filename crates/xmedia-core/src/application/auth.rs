mod rate_limit;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, RwLock};

use tokio::sync::Mutex;

use crate::application::activity::ActivityService;
use crate::application::bindings::MediaBindingService;
use crate::application::dto::{
    AuthSessionDto, ChangePasswordInput, CreateUserInput, SetupAdminInput, SetupStatusDto,
    SubsonicCredentials, UpdateUserInput, UserDto,
};
use crate::application::ports::{
    AuthClaims, MediaDomainLifecycle, PasswordHasherPort, TokenServicePort,
};
use crate::domain::error::{DomainError, DomainResult};
use crate::domain::library::MediaKind;
use crate::domain::repository::{OnlineLibraryItemRepository, UserRepository};
use crate::domain::user::{User, UserId, UserPreferences, UserRole};

pub use rate_limit::constant_time_eq;
use rate_limit::{
    AUTH_RATE_LIMIT_MAX_FAILURES, AUTH_RATE_LIMIT_WINDOW, AuthRateLimiter, MAX_REVOKED_JTIS,
    compute_user_cred_ver,
};

#[derive(Clone)]
pub struct AuthService {
    users: Arc<dyn UserRepository>,
    hasher: Arc<dyn PasswordHasherPort>,
    tokens: Arc<dyn TokenServicePort>,
    lifecycles: Arc<RwLock<HashMap<MediaKind, Arc<dyn MediaDomainLifecycle>>>>,
    activity: Option<Arc<ActivityService>>,
    online_items: Option<Arc<dyn OnlineLibraryItemRepository>>,
    bindings: Option<Arc<MediaBindingService>>,
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
            online_items: None,
            bindings: None,
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

    pub fn with_online_items(mut self, online_items: Arc<dyn OnlineLibraryItemRepository>) -> Self {
        self.online_items = Some(online_items);
        self
    }

    pub fn with_bindings(mut self, bindings: Arc<MediaBindingService>) -> Self {
        self.bindings = Some(bindings);
        self
    }

    pub fn has_activity_cleanup(&self) -> bool {
        self.activity.is_some()
    }

    pub fn has_online_items_cleanup(&self) -> bool {
        self.online_items.is_some()
    }

    pub fn has_bindings_cleanup(&self) -> bool {
        self.bindings.is_some()
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
        self.login_with_ip(username, password, None).await
    }

    /// Authenticates `username`/`password` while scoping brute-force rate limiting to
    /// `(client_ip, username)` so an attacker on IP A cannot lock out legitimate logins from IP B.
    pub async fn login_with_ip(
        &self,
        username: &str,
        password: &str,
        client_ip: Option<IpAddr>,
    ) -> DomainResult<AuthSessionDto> {
        let Some(user) = self.users.find_by_username(username.trim()).await? else {
            self.rate_limiter
                .record_failure_and_check(client_ip, "login:<unknown>")?;
            return Err(DomainError::Unauthorized(
                "invalid username or password".into(),
            ));
        };

        let rate_key = format!("login:{}", user.username);
        let verified = if self.rate_limiter.is_limited(client_ip, &rate_key) {
            if client_ip.is_some() {
                return Err(DomainError::TooManyRequests(
                    "too many failed authentication attempts; please try again later".into(),
                ));
            }
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
            self.rate_limiter
                .record_failure_and_check(client_ip, &rate_key)?;
            return Err(DomainError::Unauthorized(
                "invalid username or password".into(),
            ));
        }

        self.rate_limiter.clear_below_limit(client_ip, &rate_key);
        user.ensure_active()?;
        self.issue_session(user)
    }

    /// Issues a session directly for an active local administrator without password verification.
    /// Intended strictly for the in-process standalone loopback client gateway.
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
        user.ensure_active()?;
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
        self.authenticate_api_key_with_ip(key, None).await
    }

    pub async fn authenticate_api_key_with_ip(
        &self,
        key: &str,
        client_ip: Option<IpAddr>,
    ) -> DomainResult<User> {
        let trimmed = key.trim();
        if trimmed.is_empty() {
            self.rate_limiter
                .record_failure_and_check(client_ip, "api_key:<invalid>")?;
            return Err(DomainError::Unauthorized("empty api key".into()));
        }
        let Some(user) = self.users.find_by_api_key(trimmed).await? else {
            self.rate_limiter
                .record_failure_and_check(client_ip, "api_key:<invalid>")?;
            return Err(DomainError::Unauthorized("invalid api key".into()));
        };
        if !constant_time_eq(trimmed.as_bytes(), user.api_key.as_bytes()) {
            self.rate_limiter
                .record_failure_and_check(client_ip, "api_key:<invalid>")?;
            return Err(DomainError::Unauthorized("invalid api key".into()));
        }
        user.ensure_active()?;
        Ok(user)
    }

    pub async fn verify_basic_credentials(
        &self,
        username: &str,
        password: &str,
    ) -> DomainResult<User> {
        self.verify_basic_credentials_with_ip(username, password, None)
            .await
    }

    pub async fn verify_basic_credentials_with_ip(
        &self,
        username: &str,
        password: &str,
        client_ip: Option<IpAddr>,
    ) -> DomainResult<User> {
        let Some(user) = self.users.find_by_username(username.trim()).await? else {
            self.rate_limiter
                .record_failure_and_check(client_ip, "basic:<unknown>")?;
            return Err(DomainError::Unauthorized("invalid credentials".into()));
        };
        let rate_key = format!("basic:{}", user.username);
        let ok = if !user.api_key.is_empty()
            && constant_time_eq(password.as_bytes(), user.api_key.as_bytes())
        {
            true
        } else if self.rate_limiter.is_limited(client_ip, &rate_key) {
            if client_ip.is_some() {
                return Err(DomainError::TooManyRequests(
                    "too many failed authentication attempts; please try again later".into(),
                ));
            }
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
            self.rate_limiter
                .record_failure_and_check(client_ip, &rate_key)?;
            return Err(DomainError::Unauthorized("invalid credentials".into()));
        }
        self.rate_limiter.clear_below_limit(client_ip, &rate_key);
        user.ensure_active()?;
        Ok(user)
    }

    pub async fn authenticate_subsonic(&self, creds: SubsonicCredentials) -> DomainResult<User> {
        self.authenticate_subsonic_with_ip(creds, None).await
    }

    pub async fn authenticate_subsonic_with_ip(
        &self,
        creds: SubsonicCredentials,
        client_ip: Option<IpAddr>,
    ) -> DomainResult<User> {
        match creds {
            SubsonicCredentials::ApiKey(key) => {
                self.authenticate_api_key_with_ip(&key, client_ip).await
            }
            SubsonicCredentials::Password { username, password } => {
                let plain = decode_subsonic_password(&password)?;
                self.verify_basic_credentials_with_ip(&username, &plain, client_ip)
                    .await
            }
            SubsonicCredentials::TokenSalt {
                username,
                token,
                salt,
            } => {
                let Some(user) = self.users.find_by_username(username.trim()).await? else {
                    self.rate_limiter
                        .record_failure_and_check(client_ip, "subsonic:<unknown>")?;
                    return Err(DomainError::Unauthorized(
                        "wrong username or password".into(),
                    ));
                };
                let rate_key = format!("subsonic:{}", user.username);
                if client_ip.is_some() && self.rate_limiter.is_limited(client_ip, &rate_key) {
                    return Err(DomainError::TooManyRequests(
                        "too many failed authentication attempts; please try again later".into(),
                    ));
                }
                let ok = self.hasher.verify_subsonic_token(
                    &user.subsonic_secret_enc,
                    &user.api_key,
                    &salt,
                    &token,
                )?;
                if !ok {
                    self.rate_limiter
                        .record_failure_and_check(client_ip, &rate_key)?;
                    return Err(DomainError::Unauthorized(
                        "wrong username or password".into(),
                    ));
                }
                self.rate_limiter.clear_below_limit(client_ip, &rate_key);
                user.ensure_active()?;
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
        actor.ensure_admin()?;
        let all = self.users.list_all().await?;
        Ok(all.into_iter().map(UserDto::from_redacted).collect())
    }

    pub async fn create_user(&self, actor: &User, input: CreateUserInput) -> DomainResult<UserDto> {
        actor.ensure_admin()?;
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
        actor.ensure_can_modify_user(target_id, input.role, input.is_active)?;
        let mut target = self
            .users
            .find_by_id(target_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("target user not found".into()))?;

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
        actor.ensure_can_delete_user(target_id)?;
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
        if let Some(ref online_items) = self.online_items {
            online_items.delete_by_user(target_id).await?;
        }
        if let Some(ref bindings) = self.bindings {
            bindings.purge_user(target_id).await?;
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

mod rate_limit;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, RwLock};

use tokio::sync::{Mutex, OnceCell};

use crate::application::activity::ActivityService;
use crate::application::bindings::MediaBindingService;
use crate::application::dto::{
    AuthCredential, AuthSessionDto, ChangePasswordInput, CreateUserInput, SetupAdminInput,
    SetupStatusDto, SubsonicCredentials, UpdateUserInput, UserDto,
};
use crate::application::ports::{
    AuthClaims, MediaDomainLifecycle, PasswordHasherPort, TokenServicePort,
};
use crate::domain::error::{DomainError, DomainResult};
use crate::domain::library::MediaKind;
use crate::domain::repository::{
    BindingDeleteScope, OnlineItemDeleteScope, OnlineLibraryItemRepository, UserLookup,
    UserMutation, UserRepository,
};
use crate::domain::user::{User, UserId, UserPreferences, UserRole, UserSecret, validate_password};

pub use rate_limit::constant_time_eq;
use rate_limit::{
    AUTH_RATE_LIMIT_MAX_FAILURES, AUTH_RATE_LIMIT_WINDOW, AuthRateLimiter, MAX_REVOKED_JTIS,
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
    dummy_password_hash: Arc<OnceCell<String>>,
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
            dummy_password_hash: Arc::new(OnceCell::new()),
            revoked_jtis: Arc::new(RwLock::new(HashMap::new())),
            rate_limiter: Arc::new(AuthRateLimiter::new(
                AUTH_RATE_LIMIT_WINDOW,
                AUTH_RATE_LIMIT_MAX_FAILURES,
            )),
        }
    }

    pub fn with_cleanup(
        mut self,
        activity: Option<Arc<ActivityService>>,
        online_items: Option<Arc<dyn OnlineLibraryItemRepository>>,
        bindings: Option<Arc<MediaBindingService>>,
    ) -> Self {
        self.activity = activity;
        self.online_items = online_items;
        self.bindings = bindings;
        self
    }

    async fn verify_against_dummy_hash(&self, password: &str) {
        if let Ok(dummy) = self
            .dummy_password_hash
            .get_or_try_init(|| {
                self.hasher
                    .hash_password("xmedia-dummy-password-mitigation")
            })
            .await
        {
            let _ = self.hasher.verify_password(password, dummy).await;
        }
    }

    async fn verify_password_rate_limited(
        &self,
        password: &str,
        password_hash: &str,
        client_ip: Option<IpAddr>,
        rate_key: &str,
    ) -> DomainResult<bool> {
        if self.rate_limiter.is_limited(client_ip, rate_key) {
            if client_ip.is_some() {
                return Err(DomainError::TooManyRequests(
                    "too many failed authentication attempts; please try again later".into(),
                ));
            }
            let _throttle = self.throttled_verify_lock.lock().await;
            self.hasher.verify_password(password, password_hash).await
        } else {
            self.hasher.verify_password(password, password_hash).await
        }
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
        Ok(SetupStatusDto {
            needs_setup: self.users.count().await? == 0,
        })
    }

    pub async fn setup_initial_admin(&self, req: SetupAdminInput) -> DomainResult<AuthSessionDto> {
        let _guard = self.setup_lock.lock().await;
        if self.users.count().await? > 0 {
            return Err(DomainError::Conflict(
                "initial administrator has already been configured".into(),
            ));
        }
        let (hash, enc_secret) = self.hash_new_password(&req.password).await?;
        let user = User::new(
            req.username,
            req.display_name.unwrap_or_default(),
            hash,
            enc_secret,
            self.hasher.generate_api_key(),
            UserRole::Admin,
        )?;
        self.users.save(&user).await?;
        self.issue_session(user)
    }

    pub async fn login(
        &self,
        username: &str,
        password: &str,
        client_ip: Option<IpAddr>,
    ) -> DomainResult<AuthSessionDto> {
        let trimmed = username.trim();
        let Some(user) = self.users.find(UserLookup::Username(trimmed)).await? else {
            self.verify_against_dummy_hash(password).await;
            self.rate_limiter
                .record_failure_and_check(client_ip, "login:<unknown>")?;
            return Err(DomainError::Unauthorized(
                "invalid username or password".into(),
            ));
        };
        let rate_key = format!("login:{}", user.username);
        let verified = self
            .verify_password_rate_limited(password, &user.password_hash, client_ip, &rate_key)
            .await?;
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

    pub async fn issue_local_admin_session(
        &self,
        preferred_username: Option<&str>,
    ) -> DomainResult<AuthSessionDto> {
        if let Some(username) = preferred_username.map(str::trim).filter(|u| !u.is_empty())
            && let Some(user) = self.users.find(UserLookup::Username(username)).await?
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

    pub async fn authenticate(
        &self,
        cred: AuthCredential<'_>,
        client_ip: Option<IpAddr>,
    ) -> DomainResult<User> {
        match cred {
            AuthCredential::Jwt(token) => self.authenticate_jwt(token).await,
            AuthCredential::ApiKey(key) => self.verify_api_key(key, client_ip).await,
            AuthCredential::Basic { username, password } => {
                self.verify_basic(username, password, client_ip).await
            }
            AuthCredential::Subsonic(creds) => self.verify_subsonic(creds, client_ip).await,
        }
    }

    async fn authenticate_jwt(&self, raw_token: &str) -> DomainResult<User> {
        let claims = self.tokens.verify_token(raw_token)?;
        if self.is_jti_revoked(&claims.jti) {
            return Err(DomainError::Unauthorized("token has been revoked".into()));
        }
        let user = self
            .users
            .find(UserLookup::Id(claims.user_id))
            .await?
            .ok_or_else(|| DomainError::Unauthorized("user no longer exists".into()))?;
        user.ensure_active()?;
        let expected_ver = user.cred_version();
        if claims.cred_ver.is_empty()
            || !constant_time_eq(claims.cred_ver.as_bytes(), expected_ver.as_bytes())
        {
            return Err(DomainError::Unauthorized(
                "session has been invalidated by a credential change".into(),
            ));
        }
        Ok(user)
    }

    async fn verify_api_key(&self, key: &str, client_ip: Option<IpAddr>) -> DomainResult<User> {
        let trimmed = key.trim();
        if trimmed.is_empty() {
            self.rate_limiter
                .record_failure_and_check(client_ip, "api_key:<invalid>")?;
            return Err(DomainError::Unauthorized("empty api key".into()));
        }
        let Some(user) = self.users.find(UserLookup::ApiKey(trimmed)).await? else {
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

    async fn verify_basic(
        &self,
        username: &str,
        password: &str,
        client_ip: Option<IpAddr>,
    ) -> DomainResult<User> {
        let Some(user) = self
            .users
            .find(UserLookup::Username(username.trim()))
            .await?
        else {
            self.verify_against_dummy_hash(password).await;
            self.rate_limiter
                .record_failure_and_check(client_ip, "basic:<unknown>")?;
            return Err(DomainError::Unauthorized("invalid credentials".into()));
        };
        let rate_key = format!("basic:{}", user.username);
        let ok = if !user.api_key.is_empty()
            && constant_time_eq(password.as_bytes(), user.api_key.as_bytes())
        {
            true
        } else {
            self.verify_password_rate_limited(password, &user.password_hash, client_ip, &rate_key)
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

    async fn verify_subsonic(
        &self,
        creds: SubsonicCredentials,
        client_ip: Option<IpAddr>,
    ) -> DomainResult<User> {
        match creds {
            SubsonicCredentials::ApiKey(key) => self.verify_api_key(&key, client_ip).await,
            SubsonicCredentials::Password { username, password } => {
                let plain = decode_subsonic_password(&password)?;
                self.verify_basic(&username, &plain, client_ip).await
            }
            SubsonicCredentials::TokenSalt {
                username,
                token,
                salt,
            } => {
                let Some(user) = self
                    .users
                    .find(UserLookup::Username(username.trim()))
                    .await?
                else {
                    self.rate_limiter
                        .record_failure_and_check(client_ip, "subsonic:<unknown>")?;
                    return Err(DomainError::Unauthorized(
                        "wrong username or password".into(),
                    ));
                };
                let rate_key = format!("subsonic:{}", user.username);
                if self.rate_limiter.is_limited(client_ip, &rate_key) {
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

    async fn load_user(&self, id: UserId, not_found_msg: &'static str) -> DomainResult<User> {
        self.users
            .find(UserLookup::Id(id))
            .await?
            .ok_or_else(|| DomainError::NotFound(not_found_msg.into()))
    }

    async fn hash_new_password(&self, password: &str) -> DomainResult<(String, String)> {
        validate_password(password)?;
        let hash = self.hasher.hash_password(password).await?;
        let enc = self.hasher.encrypt_subsonic_secret(password)?;
        Ok((hash, enc))
    }

    pub async fn update_preferences(
        &self,
        actor: &User,
        preferences: UserPreferences,
    ) -> DomainResult<UserDto> {
        let mut user = self.load_user(actor.id, "user not found").await?;
        user.set_preferences(preferences);
        if !self
            .users
            .update(
                user.id,
                UserMutation::Preferences(&user.preferences),
                user.updated_at,
            )
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
        let mut user = self.load_user(actor.id, "user not found").await?;
        if !self
            .hasher
            .verify_password(&input.current_password, &user.password_hash)
            .await?
        {
            return Err(DomainError::Unauthorized(
                "current password is incorrect".into(),
            ));
        }
        let (hash, enc_secret) = self.hash_new_password(&input.new_password).await?;
        user.rotate_secret(UserSecret::Password {
            password_hash: hash,
            subsonic_secret_enc: enc_secret,
        })?;
        if !self
            .users
            .update(
                user.id,
                UserMutation::Credentials {
                    password_hash: &user.password_hash,
                    subsonic_secret_enc: &user.subsonic_secret_enc,
                },
                user.updated_at,
            )
            .await?
        {
            return Err(DomainError::NotFound("user not found".into()));
        }
        self.issue_session(user)
    }

    pub async fn rotate_api_key(&self, actor: &User) -> DomainResult<UserDto> {
        let mut user = self.load_user(actor.id, "user not found").await?;
        user.rotate_secret(UserSecret::ApiKey(self.hasher.generate_api_key()))?;
        if !self
            .users
            .update(
                user.id,
                UserMutation::ApiKey(&user.api_key),
                user.updated_at,
            )
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
        let (hash, enc_secret) = self.hash_new_password(&input.password).await?;
        if self
            .users
            .find(UserLookup::Username(input.username.trim()))
            .await?
            .is_some()
        {
            return Err(DomainError::Conflict("username already exists".into()));
        }
        let user = User::new(
            input.username,
            input.display_name.unwrap_or_default(),
            hash,
            enc_secret,
            self.hasher.generate_api_key(),
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
        guard_self_admin_modification(actor, target_id, input.role, input.is_active)?;
        let mut target = self.load_user(target_id, "target user not found").await?;
        target.update_profile(input.display_name, input.role, input.is_active)?;
        if let Some(ref new_pwd) = input.password
            && !new_pwd.trim().is_empty()
        {
            let (hash, enc) = self.hash_new_password(new_pwd).await?;
            target.rotate_secret(UserSecret::Password {
                password_hash: hash,
                subsonic_secret_enc: enc,
            })?;
        }
        self.users.save(&target).await?;
        if target.id == actor.id {
            Ok(UserDto::from(target))
        } else {
            Ok(UserDto::from_redacted(target))
        }
    }

    pub async fn delete_user(&self, actor: &User, target_id: UserId) -> DomainResult<()> {
        guard_self_admin_deletion(actor, target_id)?;
        let _ = self.load_user(target_id, "target user not found").await?;
        self.purge_deleted_user_data(target_id).await?;
        self.users.delete(target_id).await
    }

    async fn purge_deleted_user_data(&self, target_id: UserId) -> DomainResult<()> {
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
            online_items
                .delete_scope(OnlineItemDeleteScope::User(target_id))
                .await?;
        }
        if let Some(ref bindings) = self.bindings {
            let _ = bindings.purge(BindingDeleteScope::User(target_id)).await?;
        }
        if let Some(ref activity) = self.activity {
            activity.purge_user(target_id).await?;
        }
        Ok(())
    }

    fn issue_session(&self, user: User) -> DomainResult<AuthSessionDto> {
        let cred_ver = user.cred_version();
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

fn guard_self_admin_modification(
    actor: &User,
    target_id: UserId,
    role: Option<UserRole>,
    is_active: Option<bool>,
) -> DomainResult<()> {
    actor.ensure_admin()?;
    if actor.id == target_id {
        if matches!(is_active, Some(false)) {
            return Err(DomainError::Validation(
                "cannot deactivate your own account".into(),
            ));
        }
        if let Some(r) = role
            && !r.is_admin()
        {
            return Err(DomainError::Validation(
                "cannot demote your own admin role".into(),
            ));
        }
    }
    Ok(())
}

fn guard_self_admin_deletion(actor: &User, target_id: UserId) -> DomainResult<()> {
    actor.ensure_admin()?;
    if actor.id == target_id {
        return Err(DomainError::Validation(
            "cannot delete your own account".into(),
        ));
    }
    Ok(())
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

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use tokio::sync::Mutex;

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

#[derive(Clone)]
pub struct AuthService {
    users: Arc<dyn UserRepository>,
    hasher: Arc<dyn PasswordHasherPort>,
    tokens: Arc<dyn TokenServicePort>,
    lifecycles: Arc<RwLock<HashMap<MediaKind, Arc<dyn MediaDomainLifecycle>>>>,
    setup_lock: Arc<Mutex<()>>,
    revoked_jtis: Arc<RwLock<HashMap<String, u64>>>,
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
            setup_lock: Arc::new(Mutex::new(())),
            revoked_jtis: Arc::new(RwLock::new(HashMap::new())),
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
        if let Ok(mut guard) = self.revoked_jtis.write() {
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
    }

    fn is_jti_revoked(&self, jti: &str) -> bool {
        if jti.is_empty() {
            return false;
        }
        let now = jsonwebtoken::get_current_timestamp();
        if let Ok(guard) = self.revoked_jtis.read() {
            guard.get(jti).is_some_and(|exp| *exp > now)
        } else {
            false
        }
    }

    pub fn token_ttl_seconds(&self) -> u64 {
        self.tokens.ttl_seconds()
    }

    pub async fn setup_status(&self) -> DomainResult<SetupStatusDto> {
        let count = self.users.count().await?;
        Ok(SetupStatusDto {
            needs_setup: count == 0,
        })
    }

    pub async fn setup_initial_admin(&self, req: SetupAdminInput) -> DomainResult<AuthSessionDto> {
        let _guard = self.setup_lock.lock().await;
        if self.users.count().await? > 0 {
            return Err(DomainError::Conflict(
                "initial administrator has already been configured".into(),
            ));
        }
        Self::validate_password(&req.password)?;
        let hash = self.hasher.hash_password(&req.password)?;
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
        let user = self
            .users
            .find_by_username(username.trim())
            .await?
            .ok_or_else(|| DomainError::Unauthorized("invalid username or password".into()))?;

        if !self.hasher.verify_password(password, &user.password_hash)? {
            return Err(DomainError::Unauthorized(
                "invalid username or password".into(),
            ));
        }

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
            return Err(DomainError::Unauthorized("empty api key".into()));
        }
        let user = self
            .users
            .find_by_api_key(trimmed)
            .await?
            .ok_or_else(|| DomainError::Unauthorized("invalid api key".into()))?;
        if !constant_time_eq(trimmed.as_bytes(), user.api_key.as_bytes()) {
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
        let user = self
            .users
            .find_by_username(username.trim())
            .await?
            .ok_or_else(|| DomainError::Unauthorized("invalid credentials".into()))?;
        let ok = constant_time_eq(password.as_bytes(), user.api_key.as_bytes())
            || self.hasher.verify_password(password, &user.password_hash)?;
        if !ok {
            return Err(DomainError::Unauthorized("invalid credentials".into()));
        }
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
                let user = self
                    .users
                    .find_by_username(username.trim())
                    .await?
                    .ok_or_else(|| {
                        DomainError::Unauthorized("wrong username or password".into())
                    })?;
                let ok = self.hasher.verify_subsonic_token(
                    &user.subsonic_secret_enc,
                    &user.api_key,
                    &salt,
                    &token,
                )?;
                if !ok {
                    return Err(DomainError::Unauthorized(
                        "wrong username or password".into(),
                    ));
                }
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
        self.users.save(&user).await?;
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
            .verify_password(&input.current_password, &user.password_hash)?
        {
            return Err(DomainError::Unauthorized(
                "current password is incorrect".into(),
            ));
        }
        Self::validate_password(&input.new_password)?;
        let hash = self.hasher.hash_password(&input.new_password)?;
        let enc_secret = self.hasher.encrypt_subsonic_secret(&input.new_password)?;
        user.update_credentials(hash, enc_secret)?;
        self.users.save(&user).await?;
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
        self.users.save(&user).await?;
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
        let hash = self.hasher.hash_password(&input.password)?;
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
            let hash = self.hasher.hash_password(new_pwd)?;
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
            .expect("lifecycle registry lock poisoned")
            .values()
            .cloned()
            .collect();
        for handler in handlers {
            handler.on_user_deleted(target_id).await?;
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
}

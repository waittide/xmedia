use std::collections::HashMap;
use std::sync::{Arc, RwLock};

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
use crate::domain::user::{User, UserId, UserPreferences, UserRole};

#[derive(Clone)]
pub struct AuthService {
    users: Arc<dyn UserRepository>,
    hasher: Arc<dyn PasswordHasherPort>,
    tokens: Arc<dyn TokenServicePort>,
    lifecycles: Arc<RwLock<HashMap<MediaKind, Arc<dyn MediaDomainLifecycle>>>>,
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
        }
    }

    pub async fn setup_status(&self) -> DomainResult<SetupStatusDto> {
        let count = self.users.count().await?;
        Ok(SetupStatusDto {
            needs_setup: count == 0,
        })
    }

    pub async fn setup_initial_admin(&self, req: SetupAdminInput) -> DomainResult<AuthSessionDto> {
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

        if !user.is_active {
            return Err(DomainError::Forbidden("account has been disabled".into()));
        }

        if !self.hasher.verify_password(password, &user.password_hash)? {
            return Err(DomainError::Unauthorized(
                "invalid username or password".into(),
            ));
        }

        self.issue_session(user)
    }

    pub async fn authenticate_jwt(&self, raw_token: &str) -> DomainResult<User> {
        let claims = self.tokens.verify_token(raw_token)?;
        let user = self
            .users
            .find_by_id(claims.user_id)
            .await?
            .ok_or_else(|| DomainError::Unauthorized("user no longer exists".into()))?;
        if !user.is_active {
            return Err(DomainError::Forbidden("account has been disabled".into()));
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
        if !user.is_active {
            return Err(DomainError::Forbidden("account has been disabled".into()));
        }
        let ok = password == user.api_key
            || self.hasher.verify_password(password, &user.password_hash)?;
        if !ok {
            return Err(DomainError::Unauthorized("invalid credentials".into()));
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
                if !user.is_active {
                    return Err(DomainError::Forbidden("account has been disabled".into()));
                }
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
    ) -> DomainResult<UserDto> {
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
        Ok(UserDto::from(user))
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
        Self::require_admin(actor)?;
        let all = self.users.list_all().await?;
        Ok(all.into_iter().map(UserDto::from).collect())
    }

    pub async fn create_user(&self, actor: &User, input: CreateUserInput) -> DomainResult<UserDto> {
        Self::require_admin(actor)?;
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
        Ok(UserDto::from(user))
    }

    pub async fn update_user(
        &self,
        actor: &User,
        target_id: UserId,
        input: UpdateUserInput,
    ) -> DomainResult<UserDto> {
        Self::require_admin(actor)?;
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
        Ok(UserDto::from(target))
    }

    pub async fn delete_user(&self, actor: &User, target_id: UserId) -> DomainResult<()> {
        Self::require_admin(actor)?;
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

    fn require_admin(actor: &User) -> DomainResult<()> {
        if !actor.role.is_admin() {
            return Err(DomainError::Forbidden(
                "administrator privileges required".into(),
            ));
        }
        Ok(())
    }

    fn validate_password(password: &str) -> DomainResult<()> {
        if password.len() < 6 || password.len() > 128 {
            return Err(DomainError::Validation(
                "password length must be between 6 and 128 characters".into(),
            ));
        }
        Ok(())
    }

    fn issue_session(&self, user: User) -> DomainResult<AuthSessionDto> {
        let token = self.tokens.issue_token(&AuthClaims {
            user_id: user.id,
            username: user.username.clone(),
            role: user.role,
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

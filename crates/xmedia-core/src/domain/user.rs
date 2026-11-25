use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::domain::error::{DomainError, DomainResult};
pub use crate::domain::preferences::{
    AudioTranscodeProfile, BookReaderPreferences, ComicFitMode, ComicReaderMode,
    ComicReaderPreferences, Locale, MusicPlayMode, MusicPlaybackPreferences,
    MusicPlayerPreferences, ReadingFlow, ThemeMode, UiPreferences, UserPreferences,
    VideoPlaybackPreferences, VideoPlayerPreferences, VideoQualityProfile,
};

crate::define_uuid_id!(UserId);

/// Verifies that the acting user holds administrator privileges.
pub fn require_admin(actor: &User) -> DomainResult<()> {
    actor.ensure_admin()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum UserRole {
    Admin,
    #[default]
    Member,
}

impl UserRole {
    pub fn is_admin(self) -> bool {
        matches!(self, Self::Admin)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::Member => "member",
        }
    }

    pub fn parse(value: &str) -> DomainResult<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "admin" => Ok(Self::Admin),
            "member" | "reader" | "listener" | "user" => Ok(Self::Member),
            other => Err(DomainError::Validation(format!(
                "unknown user role: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct User {
    pub id: UserId,
    pub username: String,
    pub display_name: String,
    pub password_hash: String,
    pub subsonic_secret_enc: String,
    pub api_key: String,
    pub role: UserRole,
    pub is_active: bool,
    pub preferences: UserPreferences,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl User {
    pub fn new(
        username: impl Into<String>,
        display_name: impl Into<String>,
        password_hash: impl Into<String>,
        subsonic_secret_enc: impl Into<String>,
        api_key: impl Into<String>,
        role: UserRole,
    ) -> DomainResult<Self> {
        let username = Self::validate_username(username.into())?;
        let display_name = display_name.into().trim().to_string();
        if display_name.chars().count() > 128 {
            return Err(DomainError::Validation(
                "display name cannot exceed 128 characters".into(),
            ));
        }
        let display_name = if display_name.is_empty() {
            username.clone()
        } else {
            display_name
        };
        let password_hash = password_hash.into();
        if password_hash.trim().is_empty() {
            return Err(DomainError::Validation(
                "password hash cannot be empty".into(),
            ));
        }
        let subsonic_secret_enc = subsonic_secret_enc.into();
        let api_key = api_key.into();
        if api_key.trim().is_empty() {
            return Err(DomainError::Validation("api key cannot be empty".into()));
        }

        let now = Timestamp::now();
        Ok(Self {
            id: UserId::new(),
            username,
            display_name,
            password_hash,
            subsonic_secret_enc,
            api_key,
            role,
            is_active: true,
            preferences: UserPreferences::default(),
            created_at: now,
            updated_at: now,
        })
    }

    pub fn validate_username(raw: String) -> DomainResult<String> {
        let trimmed = raw.trim().to_string();
        if trimmed.len() < 2 || trimmed.len() > 64 {
            return Err(DomainError::Validation(
                "username length must be between 2 and 64 characters".into(),
            ));
        }
        if !trimmed
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
        {
            return Err(DomainError::Validation(
                "username may only contain ASCII letters, digits, '_', '-', or '.'".into(),
            ));
        }
        Ok(trimmed)
    }

    pub fn ensure_active(&self) -> DomainResult<()> {
        if !self.is_active {
            return Err(DomainError::Forbidden("account has been disabled".into()));
        }
        Ok(())
    }

    pub fn ensure_admin(&self) -> DomainResult<()> {
        if !self.role.is_admin() {
            return Err(DomainError::Forbidden(
                "administrator privileges required".into(),
            ));
        }
        Ok(())
    }

    pub fn ensure_can_modify_user(
        &self,
        target_id: UserId,
        new_role: Option<UserRole>,
        new_is_active: Option<bool>,
    ) -> DomainResult<()> {
        self.ensure_admin()?;
        if self.id == target_id {
            if matches!(new_is_active, Some(false)) {
                return Err(DomainError::Validation(
                    "cannot deactivate your own account".into(),
                ));
            }
            if let Some(role) = new_role
                && !role.is_admin()
            {
                return Err(DomainError::Validation(
                    "cannot demote your own admin role".into(),
                ));
            }
        }
        Ok(())
    }

    pub fn ensure_can_delete_user(&self, target_id: UserId) -> DomainResult<()> {
        self.ensure_admin()?;
        if self.id == target_id {
            return Err(DomainError::Validation(
                "cannot delete your own account".into(),
            ));
        }
        Ok(())
    }

    pub fn activate(&mut self) {
        self.is_active = true;
        self.updated_at = Timestamp::now();
    }

    pub fn deactivate(&mut self) {
        self.is_active = false;
        self.updated_at = Timestamp::now();
    }

    pub fn update_profile(
        &mut self,
        display_name: Option<String>,
        role: Option<UserRole>,
        is_active: Option<bool>,
    ) -> DomainResult<()> {
        if let Some(name) = display_name {
            let trimmed = name.trim();
            if trimmed.is_empty() || trimmed.chars().count() > 128 {
                return Err(DomainError::Validation(
                    "display name must be between 1 and 128 characters".into(),
                ));
            }
            self.display_name = trimmed.to_string();
        }
        if let Some(role) = role {
            self.role = role;
        }
        if let Some(active) = is_active {
            self.is_active = active;
        }
        self.updated_at = Timestamp::now();
        Ok(())
    }

    pub fn update_credentials(
        &mut self,
        new_hash: impl Into<String>,
        new_subsonic_secret_enc: impl Into<String>,
    ) -> DomainResult<()> {
        let new_hash = new_hash.into();
        if new_hash.trim().is_empty() {
            return Err(DomainError::Validation(
                "password hash cannot be empty".into(),
            ));
        }
        self.password_hash = new_hash;
        self.subsonic_secret_enc = new_subsonic_secret_enc.into();
        self.updated_at = Timestamp::now();
        Ok(())
    }

    pub fn rotate_api_key(&mut self, new_api_key: impl Into<String>) -> DomainResult<()> {
        let key = new_api_key.into();
        if key.trim().is_empty() {
            return Err(DomainError::Validation("api key cannot be empty".into()));
        }
        self.api_key = key;
        self.updated_at = Timestamp::now();
        Ok(())
    }

    pub fn update_preferences(&mut self, mut preferences: UserPreferences) {
        preferences.normalize();
        self.preferences = preferences;
        self.updated_at = Timestamp::now();
    }
}

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use xmedia_formats::common::sha256_hex;

use crate::domain::error::{DomainError, DomainResult};
pub use crate::domain::preferences::{
    AudioTranscodeProfile, BookReaderPreferences, ComicFitMode, ComicReaderMode,
    ComicReaderPreferences, Locale, MusicPlayMode, MusicPlaybackPreferences,
    MusicPlayerPreferences, ReadingFlow, ThemeMode, UiPreferences, UserPreferences,
    VideoPlaybackPreferences, VideoPlayerPreferences, VideoQualityProfile,
};

crate::define_uuid_id!(UserId);

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

#[derive(Debug, Clone)]
pub enum UserSecret {
    Password {
        password_hash: String,
        subsonic_secret_enc: String,
    },
    ApiKey(String),
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

fn validate_username(raw: String) -> DomainResult<String> {
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

fn sanitize_initial_display_name(raw: String, fallback_username: &str) -> DomainResult<String> {
    let trimmed = raw.trim();
    if trimmed.chars().count() > 128 {
        return Err(DomainError::Validation(
            "display name cannot exceed 128 characters".into(),
        ));
    }
    Ok(if trimmed.is_empty() {
        fallback_username.to_string()
    } else {
        trimmed.to_string()
    })
}

fn validate_non_empty_secret(raw: String, label: &str) -> DomainResult<String> {
    if raw.trim().is_empty() {
        return Err(DomainError::Validation(format!("{label} cannot be empty")));
    }
    Ok(raw)
}

/// Validates user password length (6..=128 chars) and non-whitespace policy.
pub fn validate_password(password: &str) -> DomainResult<()> {
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

fn validate_display_name(raw: &str) -> DomainResult<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.chars().count() > 128 {
        return Err(DomainError::Validation(
            "display name must be between 1 and 128 characters".into(),
        ));
    }
    Ok(trimmed.to_string())
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
        let username = validate_username(username.into())?;
        let display_name = sanitize_initial_display_name(display_name.into(), &username)?;
        let password_hash = validate_non_empty_secret(password_hash.into(), "password hash")?;
        let api_key = validate_non_empty_secret(api_key.into(), "api key")?;

        let now = Timestamp::now();
        Ok(Self {
            id: UserId::new(),
            username,
            display_name,
            password_hash,
            subsonic_secret_enc: subsonic_secret_enc.into(),
            api_key,
            role,
            is_active: true,
            preferences: UserPreferences::default(),
            created_at: now,
            updated_at: now,
        })
    }

    /// Deterministic credential version fingerprint invalidated whenever password hash,
    /// API key, or role changes.
    pub fn cred_version(&self) -> String {
        let material = format!(
            "{}\0{}\0{}",
            self.password_hash,
            self.api_key,
            self.role.as_str()
        );
        sha256_hex(material.as_bytes())[..16].to_string()
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

    pub fn update_profile(
        &mut self,
        display_name: Option<String>,
        role: Option<UserRole>,
        is_active: Option<bool>,
    ) -> DomainResult<()> {
        if let Some(name) = display_name {
            self.display_name = validate_display_name(&name)?;
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

    pub fn rotate_secret(&mut self, secret: UserSecret) -> DomainResult<()> {
        match secret {
            UserSecret::Password {
                password_hash,
                subsonic_secret_enc,
            } => {
                self.password_hash = validate_non_empty_secret(password_hash, "password hash")?;
                self.subsonic_secret_enc = subsonic_secret_enc;
            }
            UserSecret::ApiKey(key) => {
                self.api_key = validate_non_empty_secret(key, "api key")?;
            }
        }
        self.updated_at = Timestamp::now();
        Ok(())
    }

    pub fn set_preferences(&mut self, preferences: UserPreferences) {
        self.preferences = preferences.normalized();
        self.updated_at = Timestamp::now();
    }
}

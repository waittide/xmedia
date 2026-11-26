#[cfg(feature = "server")]
use jiff::Timestamp;
use serde::{Deserialize, Serialize};

#[cfg(feature = "server")]
use crate::domain::library::{Library, LibraryId, LibraryKind, MediaKind, ScanStatus};
#[cfg(feature = "server")]
use crate::domain::user::{User, UserId, UserPreferences, UserRole};

#[cfg(feature = "server")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetupStatusDto {
    pub needs_setup: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FfmpegStatusDto {
    pub ffmpeg_available: bool,
    pub ffmpeg_version: Option<String>,
    pub ffprobe_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomFontDto {
    pub id: uuid::Uuid,
    pub name: String,
    pub file_name: String,
    pub format: xmedia_formats::font::FontFormat,
    pub css_format: String,
    pub mime_type: String,
    pub size_bytes: u64,
    pub created_at: String,
}

#[cfg(feature = "server")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemCapabilitiesDto {
    pub version: String,
    pub modules: Vec<MediaKind>,
    pub ffmpeg: FfmpegStatusDto,
}

#[cfg(feature = "server")]
#[derive(Debug, Clone, Deserialize)]
pub struct SetupAdminInput {
    pub username: String,
    #[serde(default)]
    pub display_name: Option<String>,
    pub password: String,
}

#[cfg(feature = "server")]
#[derive(Debug, Clone, Deserialize)]
pub struct LoginInput {
    pub username: String,
    pub password: String,
}

#[cfg(feature = "server")]
#[derive(Debug, Clone, Serialize)]
pub struct UserDto {
    pub id: UserId,
    pub username: String,
    pub display_name: String,
    pub role: UserRole,
    pub is_active: bool,
    pub api_key: String,
    pub preferences: UserPreferences,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[cfg(feature = "server")]
impl UserDto {
    /// Builds a `UserDto` with `api_key` redacted (empty string) for administrative user listings
    /// so user secrets are never leaked in bulk responses.
    pub fn from_redacted(u: User) -> Self {
        let mut dto = Self::from(u);
        dto.api_key.clear();
        dto
    }
}

#[cfg(feature = "server")]
impl From<User> for UserDto {
    fn from(u: User) -> Self {
        Self {
            id: u.id,
            username: u.username,
            display_name: u.display_name,
            role: u.role,
            is_active: u.is_active,
            api_key: u.api_key,
            preferences: u.preferences,
            created_at: u.created_at,
            updated_at: u.updated_at,
        }
    }
}

#[cfg(feature = "server")]
#[derive(Debug, Clone, Serialize)]
pub struct AuthSessionDto {
    pub token: String,
    pub user: UserDto,
}

#[cfg(feature = "server")]
#[derive(Debug, Clone, Deserialize)]
pub struct CreateUserInput {
    pub username: String,
    #[serde(default)]
    pub display_name: Option<String>,
    pub password: String,
    #[serde(default)]
    pub role: UserRole,
}

#[cfg(feature = "server")]
#[derive(Debug, Clone, Deserialize)]
pub struct UpdateUserInput {
    pub display_name: Option<String>,
    pub role: Option<UserRole>,
    pub is_active: Option<bool>,
    pub password: Option<String>,
}

#[cfg(feature = "server")]
#[derive(Debug, Clone, Deserialize)]
pub struct ChangePasswordInput {
    pub current_password: String,
    pub new_password: String,
}

#[cfg(feature = "server")]
#[derive(Debug, Clone)]
pub enum AuthCredential<'a> {
    Jwt(&'a str),
    ApiKey(&'a str),
    Basic {
        username: &'a str,
        password: &'a str,
    },
    Subsonic(SubsonicCredentials),
}

#[cfg(feature = "server")]
#[derive(Debug, Clone)]
pub enum SubsonicCredentials {
    ApiKey(String),
    Password {
        username: String,
        password: String,
    },
    TokenSalt {
        username: String,
        token: String,
        salt: String,
    },
}

#[cfg(feature = "server")]
#[derive(Debug, Clone, Serialize)]
pub struct LibraryDto {
    pub id: LibraryId,
    pub name: String,
    pub media_kind: MediaKind,
    pub kind: LibraryKind,
    pub scan_status: ScanStatus,
    pub item_count: u64,
    pub last_scanned_at: Option<Timestamp>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[cfg(feature = "server")]
impl LibraryDto {
    pub fn from_domain(lib: Library, item_count: u64) -> Self {
        Self {
            id: lib.id,
            name: lib.name,
            media_kind: lib.media_kind,
            kind: lib.kind,
            scan_status: lib.scan_status,
            item_count,
            last_scanned_at: lib.last_scanned_at,
            created_at: lib.created_at,
            updated_at: lib.updated_at,
        }
    }
}

#[cfg(feature = "server")]
#[derive(Debug, Clone, Deserialize)]
pub struct CreateLibraryInput {
    pub name: String,
    pub media_kind: MediaKind,
    pub root_path: String,
}

#[cfg(feature = "server")]
#[derive(Debug, Clone, Deserialize)]
pub struct RenameLibraryInput {
    pub name: String,
}

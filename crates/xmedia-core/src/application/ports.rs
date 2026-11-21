use std::pin::Pin;

#[cfg(feature = "server")]
use async_trait::async_trait;
use bytes::Bytes;
use futures_core::Stream;
#[cfg(feature = "server")]
use serde::{Deserialize, Serialize};

use crate::domain::error::DomainResult;
#[cfg(feature = "server")]
use crate::domain::library::{Library, LibraryId, MediaKind};
#[cfg(feature = "server")]
use crate::domain::user::{UserId, UserRole};

pub type ByteChunkStream = Pin<Box<dyn Stream<Item = DomainResult<Bytes>> + Send + 'static>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ByteRangeSpec {
    Full,
    FromTo {
        start: u64,
        end_inclusive: Option<u64>,
    },
    Suffix {
        last_bytes: u64,
    },
    Malformed,
}

impl ByteRangeSpec {
    pub fn is_partial(self) -> bool {
        matches!(self, Self::FromTo { .. } | Self::Suffix { .. })
    }
}

pub struct ByteRangeStream {
    pub stream: ByteChunkStream,
    pub start: u64,
    pub end_inclusive: u64,
    pub total_size: u64,
}

#[derive(Debug, Clone)]
pub struct BinaryPayload {
    pub bytes: Vec<u8>,
    pub mime_type: String,
}

#[cfg(feature = "server")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthClaims {
    pub user_id: UserId,
    pub username: String,
    pub role: UserRole,
    pub cred_ver: String,
    pub jti: String,
    pub exp: u64,
}

#[cfg(feature = "server")]
#[async_trait]
pub trait PasswordHasherPort: Send + Sync {
    async fn hash_password(&self, plain: &str) -> DomainResult<String>;
    async fn verify_password(&self, plain: &str, hash: &str) -> DomainResult<bool>;
    fn encrypt_subsonic_secret(&self, plain: &str) -> DomainResult<String>;
    fn verify_subsonic_token(
        &self,
        encrypted_secret: &str,
        fallback_api_key: &str,
        salt: &str,
        expected_token_hex: &str,
    ) -> DomainResult<bool>;
    fn generate_api_key(&self) -> String;
}

#[cfg(feature = "server")]
pub trait TokenServicePort: Send + Sync {
    fn issue_token(&self, claims: &AuthClaims) -> DomainResult<String>;
    fn verify_token(&self, token: &str) -> DomainResult<AuthClaims>;
    fn ttl_seconds(&self) -> u64;
}

#[cfg(feature = "server")]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum SystemEvent {
    LibraryScanStarted {
        library_id: LibraryId,
        #[serde(default)]
        library_name: String,
        media_kind: MediaKind,
    },
    LibraryScanProgress {
        library_id: LibraryId,
        #[serde(default)]
        library_name: String,
        media_kind: MediaKind,
        scanned: u64,
        #[serde(default)]
        total: Option<u64>,
    },
    LibraryScanCompleted {
        library_id: LibraryId,
        #[serde(default)]
        library_name: String,
        media_kind: MediaKind,
        scanned: u64,
        added: u64,
        updated: u64,
        missing: u64,
    },
    LibraryScanFailed {
        library_id: LibraryId,
        #[serde(default)]
        library_name: String,
        media_kind: MediaKind,
        message: String,
    },
    MediaChanged {
        media_kind: MediaKind,
    },
}

#[cfg(feature = "server")]
pub trait RealtimeEventPort: Send + Sync {
    fn publish(&self, event: SystemEvent);
}

#[cfg(feature = "server")]
#[async_trait]
pub trait MediaDomainLifecycle: Send + Sync {
    fn media_kind(&self) -> MediaKind;
    async fn scan_library(&self, library: Library) -> DomainResult<()>;
    async fn count_library_items(&self, library_id: LibraryId) -> DomainResult<u64>;
    async fn on_library_deleted(&self, library_id: LibraryId) -> DomainResult<()>;
    async fn on_user_deleted(&self, user_id: UserId) -> DomainResult<()>;
}

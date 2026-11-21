mod http;
mod parser;
#[cfg(test)]
mod tests;

use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;

pub use parser::build_subsonic_url;

pub(super) const SUBSONIC_API_VERSION: &str = "1.16.1";
pub(super) const SUBSONIC_CLIENT_NAME: &str = "XMedia";
pub(super) const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
pub(super) const TEST_TIMEOUT: Duration = Duration::from_secs(10);
/// Leave 1 MiB headroom for multipart framing under the 512 MiB local upload limit.
pub(super) const MAX_IMPORT_BYTES: u64 = 511 * 1024 * 1024;
pub(super) const MAX_STREAM_ERROR_SNIFF_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubsonicTestResult {
    pub ok: bool,
    pub latency_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_subsonic: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubsonicAlbumDto {
    pub id: String,
    pub name: String,
    pub artist: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub year: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub song_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubsonicSongDto {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suffix: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bit_rate: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover_url: Option<String>,
    pub stream_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubsonicAlbumDetailDto {
    pub album: SubsonicAlbumDto,
    pub songs: Vec<SubsonicSongDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubsonicSearchResultDto {
    pub albums: Vec<SubsonicAlbumDto>,
    pub songs: Vec<SubsonicSongDto>,
}

#[derive(Debug)]
pub enum SubsonicError {
    BadRequest(String),
    Upstream(String),
    Unsupported(String),
    TooLarge(String),
    Internal(String),
}

impl SubsonicError {
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Upstream(_) => StatusCode::BAD_GATEWAY,
            Self::Unsupported(_) => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Self::TooLarge(_) => StatusCode::PAYLOAD_TOO_LARGE,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::BadRequest(_) => "invalid_request",
            Self::Upstream(_) => "upstream_error",
            Self::Unsupported(_) => "unsupported_media_type",
            Self::TooLarge(_) => "payload_too_large",
            Self::Internal(_) => "internal_error",
        }
    }
}

impl fmt::Display for SubsonicError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadRequest(m)
            | Self::Upstream(m)
            | Self::Unsupported(m)
            | Self::TooLarge(m)
            | Self::Internal(m) => {
                write!(f, "{m}")
            }
        }
    }
}

impl std::error::Error for SubsonicError {}

#[derive(Clone)]
pub struct SubsonicClient {
    pub(super) secure: reqwest::Client,
    pub(super) insecure: reqwest::Client,
    pub(super) local_upload: reqwest::Client,
    pub(super) import_semaphore: Arc<Semaphore>,
    pub(super) temp_dir: PathBuf,
}

impl SubsonicClient {
    pub fn new(
        secure: reqwest::Client,
        insecure: reqwest::Client,
        local_upload: reqwest::Client,
        import_semaphore: Arc<Semaphore>,
        temp_dir: PathBuf,
    ) -> Self {
        Self {
            secure,
            insecure,
            local_upload,
            import_semaphore,
            temp_dir,
        }
    }

    pub(super) fn client_for(&self, allow_insecure_tls: bool) -> &reqwest::Client {
        if allow_insecure_tls {
            &self.insecure
        } else {
            &self.secure
        }
    }
}

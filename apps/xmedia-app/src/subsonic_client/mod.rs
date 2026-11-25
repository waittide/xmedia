mod http;
mod parser;
#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;

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

pub type SubsonicError = crate::remote_common::RemoteClientError;

#[derive(Clone)]
pub struct SubsonicClient {
    pub(super) secure: reqwest::Client,
    pub(super) insecure: reqwest::Client,
    pub(super) import_semaphore: Arc<Semaphore>,
    pub(super) temp_dir: PathBuf,
}

impl SubsonicClient {
    pub fn new(
        secure: reqwest::Client,
        insecure: reqwest::Client,
        import_semaphore: Arc<Semaphore>,
        temp_dir: PathBuf,
    ) -> Self {
        Self {
            secure,
            insecure,
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

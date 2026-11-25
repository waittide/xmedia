mod helpers;
mod http;
mod parser;
#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;

pub(crate) use helpers::{
    expand_pse_template_url, parse_filename_from_content_disposition, sanitize_filename_component,
};
pub(crate) use http::{
    ReadLimitError, SniffImageError, apply_proxy_security_headers,
    build_sniffed_raster_image_response, is_disallowed_non_raster_header,
    is_safe_raster_image_content_type, read_response_text_limited, same_origin_redirect_policy,
};

pub(super) const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
pub(super) const TEST_TIMEOUT: Duration = Duration::from_secs(10);
pub(crate) const MAX_METADATA_BYTES: usize = 8 * 1024 * 1024;
pub(crate) const MAX_IMAGE_BYTES: u64 = 12 * 1024 * 1024;
/// Leave 1 MiB headroom for multipart framing under the 512 MiB local upload limit.
pub(super) const MAX_IMPORT_BYTES: u64 = 511 * 1024 * 1024;
pub(super) const MAX_REDIRECTS: usize = 10;
pub(super) const MAX_XML_DEPTH: usize = 64;
pub(super) const MAX_SANITIZED_FILENAME_CHARS: usize = 120;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpdsTestResult {
    pub ok: bool,
    pub latency_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opds_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpdsNavLinkDto {
    pub title: String,
    pub href: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpdsDownloadLinkDto {
    pub href: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub format: String,
    pub media_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpdsEntryDto {
    pub id: String,
    pub title: String,
    pub authors: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pse_stream_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_count: Option<u64>,
    pub downloads: Vec<OpdsDownloadLinkDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpdsFeedDto {
    pub title: String,
    pub url: String,
    pub opds_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prev_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_url: Option<String>,
    pub nav_links: Vec<OpdsNavLinkDto>,
    pub entries: Vec<OpdsEntryDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteImportResultDto {
    pub media_kind: String,
    pub imported_id: String,
    pub title: String,
}

pub type OpdsError = crate::remote_common::RemoteClientError;

#[derive(Clone)]
pub struct OpdsClient {
    pub(super) secure: reqwest::Client,
    pub(super) insecure: reqwest::Client,
    pub(super) local_upload: reqwest::Client,
    pub(super) import_semaphore: Arc<Semaphore>,
    pub(super) temp_dir: PathBuf,
}

impl OpdsClient {
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

    pub(super) fn apply_auth(
        &self,
        req: reqwest::RequestBuilder,
        username: &str,
        password: &str,
    ) -> reqwest::RequestBuilder {
        if !username.trim().is_empty() {
            req.basic_auth(username.trim(), Some(password))
        } else {
            req
        }
    }
}

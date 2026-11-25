use std::fmt;
use std::path::{Path, PathBuf};

use axum::http::StatusCode;
use bytes::Bytes;
use futures_util::StreamExt;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

#[derive(Debug)]
pub enum RemoteClientError {
    BadRequest(String),
    Upstream(String),
    Unsupported(String),
    TooLarge(String),
    Internal(String),
}

impl RemoteClientError {
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

impl fmt::Display for RemoteClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadRequest(m)
            | Self::Upstream(m)
            | Self::Unsupported(m)
            | Self::TooLarge(m)
            | Self::Internal(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for RemoteClientError {}

pub(crate) struct TempDownloadFile {
    pub path: PathBuf,
    pub size: u64,
}

impl Drop for TempDownloadFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

async fn create_private_temp_file(path: &Path) -> std::io::Result<tokio::fs::File> {
    let mut opts = tokio::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        opts.mode(0o600);
    }
    opts.open(path).await
}

pub(crate) async fn stream_response_to_temp_file(
    resp: reqwest::Response,
    max_bytes: u64,
    temp_dir: &Path,
    prefix: &str,
    label: &str,
) -> Result<TempDownloadFile, RemoteClientError> {
    if let Some(len) = resp.content_length()
        && len > max_bytes
    {
        return Err(RemoteClientError::TooLarge(format!(
            "Remote {label} exceeds maximum limit of {max_bytes} bytes ({len} bytes)"
        )));
    }

    tokio::fs::create_dir_all(temp_dir).await.map_err(|e| {
        RemoteClientError::Internal(format!("Failed to prepare temp directory: {e}"))
    })?;
    let path = temp_dir.join(format!("{prefix}-{}.tmp", Uuid::new_v4()));
    // Declare RAII guard BEFORE opening the file handle so `file` drops first (Windows unlink safety).
    let mut guard = TempDownloadFile {
        path: path.clone(),
        size: 0,
    };
    let mut file = create_private_temp_file(&path)
        .await
        .map_err(|e| RemoteClientError::Internal(format!("Failed to create temp file: {e}")))?;

    let mut stream = resp.bytes_stream();
    while let Some(chunk_res) = stream.next().await {
        let chunk = chunk_res.map_err(|e| {
            RemoteClientError::Upstream(format!(
                "Error reading {label} stream: {}",
                e.without_url()
            ))
        })?;
        guard.size = guard.size.saturating_add(chunk.len() as u64);
        if guard.size > max_bytes {
            drop(file);
            return Err(RemoteClientError::TooLarge(format!(
                "Remote {label} exceeded maximum limit of {max_bytes} bytes"
            )));
        }
        file.write_all(&chunk).await.map_err(|err| {
            RemoteClientError::Internal(format!(
                "Failed writing downloaded {label} data to temp file: {err}"
            ))
        })?;
    }
    file.flush()
        .await
        .map_err(|e| RemoteClientError::Internal(format!("Failed to flush temp file: {e}")))?;

    if guard.size == 0 {
        drop(file);
        return Err(RemoteClientError::Upstream(format!(
            "Remote {label} returned an empty file"
        )));
    }

    Ok(guard)
}

pub(crate) async fn read_temp_file_bytes(
    temp_file: &TempDownloadFile,
    label: &str,
) -> Result<Bytes, RemoteClientError> {
    let bytes = tokio::fs::read(&temp_file.path).await.map_err(|e| {
        RemoteClientError::Internal(format!("Failed reading downloaded {label} file: {e}"))
    })?;
    Ok(Bytes::from(bytes))
}

use std::fmt;
use std::path::{Path, PathBuf};

use axum::http::StatusCode;
use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use serde_json::Value;
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

pub(crate) fn tokio_util_file_stream(
    file: tokio::fs::File,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static {
    futures_util::stream::unfold(Some(file), |state| async move {
        let mut f = state?;
        let mut buf = vec![0u8; 64 * 1024];
        match tokio::io::AsyncReadExt::read(&mut f, &mut buf).await {
            Ok(0) => None,
            Ok(n) => {
                buf.truncate(n);
                Some((Ok(Bytes::from(buf)), Some(f)))
            }
            Err(err) => Some((Err(err), None)),
        }
    })
}

pub(crate) async fn upload_temp_file_to_local(
    local_upload: &reqwest::Client,
    temp_file: &TempDownloadFile,
    filename: &str,
    upload_url: &str,
    local_token: &str,
    label: &str,
) -> Result<Value, RemoteClientError> {
    let file = tokio::fs::File::open(&temp_file.path)
        .await
        .map_err(|e| RemoteClientError::Internal(format!("Failed to reopen temp file: {e}")))?;
    let stream = tokio_util_file_stream(file);
    let part = reqwest::multipart::Part::stream_with_length(
        reqwest::Body::wrap_stream(stream),
        temp_file.size,
    )
    .file_name(filename.to_string());
    let form = reqwest::multipart::Form::new().part("file", part);

    let upload_resp = local_upload
        .post(upload_url)
        .bearer_auth(local_token)
        .multipart(form)
        .send()
        .await
        .map_err(|e| {
            RemoteClientError::Internal(format!(
                "Failed to upload {label} into local library: {}",
                e.without_url()
            ))
        })?;

    let status = upload_resp.status();
    let body_bytes = upload_resp.bytes().await.map_err(|e| {
        RemoteClientError::Internal(format!(
            "Failed to read local {label} upload response: {}",
            e.without_url()
        ))
    })?;
    let parsed_json: Option<Value> = serde_json::from_slice(&body_bytes).ok();

    if !status.is_success() {
        let msg = parsed_json
            .as_ref()
            .and_then(|v| v.pointer("/error/message").and_then(Value::as_str))
            .map(str::to_string)
            .unwrap_or_else(|| {
                let snippet = String::from_utf8_lossy(&body_bytes);
                let trimmed = snippet.trim();
                if trimmed.is_empty() {
                    format!("Local library rejected uploaded {label}")
                } else {
                    trimmed.chars().take(200).collect()
                }
            });
        if status == StatusCode::PAYLOAD_TOO_LARGE {
            return Err(RemoteClientError::TooLarge(format!(
                "Local {label} import rejected (HTTP 413): {msg}"
            )));
        }
        return Err(RemoteClientError::Upstream(format!(
            "Local {label} import failed (HTTP {}): {msg}",
            status.as_u16()
        )));
    }

    parsed_json.ok_or_else(|| {
        RemoteClientError::Internal(format!(
            "Local {label} upload succeeded but returned invalid JSON"
        ))
    })
}

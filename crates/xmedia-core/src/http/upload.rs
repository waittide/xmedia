use std::path::{Path, PathBuf};
use std::sync::RwLock;

use axum::extract::multipart::Field;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use super::{ApiError, ApiResult};
use crate::domain::error::{DomainError, DomainResult};

/// RAII temporary upload file created while streaming multipart uploads to disk.
/// Automatically removes the underlying file on drop unless moved elsewhere.
#[derive(Debug)]
pub struct TempUploadFile {
    pub(crate) path: PathBuf,
    pub(crate) size: u64,
}

impl TempUploadFile {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn is_empty(&self) -> bool {
        self.size == 0
    }

    pub async fn from_bytes(bytes: &[u8]) -> DomainResult<Self> {
        let dir = resolve_temp_dir();
        let path = dir.join(format!("xmedia-upload-{}.tmp", Uuid::new_v4()));
        tokio::fs::write(&path, bytes)
            .await
            .map_err(|e| DomainError::Storage(format!("failed to write temp upload file: {e}")))?;
        Ok(Self {
            path,
            size: bytes.len() as u64,
        })
    }
}

impl Drop for TempUploadFile {
    fn drop(&mut self) {
        let path = std::mem::take(&mut self.path);
        if path.as_os_str().is_empty() {
            return;
        }
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn_blocking(move || {
                let _ = std::fs::remove_file(&path);
            });
        } else {
            let _ = std::fs::remove_file(&path);
        }
    }
}

static CONFIGURED_TEMP_DIR: RwLock<Option<PathBuf>> = RwLock::new(None);

/// Ensures `dir` exists and removes leftover temporary files/directories from previous interrupted runs.
pub fn prepare_temp_dir(dir: &Path) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if path.is_dir() {
                if name_str.starts_with("xmedia-hls-") {
                    let _ = std::fs::remove_dir_all(path);
                }
                continue;
            }
            if path.is_file() {
                let is_tmp_ext = path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("tmp"));
                let is_scoped_temp =
                    name_str.starts_with("xmedia_import_") || name_str.starts_with("xmedia-video-");
                if is_tmp_ext || is_scoped_temp {
                    let _ = std::fs::remove_file(path);
                }
            }
        }
    }
    Ok(dir.to_path_buf())
}

/// Configures the process-wide writable temporary directory used for multipart uploads
/// (e.g. `<data_dir>/tmp`), creating it and cleaning up stale `*.tmp` files.
pub fn configure_temp_dir(dir: PathBuf) -> std::io::Result<PathBuf> {
    let prepared = prepare_temp_dir(&dir)?;
    if let Ok(mut guard) = CONFIGURED_TEMP_DIR.write() {
        *guard = Some(prepared.clone());
    }
    Ok(prepared)
}

/// Returns the configured application temporary directory if available and existing,
/// otherwise falls back to `std::env::temp_dir()`.
pub fn resolve_temp_dir() -> PathBuf {
    if let Ok(guard) = CONFIGURED_TEMP_DIR.read()
        && let Some(ref dir) = *guard
        && dir.is_dir()
    {
        return dir.clone();
    }
    std::env::temp_dir()
}

/// Streams an Axum multipart `Field` directly into a temporary file chunk-by-chunk,
/// enforcing `max_bytes` incrementally so large uploads never buffer in RAM.
pub async fn stream_multipart_field_to_temp_file(
    field: &mut Field<'_>,
    max_bytes: u64,
    ext_hint: Option<&str>,
) -> ApiResult<TempUploadFile> {
    let dir = resolve_temp_dir();
    stream_multipart_field_to_temp_file_in_dir(field, max_bytes, ext_hint, &dir).await
}

/// Streams an Axum multipart `Field` directly into a temporary file inside `temp_dir`.
pub async fn stream_multipart_field_to_temp_file_in_dir(
    field: &mut Field<'_>,
    max_bytes: u64,
    ext_hint: Option<&str>,
    temp_dir: &Path,
) -> ApiResult<TempUploadFile> {
    let clean_ext = ext_hint
        .map(|e| {
            e.trim()
                .trim_start_matches('.')
                .chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .collect::<String>()
        })
        .filter(|e| !e.is_empty());

    let file_name = match clean_ext {
        Some(ext) => format!("xmedia_upload_{}.{ext}.tmp", Uuid::now_v7()),
        None => format!("xmedia_upload_{}.tmp", Uuid::now_v7()),
    };
    tokio::fs::create_dir_all(temp_dir).await.map_err(|e| {
        ApiError(DomainError::Storage(format!(
            "failed to prepare temp upload directory: {e}"
        )))
    })?;
    let path = temp_dir.join(file_name);
    let mut guard = TempUploadFile {
        path: path.clone(),
        size: 0,
    };
    let mut file = tokio::fs::File::create(&path).await.map_err(|e| {
        ApiError(DomainError::Storage(format!(
            "failed to create temp upload file: {e}"
        )))
    })?;

    while let Some(chunk) = field
        .chunk()
        .await
        .map_err(|e| ApiError(DomainError::Validation(format!("upload read error: {e}"))))?
    {
        if chunk.is_empty() {
            continue;
        }
        guard.size = guard.size.saturating_add(chunk.len() as u64);
        if guard.size > max_bytes {
            return Err(ApiError(DomainError::Validation(format!(
                "uploaded file exceeds maximum allowed size ({max_bytes} bytes)"
            ))));
        }
        file.write_all(&chunk).await.map_err(|e| {
            ApiError(DomainError::Storage(format!(
                "failed writing temp upload chunk: {e}"
            )))
        })?;
    }

    file.flush().await.map_err(|e| {
        ApiError(DomainError::Storage(format!(
            "failed flushing temp upload file: {e}"
        )))
    })?;

    Ok(guard)
}

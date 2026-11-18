use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::time::UNIX_EPOCH;

use crate::domain::error::{DomainError, DomainResult};

const WINDOW_BYTES: usize = 65_536;

/// Computes a fast, path- and timestamp-independent content fingerprint:
/// `BLAKE3(size_le || head_64KB || tail_64KB)`.
pub fn fast_content_fingerprint(path: &Path) -> DomainResult<(u64, String)> {
    let meta = fs::metadata(path)
        .map_err(|e| DomainError::Storage(format!("cannot stat '{}': {e}", path.display())))?;
    let size = meta.len();

    let mut hasher = blake3::Hasher::new();
    hasher.update(&size.to_le_bytes());

    let mut file = File::open(path)
        .map_err(|e| DomainError::Storage(format!("cannot open '{}': {e}", path.display())))?;
    let mut head = vec![
        0_u8;
        usize::try_from(size)
            .unwrap_or(WINDOW_BYTES)
            .min(WINDOW_BYTES)
    ];
    let n = file.read(&mut head).unwrap_or(0);
    hasher.update(&head[..n]);

    if size > WINDOW_BYTES as u64 {
        let tail_offset = size.saturating_sub(WINDOW_BYTES as u64);
        if file.seek(SeekFrom::Start(tail_offset)).is_ok() {
            let mut tail = vec![0_u8; WINDOW_BYTES];
            let tn = file.read(&mut tail).unwrap_or(0);
            hasher.update(&tail[..tn]);
        }
    }

    Ok((size, hasher.finalize().to_hex().to_string()))
}

/// Computes the exact same fingerprint scheme directly over an in-memory buffer.
pub fn fast_bytes_fingerprint(bytes: &[u8]) -> String {
    let size = bytes.len() as u64;
    let mut hasher = blake3::Hasher::new();
    hasher.update(&size.to_le_bytes());
    let head_len = bytes.len().min(WINDOW_BYTES);
    hasher.update(&bytes[..head_len]);
    if bytes.len() > WINDOW_BYTES {
        let tail_start = bytes.len().saturating_sub(WINDOW_BYTES);
        hasher.update(&bytes[tail_start..]);
    }
    hasher.finalize().to_hex().to_string()
}

/// Returns Unix modification timestamp in seconds for `path` if available.
pub fn file_mtime_secs(path: &Path) -> Option<i64> {
    let meta = fs::metadata(path).ok()?;
    let modified = meta.modified().ok()?;
    let dur = modified.duration_since(UNIX_EPOCH).ok()?;
    i64::try_from(dur.as_secs()).ok()
}

/// Returns the maximum Unix modification timestamp (in seconds) across `primary`
/// and any existing `sidecars` files so sidecar edits invalidate incremental caches.
pub fn max_mtime_secs(primary: &Path, sidecars: impl IntoIterator<Item = impl AsRef<Path>>) -> i64 {
    let mut latest = file_mtime_secs(primary).unwrap_or(0);
    for sidecar in sidecars {
        if let Some(ts) = file_mtime_secs(sidecar.as_ref()) {
            latest = latest.max(ts);
        }
    }
    latest
}

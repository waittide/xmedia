use std::io::SeekFrom;
use std::path::{Component, Path, PathBuf};
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::{Bytes, BytesMut};
use chardetng::{EncodingDetector, Iso2022JpDetection, Utf8Detection};
use encoding_rs::{GB18030, UTF_8, UTF_16BE, UTF_16LE};
use futures_core::Stream;
#[cfg(feature = "server")]
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::sync::mpsc;

use crate::application::ports::{ByteChunkStream, ByteRangeSpec, ByteRangeStream};
use crate::domain::error::{DomainError, DomainResult};

pub struct MpscByteStream {
    rx: mpsc::Receiver<DomainResult<Bytes>>,
}

impl MpscByteStream {
    pub fn new(rx: mpsc::Receiver<DomainResult<Bytes>>) -> Self {
        Self { rx }
    }

    pub fn into_boxed(rx: mpsc::Receiver<DomainResult<Bytes>>) -> ByteChunkStream {
        Box::pin(Self::new(rx))
    }
}

impl Stream for MpscByteStream {
    type Item = DomainResult<Bytes>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.rx.poll_recv(cx)
    }
}

pub fn sanitize_join(base: &Path, rel: &str) -> DomainResult<PathBuf> {
    let rel_path = Path::new(rel);
    if rel_path.is_absolute() {
        return Err(DomainError::Storage(
            "relative path must not be absolute".into(),
        ));
    }
    let mut out = base.to_path_buf();
    for comp in rel_path.components() {
        match comp {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(DomainError::Storage(
                    "path traversal component rejected".into(),
                ));
            }
        }
    }
    Ok(out)
}

#[cfg(feature = "server")]
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

pub async fn open_file_range_stream_spec(
    path: &Path,
    spec: ByteRangeSpec,
) -> DomainResult<ByteRangeStream> {
    let mut file = tokio::fs::File::open(path).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            DomainError::NotFound("media file not found".into())
        } else {
            DomainError::Storage(format!("failed to open media file {}: {e}", path.display()))
        }
    })?;
    let meta = file.metadata().await.map_err(|e| {
        DomainError::Storage(format!("failed to stat media file {}: {e}", path.display()))
    })?;
    if !meta.is_file() {
        return Err(DomainError::NotFound(
            "media path is not a regular file".into(),
        ));
    }
    let total_size = meta.len();
    if total_size == 0 {
        return match spec {
            ByteRangeSpec::Full | ByteRangeSpec::Malformed => {
                let (_tx, rx) = mpsc::channel::<DomainResult<Bytes>>(1);
                Ok(ByteRangeStream {
                    stream: MpscByteStream::into_boxed(rx),
                    start: 0,
                    end_inclusive: 0,
                    total_size: 0,
                })
            }
            ByteRangeSpec::FromTo { .. } | ByteRangeSpec::Suffix { .. } => {
                Err(DomainError::RangeNotSatisfiable { total_size: 0 })
            }
        };
    }

    let (start, end_inclusive) = match spec {
        // RFC 9110 §14.1.2: malformed Range headers are ignored and served as full 200 OK.
        ByteRangeSpec::Full | ByteRangeSpec::Malformed => (0, total_size - 1),
        ByteRangeSpec::FromTo {
            start: s,
            end_inclusive: maybe_end,
        } => {
            if s >= total_size {
                return Err(DomainError::RangeNotSatisfiable { total_size });
            }
            let end = maybe_end.unwrap_or(total_size - 1).min(total_size - 1);
            if end < s {
                return Err(DomainError::RangeNotSatisfiable { total_size });
            }
            (s, end)
        }
        ByteRangeSpec::Suffix { last_bytes } => {
            if last_bytes == 0 {
                return Err(DomainError::RangeNotSatisfiable { total_size });
            }
            let take_len = last_bytes.min(total_size);
            (total_size - take_len, total_size - 1)
        }
    };

    if start > 0 {
        file.seek(SeekFrom::Start(start))
            .await
            .map_err(|e| DomainError::Storage(format!("failed to seek media file: {e}")))?;
    }

    let remaining = end_inclusive - start + 1;
    let (tx, rx) = mpsc::channel::<DomainResult<Bytes>>(8);
    let mut limited = file.take(remaining);

    tokio::spawn(async move {
        const CHUNK_SIZE: usize = 64 * 1024;
        let mut buf = BytesMut::with_capacity(CHUNK_SIZE);
        loop {
            if buf.capacity() < CHUNK_SIZE / 2 {
                buf.reserve(CHUNK_SIZE);
            }
            match limited.read_buf(&mut buf).await {
                Ok(0) => break,
                Ok(_) => {
                    let chunk = buf.split().freeze();
                    if tx.send(Ok(chunk)).await.is_err() {
                        break;
                    }
                }
                Err(e) => {
                    let _ = tx
                        .send(Err(DomainError::Storage(format!(
                            "file stream read error: {e}"
                        ))))
                        .await;
                    break;
                }
            }
        }
    });

    Ok(ByteRangeStream {
        stream: MpscByteStream::into_boxed(rx),
        start,
        end_inclusive,
        total_size,
    })
}

/// Moves a temporary file into `dest`, falling back to streaming copy + delete when crossing
/// filesystems (`EXDEV`), returning the final file size in bytes.
pub async fn move_or_copy_file(src: &Path, dest: &Path) -> DomainResult<u64> {
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|e| {
            DomainError::Storage(format!(
                "failed to create destination dir {}: {e}",
                parent.display()
            ))
        })?;
    }
    if tokio::fs::rename(src, dest).await.is_err() {
        let parent = dest.parent().unwrap_or_else(|| Path::new("."));
        let file_name = dest
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("staged");
        let staged = parent.join(format!(".{file_name}.{}.tmp", uuid::Uuid::now_v7()));
        if let Err(e) = tokio::fs::copy(src, &staged).await {
            let _ = tokio::fs::remove_file(&staged).await;
            return Err(DomainError::Storage(format!(
                "failed to copy uploaded file to {}: {e}",
                dest.display()
            )));
        }
        if let Err(e) = tokio::fs::rename(&staged, dest).await {
            let _ = tokio::fs::remove_file(&staged).await;
            return Err(DomainError::Storage(format!(
                "failed to finalize uploaded file {}: {e}",
                dest.display()
            )));
        }
        let _ = tokio::fs::remove_file(src).await;
    }
    let meta = tokio::fs::metadata(dest).await.map_err(|e| {
        DomainError::Storage(format!(
            "failed to stat persisted file {}: {e}",
            dest.display()
        ))
    })?;
    Ok(meta.len())
}

/// Computes SHA-256 hex digest of a file on disk using a streaming buffer inside `spawn_blocking`.
#[cfg(feature = "server")]
pub async fn sha256_file_hex(path: PathBuf) -> DomainResult<(u64, String)> {
    tokio::task::spawn_blocking(move || {
        use std::io::Read;
        let mut file = std::fs::File::open(&path).map_err(|e| {
            DomainError::Storage(format!(
                "failed to open file for hashing {}: {e}",
                path.display()
            ))
        })?;
        let mut hasher = Sha256::new();
        let mut buf = [0u8; 64 * 1024];
        let mut total = 0u64;
        loop {
            let n = file.read(&mut buf).map_err(|e| {
                DomainError::Storage(format!("failed reading file for hashing: {e}"))
            })?;
            if n == 0 {
                break;
            }
            total += n as u64;
            hasher.update(&buf[..n]);
        }
        Ok((total, hex::encode(hasher.finalize())))
    })
    .await
    .map_err(|e| DomainError::Storage(format!("sha256_file_hex task failed: {e}")))?
}

/// Decodes arbitrary text bytes with BOM & chardetng fallback (UTF-8 / UTF-16 / GB18030 / Shift-JIS).
pub fn decode_text_smart(bytes: &[u8]) -> (String, &'static str) {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        let (cow, _, _) = UTF_8.decode(&bytes[3..]);
        return (cow.into_owned(), "UTF-8");
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let (cow, _, _) = UTF_16LE.decode(&bytes[2..]);
        return (cow.into_owned(), "UTF-16LE");
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        let (cow, _, _) = UTF_16BE.decode(&bytes[2..]);
        return (cow.into_owned(), "UTF-16BE");
    }
    if let Ok(utf8) = std::str::from_utf8(bytes) {
        return (utf8.to_string(), "UTF-8");
    }
    let mut detector = EncodingDetector::new(Iso2022JpDetection::Allow);
    let sample_len = bytes.len().min(64 * 1024);
    detector.feed(&bytes[..sample_len], sample_len == bytes.len());
    let guessed = detector.guess(Some(b"cn"), Utf8Detection::Allow);
    let encoding = if guessed == UTF_8 { GB18030 } else { guessed };
    let (cow, used, _) = encoding.decode(bytes);
    (cow.into_owned(), used.name())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::task::Poll;
    use tempfile::TempDir;

    async fn collect_stream(mut stream: ByteChunkStream) -> Vec<u8> {
        let mut out = Vec::new();
        while let Some(item) = std::future::poll_fn(|cx| match stream.as_mut().poll_next(cx) {
            Poll::Ready(v) => Poll::Ready(v),
            Poll::Pending => Poll::Pending,
        })
        .await
        {
            out.extend_from_slice(&item.unwrap());
        }
        out
    }

    #[tokio::test]
    async fn open_file_range_stream_spec_handles_full_partial_and_suffix_ranges() {
        let tmp = TempDir::new().unwrap();
        let file_path = tmp.path().join("sample.bin");
        let payload: Vec<u8> = (0u8..=255).cycle().take(150_000).collect();
        tokio::fs::write(&file_path, &payload).await.unwrap();

        let full = open_file_range_stream_spec(&file_path, ByteRangeSpec::Full)
            .await
            .unwrap();
        assert_eq!(full.start, 0);
        assert_eq!(full.end_inclusive, 149_999);
        assert_eq!(full.total_size, 150_000);
        assert_eq!(collect_stream(full.stream).await, payload);

        let partial = open_file_range_stream_spec(
            &file_path,
            ByteRangeSpec::FromTo {
                start: 100,
                end_inclusive: Some(70_099),
            },
        )
        .await
        .unwrap();
        assert_eq!(partial.start, 100);
        assert_eq!(partial.end_inclusive, 70_099);
        let partial_bytes = collect_stream(partial.stream).await;
        assert_eq!(partial_bytes.len(), 70_000);
        assert_eq!(partial_bytes, payload[100..=70_099]);

        let suffix =
            open_file_range_stream_spec(&file_path, ByteRangeSpec::Suffix { last_bytes: 512 })
                .await
                .unwrap();
        assert_eq!(suffix.start, 150_000 - 512);
        assert_eq!(suffix.end_inclusive, 149_999);
        assert_eq!(
            collect_stream(suffix.stream).await,
            payload[150_000 - 512..]
        );
    }
}

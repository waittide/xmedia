use std::io::SeekFrom;
use std::path::{Component, Path, PathBuf};
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use chardetng::{EncodingDetector, Iso2022JpDetection, Utf8Detection};
use encoding_rs::{GB18030, UTF_8, UTF_16BE, UTF_16LE};
use futures_core::Stream;
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

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

pub async fn open_file_range_stream(
    path: &Path,
    range: Option<(u64, Option<u64>)>,
) -> DomainResult<ByteRangeStream> {
    let spec = match range {
        None => ByteRangeSpec::Full,
        Some((start, end_inclusive)) => ByteRangeSpec::FromTo {
            start,
            end_inclusive,
        },
    };
    open_file_range_stream_spec(path, spec).await
}

pub async fn open_file_range_stream_spec(
    path: &Path,
    spec: ByteRangeSpec,
) -> DomainResult<ByteRangeStream> {
    let mut file = tokio::fs::File::open(path).await.map_err(|e| {
        DomainError::Storage(format!("failed to open media file {}: {e}", path.display()))
    })?;
    let meta = file.metadata().await.map_err(|e| {
        DomainError::Storage(format!("failed to stat media file {}: {e}", path.display()))
    })?;
    let total_size = meta.len();
    if total_size == 0 {
        return Err(DomainError::Storage("media file is empty".into()));
    }

    let (start, end_inclusive) = match spec {
        ByteRangeSpec::Full => (0, total_size - 1),
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
        ByteRangeSpec::Malformed => {
            return Err(DomainError::RangeNotSatisfiable { total_size });
        }
    };

    if start > 0 {
        file.seek(SeekFrom::Start(start))
            .await
            .map_err(|e| DomainError::Storage(format!("failed to seek media file: {e}")))?;
    }

    let mut remaining = end_inclusive - start + 1;
    let (tx, rx) = mpsc::channel::<DomainResult<Bytes>>(8);

    tokio::spawn(async move {
        let mut buf = vec![0u8; 64 * 1024];
        while remaining > 0 {
            let to_read = (remaining as usize).min(buf.len());
            match file.read(&mut buf[..to_read]).await {
                Ok(0) => break,
                Ok(n) => {
                    remaining = remaining.saturating_sub(n as u64);
                    let chunk = Bytes::copy_from_slice(&buf[..n]);
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

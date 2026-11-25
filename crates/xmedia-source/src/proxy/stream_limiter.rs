use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use futures_core::Stream;
use xmedia_core::application::ports::ByteChunkStream;
use xmedia_core::domain::error::{DomainError, DomainResult};

/// Default max byte ceiling for proxied online audio streams (`512 MiB`).
pub const DEFAULT_AUDIO_PROXY_MAX_BYTES: u64 = 512 * 1024 * 1024;
/// Default max byte ceiling for proxied online video streams (`16 GiB`).
pub const DEFAULT_VIDEO_PROXY_MAX_BYTES: u64 = 16 * 1024 * 1024 * 1024;
/// Default max byte ceiling for proxied online subtitle files (`16 MiB`).
pub const DEFAULT_SUBTITLE_PROXY_MAX_BYTES: u64 = 16 * 1024 * 1024;

/// Rejects upstream responses whose declared `Content-Length` already exceeds `max_bytes`.
pub fn validate_proxy_content_length(
    content_length: Option<u64>,
    max_bytes: u64,
    label: &str,
) -> DomainResult<()> {
    if let Some(len) = content_length
        && len > max_bytes
    {
        return Err(DomainError::Validation(format!(
            "{label}体积 ({len} 字节) 超过最大限制 ({max_bytes} 字节)"
        )));
    }
    Ok(())
}

/// Stream wrapper enforcing a strict cumulative byte ceiling on proxied media streams.
///
/// Once the cumulative emitted bytes would exceed `max_bytes`, `StreamByteLimiter` drops the
/// upstream stream immediately, emits a single `Err(DomainError::Validation(...))`, and yields
/// `None` on all subsequent polls so unbounded upstream responses cannot exhaust bandwidth or disk.
pub struct StreamByteLimiter {
    inner: Option<ByteChunkStream>,
    max_bytes: u64,
    emitted_bytes: u64,
}

impl StreamByteLimiter {
    pub fn new<S>(stream: S, max_bytes: u64) -> Self
    where
        S: Stream<Item = DomainResult<Bytes>> + Send + 'static,
    {
        Self {
            inner: Some(Box::pin(stream)),
            max_bytes,
            emitted_bytes: 0,
        }
    }

    pub fn max_bytes(&self) -> u64 {
        self.max_bytes
    }

    pub fn emitted_bytes(&self) -> u64 {
        self.emitted_bytes
    }

    pub fn into_boxed(self) -> ByteChunkStream {
        Box::pin(self)
    }
}

/// Convenience helper wrapping `stream` in a boxed [`StreamByteLimiter`].
pub fn limit_byte_stream<S>(stream: S, max_bytes: u64) -> ByteChunkStream
where
    S: Stream<Item = DomainResult<Bytes>> + Send + 'static,
{
    StreamByteLimiter::new(stream, max_bytes).into_boxed()
}

impl Stream for StreamByteLimiter {
    type Item = DomainResult<Bytes>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let Some(inner) = self.inner.as_mut() else {
            return Poll::Ready(None);
        };

        match inner.as_mut().poll_next(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(None) => {
                self.inner = None;
                Poll::Ready(None)
            }
            Poll::Ready(Some(Err(err))) => {
                self.inner = None;
                Poll::Ready(Some(Err(err)))
            }
            Poll::Ready(Some(Ok(chunk))) => {
                let next_total = self.emitted_bytes.saturating_add(chunk.len() as u64);
                if next_total > self.max_bytes {
                    let max_bytes = self.max_bytes;
                    self.inner = None;
                    return Poll::Ready(Some(Err(DomainError::Validation(format!(
                        "代理媒体流超过最大字节限制 ({max_bytes} bytes)"
                    )))));
                }
                self.emitted_bytes = next_total;
                Poll::Ready(Some(Ok(chunk)))
            }
        }
    }
}

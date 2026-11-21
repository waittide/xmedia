mod books;
mod comics;
mod fs;
mod music;
mod scrapers;
mod settings;
mod video;

use std::collections::HashMap;
use std::convert::Infallible;
use std::pin::Pin;
use std::sync::{LazyLock, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::State;
use axum::http::header::{
    CACHE_CONTROL, CONTENT_SECURITY_POLICY, CONTENT_TYPE, SET_COOKIE, X_CONTENT_TYPE_OPTIONS,
};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::middleware;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use bytes::Bytes;
use futures_core::Stream;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use uuid::Uuid;
use xmedia_core::application::dto::FfmpegStatusDto;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::domain::library::MediaKind;
use xmedia_core::http::ApiResult;

use crate::application::workbench::WorkbenchProgress;
use crate::http::ToolHttpState;
use crate::http::auth::{build_tool_session_cookie, extract_tool_token, require_tool_auth};

pub fn tool_api_router(state: ToolHttpState) -> Router<ToolHttpState> {
    let public_routes = Router::new()
        .route("/status", get(get_tool_status))
        .route("/auth/unlock", post(unlock_tool));

    let protected_routes = Router::new()
        .merge(settings::router())
        .merge(fs::router())
        .merge(scrapers::router())
        .merge(books::router())
        .merge(comics::router())
        .merge(music::router())
        .merge(video::router())
        .route_layer(middleware::from_fn_with_state(state, require_tool_auth));

    public_routes.merge(protected_routes)
}

#[derive(Deserialize)]
pub(super) struct FileTargetQuery {
    pub root_id: Uuid,
    pub path: String,
    pub kind: Option<MediaKind>,
    #[serde(default)]
    pub slot: Option<String>,
}

pub(super) fn default_jpeg_quality() -> u8 {
    84
}

/// Applies strict anti-XSS sandboxing headers to binary asset endpoints (covers, EPUB internal
/// assets, comic pages, scraper image proxy) so untrusted files can never execute scripts in
/// the `xmedia-tool` origin even if navigated to directly.
pub(super) fn apply_sandboxed_asset_headers(
    headers: &mut HeaderMap,
    mime_type: &str,
    cache_control: &'static str,
) {
    if let Ok(ct) = HeaderValue::from_str(mime_type) {
        headers.insert(CONTENT_TYPE, ct);
    }
    headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    headers.insert(
        CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("sandbox; default-src 'none'; img-src 'self' data:"),
    );
    headers.insert(CACHE_CONTROL, HeaderValue::from_static(cache_control));
}

#[derive(Serialize)]
struct ToolStatusResponse {
    version: &'static str,
    requires_password: bool,
    authenticated: bool,
    ffmpeg: FfmpegStatusDto,
}

const FFMPEG_STATUS_CACHE_TTL: Duration = Duration::from_secs(60);

struct CachedFfmpegStatus {
    checked_at: Instant,
    status: FfmpegStatusDto,
}

static FFMPEG_STATUS_CACHE: LazyLock<Mutex<HashMap<(String, String), CachedFfmpegStatus>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

async fn get_cached_ffmpeg_status(
    ffmpeg: &xmedia_core::infra::ffmpeg::FfmpegToolkit,
) -> FfmpegStatusDto {
    let key = (
        ffmpeg.ffmpeg_bin().to_string(),
        ffmpeg.ffprobe_bin().to_string(),
    );
    if let Ok(guard) = FFMPEG_STATUS_CACHE.lock()
        && let Some(entry) = guard.get(&key)
        && entry.checked_at.elapsed() < FFMPEG_STATUS_CACHE_TTL
    {
        return entry.status.clone();
    }
    let status = ffmpeg.check_status().await;
    if let Ok(mut guard) = FFMPEG_STATUS_CACHE.lock() {
        if guard.len() >= 16 && !guard.contains_key(&key) {
            guard.clear();
        }
        guard.insert(
            key,
            CachedFfmpegStatus {
                checked_at: Instant::now(),
                status: status.clone(),
            },
        );
    }
    status
}

async fn get_tool_status(
    State(state): State<ToolHttpState>,
    parts: Parts,
) -> Json<ToolStatusResponse> {
    let provided = extract_tool_token(&parts);
    let authenticated = state.auth.verify_token(provided.as_deref()).is_ok();
    let ffmpeg = get_cached_ffmpeg_status(&state.ffmpeg).await;
    Json(ToolStatusResponse {
        version: env!("CARGO_PKG_VERSION"),
        requires_password: state.auth.requires_password(),
        authenticated,
        ffmpeg,
    })
}

#[derive(Deserialize)]
struct UnlockRequest {
    password: String,
}

#[derive(Serialize)]
struct UnlockResponse {
    token: String,
}

async fn unlock_tool(
    State(state): State<ToolHttpState>,
    Json(req): Json<UnlockRequest>,
) -> ApiResult<Response> {
    let token = state.auth.unlock(&req.password)?;
    let cookie = build_tool_session_cookie(&token);
    let mut res = (StatusCode::OK, Json(UnlockResponse { token })).into_response();
    res.headers_mut().insert(SET_COOKIE, cookie);
    Ok(res)
}

struct MpscByteStream {
    rx: mpsc::Receiver<Result<Bytes, Infallible>>,
}

impl Stream for MpscByteStream {
    type Item = Result<Bytes, Infallible>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.rx.poll_recv(cx)
    }
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum NdjsonFrame<T: Serialize> {
    Progress {
        processed: u64,
        total: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
    Done {
        data: T,
    },
    Error {
        message: String,
    },
}

fn encode_ndjson_line<T: Serialize>(frame: &NdjsonFrame<T>) -> Bytes {
    let mut line = serde_json::to_vec(frame).unwrap_or_default();
    line.push(b'\n');
    Bytes::from(line)
}

pub(super) fn spawn_ndjson_stream<T, Fut, Work>(work: Work) -> Response
where
    T: Serialize + Send + 'static,
    Fut: std::future::Future<Output = DomainResult<T>> + Send + 'static,
    Work: FnOnce(mpsc::Sender<WorkbenchProgress>) -> Fut + Send + 'static,
{
    let (byte_tx, byte_rx) = mpsc::channel::<Result<Bytes, Infallible>>(64);
    let (prog_tx, mut prog_rx) = mpsc::channel::<WorkbenchProgress>(64);

    let progress_byte_tx = byte_tx.clone();
    let mut forward_handle = tokio::spawn(async move {
        while let Some(p) = prog_rx.recv().await {
            if progress_byte_tx.is_closed() {
                break;
            }
            let line = encode_ndjson_line::<T>(&NdjsonFrame::Progress {
                processed: p.processed,
                total: p.total,
                detail: p.detail,
            });
            if progress_byte_tx.send(Ok(line)).await.is_err() {
                break;
            }
        }
    });

    tokio::spawn(async move {
        let work_fut = work(prog_tx);
        tokio::select! {
            biased;
            _ = byte_tx.closed() => {
                forward_handle.abort();
            }
            res = work_fut => {
                tokio::select! {
                    biased;
                    _ = byte_tx.closed() => {
                        forward_handle.abort();
                    }
                    _ = &mut forward_handle => {
                        if !byte_tx.is_closed() {
                            let final_line = match res {
                                Ok(data) => encode_ndjson_line(&NdjsonFrame::Done { data }),
                                Err(err) => encode_ndjson_line::<T>(&NdjsonFrame::Error {
                                    message: err.to_string(),
                                }),
                            };
                            let _ = byte_tx.send(Ok(final_line)).await;
                        }
                    }
                }
            }
        }
    });

    let mut res = (
        StatusCode::OK,
        Body::from_stream(MpscByteStream { rx: byte_rx }),
    )
        .into_response();
    res.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("application/x-ndjson; charset=utf-8"),
    );
    res.headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    res.headers_mut()
        .insert("x-accel-buffering", HeaderValue::from_static("no"));
    res
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    use tokio::sync::Notify;

    use super::*;

    struct CancelGuard {
        dropped: Arc<AtomicBool>,
        notify: Arc<Notify>,
    }

    impl Drop for CancelGuard {
        fn drop(&mut self) {
            self.dropped.store(true, Ordering::SeqCst);
            self.notify.notify_waiters();
        }
    }

    #[tokio::test]
    async fn ndjson_stream_aborts_background_work_when_client_disconnects() {
        let dropped = Arc::new(AtomicBool::new(false));
        let started = Arc::new(Notify::new());
        let cancelled = Arc::new(Notify::new());

        let dropped_clone = Arc::clone(&dropped);
        let started_clone = Arc::clone(&started);
        let cancelled_clone = Arc::clone(&cancelled);

        let response = spawn_ndjson_stream::<String, _, _>(move |_prog_tx| async move {
            let _guard = CancelGuard {
                dropped: dropped_clone,
                notify: cancelled_clone,
            };
            started_clone.notify_one();
            std::future::pending::<()>().await;
            Ok("unreachable".to_string())
        });

        tokio::time::timeout(Duration::from_secs(2), started.notified())
            .await
            .expect("background task should start promptly");

        // Simulate HTTP client disconnecting by dropping the response body stream
        drop(response);

        tokio::time::timeout(Duration::from_secs(2), cancelled.notified())
            .await
            .expect("background task should be cancelled promptly on client disconnect");
        assert!(dropped.load(Ordering::SeqCst));
    }
}

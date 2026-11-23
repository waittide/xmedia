use std::collections::HashMap;
use std::convert::Infallible;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use axum::extract::{FromRef, Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post, put};
use axum::{Json, Router};
use futures_core::Stream;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::{RwLock, mpsc};
use uuid::Uuid;
use xmedia_core::application::activity::{
    ActivityDayDto, ActivityHeatmapDto, ActivityHeatmapQuery, ActivityService, RecordActivityInput,
};
use xmedia_core::application::auth::AuthService;
use xmedia_core::application::dto::{
    ChangePasswordInput, CreateLibraryInput, CreateUserInput, FfmpegStatusDto, LibraryDto,
    LoginInput, RenameLibraryInput, SetupAdminInput, SetupStatusDto, SystemCapabilitiesDto,
    UpdateUserInput, UserDto,
};
use xmedia_core::application::library::LibraryService;
use xmedia_core::application::ports::SystemEvent;
use xmedia_core::domain::library::{LibraryId, MediaKind, ScanStatus};
use xmedia_core::domain::repository::LibraryRepository;
use xmedia_core::domain::user::{UserId, UserPreferences};
use xmedia_core::http::{
    AUTH_COOKIE_NAME, ApiResult, attach_cookie, build_clear_cookie, build_session_cookie,
    error_json_response,
};
use xmedia_core::infra::events::BroadcastEventHub;
use xmedia_core::infra::ffmpeg::FfmpegToolkit;

const FFMPEG_STATUS_CACHE_TTL: Duration = Duration::from_secs(60);
const MAX_SSE_STREAMS_PER_USER: usize = 8;

#[derive(Clone, Default)]
pub struct SseConnectionLimiter {
    counts: Arc<Mutex<HashMap<UserId, usize>>>,
}

impl SseConnectionLimiter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn try_acquire(&self, user_id: UserId) -> Option<SseConnectionPermit> {
        let mut guard = self.counts.lock().ok()?;
        let count = guard.entry(user_id).or_insert(0);
        if *count >= MAX_SSE_STREAMS_PER_USER {
            return None;
        }
        *count += 1;
        Some(SseConnectionPermit {
            counts: self.counts.clone(),
            user_id,
        })
    }
}

pub struct SseConnectionPermit {
    counts: Arc<Mutex<HashMap<UserId, usize>>>,
    user_id: UserId,
}

impl Drop for SseConnectionPermit {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.counts.lock()
            && let Some(count) = guard.get_mut(&self.user_id)
        {
            if *count <= 1 {
                guard.remove(&self.user_id);
            } else {
                *count -= 1;
            }
        }
    }
}

#[derive(Clone)]
struct CachedFfmpegStatus {
    status: FfmpegStatusDto,
    fetched_at: Instant,
}

#[derive(Clone, Default)]
pub struct FfmpegStatusCache {
    inner: Arc<RwLock<Option<CachedFfmpegStatus>>>,
}

impl FfmpegStatusCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn get_or_refresh(&self, ffmpeg: &FfmpegToolkit) -> FfmpegStatusDto {
        self.get_or_refresh_with_ttl(ffmpeg, FFMPEG_STATUS_CACHE_TTL)
            .await
    }

    async fn get_or_refresh_with_ttl(
        &self,
        ffmpeg: &FfmpegToolkit,
        ttl: Duration,
    ) -> FfmpegStatusDto {
        {
            let guard = self.inner.read().await;
            if let Some(entry) = guard.as_ref()
                && entry.fetched_at.elapsed() < ttl
            {
                return entry.status.clone();
            }
        }
        let mut guard = self.inner.write().await;
        if let Some(entry) = guard.as_ref()
            && entry.fetched_at.elapsed() < ttl
        {
            return entry.status.clone();
        }
        let status = ffmpeg.check_status().await;
        *guard = Some(CachedFfmpegStatus {
            status: status.clone(),
            fetched_at: Instant::now(),
        });
        status
    }
}

#[derive(Clone)]
pub struct GatewayState {
    pub auth: Arc<AuthService>,
    pub libraries: Arc<LibraryService>,
    pub libraries_repo: Arc<dyn LibraryRepository>,
    pub activity: Arc<ActivityService>,
    pub events: Arc<BroadcastEventHub>,
    pub sse_limiter: SseConnectionLimiter,
    pub ffmpeg: FfmpegToolkit,
    pub ffmpeg_status_cache: FfmpegStatusCache,
    pub enabled_modules: Arc<Vec<MediaKind>>,
    pub external_web_dir: Option<PathBuf>,
}

impl FromRef<GatewayState> for Arc<AuthService> {
    fn from_ref(state: &GatewayState) -> Self {
        state.auth.clone()
    }
}

pub use xmedia_core::http::WebAuth as CoreWebAuth;

pub fn core_api_router() -> Router<GatewayState> {
    Router::new()
        .route("/auth/setup-status", get(setup_status))
        .route("/auth/setup", post(setup_admin))
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .route("/auth/me", get(get_me))
        .route("/auth/preferences", put(update_preferences))
        .route("/auth/password", post(change_password))
        .route("/auth/rotate-api-key", post(rotate_api_key))
        .route("/users", get(list_users).post(create_user))
        .route("/users/{id}", patch(update_user).delete(delete_user))
        .route("/libraries", get(list_libraries).post(create_library))
        .route(
            "/libraries/{id}",
            get(get_library)
                .patch(rename_library)
                .delete(delete_library),
        )
        .route("/libraries/{id}/scan", post(trigger_library_scan))
        .route("/system/capabilities", get(system_capabilities))
        .route("/system/overview", get(system_overview))
        .route("/activity/heartbeat", post(record_activity_heartbeat))
        .route("/activity/heatmap", get(get_activity_heatmap))
        .route("/events/stream", get(events_stream))
}

#[derive(Serialize)]
struct OkResponse {
    ok: bool,
}

#[derive(Deserialize, Default)]
struct LibraryFilterParams {
    media_kind: Option<String>,
}

#[derive(Serialize)]
pub struct DomainStatSummary {
    pub media_kind: MediaKind,
    pub library_count: usize,
    pub item_count: u64,
    pub scanning: bool,
}

#[derive(Serialize)]
pub struct SystemOverviewDto {
    pub capabilities: SystemCapabilitiesDto,
    pub domains: Vec<DomainStatSummary>,
    pub libraries: Vec<LibraryDto>,
}

async fn setup_status(State(state): State<GatewayState>) -> ApiResult<Json<SetupStatusDto>> {
    Ok(Json(state.auth.setup_status().await?))
}

async fn setup_admin(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Json(input): Json<SetupAdminInput>,
) -> ApiResult<Response> {
    let session = state.auth.setup_initial_admin(input).await?;
    let cookie = build_session_cookie(&session.token, state.auth.token_ttl_seconds(), &headers);
    let res = (StatusCode::CREATED, Json(session)).into_response();
    Ok(attach_cookie(res, cookie))
}

async fn login(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Json(input): Json<LoginInput>,
) -> ApiResult<Response> {
    let session = state.auth.login(&input.username, &input.password).await?;
    let cookie = build_session_cookie(&session.token, state.auth.token_ttl_seconds(), &headers);
    let res = (StatusCode::OK, Json(session)).into_response();
    Ok(attach_cookie(res, cookie))
}

async fn logout(State(state): State<GatewayState>, headers: HeaderMap) -> Response {
    if let Some(auth_val) = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    {
        let trimmed = auth_val.trim();
        if trimmed.len() >= 7 && trimmed[..7].eq_ignore_ascii_case("bearer ") {
            let tok = trimmed[7..].trim();
            if !tok.is_empty() {
                state.auth.revoke_jwt_token(tok);
            }
        }
    }
    if let Some(cookie_hdr) = headers
        .get(axum::http::header::COOKIE)
        .and_then(|v| v.to_str().ok())
    {
        for pair in cookie_hdr.split(';') {
            if let Some((k, v)) = pair.trim().split_once('=')
                && k.trim() == AUTH_COOKIE_NAME
            {
                let tok = v.trim();
                if !tok.is_empty() {
                    state.auth.revoke_jwt_token(tok);
                }
            }
        }
    }
    let cookie = build_clear_cookie(&headers);
    let res = Json(OkResponse { ok: true }).into_response();
    attach_cookie(res, cookie)
}

async fn get_me(
    State(state): State<GatewayState>,
    CoreWebAuth(user): CoreWebAuth,
) -> ApiResult<Json<UserDto>> {
    Ok(Json(state.auth.get_me(&user).await?))
}

async fn update_preferences(
    State(state): State<GatewayState>,
    CoreWebAuth(user): CoreWebAuth,
    Json(prefs): Json<UserPreferences>,
) -> ApiResult<Json<UserDto>> {
    Ok(Json(state.auth.update_preferences(&user, prefs).await?))
}

async fn change_password(
    State(state): State<GatewayState>,
    CoreWebAuth(user): CoreWebAuth,
    headers: HeaderMap,
    Json(input): Json<ChangePasswordInput>,
) -> ApiResult<Response> {
    let session = state.auth.change_own_password(&user, input).await?;
    let cookie = build_session_cookie(&session.token, state.auth.token_ttl_seconds(), &headers);
    let res = (StatusCode::OK, Json(session)).into_response();
    Ok(attach_cookie(res, cookie))
}

async fn rotate_api_key(
    State(state): State<GatewayState>,
    CoreWebAuth(user): CoreWebAuth,
) -> ApiResult<Json<UserDto>> {
    Ok(Json(state.auth.rotate_api_key(&user).await?))
}

async fn list_users(
    State(state): State<GatewayState>,
    CoreWebAuth(user): CoreWebAuth,
) -> ApiResult<Json<Vec<UserDto>>> {
    Ok(Json(state.auth.list_users(&user).await?))
}

async fn create_user(
    State(state): State<GatewayState>,
    CoreWebAuth(user): CoreWebAuth,
    Json(input): Json<CreateUserInput>,
) -> ApiResult<(StatusCode, Json<UserDto>)> {
    let created = state.auth.create_user(&user, input).await?;
    Ok((StatusCode::CREATED, Json(created)))
}

async fn update_user(
    State(state): State<GatewayState>,
    CoreWebAuth(user): CoreWebAuth,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateUserInput>,
) -> ApiResult<Json<UserDto>> {
    Ok(Json(
        state.auth.update_user(&user, UserId(id), input).await?,
    ))
}

async fn delete_user(
    State(state): State<GatewayState>,
    CoreWebAuth(user): CoreWebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<OkResponse>> {
    state.auth.delete_user(&user, UserId(id)).await?;
    let _ = state.activity.purge_user(UserId(id)).await;
    Ok(Json(OkResponse { ok: true }))
}

async fn list_libraries(
    State(state): State<GatewayState>,
    CoreWebAuth(_user): CoreWebAuth,
    Query(q): Query<LibraryFilterParams>,
) -> ApiResult<Json<Vec<LibraryDto>>> {
    let kind = match q.media_kind.as_deref() {
        Some(raw) if !raw.trim().is_empty() => Some(MediaKind::parse(raw)?),
        _ => None,
    };
    Ok(Json(state.libraries.list_libraries(kind).await?))
}

async fn get_library(
    State(state): State<GatewayState>,
    CoreWebAuth(_user): CoreWebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<LibraryDto>> {
    Ok(Json(state.libraries.get_library(LibraryId(id)).await?))
}

async fn create_library(
    State(state): State<GatewayState>,
    CoreWebAuth(user): CoreWebAuth,
    Json(input): Json<CreateLibraryInput>,
) -> ApiResult<(StatusCode, Json<LibraryDto>)> {
    let dto = state.libraries.create_local_mount(&user, input).await?;
    Ok((StatusCode::CREATED, Json(dto)))
}

async fn rename_library(
    State(state): State<GatewayState>,
    CoreWebAuth(user): CoreWebAuth,
    Path(id): Path<Uuid>,
    Json(input): Json<RenameLibraryInput>,
) -> ApiResult<Json<LibraryDto>> {
    Ok(Json(
        state
            .libraries
            .rename_library(&user, LibraryId(id), input)
            .await?,
    ))
}

async fn delete_library(
    State(state): State<GatewayState>,
    CoreWebAuth(user): CoreWebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<OkResponse>> {
    state.libraries.delete_library(&user, LibraryId(id)).await?;
    Ok(Json(OkResponse { ok: true }))
}

async fn trigger_library_scan(
    State(state): State<GatewayState>,
    CoreWebAuth(user): CoreWebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<LibraryDto>> {
    Ok(Json(
        state.libraries.trigger_scan(&user, LibraryId(id)).await?,
    ))
}

async fn system_capabilities(
    State(state): State<GatewayState>,
) -> ApiResult<Json<SystemCapabilitiesDto>> {
    let mut ffmpeg = state
        .ffmpeg_status_cache
        .get_or_refresh(&state.ffmpeg)
        .await;
    // Omit raw FFmpeg version banner on the unauthenticated capabilities endpoint.
    ffmpeg.ffmpeg_version = None;
    Ok(Json(SystemCapabilitiesDto {
        version: env!("CARGO_PKG_VERSION").to_string(),
        modules: (*state.enabled_modules).clone(),
        ffmpeg,
    }))
}

async fn system_overview(
    State(state): State<GatewayState>,
    CoreWebAuth(_user): CoreWebAuth,
) -> ApiResult<Json<SystemOverviewDto>> {
    let ffmpeg = state
        .ffmpeg_status_cache
        .get_or_refresh(&state.ffmpeg)
        .await;
    let libraries = state.libraries.list_libraries(None).await?;
    let mut domains = Vec::with_capacity(state.enabled_modules.len());

    for &kind in state.enabled_modules.iter() {
        let matching: Vec<&LibraryDto> =
            libraries.iter().filter(|l| l.media_kind == kind).collect();
        let item_count: u64 = matching.iter().map(|l| l.item_count).sum();
        let scanning = matching.iter().any(|l| l.scan_status.is_scanning());
        domains.push(DomainStatSummary {
            media_kind: kind,
            library_count: matching.len(),
            item_count,
            scanning,
        });
    }

    Ok(Json(SystemOverviewDto {
        capabilities: SystemCapabilitiesDto {
            version: env!("CARGO_PKG_VERSION").to_string(),
            modules: (*state.enabled_modules).clone(),
            ffmpeg,
        },
        domains,
        libraries,
    }))
}

struct SseEventStream {
    rx: mpsc::Receiver<Result<Event, Infallible>>,
    _permit: Option<SseConnectionPermit>,
}

impl Stream for SseEventStream {
    type Item = Result<Event, Infallible>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.rx.poll_recv(cx)
    }
}

fn spawn_sse_event_stream(
    mut rx: tokio::sync::broadcast::Receiver<SystemEvent>,
    initial_events: Vec<SystemEvent>,
    resync_modules: Vec<MediaKind>,
    permit: Option<SseConnectionPermit>,
) -> SseEventStream {
    let (tx, stream_rx) = mpsc::channel::<Result<Event, Infallible>>(64);
    tokio::spawn(async move {
        for ev in initial_events {
            let Ok(event) = Event::default().json_data(&ev) else {
                continue;
            };
            if tx.send(Ok(event)).await.is_err() {
                return;
            }
        }
        loop {
            tokio::select! {
                biased;
                _ = tx.closed() => break,
                recv = rx.recv() => match recv {
                    Ok(event) => {
                        let Ok(sse_event) = Event::default().json_data(&event) else {
                            continue;
                        };
                        if tx.send(Ok(sse_event)).await.is_err() {
                            break;
                        }
                    }
                    Err(RecvError::Lagged(_)) => {
                        for &media_kind in &resync_modules {
                            let resync = SystemEvent::MediaChanged { media_kind };
                            let Ok(sse_event) = Event::default().json_data(&resync) else {
                                continue;
                            };
                            if tx.send(Ok(sse_event)).await.is_err() {
                                return;
                            }
                        }
                    }
                    Err(RecvError::Closed) => break,
                }
            }
        }
    });
    SseEventStream {
        rx: stream_rx,
        _permit: permit,
    }
}

async fn record_activity_heartbeat(
    State(state): State<GatewayState>,
    CoreWebAuth(user): CoreWebAuth,
    Json(input): Json<RecordActivityInput>,
) -> ApiResult<Json<ActivityDayDto>> {
    Ok(Json(state.activity.record_heartbeat(user.id, input).await?))
}

async fn get_activity_heatmap(
    State(state): State<GatewayState>,
    CoreWebAuth(user): CoreWebAuth,
    Query(query): Query<ActivityHeatmapQuery>,
) -> ApiResult<Json<ActivityHeatmapDto>> {
    Ok(Json(state.activity.get_heatmap(user.id, query).await?))
}

async fn events_stream(
    State(state): State<GatewayState>,
    CoreWebAuth(user): CoreWebAuth,
) -> Response {
    let Some(permit) = state.sse_limiter.try_acquire(user.id) else {
        return error_json_response(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            "too many concurrent event stream connections for this user",
        );
    };
    let rx = state.events.subscribe();
    let initial_scans = state
        .libraries_repo
        .list_all()
        .await
        .unwrap_or_default()
        .into_iter()
        .filter_map(|lib| match lib.scan_status {
            ScanStatus::Scanning { scanned, total } => Some(SystemEvent::LibraryScanProgress {
                library_id: lib.id,
                library_name: lib.name,
                media_kind: lib.media_kind,
                scanned,
                total,
            }),
            _ => None,
        })
        .collect::<Vec<_>>();

    let stream = spawn_sse_event_stream(
        rx,
        initial_scans,
        (*state.enabled_modules).clone(),
        Some(permit),
    );
    let mut res = Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response();
    res.headers_mut()
        .insert("x-accel-buffering", HeaderValue::from_static("no"));
    res
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::poll_fn;
    use tokio::sync::broadcast;

    async fn next_event(stream: &mut SseEventStream) -> Option<Result<Event, Infallible>> {
        poll_fn(|cx| Pin::new(&mut *stream).poll_next(cx)).await
    }

    #[tokio::test]
    async fn sse_stream_forwards_events_and_cleans_up_receiver_on_drop() {
        let (tx, rx) = broadcast::channel(16);
        assert_eq!(tx.receiver_count(), 1);

        let initial = vec![SystemEvent::LibraryScanProgress {
            library_id: LibraryId::new(),
            library_name: "Books".into(),
            media_kind: MediaKind::Book,
            scanned: 3,
            total: Some(10),
        }];
        let mut stream = spawn_sse_event_stream(rx, initial, vec![MediaKind::Book], None);

        let first = tokio::time::timeout(Duration::from_secs(2), next_event(&mut stream))
            .await
            .unwrap();
        assert!(first.is_some());

        tx.send(SystemEvent::MediaChanged {
            media_kind: MediaKind::Video,
        })
        .unwrap();
        let live = tokio::time::timeout(Duration::from_secs(2), next_event(&mut stream))
            .await
            .unwrap();
        assert!(live.is_some());

        // Dropping the SSE stream must terminate the forwarding task and drop the broadcast receiver
        drop(stream);
        tokio::time::timeout(Duration::from_secs(2), async {
            while tx.receiver_count() > 0 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("broadcast receiver should be dropped promptly after SSE client disconnect");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn ffmpeg_status_cache_reuses_result_within_ttl_and_refreshes_after() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::TempDir::new().unwrap();
        let counter_path = tmp.path().join("invocations.log");
        let fake_bin = tmp.path().join("fake-ffmpeg.sh");
        let script = format!(
            "#!/bin/sh\necho x >> \"{}\"\necho 'ffmpeg version fake-1.0'\n",
            counter_path.display()
        );
        std::fs::write(&fake_bin, script).unwrap();
        let mut perms = std::fs::metadata(&fake_bin).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&fake_bin, perms).unwrap();

        let toolkit = FfmpegToolkit::new(
            fake_bin.to_string_lossy().to_string(),
            fake_bin.to_string_lossy().to_string(),
        );
        let cache = FfmpegStatusCache::new();
        let ttl = Duration::from_millis(80);

        let s1 = cache.get_or_refresh_with_ttl(&toolkit, ttl).await;
        assert!(s1.ffmpeg_available);
        assert!(s1.ffprobe_available);
        assert_eq!(
            s1.ffmpeg_version.as_deref(),
            Some("ffmpeg version fake-1.0")
        );

        // Second & third calls within TTL must hit cache without spawning subprocesses
        let s2 = cache.get_or_refresh_with_ttl(&toolkit, ttl).await;
        let s3 = cache.get_or_refresh_with_ttl(&toolkit, ttl).await;
        assert_eq!(s2.ffmpeg_version, s1.ffmpeg_version);
        assert_eq!(s3.ffmpeg_version, s1.ffmpeg_version);

        let count_after_cached = std::fs::read_to_string(&counter_path)
            .unwrap()
            .lines()
            .count();
        assert_eq!(
            count_after_cached, 2,
            "expected exactly 1 ffmpeg + 1 ffprobe invocation within TTL"
        );

        tokio::time::sleep(Duration::from_millis(100)).await;
        let _s4 = cache.get_or_refresh_with_ttl(&toolkit, ttl).await;
        let count_after_expiry = std::fs::read_to_string(&counter_path)
            .unwrap()
            .lines()
            .count();
        assert_eq!(
            count_after_expiry, 4,
            "expected cache refresh after TTL expiry"
        );
    }
}

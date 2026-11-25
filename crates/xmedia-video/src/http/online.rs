use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::header::{
    ACCEPT_RANGES, CACHE_CONTROL, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_SECURITY_POLICY,
    CONTENT_TYPE, RANGE, X_CONTENT_TYPE_OPTIONS,
};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use uuid::Uuid;
use xmedia_core::domain::error::DomainError;
use xmedia_core::http::{ApiError, ApiResult, WebAuth};
use xmedia_source::crud::{LoginActionRequest, sources_prefixed_crud_router};
use xmedia_source::proxy::{IMAGE_PROXY_CACHE_CONTROL, IMAGE_PROXY_CSP};

use crate::application::dto::{ShowDetailDto, VideoSummaryDto};
use crate::application::online::{
    CacheTrackedShowInput, HlsProxyOutcome, LoginActionResponseDto, OnlineVideoBrowseResponseDto,
    OnlineVideoCategoriesResponseDto, OnlineVideoDetailResponseDto, OnlineVideoPlayResponseDto,
    OnlineVideoSearchResponseDto, ProxyStreamOutcome, ProxyStreamPayload, TrackOnlineVideoInput,
    TrackOnlineVideoResultDto, TrackedVideoPlayResponseDto, VideoCacheReportDto,
    VideoSourceTestInput, VideoSourceTestReportDto,
};
use crate::application::ports::VideoProxyUrlBuilder;
use crate::domain::id::{ShowId, VideoId};
use crate::domain::online::VideoSourceSpec;
use crate::http::state::VideoHttpState;

pub fn encode_query_component(val: &str) -> String {
    form_urlencoded::byte_serialize(val.as_bytes()).collect()
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RestVideoProxyUrlBuilder;

impl VideoProxyUrlBuilder for RestVideoProxyUrlBuilder {
    fn poster_proxy_url(&self, target_url: &str, source_id: Option<Uuid>) -> String {
        let mut out = format!(
            "/api/v1/video/online/poster-proxy?url={}",
            encode_query_component(target_url)
        );
        if let Some(sid) = source_id {
            out.push_str("&source_id=");
            out.push_str(&sid.to_string());
        }
        out
    }

    fn stream_proxy_url(&self, target_url: &str, source_id: Uuid, token: Option<&str>) -> String {
        let mut out = format!(
            "/api/v1/video/online/stream-proxy?url={}&source_id={source_id}",
            encode_query_component(target_url)
        );
        if let Some(tok) = token.map(str::trim).filter(|t| !t.is_empty()) {
            out.push_str("&token=");
            out.push_str(&encode_query_component(tok));
        }
        out
    }

    fn hls_proxy_url(&self, target_url: &str, source_id: Uuid, token: Option<&str>) -> String {
        let mut out = format!(
            "/api/v1/video/online/hls-proxy?url={}&source_id={source_id}",
            encode_query_component(target_url)
        );
        if let Some(tok) = token.map(str::trim).filter(|t| !t.is_empty()) {
            out.push_str("&token=");
            out.push_str(&encode_query_component(tok));
        }
        out
    }

    fn subtitle_proxy_url(&self, target_url: &str, source_id: Uuid, token: Option<&str>) -> String {
        let mut out = format!(
            "/api/v1/video/online/subtitle-proxy?url={}&source_id={source_id}",
            encode_query_component(target_url)
        );
        if let Some(tok) = token.map(str::trim).filter(|t| !t.is_empty()) {
            out.push_str("&token=");
            out.push_str(&encode_query_component(tok));
        }
        out
    }
}

pub fn router() -> Router<VideoHttpState> {
    Router::new()
        .merge(sources_prefixed_crud_router::<
            VideoHttpState,
            VideoSourceSpec,
        >())
        .route("/sources/{id}/login-action", post(run_login_action))
        .route("/sources/{id}/test", post(test_source))
        // Online video endpoints & proxies
        .route("/online/categories", post(online_categories))
        .route("/online/search", post(online_search))
        .route("/online/browse", post(online_browse))
        .route("/online/detail", post(online_detail))
        .route("/online/play", post(online_play))
        .route("/online/track", post(track_online_video))
        .route("/shows/{id}/online/refresh", post(refresh_tracked_show))
        .route("/shows/{id}/online/cache", post(cache_tracked_show))
        .route("/videos/{id}/online/cache", post(cache_tracked_video))
        .route("/videos/{id}/online/play", get(resolve_tracked_video_play))
        .route("/online/poster-proxy", get(online_poster_proxy))
        .route("/online/stream-proxy", get(online_stream_proxy))
        .route("/online/hls-proxy", get(online_hls_proxy))
        .route("/online/subtitle-proxy", get(online_subtitle_proxy))
        // Per-user online video favorites & episode playback progress
        .nest(
            "/online/items",
            xmedia_core::http::online_items::online_items_router(),
        )
}

fn default_page() -> u32 {
    1
}

#[derive(Debug, Deserialize)]
struct OnlineCategoriesRequest {
    source_id: Uuid,
}

#[derive(Debug, Deserialize)]
struct OnlineSearchRequest {
    keyword: String,
    source_ids: Option<Vec<Uuid>>,
    #[serde(default = "default_page")]
    page: u32,
}

#[derive(Debug, Deserialize)]
struct OnlineBrowseRequest {
    source_id: Uuid,
    category_id: Option<String>,
    #[serde(default = "default_page")]
    page: u32,
}

#[derive(Debug, Deserialize)]
struct OnlineDetailRequest {
    source_id: Uuid,
    video_id: Option<String>,
    detail_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OnlinePlayRequest {
    source_id: Uuid,
    play_url: String,
    group_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PosterProxyQuery {
    url: String,
    source_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
struct StreamProxyQuery {
    url: String,
    source_id: Option<Uuid>,
    token: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct TrackedPlayQuery {
    token: Option<String>,
}

fn require_query_source_id(source_id: Option<Uuid>) -> Result<Uuid, DomainError> {
    source_id.ok_or_else(|| DomainError::Validation("缺少必填参数 source_id".into()))
}

async fn run_login_action(
    State(state): State<VideoHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
    Json(req): Json<LoginActionRequest>,
) -> ApiResult<Json<LoginActionResponseDto>> {
    let out = state
        .services
        .online
        .run_login_action(&user, id, req.login_info, req.action)
        .await?;
    Ok(Json(out))
}

async fn test_source(
    State(state): State<VideoHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
    Json(req): Json<VideoSourceTestInput>,
) -> ApiResult<Json<VideoSourceTestReportDto>> {
    let out = state
        .services
        .online
        .test_source(&user, id, req, &RestVideoProxyUrlBuilder)
        .await?;
    Ok(Json(out))
}

async fn online_categories(
    State(state): State<VideoHttpState>,
    WebAuth(user): WebAuth,
    Json(req): Json<OnlineCategoriesRequest>,
) -> ApiResult<Json<OnlineVideoCategoriesResponseDto>> {
    let out = state
        .services
        .online
        .categories(&user, req.source_id)
        .await?;
    Ok(Json(out))
}

async fn online_search(
    State(state): State<VideoHttpState>,
    WebAuth(user): WebAuth,
    Json(req): Json<OnlineSearchRequest>,
) -> ApiResult<Json<OnlineVideoSearchResponseDto>> {
    let out = state
        .services
        .online
        .search(
            &user,
            &req.keyword,
            req.source_ids,
            req.page,
            &RestVideoProxyUrlBuilder,
        )
        .await?;
    Ok(Json(out))
}

async fn online_browse(
    State(state): State<VideoHttpState>,
    WebAuth(user): WebAuth,
    Json(req): Json<OnlineBrowseRequest>,
) -> ApiResult<Json<OnlineVideoBrowseResponseDto>> {
    let out = state
        .services
        .online
        .browse(
            &user,
            req.source_id,
            req.category_id.as_deref(),
            req.page,
            &RestVideoProxyUrlBuilder,
        )
        .await?;
    Ok(Json(out))
}

async fn online_detail(
    State(state): State<VideoHttpState>,
    WebAuth(user): WebAuth,
    Json(req): Json<OnlineDetailRequest>,
) -> ApiResult<Json<OnlineVideoDetailResponseDto>> {
    let out = state
        .services
        .online
        .detail(
            &user,
            req.source_id,
            req.video_id.as_deref(),
            req.detail_url.as_deref(),
            Some(&RestVideoProxyUrlBuilder),
        )
        .await?;
    Ok(Json(out))
}

async fn online_play(
    State(state): State<VideoHttpState>,
    WebAuth(user): WebAuth,
    Json(req): Json<OnlinePlayRequest>,
) -> ApiResult<Json<OnlineVideoPlayResponseDto>> {
    let out = state
        .services
        .online
        .play(
            &user,
            req.source_id,
            &req.play_url,
            req.group_name.as_deref(),
            &RestVideoProxyUrlBuilder,
        )
        .await?;
    Ok(Json(out))
}

async fn track_online_video(
    State(state): State<VideoHttpState>,
    WebAuth(user): WebAuth,
    Json(req): Json<TrackOnlineVideoInput>,
) -> ApiResult<Json<TrackOnlineVideoResultDto>> {
    let out = state.services.online.track_online_video(&user, req).await?;
    Ok(Json(out))
}

async fn refresh_tracked_show(
    State(state): State<VideoHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<ShowDetailDto>> {
    let out = state
        .services
        .online
        .refresh_tracked_show(&user, ShowId(id))
        .await?;
    Ok(Json(out))
}

async fn cache_tracked_video(
    State(state): State<VideoHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<VideoSummaryDto>> {
    let out = state
        .services
        .online
        .cache_tracked_video(&user, VideoId(id))
        .await?;
    Ok(Json(out))
}

async fn cache_tracked_show(
    State(state): State<VideoHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
    body: Option<Json<CacheTrackedShowInput>>,
) -> ApiResult<Json<VideoCacheReportDto>> {
    let video_ids = body.and_then(|Json(b)| b.video_ids);
    let out = state
        .services
        .online
        .cache_tracked_show(&user, ShowId(id), video_ids)
        .await?;
    Ok(Json(out))
}

async fn resolve_tracked_video_play(
    State(state): State<VideoHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
    Query(q): Query<TrackedPlayQuery>,
) -> ApiResult<Json<TrackedVideoPlayResponseDto>> {
    let out = state
        .services
        .online
        .resolve_tracked_video_play(
            &user,
            VideoId(id),
            q.token.as_deref(),
            &RestVideoProxyUrlBuilder,
        )
        .await?;
    Ok(Json(out))
}

async fn online_poster_proxy(
    State(state): State<VideoHttpState>,
    WebAuth(user): WebAuth,
    Query(q): Query<PosterProxyQuery>,
) -> Result<Response, ApiError> {
    let (bytes, mime) = state
        .services
        .online
        .fetch_poster_proxy(&user, &q.url, q.source_id)
        .await?;
    let mut resp = Response::new(Body::from(bytes));
    if let Ok(hv) = HeaderValue::from_str(mime) {
        resp.headers_mut().insert(CONTENT_TYPE, hv);
    }
    resp.headers_mut()
        .insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    resp.headers_mut().insert(
        CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(IMAGE_PROXY_CSP),
    );
    resp.headers_mut().insert(
        CACHE_CONTROL,
        HeaderValue::from_static(IMAGE_PROXY_CACHE_CONTROL),
    );
    Ok(resp)
}

pub(crate) fn build_stream_response(
    payload: ProxyStreamPayload,
    has_query_token: bool,
) -> Response {
    let status = StatusCode::from_u16(payload.status).unwrap_or(StatusCode::OK);
    let mut resp = Response::new(Body::from_stream(payload.stream));
    *resp.status_mut() = status;
    if let Ok(hv) = HeaderValue::from_str(&payload.content_type) {
        resp.headers_mut().insert(CONTENT_TYPE, hv);
    }
    if let Some(len) = payload.content_length
        && let Ok(hv) = HeaderValue::from_str(&len.to_string())
    {
        resp.headers_mut().insert(CONTENT_LENGTH, hv);
    }
    if let Some(ref cr) = payload.content_range
        && let Ok(hv) = HeaderValue::from_str(cr)
    {
        resp.headers_mut().insert(CONTENT_RANGE, hv);
    }
    if let Ok(hv) = HeaderValue::from_str(&payload.accept_ranges) {
        resp.headers_mut().insert(ACCEPT_RANGES, hv);
    }
    resp.headers_mut()
        .insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    resp.headers_mut().insert(
        CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("sandbox; default-src 'none'; media-src 'self'"),
    );
    resp.headers_mut().insert(
        CACHE_CONTROL,
        HeaderValue::from_static(if has_query_token {
            "no-store"
        } else {
            "private, max-age=3600"
        }),
    );
    resp
}

pub(crate) fn build_416_response(content_range: Option<String>) -> Response {
    let mut resp = Response::new(Body::empty());
    *resp.status_mut() = StatusCode::RANGE_NOT_SATISFIABLE;
    if let Some(ref cr) = content_range
        && let Ok(hv) = HeaderValue::from_str(cr)
    {
        resp.headers_mut().insert(CONTENT_RANGE, hv);
    }
    resp.headers_mut()
        .insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    resp
}

async fn online_stream_proxy(
    State(state): State<VideoHttpState>,
    WebAuth(user): WebAuth,
    Query(q): Query<StreamProxyQuery>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let source_id = require_query_source_id(q.source_id)?;
    let range_header = headers.get(RANGE).and_then(|v| v.to_str().ok());
    let has_query_token = q.token.as_deref().is_some_and(|t| !t.trim().is_empty());

    match state
        .services
        .online
        .open_stream_proxy(&user, &q.url, source_id, range_header)
        .await?
    {
        ProxyStreamOutcome::Stream(payload) => Ok(build_stream_response(payload, has_query_token)),
        ProxyStreamOutcome::RangeNotSatisfiable { content_range } => {
            Ok(build_416_response(content_range))
        }
    }
}

async fn online_hls_proxy(
    State(state): State<VideoHttpState>,
    WebAuth(user): WebAuth,
    Query(q): Query<StreamProxyQuery>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let source_id = require_query_source_id(q.source_id)?;
    let range_header = headers.get(RANGE).and_then(|v| v.to_str().ok());
    let query_token = q.token.as_deref().map(str::trim).filter(|t| !t.is_empty());

    match state
        .services
        .online
        .open_hls_proxy(
            &user,
            &q.url,
            source_id,
            range_header,
            query_token,
            &RestVideoProxyUrlBuilder,
        )
        .await?
    {
        HlsProxyOutcome::Playlist(rewritten) => {
            let mut resp = Response::new(Body::from(rewritten));
            resp.headers_mut().insert(
                CONTENT_TYPE,
                HeaderValue::from_static("application/vnd.apple.mpegurl; charset=utf-8"),
            );
            resp.headers_mut()
                .insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
            resp.headers_mut().insert(
                CONTENT_SECURITY_POLICY,
                HeaderValue::from_static("sandbox; default-src 'none'"),
            );
            resp.headers_mut()
                .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
            Ok(resp)
        }
        HlsProxyOutcome::Segment(payload) => {
            Ok(build_stream_response(payload, query_token.is_some()))
        }
        HlsProxyOutcome::RangeNotSatisfiable { content_range } => {
            Ok(build_416_response(content_range))
        }
    }
}

async fn online_subtitle_proxy(
    State(state): State<VideoHttpState>,
    WebAuth(user): WebAuth,
    Query(q): Query<StreamProxyQuery>,
) -> Result<Response, ApiError> {
    let source_id = require_query_source_id(q.source_id)?;
    let has_query_token = q.token.as_deref().is_some_and(|t| !t.trim().is_empty());
    let vtt = state
        .services
        .online
        .open_subtitle_proxy(&user, &q.url, source_id)
        .await?;
    let mut resp = Response::new(Body::from(vtt));
    resp.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/vtt; charset=utf-8"),
    );
    resp.headers_mut()
        .insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    resp.headers_mut().insert(
        CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("sandbox; default-src 'none'"),
    );
    resp.headers_mut().insert(
        CACHE_CONTROL,
        HeaderValue::from_static(if has_query_token {
            "no-store"
        } else {
            "private, max-age=3600"
        }),
    );
    Ok(resp)
}

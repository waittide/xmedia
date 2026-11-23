use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::header::{
    ACCEPT_RANGES, CACHE_CONTROL, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_SECURITY_POLICY,
    CONTENT_TYPE, X_CONTENT_TYPE_OPTIONS,
};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::Response;
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use serde::Deserialize;
use uuid::Uuid;
use xmedia_core::http::{ApiError, ApiResult, WebAuth};
use xmedia_source::crud::{
    ExportSourcesBatchRequest, ImportSourcesRequest, LoginActionRequest, ReorderSourcesRequest,
    SourceVariableResponse,
};
use xmedia_source::proxy::{IMAGE_PROXY_CACHE_CONTROL, IMAGE_PROXY_CSP};

use crate::application::dto::TrackDto;
use crate::application::online::{
    BatchMusicSourcesInput, BatchMusicSourcesReportDto, ImportMusicSourcesReportDto,
    LoginActionResponseDto, MusicSourceSummaryDto, MusicSourceTestInput, MusicSourceTestReportDto,
    OnlineMusicExploreResponseDto, OnlineMusicResolvedTrackDto, OnlineMusicSearchResponseDto,
    OnlineMusicStreamResponseDto, StructuredLyricsDto, TrackOnlineMusicBatchInput,
    TrackOnlineMusicInput, UpdateMusicSourceInput,
};
use crate::domain::catalog::TrackId;
use crate::http::state::MusicHttpState;
use crate::source::model::MusicTrackContext;

pub fn router() -> Router<MusicHttpState> {
    Router::new()
        // Music source management
        .route("/sources", get(list_sources))
        .route("/sources/reorder", post(reorder_sources))
        .route("/sources/batch", post(batch_sources))
        .route("/sources/export-batch", post(export_sources_batch))
        .route(
            "/sources/import",
            post(import_sources).layer(DefaultBodyLimit::max(32 * 1024 * 1024)),
        )
        .route("/sources/{id}", patch(update_source).delete(delete_source))
        .route("/sources/{id}/variable", get(get_source_variable))
        .route("/sources/{id}/export", get(export_source))
        .route("/sources/{id}/login-action", post(run_login_action))
        .route("/sources/{id}/test", post(test_source))
        // Online search / explore / track-info / stream / lyrics / proxies
        .route("/online/search", post(online_search))
        .route("/online/explore", post(online_explore))
        .route("/online/track-info", post(online_track_info))
        .route("/online/stream", post(online_stream))
        .route("/online/lyrics", post(online_lyrics))
        .route("/online/track", post(track_online_track))
        .route("/online/track-batch", post(track_online_tracks_batch))
        .route("/tracks/{id}/online/cache", post(cache_tracked_track))
        .route("/online/cover-proxy", get(online_cover_proxy))
        .route("/online/audio-proxy", get(online_audio_proxy))
        // Per-user online/remote music favorites & playback progress
        .nest(
            "/online/items",
            xmedia_core::http::online_items::online_items_router(),
        )
}

#[derive(Debug, Deserialize)]
struct OnlineSearchRequest {
    keyword: String,
    source_ids: Option<Vec<Uuid>>,
    #[serde(default = "default_page")]
    page: u32,
    #[serde(default = "default_limit")]
    limit: u32,
}

fn default_page() -> u32 {
    1
}

fn default_limit() -> u32 {
    20
}

#[derive(Debug, Deserialize)]
struct OnlineExploreRequest {
    source_id: Uuid,
    explore_url: Option<String>,
    #[serde(default = "default_page")]
    page: u32,
    #[serde(default = "default_limit")]
    limit: u32,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct OnlineTrackInfoRequest {
    source_id: Uuid,
    track_id: Option<String>,
    detail_url: Option<String>,
    quality: Option<String>,
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
    duration_secs: Option<u32>,
    cover_url: Option<String>,
    cover_id: Option<String>,
    url_id: Option<String>,
    lyric_id: Option<String>,
    play_url: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct OnlineStreamRequest {
    source_id: Uuid,
    track_id: Option<String>,
    url_id: Option<String>,
    play_url: Option<String>,
    quality: Option<String>,
    detail_url: Option<String>,
    title: Option<String>,
    artist: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct OnlineLyricsRequest {
    source_id: Uuid,
    track_id: Option<String>,
    lyric_id: Option<String>,
    title: Option<String>,
    artist: Option<String>,
    detail_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MediaProxyQuery {
    url: String,
    source_id: Option<Uuid>,
}

async fn list_sources(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
) -> ApiResult<Json<Vec<MusicSourceSummaryDto>>> {
    let list = state.services.online.list_sources(&user).await?;
    Ok(Json(list))
}

async fn import_sources(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Json(req): Json<ImportSourcesRequest>,
) -> ApiResult<Json<ImportMusicSourcesReportDto>> {
    let report = state
        .services
        .online
        .import_sources(&user, &req.raw_json)
        .await?;
    Ok(Json(report))
}

async fn reorder_sources(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Json(req): Json<ReorderSourcesRequest>,
) -> ApiResult<Json<Vec<MusicSourceSummaryDto>>> {
    let list = state
        .services
        .online
        .reorder_sources(&user, req.ordered_ids)
        .await?;
    Ok(Json(list))
}

async fn batch_sources(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Json(req): Json<BatchMusicSourcesInput>,
) -> ApiResult<Json<BatchMusicSourcesReportDto>> {
    let report = state.services.online.batch_sources(&user, req).await?;
    Ok(Json(report))
}

async fn get_source_variable(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<SourceVariableResponse>> {
    let variable = state.services.online.get_source_variable(&user, id).await?;
    Ok(Json(SourceVariableResponse { variable }))
}

async fn export_sources_batch(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Json(req): Json<ExportSourcesBatchRequest>,
) -> ApiResult<Response> {
    let raw = state
        .services
        .online
        .export_sources_batch_json(&user, req.ids)
        .await?;
    let mut resp = Response::new(Body::from(raw));
    resp.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    resp.headers_mut()
        .insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    Ok(resp)
}

async fn update_source(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateMusicSourceInput>,
) -> ApiResult<Json<MusicSourceSummaryDto>> {
    let updated = state.services.online.update_source(&user, id, req).await?;
    Ok(Json(updated))
}

async fn delete_source(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    state.services.online.delete_source(&user, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn export_source(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Response> {
    let raw = state.services.online.export_source_json(&user, id).await?;
    let mut resp = Response::new(Body::from(raw));
    resp.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    resp.headers_mut()
        .insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    Ok(resp)
}

async fn run_login_action(
    State(state): State<MusicHttpState>,
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
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
    Json(req): Json<MusicSourceTestInput>,
) -> ApiResult<Json<MusicSourceTestReportDto>> {
    let out = state.services.online.test_source(&user, id, req).await?;
    Ok(Json(out))
}

async fn online_search(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Json(req): Json<OnlineSearchRequest>,
) -> ApiResult<Json<OnlineMusicSearchResponseDto>> {
    let out = state
        .services
        .online
        .search(&user, &req.keyword, req.source_ids, req.page, req.limit)
        .await?;
    Ok(Json(out))
}

async fn online_explore(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Json(req): Json<OnlineExploreRequest>,
) -> ApiResult<Json<OnlineMusicExploreResponseDto>> {
    let out = state
        .services
        .online
        .explore(
            &user,
            req.source_id,
            req.explore_url.as_deref(),
            req.page,
            req.limit,
        )
        .await?;
    Ok(Json(out))
}

async fn online_track_info(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Json(req): Json<OnlineTrackInfoRequest>,
) -> ApiResult<Json<OnlineMusicResolvedTrackDto>> {
    let ctx = MusicTrackContext {
        track_id: req.track_id.unwrap_or_default(),
        title: req.title.unwrap_or_default(),
        artist: req.artist.unwrap_or_default(),
        album: req.album,
        duration_secs: req.duration_secs,
        cover_url: req.cover_url,
        cover_id: req.cover_id,
        url_id: req.url_id,
        lyric_id: req.lyric_id,
        play_url: req.play_url,
        detail_url: req.detail_url,
        quality: req.quality.clone(),
    };
    let out = state
        .services
        .online
        .fetch_track_info(&user, req.source_id, ctx, req.quality)
        .await?;
    Ok(Json(out))
}

async fn online_stream(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Json(req): Json<OnlineStreamRequest>,
) -> ApiResult<Json<OnlineMusicStreamResponseDto>> {
    let ctx = MusicTrackContext {
        track_id: req.track_id.unwrap_or_default(),
        title: req.title.unwrap_or_default(),
        artist: req.artist.unwrap_or_default(),
        url_id: req.url_id,
        play_url: req.play_url,
        detail_url: req.detail_url,
        quality: req.quality.clone(),
        ..Default::default()
    };
    let out = state
        .services
        .online
        .fetch_stream(&user, req.source_id, ctx, req.quality)
        .await?;
    Ok(Json(out))
}

async fn online_lyrics(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Json(req): Json<OnlineLyricsRequest>,
) -> ApiResult<Json<StructuredLyricsDto>> {
    let ctx = MusicTrackContext {
        track_id: req.track_id.unwrap_or_default(),
        title: req.title.unwrap_or_default(),
        artist: req.artist.unwrap_or_default(),
        lyric_id: req.lyric_id,
        detail_url: req.detail_url,
        ..Default::default()
    };
    let out = state
        .services
        .online
        .fetch_lyrics(&user, req.source_id, ctx)
        .await?;
    Ok(Json(out))
}

async fn track_online_track(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Json(req): Json<TrackOnlineMusicInput>,
) -> ApiResult<Json<TrackDto>> {
    let out = state.services.online.track_online_track(&user, req).await?;
    Ok(Json(out))
}

async fn track_online_tracks_batch(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Json(req): Json<TrackOnlineMusicBatchInput>,
) -> ApiResult<Json<Vec<TrackDto>>> {
    let out = state
        .services
        .online
        .track_online_tracks_batch(&user, req.tracks)
        .await?;
    Ok(Json(out))
}

async fn cache_tracked_track(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<TrackDto>> {
    let out = state
        .services
        .online
        .cache_tracked_track(&user, TrackId(id))
        .await?;
    Ok(Json(out))
}

async fn online_cover_proxy(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Query(q): Query<MediaProxyQuery>,
) -> Result<Response, ApiError> {
    let (bytes, mime) = state
        .services
        .online
        .fetch_cover_proxy(&user, &q.url, q.source_id)
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

async fn online_audio_proxy(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Query(q): Query<MediaProxyQuery>,
    req_headers: HeaderMap,
) -> Result<Response, ApiError> {
    let proxy_resp = state
        .services
        .online
        .stream_audio_proxy(&user, &q.url, q.source_id, &req_headers)
        .await?;

    let mut resp = Response::new(Body::from_stream(proxy_resp.stream));
    *resp.status_mut() = proxy_resp.status;

    if let Ok(hv) = HeaderValue::from_str(&proxy_resp.content_type) {
        resp.headers_mut().insert(CONTENT_TYPE, hv);
    }
    if let Some(len) = proxy_resp.content_length
        && let Ok(hv) = HeaderValue::from_str(&len)
    {
        resp.headers_mut().insert(CONTENT_LENGTH, hv);
    }
    if let Some(cr) = proxy_resp.content_range
        && let Ok(hv) = HeaderValue::from_str(&cr)
    {
        resp.headers_mut().insert(CONTENT_RANGE, hv);
    }
    if let Some(ar) = proxy_resp.accept_ranges
        && let Ok(hv) = HeaderValue::from_str(&ar)
    {
        resp.headers_mut().insert(ACCEPT_RANGES, hv);
    }
    resp.headers_mut()
        .insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    resp.headers_mut().insert(
        CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("sandbox; default-src 'none'; media-src 'self'"),
    );
    resp.headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
    Ok(resp)
}

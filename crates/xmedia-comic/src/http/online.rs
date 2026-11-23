use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::header::{
    CACHE_CONTROL, CONTENT_SECURITY_POLICY, CONTENT_TYPE, X_CONTENT_TYPE_OPTIONS,
};
use axum::http::{HeaderValue, StatusCode};
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

use crate::application::dto::{ChapterItemDto, SeriesDetailDto};
use crate::application::online::{
    BatchComicSourcesInput, BatchComicSourcesReportDto, ComicCacheReportDto, ComicSourceSummaryDto,
    ComicSourceTestInput, ComicSourceTestReportDto, ImportComicSourcesReportDto,
    LoginActionResponseDto, OnlineComicChaptersResponseDto, OnlineComicDetailResponseDto,
    OnlineComicExploreResponseDto, OnlineComicPagesResponseDto, OnlineComicSearchResponseDto,
    TrackOnlineComicInput, UpdateComicSourceInput,
};
use crate::domain::id::{ChapterId, SeriesId};
use crate::http::state::ComicHttpState;
use crate::source::model::{ComicChapterJsContext, ComicJsContext};

pub fn router() -> Router<ComicHttpState> {
    Router::new()
        // Comic source management
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
        // Online comic search / explore / detail / chapters / pages / image-proxy
        .route("/online/search", post(online_search))
        .route("/online/explore", post(online_explore))
        .route("/online/detail", post(online_detail))
        .route("/online/chapters", post(online_chapters))
        .route("/online/pages", post(online_pages))
        .route("/online/image-proxy", get(online_image_proxy))
        // Online-tracked comic series (hybrid mode: track, refresh TOC, offline cache CBZ)
        .route("/online/track", post(track_online_comic))
        .route("/series/{id}/online/refresh", post(refresh_tracked_series))
        .route("/series/{id}/online/cache", post(cache_tracked_series))
        .route("/chapters/{id}/online/cache", post(cache_tracked_chapter))
        // Per-user online comic favorites & reading progress
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
}

fn default_page() -> u32 {
    1
}

#[derive(Debug, Deserialize)]
struct OnlineExploreRequest {
    source_id: Uuid,
    explore_url: Option<String>,
    #[serde(default = "default_page")]
    page: u32,
}

#[derive(Debug, Deserialize)]
struct OnlineDetailRequest {
    source_id: Uuid,
    #[serde(alias = "book_url")]
    comic_url: String,
    fallback_hint: Option<ComicJsContext>,
}

#[derive(Debug, Deserialize)]
struct OnlineChaptersRequest {
    source_id: Uuid,
    toc_url: String,
    #[serde(default, alias = "book")]
    comic: ComicJsContext,
}

#[derive(Debug, Deserialize)]
struct OnlinePagesRequest {
    source_id: Uuid,
    chapter_url: String,
    comic_url: Option<String>,
    #[serde(default, alias = "book")]
    comic: ComicJsContext,
    #[serde(default)]
    chapter: ComicChapterJsContext,
}

#[derive(Debug, Deserialize)]
struct ImageProxyQuery {
    url: String,
    source_id: Option<Uuid>,
}

async fn list_sources(
    State(state): State<ComicHttpState>,
    WebAuth(user): WebAuth,
) -> ApiResult<Json<Vec<ComicSourceSummaryDto>>> {
    let list = state.services.online.list_sources(&user).await?;
    Ok(Json(list))
}

async fn import_sources(
    State(state): State<ComicHttpState>,
    WebAuth(user): WebAuth,
    Json(req): Json<ImportSourcesRequest>,
) -> ApiResult<Json<ImportComicSourcesReportDto>> {
    let report = state
        .services
        .online
        .import_sources(&user, &req.raw_json)
        .await?;
    Ok(Json(report))
}

async fn reorder_sources(
    State(state): State<ComicHttpState>,
    WebAuth(user): WebAuth,
    Json(req): Json<ReorderSourcesRequest>,
) -> ApiResult<Json<Vec<ComicSourceSummaryDto>>> {
    let list = state
        .services
        .online
        .reorder_sources(&user, req.ordered_ids)
        .await?;
    Ok(Json(list))
}

async fn batch_sources(
    State(state): State<ComicHttpState>,
    WebAuth(user): WebAuth,
    Json(req): Json<BatchComicSourcesInput>,
) -> ApiResult<Json<BatchComicSourcesReportDto>> {
    let report = state.services.online.batch_sources(&user, req).await?;
    Ok(Json(report))
}

async fn get_source_variable(
    State(state): State<ComicHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<SourceVariableResponse>> {
    let variable = state.services.online.get_source_variable(&user, id).await?;
    Ok(Json(SourceVariableResponse { variable }))
}

async fn export_sources_batch(
    State(state): State<ComicHttpState>,
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
    State(state): State<ComicHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateComicSourceInput>,
) -> ApiResult<Json<ComicSourceSummaryDto>> {
    let updated = state.services.online.update_source(&user, id, req).await?;
    Ok(Json(updated))
}

async fn delete_source(
    State(state): State<ComicHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    state.services.online.delete_source(&user, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn export_source(
    State(state): State<ComicHttpState>,
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
    State(state): State<ComicHttpState>,
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
    State(state): State<ComicHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
    Json(req): Json<ComicSourceTestInput>,
) -> ApiResult<Json<ComicSourceTestReportDto>> {
    let out = state.services.online.test_source(&user, id, req).await?;
    Ok(Json(out))
}

async fn online_search(
    State(state): State<ComicHttpState>,
    WebAuth(user): WebAuth,
    Json(req): Json<OnlineSearchRequest>,
) -> ApiResult<Json<OnlineComicSearchResponseDto>> {
    let out = state
        .services
        .online
        .search(&user, &req.keyword, req.source_ids, req.page)
        .await?;
    Ok(Json(out))
}

async fn online_explore(
    State(state): State<ComicHttpState>,
    WebAuth(user): WebAuth,
    Json(req): Json<OnlineExploreRequest>,
) -> ApiResult<Json<OnlineComicExploreResponseDto>> {
    let out = state
        .services
        .online
        .explore(&user, req.source_id, req.explore_url.as_deref(), req.page)
        .await?;
    Ok(Json(out))
}

async fn online_detail(
    State(state): State<ComicHttpState>,
    WebAuth(user): WebAuth,
    Json(req): Json<OnlineDetailRequest>,
) -> ApiResult<Json<OnlineComicDetailResponseDto>> {
    let out = state
        .services
        .online
        .fetch_detail(&user, req.source_id, &req.comic_url, req.fallback_hint)
        .await?;
    Ok(Json(out))
}

async fn online_chapters(
    State(state): State<ComicHttpState>,
    WebAuth(user): WebAuth,
    Json(req): Json<OnlineChaptersRequest>,
) -> ApiResult<Json<OnlineComicChaptersResponseDto>> {
    let out = state
        .services
        .online
        .fetch_chapters(&user, req.source_id, &req.toc_url, req.comic)
        .await?;
    Ok(Json(out))
}

async fn online_pages(
    State(state): State<ComicHttpState>,
    WebAuth(user): WebAuth,
    Json(mut req): Json<OnlinePagesRequest>,
) -> ApiResult<Json<OnlineComicPagesResponseDto>> {
    if req.comic.book_url.is_empty()
        && let Some(ref c_url) = req.comic_url
    {
        req.comic.book_url = c_url.clone();
    }
    let out = state
        .services
        .online
        .fetch_pages(
            &user,
            req.source_id,
            &req.chapter_url,
            req.comic,
            req.chapter,
        )
        .await?;
    Ok(Json(out))
}

async fn online_image_proxy(
    State(state): State<ComicHttpState>,
    WebAuth(user): WebAuth,
    Query(q): Query<ImageProxyQuery>,
) -> Result<Response, ApiError> {
    let (bytes, mime) = state
        .services
        .online
        .fetch_image_proxy(&user, q.source_id, &q.url)
        .await?;
    let mut resp = Response::new(Body::from(bytes));
    if let Ok(hv) = HeaderValue::from_str(&mime) {
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

#[derive(Debug, Default, Deserialize)]
struct CacheTrackedSeriesRequest {
    #[serde(default)]
    chapter_ids: Option<Vec<Uuid>>,
}

async fn track_online_comic(
    State(state): State<ComicHttpState>,
    WebAuth(user): WebAuth,
    Json(input): Json<TrackOnlineComicInput>,
) -> ApiResult<Json<SeriesDetailDto>> {
    let detail = state
        .services
        .online
        .track_online_comic(&user, input)
        .await?;
    Ok(Json(detail))
}

async fn refresh_tracked_series(
    State(state): State<ComicHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<SeriesDetailDto>> {
    let detail = state
        .services
        .online
        .refresh_tracked_series(&user, SeriesId(id))
        .await?;
    Ok(Json(detail))
}

async fn cache_tracked_series(
    State(state): State<ComicHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
    payload: Option<Json<CacheTrackedSeriesRequest>>,
) -> ApiResult<Json<ComicCacheReportDto>> {
    let chapter_ids = payload
        .and_then(|Json(req)| req.chapter_ids)
        .map(|ids| ids.into_iter().map(ChapterId).collect());
    let report = state
        .services
        .online
        .cache_tracked_series(&user, SeriesId(id), chapter_ids)
        .await?;
    Ok(Json(report))
}

async fn cache_tracked_chapter(
    State(state): State<ComicHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<ChapterItemDto>> {
    let ch = state
        .services
        .online
        .cache_tracked_chapter(&user, ChapterId(id))
        .await?;
    Ok(Json(ch))
}

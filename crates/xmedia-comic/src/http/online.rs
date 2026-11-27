use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::HeaderValue;
use axum::http::header::{
    CACHE_CONTROL, CONTENT_SECURITY_POLICY, CONTENT_TYPE, X_CONTENT_TYPE_OPTIONS,
};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use uuid::Uuid;
use xmedia_core::domain::error::DomainError;
use xmedia_core::http::{ApiError, ApiResult, WebAuth};
use xmedia_source::crud::{CrudPluginDomainHandler, PluginDomainHandler};
use xmedia_source::proxy::{IMAGE_PROXY_CACHE_CONTROL, IMAGE_PROXY_CSP};

use crate::application::dto::{ChapterItemDto, SeriesDetailDto};
use crate::application::online::{
    ComicCacheReportDto, ComicOnlineService, ComicSourceTestInput, OnlineComicChaptersResponseDto,
    OnlineComicDetailResponseDto, OnlineComicExploreResponseDto, OnlineComicPagesResponseDto,
    OnlineComicSearchResponseDto, TrackOnlineComicInput,
};
use crate::domain::id::{ChapterId, SeriesId};
use crate::domain::online::{ComicChapterJsContext, ComicJsContext};
use crate::http::state::ComicHttpState;

pub fn plugin_handler(online: Arc<ComicOnlineService>) -> Arc<dyn PluginDomainHandler> {
    let crud = online.source_crud().clone();
    let online_login = online.clone();
    let online_test = online;
    CrudPluginDomainHandler::into_arc(
        crud,
        move |user, id, req| {
            let svc = online_login.clone();
            async move {
                svc.run_login_action(&user, id, req.login_info, req.action)
                    .await
            }
        },
        move |user, id, payload| {
            let svc = online_test.clone();
            async move {
                let input: ComicSourceTestInput = serde_json::from_value(payload)
                    .map_err(|e| DomainError::Validation(format!("漫画插件测试参数无效: {e}")))?;
                let out = svc.test_source(&user, id, input).await?;
                serde_json::to_value(out).map_err(|e| DomainError::Storage(e.to_string()))
            }
        },
    )
}

pub fn router() -> Router<ComicHttpState> {
    Router::new()
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
    comic_url: String,
    fallback_hint: Option<ComicJsContext>,
}

#[derive(Debug, Deserialize)]
struct OnlineChaptersRequest {
    source_id: Uuid,
    toc_url: String,
    #[serde(default)]
    comic: ComicJsContext,
}

#[derive(Debug, Deserialize)]
struct OnlinePagesRequest {
    source_id: Uuid,
    chapter_url: String,
    comic_url: Option<String>,
    #[serde(default)]
    comic: ComicJsContext,
    #[serde(default)]
    chapter: ComicChapterJsContext,
}

#[derive(Debug, Deserialize)]
struct ImageProxyQuery {
    url: String,
    source_id: Option<Uuid>,
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

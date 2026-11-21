use std::collections::HashMap;

use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::header::{
    CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE, X_CONTENT_TYPE_OPTIONS,
};
use axum::http::{HeaderValue, StatusCode};
use axum::response::Response;
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use serde::Deserialize;
use uuid::Uuid;
use xmedia_core::http::{ApiError, ApiResult, WebAuth, sanitize_download_filename};

use crate::application::dto::BookSummaryDto;
use crate::application::online::{
    AddOnlineShelfInput, AggregatedSearchResponseDto, BookSourceSummaryDto,
    ImportBookSourcesReportDto, LoginActionResponseDto, OnlineBookDetailResponseDto,
    OnlineContentResponseDto, OnlineDownloadJobDto, OnlineShelfBookDto, OnlineTocResponseDto,
    SourceTestInput, SourceTestReportDto, StartDownloadInput, SwitchTrackedSourceInput,
    TrackOnlineBookInput, TrackedChapterResponseDto, TrackedTocResponseDto, UpdateBookSourceInput,
    UpdateOnlineShelfInput,
};
use crate::domain::book::BookId;
use crate::http::state::BookHttpState;
use crate::source::model::{BookJsContext, ChapterJsContext};

pub fn router() -> Router<BookHttpState> {
    Router::new()
        // Book source management
        .route("/sources", get(list_sources))
        .route(
            "/sources/import",
            post(import_sources).layer(DefaultBodyLimit::max(32 * 1024 * 1024)),
        )
        .route("/sources/{id}", patch(update_source).delete(delete_source))
        .route("/sources/{id}/export", get(export_source))
        .route("/sources/{id}/login-action", post(run_login_action))
        .route("/sources/{id}/test", post(test_source))
        // Online search / detail / TOC / content / cover proxy
        .route("/online/search", post(online_search))
        .route("/online/book-info", post(online_book_info))
        .route("/online/toc", post(online_toc))
        .route("/online/content", post(online_content))
        .route("/online/cover-proxy", get(online_cover_proxy))
        // Unified Online-Tracked Books (BookModel + Incremental Chapters + EPUB 3.0)
        .route("/online/track", post(track_online_book))
        .route("/books/{id}/online/toc", get(get_tracked_toc))
        .route(
            "/books/{id}/online/chapter/{index}",
            get(get_tracked_chapter),
        )
        .route(
            "/books/{id}/online/switch-source",
            post(switch_tracked_source),
        )
        .route("/books/{id}/online/sync-epub", post(sync_tracked_epub))
        // Per-user online bookshelf (legacy compatibility)
        .route("/online/shelf", get(list_shelf).post(add_to_shelf))
        .route(
            "/online/shelf/{id}",
            patch(update_shelf_book).delete(remove_from_shelf),
        )
        // Background downloads & EPUB/TXT packaging
        .route(
            "/online/downloads",
            get(list_downloads).post(start_download),
        )
        .route(
            "/online/downloads/{id}",
            get(get_download).delete(delete_download),
        )
        .route("/online/downloads/{id}/cancel", post(cancel_download))
        .route("/online/downloads/{id}/file", get(download_packaged_file))
}

#[derive(Debug, Deserialize)]
struct ImportSourcesRequest {
    raw_json: String,
}

#[derive(Debug, Deserialize)]
struct LoginActionRequest {
    #[serde(default)]
    login_info: HashMap<String, String>,
    action: Option<String>,
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
struct OnlineBookInfoRequest {
    source_id: Uuid,
    book_url: String,
    fallback_hint: Option<BookJsContext>,
}

#[derive(Debug, Deserialize)]
struct OnlineTocRequest {
    source_id: Uuid,
    toc_url: String,
    #[serde(default)]
    book: BookJsContext,
}

#[derive(Debug, Deserialize)]
struct OnlineContentRequest {
    source_id: Uuid,
    chapter_url: String,
    #[serde(default)]
    book: BookJsContext,
    #[serde(default)]
    chapter: ChapterJsContext,
    #[serde(default)]
    force_refresh: bool,
}

#[derive(Debug, Deserialize)]
struct CoverProxyQuery {
    url: String,
}

async fn list_sources(
    State(state): State<BookHttpState>,
    WebAuth(user): WebAuth,
) -> ApiResult<Json<Vec<BookSourceSummaryDto>>> {
    let list = state.services.online.list_sources(&user).await?;
    Ok(Json(list))
}

async fn import_sources(
    State(state): State<BookHttpState>,
    WebAuth(user): WebAuth,
    Json(req): Json<ImportSourcesRequest>,
) -> ApiResult<Json<ImportBookSourcesReportDto>> {
    let report = state
        .services
        .online
        .import_sources(&user, &req.raw_json)
        .await?;
    Ok(Json(report))
}

async fn update_source(
    State(state): State<BookHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateBookSourceInput>,
) -> ApiResult<Json<BookSourceSummaryDto>> {
    let updated = state.services.online.update_source(&user, id, req).await?;
    Ok(Json(updated))
}

async fn delete_source(
    State(state): State<BookHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    state.services.online.delete_source(&user, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn export_source(
    State(state): State<BookHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Response> {
    let raw = state.services.online.export_source_json(&user, id).await?;
    let mut resp = Response::new(Body::from(raw));
    resp.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    Ok(resp)
}

async fn run_login_action(
    State(state): State<BookHttpState>,
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
    State(state): State<BookHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
    Json(req): Json<SourceTestInput>,
) -> ApiResult<Json<SourceTestReportDto>> {
    let out = state.services.online.test_source(&user, id, req).await?;
    Ok(Json(out))
}

async fn online_search(
    State(state): State<BookHttpState>,
    WebAuth(_user): WebAuth,
    Json(req): Json<OnlineSearchRequest>,
) -> ApiResult<Json<AggregatedSearchResponseDto>> {
    let out = state
        .services
        .online
        .search(&req.keyword, req.source_ids, req.page)
        .await?;
    Ok(Json(out))
}

async fn online_book_info(
    State(state): State<BookHttpState>,
    WebAuth(_user): WebAuth,
    Json(req): Json<OnlineBookInfoRequest>,
) -> ApiResult<Json<OnlineBookDetailResponseDto>> {
    let out = state
        .services
        .online
        .fetch_book_info(req.source_id, &req.book_url, req.fallback_hint)
        .await?;
    Ok(Json(out))
}

async fn online_toc(
    State(state): State<BookHttpState>,
    WebAuth(_user): WebAuth,
    Json(req): Json<OnlineTocRequest>,
) -> ApiResult<Json<OnlineTocResponseDto>> {
    let out = state
        .services
        .online
        .fetch_toc(req.source_id, &req.toc_url, req.book)
        .await?;
    Ok(Json(out))
}

async fn online_content(
    State(state): State<BookHttpState>,
    WebAuth(_user): WebAuth,
    Json(req): Json<OnlineContentRequest>,
) -> ApiResult<Json<OnlineContentResponseDto>> {
    let out = state
        .services
        .online
        .fetch_chapter_content(
            req.source_id,
            &req.chapter_url,
            req.book,
            req.chapter,
            req.force_refresh,
        )
        .await?;
    Ok(Json(out))
}

async fn online_cover_proxy(
    State(state): State<BookHttpState>,
    WebAuth(_user): WebAuth,
    Query(q): Query<CoverProxyQuery>,
) -> Result<Response, ApiError> {
    let (bytes, mime) = state.services.online.fetch_cover_proxy(&q.url).await?;
    let mut resp = Response::new(Body::from(bytes));
    if let Ok(hv) = HeaderValue::from_str(&mime) {
        resp.headers_mut().insert(CONTENT_TYPE, hv);
    }
    resp.headers_mut()
        .insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    resp.headers_mut().insert(
        CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=86400"),
    );
    Ok(resp)
}

async fn list_shelf(
    State(state): State<BookHttpState>,
    WebAuth(user): WebAuth,
) -> ApiResult<Json<Vec<OnlineShelfBookDto>>> {
    let list = state.services.online.list_shelf(user.id).await?;
    Ok(Json(list))
}

async fn add_to_shelf(
    State(state): State<BookHttpState>,
    WebAuth(user): WebAuth,
    Json(req): Json<AddOnlineShelfInput>,
) -> ApiResult<Json<OnlineShelfBookDto>> {
    let item = state.services.online.add_to_shelf(user.id, req).await?;
    Ok(Json(item))
}

async fn update_shelf_book(
    State(state): State<BookHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateOnlineShelfInput>,
) -> ApiResult<Json<OnlineShelfBookDto>> {
    let item = state
        .services
        .online
        .update_shelf_book(user.id, id, req)
        .await?;
    Ok(Json(item))
}

async fn remove_from_shelf(
    State(state): State<BookHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    state.services.online.remove_from_shelf(user.id, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_downloads(
    State(state): State<BookHttpState>,
    WebAuth(user): WebAuth,
) -> ApiResult<Json<Vec<OnlineDownloadJobDto>>> {
    Ok(Json(state.services.online.list_downloads(user.id).await))
}

async fn start_download(
    State(state): State<BookHttpState>,
    WebAuth(user): WebAuth,
    Json(req): Json<StartDownloadInput>,
) -> ApiResult<Json<OnlineDownloadJobDto>> {
    let job = state.services.online.start_download(&user, req).await?;
    Ok(Json(job))
}

async fn get_download(
    State(state): State<BookHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<OnlineDownloadJobDto>> {
    let job = state.services.online.get_download(user.id, id).await?;
    Ok(Json(job))
}

async fn cancel_download(
    State(state): State<BookHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<OnlineDownloadJobDto>> {
    let job = state.services.online.cancel_download(user.id, id).await?;
    Ok(Json(job))
}

async fn delete_download(
    State(state): State<BookHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    state.services.online.delete_download(user.id, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn download_packaged_file(
    State(state): State<BookHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    let (file_name, mime, range_stream) = state
        .services
        .online
        .open_download_file_stream(user.id, id)
        .await?;
    let (stem, ext) = file_name
        .rsplit_once('.')
        .unwrap_or((file_name.as_str(), "epub"));
    let safe_name = sanitize_download_filename(stem, ext);
    let mut resp = Response::new(Body::from_stream(range_stream.stream));
    resp.headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static(mime));
    if let Ok(len_val) = HeaderValue::from_str(&range_stream.total_size.to_string()) {
        resp.headers_mut().insert(CONTENT_LENGTH, len_val);
    }
    if let Ok(cd) = HeaderValue::from_str(&format!(
        "attachment; filename=\"{safe_name}\"; filename*=UTF-8''{}",
        form_urlencoded::byte_serialize(file_name.as_bytes()).collect::<String>()
    )) {
        resp.headers_mut().insert(CONTENT_DISPOSITION, cd);
    }
    Ok(resp)
}

#[derive(Debug, Default, Deserialize)]
struct RefreshQuery {
    #[serde(default)]
    refresh: bool,
}

async fn track_online_book(
    State(state): State<BookHttpState>,
    WebAuth(user): WebAuth,
    Json(req): Json<TrackOnlineBookInput>,
) -> ApiResult<Json<BookSummaryDto>> {
    let book = state.services.online.track_online_book(&user, req).await?;
    Ok(Json(book))
}

async fn get_tracked_toc(
    State(state): State<BookHttpState>,
    WebAuth(_user): WebAuth,
    Path(id): Path<Uuid>,
    Query(q): Query<RefreshQuery>,
) -> ApiResult<Json<TrackedTocResponseDto>> {
    let toc = state
        .services
        .online
        .get_tracked_toc(BookId(id), q.refresh)
        .await?;
    Ok(Json(toc))
}

async fn get_tracked_chapter(
    State(state): State<BookHttpState>,
    WebAuth(_user): WebAuth,
    Path((id, index)): Path<(Uuid, usize)>,
    Query(q): Query<RefreshQuery>,
) -> ApiResult<Json<TrackedChapterResponseDto>> {
    let ch = state
        .services
        .online
        .get_tracked_chapter(BookId(id), index, q.refresh)
        .await?;
    Ok(Json(ch))
}

async fn switch_tracked_source(
    State(state): State<BookHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
    Json(req): Json<SwitchTrackedSourceInput>,
) -> ApiResult<Json<TrackedTocResponseDto>> {
    let toc = state
        .services
        .online
        .switch_tracked_source(&user, BookId(id), req)
        .await?;
    Ok(Json(toc))
}

async fn sync_tracked_epub(
    State(state): State<BookHttpState>,
    WebAuth(_user): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<BookSummaryDto>> {
    let book = state
        .services
        .online
        .sync_tracked_epub(BookId(id), true)
        .await?;
    Ok(Json(book))
}

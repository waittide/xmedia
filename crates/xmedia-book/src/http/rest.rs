use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Multipart, Path, Query, State};
use axum::http::header::{
    CACHE_CONTROL, CONTENT_SECURITY_POLICY, CONTENT_TYPE, X_CONTENT_TYPE_OPTIONS,
};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use xmedia_core::domain::error::DomainError;
use xmedia_core::domain::library::LibraryId;
use xmedia_core::domain::user::require_admin;
use xmedia_core::http::{
    ApiError, ApiResult, WebAuth, into_range_http_response, parse_http_range_spec,
    sanitize_download_filename, stream_multipart_field_to_temp_file,
};

use crate::application::dto::{
    BookBookmarkItemDto, BookHomeOverviewDto, BookSummaryDto, CatalogSearchFilter, ShelfDto,
    UpdateMetadataPatch,
};
use crate::application::ports::{TocItem, TxtChapterContent};
use crate::application::reading::{CreateBookmarkCommand, SaveProgressCommand};
use crate::domain::book::{
    BookFormat, BookId, BookSortField, FacetCount, PagedResult, SortOrder, TagMatchMode,
};
use crate::domain::reading::{Bookmark, BookmarkId, ReadingProgress, ShelfId};
use crate::http::state::BookHttpState;

pub fn router() -> Router<BookHttpState> {
    Router::new()
        .route("/home", get(home_overview))
        .route("/books", get(search_books))
        .route(
            "/books/upload",
            post(upload_books).layer(DefaultBodyLimit::max(512 * 1024 * 1024)),
        )
        .route("/books/batch", post(batch_books_action))
        .route(
            "/books/{id}",
            get(get_book).patch(update_book).delete(delete_book),
        )
        .route(
            "/books/{id}/cover",
            get(get_book_cover)
                .put(replace_book_cover)
                .layer(DefaultBodyLimit::max(16 * 1024 * 1024)),
        )
        .route("/books/{id}/file", get(stream_book_file))
        .route("/books/{id}/toc", get(get_book_toc))
        .route("/books/{id}/chapters/{index}", get(get_txt_chapter))
        .route("/books/{id}/resources/{*path}", get(get_epub_resource))
        .route("/authors", get(list_authors))
        .route("/tags", get(list_tags))
        .route("/recent", get(list_recent_books))
        .route("/books/{id}/progress", get(get_progress).put(save_progress))
        .route("/books/{id}/favorite", axum::routing::put(set_favorite))
        .route(
            "/books/{id}/bookmarks",
            get(list_bookmarks).post(create_bookmark),
        )
        .route("/bookmarks", get(list_all_bookmarks))
        .route("/bookmarks/{bookmark_id}", delete(delete_bookmark))
        .route("/shelves", get(list_shelves).post(create_shelf))
        .route(
            "/shelves/{shelf_id}",
            patch(rename_shelf).delete(delete_shelf),
        )
        .route(
            "/shelves/{shelf_id}/books/{book_id}",
            post(add_book_to_shelf).delete(remove_book_from_shelf),
        )
}

#[derive(Debug, Default, Deserialize)]
struct SearchBooksQuery {
    keyword: Option<String>,
    author: Option<String>,
    authors: Option<String>,
    tag: Option<String>,
    tags: Option<String>,
    tag_mode: Option<TagMatchMode>,
    format: Option<BookFormat>,
    formats: Option<String>,
    library_id: Option<Uuid>,
    library_ids: Option<String>,
    favorites_only: Option<bool>,
    shelf_id: Option<Uuid>,
    include_missing: Option<bool>,
    sort_by: Option<BookSortField>,
    sort_order: Option<SortOrder>,
    page: Option<u32>,
    page_size: Option<u32>,
}

fn split_multi_param(raw: Option<String>) -> Vec<String> {
    let Some(text) = raw else {
        return Vec::new();
    };
    if let Ok(arr) = serde_json::from_str::<Vec<String>>(&text) {
        return arr
            .into_iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
    }
    text.split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(ToString::to_string)
        .collect()
}

async fn search_books(
    State(state): State<BookHttpState>,
    WebAuth(actor): WebAuth,
    Query(q): Query<SearchBooksQuery>,
) -> ApiResult<Json<PagedResult<BookSummaryDto>>> {
    let parsed_formats: Vec<BookFormat> = split_multi_param(q.formats)
        .into_iter()
        .filter_map(|f| BookFormat::from_extension(&f))
        .collect();

    let parsed_lib_ids: Vec<LibraryId> = split_multi_param(q.library_ids)
        .into_iter()
        .filter_map(|id| id.parse::<Uuid>().ok().map(LibraryId))
        .collect();

    let filter = CatalogSearchFilter {
        keyword: q.keyword,
        author: q.author,
        authors: split_multi_param(q.authors),
        tag: q.tag,
        tags: split_multi_param(q.tags),
        tag_mode: q.tag_mode.unwrap_or_default(),
        format: q.format,
        formats: parsed_formats,
        library_id: q.library_id.map(LibraryId),
        library_ids: parsed_lib_ids,
        favorites_only: q.favorites_only.unwrap_or(false),
        shelf_id: q.shelf_id.map(ShelfId),
        include_missing: q.include_missing.unwrap_or(false),
        sort_by: q.sort_by,
        sort_order: q.sort_order,
        page: q.page,
        page_size: q.page_size,
    };
    let page = state
        .services
        .catalog
        .search_books(Some(&actor), filter)
        .await?;
    Ok(Json(page))
}

#[derive(Serialize)]
struct UploadBooksResponse {
    uploaded: Vec<BookSummaryDto>,
}

const MAX_BOOK_UPLOAD_BYTES: u64 = 512 * 1024 * 1024;

async fn upload_books(
    State(state): State<BookHttpState>,
    WebAuth(actor): WebAuth,
    mut multipart: Multipart,
) -> ApiResult<(StatusCode, Json<UploadBooksResponse>)> {
    require_admin(&actor)?;
    let mut uploaded = Vec::new();
    let mut total_bytes: u64 = 0;

    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError(DomainError::Validation(format!("multipart error: {e}"))))?
    {
        let Some(file_name) = field.file_name().map(ToString::to_string) else {
            continue;
        };
        let ext = std::path::Path::new(&file_name)
            .extension()
            .and_then(|s| s.to_str())
            .ok_or_else(|| ApiError(DomainError::Validation("missing file extension".into())))?;
        let _ = BookFormat::parse(ext)?;
        let remaining_cap = MAX_BOOK_UPLOAD_BYTES.saturating_sub(total_bytes);
        let temp =
            stream_multipart_field_to_temp_file(&mut field, remaining_cap, Some(ext)).await?;
        if temp.is_empty() {
            continue;
        }
        total_bytes = total_bytes.saturating_add(temp.size());
        let book = state
            .services
            .catalog
            .upload_book_from_path(&actor, &file_name, temp.path())
            .await?;
        uploaded.push(book);
    }

    if uploaded.is_empty() {
        return Err(ApiError(DomainError::Validation(
            "no valid book files received in multipart payload".into(),
        )));
    }

    Ok((StatusCode::CREATED, Json(UploadBooksResponse { uploaded })))
}

#[derive(Deserialize)]
struct BatchBooksRequest {
    book_ids: Vec<BookId>,
    #[serde(default)]
    add_tags: Vec<String>,
    #[serde(default)]
    remove_tags: Vec<String>,
    set_favorite: Option<bool>,
}

#[derive(Serialize)]
struct BatchBooksResponse {
    affected: usize,
}

async fn batch_books_action(
    State(state): State<BookHttpState>,
    WebAuth(actor): WebAuth,
    Json(req): Json<BatchBooksRequest>,
) -> ApiResult<Json<BatchBooksResponse>> {
    let mut affected = 0usize;
    if !req.add_tags.is_empty() || !req.remove_tags.is_empty() {
        affected = affected.max(
            state
                .services
                .catalog
                .batch_update_tags(&actor, req.book_ids.clone(), req.add_tags, req.remove_tags)
                .await?,
        );
    }
    if let Some(fav) = req.set_favorite {
        affected = affected.max(
            state
                .services
                .reading
                .batch_set_favorite(&actor, req.book_ids, fav)
                .await?,
        );
    }
    Ok(Json(BatchBooksResponse { affected }))
}

async fn get_book(
    State(state): State<BookHttpState>,
    WebAuth(actor): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<BookSummaryDto>> {
    Ok(Json(
        state
            .services
            .catalog
            .get_book(Some(&actor), BookId(id))
            .await?,
    ))
}

async fn update_book(
    State(state): State<BookHttpState>,
    WebAuth(actor): WebAuth,
    Path(id): Path<Uuid>,
    Json(patch): Json<UpdateMetadataPatch>,
) -> ApiResult<Json<BookSummaryDto>> {
    Ok(Json(
        state
            .services
            .catalog
            .update_metadata(&actor, BookId(id), patch)
            .await?,
    ))
}

async fn delete_book(
    State(state): State<BookHttpState>,
    WebAuth(actor): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    state
        .services
        .catalog
        .delete_book(&actor, BookId(id))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn get_book_cover(
    State(state): State<BookHttpState>,
    WebAuth(_actor): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Response> {
    let cover = state.services.catalog.get_cover(BookId(id)).await?;
    let mut res = (StatusCode::OK, Body::from(cover.bytes)).into_response();
    if let Ok(ct) = HeaderValue::from_str(&cover.mime_type) {
        res.headers_mut().insert(CONTENT_TYPE, ct);
    }
    res.headers_mut()
        .insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    res.headers_mut().insert(
        CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("sandbox; default-src 'none'; img-src 'self' data:"),
    );
    res.headers_mut().insert(
        CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=600"),
    );
    Ok(res)
}

async fn replace_book_cover(
    State(state): State<BookHttpState>,
    WebAuth(actor): WebAuth,
    Path(id): Path<Uuid>,
    mut multipart: Multipart,
) -> ApiResult<Json<BookSummaryDto>> {
    require_admin(&actor)?;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError(DomainError::Validation(format!("multipart error: {e}"))))?
    {
        let mime_type = field.content_type().unwrap_or("image/jpeg").to_string();
        let bytes: Bytes = field
            .bytes()
            .await
            .map_err(|e| ApiError(DomainError::Validation(format!("read cover error: {e}"))))?;
        if !bytes.is_empty() {
            let updated = state
                .services
                .catalog
                .replace_cover(&actor, BookId(id), bytes, mime_type)
                .await?;
            return Ok(Json(updated));
        }
    }
    Err(ApiError(DomainError::Validation(
        "missing cover image in multipart payload".into(),
    )))
}

#[derive(Default, Deserialize)]
struct DownloadQuery {
    download: Option<bool>,
}

pub(crate) async fn serve_book_file_http(
    state: &BookHttpState,
    book_id: BookId,
    headers: &HeaderMap,
    as_attachment: bool,
) -> ApiResult<Response> {
    // If this is an online-tracked book with newly cached chapters, atomically flush
    // its standard EPUB 3.0 archive before streaming (transparent for both Web and OPDS).
    state
        .services
        .online
        .ensure_tracked_epub_synced(book_id)
        .await?;

    let spec = parse_http_range_spec(headers);
    let (book, range_stream) = state
        .services
        .catalog
        .open_book_file_stream(book_id, spec)
        .await?;
    let filename = sanitize_download_filename(&book.metadata.title, book.format.as_str());
    Ok(into_range_http_response(
        range_stream,
        spec.is_partial(),
        book.format.mime_type(),
        Some(&filename),
        as_attachment,
        None,
    ))
}

async fn stream_book_file(
    State(state): State<BookHttpState>,
    WebAuth(_actor): WebAuth,
    Path(id): Path<Uuid>,
    Query(q): Query<DownloadQuery>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    serve_book_file_http(&state, BookId(id), &headers, q.download.unwrap_or(false)).await
}

async fn get_book_toc(
    State(state): State<BookHttpState>,
    WebAuth(_actor): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<TocItem>>> {
    Ok(Json(state.services.catalog.get_toc(BookId(id)).await?))
}

async fn get_txt_chapter(
    State(state): State<BookHttpState>,
    WebAuth(_actor): WebAuth,
    Path((id, index)): Path<(Uuid, u32)>,
) -> ApiResult<Json<TxtChapterContent>> {
    Ok(Json(
        state
            .services
            .catalog
            .get_txt_chapter(BookId(id), index)
            .await?,
    ))
}

async fn get_epub_resource(
    State(state): State<BookHttpState>,
    WebAuth(_actor): WebAuth,
    Path((id, path)): Path<(Uuid, String)>,
) -> ApiResult<Response> {
    let asset = state
        .services
        .catalog
        .get_epub_resource(BookId(id), &path)
        .await?;
    let mut res = (StatusCode::OK, Body::from(asset.bytes)).into_response();
    if let Ok(ct) = HeaderValue::from_str(&asset.mime_type) {
        res.headers_mut().insert(CONTENT_TYPE, ct);
    }
    res.headers_mut()
        .insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    res.headers_mut().insert(
        CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "sandbox; default-src 'none'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; font-src 'self' data:",
        ),
    );
    res.headers_mut().insert(
        CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=3600"),
    );
    Ok(res)
}

async fn list_authors(
    State(state): State<BookHttpState>,
    WebAuth(_actor): WebAuth,
) -> ApiResult<Json<Vec<FacetCount>>> {
    Ok(Json(state.services.catalog.list_authors().await?))
}

async fn list_tags(
    State(state): State<BookHttpState>,
    WebAuth(_actor): WebAuth,
) -> ApiResult<Json<Vec<FacetCount>>> {
    Ok(Json(state.services.catalog.list_tags().await?))
}

#[derive(Default, Deserialize)]
struct LimitQuery {
    limit: Option<u32>,
}

async fn list_recent_books(
    State(state): State<BookHttpState>,
    WebAuth(actor): WebAuth,
    Query(q): Query<LimitQuery>,
) -> ApiResult<Json<Vec<BookSummaryDto>>> {
    let items = state
        .services
        .reading
        .list_recent_books(&actor, q.limit.unwrap_or(20), &state.services.catalog)
        .await?;
    Ok(Json(items))
}

async fn get_progress(
    State(state): State<BookHttpState>,
    WebAuth(actor): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Option<ReadingProgress>>> {
    Ok(Json(
        state
            .services
            .reading
            .get_progress(&actor, BookId(id))
            .await?,
    ))
}

async fn save_progress(
    State(state): State<BookHttpState>,
    WebAuth(actor): WebAuth,
    Path(id): Path<Uuid>,
    Json(cmd): Json<SaveProgressCommand>,
) -> ApiResult<Json<ReadingProgress>> {
    Ok(Json(
        state
            .services
            .reading
            .save_progress(&actor, BookId(id), cmd)
            .await?,
    ))
}

#[derive(Deserialize)]
struct SetFavoriteRequest {
    favorite: bool,
}

async fn set_favorite(
    State(state): State<BookHttpState>,
    WebAuth(actor): WebAuth,
    Path(id): Path<Uuid>,
    Json(req): Json<SetFavoriteRequest>,
) -> ApiResult<StatusCode> {
    state
        .services
        .reading
        .set_favorite(&actor, BookId(id), req.favorite)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_bookmarks(
    State(state): State<BookHttpState>,
    WebAuth(actor): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<Bookmark>>> {
    Ok(Json(
        state
            .services
            .reading
            .list_bookmarks(&actor, BookId(id))
            .await?,
    ))
}

async fn home_overview(
    State(state): State<BookHttpState>,
    WebAuth(actor): WebAuth,
) -> ApiResult<Json<BookHomeOverviewDto>> {
    Ok(Json(
        state
            .services
            .catalog
            .home_overview(&actor, &state.services.reading)
            .await?,
    ))
}

async fn list_all_bookmarks(
    State(state): State<BookHttpState>,
    WebAuth(actor): WebAuth,
) -> ApiResult<Json<Vec<BookBookmarkItemDto>>> {
    Ok(Json(
        state.services.reading.list_all_bookmarks(&actor).await?,
    ))
}

async fn create_bookmark(
    State(state): State<BookHttpState>,
    WebAuth(actor): WebAuth,
    Path(id): Path<Uuid>,
    Json(cmd): Json<CreateBookmarkCommand>,
) -> ApiResult<(StatusCode, Json<Bookmark>)> {
    let bm = state
        .services
        .reading
        .create_bookmark(&actor, BookId(id), cmd)
        .await?;
    Ok((StatusCode::CREATED, Json(bm)))
}

async fn delete_bookmark(
    State(state): State<BookHttpState>,
    WebAuth(actor): WebAuth,
    Path(bookmark_id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    state
        .services
        .reading
        .delete_bookmark(&actor, BookmarkId(bookmark_id))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_shelves(
    State(state): State<BookHttpState>,
    WebAuth(actor): WebAuth,
) -> ApiResult<Json<Vec<ShelfDto>>> {
    Ok(Json(state.services.reading.list_shelves(&actor).await?))
}

#[derive(Deserialize)]
struct ShelfNameRequest {
    name: String,
}

async fn create_shelf(
    State(state): State<BookHttpState>,
    WebAuth(actor): WebAuth,
    Json(req): Json<ShelfNameRequest>,
) -> ApiResult<(StatusCode, Json<ShelfDto>)> {
    let shelf = state
        .services
        .reading
        .create_shelf(&actor, req.name)
        .await?;
    Ok((StatusCode::CREATED, Json(shelf)))
}

async fn rename_shelf(
    State(state): State<BookHttpState>,
    WebAuth(actor): WebAuth,
    Path(shelf_id): Path<Uuid>,
    Json(req): Json<ShelfNameRequest>,
) -> ApiResult<Json<ShelfDto>> {
    Ok(Json(
        state
            .services
            .reading
            .rename_shelf(&actor, ShelfId(shelf_id), req.name)
            .await?,
    ))
}

async fn delete_shelf(
    State(state): State<BookHttpState>,
    WebAuth(actor): WebAuth,
    Path(shelf_id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    state
        .services
        .reading
        .delete_shelf(&actor, ShelfId(shelf_id))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn add_book_to_shelf(
    State(state): State<BookHttpState>,
    WebAuth(actor): WebAuth,
    Path((shelf_id, book_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<ShelfDto>> {
    Ok(Json(
        state
            .services
            .reading
            .add_book_to_shelf(&actor, ShelfId(shelf_id), BookId(book_id))
            .await?,
    ))
}

async fn remove_book_from_shelf(
    State(state): State<BookHttpState>,
    WebAuth(actor): WebAuth,
    Path((shelf_id, book_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<ShelfDto>> {
    Ok(Json(
        state
            .services
            .reading
            .remove_book_from_shelf(&actor, ShelfId(shelf_id), BookId(book_id))
            .await?,
    ))
}

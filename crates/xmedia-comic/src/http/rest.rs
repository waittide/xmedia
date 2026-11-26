use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Multipart, Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::Response;
use axum::routing::{get, patch, post, put};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use xmedia_core::application::bindings::{
    BindMediaItemsInput, BindMediaItemsResultDto, UnbindMediaItemResultDto,
};
use xmedia_core::domain::error::DomainError;
use xmedia_core::http::{
    ApiError, ApiResult, TempUploadFile, WebAuth, into_range_http_response, parse_http_range_spec,
    stream_multipart_field_to_temp_file,
};

use crate::application::dto::{
    ChapterItemDto, ComicBookmarkItemDto, CreateComicBookmarkInput, HomeOverviewDto,
    PoolSeriesSummaryDto, ReaderContextDto, SaveSeriesReaderPrefInput, SeriesDetailDto,
    SeriesQueryFilter, SeriesSummaryDto, ShelfDetailDto, TaxonomyOverviewDto, UpdateProgressInput,
    UploadArchivePathInput, UploadArchiveResultDto, UpsertShelfInput,
};
use crate::application::ports::PageContent;
use crate::domain::chapter::ChapterMetadataPatch;
use crate::domain::id::{ChapterId, ComicBookmarkId, ComicShelfId, SeriesId};
use crate::domain::reading::{ChapterProgress, ComicBookmark, ComicShelf, SeriesReaderPref};
use crate::domain::series::{Series, SeriesMetadataPatch};
use crate::http::state::ComicHttpState;

pub fn router() -> Router<ComicHttpState> {
    Router::new()
        .route("/home", get(home_overview))
        .route("/taxonomy", get(taxonomy_overview))
        .route("/pool", get(list_pool_series))
        .route("/bindings", post(bind_series))
        .route("/bindings/{id}", axum::routing::delete(unbind_series))
        .route("/series", get(list_series))
        .route(
            "/series/{id}",
            get(get_series_detail)
                .patch(update_series_metadata)
                .delete(delete_series),
        )
        .route(
            "/series/{id}/cover",
            post(upload_series_cover).layer(DefaultBodyLimit::max(32 * 1024 * 1024)),
        )
        .route("/series/{id}/favorite", put(set_favorite))
        .route("/series/{id}/shelves", put(set_series_shelves))
        .route("/series/{id}/reader-pref", put(save_series_reader_pref))
        .route("/series/{id}/mark-read", post(mark_series_read))
        .route("/series/{id}/mark-volume-read", post(mark_volume_read))
        .route(
            "/chapters/{id}",
            patch(update_chapter_metadata).delete(delete_chapter),
        )
        .route("/chapters/{id}/reader", get(get_reader_context))
        .route("/chapters/{id}/pages/{page_num}", get(stream_chapter_page))
        .route("/chapters/{id}/download", get(download_chapter_archive))
        .route("/chapters/{id}/progress", put(update_chapter_progress))
        .route("/chapters/{id}/complete", post(set_chapter_completed))
        .route(
            "/chapters/{id}/mark-previous-read",
            post(mark_previous_read),
        )
        .route(
            "/chapters/{id}/bookmarks",
            get(list_chapter_bookmarks).post(create_bookmark),
        )
        .route("/bookmarks", get(list_all_bookmarks))
        .route("/bookmarks/{id}", axum::routing::delete(delete_bookmark))
        .route("/covers/{key}", get(stream_cover))
        .route("/shelves", get(list_shelves).post(create_shelf))
        .route("/shelves/{id}", put(update_shelf).delete(delete_shelf))
        .route(
            "/upload",
            post(upload_comic_archive).layer(DefaultBodyLimit::max(2 * 1024 * 1024 * 1024)),
        )
}

async fn home_overview(
    State(state): State<ComicHttpState>,
    WebAuth(user): WebAuth,
) -> ApiResult<Json<HomeOverviewDto>> {
    Ok(Json(state.services.catalog.home_overview(&user).await?))
}

async fn taxonomy_overview(
    State(state): State<ComicHttpState>,
    WebAuth(user): WebAuth,
) -> ApiResult<Json<TaxonomyOverviewDto>> {
    Ok(Json(state.services.catalog.taxonomy_overview(&user).await?))
}

async fn list_pool_series(
    State(state): State<ComicHttpState>,
    WebAuth(user): WebAuth,
    Query(filter): Query<SeriesQueryFilter>,
) -> ApiResult<Json<Vec<PoolSeriesSummaryDto>>> {
    Ok(Json(
        state
            .services
            .catalog
            .list_pool_series(&user, filter)
            .await?,
    ))
}

async fn bind_series(
    State(state): State<ComicHttpState>,
    WebAuth(user): WebAuth,
    Json(input): Json<BindMediaItemsInput>,
) -> ApiResult<Json<BindMediaItemsResultDto>> {
    Ok(Json(
        state.services.catalog.bind_series(&user, input).await?,
    ))
}

#[derive(Deserialize, Default)]
struct UnbindQuery {
    user_id: Option<Uuid>,
}

async fn unbind_series(
    State(state): State<ComicHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
    Query(query): Query<UnbindQuery>,
) -> ApiResult<Json<UnbindMediaItemResultDto>> {
    Ok(Json(
        state
            .services
            .catalog
            .unbind_series(&user, SeriesId(id), query.user_id)
            .await?,
    ))
}

async fn list_series(
    State(state): State<ComicHttpState>,
    WebAuth(user): WebAuth,
    Query(filter): Query<SeriesQueryFilter>,
) -> ApiResult<Json<Vec<SeriesSummaryDto>>> {
    Ok(Json(
        state.services.catalog.list_series(&user, filter).await?,
    ))
}

async fn get_series_detail(
    State(state): State<ComicHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<SeriesDetailDto>> {
    Ok(Json(
        state
            .services
            .catalog
            .get_series_detail(&user, SeriesId(id))
            .await?,
    ))
}

async fn update_series_metadata(
    State(state): State<ComicHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
    Json(patch): Json<SeriesMetadataPatch>,
) -> ApiResult<Json<Series>> {
    Ok(Json(
        state
            .services
            .catalog
            .update_series_metadata(&user, SeriesId(id), patch)
            .await?,
    ))
}

async fn delete_series(
    State(state): State<ComicHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    state
        .services
        .catalog
        .delete_series(&user, SeriesId(id))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn upload_series_cover(
    State(state): State<ComicHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
    mut multipart: Multipart,
) -> ApiResult<Json<Series>> {
    user.ensure_admin()?;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError(DomainError::Validation(format!("multipart error: {e}"))))?
    {
        let bytes = field
            .bytes()
            .await
            .map_err(|e| ApiError(DomainError::Validation(format!("read cover error: {e}"))))?;
        if !bytes.is_empty() {
            let updated = state
                .services
                .catalog
                .update_series_cover(&user, SeriesId(id), &bytes)
                .await?;
            return Ok(Json(updated));
        }
    }
    Err(ApiError(DomainError::Validation(
        "no cover image provided".into(),
    )))
}

#[derive(Deserialize)]
struct FavoritePayload {
    favorite: bool,
}

async fn set_favorite(
    State(state): State<ComicHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
    Json(payload): Json<FavoritePayload>,
) -> ApiResult<StatusCode> {
    state
        .services
        .reading
        .set_favorite(user.id, SeriesId(id), payload.favorite)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct SeriesShelvesPayload {
    shelf_ids: Vec<ComicShelfId>,
}

async fn set_series_shelves(
    State(state): State<ComicHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
    Json(payload): Json<SeriesShelvesPayload>,
) -> ApiResult<StatusCode> {
    state
        .services
        .reading
        .set_series_shelves(user.id, SeriesId(id), &payload.shelf_ids)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn save_series_reader_pref(
    State(state): State<ComicHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
    Json(input): Json<SaveSeriesReaderPrefInput>,
) -> ApiResult<Json<SeriesReaderPref>> {
    Ok(Json(
        state
            .services
            .reading
            .save_series_reader_pref(user.id, SeriesId(id), input)
            .await?,
    ))
}

#[derive(Deserialize)]
struct CompletedPayload {
    completed: bool,
}

#[derive(Serialize)]
struct MarkedCountResponse {
    marked_count: u32,
}

async fn mark_series_read(
    State(state): State<ComicHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
    Json(payload): Json<CompletedPayload>,
) -> ApiResult<Json<MarkedCountResponse>> {
    let marked_count = state
        .services
        .reading
        .mark_series_completed(user.id, SeriesId(id), payload.completed)
        .await?;
    Ok(Json(MarkedCountResponse { marked_count }))
}

#[derive(Deserialize)]
struct MarkVolumePayload {
    volume_number: f64,
    completed: bool,
}

async fn mark_volume_read(
    State(state): State<ComicHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
    Json(payload): Json<MarkVolumePayload>,
) -> ApiResult<Json<MarkedCountResponse>> {
    let marked_count = state
        .services
        .reading
        .mark_volume_completed(
            user.id,
            SeriesId(id),
            payload.volume_number,
            payload.completed,
        )
        .await?;
    Ok(Json(MarkedCountResponse { marked_count }))
}

async fn update_chapter_metadata(
    State(state): State<ComicHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
    Json(patch): Json<ChapterMetadataPatch>,
) -> ApiResult<Json<ChapterItemDto>> {
    Ok(Json(
        state
            .services
            .catalog
            .update_chapter_metadata(&user, ChapterId(id), patch)
            .await?,
    ))
}

async fn delete_chapter(
    State(state): State<ComicHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    state
        .services
        .catalog
        .delete_chapter(&user, ChapterId(id))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn get_reader_context(
    State(state): State<ComicHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<ReaderContextDto>> {
    let chapter_id = ChapterId(id);
    state
        .services
        .catalog
        .require_chapter_bound(&user, chapter_id)
        .await?;
    state
        .services
        .online
        .ensure_tracked_chapter_cached(chapter_id)
        .await?;
    Ok(Json(
        state
            .services
            .catalog
            .get_reader_context(&user, chapter_id)
            .await?,
    ))
}

async fn stream_chapter_page(
    State(state): State<ComicHttpState>,
    WebAuth(user): WebAuth,
    headers: HeaderMap,
    Path((id, page_num)): Path<(Uuid, u32)>,
) -> ApiResult<Response> {
    let chapter_id = ChapterId(id);
    state
        .services
        .catalog
        .require_chapter_bound(&user, chapter_id)
        .await?;
    state
        .services
        .online
        .ensure_tracked_chapter_cached(chapter_id)
        .await?;
    let page = state
        .services
        .catalog
        .read_chapter_page(&user, chapter_id, page_num)
        .await?;
    Ok(respond_with_etag(page, &headers))
}

async fn download_chapter_archive(
    State(state): State<ComicHttpState>,
    WebAuth(user): WebAuth,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> ApiResult<Response> {
    let chapter_id = ChapterId(id);
    state
        .services
        .catalog
        .require_chapter_bound(&user, chapter_id)
        .await?;
    state
        .services
        .online
        .ensure_tracked_chapter_cached(chapter_id)
        .await?;
    let spec = parse_http_range_spec(&headers);
    let (stream, filename, mime) = state
        .services
        .catalog
        .open_chapter_archive_stream(&user, chapter_id, spec)
        .await?;
    Ok(into_range_http_response(
        stream,
        spec.is_partial(),
        &mime,
        Some(&filename),
        true,
        None,
    ))
}

#[derive(Deserialize, Default)]
struct CoverSizeQuery {
    size: Option<u32>,
}

async fn stream_cover(
    State(state): State<ComicHttpState>,
    WebAuth(user): WebAuth,
    headers: HeaderMap,
    Path(key): Path<String>,
    Query(query): Query<CoverSizeQuery>,
) -> ApiResult<Response> {
    let thumb = state
        .services
        .catalog
        .read_cover_sized(&user, &key, query.size)
        .await?;
    Ok(respond_with_etag(thumb, &headers))
}

pub fn respond_with_etag(content: PageContent, req_headers: &HeaderMap) -> Response {
    if let Some(inm) = req_headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        && (inm.trim() == content.etag || inm.trim() == "*")
    {
        let mut not_mod = Response::new(Body::empty());
        *not_mod.status_mut() = StatusCode::NOT_MODIFIED;
        if let Ok(et) = HeaderValue::from_str(&content.etag) {
            not_mod.headers_mut().insert(header::ETAG, et);
        }
        return not_mod;
    }

    let mut res = Response::new(Body::from(content.bytes));
    *res.status_mut() = StatusCode::OK;
    let headers = res.headers_mut();
    if let Ok(ct) = HeaderValue::from_str(&content.mime_type) {
        headers.insert(header::CONTENT_TYPE, ct);
    }
    if let Ok(et) = HeaderValue::from_str(&content.etag) {
        headers.insert(header::ETAG, et);
    }
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("sandbox; default-src 'none'; img-src 'self' data:"),
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=86400"),
    );
    res
}

async fn update_chapter_progress(
    State(state): State<ComicHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateProgressInput>,
) -> ApiResult<Json<ChapterProgress>> {
    Ok(Json(
        state
            .services
            .reading
            .update_progress(user.id, ChapterId(id), input)
            .await?,
    ))
}

async fn set_chapter_completed(
    State(state): State<ComicHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
    Json(payload): Json<CompletedPayload>,
) -> ApiResult<StatusCode> {
    state
        .services
        .reading
        .set_chapter_completed(user.id, ChapterId(id), payload.completed)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn mark_previous_read(
    State(state): State<ComicHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<MarkedCountResponse>> {
    let marked_count = state
        .services
        .reading
        .mark_previous_chapters_read(user.id, ChapterId(id))
        .await?;
    Ok(Json(MarkedCountResponse { marked_count }))
}

async fn list_shelves(
    State(state): State<ComicHttpState>,
    WebAuth(user): WebAuth,
) -> ApiResult<Json<Vec<ShelfDetailDto>>> {
    Ok(Json(
        state
            .services
            .reading
            .list_shelves(&user, &state.services.catalog)
            .await?,
    ))
}

async fn create_shelf(
    State(state): State<ComicHttpState>,
    WebAuth(user): WebAuth,
    Json(input): Json<UpsertShelfInput>,
) -> ApiResult<(StatusCode, Json<ComicShelf>)> {
    let shelf = state.services.reading.create_shelf(user.id, input).await?;
    Ok((StatusCode::CREATED, Json(shelf)))
}

async fn update_shelf(
    State(state): State<ComicHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
    Json(input): Json<UpsertShelfInput>,
) -> ApiResult<Json<ComicShelf>> {
    Ok(Json(
        state
            .services
            .reading
            .update_shelf(user.id, ComicShelfId(id), input)
            .await?,
    ))
}

async fn delete_shelf(
    State(state): State<ComicHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    state
        .services
        .reading
        .delete_shelf(user.id, ComicShelfId(id))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_chapter_bookmarks(
    State(state): State<ComicHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<ComicBookmark>>> {
    Ok(Json(
        state
            .services
            .reading
            .list_chapter_bookmarks(user.id, ChapterId(id))
            .await?,
    ))
}

async fn list_all_bookmarks(
    State(state): State<ComicHttpState>,
    WebAuth(user): WebAuth,
) -> ApiResult<Json<Vec<ComicBookmarkItemDto>>> {
    Ok(Json(
        state.services.reading.list_all_bookmarks(user.id).await?,
    ))
}

async fn create_bookmark(
    State(state): State<ComicHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
    Json(input): Json<CreateComicBookmarkInput>,
) -> ApiResult<(StatusCode, Json<ComicBookmark>)> {
    let bm = state
        .services
        .reading
        .create_bookmark(user.id, ChapterId(id), input)
        .await?;
    Ok((StatusCode::CREATED, Json(bm)))
}

async fn delete_bookmark(
    State(state): State<ComicHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    state
        .services
        .reading
        .delete_bookmark(user.id, ComicBookmarkId(id))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
struct UploadArchivesResponse {
    uploaded: Vec<UploadArchiveResultDto>,
}

const MAX_COMIC_UPLOAD_BYTES: u64 = 2 * 1024 * 1024 * 1024;

async fn upload_comic_archive(
    State(state): State<ComicHttpState>,
    WebAuth(user): WebAuth,
    mut multipart: Multipart,
) -> ApiResult<(StatusCode, Json<UploadArchivesResponse>)> {
    user.ensure_admin()?;
    let mut target_series_id: Option<SeriesId> = None;
    let mut series_title_override: Option<String> = None;
    let mut files: Vec<(String, TempUploadFile)> = Vec::new();
    let mut total_bytes: u64 = 0;

    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError(DomainError::Validation(format!("multipart error: {e}"))))?
    {
        let field_name = field.name().unwrap_or("").to_owned();
        if let Some(filename) = field.file_name().map(ToString::to_string) {
            crate::application::catalog::validate_archive_filename(&filename)?;
            let ext = std::path::Path::new(&filename)
                .extension()
                .and_then(|s| s.to_str());
            let remaining_cap = MAX_COMIC_UPLOAD_BYTES.saturating_sub(total_bytes);
            let temp = stream_multipart_field_to_temp_file(&mut field, remaining_cap, ext).await?;
            if !temp.is_empty() {
                total_bytes = total_bytes.saturating_add(temp.size());
                files.push((filename, temp));
            }
        } else {
            let raw_bytes = field.bytes().await.map_err(|e| {
                ApiError(DomainError::Validation(format!(
                    "failed to read multipart text field: {e}"
                )))
            })?;
            if raw_bytes.len() > 64 * 1024 {
                return Err(ApiError(DomainError::Validation(
                    "multipart text field exceeds 64 KiB limit".into(),
                )));
            }
            let text = String::from_utf8_lossy(&raw_bytes);
            match field_name.as_str() {
                "target_series_id" | "series_id" => {
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        let parsed = trimmed.parse::<Uuid>().map_err(|_| {
                            ApiError(DomainError::Validation(
                                "invalid target_series_id UUID".into(),
                            ))
                        })?;
                        target_series_id = Some(SeriesId(parsed));
                    }
                }
                "series_title" | "series_title_override" if !text.trim().is_empty() => {
                    series_title_override = Some(text.trim().to_owned());
                }
                _ => {}
            }
        }
    }

    if files.is_empty() {
        return Err(ApiError(DomainError::Validation(
            "no comic archive files received in multipart payload".into(),
        )));
    }

    let mut uploaded = Vec::with_capacity(files.len());
    for (filename, temp_guard) in files {
        let res = state
            .services
            .catalog
            .upload_archive_from_path(
                &user,
                UploadArchivePathInput {
                    filename,
                    temp_path: temp_guard.path().to_path_buf(),
                    target_series_id,
                    series_title_override: series_title_override.clone(),
                },
            )
            .await?;
        uploaded.push(res);
    }

    Ok((
        StatusCode::CREATED,
        Json(UploadArchivesResponse { uploaded }),
    ))
}

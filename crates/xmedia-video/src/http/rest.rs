use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Multipart, Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use xmedia_core::application::bindings::{
    BindMediaItemsInput, BindMediaItemsResultDto, UnbindMediaItemResultDto,
};
use xmedia_core::domain::error::DomainError;
use xmedia_core::domain::library::LibraryId;
use xmedia_core::domain::user::require_admin;
use xmedia_core::http::{
    ApiError, ApiResult, TempUploadFile, WebAuth, into_binary_cache_response,
    parse_http_range_spec, stream_multipart_field_to_temp_file,
};

use crate::application::dto::{
    SaveWatchProgressInput, ShowDetailDto, ShowSummaryDto, SubtitleTrackDto, UpsertVideoShelfInput,
    VideoDetailDto, VideoFavoritesDto, VideoHomeOverviewDto, VideoPoolDto, VideoShelfDetailDto,
    VideoShelfDto, VideoSummaryDto, VideoSystemStatusDto, VideoTaxonomyDto, VideoUploadPathItem,
    VideoUploadResultDto,
};
use crate::application::ports::parse_quality_param;
use crate::domain::id::{ShowId, VideoId, VideoShelfId};
use crate::domain::video::{
    ShowMetadataPatch, ShowQueryFilter, VideoContainer, VideoMetadataPatch, VideoQueryFilter,
    is_subtitle_extension,
};
use crate::domain::watch::{VideoFavoriteKind, WatchProgress};
use crate::http::into_video_http_response;
use crate::http::state::VideoHttpState;

pub fn router() -> Router<VideoHttpState> {
    Router::new()
        .route("/status", get(video_status))
        .route("/home", get(home_overview))
        .route("/taxonomy", get(video_taxonomy))
        .route("/pool", get(list_pool))
        .route("/bindings", post(bind_items))
        .route("/bindings/{id}", delete(unbind_item))
        .route(
            "/upload",
            post(upload_videos).layer(DefaultBodyLimit::max(4 * 1024 * 1024 * 1024)),
        )
        .route("/videos", get(list_videos))
        .route(
            "/videos/{id}",
            get(get_video_detail)
                .patch(update_video_metadata)
                .delete(delete_video),
        )
        .route(
            "/videos/{id}/poster",
            post(upload_video_poster).layer(DefaultBodyLimit::max(32 * 1024 * 1024)),
        )
        .route("/videos/{id}/stream", get(stream_video))
        .route("/videos/{id}/download", get(download_video))
        .route("/videos/{id}/subtitles", get(list_video_subtitles))
        .route("/videos/{id}/subtitles/{sub_id}", get(get_subtitle_vtt))
        .route("/videos/{id}/progress", put(save_watch_progress))
        .route("/videos/{id}/watched", post(set_video_watched))
        .route("/shows", get(list_shows))
        .route(
            "/shows/{id}",
            get(get_show_detail).patch(update_show_metadata),
        )
        .route(
            "/shows/{id}/poster",
            post(upload_show_poster).layer(DefaultBodyLimit::max(32 * 1024 * 1024)),
        )
        .route("/posters/{key}", get(get_poster))
        .route("/favorites", get(list_favorites))
        .route("/favorites/{kind}/{id}", put(set_favorite))
        .route("/shelves", get(list_shelves).post(create_shelf))
        .route(
            "/shelves/{id}",
            get(get_shelf).put(update_shelf).delete(delete_shelf),
        )
}

#[derive(Serialize)]
struct OkResponse {
    ok: bool,
}

#[derive(Deserialize, Default)]
struct StreamQueryParams {
    quality: Option<String>,
    audio_stream: Option<u32>,
    start_secs: Option<u32>,
}

#[derive(Deserialize, Default)]
struct PosterParams {
    size: Option<u32>,
}

#[derive(Deserialize, Default)]
struct SubtitleParams {
    offset_secs: Option<f64>,
}

#[derive(Deserialize)]
struct WatchedBody {
    watched: bool,
}

#[derive(Deserialize)]
struct FavoriteBody {
    favorite: bool,
}

#[derive(Deserialize, Default)]
struct PoolQueryParams {
    library_id: Option<Uuid>,
    search: Option<String>,
}

#[derive(Deserialize, Default)]
struct UnbindQueryParams {
    user_id: Option<Uuid>,
}

async fn video_status(
    State(state): State<VideoHttpState>,
    WebAuth(user): WebAuth,
) -> ApiResult<Json<VideoSystemStatusDto>> {
    Ok(Json(state.services.catalog.system_status(user.id).await?))
}

async fn home_overview(
    State(state): State<VideoHttpState>,
    WebAuth(user): WebAuth,
) -> ApiResult<Json<VideoHomeOverviewDto>> {
    Ok(Json(state.services.catalog.home_overview(&user).await?))
}

async fn video_taxonomy(
    State(state): State<VideoHttpState>,
    WebAuth(user): WebAuth,
) -> ApiResult<Json<VideoTaxonomyDto>> {
    Ok(Json(state.services.catalog.get_taxonomy(user.id).await?))
}

async fn list_pool(
    State(state): State<VideoHttpState>,
    WebAuth(user): WebAuth,
    Query(q): Query<PoolQueryParams>,
) -> ApiResult<Json<VideoPoolDto>> {
    Ok(Json(
        state
            .services
            .catalog
            .list_pool(&user, q.library_id.map(LibraryId), q.search)
            .await?,
    ))
}

async fn bind_items(
    State(state): State<VideoHttpState>,
    WebAuth(user): WebAuth,
    Json(input): Json<BindMediaItemsInput>,
) -> ApiResult<Json<BindMediaItemsResultDto>> {
    Ok(Json(state.services.catalog.bind_items(&user, input).await?))
}

async fn unbind_item(
    State(state): State<VideoHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
    Query(q): Query<UnbindQueryParams>,
) -> ApiResult<Json<UnbindMediaItemResultDto>> {
    Ok(Json(
        state
            .services
            .catalog
            .unbind_item(&user, id, q.user_id)
            .await?,
    ))
}

const MAX_VIDEO_UPLOAD_BYTES: u64 = 4 * 1024 * 1024 * 1024;

async fn upload_videos(
    State(state): State<VideoHttpState>,
    WebAuth(user): WebAuth,
    mut multipart: Multipart,
) -> ApiResult<(StatusCode, Json<VideoUploadResultDto>)> {
    require_admin(&user)?;
    let mut temp_guards: Vec<(String, TempUploadFile)> = Vec::new();
    let mut total_bytes: u64 = 0;

    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError(DomainError::Validation(format!("multipart error: {e}"))))?
    {
        let Some(filename) = field.file_name().map(str::to_string) else {
            continue;
        };
        let ext = std::path::Path::new(&filename)
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .trim();
        if VideoContainer::from_extension(ext).is_none() && !is_subtitle_extension(ext) {
            return Err(ApiError(DomainError::UnsupportedFormat(format!(
                "unsupported file extension '.{ext}'"
            ))));
        }
        let remaining_cap = MAX_VIDEO_UPLOAD_BYTES.saturating_sub(total_bytes);
        let temp =
            stream_multipart_field_to_temp_file(&mut field, remaining_cap, Some(ext)).await?;
        if !temp.is_empty() {
            total_bytes = total_bytes.saturating_add(temp.size());
            temp_guards.push((filename, temp));
        }
    }

    let items: Vec<VideoUploadPathItem> = temp_guards
        .iter()
        .map(|(filename, guard)| VideoUploadPathItem {
            filename: filename.clone(),
            temp_path: guard.path().to_path_buf(),
        })
        .collect();

    let res = state
        .services
        .ingest
        .upload_files_from_paths(&user, items)
        .await?;
    Ok((StatusCode::CREATED, Json(res)))
}

async fn list_videos(
    State(state): State<VideoHttpState>,
    WebAuth(user): WebAuth,
    Query(filter): Query<VideoQueryFilter>,
) -> ApiResult<Json<Vec<VideoSummaryDto>>> {
    Ok(Json(
        state.services.catalog.list_videos(&user, filter).await?,
    ))
}

async fn get_video_detail(
    State(state): State<VideoHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<VideoDetailDto>> {
    Ok(Json(
        state
            .services
            .catalog
            .get_video_detail(&user, VideoId(id))
            .await?,
    ))
}

async fn update_video_metadata(
    State(state): State<VideoHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
    Json(patch): Json<VideoMetadataPatch>,
) -> ApiResult<Json<VideoSummaryDto>> {
    Ok(Json(
        state
            .services
            .catalog
            .update_video_metadata(&user, VideoId(id), patch)
            .await?,
    ))
}

async fn delete_video(
    State(state): State<VideoHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<OkResponse>> {
    state
        .services
        .catalog
        .delete_video(&user, VideoId(id))
        .await?;
    Ok(Json(OkResponse { ok: true }))
}

async fn upload_video_poster(
    State(state): State<VideoHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
    mut multipart: Multipart,
) -> ApiResult<Json<VideoSummaryDto>> {
    require_admin(&user)?;
    let bytes = read_first_multipart_bytes(&mut multipart).await?;
    Ok(Json(
        state
            .services
            .catalog
            .upload_video_poster(&user, VideoId(id), &bytes)
            .await?,
    ))
}

async fn stream_video(
    State(state): State<VideoHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
    Query(q): Query<StreamQueryParams>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let spec = parse_http_range_spec(&headers);
    let quality = parse_quality_param(q.quality.as_deref());
    let resolved = state
        .services
        .streaming
        .open_stream(
            user.id,
            VideoId(id),
            quality,
            q.audio_stream,
            q.start_secs,
            spec,
        )
        .await?;
    Ok(into_video_http_response(resolved, false))
}

async fn download_video(
    State(state): State<VideoHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let spec = parse_http_range_spec(&headers);
    let resolved = state
        .services
        .streaming
        .open_raw_download(user.id, VideoId(id), spec)
        .await?;
    Ok(into_video_http_response(resolved, true))
}

async fn list_video_subtitles(
    State(state): State<VideoHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<SubtitleTrackDto>>> {
    Ok(Json(
        state
            .services
            .catalog
            .list_video_subtitles(user.id, VideoId(id))
            .await?,
    ))
}

async fn get_subtitle_vtt(
    State(state): State<VideoHttpState>,
    WebAuth(user): WebAuth,
    Path((id, sub_id)): Path<(Uuid, String)>,
    Query(q): Query<SubtitleParams>,
) -> ApiResult<Response> {
    let vtt = state
        .services
        .catalog
        .get_subtitle_webvtt_with_offset(user.id, VideoId(id), &sub_id, q.offset_secs)
        .await?;
    let mut res = (StatusCode::OK, Body::from(vtt)).into_response();
    let headers = res.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/vtt; charset=utf-8"),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'none'"),
    );
    Ok(res)
}

async fn save_watch_progress(
    State(state): State<VideoHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
    Json(input): Json<SaveWatchProgressInput>,
) -> ApiResult<Json<WatchProgress>> {
    Ok(Json(
        state
            .services
            .watch
            .save_progress(&user, VideoId(id), input)
            .await?,
    ))
}

async fn set_video_watched(
    State(state): State<VideoHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
    Json(body): Json<WatchedBody>,
) -> ApiResult<Json<WatchProgress>> {
    Ok(Json(
        state
            .services
            .watch
            .set_watched(&user, VideoId(id), body.watched)
            .await?,
    ))
}

async fn list_shows(
    State(state): State<VideoHttpState>,
    WebAuth(user): WebAuth,
    Query(filter): Query<ShowQueryFilter>,
) -> ApiResult<Json<Vec<ShowSummaryDto>>> {
    Ok(Json(
        state.services.catalog.list_shows(&user, filter).await?,
    ))
}

async fn get_show_detail(
    State(state): State<VideoHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<ShowDetailDto>> {
    Ok(Json(
        state
            .services
            .catalog
            .get_show_detail(&user, ShowId(id))
            .await?,
    ))
}

async fn update_show_metadata(
    State(state): State<VideoHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
    Json(patch): Json<ShowMetadataPatch>,
) -> ApiResult<Json<ShowSummaryDto>> {
    Ok(Json(
        state
            .services
            .catalog
            .update_show_metadata(&user, ShowId(id), patch)
            .await?,
    ))
}

async fn upload_show_poster(
    State(state): State<VideoHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
    mut multipart: Multipart,
) -> ApiResult<Json<ShowSummaryDto>> {
    require_admin(&user)?;
    let bytes = read_first_multipart_bytes(&mut multipart).await?;
    Ok(Json(
        state
            .services
            .catalog
            .upload_show_poster(&user, ShowId(id), &bytes)
            .await?,
    ))
}

async fn get_poster(
    State(state): State<VideoHttpState>,
    WebAuth(user): WebAuth,
    Path(key): Path<String>,
    Query(p): Query<PosterParams>,
) -> ApiResult<Response> {
    let payload = state
        .services
        .catalog
        .get_poster(Some(&user), &key, p.size)
        .await?;
    Ok(into_binary_cache_response(payload))
}

async fn list_favorites(
    State(state): State<VideoHttpState>,
    WebAuth(user): WebAuth,
) -> ApiResult<Json<VideoFavoritesDto>> {
    Ok(Json(state.services.watch.list_favorites(&user).await?))
}

async fn set_favorite(
    State(state): State<VideoHttpState>,
    WebAuth(user): WebAuth,
    Path((kind_raw, id)): Path<(String, Uuid)>,
    Json(body): Json<FavoriteBody>,
) -> ApiResult<Json<OkResponse>> {
    let kind = VideoFavoriteKind::parse(&kind_raw)?;
    state
        .services
        .watch
        .set_favorite(&user, kind, id, body.favorite)
        .await?;
    Ok(Json(OkResponse { ok: true }))
}

async fn list_shelves(
    State(state): State<VideoHttpState>,
    WebAuth(user): WebAuth,
) -> ApiResult<Json<Vec<VideoShelfDto>>> {
    Ok(Json(state.services.watch.list_shelves(&user).await?))
}

async fn get_shelf(
    State(state): State<VideoHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<VideoShelfDetailDto>> {
    Ok(Json(
        state
            .services
            .watch
            .get_shelf(&user, VideoShelfId(id))
            .await?,
    ))
}

async fn create_shelf(
    State(state): State<VideoHttpState>,
    WebAuth(user): WebAuth,
    Json(input): Json<UpsertVideoShelfInput>,
) -> ApiResult<(StatusCode, Json<VideoShelfDetailDto>)> {
    let created = state.services.watch.create_shelf(&user, input).await?;
    Ok((StatusCode::CREATED, Json(created)))
}

async fn update_shelf(
    State(state): State<VideoHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
    Json(input): Json<UpsertVideoShelfInput>,
) -> ApiResult<Json<VideoShelfDetailDto>> {
    Ok(Json(
        state
            .services
            .watch
            .update_shelf(&user, VideoShelfId(id), input)
            .await?,
    ))
}

async fn delete_shelf(
    State(state): State<VideoHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<OkResponse>> {
    state
        .services
        .watch
        .delete_shelf(&user, VideoShelfId(id))
        .await?;
    Ok(Json(OkResponse { ok: true }))
}

async fn read_first_multipart_bytes(multipart: &mut Multipart) -> ApiResult<Vec<u8>> {
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError(DomainError::Validation(format!("multipart error: {e}"))))?
    {
        let data = field
            .bytes()
            .await
            .map_err(|e| ApiError(DomainError::Validation(format!("upload read error: {e}"))))?;
        if !data.is_empty() {
            return Ok(data.to_vec());
        }
    }
    Err(ApiError(DomainError::Validation(
        "no file payload received".into(),
    )))
}

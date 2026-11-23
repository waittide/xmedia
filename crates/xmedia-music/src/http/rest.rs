use axum::extract::{DefaultBodyLimit, Multipart, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use xmedia_core::domain::error::DomainError;
use xmedia_core::domain::library::LibraryId;
use xmedia_core::domain::user::require_admin;
use xmedia_core::http::{
    ApiError, ApiResult, TempUploadFile, WebAuth, into_binary_cache_response,
    parse_http_range_spec, stream_multipart_field_to_temp_file,
};

use crate::application::dto::{
    AlbumDetailDto, AlbumDto, ArtistDetailDto, ArtistDto, BookmarkDto, GlobalScanStatusDto,
    HomeOverviewDto, NowPlayingDto, PlayQueueDto, PlaylistDetailDto, PlaylistSummaryDto,
    SearchResultDto, StarredBundleDto, TrackDto, UpdatePlaylistInput, UploadBatchResultDto,
    UploadFilePathItem,
};
use crate::application::ports::FfmpegStatusDto;
use crate::domain::catalog::{
    AlbumId, AlbumListQuery, AlbumListType, ArtistId, ArtistQuery, ArtistRoleFilter, AudioFormat,
    GenreStat, MusicTagStat, PagedResult, SearchQuery, SortOrder, StructuredLyrics, TrackId,
    TrackQuery, TrackSortField,
};
use crate::domain::collection::PlaylistId;
use crate::http::into_audio_http_response;
use crate::http::state::MusicHttpState;
use crate::http::subsonic::envelope::SUBSONIC_API_VERSION;

pub fn router() -> Router<MusicHttpState> {
    Router::new()
        .route("/status", get(music_status))
        .route("/home", get(home_overview))
        .route(
            "/upload",
            post(upload_tracks).layer(DefaultBodyLimit::max(512 * 1024 * 1024)),
        )
        .route(
            "/tracks/upload",
            post(upload_tracks).layer(DefaultBodyLimit::max(512 * 1024 * 1024)),
        )
        .route("/tracks", get(query_tracks))
        .route("/tracks/recent", get(recent_tracks))
        .route("/tracks/random", get(random_tracks))
        .route("/tracks/{id}", get(get_track).delete(delete_track))
        .route("/tracks/{id}/tags", put(set_track_tags))
        .route("/tracks/{id}/lyrics", get(get_track_lyrics))
        .route("/tracks/{id}/stream", get(stream_track))
        .route("/tracks/{id}/download", get(download_track))
        .route("/covers/{id}", get(get_cover))
        .route("/albums", get(list_albums))
        .route("/albums/{id}", get(get_album))
        .route("/artists", get(list_artists))
        .route("/artists/{id}", get(get_artist))
        .route("/genres", get(list_genres))
        .route("/tags", get(list_tags))
        .route("/search", get(search_catalog))
        .route("/playlists", get(list_playlists).post(create_playlist))
        .route(
            "/playlists/{id}",
            get(get_playlist)
                .patch(update_playlist)
                .delete(delete_playlist),
        )
        .route("/starred", get(get_starred).post(star_items))
        .route("/unstarred", post(unstar_items))
        .route("/ratings", post(set_rating))
        .route("/scrobble", post(scrobble_track))
        .route("/now-playing", get(list_now_playing))
        .route("/play-queue", get(get_play_queue).put(save_play_queue))
        .route("/bookmarks", get(list_bookmarks))
        .route(
            "/bookmarks/{track_id}",
            put(upsert_bookmark).delete(delete_bookmark),
        )
}

#[derive(Serialize)]
struct OkResponse {
    ok: bool,
}

#[derive(Serialize)]
struct MusicFfmpegStatusResponse {
    available: bool,
    version: Option<String>,
    #[serde(flatten)]
    core: FfmpegStatusDto,
}

impl From<FfmpegStatusDto> for MusicFfmpegStatusResponse {
    fn from(core: FfmpegStatusDto) -> Self {
        Self {
            available: core.ffmpeg_available,
            version: core.ffmpeg_version.clone(),
            core,
        }
    }
}

#[derive(Serialize)]
struct MusicStatusResponse {
    ffmpeg: MusicFfmpegStatusResponse,
    scan: GlobalScanStatusDto,
    subsonic_api_version: &'static str,
    open_subsonic: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Deserialize, Default)]
struct TrackListParams {
    library_id: Option<Uuid>,
    album_id: Option<Uuid>,
    artist_id: Option<Uuid>,
    genre: Option<String>,
    tag: Option<String>,
    year_from: Option<i32>,
    year_to: Option<i32>,
    search: Option<String>,
    sort_by: Option<TrackSortField>,
    sort_order: Option<SortOrder>,
    page: Option<u32>,
    page_size: Option<u32>,
}

#[derive(Deserialize, Default)]
struct RandomTrackParams {
    size: Option<u32>,
    library_id: Option<Uuid>,
    genre: Option<String>,
    from_year: Option<i32>,
    to_year: Option<i32>,
}

#[derive(Deserialize, Default)]
struct RecentTrackParams {
    limit: Option<u32>,
}

#[derive(Deserialize, Default)]
struct StreamParams {
    format: Option<String>,
    max_bit_rate: Option<u32>,
    time_offset: Option<u32>,
}

#[derive(Deserialize, Default)]
struct CoverParams {
    size: Option<u32>,
}

#[derive(Deserialize, Default)]
struct AlbumListParams {
    #[serde(rename = "type")]
    list_type: Option<String>,
    library_id: Option<Uuid>,
    genre: Option<String>,
    from_year: Option<i32>,
    to_year: Option<i32>,
    search: Option<String>,
    offset: Option<u32>,
    size: Option<u32>,
}

#[derive(Deserialize, Default)]
struct ArtistListParams {
    search: Option<String>,
    library_id: Option<Uuid>,
    role: Option<ArtistRoleFilter>,
    offset: Option<u32>,
    size: Option<u32>,
}

#[derive(Deserialize, Default)]
struct SearchParams {
    q: Option<String>,
    library_id: Option<Uuid>,
    limit: Option<u32>,
}

#[derive(Deserialize)]
struct CreatePlaylistBody {
    name: String,
    comment: Option<String>,
    #[serde(default)]
    is_public: bool,
    #[serde(default)]
    track_ids: Vec<TrackId>,
}

#[derive(Deserialize, Default)]
struct StarBody {
    #[serde(default)]
    ids: Vec<String>,
    #[serde(default)]
    album_ids: Vec<AlbumId>,
    #[serde(default)]
    artist_ids: Vec<ArtistId>,
}

#[derive(Deserialize)]
struct RatingBody {
    id: String,
    rating: u8,
}

#[derive(Deserialize)]
struct ScrobbleBody {
    track_id: TrackId,
    #[serde(default = "default_true")]
    submission: bool,
    played_at_ms: Option<i64>,
}

#[derive(Deserialize)]
struct SaveQueueBody {
    #[serde(default)]
    track_ids: Vec<TrackId>,
    current_track_id: Option<TrackId>,
    #[serde(default)]
    position_ms: u64,
}

#[derive(Deserialize)]
struct BookmarkBody {
    position_ms: u64,
    comment: Option<String>,
}

async fn music_status(
    State(state): State<MusicHttpState>,
    WebAuth(_user): WebAuth,
) -> ApiResult<Json<MusicStatusResponse>> {
    let ffmpeg =
        MusicFfmpegStatusResponse::from(state.services.streaming.get_ffmpeg_status().await);
    let scan = state.services.ingest.get_scan_status().await?;
    Ok(Json(MusicStatusResponse {
        ffmpeg,
        scan,
        subsonic_api_version: SUBSONIC_API_VERSION,
        open_subsonic: true,
    }))
}

const MAX_MUSIC_UPLOAD_BYTES: u64 = 512 * 1024 * 1024;

async fn upload_tracks(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    mut multipart: Multipart,
) -> ApiResult<(StatusCode, Json<UploadBatchResultDto>)> {
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
        if !ext.eq_ignore_ascii_case("lrc") && AudioFormat::from_extension(ext).is_none() {
            return Err(ApiError(DomainError::UnsupportedFormat(format!(
                "unsupported file extension '.{ext}' for '{filename}'"
            ))));
        }
        let remaining_cap = MAX_MUSIC_UPLOAD_BYTES.saturating_sub(total_bytes);
        let temp =
            stream_multipart_field_to_temp_file(&mut field, remaining_cap, Some(ext)).await?;
        if !temp.is_empty() {
            total_bytes = total_bytes.saturating_add(temp.size());
            temp_guards.push((filename, temp));
        }
    }

    let items: Vec<UploadFilePathItem> = temp_guards
        .iter()
        .map(|(filename, guard)| UploadFilePathItem {
            filename: filename.clone(),
            temp_path: guard.path().to_path_buf(),
        })
        .collect();

    let result = state
        .services
        .ingest
        .upload_files_from_paths(&user, items)
        .await?;
    Ok((StatusCode::CREATED, Json(result)))
}

async fn home_overview(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
) -> ApiResult<Json<HomeOverviewDto>> {
    Ok(Json(
        state.services.catalog.get_home_overview(user.id).await?,
    ))
}

async fn query_tracks(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Query(p): Query<TrackListParams>,
) -> ApiResult<Json<PagedResult<TrackDto>>> {
    let q = TrackQuery {
        library_id: p.library_id.map(LibraryId),
        album_id: p.album_id.map(AlbumId),
        artist_id: p.artist_id.map(ArtistId),
        genre: p.genre,
        tag: p.tag,
        year_from: p.year_from,
        year_to: p.year_to,
        search: p.search,
        sort_by: p.sort_by.unwrap_or(TrackSortField::CreatedAt),
        sort_order: p.sort_order.unwrap_or(SortOrder::Desc),
        page: p.page.unwrap_or(1),
        page_size: p.page_size.unwrap_or(50).clamp(1, 500),
    };
    Ok(Json(state.services.catalog.query_tracks(user.id, q).await?))
}

async fn recent_tracks(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Query(p): Query<RecentTrackParams>,
) -> ApiResult<Json<Vec<TrackDto>>> {
    Ok(Json(
        state
            .services
            .catalog
            .list_recent_played_tracks(user.id, p.limit.unwrap_or(50))
            .await?,
    ))
}

async fn random_tracks(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Query(p): Query<RandomTrackParams>,
) -> ApiResult<Json<Vec<TrackDto>>> {
    Ok(Json(
        state
            .services
            .catalog
            .get_random_tracks(
                user.id,
                p.size.unwrap_or(20),
                p.library_id.map(LibraryId),
                p.genre.as_deref(),
                p.from_year,
                p.to_year,
            )
            .await?,
    ))
}

async fn get_track(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<TrackDto>> {
    Ok(Json(
        state
            .services
            .catalog
            .get_track(user.id, TrackId(id))
            .await?,
    ))
}

async fn delete_track(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<OkResponse>> {
    state
        .services
        .ingest
        .delete_track(&user, TrackId(id))
        .await?;
    Ok(Json(OkResponse { ok: true }))
}

async fn get_track_lyrics(
    State(state): State<MusicHttpState>,
    WebAuth(_user): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<StructuredLyrics>> {
    let _ = state
        .services
        .online
        .ensure_tracked_lyrics_cached(TrackId(id))
        .await;
    Ok(Json(
        state
            .services
            .catalog
            .get_lyrics_by_track(TrackId(id))
            .await?,
    ))
}

async fn stream_track(
    State(state): State<MusicHttpState>,
    WebAuth(_user): WebAuth,
    Path(id): Path<Uuid>,
    Query(p): Query<StreamParams>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    state
        .services
        .online
        .ensure_tracked_track_cached(TrackId(id))
        .await?;
    let spec = parse_http_range_spec(&headers);
    let resolved = state
        .services
        .streaming
        .open_stream(
            TrackId(id),
            p.format.as_deref(),
            p.max_bit_rate,
            p.time_offset,
            spec,
        )
        .await?;
    Ok(into_audio_http_response(resolved, false))
}

async fn download_track(
    State(state): State<MusicHttpState>,
    WebAuth(_user): WebAuth,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    state
        .services
        .online
        .ensure_tracked_track_cached(TrackId(id))
        .await?;
    let spec = parse_http_range_spec(&headers);
    let resolved = state
        .services
        .streaming
        .open_raw_download(TrackId(id), spec)
        .await?;
    Ok(into_audio_http_response(resolved, true))
}

async fn get_cover(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<String>,
    Query(p): Query<CoverParams>,
) -> ApiResult<Response> {
    let cover = state
        .services
        .catalog
        .get_cover_art(&user, &id, p.size)
        .await?;
    Ok(into_binary_cache_response(cover))
}

async fn list_albums(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Query(p): Query<AlbumListParams>,
) -> ApiResult<Json<PagedResult<AlbumDto>>> {
    let list_type = match p.list_type.as_deref() {
        Some(raw) if !raw.trim().is_empty() => AlbumListType::parse(raw)?,
        _ => AlbumListType::Newest,
    };
    let q = AlbumListQuery {
        list_type,
        library_id: p.library_id.map(LibraryId),
        genre: p.genre,
        from_year: p.from_year,
        to_year: p.to_year,
        search: p.search,
        offset: p.offset.unwrap_or(0),
        size: p.size.unwrap_or(48).clamp(1, 500),
    };
    Ok(Json(state.services.catalog.list_albums(user.id, q).await?))
}

async fn get_album(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<AlbumDetailDto>> {
    Ok(Json(
        state
            .services
            .catalog
            .get_album_detail(user.id, AlbumId(id))
            .await?,
    ))
}

async fn list_artists(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Query(p): Query<ArtistListParams>,
) -> ApiResult<Json<PagedResult<ArtistDto>>> {
    let q = ArtistQuery {
        search: p.search,
        library_id: p.library_id.map(LibraryId),
        role_filter: p.role.unwrap_or_default(),
        offset: p.offset.unwrap_or(0),
        size: p.size.unwrap_or(100).clamp(1, 1000),
    };
    Ok(Json(state.services.catalog.list_artists(user.id, q).await?))
}

async fn get_artist(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<ArtistDetailDto>> {
    Ok(Json(
        state
            .services
            .catalog
            .get_artist_detail(user.id, ArtistId(id))
            .await?,
    ))
}

#[derive(Deserialize)]
struct SetTrackTagsBody {
    #[serde(default)]
    tags: Vec<String>,
}

async fn list_genres(
    State(state): State<MusicHttpState>,
    WebAuth(_user): WebAuth,
) -> ApiResult<Json<Vec<GenreStat>>> {
    Ok(Json(state.services.catalog.list_genres().await?))
}

async fn list_tags(
    State(state): State<MusicHttpState>,
    WebAuth(_user): WebAuth,
) -> ApiResult<Json<Vec<MusicTagStat>>> {
    Ok(Json(state.services.catalog.list_tags().await?))
}

async fn set_track_tags(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
    Json(body): Json<SetTrackTagsBody>,
) -> ApiResult<Json<TrackDto>> {
    Ok(Json(
        state
            .services
            .catalog
            .set_track_tags(&user, TrackId(id), body.tags)
            .await?,
    ))
}

async fn search_catalog(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Query(p): Query<SearchParams>,
) -> ApiResult<Json<SearchResultDto>> {
    let limit = p.limit.unwrap_or(24).clamp(1, 100);
    let q = SearchQuery {
        query: p.q.unwrap_or_default(),
        library_id: p.library_id.map(LibraryId),
        artist_count: limit,
        artist_offset: 0,
        album_count: limit,
        album_offset: 0,
        song_count: limit,
        song_offset: 0,
    };
    Ok(Json(state.services.catalog.search(user.id, q).await?))
}

async fn list_playlists(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
) -> ApiResult<Json<Vec<PlaylistSummaryDto>>> {
    Ok(Json(
        state.services.collection.list_playlists(user.id).await?,
    ))
}

async fn create_playlist(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Json(body): Json<CreatePlaylistBody>,
) -> ApiResult<(StatusCode, Json<PlaylistDetailDto>)> {
    let created = state
        .services
        .collection
        .create_playlist(
            &user,
            &body.name,
            body.comment,
            body.is_public,
            body.track_ids,
        )
        .await?;
    Ok((StatusCode::CREATED, Json(created)))
}

async fn get_playlist(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<PlaylistDetailDto>> {
    Ok(Json(
        state
            .services
            .collection
            .get_playlist(&user, PlaylistId(id))
            .await?,
    ))
}

async fn update_playlist(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdatePlaylistInput>,
) -> ApiResult<Json<PlaylistDetailDto>> {
    Ok(Json(
        state
            .services
            .collection
            .update_playlist(&user, PlaylistId(id), body)
            .await?,
    ))
}

async fn delete_playlist(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<OkResponse>> {
    state
        .services
        .collection
        .delete_playlist(&user, PlaylistId(id))
        .await?;
    Ok(Json(OkResponse { ok: true }))
}

async fn get_starred(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
) -> ApiResult<Json<StarredBundleDto>> {
    Ok(Json(
        state.services.collection.get_starred(user.id, None).await?,
    ))
}

async fn star_items(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Json(body): Json<StarBody>,
) -> ApiResult<Json<OkResponse>> {
    state
        .services
        .collection
        .star_items(user.id, &body.ids, &body.album_ids, &body.artist_ids)
        .await?;
    Ok(Json(OkResponse { ok: true }))
}

async fn unstar_items(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Json(body): Json<StarBody>,
) -> ApiResult<Json<OkResponse>> {
    state
        .services
        .collection
        .unstar_items(user.id, &body.ids, &body.album_ids, &body.artist_ids)
        .await?;
    Ok(Json(OkResponse { ok: true }))
}

async fn set_rating(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Json(body): Json<RatingBody>,
) -> ApiResult<Json<OkResponse>> {
    state
        .services
        .collection
        .set_rating(user.id, &body.id, body.rating)
        .await?;
    Ok(Json(OkResponse { ok: true }))
}

async fn scrobble_track(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Json(body): Json<ScrobbleBody>,
) -> ApiResult<Json<OkResponse>> {
    let ts = body
        .played_at_ms
        .and_then(|ms| Timestamp::from_millisecond(ms).ok());
    state
        .services
        .collection
        .scrobble(
            &user,
            vec![(body.track_id, ts)],
            body.submission,
            "xmedia-web",
        )
        .await?;
    Ok(Json(OkResponse { ok: true }))
}

async fn list_now_playing(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
) -> ApiResult<Json<Vec<NowPlayingDto>>> {
    Ok(Json(
        state.services.collection.list_now_playing(user.id).await?,
    ))
}

async fn get_play_queue(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
) -> ApiResult<Json<Option<PlayQueueDto>>> {
    Ok(Json(
        state.services.collection.get_play_queue(user.id).await?,
    ))
}

async fn save_play_queue(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Json(body): Json<SaveQueueBody>,
) -> ApiResult<Json<OkResponse>> {
    state
        .services
        .collection
        .save_play_queue(
            user.id,
            body.track_ids,
            body.current_track_id,
            body.position_ms,
            "xmedia-web",
        )
        .await?;
    Ok(Json(OkResponse { ok: true }))
}

async fn list_bookmarks(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
) -> ApiResult<Json<Vec<BookmarkDto>>> {
    Ok(Json(
        state.services.collection.list_bookmarks(user.id).await?,
    ))
}

async fn upsert_bookmark(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Path(track_id): Path<Uuid>,
    Json(body): Json<BookmarkBody>,
) -> ApiResult<Response> {
    let dto = state
        .services
        .collection
        .upsert_bookmark(user.id, TrackId(track_id), body.position_ms, body.comment)
        .await?;
    Ok((StatusCode::OK, Json(dto)).into_response())
}

async fn delete_bookmark(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Path(track_id): Path<Uuid>,
) -> ApiResult<Json<OkResponse>> {
    state
        .services
        .collection
        .delete_bookmark(user.id, TrackId(track_id))
        .await?;
    Ok(Json(OkResponse { ok: true }))
}

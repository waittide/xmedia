mod collections;

use axum::extract::{DefaultBodyLimit, Multipart, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use xmedia_core::application::bindings::{BindMediaItemsResultDto, UnbindMediaItemResultDto};
use xmedia_core::domain::error::DomainError;
use xmedia_core::domain::library::LibraryId;
use xmedia_core::http::{
    ApiError, ApiResult, TempUploadFile, WebAuth, into_binary_cache_response,
    parse_http_range_spec, stream_multipart_field_to_temp_file,
};

use crate::application::dto::{
    AlbumDetailDto, AlbumDto, ArtistDetailDto, ArtistDto, BindMusicItemsInput, GlobalScanStatusDto,
    HomeOverviewDto, PoolTrackDto, SearchResultDto, TrackDto, UploadBatchResultDto,
    UploadFilePathItem,
};
use crate::application::ports::FfmpegStatusDto;
use crate::domain::catalog::{
    AlbumId, AlbumListQuery, AlbumListType, ArtistId, ArtistQuery, ArtistRoleFilter, AudioFormat,
    GenreStat, MusicTagStat, PagedResult, SearchQuery, SortOrder, StructuredLyrics, TrackId,
    TrackQuery, TrackSortField,
};
use crate::http::into_audio_http_response;
use crate::http::state::MusicHttpState;
use crate::http::subsonic::envelope::SUBSONIC_API_VERSION;

use collections::{
    create_playlist, delete_bookmark, delete_playlist, get_play_queue, get_playlist, get_starred,
    list_bookmarks, list_now_playing, list_playlists, save_play_queue, scrobble_track, set_rating,
    star_items, unstar_items, update_playlist, upsert_bookmark,
};

pub fn router() -> Router<MusicHttpState> {
    Router::new()
        .route("/status", get(music_status))
        .route("/home", get(home_overview))
        .route("/pool", get(list_pool_tracks))
        .route("/bindings", post(bind_tracks))
        .route("/bindings/{id}", axum::routing::delete(unbind_track))
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
pub(super) struct OkResponse {
    pub(super) ok: bool,
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

async fn music_status(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
) -> ApiResult<Json<MusicStatusResponse>> {
    let ffmpeg =
        MusicFfmpegStatusResponse::from(state.services.streaming.get_ffmpeg_status().await);
    let scan = state.services.ingest.get_scan_status(user.id).await?;
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
    user.ensure_admin()?;
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
        if !ext.eq_ignore_ascii_case("lrc") && ext.parse::<AudioFormat>().is_err() {
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

async fn list_pool_tracks(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Query(p): Query<TrackListParams>,
) -> ApiResult<Json<PagedResult<PoolTrackDto>>> {
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
        exclude_online_tracked: true,
    };
    Ok(Json(
        state.services.ingest.list_pool_tracks(user.id, q).await?,
    ))
}

async fn bind_tracks(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Json(input): Json<BindMusicItemsInput>,
) -> ApiResult<Json<BindMediaItemsResultDto>> {
    Ok(Json(state.services.ingest.bind_tracks(&user, input).await?))
}

#[derive(Deserialize, Default)]
struct UnbindQuery {
    user_id: Option<Uuid>,
}

async fn unbind_track(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
    Query(query): Query<UnbindQuery>,
) -> ApiResult<Json<UnbindMediaItemResultDto>> {
    Ok(Json(
        state
            .services
            .ingest
            .unbind_track(&user, TrackId(id), query.user_id)
            .await?,
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
        exclude_online_tracked: false,
    };
    Ok(Json(state.services.tracks.query_tracks(user.id, q).await?))
}

async fn recent_tracks(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Query(p): Query<RecentTrackParams>,
) -> ApiResult<Json<Vec<TrackDto>>> {
    Ok(Json(
        state
            .services
            .tracks
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
            .tracks
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
            .tracks
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
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<StructuredLyrics>> {
    state
        .services
        .online
        .ensure_tracked_lyrics_cached(user.id, TrackId(id))
        .await?;
    Ok(Json(
        state
            .services
            .streaming
            .get_lyrics_by_track(user.id, TrackId(id))
            .await?,
    ))
}

async fn stream_track(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
    Query(p): Query<StreamParams>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    state
        .services
        .online
        .ensure_tracked_track_cached(user.id, TrackId(id))
        .await?;
    let spec = parse_http_range_spec(&headers);
    let resolved = state
        .services
        .streaming
        .open_stream(
            user.id,
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
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    state
        .services
        .online
        .ensure_tracked_track_cached(user.id, TrackId(id))
        .await?;
    let spec = parse_http_range_spec(&headers);
    let resolved = state
        .services
        .streaming
        .open_raw_download(user.id, TrackId(id), spec)
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
        .streaming
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
        Some(raw) if !raw.trim().is_empty() => raw.parse::<AlbumListType>()?,
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
    Ok(Json(state.services.browse.list_albums(user.id, q).await?))
}

async fn get_album(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<AlbumDetailDto>> {
    Ok(Json(
        state
            .services
            .browse
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
    Ok(Json(state.services.browse.list_artists(user.id, q).await?))
}

async fn get_artist(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<ArtistDetailDto>> {
    Ok(Json(
        state
            .services
            .browse
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
    WebAuth(user): WebAuth,
) -> ApiResult<Json<Vec<GenreStat>>> {
    Ok(Json(state.services.catalog.list_genres(user.id).await?))
}

async fn list_tags(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
) -> ApiResult<Json<Vec<MusicTagStat>>> {
    Ok(Json(state.services.catalog.list_tags(user.id).await?))
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

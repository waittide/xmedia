use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use jiff::Timestamp;
use serde::Deserialize;
use uuid::Uuid;
use xmedia_core::http::{ApiResult, WebAuth};

use super::OkResponse;
use crate::application::dto::{
    BookmarkDto, NowPlayingDto, PlayQueueDto, PlaylistDetailDto, PlaylistSummaryDto,
    StarredBundleDto, UpdatePlaylistInput,
};
use crate::domain::catalog::{AlbumId, ArtistId, TrackId};
use crate::domain::collection::PlaylistId;
use crate::http::state::MusicHttpState;

fn default_true() -> bool {
    true
}

#[derive(Deserialize)]
pub(super) struct CreatePlaylistBody {
    name: String,
    comment: Option<String>,
    #[serde(default)]
    is_public: bool,
    #[serde(default)]
    track_ids: Vec<TrackId>,
}

#[derive(Deserialize, Default)]
pub(super) struct StarBody {
    #[serde(default)]
    ids: Vec<String>,
    #[serde(default)]
    album_ids: Vec<AlbumId>,
    #[serde(default)]
    artist_ids: Vec<ArtistId>,
}

#[derive(Deserialize)]
pub(super) struct RatingBody {
    id: String,
    rating: u8,
}

#[derive(Deserialize)]
pub(super) struct ScrobbleBody {
    track_id: TrackId,
    #[serde(default = "default_true")]
    submission: bool,
    played_at_ms: Option<i64>,
}

#[derive(Deserialize)]
pub(super) struct SaveQueueBody {
    #[serde(default)]
    track_ids: Vec<TrackId>,
    current_track_id: Option<TrackId>,
    #[serde(default)]
    position_ms: u64,
}

#[derive(Deserialize)]
pub(super) struct BookmarkBody {
    position_ms: u64,
    comment: Option<String>,
}

pub(super) async fn list_playlists(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
) -> ApiResult<Json<Vec<PlaylistSummaryDto>>> {
    Ok(Json(
        state.services.collection.list_playlists(user.id).await?,
    ))
}

pub(super) async fn create_playlist(
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

pub(super) async fn get_playlist(
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

pub(super) async fn update_playlist(
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

pub(super) async fn delete_playlist(
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

pub(super) async fn get_starred(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
) -> ApiResult<Json<StarredBundleDto>> {
    Ok(Json(
        state.services.collection.get_starred(user.id, None).await?,
    ))
}

pub(super) async fn star_items(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Json(body): Json<StarBody>,
) -> ApiResult<Json<OkResponse>> {
    state
        .services
        .collection
        .set_starred(user.id, true, &body.ids, &body.album_ids, &body.artist_ids)
        .await?;
    Ok(Json(OkResponse { ok: true }))
}

pub(super) async fn unstar_items(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Json(body): Json<StarBody>,
) -> ApiResult<Json<OkResponse>> {
    state
        .services
        .collection
        .set_starred(user.id, false, &body.ids, &body.album_ids, &body.artist_ids)
        .await?;
    Ok(Json(OkResponse { ok: true }))
}

pub(super) async fn set_rating(
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

pub(super) async fn scrobble_track(
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

pub(super) async fn list_now_playing(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
) -> ApiResult<Json<Vec<NowPlayingDto>>> {
    Ok(Json(
        state.services.collection.list_now_playing(user.id).await?,
    ))
}

pub(super) async fn get_play_queue(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
) -> ApiResult<Json<Option<PlayQueueDto>>> {
    Ok(Json(
        state.services.collection.get_play_queue(user.id).await?,
    ))
}

pub(super) async fn save_play_queue(
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

pub(super) async fn list_bookmarks(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
) -> ApiResult<Json<Vec<BookmarkDto>>> {
    Ok(Json(
        state.services.collection.list_bookmarks(user.id).await?,
    ))
}

pub(super) async fn upsert_bookmark(
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

pub(super) async fn delete_bookmark(
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

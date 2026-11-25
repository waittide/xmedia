mod items;
mod shows_discovery;

use std::collections::HashSet;

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::{LibraryId, MediaKind};
use xmedia_core::domain::user::User;

use crate::domain::id::{ShowId, VideoId};
use crate::domain::video::{ShowQueryFilter, VideoKindFilter, VideoQueryFilter};
use crate::domain::watch::VideoFavoriteKind;
use crate::http::state::VideoHttpState;

use super::dto::{
    build_library_view_item, build_season_item, build_series_item, build_video_summary_item,
    build_video_user_data, format_jf_id, season_uuid,
};

pub use items::{handle_counts, handle_items, handle_latest, handle_resume};
pub use shows_discovery::{
    handle_genres, handle_next_up, handle_search_hints, handle_show_episodes, handle_show_seasons,
};

fn parse_csv_lower(raw: Option<&str>) -> HashSet<String> {
    raw.unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

pub(super) async fn resolve_synthetic_season(
    state: &VideoHttpState,
    target_uuid: Uuid,
) -> DomainResult<Option<(ShowId, u32)>> {
    let shows = state
        .services
        .catalog
        .list_raw_shows(&ShowQueryFilter::default())
        .await?;
    for show in shows {
        let max_s = show.season_count.max(1) + 2;
        for s_num in 0..=max_s {
            if season_uuid(show.id, s_num) == target_uuid {
                return Ok(Some((show.id, s_num)));
            }
        }
    }
    Ok(None)
}

pub async fn handle_views(state: &VideoHttpState, user: &User) -> DomainResult<Response> {
    let libs = state
        .libraries
        .list_libraries(Some(MediaKind::Video))
        .await?;
    let all_movies = state
        .services
        .catalog
        .list_videos(
            user,
            VideoQueryFilter {
                kind: VideoKindFilter::Standalone,
                ..Default::default()
            },
        )
        .await?;
    let all_shows = state
        .services
        .catalog
        .list_shows(user, ShowQueryFilter::default())
        .await?;

    let mut items = Vec::with_capacity(libs.len());
    for lib in libs {
        let movie_cnt = all_movies.iter().filter(|m| m.library_id == lib.id).count();
        let show_cnt = all_shows.iter().filter(|s| s.library_id == lib.id).count();
        if movie_cnt + show_cnt == 0 {
            continue;
        }
        let collection_type = if show_cnt > 0 && movie_cnt == 0 {
            Some("tvshows")
        } else if movie_cnt > 0 && show_cnt == 0 {
            Some("movies")
        } else {
            None
        };
        items.push(build_library_view_item(
            &lib,
            collection_type,
            movie_cnt + show_cnt,
        ));
    }

    let total = items.len();
    Ok((
        StatusCode::OK,
        Json(json!({
            "Items": items,
            "TotalRecordCount": total,
            "StartIndex": 0
        })),
    )
        .into_response())
}

pub async fn handle_item_detail(
    state: &VideoHttpState,
    user: &User,
    raw_item_id: &str,
    api_key: &str,
) -> DomainResult<Response> {
    let uuid = Uuid::parse_str(raw_item_id.trim())
        .map_err(|_| DomainError::NotFound("invalid item id".into()))?;

    if let Some(raw_video) = state.services.catalog.find_raw_video(VideoId(uuid)).await? {
        let detail = state
            .services
            .catalog
            .get_video_detail(user, VideoId(uuid))
            .await?;
        let dto = build_video_summary_item(&detail.video, Some(&detail), Some(&raw_video), api_key);
        return Ok((StatusCode::OK, Json(dto)).into_response());
    }

    if state
        .services
        .catalog
        .find_raw_show(ShowId(uuid))
        .await?
        .is_some()
    {
        let detail = state
            .services
            .catalog
            .get_show_detail(user, ShowId(uuid))
            .await?;
        return Ok((StatusCode::OK, Json(build_series_item(&detail.show))).into_response());
    }

    if let Ok(lib) = state.libraries.get_library(LibraryId(uuid)).await
        && lib.media_kind == MediaKind::Video
    {
        let movie_cnt = state
            .services
            .catalog
            .list_videos(
                user,
                VideoQueryFilter {
                    library_id: Some(lib.id),
                    kind: VideoKindFilter::Standalone,
                    ..Default::default()
                },
            )
            .await?
            .len();
        let show_cnt = state
            .services
            .catalog
            .list_shows(
                user,
                ShowQueryFilter {
                    library_id: Some(lib.id),
                    ..Default::default()
                },
            )
            .await?
            .len();
        let total_cnt = movie_cnt + show_cnt;
        if total_cnt > 0 {
            let collection_type = if show_cnt > 0 && movie_cnt == 0 {
                Some("tvshows")
            } else if movie_cnt > 0 && show_cnt == 0 {
                Some("movies")
            } else {
                None
            };
            let dto = build_library_view_item(&lib, collection_type, total_cnt);
            return Ok((StatusCode::OK, Json(dto)).into_response());
        }
    }

    if let Some((show_id, season_num)) = resolve_synthetic_season(state, uuid).await? {
        let detail = state
            .services
            .catalog
            .get_show_detail(user, show_id)
            .await?;
        if let Some(season) = detail
            .seasons
            .iter()
            .find(|s| s.season_number == season_num)
        {
            return Ok((
                StatusCode::OK,
                Json(build_season_item(&detail.show, season)),
            )
                .into_response());
        }
    }

    Err(DomainError::NotFound("item not found".into()))
}

pub async fn handle_played_item(
    state: &VideoHttpState,
    user: &User,
    raw_item_id: &str,
    played: bool,
) -> DomainResult<Response> {
    let uuid = Uuid::parse_str(raw_item_id.trim())
        .map_err(|_| DomainError::NotFound("invalid item id".into()))?;

    if state
        .services
        .catalog
        .find_raw_video(VideoId(uuid))
        .await?
        .is_some()
    {
        state
            .services
            .watch
            .set_watched(user, VideoId(uuid), played)
            .await?;
        let detail = state
            .services
            .catalog
            .get_video_detail(user, VideoId(uuid))
            .await?;
        return Ok((StatusCode::OK, Json(build_video_user_data(&detail.video))).into_response());
    }

    if state
        .services
        .catalog
        .find_raw_show(ShowId(uuid))
        .await?
        .is_some()
    {
        let detail = state
            .services
            .catalog
            .get_show_detail(user, ShowId(uuid))
            .await?;
        for season in &detail.seasons {
            for ep in &season.episodes {
                let _ = state.services.watch.set_watched(user, ep.id, played).await;
            }
        }
        return Ok((
            StatusCode::OK,
            Json(json!({
                "PlaybackPositionTicks": 0,
                "PlayCount": if played { 1 } else { 0 },
                "IsFavorite": detail.show.is_favorite,
                "Played": played,
                "Key": format_jf_id(uuid)
            })),
        )
            .into_response());
    }

    Err(DomainError::NotFound("item not found".into()))
}

pub async fn handle_favorite_item(
    state: &VideoHttpState,
    user: &User,
    raw_item_id: &str,
    favorite: bool,
) -> DomainResult<Response> {
    let uuid = Uuid::parse_str(raw_item_id.trim())
        .map_err(|_| DomainError::NotFound("invalid item id".into()))?;

    if state
        .services
        .catalog
        .find_raw_video(VideoId(uuid))
        .await?
        .is_some()
    {
        state
            .services
            .watch
            .set_favorite(user, VideoFavoriteKind::Video, uuid, favorite)
            .await?;
        let detail = state
            .services
            .catalog
            .get_video_detail(user, VideoId(uuid))
            .await?;
        return Ok((StatusCode::OK, Json(build_video_user_data(&detail.video))).into_response());
    }

    if state
        .services
        .catalog
        .find_raw_show(ShowId(uuid))
        .await?
        .is_some()
    {
        state
            .services
            .watch
            .set_favorite(user, VideoFavoriteKind::Show, uuid, favorite)
            .await?;
        let detail = state
            .services
            .catalog
            .get_show_detail(user, ShowId(uuid))
            .await?;
        let played = detail.show.episode_count > 0
            && detail.show.watched_episode_count >= detail.show.episode_count;
        return Ok((
            StatusCode::OK,
            Json(json!({
                "PlaybackPositionTicks": 0,
                "PlayCount": if played { 1 } else { 0 },
                "IsFavorite": detail.show.is_favorite,
                "Played": played,
                "Key": format_jf_id(uuid)
            })),
        )
            .into_response());
    }

    Err(DomainError::NotFound("item not found".into()))
}

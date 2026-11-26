mod items;
mod shows_discovery;

use std::collections::HashSet;

use axum::Json;
use axum::http::{Method, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::json;
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::{LibraryId, MediaKind};
use xmedia_core::domain::user::User;

use crate::application::dto::ResolvedCatalogEntity;
use crate::domain::video::{ShowQueryFilter, VideoKindFilter, VideoQueryFilter};
use crate::domain::watch::VideoFavoriteKind;
use crate::http::state::VideoHttpState;

use super::dto::{
    build_library_view_item, build_season_item, build_series_item, build_video_summary_item,
    build_video_user_data, format_jf_id,
};
use super::{JfQuery, verify_user_scope};

fn parse_csv_lower(raw: Option<&str>) -> HashSet<String> {
    raw.unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

pub(super) async fn try_dispatch(
    state: &VideoHttpState,
    user: &User,
    api_key: &str,
    method: &Method,
    segs: &[&str],
    raw_slice: &[String],
    query: &JfQuery,
) -> Option<DomainResult<Response>> {
    let res = match (method, segs) {
        // Library Views
        (&Method::GET, ["userviews"]) => handle_views(state, user).await,
        (&Method::GET, ["users", _, "views"]) => match verify_user_scope(user, &raw_slice[1]) {
            Ok(()) => handle_views(state, user).await,
            Err(e) => Err(e),
        },
        (&Method::GET, ["users", _, "groupingoptions"]) => {
            match verify_user_scope(user, &raw_slice[1]) {
                Ok(()) => {
                    Ok((StatusCode::OK, Json(Vec::<serde_json::Value>::new())).into_response())
                }
                Err(e) => Err(e),
            }
        }

        // Items listing / resume / latest / counts / detail
        (&Method::GET, ["items"]) => items::handle_items(state, user, query, api_key).await,
        (&Method::GET, ["users", _, "items"]) => match verify_user_scope(user, &raw_slice[1]) {
            Ok(()) => items::handle_items(state, user, query, api_key).await,
            Err(e) => Err(e),
        },
        (&Method::GET, ["useritems", "resume"]) => {
            items::handle_resume(state, user, query, api_key).await
        }
        (&Method::GET, ["users", _, "items", "resume"]) => {
            match verify_user_scope(user, &raw_slice[1]) {
                Ok(()) => items::handle_resume(state, user, query, api_key).await,
                Err(e) => Err(e),
            }
        }
        (&Method::GET, ["items", "latest"]) => {
            items::handle_latest(state, user, query, api_key).await
        }
        (&Method::GET, ["users", _, "items", "latest"]) => {
            match verify_user_scope(user, &raw_slice[1]) {
                Ok(()) => items::handle_latest(state, user, query, api_key).await,
                Err(e) => Err(e),
            }
        }
        (&Method::GET, ["items", "counts"]) => items::handle_counts(state, user).await,
        (&Method::GET, ["users", _, "items", "counts"]) => {
            match verify_user_scope(user, &raw_slice[1]) {
                Ok(()) => items::handle_counts(state, user).await,
                Err(e) => Err(e),
            }
        }
        (&Method::GET, ["items", _]) => {
            handle_item_detail(state, user, &raw_slice[1], api_key).await
        }
        (&Method::GET, ["users", _, "items", _]) => match verify_user_scope(user, &raw_slice[1]) {
            Ok(()) => handle_item_detail(state, user, &raw_slice[3], api_key).await,
            Err(e) => Err(e),
        },

        // Shows: Seasons, Episodes, NextUp
        (&Method::GET, ["shows", _, "seasons"]) => {
            shows_discovery::handle_show_seasons(state, user, &raw_slice[1]).await
        }
        (&Method::GET, ["shows", _, "episodes"]) => {
            shows_discovery::handle_show_episodes(state, user, &raw_slice[1], query, api_key).await
        }
        (&Method::GET, ["shows", "nextup"]) => {
            shows_discovery::handle_next_up(state, user, query, api_key).await
        }

        // Genres & Search Hints
        (&Method::GET, ["genres"]) | (&Method::GET, ["musicgenres"]) => {
            shows_discovery::handle_genres(state, user).await
        }
        (&Method::GET, ["search", "hints"]) => {
            shows_discovery::handle_search_hints(state, user, query).await
        }

        // Played / Watched status sync
        (&Method::POST, ["users", _, "playeditems", _]) => {
            match verify_user_scope(user, &raw_slice[1]) {
                Ok(()) => handle_played_item(state, user, &raw_slice[3], true).await,
                Err(e) => Err(e),
            }
        }
        (&Method::DELETE, ["users", _, "playeditems", _]) => {
            match verify_user_scope(user, &raw_slice[1]) {
                Ok(()) => handle_played_item(state, user, &raw_slice[3], false).await,
                Err(e) => Err(e),
            }
        }
        (&Method::POST, ["userplayeditems", _]) => {
            handle_played_item(state, user, &raw_slice[1], true).await
        }
        (&Method::DELETE, ["userplayeditems", _]) => {
            handle_played_item(state, user, &raw_slice[1], false).await
        }

        // Favorites sync
        (&Method::POST, ["users", _, "favoriteitems", _]) => {
            match verify_user_scope(user, &raw_slice[1]) {
                Ok(()) => handle_favorite_item(state, user, &raw_slice[3], true).await,
                Err(e) => Err(e),
            }
        }
        (&Method::DELETE, ["users", _, "favoriteitems", _]) => {
            match verify_user_scope(user, &raw_slice[1]) {
                Ok(()) => handle_favorite_item(state, user, &raw_slice[3], false).await,
                Err(e) => Err(e),
            }
        }
        (&Method::POST, ["userfavoriteitems", _]) => {
            handle_favorite_item(state, user, &raw_slice[1], true).await
        }
        (&Method::DELETE, ["userfavoriteitems", _]) => {
            handle_favorite_item(state, user, &raw_slice[1], false).await
        }

        _ => return None,
    };
    Some(res)
}

async fn handle_views(state: &VideoHttpState, user: &User) -> DomainResult<Response> {
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
        .shows
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

async fn handle_item_detail(
    state: &VideoHttpState,
    user: &User,
    raw_item_id: &str,
    api_key: &str,
) -> DomainResult<Response> {
    let uuid = Uuid::parse_str(raw_item_id.trim())
        .map_err(|_| DomainError::NotFound("invalid item id".into()))?;

    match state.services.shows.resolve_item_kind(uuid).await? {
        Some(ResolvedCatalogEntity::Video(raw_video)) => {
            let detail = state
                .services
                .catalog
                .get_video_detail(user, raw_video.id)
                .await?;
            let dto =
                build_video_summary_item(&detail.video, Some(&detail), Some(&raw_video), api_key);
            return Ok((StatusCode::OK, Json(dto)).into_response());
        }
        Some(ResolvedCatalogEntity::Show(show)) => {
            let detail = state.services.shows.get_show_detail(user, show.id).await?;
            return Ok((StatusCode::OK, Json(build_series_item(&detail.show))).into_response());
        }
        Some(ResolvedCatalogEntity::Season {
            show,
            season_number,
        }) => {
            let detail = state.services.shows.get_show_detail(user, show.id).await?;
            if let Some(season) = detail
                .seasons
                .iter()
                .find(|s| s.season_number == season_number)
            {
                return Ok((
                    StatusCode::OK,
                    Json(build_season_item(&detail.show, season)),
                )
                    .into_response());
            }
        }
        None => {}
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
            .shows
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

    Err(DomainError::NotFound("item not found".into()))
}

async fn handle_played_item(
    state: &VideoHttpState,
    user: &User,
    raw_item_id: &str,
    played: bool,
) -> DomainResult<Response> {
    let uuid = Uuid::parse_str(raw_item_id.trim())
        .map_err(|_| DomainError::NotFound("invalid item id".into()))?;

    match state.services.shows.resolve_item_kind(uuid).await? {
        Some(ResolvedCatalogEntity::Video(video)) => {
            state
                .services
                .watch
                .set_watched(user, video.id, played)
                .await?;
            let detail = state
                .services
                .catalog
                .get_video_detail(user, video.id)
                .await?;
            Ok((StatusCode::OK, Json(build_video_user_data(&detail.video))).into_response())
        }
        Some(ResolvedCatalogEntity::Show(show)) => {
            let detail = state.services.shows.get_show_detail(user, show.id).await?;
            for season in &detail.seasons {
                for ep in &season.episodes {
                    let _ = state.services.watch.set_watched(user, ep.id, played).await;
                }
            }
            Ok((
                StatusCode::OK,
                Json(json!({
                    "PlaybackPositionTicks": 0,
                    "PlayCount": if played { 1 } else { 0 },
                    "IsFavorite": detail.show.is_favorite,
                    "Played": played,
                    "Key": format_jf_id(uuid)
                })),
            )
                .into_response())
        }
        _ => Err(DomainError::NotFound("item not found".into())),
    }
}

async fn handle_favorite_item(
    state: &VideoHttpState,
    user: &User,
    raw_item_id: &str,
    favorite: bool,
) -> DomainResult<Response> {
    let uuid = Uuid::parse_str(raw_item_id.trim())
        .map_err(|_| DomainError::NotFound("invalid item id".into()))?;

    match state.services.shows.resolve_item_kind(uuid).await? {
        Some(ResolvedCatalogEntity::Video(video)) => {
            state
                .services
                .watch
                .set_favorite(user, VideoFavoriteKind::Video, uuid, favorite)
                .await?;
            let detail = state
                .services
                .catalog
                .get_video_detail(user, video.id)
                .await?;
            Ok((StatusCode::OK, Json(build_video_user_data(&detail.video))).into_response())
        }
        Some(ResolvedCatalogEntity::Show(show)) => {
            state
                .services
                .watch
                .set_favorite(user, VideoFavoriteKind::Show, uuid, favorite)
                .await?;
            let detail = state.services.shows.get_show_detail(user, show.id).await?;
            let played = detail.show.episode_count > 0
                && detail.show.watched_episode_count >= detail.show.episode_count;
            Ok((
                StatusCode::OK,
                Json(json!({
                    "PlaybackPositionTicks": 0,
                    "PlayCount": if played { 1 } else { 0 },
                    "IsFavorite": detail.show.is_favorite,
                    "Played": played,
                    "Key": format_jf_id(uuid)
                })),
            )
                .into_response())
        }
        _ => Err(DomainError::NotFound("item not found".into())),
    }
}

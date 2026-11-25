use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::LibraryId;
use xmedia_core::domain::user::User;

use crate::domain::id::ShowId;
use crate::domain::video::{ShowQueryFilter, VideoQueryFilter};
use crate::http::state::VideoHttpState;

use super::super::JfQuery;
use super::super::dto::{
    JELLYFIN_SERVER_ID, build_search_hint_series, build_search_hint_video, build_season_item,
    build_video_summary_item, season_uuid,
};

pub async fn handle_show_seasons(
    state: &VideoHttpState,
    user: &User,
    raw_series_id: &str,
) -> DomainResult<Response> {
    let uuid = Uuid::parse_str(raw_series_id.trim())
        .map_err(|_| DomainError::NotFound("invalid series id".into()))?;
    let detail = state
        .services
        .catalog
        .get_show_detail(user, ShowId(uuid))
        .await?;
    let items: Vec<Value> = detail
        .seasons
        .iter()
        .map(|s| build_season_item(&detail.show, s))
        .collect();
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

pub async fn handle_show_episodes(
    state: &VideoHttpState,
    user: &User,
    raw_series_id: &str,
    query: &JfQuery,
    api_key: &str,
) -> DomainResult<Response> {
    let uuid = Uuid::parse_str(raw_series_id.trim())
        .map_err(|_| DomainError::NotFound("invalid series id".into()))?;
    let detail = state
        .services
        .catalog
        .get_show_detail(user, ShowId(uuid))
        .await?;

    let season_filter_num = query
        .get("season")
        .and_then(|s| s.trim().parse::<u32>().ok());
    let season_filter_id = query
        .get("seasonid")
        .and_then(|s| Uuid::parse_str(s.trim()).ok());

    let mut eps: Vec<Value> = Vec::new();
    for season in &detail.seasons {
        if let Some(sn) = season_filter_num
            && season.season_number != sn
        {
            continue;
        }
        if let Some(sid) = season_filter_id
            && season_uuid(detail.show.id, season.season_number) != sid
        {
            continue;
        }
        for ep in &season.episodes {
            eps.push(build_video_summary_item(ep, None, None, api_key));
        }
    }

    let start_index = query.usize_param("startindex").unwrap_or(0);
    let limit = query.usize_param("limit").map(|l| l.clamp(1, 500));
    let total = eps.len();
    let page: Vec<Value> = eps
        .into_iter()
        .skip(start_index)
        .take(limit.unwrap_or(total))
        .collect();

    Ok((
        StatusCode::OK,
        Json(json!({
            "Items": page,
            "TotalRecordCount": total,
            "StartIndex": start_index
        })),
    )
        .into_response())
}

pub async fn handle_next_up(
    state: &VideoHttpState,
    user: &User,
    query: &JfQuery,
    api_key: &str,
) -> DomainResult<Response> {
    let limit = query.usize_param("limit").unwrap_or(24).clamp(1, 100);
    let series_id = query
        .get("seriesid")
        .and_then(|s| Uuid::parse_str(s.trim()).ok())
        .map(ShowId);
    let parent_lib = query
        .get("parentid")
        .and_then(|s| Uuid::parse_str(s.trim()).ok())
        .map(LibraryId);

    let mut items = Vec::new();
    if let Some(sid) = series_id {
        if let Ok(detail) = state.services.catalog.get_show_detail(user, sid).await
            && let Some(ep) = detail.continue_episode
        {
            items.push(build_video_summary_item(&ep, None, None, api_key));
        }
    } else {
        let shows = state
            .services
            .catalog
            .list_shows(
                user,
                ShowQueryFilter {
                    library_id: parent_lib,
                    ..Default::default()
                },
            )
            .await?;
        for s in shows {
            if s.watched_episode_count == 0 {
                continue;
            }
            if let Ok(detail) = state.services.catalog.get_show_detail(user, s.id).await
                && let Some(ep) = detail.continue_episode
                && !ep.progress.as_ref().is_some_and(|p| p.completed)
            {
                items.push(build_video_summary_item(&ep, None, None, api_key));
            }
            if items.len() >= limit {
                break;
            }
        }
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

pub async fn handle_genres(state: &VideoHttpState, user: &User) -> DomainResult<Response> {
    let taxonomy = state.services.catalog.get_taxonomy(user.id).await?;
    let items: Vec<Value> = taxonomy
        .genres
        .into_iter()
        .map(|g| {
            let hex = xmedia_core::infra::stream::sha256_hex(
                format!("xmedia:jf:genre:{}", g.name.to_lowercase()).as_bytes(),
            );
            json!({
                "Name": g.name,
                "ServerId": JELLYFIN_SERVER_ID,
                "Id": &hex[..32],
                "Type": "Genre",
                "IsFolder": true,
                "ChildCount": g.count
            })
        })
        .collect();
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

pub async fn handle_search_hints(
    state: &VideoHttpState,
    user: &User,
    query: &JfQuery,
) -> DomainResult<Response> {
    let term = query.get("searchterm").unwrap_or_default().trim();
    let limit = query.usize_param("limit").unwrap_or(24).clamp(1, 100);
    if term.is_empty() {
        return Ok((
            StatusCode::OK,
            Json(json!({ "SearchHints": [], "TotalRecordCount": 0 })),
        )
            .into_response());
    }

    let term_lower = term.to_lowercase();
    let videos = state
        .services
        .catalog
        .list_videos(user, VideoQueryFilter::default())
        .await?;
    let shows = state
        .services
        .catalog
        .list_shows(user, ShowQueryFilter::default())
        .await?;

    let mut hints = Vec::new();
    for s in shows {
        if s.title.to_lowercase().contains(&term_lower) {
            hints.push(build_search_hint_series(&s));
        }
    }
    for v in videos {
        if v.title.to_lowercase().contains(&term_lower)
            || v.show_title
                .as_deref()
                .is_some_and(|st| st.to_lowercase().contains(&term_lower))
        {
            hints.push(build_search_hint_video(&v));
        }
    }

    let total = hints.len();
    hints.truncate(limit);
    Ok((
        StatusCode::OK,
        Json(json!({
            "SearchHints": hints,
            "TotalRecordCount": total
        })),
    )
        .into_response())
}

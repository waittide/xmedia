use std::collections::HashSet;

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};
use uuid::Uuid;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::domain::library::LibraryId;
use xmedia_core::domain::user::User;

use crate::application::dto::{ShowSummaryDto, VideoSummaryDto};
use crate::domain::id::{ShowId, VideoId};
use crate::domain::video::{ShowQueryFilter, VideoKindFilter, VideoQueryFilter};
use crate::http::state::VideoHttpState;

use super::super::JfQuery;
use super::super::dto::{build_season_item, build_series_item, build_video_summary_item};
use super::{parse_csv_lower, resolve_synthetic_season};

struct SortableJfItem {
    sort_name: String,
    date_created: i64,
    premiere_year: i32,
    date_played: i64,
    index_num: u64,
    json: Value,
}

pub async fn handle_items(
    state: &VideoHttpState,
    user: &User,
    query: &JfQuery,
    api_key: &str,
) -> DomainResult<Response> {
    let start_index = query.usize_param("startindex").unwrap_or(0);
    let limit = query.usize_param("limit").map(|l| l.clamp(1, 500));

    if let Some(raw_ids) = query.get("ids").filter(|s| !s.trim().is_empty()) {
        let mut out = Vec::new();
        for part in raw_ids.split(',') {
            if let Ok(uuid) = Uuid::parse_str(part.trim()) {
                if let Some(raw_video) =
                    state.services.catalog.find_raw_video(VideoId(uuid)).await?
                    && let Ok(detail) = state
                        .services
                        .catalog
                        .get_video_detail(user, VideoId(uuid))
                        .await
                {
                    out.push(build_video_summary_item(
                        &detail.video,
                        Some(&detail),
                        Some(&raw_video),
                        api_key,
                    ));
                } else if state
                    .services
                    .catalog
                    .find_raw_show(ShowId(uuid))
                    .await?
                    .is_some()
                    && let Ok(detail) = state
                        .services
                        .catalog
                        .get_show_detail(user, ShowId(uuid))
                        .await
                {
                    out.push(build_series_item(&detail.show));
                }
            }
        }
        let total = out.len();
        let page: Vec<Value> = out
            .into_iter()
            .skip(start_index)
            .take(limit.unwrap_or(total))
            .collect();
        return Ok((
            StatusCode::OK,
            Json(json!({
                "Items": page,
                "TotalRecordCount": total,
                "StartIndex": start_index
            })),
        )
            .into_response());
    }

    let include_types = parse_csv_lower(query.get("includeitemtypes"));
    let exclude_types = parse_csv_lower(query.get("excludeitemtypes"));
    let filters = parse_csv_lower(query.get("filters"));
    let recursive = query.bool_param("recursive").unwrap_or(false);
    let search_term = query
        .get("searchterm")
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_lowercase());
    let is_favorite_filter = query
        .bool_param("isfavorite")
        .or_else(|| filters.contains("isfavorite").then_some(true));
    let is_played_filter = query.bool_param("isplayed").or_else(|| {
        if filters.contains("isplayed") {
            Some(true)
        } else if filters.contains("isunplayed") {
            Some(false)
        } else {
            None
        }
    });
    let is_resumable_only = filters.contains("isresumable");
    let genre_filter = parse_csv_lower(query.get("genres"));
    let tag_filter = parse_csv_lower(query.get("tags"));
    let year_filter: HashSet<i32> = query
        .get("years")
        .unwrap_or_default()
        .split(',')
        .filter_map(|s| s.trim().parse::<i32>().ok())
        .collect();

    let parent_uuid = query
        .get("parentid")
        .and_then(|s| Uuid::parse_str(s.trim()).ok());

    if let Some(pid) = parent_uuid
        && state
            .services
            .catalog
            .find_raw_show(ShowId(pid))
            .await?
            .is_some()
    {
        let detail = state
            .services
            .catalog
            .get_show_detail(user, ShowId(pid))
            .await?;
        let wants_episodes = include_types.contains("episode") || recursive;
        let items: Vec<Value> = if wants_episodes {
            detail
                .seasons
                .iter()
                .flat_map(|s| s.episodes.iter())
                .map(|ep| build_video_summary_item(ep, None, None, api_key))
                .collect()
        } else {
            detail
                .seasons
                .iter()
                .map(|s| build_season_item(&detail.show, s))
                .collect()
        };
        let total = items.len();
        let page: Vec<Value> = items
            .into_iter()
            .skip(start_index)
            .take(limit.unwrap_or(total))
            .collect();
        return Ok((
            StatusCode::OK,
            Json(json!({
                "Items": page,
                "TotalRecordCount": total,
                "StartIndex": start_index
            })),
        )
            .into_response());
    }

    if let Some(pid) = parent_uuid
        && let Some((show_id, season_num)) = resolve_synthetic_season(state, pid).await?
    {
        let detail = state
            .services
            .catalog
            .get_show_detail(user, show_id)
            .await?;
        let eps: Vec<Value> = detail
            .seasons
            .into_iter()
            .find(|s| s.season_number == season_num)
            .map(|s| {
                s.episodes
                    .iter()
                    .map(|ep| build_video_summary_item(ep, None, None, api_key))
                    .collect()
            })
            .unwrap_or_default();
        let total = eps.len();
        let page: Vec<Value> = eps
            .into_iter()
            .skip(start_index)
            .take(limit.unwrap_or(total))
            .collect();
        return Ok((
            StatusCode::OK,
            Json(json!({
                "Items": page,
                "TotalRecordCount": total,
                "StartIndex": start_index
            })),
        )
            .into_response());
    }

    let library_id = parent_uuid.map(LibraryId);

    let want_movies = if include_types.is_empty() {
        !exclude_types.contains("movie")
    } else {
        (include_types.contains("movie") || include_types.contains("video"))
            && !exclude_types.contains("movie")
    };
    let want_series = if include_types.is_empty() {
        !exclude_types.contains("series") && !is_resumable_only
    } else {
        include_types.contains("series") && !exclude_types.contains("series")
    };
    let want_episodes = if include_types.is_empty() {
        (is_resumable_only
            || (recursive && (search_term.is_some() || is_favorite_filter == Some(true))))
            && !exclude_types.contains("episode")
    } else {
        (include_types.contains("episode") || include_types.contains("video"))
            && !exclude_types.contains("episode")
    };

    let mut collected: Vec<SortableJfItem> = Vec::new();

    if want_movies || want_episodes {
        let kind = if want_movies && want_episodes {
            VideoKindFilter::All
        } else if want_movies {
            VideoKindFilter::Standalone
        } else {
            VideoKindFilter::Episodes
        };
        let videos = state
            .services
            .catalog
            .list_videos(
                user,
                VideoQueryFilter {
                    library_id,
                    kind,
                    ..Default::default()
                },
            )
            .await?;

        for v in videos {
            if !matches_video_filters(
                &v,
                search_term.as_deref(),
                is_favorite_filter,
                is_played_filter,
                is_resumable_only,
                &genre_filter,
                &tag_filter,
                &year_filter,
            ) {
                continue;
            }
            let date_played = v
                .progress
                .as_ref()
                .map(|p| p.updated_at.as_second())
                .unwrap_or(0);
            let idx_num = u64::from(v.season_number.unwrap_or(0)) * 100_000
                + u64::from(v.episode_number.unwrap_or(0));
            collected.push(SortableJfItem {
                sort_name: v.title.to_lowercase(),
                date_created: v.created_at.as_second(),
                premiere_year: v.year.unwrap_or(0),
                date_played,
                index_num: idx_num,
                json: build_video_summary_item(&v, None, None, api_key),
            });
        }
    }

    if want_series {
        let shows = state
            .services
            .catalog
            .list_shows(
                user,
                ShowQueryFilter {
                    library_id,
                    ..Default::default()
                },
            )
            .await?;

        for s in shows {
            if !matches_show_filters(
                &s,
                search_term.as_deref(),
                is_favorite_filter,
                is_played_filter,
                &genre_filter,
                &tag_filter,
                &year_filter,
            ) {
                continue;
            }
            collected.push(SortableJfItem {
                sort_name: s.title.to_lowercase(),
                date_created: s.created_at.as_second(),
                premiere_year: s.year.unwrap_or(0),
                date_played: 0,
                index_num: 0,
                json: build_series_item(&s),
            });
        }
    }

    let sort_by = query
        .get("sortby")
        .and_then(|s| s.split(',').next())
        .map(|s| s.trim().to_ascii_lowercase())
        .unwrap_or_else(|| "sortname".to_string());
    let descending = query
        .get("sortorder")
        .is_some_and(|s| s.trim().eq_ignore_ascii_case("descending"));

    match sort_by.as_str() {
        "datecreated" => {
            collected.sort_by_key(|a| a.date_created);
        }
        "premieredate" | "productionyear" => {
            collected.sort_by(|a, b| {
                a.premiere_year
                    .cmp(&b.premiere_year)
                    .then_with(|| a.sort_name.cmp(&b.sort_name))
            });
        }
        "dateplayed" => {
            collected.sort_by_key(|a| a.date_played);
        }
        "indexnumber" => {
            collected.sort_by(|a, b| {
                a.index_num
                    .cmp(&b.index_num)
                    .then_with(|| a.sort_name.cmp(&b.sort_name))
            });
        }
        _ => {
            collected.sort_by(|a, b| a.sort_name.cmp(&b.sort_name));
        }
    }
    if descending {
        collected.reverse();
    }

    let total = collected.len();
    let page: Vec<Value> = collected
        .into_iter()
        .skip(start_index)
        .take(limit.unwrap_or(total))
        .map(|it| it.json)
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

#[allow(clippy::too_many_arguments)]
fn matches_video_filters(
    v: &VideoSummaryDto,
    search: Option<&str>,
    is_fav: Option<bool>,
    is_played: Option<bool>,
    is_resumable: bool,
    genres: &HashSet<String>,
    tags: &HashSet<String>,
    years: &HashSet<i32>,
) -> bool {
    if let Some(q) = search {
        let hit = v.title.to_lowercase().contains(q)
            || v.summary.to_lowercase().contains(q)
            || v.show_title
                .as_deref()
                .is_some_and(|st| st.to_lowercase().contains(q));
        if !hit {
            return false;
        }
    }
    if let Some(fav) = is_fav
        && v.is_favorite != fav
    {
        return false;
    }
    let played = v.progress.as_ref().is_some_and(|p| p.completed);
    if let Some(p_req) = is_played
        && played != p_req
    {
        return false;
    }
    if is_resumable {
        let resumable = v
            .progress
            .as_ref()
            .is_some_and(|p| !p.completed && p.position_secs > 1.0);
        if !resumable {
            return false;
        }
    }
    if !genres.is_empty()
        && !v
            .genres
            .iter()
            .any(|g| genres.contains(&g.trim().to_ascii_lowercase()))
    {
        return false;
    }
    if !tags.is_empty()
        && !v
            .tags
            .iter()
            .any(|t| tags.contains(&t.trim().to_ascii_lowercase()))
    {
        return false;
    }
    if !years.is_empty() && !v.year.is_some_and(|y| years.contains(&y)) {
        return false;
    }
    true
}

fn matches_show_filters(
    s: &ShowSummaryDto,
    search: Option<&str>,
    is_fav: Option<bool>,
    is_played: Option<bool>,
    genres: &HashSet<String>,
    tags: &HashSet<String>,
    years: &HashSet<i32>,
) -> bool {
    if let Some(q) = search {
        let hit = s.title.to_lowercase().contains(q) || s.summary.to_lowercase().contains(q);
        if !hit {
            return false;
        }
    }
    if let Some(fav) = is_fav
        && s.is_favorite != fav
    {
        return false;
    }
    let played = s.episode_count > 0 && s.watched_episode_count >= s.episode_count;
    if let Some(p_req) = is_played
        && played != p_req
    {
        return false;
    }
    if !genres.is_empty()
        && !s
            .genres
            .iter()
            .any(|g| genres.contains(&g.trim().to_ascii_lowercase()))
    {
        return false;
    }
    if !tags.is_empty()
        && !s
            .tags
            .iter()
            .any(|t| tags.contains(&t.trim().to_ascii_lowercase()))
    {
        return false;
    }
    if !years.is_empty() && !s.year.is_some_and(|y| years.contains(&y)) {
        return false;
    }
    true
}

pub async fn handle_resume(
    state: &VideoHttpState,
    user: &User,
    query: &JfQuery,
    api_key: &str,
) -> DomainResult<Response> {
    let start_index = query.usize_param("startindex").unwrap_or(0);
    let limit = query.usize_param("limit").unwrap_or(24).clamp(1, 200);
    let parent_lib = query
        .get("parentid")
        .and_then(|s| Uuid::parse_str(s.trim()).ok())
        .map(LibraryId);

    let videos = state
        .services
        .catalog
        .list_videos(
            user,
            VideoQueryFilter {
                library_id: parent_lib,
                ..Default::default()
            },
        )
        .await?;

    let mut resumable: Vec<VideoSummaryDto> = videos
        .into_iter()
        .filter(|v| {
            v.progress
                .as_ref()
                .is_some_and(|p| !p.completed && p.position_secs > 1.0)
        })
        .collect();
    resumable.sort_by_key(|v| {
        std::cmp::Reverse(v.progress.as_ref().map(|p| p.updated_at.as_nanosecond()))
    });

    let total = resumable.len();
    let items: Vec<Value> = resumable
        .into_iter()
        .skip(start_index)
        .take(limit)
        .map(|v| build_video_summary_item(&v, None, None, api_key))
        .collect();

    Ok((
        StatusCode::OK,
        Json(json!({
            "Items": items,
            "TotalRecordCount": total,
            "StartIndex": start_index
        })),
    )
        .into_response())
}

pub async fn handle_latest(
    state: &VideoHttpState,
    user: &User,
    query: &JfQuery,
    api_key: &str,
) -> DomainResult<Response> {
    let limit = query.usize_param("limit").unwrap_or(16).clamp(1, 100);
    let parent_lib = query
        .get("parentid")
        .and_then(|s| Uuid::parse_str(s.trim()).ok())
        .map(LibraryId);
    let include_types = parse_csv_lower(query.get("includeitemtypes"));

    let want_movies = include_types.is_empty() || include_types.contains("movie");
    let want_series = include_types.is_empty() || include_types.contains("series");

    let mut combined: Vec<(i64, Value)> = Vec::new();
    if want_movies {
        let movies = state
            .services
            .catalog
            .list_videos(
                user,
                VideoQueryFilter {
                    library_id: parent_lib,
                    kind: VideoKindFilter::Standalone,
                    ..Default::default()
                },
            )
            .await?;
        for m in movies {
            combined.push((
                m.created_at.as_second(),
                build_video_summary_item(&m, None, None, api_key),
            ));
        }
    }
    if want_series {
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
            combined.push((s.created_at.as_second(), build_series_item(&s)));
        }
    }

    combined.sort_by_key(|(ts, _)| std::cmp::Reverse(*ts));
    let items: Vec<Value> = combined.into_iter().take(limit).map(|(_, v)| v).collect();
    Ok((StatusCode::OK, Json(items)).into_response())
}

pub async fn handle_counts(state: &VideoHttpState, user: &User) -> DomainResult<Response> {
    let overview = state.services.catalog.home_overview(user).await?;
    let episodes = overview.total_videos.saturating_sub(overview.movies_count);
    let total = overview.movies_count + overview.total_shows + episodes;
    Ok((
        StatusCode::OK,
        Json(json!({
            "MovieCount": overview.movies_count,
            "SeriesCount": overview.total_shows,
            "EpisodeCount": episodes,
            "ArtistCount": 0,
            "ProgramCount": 0,
            "TrailerCount": 0,
            "SongCount": 0,
            "AlbumCount": 0,
            "MusicVideoCount": 0,
            "BoxSetCount": 0,
            "BookCount": 0,
            "ItemCount": total
        })),
    )
        .into_response())
}

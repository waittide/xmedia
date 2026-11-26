use axum::Json;
use axum::body::{Body, Bytes};
use axum::http::header::{
    CACHE_CONTROL, CONTENT_SECURITY_POLICY, CONTENT_TYPE, RANGE, X_CONTENT_TYPE_OPTIONS,
};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::user::User;
use xmedia_core::http::{into_binary_cache_response, parse_http_range_spec};

use crate::application::dto::{ResolvedCatalogEntity, SaveWatchProgressInput, SubtitleTrackDto};
use crate::application::online::{HlsProxyOutcome, ProxyStreamOutcome};
use crate::domain::id::VideoId;
use crate::http::into_video_http_response;
use crate::http::online::{build_416_response, build_stream_response, encode_query_component};
use crate::http::state::VideoHttpState;

use super::JfQuery;
use super::dto::{
    build_media_source_local, build_media_source_online, build_playback_info_response,
    ticks_to_secs,
};

pub async fn handle_playback_info(
    state: &VideoHttpState,
    user: &User,
    raw_item_id: &str,
    api_key: &str,
) -> DomainResult<Response> {
    let uuid = Uuid::parse_str(raw_item_id.trim())
        .map_err(|_| DomainError::NotFound("invalid item id".into()))?;
    let video = state
        .services
        .catalog
        .require_bound_video(user.id, VideoId(uuid))
        .await?;

    let media_source = if video.source.is_cached() {
        let subs = SubtitleTrackDto::collect_from_video(&video);
        build_media_source_local(&video, &subs, api_key)
    } else {
        let (_source_id, play_res) = state
            .services
            .online
            .resolve_tracked_video_upstream(video.id)
            .await?;
        build_media_source_online(&video, &play_res, api_key)
    };

    Ok((
        StatusCode::OK,
        Json(build_playback_info_response(media_source, video.id)),
    )
        .into_response())
}

pub async fn handle_video_stream(
    state: &VideoHttpState,
    user: &User,
    raw_item_id: &str,
    headers: &HeaderMap,
    as_attachment: bool,
) -> DomainResult<Response> {
    let uuid = Uuid::parse_str(raw_item_id.trim())
        .map_err(|_| DomainError::NotFound("invalid item id".into()))?;
    let video = state
        .services
        .catalog
        .require_bound_video(user.id, VideoId(uuid))
        .await?;

    if video.source.is_cached() {
        let spec = parse_http_range_spec(headers);
        let resolved = state
            .services
            .streaming
            .open_raw_download(user.id, video.id, spec)
            .await?;
        return Ok(into_video_http_response(resolved, as_attachment));
    }

    let (source_id, play_res) = state
        .services
        .online
        .resolve_tracked_video_upstream(video.id)
        .await?;
    let range_header = headers.get(RANGE).and_then(|v| v.to_str().ok());
    match state
        .services
        .online
        .open_stream_proxy_for_tracked(&play_res.stream_url, source_id, range_header)
        .await?
    {
        ProxyStreamOutcome::Stream(payload) => Ok(build_stream_response(payload, true)),
        ProxyStreamOutcome::RangeNotSatisfiable { content_range } => {
            Ok(build_416_response(content_range))
        }
    }
}

fn build_hls_playlist_response(rewritten: String) -> Response {
    let mut resp = Response::new(Body::from(rewritten));
    resp.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("application/vnd.apple.mpegurl; charset=utf-8"),
    );
    resp.headers_mut()
        .insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    resp.headers_mut().insert(
        CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("sandbox; default-src 'none'"),
    );
    resp.headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    resp
}

pub async fn handle_video_hls_master(
    state: &VideoHttpState,
    user: &User,
    raw_item_id: &str,
    headers: &HeaderMap,
    api_key: &str,
) -> DomainResult<Response> {
    let uuid = Uuid::parse_str(raw_item_id.trim())
        .map_err(|_| DomainError::NotFound("invalid item id".into()))?;
    state
        .services
        .catalog
        .require_bound_video(user.id, VideoId(uuid))
        .await?;
    let (source_id, play_res) = state
        .services
        .online
        .resolve_tracked_video_upstream(VideoId(uuid))
        .await?;

    let range_header = headers.get(RANGE).and_then(|v| v.to_str().ok());
    let encoded_key = encode_query_component(api_key);

    match state
        .services
        .online
        .open_hls_proxy_for_tracked(
            user,
            &play_res.stream_url,
            source_id,
            range_header,
            |resolved| {
                format!(
                    "proxy?url={}&source_id={source_id}&api_key={encoded_key}",
                    encode_query_component(resolved)
                )
            },
        )
        .await?
    {
        HlsProxyOutcome::Playlist(rewritten) => Ok(build_hls_playlist_response(rewritten)),
        HlsProxyOutcome::Segment(payload) => Ok(build_stream_response(payload, true)),
        HlsProxyOutcome::RangeNotSatisfiable { content_range } => {
            Ok(build_416_response(content_range))
        }
    }
}

pub async fn handle_video_hls_proxy(
    state: &VideoHttpState,
    user: &User,
    raw_item_id: &str,
    query: &JfQuery,
    headers: &HeaderMap,
    api_key: &str,
) -> DomainResult<Response> {
    let uuid = Uuid::parse_str(raw_item_id.trim())
        .map_err(|_| DomainError::NotFound("invalid item id".into()))?;
    let video = state
        .services
        .catalog
        .require_bound_video(user.id, VideoId(uuid))
        .await?;
    if video.source.is_cached() {
        return Err(DomainError::Validation(
            "cached video does not use online HLS proxy".into(),
        ));
    }
    let tracking = video
        .source
        .online_tracking_info()
        .ok_or_else(|| DomainError::NotFound("video is not an online tracked item".into()))?;
    let bound_source_id = tracking.source_id;
    if let Some(raw_sid) = query
        .get("source_id")
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let parsed_sid = Uuid::parse_str(raw_sid)
            .map_err(|_| DomainError::Validation("invalid source_id parameter".into()))?;
        if parsed_sid != bound_source_id {
            return Err(DomainError::Forbidden(
                "source_id does not match the bound video".into(),
            ));
        }
    }

    let raw_url = query
        .get("url")
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| DomainError::Validation("missing url parameter".into()))?;
    if !raw_url
        .get(..7)
        .is_some_and(|p| p.eq_ignore_ascii_case("http://"))
        && !raw_url
            .get(..8)
            .is_some_and(|p| p.eq_ignore_ascii_case("https://"))
    {
        return Err(DomainError::Validation(
            "only http/https HLS proxy URLs are supported".into(),
        ));
    }
    let range_header = headers.get(RANGE).and_then(|v| v.to_str().ok());
    let encoded_key = encode_query_component(api_key);

    match state
        .services
        .online
        .open_hls_proxy_for_tracked(user, raw_url, bound_source_id, range_header, |resolved| {
            format!(
                "proxy?url={}&source_id={bound_source_id}&api_key={encoded_key}",
                encode_query_component(resolved)
            )
        })
        .await?
    {
        HlsProxyOutcome::Playlist(rewritten) => Ok(build_hls_playlist_response(rewritten)),
        HlsProxyOutcome::Segment(payload) => Ok(build_stream_response(payload, true)),
        HlsProxyOutcome::RangeNotSatisfiable { content_range } => {
            Ok(build_416_response(content_range))
        }
    }
}

pub async fn handle_subtitle_stream(
    state: &VideoHttpState,
    user: &User,
    raw_item_id: &str,
    raw_index: &str,
) -> DomainResult<Response> {
    let uuid = Uuid::parse_str(raw_item_id.trim())
        .map_err(|_| DomainError::NotFound("invalid item id".into()))?;
    let idx = raw_index
        .trim()
        .parse::<usize>()
        .map_err(|_| DomainError::NotFound("invalid subtitle index".into()))?;
    let video = state
        .services
        .catalog
        .require_bound_video(user.id, VideoId(uuid))
        .await?;

    let vtt = if video.source.is_cached() {
        let subs = SubtitleTrackDto::collect_from_video(&video);
        let audio_offset = 1 + video.audio_streams.len().max(1);
        let target = subs
            .get(idx)
            .or_else(|| idx.checked_sub(audio_offset).and_then(|i| subs.get(i)))
            .ok_or_else(|| DomainError::NotFound("subtitle stream not found".into()))?;
        state
            .services
            .streaming
            .get_subtitle_webvtt(user.id, video.id, &target.id, None)
            .await?
    } else {
        let (source_id, play_res) = state
            .services
            .online
            .resolve_tracked_video_upstream(video.id)
            .await?;
        let sub = play_res
            .subtitles
            .get(idx)
            .or_else(|| idx.checked_sub(2).and_then(|i| play_res.subtitles.get(i)))
            .ok_or_else(|| DomainError::NotFound("subtitle stream not found".into()))?;
        state
            .services
            .online
            .open_subtitle_proxy(user, &sub.url, source_id)
            .await?
    };

    let mut resp = Response::new(Body::from(vtt));
    resp.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/vtt; charset=utf-8"),
    );
    resp.headers_mut()
        .insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    resp.headers_mut().insert(
        CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'none'"),
    );
    Ok(resp)
}

pub async fn handle_image(
    state: &VideoHttpState,
    raw_item_id: &str,
    query: &JfQuery,
    authenticated_user: Option<&User>,
) -> DomainResult<Response> {
    let tag_param = query.get("tag").map(str::trim).filter(|s| !s.is_empty());
    if authenticated_user.is_none() && tag_param.is_none() {
        return Err(DomainError::Unauthorized("authentication required".into()));
    }

    let uuid = Uuid::parse_str(raw_item_id.trim())
        .map_err(|_| DomainError::NotFound("invalid item id".into()))?;

    let (poster_key, target_video, target_show) =
        match state.services.shows.resolve_item_kind(uuid).await? {
            Some(ResolvedCatalogEntity::Video(video)) => (video.poster_key, Some(video.id), None),
            Some(ResolvedCatalogEntity::Show(show))
            | Some(ResolvedCatalogEntity::Season { show, .. }) => {
                (show.poster_key, None, Some(show.id))
            }
            None => (None, None, None),
        };
    let Some(poster_key) = poster_key else {
        if authenticated_user.is_none() {
            return Err(DomainError::Unauthorized("authentication required".into()));
        }
        return Err(DomainError::NotFound("image not found".into()));
    };

    let tag_matches = tag_param.is_some_and(|t| t == poster_key.as_str());
    match authenticated_user {
        Some(user) => {
            if let Some(vid) = target_video {
                state
                    .services
                    .catalog
                    .require_bound_video(user.id, vid)
                    .await?;
            } else if let Some(sid) = target_show {
                state
                    .services
                    .shows
                    .require_bound_show(user.id, sid)
                    .await?;
            }
        }
        None => {
            // Jellyfin clients (Infuse/Swiftfin) fetch images unauthenticated using the
            // high-entropy content-hash `tag` returned by authenticated `/Items` responses.
            if !tag_matches {
                return Err(DomainError::Unauthorized("authentication required".into()));
            }
        }
    }

    let max_size = query
        .u32_param("maxwidth")
        .or_else(|| query.u32_param("maxheight"))
        .or_else(|| query.u32_param("width"));
    let payload = state
        .services
        .streaming
        .get_poster(None, &poster_key, max_size)
        .await?;
    Ok(into_binary_cache_response(payload))
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct SessionPlayingPayload {
    #[serde(alias = "ItemId", alias = "itemId")]
    item_id: Option<String>,
    #[serde(alias = "PositionTicks", alias = "positionTicks")]
    position_ticks: Option<i64>,
}

pub async fn handle_session_playing(
    state: &VideoHttpState,
    user: &User,
    body: &Bytes,
    save_zero_position: bool,
) -> DomainResult<Response> {
    if let Ok(payload) = serde_json::from_slice::<SessionPlayingPayload>(body)
        && let Some(ref raw_id) = payload.item_id
        && let Ok(uuid) = Uuid::parse_str(raw_id.trim())
        && let Some(ticks) = payload.position_ticks
        && (ticks > 0 || save_zero_position)
        && matches!(
            state.services.shows.resolve_item_kind(uuid).await?,
            Some(ResolvedCatalogEntity::Video(_))
        )
    {
        let pos_secs = ticks_to_secs(ticks);
        let _ = state
            .services
            .watch
            .save_progress(
                user,
                VideoId(uuid),
                SaveWatchProgressInput {
                    position_secs: pos_secs,
                    duration_secs: None,
                    completed: None,
                    selected_audio_stream: None,
                    selected_subtitle: None,
                },
            )
            .await;
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

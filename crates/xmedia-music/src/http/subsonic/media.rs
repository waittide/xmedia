use axum::http::HeaderMap;
use axum::response::Response;
use xmedia_core::domain::library::MediaKind;
use xmedia_core::domain::user::{User, require_admin};
use xmedia_core::http::{into_binary_cache_response, parse_http_range_spec};

use super::envelope::{SsNode, SubsonicError, SubsonicParams, render_ok};
use super::parse_uuid_param;
use crate::domain::catalog::TrackId;
use crate::http::into_audio_http_response;
use crate::http::state::MusicHttpState;

pub(super) async fn handle_media_method(
    state: &MusicHttpState,
    user: &User,
    method: &str,
    params: &SubsonicParams,
    headers: &HeaderMap,
) -> Result<Response, SubsonicError> {
    match method {
        "stream" => {
            let id = parse_uuid_param(params, "id")?;
            state
                .services
                .catalog
                .require_track_bound(user.id, TrackId(id))
                .await?;
            state
                .services
                .online
                .ensure_tracked_track_cached(TrackId(id))
                .await?;
            let spec = parse_http_range_spec(headers);
            let resolved = state
                .services
                .streaming
                .open_stream(
                    user.id,
                    TrackId(id),
                    params.first("format"),
                    params.u32("maxBitRate"),
                    params.u32("timeOffset"),
                    spec,
                )
                .await?;
            Ok(into_audio_http_response(resolved, false))
        }

        "download" => {
            let id = parse_uuid_param(params, "id")?;
            state
                .services
                .catalog
                .require_track_bound(user.id, TrackId(id))
                .await?;
            state
                .services
                .online
                .ensure_tracked_track_cached(TrackId(id))
                .await?;
            let spec = parse_http_range_spec(headers);
            let resolved = state
                .services
                .streaming
                .open_raw_download(user.id, TrackId(id), spec)
                .await?;
            Ok(into_audio_http_response(resolved, true))
        }

        "getCoverArt" => {
            let id = params
                .first("id")
                .ok_or_else(|| SubsonicError::missing_param("id"))?;
            let size = params.u32("size");
            let cover = state.services.catalog.get_cover_art(user, id, size).await?;
            Ok(into_binary_cache_response(cover))
        }

        "getLyrics" => {
            let artist = params.first("artist");
            let title = params.first("title");
            let lyr = state
                .services
                .catalog
                .get_lyrics_by_artist_title(user.id, artist, title)
                .await?;
            let node = SsNode::new()
                .attr_opt_str(
                    "artist",
                    lyr.display_artist.or_else(|| artist.map(str::to_string)),
                )
                .attr_opt_str(
                    "title",
                    lyr.display_title.or_else(|| title.map(str::to_string)),
                )
                .with_text(lyr.plain_text);
            Ok(render_ok(params, Some(("lyrics", node))))
        }

        "getLyricsBySongId" => {
            let id = parse_uuid_param(params, "id")?;
            state
                .services
                .catalog
                .require_track_bound(user.id, TrackId(id))
                .await?;
            let _ = state
                .services
                .online
                .ensure_tracked_lyrics_cached(TrackId(id))
                .await;
            let lyr = state
                .services
                .catalog
                .get_lyrics_by_track(user.id, TrackId(id))
                .await?;
            let structured = if lyr.is_empty() {
                Vec::new()
            } else {
                let line_nodes: Vec<SsNode> = if lyr.synced {
                    lyr.lines
                        .into_iter()
                        .map(|l| {
                            SsNode::new()
                                .attr_uint("start", l.start_ms)
                                .with_text(l.text)
                        })
                        .collect()
                } else {
                    lyr.plain_text
                        .lines()
                        .map(|line| SsNode::new().with_text(line.to_string()))
                        .collect()
                };

                vec![
                    SsNode::new()
                        .attr_str("lang", lyr.lang)
                        .attr_bool("synced", lyr.synced)
                        .attr_int("offset", lyr.offset_ms)
                        .attr_opt_str("displayArtist", lyr.display_artist)
                        .attr_opt_str("displayTitle", lyr.display_title)
                        .child_list("line", line_nodes),
                ]
            };
            let wrapper = SsNode::new().child_list("structuredLyrics", structured);
            Ok(render_ok(params, Some(("lyricsList", wrapper))))
        }

        "startScan" => {
            require_admin(user)?;
            let libs = state
                .libraries
                .list_libraries(Some(MediaKind::Music))
                .await?;
            for lib in libs {
                if !lib.kind.is_managed() && !lib.scan_status.is_scanning() {
                    let _ = state.libraries.trigger_scan(user, lib.id).await;
                }
            }
            let status = state.services.ingest.get_scan_status(user.id).await?;
            let node = SsNode::new()
                .attr_bool("scanning", status.scanning)
                .attr_uint("count", status.count);
            Ok(render_ok(params, Some(("scanStatus", node))))
        }

        "getScanStatus" => {
            let status = state.services.ingest.get_scan_status(user.id).await?;
            let node = SsNode::new()
                .attr_bool("scanning", status.scanning)
                .attr_uint("count", status.count);
            Ok(render_ok(params, Some(("scanStatus", node))))
        }

        unknown => Err(SubsonicError::generic(format!(
            "Unsupported Subsonic endpoint: {unknown}"
        ))),
    }
}

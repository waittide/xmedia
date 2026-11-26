use axum::response::Response;
use jiff::Timestamp;
use uuid::Uuid;
use xmedia_core::domain::user::User;

use super::envelope::{SsNode, SubsonicError, SubsonicParams, render_ok};
use super::{
    album_to_dir_node, album_to_id3_node, artist_to_node, parse_uuid_param,
    playlist_summary_to_node, resolve_optional_library_id, track_to_node,
};
use crate::application::dto::UpdatePlaylistInput;
use crate::domain::catalog::{AlbumId, ArtistId, TrackId};
use crate::domain::collection::PlaylistId;
use crate::http::state::MusicHttpState;

pub(super) async fn handle_collections_method(
    state: &MusicHttpState,
    user: &User,
    method: &str,
    params: &SubsonicParams,
    client_name: &str,
) -> Result<Response, SubsonicError> {
    match method {
        "star" | "unstar" => {
            let ids: Vec<String> = params
                .all("id")
                .into_iter()
                .map(|s| super::strip_subsonic_id_prefix(&s).to_string())
                .collect();
            let album_ids: Vec<AlbumId> = params
                .all("albumId")
                .iter()
                .filter_map(|s| {
                    Uuid::parse_str(super::strip_subsonic_id_prefix(s))
                        .ok()
                        .map(AlbumId)
                })
                .collect();
            let artist_ids: Vec<ArtistId> = params
                .all("artistId")
                .iter()
                .filter_map(|s| {
                    Uuid::parse_str(super::strip_subsonic_id_prefix(s))
                        .ok()
                        .map(ArtistId)
                })
                .collect();

            state
                .services
                .collection
                .set_starred(user.id, method == "star", &ids, &album_ids, &artist_ids)
                .await?;
            Ok(render_ok(params, None))
        }

        "getStarred" | "getStarred2" => {
            let lib_id = resolve_optional_library_id(state, params, "musicFolderId").await;
            let starred = state
                .services
                .collection
                .get_starred(user.id, lib_id)
                .await?;
            let artist_nodes: Vec<SsNode> = starred.artists.iter().map(artist_to_node).collect();
            let song_nodes: Vec<SsNode> = starred.tracks.iter().map(track_to_node).collect();

            if method == "getStarred2" {
                let album_nodes: Vec<SsNode> =
                    starred.albums.iter().map(album_to_id3_node).collect();
                let wrapper = SsNode::new()
                    .child_list("artist", artist_nodes)
                    .child_list("album", album_nodes)
                    .child_list("song", song_nodes);
                Ok(render_ok(params, Some(("starred2", wrapper))))
            } else {
                let album_nodes: Vec<SsNode> =
                    starred.albums.iter().map(album_to_dir_node).collect();
                let wrapper = SsNode::new()
                    .child_list("artist", artist_nodes)
                    .child_list("album", album_nodes)
                    .child_list("song", song_nodes);
                Ok(render_ok(params, Some(("starred", wrapper))))
            }
        }

        "setRating" => {
            let id = params
                .first("id")
                .ok_or_else(|| SubsonicError::missing_param("id"))?;
            let clean_id = super::strip_subsonic_id_prefix(id);
            let rating = params
                .u32("rating")
                .ok_or_else(|| SubsonicError::missing_param("rating"))?;
            if rating > 5 {
                return Err(SubsonicError::generic("rating must be between 0 and 5"));
            }
            state
                .services
                .collection
                .set_rating(user.id, clean_id, rating as u8)
                .await?;
            Ok(render_ok(params, None))
        }

        "scrobble" => {
            let raw_ids = params.all("id");
            if raw_ids.is_empty() {
                return Err(SubsonicError::missing_param("id"));
            }
            let raw_times = params.all("time");
            let submission = params.bool_or("submission", true);
            let mut entries = Vec::new();
            for (idx, raw_id) in raw_ids.iter().enumerate() {
                let tid = Uuid::parse_str(raw_id.trim())
                    .map(TrackId)
                    .map_err(|_| SubsonicError::not_found("invalid song id"))?;
                let ts = raw_times
                    .get(idx)
                    .and_then(|s| s.trim().parse::<i64>().ok())
                    .and_then(|ms| Timestamp::from_millisecond(ms).ok());
                entries.push((tid, ts));
            }
            state
                .services
                .collection
                .scrobble(user, entries, submission, client_name)
                .await?;
            Ok(render_ok(params, None))
        }

        "getNowPlaying" => {
            let entries = state.services.collection.list_now_playing(user.id).await?;
            let nodes: Vec<SsNode> = entries
                .iter()
                .map(|np| {
                    track_to_node(&np.track)
                        .attr_str("username", &np.username)
                        .attr_uint("minutesAgo", u64::from(np.minutes_ago))
                        .attr_str("playerName", &np.client_name)
                })
                .collect();
            let wrapper = SsNode::new().child_list("entry", nodes);
            Ok(render_ok(params, Some(("nowPlaying", wrapper))))
        }

        "getPlaylists" => {
            let list = state.services.collection.list_playlists(user.id).await?;
            let nodes: Vec<SsNode> = list.iter().map(playlist_summary_to_node).collect();
            let wrapper = SsNode::new().child_list("playlist", nodes);
            Ok(render_ok(params, Some(("playlists", wrapper))))
        }

        "getPlaylist" => {
            let id = parse_uuid_param(params, "id")?;
            let detail = state
                .services
                .collection
                .get_playlist(user, PlaylistId(id))
                .await?;
            let entries: Vec<SsNode> = detail.tracks.iter().map(track_to_node).collect();
            let node = playlist_summary_to_node(&detail.playlist).child_list("entry", entries);
            Ok(render_ok(params, Some(("playlist", node))))
        }

        "createPlaylist" => {
            let song_ids: Vec<TrackId> = params
                .all("songId")
                .iter()
                .filter_map(|s| Uuid::parse_str(s.trim()).ok().map(TrackId))
                .collect();

            let detail = if let Some(existing_id_raw) = params.first("playlistId") {
                let pl_id = Uuid::parse_str(existing_id_raw.trim())
                    .map(PlaylistId)
                    .map_err(|_| SubsonicError::not_found("invalid playlistId"))?;
                state
                    .services
                    .collection
                    .update_playlist(
                        user,
                        pl_id,
                        UpdatePlaylistInput {
                            name: params.first("name").map(str::to_string),
                            replace_track_ids: Some(song_ids),
                            ..Default::default()
                        },
                    )
                    .await?
            } else {
                let name = params
                    .first("name")
                    .ok_or_else(|| SubsonicError::missing_param("name"))?;
                state
                    .services
                    .collection
                    .create_playlist(user, name, None, false, song_ids)
                    .await?
            };

            let entries: Vec<SsNode> = detail.tracks.iter().map(track_to_node).collect();
            let node = playlist_summary_to_node(&detail.playlist).child_list("entry", entries);
            Ok(render_ok(params, Some(("playlist", node))))
        }

        "updatePlaylist" => {
            let pl_id = parse_uuid_param(params, "playlistId")?;
            let add_ids: Vec<TrackId> = params
                .all("songIdToAdd")
                .iter()
                .filter_map(|s| Uuid::parse_str(s.trim()).ok().map(TrackId))
                .collect();
            let remove_indices: Vec<usize> = params
                .all("songIndexToRemove")
                .iter()
                .filter_map(|s| s.trim().parse::<usize>().ok())
                .collect();
            let is_public = params
                .first("public")
                .map(|_| params.bool_or("public", false));

            state
                .services
                .collection
                .update_playlist(
                    user,
                    PlaylistId(pl_id),
                    UpdatePlaylistInput {
                        name: params.first("name").map(str::to_string),
                        comment: params.first("comment").map(str::to_string),
                        is_public,
                        replace_track_ids: None,
                        add_track_ids: add_ids,
                        remove_indices,
                    },
                )
                .await?;
            Ok(render_ok(params, None))
        }

        "deletePlaylist" => {
            let id = parse_uuid_param(params, "id")?;
            state
                .services
                .collection
                .delete_playlist(user, PlaylistId(id))
                .await?;
            Ok(render_ok(params, None))
        }

        "getPlayQueue" => {
            let queue_opt = state.services.collection.get_play_queue(user.id).await?;
            let Some(q) = queue_opt else {
                return Ok(render_ok(params, None));
            };
            let entries: Vec<SsNode> = q.tracks.iter().map(track_to_node).collect();
            let node = SsNode::new()
                .attr_opt_str("current", q.current_track_id.map(|id| id.to_string()))
                .attr_uint("position", q.position_ms)
                .attr_str("username", &user.username)
                .attr_str("changed", q.updated_at.to_string())
                .attr_str("changedBy", q.changed_by)
                .child_list("entry", entries);
            Ok(render_ok(params, Some(("playQueue", node))))
        }

        "savePlayQueue" => {
            let track_ids: Vec<TrackId> = params
                .all("id")
                .iter()
                .filter_map(|s| Uuid::parse_str(s.trim()).ok().map(TrackId))
                .collect();
            let current = params
                .first("current")
                .and_then(|s| Uuid::parse_str(s.trim()).ok())
                .map(TrackId);
            let position = params.u64("position").unwrap_or(0);
            state
                .services
                .collection
                .save_play_queue(user.id, track_ids, current, position, client_name)
                .await?;
            Ok(render_ok(params, None))
        }

        "getBookmarks" => {
            let bms = state.services.collection.list_bookmarks(user.id).await?;
            let nodes: Vec<SsNode> = bms
                .iter()
                .map(|bm| {
                    SsNode::new()
                        .attr_uint("position", bm.position_ms)
                        .attr_str("username", &user.username)
                        .attr_opt_str("comment", bm.comment.as_deref())
                        .attr_str("created", bm.created_at.to_string())
                        .attr_str("changed", bm.updated_at.to_string())
                        .child_single("entry", track_to_node(&bm.track))
                })
                .collect();
            let wrapper = SsNode::new().child_list("bookmark", nodes);
            Ok(render_ok(params, Some(("bookmarks", wrapper))))
        }

        "createBookmark" => {
            let track_id = parse_uuid_param(params, "id")?;
            let pos = params
                .u64("position")
                .ok_or_else(|| SubsonicError::missing_param("position"))?;
            let comment = params.first("comment").map(str::to_string);
            state
                .services
                .collection
                .upsert_bookmark(user.id, TrackId(track_id), pos, comment)
                .await?;
            Ok(render_ok(params, None))
        }

        "deleteBookmark" => {
            let track_id = parse_uuid_param(params, "id")?;
            state
                .services
                .collection
                .delete_bookmark(user.id, TrackId(track_id))
                .await?;
            Ok(render_ok(params, None))
        }

        unknown => Err(SubsonicError::generic(format!(
            "Unsupported Subsonic endpoint: {unknown}"
        ))),
    }
}

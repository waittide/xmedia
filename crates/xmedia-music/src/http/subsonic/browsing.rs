use std::collections::BTreeMap;

use axum::response::Response;
use jiff::Timestamp;
use xmedia_core::domain::library::MediaKind;
use xmedia_core::domain::user::User;

use super::envelope::{SsNode, SubsonicError, SubsonicParams, render_ok};
use super::{
    album_to_dir_node, album_to_id3_node, artist_to_node, parse_uuid_param,
    resolve_library_id_value, resolve_optional_library_id, track_to_node,
};
use crate::application::browse::MusicDirectoryNode;
use crate::domain::catalog::{
    AlbumId, AlbumListQuery, AlbumListType, ArtistId, ArtistQuery, SearchQuery, TrackId,
};
use crate::http::state::MusicHttpState;

pub(super) async fn handle_browsing_method(
    state: &MusicHttpState,
    user: &User,
    method: &str,
    params: &SubsonicParams,
) -> Result<Response, SubsonicError> {
    match method {
        "getUser" => {
            let target_username = params.first("username").unwrap_or(&user.username);
            let (resolved_username, is_admin) = if target_username != user.username {
                if !user.role.is_admin() {
                    return Err(SubsonicError {
                        code: 50,
                        message: "not authorized to inspect other users".into(),
                    });
                }
                let all_users = state.auth.list_users(user).await?;
                let target = all_users
                    .into_iter()
                    .find(|u| u.username == target_username)
                    .ok_or_else(|| SubsonicError::not_found("user not found"))?;
                (target.username, target.role.is_admin())
            } else {
                (user.username.clone(), user.role.is_admin())
            };
            let libs = state
                .libraries
                .list_libraries(Some(MediaKind::Music))
                .await?;
            let folder_ids: Vec<i64> = (1..=libs.len() as i64).collect();
            let u_node = SsNode::new()
                .attr_str("username", &resolved_username)
                .attr_str("email", format!("{resolved_username}@xmedia.local"))
                .attr_bool("scrobblingEnabled", true)
                .attr_bool("adminRole", is_admin)
                .attr_bool("settingsRole", true)
                .attr_bool("downloadRole", true)
                .attr_bool("uploadRole", is_admin)
                .attr_bool("playlistRole", true)
                .attr_bool("coverArtRole", true)
                .attr_bool("commentRole", true)
                .attr_bool("podcastRole", false)
                .attr_bool("streamRole", true)
                .attr_bool("jukeboxRole", false)
                .attr_bool("shareRole", false)
                .child_int_list("folder", folder_ids);
            Ok(render_ok(params, Some(("user", u_node))))
        }

        "getMusicFolders" => {
            let libs = state
                .libraries
                .list_libraries(Some(MediaKind::Music))
                .await?;
            let counts = state
                .services
                .browse
                .count_bound_tracks_by_library(user.id)
                .await?;
            let folders: Vec<SsNode> = libs
                .into_iter()
                .filter(|l| counts.get(&l.id).copied().unwrap_or(0) > 0)
                .map(|l| {
                    SsNode::new()
                        .attr_str("id", l.id.to_string())
                        .attr_str("name", l.name)
                })
                .collect();
            let wrapper = SsNode::new().child_list("musicFolder", folders);
            Ok(render_ok(params, Some(("musicFolders", wrapper))))
        }

        "getIndexes" | "getArtists" => {
            let lib_id = resolve_optional_library_id(state, params, "musicFolderId").await;
            let artists = state
                .services
                .browse
                .list_artists(
                    user.id,
                    ArtistQuery {
                        library_id: lib_id,
                        size: 5000,
                        ..Default::default()
                    },
                )
                .await?
                .items;

            const IGNORED_ARTICLES: &[&str] =
                &["the ", "el ", "la ", "los ", "las ", "le ", "les "];
            let mut buckets: BTreeMap<String, Vec<SsNode>> = BTreeMap::new();
            for ar in artists {
                let trimmed = ar.name.trim();
                let lower = trimmed.to_ascii_lowercase();
                let sort_name = IGNORED_ARTICLES
                    .iter()
                    .find_map(|prefix| {
                        if lower.starts_with(prefix) {
                            let rest = trimmed[prefix.len()..].trim_start();
                            (!rest.is_empty()).then_some(rest)
                        } else {
                            None
                        }
                    })
                    .unwrap_or(trimmed);
                let first_ch = sort_name
                    .chars()
                    .next()
                    .map(|c| {
                        if c.is_ascii_alphabetic() {
                            c.to_ascii_uppercase().to_string()
                        } else {
                            "#".to_string()
                        }
                    })
                    .unwrap_or_else(|| "#".to_string());
                buckets
                    .entry(first_ch)
                    .or_default()
                    .push(artist_to_node(&ar));
            }

            let index_nodes: Vec<SsNode> = buckets
                .into_iter()
                .map(|(letter, list)| {
                    SsNode::new()
                        .attr_str("name", letter)
                        .child_list("artist", list)
                })
                .collect();

            let root_tag = if method == "getArtists" {
                "artists"
            } else {
                "indexes"
            };
            let wrapper = SsNode::new()
                .attr_str("ignoredArticles", "The El La Los Las Le Les")
                .attr_int("lastModified", Timestamp::now().as_millisecond())
                .child_list("index", index_nodes);
            Ok(render_ok(params, Some((root_tag, wrapper))))
        }

        "getMusicDirectory" => {
            let raw_id = params
                .first("id")
                .ok_or_else(|| SubsonicError::missing_param("id"))?;
            let resolved_id = resolve_library_id_value(state, raw_id)
                .await
                .map(|lid| lid.to_string())
                .unwrap_or_else(|| raw_id.to_string());
            let MusicDirectoryNode::Folder {
                id,
                parent,
                name,
                sub_albums,
                tracks,
            } = state
                .services
                .browse
                .get_music_directory(user.id, &resolved_id)
                .await?;

            let mut children: Vec<SsNode> = sub_albums.iter().map(album_to_dir_node).collect();
            children.extend(tracks.iter().map(track_to_node));

            let dir = SsNode::new()
                .attr_str("id", id)
                .attr_opt_str("parent", parent)
                .attr_str("name", name)
                .child_list("child", children);
            Ok(render_ok(params, Some(("directory", dir))))
        }

        "getGenres" => {
            let genres = state.services.catalog.list_genres(user.id).await?;
            let items: Vec<SsNode> = genres
                .into_iter()
                .map(|g| {
                    SsNode::new()
                        .attr_uint("songCount", g.song_count)
                        .attr_uint("albumCount", g.album_count)
                        .with_text(g.name)
                })
                .collect();
            let wrapper = SsNode::new().child_list("genre", items);
            Ok(render_ok(params, Some(("genres", wrapper))))
        }

        "getArtist" => {
            let id = parse_uuid_param(params, "id")?;
            let detail = state
                .services
                .browse
                .get_artist_detail(user.id, ArtistId(id))
                .await?;
            let album_nodes: Vec<SsNode> = detail.albums.iter().map(album_to_id3_node).collect();
            let node = artist_to_node(&detail.artist).child_list("album", album_nodes);
            Ok(render_ok(params, Some(("artist", node))))
        }

        "getArtistInfo" => Ok(render_ok(params, Some(("artistInfo", SsNode::new())))),
        "getArtistInfo2" => Ok(render_ok(params, Some(("artistInfo2", SsNode::new())))),
        "getAlbumInfo" => Ok(render_ok(params, Some(("albumInfo", SsNode::new())))),
        "getAlbumInfo2" => Ok(render_ok(params, Some(("albumInfo2", SsNode::new())))),

        "getAlbum" => {
            let id = parse_uuid_param(params, "id")?;
            let detail = state
                .services
                .browse
                .get_album_detail(user.id, AlbumId(id))
                .await?;
            let song_nodes: Vec<SsNode> = detail.tracks.iter().map(track_to_node).collect();
            let node = album_to_id3_node(&detail.album).child_list("song", song_nodes);
            Ok(render_ok(params, Some(("album", node))))
        }

        "getSong" => {
            let id = parse_uuid_param(params, "id")?;
            let track = state
                .services
                .tracks
                .get_track(user.id, TrackId(id))
                .await?;
            Ok(render_ok(params, Some(("song", track_to_node(&track)))))
        }

        "getAlbumList" | "getAlbumList2" => {
            let raw_type = params
                .first("type")
                .ok_or_else(|| SubsonicError::missing_param("type"))?;
            let list_type = raw_type.parse::<AlbumListType>()?;
            let lib_id = resolve_optional_library_id(state, params, "musicFolderId").await;
            let q = AlbumListQuery {
                list_type,
                library_id: lib_id,
                genre: params.first("genre").map(str::to_string),
                from_year: params.i32("fromYear"),
                to_year: params.i32("toYear"),
                search: None,
                offset: params.u32("offset").unwrap_or(0),
                size: params.u32("size").unwrap_or(10).clamp(1, 500),
            };
            let paged = state.services.browse.list_albums(user.id, q).await?;
            if method == "getAlbumList2" {
                let nodes: Vec<SsNode> = paged.items.iter().map(album_to_id3_node).collect();
                let wrapper = SsNode::new().child_list("album", nodes);
                Ok(render_ok(params, Some(("albumList2", wrapper))))
            } else {
                let nodes: Vec<SsNode> = paged.items.iter().map(album_to_dir_node).collect();
                let wrapper = SsNode::new().child_list("album", nodes);
                Ok(render_ok(params, Some(("albumList", wrapper))))
            }
        }

        "getRandomSongs" => {
            let size = params.u32("size").unwrap_or(10).clamp(1, 500);
            let lib_id = resolve_optional_library_id(state, params, "musicFolderId").await;
            let tracks = state
                .services
                .tracks
                .get_random_tracks(
                    user.id,
                    size,
                    lib_id,
                    params.first("genre"),
                    params.i32("fromYear"),
                    params.i32("toYear"),
                )
                .await?;
            let nodes: Vec<SsNode> = tracks.iter().map(track_to_node).collect();
            let wrapper = SsNode::new().child_list("song", nodes);
            Ok(render_ok(params, Some(("randomSongs", wrapper))))
        }

        "getSongsByGenre" => {
            let genre = params
                .first("genre")
                .ok_or_else(|| SubsonicError::missing_param("genre"))?;
            let count = params.u32("count").unwrap_or(10).clamp(1, 500);
            let offset = params.u32("offset").unwrap_or(0);
            let lib_id = resolve_optional_library_id(state, params, "musicFolderId").await;
            let tracks = state
                .services
                .tracks
                .get_tracks_by_genre(user.id, genre, offset, count, lib_id)
                .await?;
            let nodes: Vec<SsNode> = tracks.iter().map(track_to_node).collect();
            let wrapper = SsNode::new().child_list("song", nodes);
            Ok(render_ok(params, Some(("songsByGenre", wrapper))))
        }

        "getTopSongs" => {
            let artist_name = params
                .first("artist")
                .ok_or_else(|| SubsonicError::missing_param("artist"))?;
            let count = params.u32("count").unwrap_or(50).clamp(1, 200);
            let tracks = state
                .services
                .tracks
                .get_top_tracks_for_artist_name(user.id, artist_name, count)
                .await?;
            let nodes: Vec<SsNode> = tracks.iter().map(track_to_node).collect();
            let wrapper = SsNode::new().child_list("song", nodes);
            Ok(render_ok(params, Some(("topSongs", wrapper))))
        }

        "search2" | "search3" => {
            let query_str = params.first("query").unwrap_or_default().to_string();
            let lib_id = resolve_optional_library_id(state, params, "musicFolderId").await;
            let sq = SearchQuery {
                query: query_str,
                library_id: lib_id,
                artist_count: params.u32("artistCount").unwrap_or(20).clamp(0, 500),
                artist_offset: params.u32("artistOffset").unwrap_or(0),
                album_count: params.u32("albumCount").unwrap_or(20).clamp(0, 500),
                album_offset: params.u32("albumOffset").unwrap_or(0),
                song_count: params.u32("songCount").unwrap_or(20).clamp(0, 500),
                song_offset: params.u32("songOffset").unwrap_or(0),
            };
            let res = state.services.catalog.search(user.id, sq).await?;
            let artist_nodes: Vec<SsNode> = res.artists.iter().map(artist_to_node).collect();
            let song_nodes: Vec<SsNode> = res.tracks.iter().map(track_to_node).collect();

            if method == "search3" {
                let album_nodes: Vec<SsNode> = res.albums.iter().map(album_to_id3_node).collect();
                let wrapper = SsNode::new()
                    .child_list("artist", artist_nodes)
                    .child_list("album", album_nodes)
                    .child_list("song", song_nodes);
                Ok(render_ok(params, Some(("searchResult3", wrapper))))
            } else {
                let album_nodes: Vec<SsNode> = res.albums.iter().map(album_to_dir_node).collect();
                let wrapper = SsNode::new()
                    .child_list("artist", artist_nodes)
                    .child_list("album", album_nodes)
                    .child_list("song", song_nodes);
                Ok(render_ok(params, Some(("searchResult2", wrapper))))
            }
        }

        unknown => Err(SubsonicError::generic(format!(
            "Unsupported Subsonic endpoint: {unknown}"
        ))),
    }
}

pub mod envelope;

use std::collections::BTreeMap;

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Uri};
use axum::response::Response;
use axum::routing::any;
use jiff::Timestamp;
use uuid::Uuid;
use xmedia_core::application::dto::SubsonicCredentials;
use xmedia_core::domain::library::{LibraryId, MediaKind};
use xmedia_core::domain::user::{User, require_admin};
use xmedia_core::http::{into_binary_cache_response, parse_http_range_spec};

use crate::application::catalog::MusicDirectoryNode;
use crate::application::dto::{
    AlbumDto, ArtistDto, PlaylistSummaryDto, TrackDto, UpdatePlaylistInput,
};
use crate::domain::catalog::{
    AlbumId, AlbumListQuery, AlbumListType, ArtistId, ArtistQuery, SearchQuery, TrackId,
};
use crate::domain::collection::PlaylistId;
use crate::http::into_audio_http_response;
use crate::http::state::MusicHttpState;
use envelope::{SsNode, SubsonicError, SubsonicParams, render_array_root, render_err, render_ok};

pub fn router() -> Router<MusicHttpState> {
    Router::new().route("/{endpoint}", any(dispatch_subsonic))
}

async fn dispatch_subsonic(
    State(state): State<MusicHttpState>,
    Path(raw_endpoint): Path<String>,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let content_type = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let form_slice = if content_type.contains("application/x-www-form-urlencoded") {
        body.as_ref()
    } else {
        &[]
    };

    let params = SubsonicParams::from_query_and_form(uri.query(), form_slice);
    let method = raw_endpoint
        .strip_suffix(".view")
        .unwrap_or(&raw_endpoint)
        .trim();

    if method == "getOpenSubsonicExtensions" {
        return handle_open_subsonic_extensions(&params);
    }

    let user = match authenticate_request(&state, &params, &headers).await {
        Ok(u) => u,
        Err(err) => return render_err(&params, err),
    };

    match handle_authenticated_method(&state, &user, method, &params, &headers).await {
        Ok(resp) => resp,
        Err(err) => render_err(&params, err),
    }
}

async fn authenticate_request(
    state: &MusicHttpState,
    params: &SubsonicParams,
    headers: &HeaderMap,
) -> Result<User, SubsonicError> {
    if let Some(api_key) = params.first("apiKey").filter(|k| !k.trim().is_empty()) {
        return state
            .auth
            .authenticate_subsonic(SubsonicCredentials::ApiKey(api_key.to_string()))
            .await
            .map_err(SubsonicError::from);
    }

    if let Some(hdr_key) = headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .filter(|k| !k.trim().is_empty())
    {
        return state
            .auth
            .authenticate_subsonic(SubsonicCredentials::ApiKey(hdr_key.to_string()))
            .await
            .map_err(SubsonicError::from);
    }

    let username = params
        .first("u")
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| SubsonicError::missing_param("u"))?;

    if let (Some(token), Some(salt)) = (params.first("t"), params.first("s")) {
        return state
            .auth
            .authenticate_subsonic(SubsonicCredentials::TokenSalt {
                username: username.to_string(),
                token: token.to_string(),
                salt: salt.to_string(),
            })
            .await
            .map_err(SubsonicError::from);
    }

    if let Some(pwd) = params.first("p") {
        return state
            .auth
            .authenticate_subsonic(SubsonicCredentials::Password {
                username: username.to_string(),
                password: pwd.to_string(),
            })
            .await
            .map_err(SubsonicError::from);
    }

    Err(SubsonicError::missing_param("t/s or p or apiKey"))
}

fn handle_open_subsonic_extensions(params: &SubsonicParams) -> Response {
    let exts = vec![
        SsNode::new()
            .attr_str("name", "transcodeOffset")
            .child_int_list("versions", vec![1]),
        SsNode::new()
            .attr_str("name", "formPost")
            .child_int_list("versions", vec![1]),
        SsNode::new()
            .attr_str("name", "songLyrics")
            .child_int_list("versions", vec![1]),
        SsNode::new()
            .attr_str("name", "apiKeyAuthentication")
            .child_int_list("versions", vec![1]),
    ];
    render_array_root(
        params,
        "openSubsonicExtensions",
        "openSubsonicExtensions",
        exts,
    )
}

async fn handle_authenticated_method(
    state: &MusicHttpState,
    user: &User,
    method: &str,
    params: &SubsonicParams,
    headers: &HeaderMap,
) -> Result<Response, SubsonicError> {
    let client_name = params.first("c").unwrap_or("subsonic-client");

    match method {
        "ping" => Ok(render_ok(params, None)),

        "getLicense" => {
            let node = SsNode::new()
                .attr_bool("valid", true)
                .attr_str("email", "selfhosted@xmedia.local")
                .attr_str("licenseExpires", "2099-12-31T23:59:59Z");
            Ok(render_ok(params, Some(("license", node))))
        }

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
            let folders: Vec<SsNode> = libs
                .into_iter()
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
                .catalog
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
                .catalog
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
            let genres = state.services.catalog.list_genres().await?;
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
                .catalog
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
                .catalog
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
                .catalog
                .get_track(user.id, TrackId(id))
                .await?;
            Ok(render_ok(params, Some(("song", track_to_node(&track)))))
        }

        "getAlbumList" | "getAlbumList2" => {
            let raw_type = params
                .first("type")
                .ok_or_else(|| SubsonicError::missing_param("type"))?;
            let list_type = AlbumListType::parse(raw_type)?;
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
            let paged = state.services.catalog.list_albums(user.id, q).await?;
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
                .catalog
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
                .catalog
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
                .catalog
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

        "stream" => {
            let id = parse_uuid_param(params, "id")?;
            let spec = parse_http_range_spec(headers);
            let resolved = state
                .services
                .streaming
                .open_stream(
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
            let spec = parse_http_range_spec(headers);
            let resolved = state
                .services
                .streaming
                .open_raw_download(TrackId(id), spec)
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
                .get_lyrics_by_artist_title(artist, title)
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
            let lyr = state
                .services
                .catalog
                .get_lyrics_by_track(TrackId(id))
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

        "star" | "unstar" => {
            let ids = params.all("id");
            let album_ids: Vec<AlbumId> = params
                .all("albumId")
                .iter()
                .filter_map(|s| Uuid::parse_str(s.trim()).ok().map(AlbumId))
                .collect();
            let artist_ids: Vec<ArtistId> = params
                .all("artistId")
                .iter()
                .filter_map(|s| Uuid::parse_str(s.trim()).ok().map(ArtistId))
                .collect();

            if method == "star" {
                state
                    .services
                    .collection
                    .star_items(user.id, &ids, &album_ids, &artist_ids)
                    .await?;
            } else {
                state
                    .services
                    .collection
                    .unstar_items(user.id, &ids, &album_ids, &artist_ids)
                    .await?;
            }
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
            let rating = params
                .u32("rating")
                .ok_or_else(|| SubsonicError::missing_param("rating"))?;
            if rating > 5 {
                return Err(SubsonicError::generic("rating must be between 0 and 5"));
            }
            state
                .services
                .collection
                .set_rating(user.id, id, rating as u8)
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
            let status = state.services.ingest.get_scan_status().await?;
            let node = SsNode::new()
                .attr_bool("scanning", status.scanning)
                .attr_uint("count", status.count);
            Ok(render_ok(params, Some(("scanStatus", node))))
        }

        "getScanStatus" => {
            let status = state.services.ingest.get_scan_status().await?;
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

fn parse_uuid_param(params: &SubsonicParams, key: &str) -> Result<Uuid, SubsonicError> {
    let raw = params
        .first(key)
        .ok_or_else(|| SubsonicError::missing_param(key))?;
    let trimmed = raw.trim();
    let clean = trimmed
        .strip_prefix("al-")
        .or_else(|| trimmed.strip_prefix("ar-"))
        .or_else(|| trimmed.strip_prefix("tr-"))
        .or_else(|| trimmed.strip_prefix("pl-"))
        .unwrap_or(trimmed);
    Uuid::parse_str(clean).map_err(|_| SubsonicError::not_found(format!("invalid id '{raw}'")))
}

async fn resolve_optional_library_id(
    state: &MusicHttpState,
    params: &SubsonicParams,
    key: &str,
) -> Option<LibraryId> {
    let raw = params.first(key)?;
    resolve_library_id_value(state, raw).await
}

async fn resolve_library_id_value(state: &MusicHttpState, raw: &str) -> Option<LibraryId> {
    let trimmed = raw.trim();
    if let Ok(uuid) = Uuid::parse_str(trimmed) {
        return Some(LibraryId(uuid));
    }
    if let Ok(folder_num) = trimmed.parse::<usize>()
        && folder_num >= 1
        && let Ok(libs) = state.libraries.list_libraries(Some(MediaKind::Music)).await
        && let Some(lib) = libs.get(folder_num - 1)
    {
        return Some(lib.id);
    }
    None
}

fn artist_to_node(ar: &ArtistDto) -> SsNode {
    SsNode::new()
        .attr_str("id", ar.id.to_string())
        .attr_str("name", &ar.name)
        .attr_uint("albumCount", u64::from(ar.album_count))
        .attr_opt_str("coverArt", ar.cover_key.as_ref().map(|_| ar.id.to_string()))
        .attr_opt_str("starred", ar.starred_at.map(|ts| ts.to_string()))
        .attr_opt_uint("userRating", ar.user_rating.map(u64::from))
}

fn album_to_id3_node(al: &AlbumDto) -> SsNode {
    SsNode::new()
        .attr_str("id", al.id.to_string())
        .attr_str("name", &al.title)
        .attr_str("title", &al.title)
        .attr_str("artist", &al.album_artist_name)
        .attr_str("artistId", al.album_artist_id.to_string())
        .attr_opt_str("coverArt", al.cover_key.as_ref().map(|_| al.id.to_string()))
        .attr_uint("songCount", u64::from(al.track_count))
        .attr_uint("duration", u64::from(al.duration_secs))
        .attr_uint("playCount", al.play_count)
        .attr_str("created", al.created_at.to_string())
        .attr_opt_int("year", al.year.map(i64::from))
        .attr_opt_str("genre", al.genre.as_deref())
        .attr_bool("isCompilation", al.is_compilation)
        .attr_opt_str("starred", al.starred_at.map(|ts| ts.to_string()))
        .attr_opt_uint("userRating", al.user_rating.map(u64::from))
}

fn album_to_dir_node(al: &AlbumDto) -> SsNode {
    album_to_id3_node(al)
        .attr_str("parent", al.album_artist_id.to_string())
        .attr_bool("isDir", true)
        .attr_str("album", &al.title)
}

fn track_to_node(tr: &TrackDto) -> SsNode {
    let mut node = SsNode::new()
        .attr_str("id", tr.id.to_string())
        .attr_str("parent", tr.album_id.to_string())
        .attr_bool("isDir", false)
        .attr_str("title", &tr.title)
        .attr_str("album", &tr.album_title)
        .attr_str("artist", &tr.artist_name)
        .attr_opt_uint("track", tr.track_number.map(u64::from))
        .attr_opt_uint("discNumber", tr.disc_number.map(u64::from))
        .attr_opt_int("year", tr.year.map(i64::from))
        .attr_opt_str("genre", tr.genre.as_deref())
        .attr_opt_str("coverArt", tr.cover_key.as_ref().map(|_| tr.id.to_string()))
        .attr_uint("size", tr.file_size)
        .attr_str("contentType", &tr.mime_type)
        .attr_str("suffix", tr.format.extension())
        .attr_uint("duration", u64::from(tr.duration_secs))
        .attr_opt_uint("bitRate", tr.bit_rate_kbps.map(u64::from))
        .attr_opt_uint("samplingRate", tr.sample_rate_hz.map(u64::from))
        .attr_opt_uint("bitDepth", tr.bit_depth.map(u64::from))
        .attr_opt_uint("channelCount", tr.channels.map(u64::from))
        .attr_str("path", &tr.path)
        .attr_bool("isVideo", false)
        .attr_str("type", "music")
        .attr_str("albumId", tr.album_id.to_string())
        .attr_str("artistId", tr.artist_id.to_string())
        .attr_uint("playCount", tr.play_count)
        .attr_opt_str("played", tr.last_played_at.map(|ts| ts.to_string()))
        .attr_str("created", tr.created_at.to_string())
        .attr_opt_str("starred", tr.starred_at.map(|ts| ts.to_string()))
        .attr_opt_uint("userRating", tr.user_rating.map(u64::from));

    let rg = &tr.replay_gain;
    if rg.track_gain_db.is_some()
        || rg.track_peak.is_some()
        || rg.album_gain_db.is_some()
        || rg.album_peak.is_some()
    {
        let rg_node = SsNode::new()
            .attr_opt_float("trackGain", rg.track_gain_db)
            .attr_opt_float("trackPeak", rg.track_peak)
            .attr_opt_float("albumGain", rg.album_gain_db)
            .attr_opt_float("albumPeak", rg.album_peak);
        node = node.child_single("replayGain", rg_node);
    }

    node
}

fn playlist_summary_to_node(pl: &PlaylistSummaryDto) -> SsNode {
    SsNode::new()
        .attr_str("id", pl.id.to_string())
        .attr_str("name", &pl.name)
        .attr_opt_str("comment", pl.comment.as_deref())
        .attr_str("owner", &pl.owner_username)
        .attr_bool("public", pl.is_public)
        .attr_uint("songCount", u64::from(pl.track_count))
        .attr_uint("duration", u64::from(pl.duration_secs))
        .attr_str("created", pl.created_at.to_string())
        .attr_str("changed", pl.updated_at.to_string())
        .attr_opt_str("coverArt", pl.cover_key.as_ref().map(|_| pl.id.to_string()))
}

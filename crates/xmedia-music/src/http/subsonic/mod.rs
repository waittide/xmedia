mod browsing;
mod collections;
pub mod envelope;
mod media;

use std::net::IpAddr;

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Uri};
use axum::response::Response;
use axum::routing::any;
use uuid::Uuid;
use xmedia_core::application::dto::SubsonicCredentials;
use xmedia_core::domain::library::{LibraryId, MediaKind};
use xmedia_core::domain::user::User;
use xmedia_core::http::ClientIp;

use crate::application::dto::{AlbumDto, ArtistDto, PlaylistSummaryDto, TrackDto};
use crate::http::state::MusicHttpState;
use envelope::{SsNode, SubsonicError, SubsonicParams, render_array_root, render_err, render_ok};

pub fn router() -> Router<MusicHttpState> {
    Router::new().route("/{endpoint}", any(dispatch_subsonic))
}

async fn dispatch_subsonic(
    State(state): State<MusicHttpState>,
    ClientIp(client_ip): ClientIp,
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

    let user = match authenticate_request(&state, &params, &headers, client_ip).await {
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
    client_ip: Option<IpAddr>,
) -> Result<User, SubsonicError> {
    if let Some(api_key) = params.first("apiKey").filter(|k| !k.trim().is_empty()) {
        return state
            .auth
            .authenticate(
                xmedia_core::application::dto::AuthCredential::Subsonic(
                    SubsonicCredentials::ApiKey(api_key.to_string()),
                ),
                client_ip,
            )
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
            .authenticate(
                xmedia_core::application::dto::AuthCredential::Subsonic(
                    SubsonicCredentials::ApiKey(hdr_key.to_string()),
                ),
                client_ip,
            )
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
            .authenticate(
                xmedia_core::application::dto::AuthCredential::Subsonic(
                    SubsonicCredentials::TokenSalt {
                        username: username.to_string(),
                        token: token.to_string(),
                        salt: salt.to_string(),
                    },
                ),
                client_ip,
            )
            .await
            .map_err(SubsonicError::from);
    }

    if let Some(pwd) = params.first("p") {
        return state
            .auth
            .authenticate(
                xmedia_core::application::dto::AuthCredential::Subsonic(
                    SubsonicCredentials::Password {
                        username: username.to_string(),
                        password: pwd.to_string(),
                    },
                ),
                client_ip,
            )
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

        "getUser" | "getMusicFolders" | "getIndexes" | "getArtists" | "getMusicDirectory"
        | "getGenres" | "getArtist" | "getArtistInfo" | "getArtistInfo2" | "getAlbumInfo"
        | "getAlbumInfo2" | "getAlbum" | "getSong" | "getAlbumList" | "getAlbumList2"
        | "getRandomSongs" | "getSongsByGenre" | "getTopSongs" | "search2" | "search3" => {
            browsing::handle_browsing_method(state, user, method, params).await
        }

        "stream" | "download" | "getCoverArt" | "getLyrics" | "getLyricsBySongId" | "startScan"
        | "getScanStatus" => media::handle_media_method(state, user, method, params, headers).await,

        "star" | "unstar" | "getStarred" | "getStarred2" | "setRating" | "scrobble"
        | "getNowPlaying" | "getPlaylists" | "getPlaylist" | "createPlaylist"
        | "updatePlaylist" | "deletePlaylist" | "getPlayQueue" | "savePlayQueue"
        | "getBookmarks" | "createBookmark" | "deleteBookmark" => {
            collections::handle_collections_method(state, user, method, params, client_name).await
        }

        unknown => Err(SubsonicError::generic(format!(
            "Unsupported Subsonic endpoint: {unknown}"
        ))),
    }
}

pub(super) fn strip_subsonic_id_prefix(raw: &str) -> &str {
    let trimmed = raw.trim();
    trimmed
        .strip_prefix("al-")
        .or_else(|| trimmed.strip_prefix("ar-"))
        .or_else(|| trimmed.strip_prefix("tr-"))
        .or_else(|| trimmed.strip_prefix("pl-"))
        .unwrap_or(trimmed)
}

fn parse_uuid_param(params: &SubsonicParams, key: &str) -> Result<Uuid, SubsonicError> {
    let raw = params
        .first(key)
        .ok_or_else(|| SubsonicError::missing_param(key))?;
    let clean = strip_subsonic_id_prefix(raw);
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

mod catalog;
mod dto;
mod playback;

use std::collections::HashMap;

use axum::Json;
use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::user::User;
use xmedia_core::http::ApiError;

use crate::http::state::VideoHttpState;

use dto::{
    JELLYFIN_SERVER_ID, JELLYFIN_SERVER_NAME, JELLYFIN_SERVER_VERSION, build_user_dto, format_jf_id,
};

pub fn router() -> Router<VideoHttpState> {
    Router::new().fallback(dispatch_jellyfin)
}

#[derive(Debug, Default)]
pub struct JfQuery {
    params: HashMap<String, String>,
}

impl JfQuery {
    pub fn parse(raw: Option<&str>) -> Self {
        let mut params = HashMap::new();
        if let Some(qs) = raw {
            for (k, v) in form_urlencoded::parse(qs.as_bytes()) {
                let key = k.trim().to_ascii_lowercase();
                if !key.is_empty() {
                    params.insert(key, v.into_owned());
                }
            }
        }
        Self { params }
    }

    pub fn get(&self, key_lower: &str) -> Option<&str> {
        self.params.get(key_lower).map(String::as_str)
    }

    pub fn usize_param(&self, key_lower: &str) -> Option<usize> {
        self.get(key_lower)
            .and_then(|s| s.trim().parse::<usize>().ok())
    }

    pub fn u32_param(&self, key_lower: &str) -> Option<u32> {
        self.get(key_lower)
            .and_then(|s| s.trim().parse::<u32>().ok())
    }

    pub fn bool_param(&self, key_lower: &str) -> Option<bool> {
        self.get(key_lower)
            .and_then(|s| match s.trim().to_ascii_lowercase().as_str() {
                "true" | "1" | "yes" => Some(true),
                "false" | "0" | "no" => Some(false),
                _ => None,
            })
    }
}

fn extract_auth_kv(header_val: &str, wanted_key: &str) -> Option<String> {
    let lower_wanted = wanted_key.to_ascii_lowercase();
    let body = header_val
        .trim()
        .strip_prefix("MediaBrowser")
        .or_else(|| header_val.trim().strip_prefix("Emby"))
        .or_else(|| header_val.trim().strip_prefix("mediabrowser"))
        .or_else(|| header_val.trim().strip_prefix("emby"))
        .unwrap_or(header_val);

    for part in body.split(',') {
        if let Some((k, v)) = part.split_once('=')
            && k.trim().eq_ignore_ascii_case(&lower_wanted)
        {
            let unquoted = v.trim().trim_matches('"').trim_matches('\'').trim();
            if !unquoted.is_empty() {
                return Some(unquoted.to_string());
            }
        }
    }
    None
}

fn extract_token(headers: &HeaderMap, query: &JfQuery) -> Option<String> {
    for hdr_name in ["x-emby-token", "x-mediabrowser-token"] {
        if let Some(val) = headers
            .get(hdr_name)
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return Some(val.to_string());
        }
    }

    for hdr_name in ["x-emby-authorization", "authorization"] {
        if let Some(raw) = headers.get(hdr_name).and_then(|v| v.to_str().ok()) {
            let trimmed = raw.trim();
            if let Some(bearer) = trimmed
                .strip_prefix("Bearer ")
                .or_else(|| trimmed.strip_prefix("bearer "))
            {
                let t = bearer.trim();
                if !t.is_empty() {
                    return Some(t.to_string());
                }
            }
            if let Some(tok) = extract_auth_kv(trimmed, "token") {
                return Some(tok);
            }
        }
    }

    for qk in ["api_key", "apikey", "token"] {
        if let Some(val) = query.get(qk).map(str::trim).filter(|s| !s.is_empty()) {
            return Some(val.to_string());
        }
    }

    None
}

async fn authenticate_optional(
    state: &VideoHttpState,
    headers: &HeaderMap,
    query: &JfQuery,
) -> Option<(User, String)> {
    let token = extract_token(headers, query)?;
    if let Ok(user) = state.auth.authenticate_api_key(&token).await {
        let key = user.api_key.clone();
        return Some((user, key));
    }
    if let Ok(user) = state.auth.authenticate_jwt(&token).await {
        let key = user.api_key.clone();
        return Some((user, key));
    }
    None
}

async fn authenticate_required(
    state: &VideoHttpState,
    headers: &HeaderMap,
    query: &JfQuery,
) -> DomainResult<(User, String)> {
    authenticate_optional(state, headers, query)
        .await
        .ok_or_else(|| DomainError::Unauthorized("Jellyfin authentication required".into()))
}

fn verify_user_scope(auth_user: &User, raw_user_id: &str) -> DomainResult<()> {
    let trimmed = raw_user_id.trim();
    if trimmed.eq_ignore_ascii_case("me") {
        return Ok(());
    }
    let parsed =
        Uuid::parse_str(trimmed).map_err(|_| DomainError::NotFound("invalid user id".into()))?;
    if parsed == auth_user.id.0 {
        Ok(())
    } else {
        Err(DomainError::Forbidden(
            "not authorized to access another user's Jellyfin view".into(),
        ))
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct AuthenticateByNameBody {
    #[serde(alias = "Username", alias = "username")]
    username: String,
    #[serde(alias = "Pw", alias = "pw", alias = "Password", alias = "password")]
    pw: String,
}

async fn handle_authenticate_by_name(
    state: &VideoHttpState,
    headers: &HeaderMap,
    body: &Bytes,
) -> DomainResult<Response> {
    let req: AuthenticateByNameBody = serde_json::from_slice(body)
        .map_err(|e| DomainError::Validation(format!("invalid authentication payload: {e}")))?;

    let session = state.auth.login(&req.username, &req.pw).await?;
    let user = state.auth.authenticate_jwt(&session.token).await?;

    let auth_hdr = headers
        .get("x-emby-authorization")
        .or_else(|| headers.get("authorization"))
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let client = extract_auth_kv(auth_hdr, "client").unwrap_or_else(|| "Jellyfin Client".into());
    let device = extract_auth_kv(auth_hdr, "device").unwrap_or_else(|| "Device".into());
    let device_id =
        extract_auth_kv(auth_hdr, "deviceid").unwrap_or_else(|| format_jf_id(Uuid::now_v7()));
    let app_ver = extract_auth_kv(auth_hdr, "version").unwrap_or_else(|| "1.0.0".into());

    let access_token = user.api_key.clone();
    let payload = json!({
        "User": build_user_dto(&user),
        "SessionInfo": {
            "Id": format_jf_id(Uuid::now_v7()),
            "UserId": format_jf_id(user.id.0),
            "UserName": user.username,
            "Client": client,
            "DeviceName": device,
            "DeviceId": device_id,
            "ApplicationVersion": app_ver,
            "ServerId": JELLYFIN_SERVER_ID,
            "IsActive": true,
            "SupportsMediaControl": false,
            "SupportsRemoteControl": false
        },
        "AccessToken": access_token,
        "ServerId": JELLYFIN_SERVER_ID
    });
    Ok((StatusCode::OK, Json(payload)).into_response())
}

async fn dispatch_jellyfin(
    State(state): State<VideoHttpState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    match dispatch_jellyfin_inner(&state, &method, &uri, &headers, &body).await {
        Ok(resp) => resp,
        Err(err) => ApiError(err).into_response(),
    }
}

async fn dispatch_jellyfin_inner(
    state: &VideoHttpState,
    method: &Method,
    uri: &Uri,
    headers: &HeaderMap,
    body: &Bytes,
) -> DomainResult<Response> {
    let query = JfQuery::parse(uri.query());
    let raw_segments: Vec<String> = uri
        .path()
        .split('/')
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect();
    let lower_segments: Vec<String> = raw_segments
        .iter()
        .map(|s| s.to_ascii_lowercase())
        .collect();

    // Strip optional `/emby` compatibility prefix
    let (raw_slice, lower_slice): (&[String], &[String]) =
        if lower_segments.first().map(String::as_str) == Some("emby") {
            (&raw_segments[1..], &lower_segments[1..])
        } else {
            (&raw_segments[..], &lower_segments[..])
        };

    let segs: Vec<&str> = lower_slice.iter().map(String::as_str).collect();

    // 1. Public / Discovery / Auth endpoints (no token required)
    match (method, segs.as_slice()) {
        (&Method::GET, ["system", "info", "public"]) => {
            return Ok((
                StatusCode::OK,
                Json(json!({
                    "LocalAddress": "",
                    "ServerName": JELLYFIN_SERVER_NAME,
                    "Version": JELLYFIN_SERVER_VERSION,
                    "ProductName": "Jellyfin Server",
                    "OperatingSystem": std::env::consts::OS,
                    "Id": JELLYFIN_SERVER_ID,
                    "StartupWizardCompleted": true
                })),
            )
                .into_response());
        }
        (&Method::GET | &Method::POST, ["system", "ping"]) => {
            return Ok((StatusCode::OK, Json("Jellyfin Server")).into_response());
        }
        (&Method::GET, ["branding", "configuration"]) => {
            return Ok((
                StatusCode::OK,
                Json(json!({
                    "LoginDisclaimer": "",
                    "CustomCss": "",
                    "SplashscreenEnabled": false
                })),
            )
                .into_response());
        }
        (&Method::GET, ["quickconnect", "enabled"]) => {
            return Ok((StatusCode::OK, Json(false)).into_response());
        }
        (&Method::GET, ["users", "public"]) => {
            return Ok((StatusCode::OK, Json(Vec::<serde_json::Value>::new())).into_response());
        }
        (&Method::POST, ["users", "authenticatebyname"]) => {
            return handle_authenticate_by_name(state, headers, body).await;
        }
        (&Method::GET | &Method::HEAD, ["items", _, "images", _])
        | (&Method::GET | &Method::HEAD, ["items", _, "images", _, _]) => {
            let auth_opt = authenticate_optional(state, headers, &query)
                .await
                .map(|(u, _)| u);
            return playback::handle_image(state, &raw_slice[1], &query, auth_opt.as_ref()).await;
        }
        _ => {}
    }

    // 2. All remaining endpoints require valid Jellyfin API Key or JWT token
    let (user, api_key) = authenticate_required(state, headers, &query).await?;

    match (method, segs.as_slice()) {
        (&Method::GET, ["system", "info"]) => Ok((
            StatusCode::OK,
            Json(json!({
                "LocalAddress": "",
                "ServerName": JELLYFIN_SERVER_NAME,
                "Version": JELLYFIN_SERVER_VERSION,
                "ProductName": "Jellyfin Server",
                "OperatingSystem": std::env::consts::OS,
                "Id": JELLYFIN_SERVER_ID,
                "StartupWizardCompleted": true,
                "HasPendingRestart": false,
                "IsShuttingDown": false,
                "SupportsLibraryMonitor": false,
                "CanSelfRestart": false,
                "CanLaunchWebBrowser": false,
                "HasUpdateAvailable": false
            })),
        )
            .into_response()),

        (&Method::GET, ["users", "me"]) => {
            Ok((StatusCode::OK, Json(build_user_dto(&user))).into_response())
        }
        (&Method::GET, ["users", _]) => {
            verify_user_scope(&user, &raw_slice[1])?;
            Ok((StatusCode::OK, Json(build_user_dto(&user))).into_response())
        }

        (&Method::POST, ["sessions", "capabilities"])
        | (&Method::POST, ["sessions", "capabilities", "full"])
        | (&Method::POST, ["sessions", "logout"]) => Ok(StatusCode::NO_CONTENT.into_response()),

        (&Method::POST, ["sessions", "playing"]) => {
            playback::handle_session_playing(state, &user, body, false).await
        }
        (&Method::POST, ["sessions", "playing", "progress"]) => {
            playback::handle_session_playing(state, &user, body, false).await
        }
        (&Method::POST, ["sessions", "playing", "stopped"]) => {
            playback::handle_session_playing(state, &user, body, true).await
        }

        // Library Views
        (&Method::GET, ["userviews"]) => catalog::handle_views(state, &user).await,
        (&Method::GET, ["users", _, "views"]) => {
            verify_user_scope(&user, &raw_slice[1])?;
            catalog::handle_views(state, &user).await
        }
        (&Method::GET, ["users", _, "groupingoptions"]) => {
            verify_user_scope(&user, &raw_slice[1])?;
            Ok((StatusCode::OK, Json(Vec::<serde_json::Value>::new())).into_response())
        }

        // Items listing / resume / latest / counts / detail
        (&Method::GET, ["items"]) => catalog::handle_items(state, &user, &query, &api_key).await,
        (&Method::GET, ["users", _, "items"]) => {
            verify_user_scope(&user, &raw_slice[1])?;
            catalog::handle_items(state, &user, &query, &api_key).await
        }
        (&Method::GET, ["useritems", "resume"]) => {
            catalog::handle_resume(state, &user, &query, &api_key).await
        }
        (&Method::GET, ["users", _, "items", "resume"]) => {
            verify_user_scope(&user, &raw_slice[1])?;
            catalog::handle_resume(state, &user, &query, &api_key).await
        }
        (&Method::GET, ["items", "latest"]) => {
            catalog::handle_latest(state, &user, &query, &api_key).await
        }
        (&Method::GET, ["users", _, "items", "latest"]) => {
            verify_user_scope(&user, &raw_slice[1])?;
            catalog::handle_latest(state, &user, &query, &api_key).await
        }
        (&Method::GET, ["items", "counts"]) => catalog::handle_counts(state, &user).await,
        (&Method::GET, ["users", _, "items", "counts"]) => {
            verify_user_scope(&user, &raw_slice[1])?;
            catalog::handle_counts(state, &user).await
        }
        (&Method::GET, ["items", _]) => {
            catalog::handle_item_detail(state, &user, &raw_slice[1], &api_key).await
        }
        (&Method::GET, ["users", _, "items", _]) => {
            verify_user_scope(&user, &raw_slice[1])?;
            catalog::handle_item_detail(state, &user, &raw_slice[3], &api_key).await
        }

        // Shows: Seasons, Episodes, NextUp
        (&Method::GET, ["shows", _, "seasons"]) => {
            catalog::handle_show_seasons(state, &user, &raw_slice[1]).await
        }
        (&Method::GET, ["shows", _, "episodes"]) => {
            catalog::handle_show_episodes(state, &user, &raw_slice[1], &query, &api_key).await
        }
        (&Method::GET, ["shows", "nextup"]) => {
            catalog::handle_next_up(state, &user, &query, &api_key).await
        }

        // Genres & Search Hints
        (&Method::GET, ["genres"]) | (&Method::GET, ["musicgenres"]) => {
            catalog::handle_genres(state, &user).await
        }
        (&Method::GET, ["search", "hints"]) => {
            catalog::handle_search_hints(state, &user, &query).await
        }

        // PlaybackInfo
        (&Method::GET | &Method::POST, ["items", _, "playbackinfo"]) => {
            playback::handle_playback_info(state, &user, &raw_slice[1], &api_key).await
        }

        // Video direct/proxy streaming & download
        (&Method::GET | &Method::HEAD, ["videos", _, seg])
            if *seg == "stream" || seg.starts_with("stream.") =>
        {
            playback::handle_video_stream(state, &user, &raw_slice[1], headers, false).await
        }
        (&Method::GET | &Method::HEAD, ["items", _, "download"]) => {
            playback::handle_video_stream(state, &user, &raw_slice[1], headers, true).await
        }

        // HLS master & segment proxy for uncached tracked online HLS videos
        (&Method::GET | &Method::HEAD, ["videos", _, "hls", "master.m3u8"]) => {
            playback::handle_video_hls_master(state, &user, &raw_slice[1], headers, &api_key).await
        }
        (&Method::GET | &Method::HEAD, ["videos", _, "hls", "proxy"]) => {
            playback::handle_video_hls_proxy(state, &user, &raw_slice[1], &query, headers, &api_key)
                .await
        }

        // Subtitles
        (&Method::GET, ["videos", _, _, "subtitles", _, seg]) if seg.starts_with("stream.") => {
            playback::handle_subtitle_stream(state, &user, &raw_slice[1], &raw_slice[4]).await
        }

        // Played / Watched status sync
        (&Method::POST, ["users", _, "playeditems", _]) => {
            verify_user_scope(&user, &raw_slice[1])?;
            catalog::handle_played_item(state, &user, &raw_slice[3], true).await
        }
        (&Method::DELETE, ["users", _, "playeditems", _]) => {
            verify_user_scope(&user, &raw_slice[1])?;
            catalog::handle_played_item(state, &user, &raw_slice[3], false).await
        }
        (&Method::POST, ["userplayeditems", _]) => {
            catalog::handle_played_item(state, &user, &raw_slice[1], true).await
        }
        (&Method::DELETE, ["userplayeditems", _]) => {
            catalog::handle_played_item(state, &user, &raw_slice[1], false).await
        }

        // Favorites sync
        (&Method::POST, ["users", _, "favoriteitems", _]) => {
            verify_user_scope(&user, &raw_slice[1])?;
            catalog::handle_favorite_item(state, &user, &raw_slice[3], true).await
        }
        (&Method::DELETE, ["users", _, "favoriteitems", _]) => {
            verify_user_scope(&user, &raw_slice[1])?;
            catalog::handle_favorite_item(state, &user, &raw_slice[3], false).await
        }
        (&Method::POST, ["userfavoriteitems", _]) => {
            catalog::handle_favorite_item(state, &user, &raw_slice[1], true).await
        }
        (&Method::DELETE, ["userfavoriteitems", _]) => {
            catalog::handle_favorite_item(state, &user, &raw_slice[1], false).await
        }

        // Auxiliary UI endpoints called by Swiftfin / Infuse / SenPlayer
        (&Method::GET, ["displaypreferences", _]) => Ok((
            StatusCode::OK,
            Json(json!({
                "Id": raw_slice[1],
                "CustomPrefs": {}
            })),
        )
            .into_response()),
        (&Method::GET, ["items", _, "similar"])
        | (&Method::GET, ["shows", _, "similar"])
        | (&Method::GET, ["movies", _, "similar"]) => Ok((
            StatusCode::OK,
            Json(json!({
                "Items": [],
                "TotalRecordCount": 0,
                "StartIndex": 0
            })),
        )
            .into_response()),
        (&Method::GET, ["items", _, "specialfeatures"]) => {
            Ok((StatusCode::OK, Json(Vec::<serde_json::Value>::new())).into_response())
        }
        (&Method::GET, ["users", _, "items", _, "specialfeatures"]) => {
            verify_user_scope(&user, &raw_slice[1])?;
            Ok((StatusCode::OK, Json(Vec::<serde_json::Value>::new())).into_response())
        }
        (&Method::GET, ["items", _, "thememedia"]) => Ok((
            StatusCode::OK,
            Json(json!({
                "ThemeVideosResult": { "Items": [], "TotalRecordCount": 0 },
                "ThemeSongsResult": { "Items": [], "TotalRecordCount": 0 },
                "SoundtrackSongsResult": { "Items": [], "TotalRecordCount": 0 }
            })),
        )
            .into_response()),

        _ => Err(DomainError::NotFound(format!(
            "unsupported Jellyfin endpoint: {} {}",
            method,
            uri.path()
        ))),
    }
}

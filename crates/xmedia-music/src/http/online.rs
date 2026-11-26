use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::header::{
    ACCEPT_RANGES, CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_RANGE,
    CONTENT_SECURITY_POLICY, CONTENT_TYPE, IF_RANGE, RANGE, X_CONTENT_TYPE_OPTIONS,
};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use uuid::Uuid;
use xmedia_core::application::ports::ByteRangeSpec;
use xmedia_core::domain::error::DomainError;
use xmedia_core::http::{
    ApiError, ApiResult, WebAuth, parse_http_range_spec, sanitize_download_filename,
};
use xmedia_source::crud::{LoginActionRequest, sources_prefixed_crud_router};
use xmedia_source::proxy::{IMAGE_PROXY_CACHE_CONTROL, IMAGE_PROXY_CSP};

use crate::application::dto::TrackDto;
use crate::application::online::{
    LoginActionResponseDto, MusicSourceTestInput, MusicSourceTestReportDto,
    OnlineMusicExploreResponseDto, OnlineMusicResolvedTrackDto, OnlineMusicSearchResponseDto,
    OnlineMusicStreamResponseDto, StructuredLyricsDto, TrackOnlineMusicBatchInput,
    TrackOnlineMusicInput,
};
use crate::domain::catalog::TrackId;
use crate::domain::online::{MusicSourceSpec, MusicTrackContext};
use crate::http::state::MusicHttpState;

pub fn router() -> Router<MusicHttpState> {
    Router::new()
        .merge(sources_prefixed_crud_router::<
            MusicHttpState,
            MusicSourceSpec,
        >())
        .route("/sources/{id}/login-action", post(run_login_action))
        .route("/sources/{id}/test", post(test_source))
        // Online search / explore / track-info / stream / lyrics / proxies
        .route("/online/search", post(online_search))
        .route("/online/explore", post(online_explore))
        .route("/online/track-info", post(online_track_info))
        .route("/online/stream", post(online_stream))
        .route("/online/lyrics", post(online_lyrics))
        .route("/online/track", post(track_online_track))
        .route("/online/track-batch", post(track_online_tracks_batch))
        .route("/tracks/{id}/online/cache", post(cache_tracked_track))
        .route("/online/cover-proxy", get(online_cover_proxy))
        .route("/online/audio-proxy", get(online_audio_proxy))
        // Per-user online/remote music favorites & playback progress
        .nest(
            "/online/items",
            xmedia_core::http::online_items::online_items_router(),
        )
}

#[derive(Debug, Deserialize)]
struct OnlineSearchRequest {
    keyword: String,
    source_ids: Option<Vec<Uuid>>,
    #[serde(default = "default_page")]
    page: u32,
    #[serde(default = "default_limit")]
    limit: u32,
}

fn default_page() -> u32 {
    1
}

fn default_limit() -> u32 {
    20
}

#[derive(Debug, Deserialize)]
struct OnlineExploreRequest {
    source_id: Uuid,
    explore_url: Option<String>,
    #[serde(default = "default_page")]
    page: u32,
    #[serde(default = "default_limit")]
    limit: u32,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct OnlineTrackInfoRequest {
    source_id: Uuid,
    track_id: Option<String>,
    detail_url: Option<String>,
    quality: Option<String>,
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
    duration_secs: Option<u32>,
    cover_url: Option<String>,
    cover_id: Option<String>,
    url_id: Option<String>,
    lyric_id: Option<String>,
    play_url: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct OnlineStreamRequest {
    source_id: Uuid,
    track_id: Option<String>,
    url_id: Option<String>,
    play_url: Option<String>,
    quality: Option<String>,
    detail_url: Option<String>,
    title: Option<String>,
    artist: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct OnlineLyricsRequest {
    source_id: Uuid,
    track_id: Option<String>,
    lyric_id: Option<String>,
    title: Option<String>,
    artist: Option<String>,
    detail_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CoverProxyQuery {
    url: Option<String>,
    source_id: Option<Uuid>,
    cover_id: Option<String>,
    track_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AudioProxyQuery {
    url: String,
    source_id: Option<Uuid>,
    download: Option<String>,
    filename: Option<String>,
    format: Option<String>,
}

fn is_truthy_query_flag(raw: Option<&str>) -> bool {
    matches!(
        raw.map(str::trim)
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn infer_audio_extension(content_type: &str, format_hint: Option<&str>, raw_url: &str) -> String {
    let base_ct = content_type
        .split(';')
        .next()
        .unwrap_or(content_type)
        .trim()
        .to_ascii_lowercase();
    match base_ct.as_str() {
        "audio/flac" | "audio/x-flac" => return "flac".to_string(),
        "audio/mpeg" | "audio/mp3" => return "mp3".to_string(),
        "audio/aac" | "audio/x-aac" | "audio/aacp" => return "aac".to_string(),
        "audio/mp4" | "audio/x-m4a" | "audio/m4a" => {
            return "m4a".to_string();
        }
        "audio/ogg" | "application/ogg" => return "ogg".to_string(),
        "audio/opus" => return "opus".to_string(),
        "audio/wav" | "audio/x-wav" | "audio/wave" => return "wav".to_string(),
        "audio/aiff" | "audio/x-aiff" => return "aiff".to_string(),
        "audio/webm" => return "webm".to_string(),
        "audio/x-ape" | "audio/ape" => return "ape".to_string(),
        "audio/x-wavpack" | "audio/wavpack" => return "wv".to_string(),
        _ => {}
    }

    if let Some(hint) = format_hint.map(str::trim).filter(|s| !s.is_empty()) {
        let lower = hint.to_ascii_lowercase();
        let clean: String = lower
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect();
        if let Ok(fmt) = clean.parse::<crate::domain::catalog::AudioFormat>() {
            return fmt.extension().to_string();
        }
        if clean == "webm" {
            return clean;
        }
    }

    let path_part = raw_url.split('?').next().unwrap_or(raw_url);
    if let Some((_, ext)) = path_part.rsplit_once('.') {
        let lower = ext.trim().to_ascii_lowercase();
        if let Ok(fmt) = lower.parse::<crate::domain::catalog::AudioFormat>() {
            return fmt.extension().to_string();
        }
        if lower == "webm" {
            return lower;
        }
    }

    "mp3".to_string()
}

fn encode_attachment_disposition(filename: &str) -> Option<HeaderValue> {
    let ascii_fallback: String = filename
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ' ') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let encoded_name: String = filename
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.') {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    HeaderValue::from_str(&format!(
        "attachment; filename=\"{ascii_fallback}\"; filename*=UTF-8''{encoded_name}"
    ))
    .ok()
}

async fn run_login_action(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
    Json(req): Json<LoginActionRequest>,
) -> ApiResult<Json<LoginActionResponseDto>> {
    let out = state
        .services
        .online
        .run_login_action(&user, id, req.login_info, req.action)
        .await?;
    Ok(Json(out))
}

async fn test_source(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
    Json(req): Json<MusicSourceTestInput>,
) -> ApiResult<Json<MusicSourceTestReportDto>> {
    let out = state.services.online.test_source(&user, id, req).await?;
    Ok(Json(out))
}

async fn online_search(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Json(req): Json<OnlineSearchRequest>,
) -> ApiResult<Json<OnlineMusicSearchResponseDto>> {
    let out = state
        .services
        .online
        .search(&user, &req.keyword, req.source_ids, req.page, req.limit)
        .await?;
    Ok(Json(out))
}

async fn online_explore(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Json(req): Json<OnlineExploreRequest>,
) -> ApiResult<Json<OnlineMusicExploreResponseDto>> {
    let out = state
        .services
        .online
        .explore(
            &user,
            req.source_id,
            req.explore_url.as_deref(),
            req.page,
            req.limit,
        )
        .await?;
    Ok(Json(out))
}

async fn online_track_info(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Json(req): Json<OnlineTrackInfoRequest>,
) -> ApiResult<Json<OnlineMusicResolvedTrackDto>> {
    let ctx = MusicTrackContext {
        track_id: req.track_id.unwrap_or_default(),
        title: req.title.unwrap_or_default(),
        artist: req.artist.unwrap_or_default(),
        album: req.album,
        duration_secs: req.duration_secs,
        cover_url: req.cover_url,
        cover_id: req.cover_id,
        url_id: req.url_id,
        lyric_id: req.lyric_id,
        play_url: req.play_url,
        detail_url: req.detail_url,
        quality: req.quality.clone(),
    };
    let out = state
        .services
        .online
        .fetch_track_info(&user, req.source_id, ctx, req.quality)
        .await?;
    Ok(Json(out))
}

async fn online_stream(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Json(req): Json<OnlineStreamRequest>,
) -> ApiResult<Json<OnlineMusicStreamResponseDto>> {
    let ctx = MusicTrackContext {
        track_id: req.track_id.unwrap_or_default(),
        title: req.title.unwrap_or_default(),
        artist: req.artist.unwrap_or_default(),
        url_id: req.url_id,
        play_url: req.play_url,
        detail_url: req.detail_url,
        quality: req.quality.clone(),
        ..Default::default()
    };
    let out = state
        .services
        .online
        .fetch_stream(&user, req.source_id, ctx, req.quality)
        .await?;
    Ok(Json(out))
}

async fn online_lyrics(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Json(req): Json<OnlineLyricsRequest>,
) -> ApiResult<Json<StructuredLyricsDto>> {
    let ctx = MusicTrackContext {
        track_id: req.track_id.unwrap_or_default(),
        title: req.title.unwrap_or_default(),
        artist: req.artist.unwrap_or_default(),
        lyric_id: req.lyric_id,
        detail_url: req.detail_url,
        ..Default::default()
    };
    let out = state
        .services
        .online
        .fetch_lyrics(&user, req.source_id, ctx)
        .await?;
    Ok(Json(out))
}

async fn track_online_track(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Json(req): Json<TrackOnlineMusicInput>,
) -> ApiResult<Json<TrackDto>> {
    let out = state.services.online.track_online_track(&user, req).await?;
    Ok(Json(out))
}

async fn track_online_tracks_batch(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Json(req): Json<TrackOnlineMusicBatchInput>,
) -> ApiResult<Json<Vec<TrackDto>>> {
    let out = state
        .services
        .online
        .track_online_tracks_batch(&user, req.tracks)
        .await?;
    Ok(Json(out))
}

async fn cache_tracked_track(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<TrackDto>> {
    let out = state
        .services
        .online
        .cache_tracked_track(&user, TrackId(id))
        .await?;
    Ok(Json(out))
}

async fn online_cover_proxy(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Query(q): Query<CoverProxyQuery>,
) -> Result<Response, ApiError> {
    let (bytes, mime) = if let Some(url) = q.url.as_deref().map(str::trim).filter(|s| !s.is_empty())
    {
        state
            .services
            .online
            .fetch_cover_proxy(&user, url, q.source_id)
            .await?
    } else if let Some(source_id) = q.source_id {
        state
            .services
            .online
            .fetch_cover_proxy_by_id(
                &user,
                source_id,
                q.cover_id.as_deref(),
                q.track_id.as_deref(),
            )
            .await?
    } else {
        return Err(DomainError::Validation("缺少封面 URL 或 source_id 参数".into()).into());
    };
    let mut resp = Response::new(Body::from(bytes));
    if let Ok(hv) = HeaderValue::from_str(mime) {
        resp.headers_mut().insert(CONTENT_TYPE, hv);
    }
    resp.headers_mut()
        .insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    resp.headers_mut().insert(
        CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(IMAGE_PROXY_CSP),
    );
    resp.headers_mut().insert(
        CACHE_CONTROL,
        HeaderValue::from_static(IMAGE_PROXY_CACHE_CONTROL),
    );
    Ok(resp)
}

async fn online_audio_proxy(
    State(state): State<MusicHttpState>,
    WebAuth(user): WebAuth,
    Query(q): Query<AudioProxyQuery>,
    req_headers: HeaderMap,
) -> Result<Response, ApiError> {
    let source_id = q
        .source_id
        .ok_or_else(|| ApiError(DomainError::Validation("缺少 source_id 参数".into())))?;

    let range_spec = parse_http_range_spec(&req_headers);
    if range_spec == ByteRangeSpec::Malformed {
        return Err(DomainError::Validation("无效的 Range 请求头".into()).into());
    }
    let range_header = if range_spec != ByteRangeSpec::Full {
        req_headers
            .get(RANGE)
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
    } else {
        None
    };
    let if_range_header = range_header.and_then(|_| {
        req_headers
            .get(IF_RANGE)
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
    });

    let proxy_resp = state
        .services
        .online
        .stream_audio_proxy(&user, &q.url, source_id, range_header, if_range_header)
        .await?;

    let mut resp = Response::new(Body::from_stream(proxy_resp.stream));
    *resp.status_mut() = if proxy_resp.is_partial {
        StatusCode::PARTIAL_CONTENT
    } else {
        StatusCode::OK
    };

    if let Ok(hv) = HeaderValue::from_str(&proxy_resp.content_type) {
        resp.headers_mut().insert(CONTENT_TYPE, hv);
    }
    if is_truthy_query_flag(q.download.as_deref()) {
        let ext = infer_audio_extension(&proxy_resp.content_type, q.format.as_deref(), &q.url);
        let base_name = q
            .filename
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("track");
        let filename = sanitize_download_filename(base_name, &ext);
        if let Some(disp) = encode_attachment_disposition(&filename) {
            resp.headers_mut().insert(CONTENT_DISPOSITION, disp);
        }
    }
    if let Some(len) = proxy_resp.content_length {
        resp.headers_mut()
            .insert(CONTENT_LENGTH, HeaderValue::from(len));
    }
    if let Some(cr) = proxy_resp.content_range
        && let Ok(hv) = HeaderValue::from_str(&cr)
    {
        resp.headers_mut().insert(CONTENT_RANGE, hv);
    }
    if let Some(ar) = proxy_resp.accept_ranges
        && let Ok(hv) = HeaderValue::from_str(&ar)
    {
        resp.headers_mut().insert(ACCEPT_RANGES, hv);
    }
    resp.headers_mut()
        .insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    resp.headers_mut().insert(
        CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("sandbox; default-src 'none'; media-src 'self'"),
    );
    resp.headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
    Ok(resp)
}

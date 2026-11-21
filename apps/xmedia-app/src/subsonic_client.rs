use std::fmt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::header::{
    ACCEPT_RANGES, CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE,
    ETAG, IF_RANGE, LAST_MODIFIED, RANGE,
};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::Response;
use futures_util::StreamExt;
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::AsyncWriteExt;
use url::Url;
use uuid::Uuid;

use crate::opds_client::{
    LimitedByteStream, MAX_IMAGE_BYTES, MAX_METADATA_BYTES, ReadLimitError, RemoteImportResultDto,
    apply_proxy_security_headers, is_safe_raster_image_content_type,
    parse_filename_from_content_disposition, read_response_text_limited, tokio_util_file_stream,
};
use crate::remote_sources::{RemoteSource, normalize_remote_url};

const SUBSONIC_API_VERSION: &str = "1.16.1";
const SUBSONIC_CLIENT_NAME: &str = "XMedia";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const TEST_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_IMPORT_BYTES: u64 = 512 * 1024 * 1024;
const MAX_STREAM_ERROR_SNIFF_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubsonicTestResult {
    pub ok: bool,
    pub latency_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_subsonic: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubsonicAlbumDto {
    pub id: String,
    pub name: String,
    pub artist: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub year: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub song_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubsonicSongDto {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suffix: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bit_rate: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover_url: Option<String>,
    pub stream_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubsonicAlbumDetailDto {
    pub album: SubsonicAlbumDto,
    pub songs: Vec<SubsonicSongDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubsonicSearchResultDto {
    pub albums: Vec<SubsonicAlbumDto>,
    pub songs: Vec<SubsonicSongDto>,
}

#[derive(Debug)]
pub enum SubsonicError {
    BadRequest(String),
    Upstream(String),
    Unsupported(String),
    TooLarge(String),
    Internal(String),
}

impl SubsonicError {
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Upstream(_) => StatusCode::BAD_GATEWAY,
            Self::Unsupported(_) => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Self::TooLarge(_) => StatusCode::PAYLOAD_TOO_LARGE,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::BadRequest(_) => "invalid_request",
            Self::Upstream(_) => "upstream_error",
            Self::Unsupported(_) => "unsupported_media_type",
            Self::TooLarge(_) => "payload_too_large",
            Self::Internal(_) => "internal_error",
        }
    }
}

impl fmt::Display for SubsonicError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadRequest(m)
            | Self::Upstream(m)
            | Self::Unsupported(m)
            | Self::TooLarge(m)
            | Self::Internal(m) => {
                write!(f, "{m}")
            }
        }
    }
}

impl std::error::Error for SubsonicError {}

#[derive(Clone)]
pub struct SubsonicClient {
    secure: reqwest::Client,
    insecure: reqwest::Client,
    temp_dir: PathBuf,
}

impl SubsonicClient {
    pub fn new(secure: reqwest::Client, insecure: reqwest::Client, temp_dir: PathBuf) -> Self {
        Self {
            secure,
            insecure,
            temp_dir,
        }
    }

    fn client_for(&self, allow_insecure_tls: bool) -> &reqwest::Client {
        if allow_insecure_tls {
            &self.insecure
        } else {
            &self.secure
        }
    }

    pub async fn test_subsonic(
        &self,
        raw_url: &str,
        username: &str,
        password: &str,
        allow_insecure_tls: bool,
    ) -> SubsonicTestResult {
        let normalized_url = match normalize_remote_url(raw_url) {
            Ok(u) => u,
            Err(err) => {
                return SubsonicTestResult {
                    ok: false,
                    latency_ms: 0,
                    server_version: None,
                    open_subsonic: None,
                    error: Some(err.to_string()),
                };
            }
        };

        let endpoint_url =
            match build_subsonic_url(&normalized_url, "ping.view", username, password, &[]) {
                Ok(u) => u,
                Err(err) => {
                    return SubsonicTestResult {
                        ok: false,
                        latency_ms: 0,
                        server_version: None,
                        open_subsonic: None,
                        error: Some(err.to_string()),
                    };
                }
            };

        let started = Instant::now();
        let resp = match self
            .client_for(allow_insecure_tls)
            .get(&endpoint_url)
            .timeout(TEST_TIMEOUT)
            .send()
            .await
        {
            Ok(r) => r,
            Err(err) => {
                return SubsonicTestResult {
                    ok: false,
                    latency_ms: started.elapsed().as_millis() as u64,
                    server_version: None,
                    open_subsonic: None,
                    error: Some(format!("Connection failed: {}", err.without_url())),
                };
            }
        };
        let latency_ms = started.elapsed().as_millis() as u64;

        if !resp.status().is_success() {
            return SubsonicTestResult {
                ok: false,
                latency_ms,
                server_version: None,
                open_subsonic: None,
                error: Some(format!(
                    "Subsonic server returned HTTP {}",
                    resp.status().as_u16()
                )),
            };
        }

        let body = match read_response_text_limited(resp, MAX_METADATA_BYTES).await {
            Ok(t) => t,
            Err(ReadLimitError::TooLarge(limit)) => {
                return SubsonicTestResult {
                    ok: false,
                    latency_ms,
                    server_version: None,
                    open_subsonic: None,
                    error: Some(format!(
                        "Subsonic response exceeded {limit} byte metadata limit"
                    )),
                };
            }
            Err(ReadLimitError::Read(err)) => {
                return SubsonicTestResult {
                    ok: false,
                    latency_ms,
                    server_version: None,
                    open_subsonic: None,
                    error: Some(format!("Failed to read Subsonic response: {err}")),
                };
            }
        };

        let json: Value = match serde_json::from_str(&body) {
            Ok(v) => v,
            Err(err) => {
                return SubsonicTestResult {
                    ok: false,
                    latency_ms,
                    server_version: None,
                    open_subsonic: None,
                    error: Some(format!("Invalid Subsonic JSON response: {err}")),
                };
            }
        };

        match extract_subsonic_ok(&json) {
            Ok(inner) => {
                let server_version = inner
                    .get("serverVersion")
                    .or_else(|| inner.get("version"))
                    .and_then(Value::as_str)
                    .map(str::to_string);
                let open_subsonic = inner.get("openSubsonic").and_then(Value::as_bool);
                SubsonicTestResult {
                    ok: true,
                    latency_ms,
                    server_version,
                    open_subsonic,
                    error: None,
                }
            }
            Err(err) => SubsonicTestResult {
                ok: false,
                latency_ms,
                server_version: None,
                open_subsonic: None,
                error: Some(err.to_string()),
            },
        }
    }

    async fn call_json(
        &self,
        source: &RemoteSource,
        endpoint: &str,
        extra_params: &[(&str, String)],
    ) -> Result<Value, SubsonicError> {
        let url = build_subsonic_url(
            &source.url,
            endpoint,
            &source.username,
            &source.password,
            extra_params,
        )?;
        let resp = self
            .client_for(source.allow_insecure_tls)
            .get(&url)
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await
            .map_err(|e| {
                SubsonicError::Upstream(format!("Subsonic request failed: {}", e.without_url()))
            })?;

        if !resp.status().is_success() {
            return Err(SubsonicError::Upstream(format!(
                "Subsonic server returned HTTP {}",
                resp.status().as_u16()
            )));
        }

        let body = read_response_text_limited(resp, MAX_METADATA_BYTES)
            .await
            .map_err(|e| match e {
                ReadLimitError::TooLarge(limit) => SubsonicError::TooLarge(format!(
                    "Subsonic JSON response exceeded {limit} byte metadata limit"
                )),
                ReadLimitError::Read(msg) => {
                    SubsonicError::Upstream(format!("Failed to read Subsonic JSON: {msg}"))
                }
            })?;
        let json: Value = serde_json::from_str(&body)
            .map_err(|e| SubsonicError::Upstream(format!("Invalid Subsonic JSON: {e}")))?;
        extract_subsonic_ok(&json).cloned()
    }

    pub async fn list_albums(
        &self,
        source: &RemoteSource,
        list_type: &str,
        size: u32,
        offset: u32,
    ) -> Result<Vec<SubsonicAlbumDto>, SubsonicError> {
        let resolved_type = if list_type.trim().is_empty() {
            "newest"
        } else {
            list_type.trim()
        };
        let clamped_size = size.clamp(1, 500);

        let inner = self
            .call_json(
                source,
                "getAlbumList2.view",
                &[
                    ("type", resolved_type.to_string()),
                    ("size", clamped_size.to_string()),
                    ("offset", offset.to_string()),
                ],
            )
            .await?;

        let albums = inner
            .pointer("/albumList2/album")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| parse_album_dto(&source.id, v))
                    .collect()
            })
            .unwrap_or_default();

        Ok(albums)
    }

    pub async fn get_album(
        &self,
        source: &RemoteSource,
        album_id: &str,
    ) -> Result<SubsonicAlbumDetailDto, SubsonicError> {
        let inner = self
            .call_json(source, "getAlbum.view", &[("id", album_id.to_string())])
            .await?;

        let album_val = inner.get("album").ok_or_else(|| {
            SubsonicError::Upstream("Subsonic getAlbum missing 'album' node".into())
        })?;
        let album = parse_album_dto(&source.id, album_val)
            .ok_or_else(|| SubsonicError::Upstream("Invalid Subsonic album payload".into()))?;
        let songs = album_val
            .get("song")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| parse_song_dto(&source.id, v))
                    .collect()
            })
            .unwrap_or_default();

        Ok(SubsonicAlbumDetailDto { album, songs })
    }

    pub async fn search(
        &self,
        source: &RemoteSource,
        query: &str,
    ) -> Result<SubsonicSearchResultDto, SubsonicError> {
        let inner = self
            .call_json(
                source,
                "search3.view",
                &[
                    ("query", query.trim().to_string()),
                    ("artistCount", "0".to_string()),
                    ("albumCount", "25".to_string()),
                    ("songCount", "50".to_string()),
                ],
            )
            .await?;

        let sr = inner.get("searchResult3");
        let albums = sr
            .and_then(|v| v.get("album"))
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| parse_album_dto(&source.id, v))
                    .collect()
            })
            .unwrap_or_default();
        let songs = sr
            .and_then(|v| v.get("song"))
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| parse_song_dto(&source.id, v))
                    .collect()
            })
            .unwrap_or_default();

        Ok(SubsonicSearchResultDto { albums, songs })
    }

    pub async fn proxy_cover(
        &self,
        source: &RemoteSource,
        cover_id: &str,
    ) -> Result<Response, SubsonicError> {
        let url = build_subsonic_url(
            &source.url,
            "getCoverArt.view",
            &source.username,
            &source.password,
            &[("id", cover_id.to_string())],
        )?;
        let resp = self
            .client_for(source.allow_insecure_tls)
            .get(&url)
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await
            .map_err(|e| {
                SubsonicError::Upstream(format!(
                    "Subsonic cover request failed: {}",
                    e.without_url()
                ))
            })?;

        if !resp.status().is_success() {
            return Err(SubsonicError::Upstream(format!(
                "Subsonic cover endpoint returned HTTP {}",
                resp.status().as_u16()
            )));
        }

        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .trim()
            .to_string();
        if is_metadata_content_type(&content_type) {
            return Err(SubsonicError::Upstream(
                "Subsonic server returned metadata error instead of cover image".to_string(),
            ));
        }
        if !is_safe_raster_image_content_type(&content_type) {
            return Err(SubsonicError::Unsupported(format!(
                "Subsonic cover endpoint returned disallowed Content-Type '{content_type}'"
            )));
        }

        if let Some(len) = resp.content_length()
            && len > MAX_IMAGE_BYTES
        {
            return Err(SubsonicError::TooLarge(format!(
                "Subsonic cover image exceeds maximum allowed size of {MAX_IMAGE_BYTES} bytes"
            )));
        }

        let content_length = resp
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| HeaderValue::from_str(s).ok());

        let limited = LimitedByteStream::new(resp.bytes_stream(), MAX_IMAGE_BYTES);
        let mut response = Response::new(Body::from_stream(limited));
        *response.status_mut() = StatusCode::OK;
        if let Ok(ct) = HeaderValue::from_str(&content_type) {
            response.headers_mut().insert(CONTENT_TYPE, ct);
        }
        if let Some(cl) = content_length {
            response.headers_mut().insert(CONTENT_LENGTH, cl);
        }
        response.headers_mut().insert(
            CACHE_CONTROL,
            HeaderValue::from_static("private, max-age=3600"),
        );
        apply_proxy_security_headers(response.headers_mut());
        Ok(response)
    }

    pub async fn proxy_stream(
        &self,
        source: &RemoteSource,
        song_id: &str,
        req_headers: &HeaderMap,
        is_head: bool,
    ) -> Result<Response, SubsonicError> {
        let url = build_subsonic_url(
            &source.url,
            "stream.view",
            &source.username,
            &source.password,
            &[("id", song_id.to_string())],
        )?;

        let client = self.client_for(source.allow_insecure_tls);
        let mut req = if is_head {
            client.head(&url)
        } else {
            client.get(&url)
        };
        if let Some(range) = req_headers.get(RANGE)
            && let Ok(val) = reqwest::header::HeaderValue::from_bytes(range.as_bytes())
        {
            req = req.header(reqwest::header::RANGE, val);
        }
        if let Some(if_range) = req_headers.get(IF_RANGE)
            && let Ok(val) = reqwest::header::HeaderValue::from_bytes(if_range.as_bytes())
        {
            req = req.header(reqwest::header::IF_RANGE, val);
        }

        let upstream = req.send().await.map_err(|e| {
            SubsonicError::Upstream(format!(
                "Subsonic stream request failed: {}",
                e.without_url()
            ))
        })?;

        let status =
            StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
        if !status.is_success() && status != StatusCode::RANGE_NOT_SATISFIABLE {
            return Err(SubsonicError::Upstream(format!(
                "Subsonic stream endpoint returned HTTP {}",
                status.as_u16()
            )));
        }

        let content_type = upstream
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .trim()
            .to_string();

        if is_disallowed_stream_html_content_type(&content_type) {
            return Err(SubsonicError::Unsupported(format!(
                "Subsonic stream endpoint returned disallowed Content-Type '{content_type}'"
            )));
        }

        if is_metadata_content_type(&content_type) {
            if !is_head
                && content_type.to_ascii_lowercase().contains("json")
                && let Ok(body) =
                    read_response_text_limited(upstream, MAX_STREAM_ERROR_SNIFF_BYTES).await
                && let Ok(json) = serde_json::from_str::<Value>(&body)
                && let Err(sub_err) = extract_subsonic_ok(&json)
            {
                return Err(sub_err);
            }
            return Err(SubsonicError::Upstream(
                "Subsonic server returned metadata error instead of audio stream".to_string(),
            ));
        }

        let mut out_headers = HeaderMap::new();
        for hdr in [
            CONTENT_TYPE,
            CONTENT_LENGTH,
            CONTENT_RANGE,
            ACCEPT_RANGES,
            ETAG,
            LAST_MODIFIED,
            CONTENT_DISPOSITION,
        ] {
            if let Some(val) = upstream.headers().get(hdr.as_str())
                && let Ok(axum_val) = HeaderValue::from_bytes(val.as_bytes())
            {
                out_headers.insert(hdr, axum_val);
            }
        }
        // Only advertise Accept-Ranges: bytes when upstream supports Range or provides Content-Length.
        if !out_headers.contains_key(ACCEPT_RANGES)
            && (status == StatusCode::PARTIAL_CONTENT
                || out_headers.contains_key(CONTENT_RANGE)
                || out_headers.contains_key(CONTENT_LENGTH))
        {
            out_headers.insert(ACCEPT_RANGES, HeaderValue::from_static("bytes"));
        }
        apply_proxy_security_headers(&mut out_headers);

        let body = if is_head {
            Body::empty()
        } else {
            Body::from_stream(upstream.bytes_stream())
        };
        let mut response = Response::new(body);
        *response.status_mut() = status;
        *response.headers_mut() = out_headers;
        Ok(response)
    }

    pub async fn import_song_to_local(
        &self,
        source: &RemoteSource,
        song_id: &str,
        filename_hint: Option<&str>,
        local_gateway_url: &str,
        local_token: &str,
    ) -> Result<RemoteImportResultDto, SubsonicError> {
        let mut resp = self
            .fetch_song_binary(source, "download.view", song_id)
            .await;

        let needs_fallback = match &resp {
            Err(_) => true,
            Ok(r) => {
                !r.status().is_success()
                    || is_metadata_content_type(
                        r.headers()
                            .get(reqwest::header::CONTENT_TYPE)
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or(""),
                    )
            }
        };

        if needs_fallback {
            resp = self.fetch_song_binary(source, "stream.view", song_id).await;
        }

        let resp = resp?;
        if !resp.status().is_success() {
            return Err(SubsonicError::Upstream(format!(
                "Remote Subsonic audio download returned HTTP {}",
                resp.status().as_u16()
            )));
        }

        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        if is_metadata_content_type(&content_type)
            || is_disallowed_stream_html_content_type(&content_type)
        {
            return Err(SubsonicError::Upstream(
                "Remote Subsonic server returned metadata instead of audio payload".to_string(),
            ));
        }

        let content_disposition = resp
            .headers()
            .get(reqwest::header::CONTENT_DISPOSITION)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);

        let temp_file = stream_audio_to_temp_file(resp, MAX_IMPORT_BYTES, &self.temp_dir).await?;

        let mut song_meta_title: Option<String> = None;
        let mut song_meta_suffix: Option<String> = None;
        if let Ok(inner) = self
            .call_json(source, "getSong.view", &[("id", song_id.to_string())])
            .await
            && let Some(song) = inner.get("song")
        {
            song_meta_title = song
                .get("title")
                .and_then(Value::as_str)
                .map(str::to_string);
            song_meta_suffix = song
                .get("suffix")
                .and_then(Value::as_str)
                .map(|s| s.trim().trim_start_matches('.').to_ascii_lowercase());
        }

        let filename = resolve_audio_filename(
            filename_hint,
            content_disposition.as_deref(),
            &content_type,
            song_meta_title.as_deref(),
            song_meta_suffix.as_deref(),
            song_id,
        );

        let file = tokio::fs::File::open(&temp_file.path).await.map_err(|e| {
            SubsonicError::Internal(format!("Failed to reopen temp audio file: {e}"))
        })?;
        let stream = tokio_util_file_stream(file);
        let part = reqwest::multipart::Part::stream_with_length(
            reqwest::Body::wrap_stream(stream),
            temp_file.size,
        )
        .file_name(filename.clone());
        let form = reqwest::multipart::Form::new().part("file", part);

        let upload_url = format!("{local_gateway_url}/api/v1/music/tracks/upload");
        let upload_resp = self
            .secure
            .post(&upload_url)
            .bearer_auth(local_token)
            .multipart(form)
            .send()
            .await
            .map_err(|e| {
                SubsonicError::Internal(format!(
                    "Failed to upload track to local library: {}",
                    e.without_url()
                ))
            })?;

        let status = upload_resp.status();
        let body_bytes = upload_resp.bytes().await.map_err(|e| {
            SubsonicError::Internal(format!(
                "Failed to read local music upload response: {}",
                e.without_url()
            ))
        })?;
        let parsed_json: Option<Value> = serde_json::from_slice(&body_bytes).ok();

        if !status.is_success() {
            let msg = parsed_json
                .as_ref()
                .and_then(|v| v.pointer("/error/message").and_then(Value::as_str))
                .map(str::to_string)
                .unwrap_or_else(|| {
                    let snippet = String::from_utf8_lossy(&body_bytes);
                    let trimmed = snippet.trim();
                    if trimmed.is_empty() {
                        "Local music library rejected uploaded track".to_string()
                    } else {
                        trimmed.chars().take(200).collect()
                    }
                });
            if status == StatusCode::PAYLOAD_TOO_LARGE {
                return Err(SubsonicError::TooLarge(format!(
                    "Local music import rejected (HTTP 413): {msg}"
                )));
            }
            return Err(SubsonicError::Upstream(format!(
                "Local music import failed (HTTP {}): {msg}",
                status.as_u16()
            )));
        }

        let resp_json = parsed_json.ok_or_else(|| {
            SubsonicError::Internal("Local music upload succeeded but returned invalid JSON".into())
        })?;

        let first = resp_json
            .get("uploaded_tracks")
            .and_then(Value::as_array)
            .and_then(|arr| arr.first())
            .ok_or_else(|| {
                SubsonicError::Internal(
                    "Local music upload succeeded but returned no uploaded tracks".into(),
                )
            })?;

        let imported_id = first
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let title = first
            .get("title")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or(song_meta_title)
            .unwrap_or(filename);

        Ok(RemoteImportResultDto {
            media_kind: "music".to_string(),
            imported_id,
            title,
        })
    }

    async fn fetch_song_binary(
        &self,
        source: &RemoteSource,
        endpoint: &str,
        song_id: &str,
    ) -> Result<reqwest::Response, SubsonicError> {
        let url = build_subsonic_url(
            &source.url,
            endpoint,
            &source.username,
            &source.password,
            &[("id", song_id.to_string())],
        )?;
        self.client_for(source.allow_insecure_tls)
            .get(&url)
            .send()
            .await
            .map_err(|e| {
                SubsonicError::Upstream(format!("Subsonic {endpoint} failed: {}", e.without_url()))
            })
    }
}

struct TempAudioFile {
    path: PathBuf,
    size: u64,
}

impl Drop for TempAudioFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

async fn stream_audio_to_temp_file(
    resp: reqwest::Response,
    max_bytes: u64,
    temp_dir: &Path,
) -> Result<TempAudioFile, SubsonicError> {
    if let Some(len) = resp.content_length()
        && len > max_bytes
    {
        return Err(SubsonicError::TooLarge(format!(
            "Remote audio exceeds 512MB import limit ({len} bytes)"
        )));
    }

    tokio::fs::create_dir_all(temp_dir)
        .await
        .map_err(|e| SubsonicError::Internal(format!("Failed to prepare temp directory: {e}")))?;
    let path = temp_dir.join(format!("xmedia-subsonic-import-{}.tmp", Uuid::new_v4()));
    let mut file = tokio::fs::File::create(&path)
        .await
        .map_err(|e| SubsonicError::Internal(format!("Failed to create temp audio file: {e}")))?;
    // Immediately attach RAII guard so stream errors, flush failures, or task cancellation clean up.
    let mut guard = TempAudioFile { path, size: 0 };

    let mut stream = resp.bytes_stream();
    while let Some(chunk_res) = stream.next().await {
        let chunk = chunk_res.map_err(|e| {
            SubsonicError::Upstream(format!("Error reading audio stream: {}", e.without_url()))
        })?;
        guard.size = guard.size.saturating_add(chunk.len() as u64);
        if guard.size > max_bytes {
            drop(file);
            return Err(SubsonicError::TooLarge(
                "Remote audio exceeded 512MB import limit".to_string(),
            ));
        }
        file.write_all(&chunk).await.map_err(|err| {
            SubsonicError::Internal(format!("Failed writing audio temp file: {err}"))
        })?;
    }
    file.flush()
        .await
        .map_err(|e| SubsonicError::Internal(format!("Failed flushing audio temp file: {e}")))?;

    Ok(guard)
}

fn is_metadata_content_type(ct: &str) -> bool {
    let lower = ct.to_ascii_lowercase();
    lower.contains("application/json")
        || lower.contains("application/xml")
        || lower.contains("text/xml")
}

fn is_disallowed_stream_html_content_type(ct: &str) -> bool {
    let essence = ct
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    matches!(
        essence.as_str(),
        "text/html" | "application/xhtml+xml" | "image/svg+xml"
    )
}

pub fn build_subsonic_url(
    base_url: &str,
    endpoint: &str,
    username: &str,
    password: &str,
    extra_params: &[(&str, String)],
) -> Result<String, SubsonicError> {
    let mut parsed = Url::parse(base_url.trim())
        .map_err(|e| SubsonicError::BadRequest(format!("Invalid Subsonic URL: {e}")))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(SubsonicError::BadRequest(format!(
            "Unsupported Subsonic URL scheme '{}'",
            parsed.scheme()
        )));
    }

    let endpoint_clean = endpoint.trim_start_matches('/');
    {
        let mut segments = parsed.path_segments_mut().map_err(|()| {
            SubsonicError::BadRequest("Subsonic base URL cannot be a base".to_string())
        })?;
        segments.pop_if_empty();
    }
    let ends_with_rest = parsed
        .path_segments()
        .and_then(|mut s| s.next_back())
        .is_some_and(|seg| seg.eq_ignore_ascii_case("rest"));

    {
        let mut segments = parsed.path_segments_mut().map_err(|()| {
            SubsonicError::BadRequest("Subsonic base URL cannot be a base".to_string())
        })?;
        if !ends_with_rest {
            segments.push("rest");
        }
        for seg in endpoint_clean.split('/').filter(|s| !s.is_empty()) {
            segments.push(seg);
        }
    }

    let salt = Uuid::new_v4().simple().to_string()[..12].to_string();
    let mut pairs = parsed.query_pairs_mut();

    let user = username.trim();
    if !user.is_empty() {
        let mut md5 = Md5::new();
        md5.update(password.as_bytes());
        md5.update(salt.as_bytes());
        let token = hex::encode(md5.finalize());
        pairs.append_pair("u", user);
        pairs.append_pair("t", &token);
        pairs.append_pair("s", &salt);
    } else if !password.is_empty() {
        pairs.append_pair("apiKey", password);
    }

    pairs.append_pair("v", SUBSONIC_API_VERSION);
    pairs.append_pair("c", SUBSONIC_CLIENT_NAME);
    pairs.append_pair("f", "json");
    for (k, v) in extra_params {
        pairs.append_pair(k, v);
    }
    drop(pairs);

    Ok(parsed.to_string())
}

fn extract_subsonic_ok(root: &Value) -> Result<&Value, SubsonicError> {
    let inner = root
        .get("subsonic-response")
        .ok_or_else(|| SubsonicError::Upstream("Missing 'subsonic-response' envelope".into()))?;
    let status = inner.get("status").and_then(Value::as_str).unwrap_or("");
    if status.eq_ignore_ascii_case("ok") {
        Ok(inner)
    } else {
        let code = inner
            .pointer("/error/code")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let msg = inner
            .pointer("/error/message")
            .and_then(Value::as_str)
            .unwrap_or("Subsonic request failed");
        Err(SubsonicError::Upstream(format!(
            "Subsonic error {code}: {msg}"
        )))
    }
}

/// RFC 3986 path-segment percent-encoding (encodes spaces as `%20`, never `+`).
fn encode_segment(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for &b in raw.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn value_to_string(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn parse_album_dto(source_id: &str, val: &Value) -> Option<SubsonicAlbumDto> {
    let id = val.get("id").and_then(value_to_string)?;
    let name = val
        .get("name")
        .or_else(|| val.get("title"))
        .and_then(Value::as_str)
        .unwrap_or("Unknown Album")
        .to_string();
    let artist = val
        .get("artist")
        .and_then(Value::as_str)
        .unwrap_or("Unknown Artist")
        .to_string();
    let year = val
        .get("year")
        .and_then(Value::as_i64)
        .and_then(|y| i32::try_from(y).ok());
    let song_count = val
        .get("songCount")
        .and_then(Value::as_u64)
        .and_then(|c| u32::try_from(c).ok());
    let duration = val.get("duration").and_then(Value::as_u64);
    let cover_url = val
        .get("coverArt")
        .and_then(value_to_string)
        .filter(|s| !s.is_empty())
        .map(|cid| format!("/_app/subsonic/{source_id}/cover/{}", encode_segment(&cid)));

    Some(SubsonicAlbumDto {
        id,
        name,
        artist,
        year,
        song_count,
        duration,
        cover_url,
    })
}

fn parse_song_dto(source_id: &str, val: &Value) -> Option<SubsonicSongDto> {
    let id = val.get("id").and_then(value_to_string)?;
    let title = val
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("Unknown Track")
        .to_string();
    let artist = val
        .get("artist")
        .and_then(Value::as_str)
        .unwrap_or("Unknown Artist")
        .to_string();
    let album = val
        .get("album")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let track = val
        .get("track")
        .and_then(Value::as_u64)
        .and_then(|t| u32::try_from(t).ok());
    let duration = val.get("duration").and_then(Value::as_u64);
    let suffix = val
        .get("suffix")
        .and_then(Value::as_str)
        .map(str::to_string);
    let bit_rate = val
        .get("bitRate")
        .and_then(Value::as_u64)
        .and_then(|b| u32::try_from(b).ok());
    let size = val.get("size").and_then(Value::as_u64);
    let cover_url = val
        .get("coverArt")
        .and_then(value_to_string)
        .filter(|s| !s.is_empty())
        .map(|cid| format!("/_app/subsonic/{source_id}/cover/{}", encode_segment(&cid)));
    let stream_url = format!("/_app/subsonic/{source_id}/stream/{}", encode_segment(&id));

    Some(SubsonicSongDto {
        id,
        title,
        artist,
        album,
        track,
        duration,
        suffix,
        bit_rate,
        size,
        cover_url,
        stream_url,
    })
}

fn audio_ext_from_mime(content_type: &str) -> Option<&'static str> {
    let mime = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    match mime.as_str() {
        "audio/mpeg" | "audio/mp3" => Some("mp3"),
        "audio/flac" | "audio/x-flac" => Some("flac"),
        "audio/wav" | "audio/x-wav" | "audio/wave" => Some("wav"),
        "audio/ogg" | "application/ogg" => Some("ogg"),
        "audio/opus" => Some("opus"),
        "audio/mp4" | "audio/x-m4a" | "audio/aac" => Some("m4a"),
        _ => None,
    }
}

fn is_supported_audio_ext(ext: &str) -> bool {
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "mp3" | "flac" | "wav" | "ogg" | "opus" | "m4a" | "aac" | "alac" | "aiff" | "wma" | "wv"
    )
}

fn sanitize_base_name(raw: &str) -> String {
    raw.rsplit(['/', '\\'])
        .next()
        .unwrap_or(raw)
        .trim()
        .replace(['\0', '\r', '\n', ':', '*', '?', '"', '<', '>', '|'], "_")
}

fn resolve_audio_filename(
    filename_hint: Option<&str>,
    content_disposition: Option<&str>,
    content_type: &str,
    meta_title: Option<&str>,
    meta_suffix: Option<&str>,
    song_id: &str,
) -> String {
    let cd_name = content_disposition.and_then(parse_filename_from_content_disposition);

    let base = filename_hint
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(sanitize_base_name)
        .or_else(|| {
            cd_name
                .filter(|s| !s.is_empty())
                .map(|s| sanitize_base_name(&s))
        })
        .or_else(|| {
            meta_title
                .filter(|s| !s.trim().is_empty())
                .map(sanitize_base_name)
        })
        .unwrap_or_else(|| format!("track-{song_id}"));

    if let Some(ext) = std::path::Path::new(&base)
        .extension()
        .and_then(|s| s.to_str())
        && is_supported_audio_ext(ext)
    {
        return base;
    }

    let ext = meta_suffix
        .filter(|s| is_supported_audio_ext(s))
        .map(str::to_string)
        .or_else(|| audio_ext_from_mime(content_type).map(str::to_string))
        .unwrap_or_else(|| "mp3".to_string());

    format!("{base}.{ext}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subsonic_url_builder_generates_valid_md5_token_and_salt() {
        let built = build_subsonic_url(
            "http://127.0.0.1:4533",
            "ping.view",
            "alice",
            "secret123",
            &[("size", "20".into())],
        )
        .unwrap();

        let parsed = Url::parse(&built).unwrap();
        assert_eq!(parsed.path(), "/rest/ping.view");
        let params: std::collections::HashMap<String, String> =
            parsed.query_pairs().into_owned().collect();
        assert_eq!(params.get("u").map(String::as_str), Some("alice"));
        assert_eq!(params.get("v").map(String::as_str), Some("1.16.1"));
        assert_eq!(params.get("c").map(String::as_str), Some("XMedia"));
        assert_eq!(params.get("f").map(String::as_str), Some("json"));
        assert_eq!(params.get("size").map(String::as_str), Some("20"));

        let salt = params.get("s").unwrap();
        let token = params.get("t").unwrap();
        let mut md5 = Md5::new();
        md5.update(b"secret123");
        md5.update(salt.as_bytes());
        assert_eq!(token, &hex::encode(md5.finalize()));
    }

    #[test]
    fn subsonic_url_builder_handles_existing_query_rest_suffix_and_api_key() {
        let built = build_subsonic_url(
            "http://127.0.0.1:4533/music/rest/?foo=bar",
            "stream.view",
            "",
            "xmk_secret_key",
            &[("id", "track 1+2".into())],
        )
        .unwrap();

        let parsed = Url::parse(&built).unwrap();
        assert_eq!(parsed.path(), "/music/rest/stream.view");
        let params: std::collections::HashMap<String, String> =
            parsed.query_pairs().into_owned().collect();
        assert_eq!(params.get("foo").map(String::as_str), Some("bar"));
        assert_eq!(
            params.get("apiKey").map(String::as_str),
            Some("xmk_secret_key")
        );
        assert_eq!(params.get("id").map(String::as_str), Some("track 1+2"));
        assert!(!params.contains_key("u"));
    }

    #[test]
    fn encode_segment_uses_rfc3986_percent20_and_preserves_special_chars() {
        assert_eq!(encode_segment("song 1+2/test"), "song%201%2B2%2Ftest");
        assert_eq!(encode_segment("abc-123_4.5~"), "abc-123_4.5~");
    }

    #[test]
    fn resolve_audio_filename_decodes_rfc5987_with_plus_intact() {
        let name = resolve_audio_filename(
            None,
            Some(
                r#"attachment; filename="fallback.flac"; filename*=UTF-8''Rock%20%2B%20Roll+Live.flac"#,
            ),
            "audio/flac",
            None,
            None,
            "song-1",
        );
        assert_eq!(name, "Rock + Roll+Live.flac");
    }
}

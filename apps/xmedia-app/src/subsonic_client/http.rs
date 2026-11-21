use std::path::{Path, PathBuf};
use std::time::Instant;

use axum::body::Body;
use axum::http::header::{
    ACCEPT_RANGES, CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE,
    ETAG, IF_RANGE, LAST_MODIFIED, RANGE,
};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::Response;
use futures_util::StreamExt;
use serde_json::Value;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use super::parser::{
    build_subsonic_url, extract_subsonic_ok, json_value_items, parse_album_dto, parse_song_dto,
    resolve_audio_filename,
};
use super::{
    MAX_IMPORT_BYTES, MAX_STREAM_ERROR_SNIFF_BYTES, REQUEST_TIMEOUT, SubsonicAlbumDetailDto,
    SubsonicAlbumDto, SubsonicClient, SubsonicError, SubsonicSearchResultDto, SubsonicTestResult,
    TEST_TIMEOUT,
};
use crate::opds_client::{
    LimitedByteStream, MAX_IMAGE_BYTES, MAX_METADATA_BYTES, ReadLimitError, RemoteImportResultDto,
    apply_proxy_security_headers, is_safe_raster_image_content_type, read_response_text_limited,
    tokio_util_file_stream,
};
use crate::remote_sources::{RemoteSource, normalize_remote_url};

impl SubsonicClient {
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
            .map(|v| {
                json_value_items(v)
                    .into_iter()
                    .filter_map(|item| parse_album_dto(&source.id, item))
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
            .map(|v| {
                json_value_items(v)
                    .into_iter()
                    .filter_map(|item| parse_song_dto(&source.id, item))
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
            .map(|v| {
                json_value_items(v)
                    .into_iter()
                    .filter_map(|item| parse_album_dto(&source.id, item))
                    .collect()
            })
            .unwrap_or_default();
        let songs = sr
            .and_then(|v| v.get("song"))
            .map(|v| {
                json_value_items(v)
                    .into_iter()
                    .filter_map(|item| parse_song_dto(&source.id, item))
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
        // Only advertise Accept-Ranges: bytes when upstream confirms Range support.
        if !out_headers.contains_key(ACCEPT_RANGES)
            && (status == StatusCode::PARTIAL_CONTENT || out_headers.contains_key(CONTENT_RANGE))
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
        let _import_permit = self
            .import_semaphore
            .acquire()
            .await
            .map_err(|_| SubsonicError::Internal("Import concurrency limiter closed".into()))?;

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
            .local_upload
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

async fn create_private_temp_file(path: &Path) -> std::io::Result<tokio::fs::File> {
    let mut opts = tokio::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        opts.mode(0o600);
    }
    opts.open(path).await
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
            "Remote audio exceeds 511MB import limit ({len} bytes)"
        )));
    }

    tokio::fs::create_dir_all(temp_dir)
        .await
        .map_err(|e| SubsonicError::Internal(format!("Failed to prepare temp directory: {e}")))?;
    let path = temp_dir.join(format!("xmedia-subsonic-import-{}.tmp", Uuid::new_v4()));
    // Declare RAII guard BEFORE opening the file handle so `file` drops first (Windows unlink safety).
    let mut guard = TempAudioFile {
        path: path.clone(),
        size: 0,
    };
    let mut file = create_private_temp_file(&path)
        .await
        .map_err(|e| SubsonicError::Internal(format!("Failed to create temp audio file: {e}")))?;

    let mut stream = resp.bytes_stream();
    while let Some(chunk_res) = stream.next().await {
        let chunk = chunk_res.map_err(|e| {
            SubsonicError::Upstream(format!("Error reading audio stream: {}", e.without_url()))
        })?;
        guard.size = guard.size.saturating_add(chunk.len() as u64);
        if guard.size > max_bytes {
            drop(file);
            return Err(SubsonicError::TooLarge(
                "Remote audio exceeded 511MB import limit".to_string(),
            ));
        }
        file.write_all(&chunk).await.map_err(|err| {
            SubsonicError::Internal(format!("Failed writing audio temp file: {err}"))
        })?;
    }
    file.flush()
        .await
        .map_err(|e| SubsonicError::Internal(format!("Failed flushing audio temp file: {e}")))?;

    if guard.size == 0 {
        drop(file);
        return Err(SubsonicError::Upstream(
            "Remote Subsonic server returned an empty audio payload".to_string(),
        ));
    }

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

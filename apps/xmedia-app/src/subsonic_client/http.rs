use std::time::Instant;

use axum::body::Body;
use axum::http::header::{
    ACCEPT_RANGES, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, ETAG,
    IF_RANGE, LAST_MODIFIED, RANGE,
};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::Response;
use serde_json::Value;
use xmedia_core::domain::library::MediaKind;

use super::parser::{
    build_subsonic_url, extract_subsonic_ok, json_value_items, parse_album_dto, parse_song_dto,
    resolve_audio_filename,
};
use super::{
    MAX_IMPORT_BYTES, MAX_STREAM_ERROR_SNIFF_BYTES, REQUEST_TIMEOUT, SubsonicAlbumDetailDto,
    SubsonicAlbumDto, SubsonicClient, SubsonicError, SubsonicSearchResultDto, SubsonicTestResult,
    TEST_TIMEOUT,
};
use crate::local_engine::LocalMediaImporter;
use crate::opds_client::{
    MAX_IMAGE_BYTES, MAX_METADATA_BYTES, ReadLimitError, RemoteImportResultDto, SniffImageError,
    apply_proxy_security_headers, build_sniffed_raster_image_response,
    is_disallowed_non_raster_header, is_safe_raster_image_content_type, read_response_text_limited,
};
use crate::remote_common::{read_temp_file_bytes, stream_response_to_temp_file};
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
        if is_disallowed_non_raster_header(&content_type)
            || (!content_type.is_empty()
                && !content_type
                    .to_ascii_lowercase()
                    .starts_with("application/octet-stream")
                && !content_type.to_ascii_lowercase().starts_with("image/jpg")
                && !is_safe_raster_image_content_type(&content_type))
        {
            return Err(SubsonicError::Unsupported(format!(
                "Subsonic cover endpoint returned disallowed Content-Type '{content_type}'"
            )));
        }

        build_sniffed_raster_image_response(resp, MAX_IMAGE_BYTES, "Subsonic cover")
            .await
            .map_err(|err| match err {
                SniffImageError::TooLarge(m) => SubsonicError::TooLarge(m),
                SniffImageError::Unsupported(m) => SubsonicError::Unsupported(m),
                SniffImageError::Upstream(m) => SubsonicError::Upstream(m),
            })
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
        importer: &dyn LocalMediaImporter,
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

        let temp_file = stream_response_to_temp_file(
            resp,
            MAX_IMPORT_BYTES,
            &self.temp_dir,
            "xmedia-subsonic-import",
            "Subsonic audio",
        )
        .await?;

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

        let data = read_temp_file_bytes(&temp_file, "Subsonic audio").await?;
        drop(temp_file);

        let imported = importer
            .import_media(MediaKind::Music, filename.clone(), data)
            .await?;
        let title = if !imported.title.trim().is_empty() && imported.title != filename {
            imported.title
        } else {
            song_meta_title.unwrap_or_else(|| {
                if imported.title.trim().is_empty() {
                    filename
                } else {
                    imported.title
                }
            })
        };

        Ok(RemoteImportResultDto {
            media_kind: "music".to_string(),
            imported_id: imported.id,
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

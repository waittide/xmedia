use std::fmt;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::header::{
    CACHE_CONTROL, CONTENT_LENGTH, CONTENT_SECURITY_POLICY, CONTENT_TYPE, X_CONTENT_TYPE_OPTIONS,
};
use axum::http::{HeaderValue, StatusCode};
use axum::response::Response;
use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::AsyncWriteExt;
use url::Url;
use uuid::Uuid;

use crate::remote_sources::{RemoteSource, is_same_origin, normalize_remote_url};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const TEST_TIMEOUT: Duration = Duration::from_secs(10);
pub(crate) const MAX_METADATA_BYTES: usize = 8 * 1024 * 1024;
pub(crate) const MAX_IMAGE_BYTES: u64 = 12 * 1024 * 1024;
const MAX_IMPORT_BYTES: u64 = 512 * 1024 * 1024;
const MAX_REDIRECTS: usize = 10;

pub(crate) fn same_origin_redirect_policy() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() >= MAX_REDIRECTS {
            return attempt.error("too many redirects");
        }
        let target = attempt.url();
        if !matches!(target.scheme(), "http" | "https") {
            return attempt.error("redirect to unsupported URL scheme");
        }
        if let Some(initial) = attempt.previous().first()
            && !is_same_origin(initial, target)
        {
            return attempt.error("cross-origin redirect is not allowed");
        }
        attempt.follow()
    })
}

pub(crate) fn is_safe_raster_image_content_type(content_type: &str) -> bool {
    let essence = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    matches!(
        essence.as_str(),
        "image/jpeg" | "image/png" | "image/webp" | "image/gif" | "image/avif"
    )
}

pub(crate) fn apply_proxy_security_headers(headers: &mut axum::http::HeaderMap) {
    headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    headers.insert(CONTENT_SECURITY_POLICY, HeaderValue::from_static("sandbox"));
}

#[derive(Debug)]
pub(crate) enum ReadLimitError {
    TooLarge(usize),
    Read(String),
}

pub(crate) async fn read_response_text_limited(
    resp: reqwest::Response,
    max_bytes: usize,
) -> Result<String, ReadLimitError> {
    if let Some(len) = resp.content_length()
        && len > max_bytes as u64
    {
        return Err(ReadLimitError::TooLarge(max_bytes));
    }

    let mut buf = Vec::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk_res) = stream.next().await {
        let chunk = chunk_res.map_err(|e| ReadLimitError::Read(e.without_url().to_string()))?;
        if buf.len().saturating_add(chunk.len()) > max_bytes {
            return Err(ReadLimitError::TooLarge(max_bytes));
        }
        buf.extend_from_slice(&chunk);
    }

    Ok(String::from_utf8_lossy(&buf).into_owned())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpdsTestResult {
    pub ok: bool,
    pub latency_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opds_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpdsNavLinkDto {
    pub title: String,
    pub href: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpdsDownloadLinkDto {
    pub href: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub format: String,
    pub media_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpdsEntryDto {
    pub id: String,
    pub title: String,
    pub authors: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pse_stream_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_count: Option<u64>,
    pub downloads: Vec<OpdsDownloadLinkDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpdsFeedDto {
    pub title: String,
    pub url: String,
    pub opds_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prev_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_url: Option<String>,
    pub nav_links: Vec<OpdsNavLinkDto>,
    pub entries: Vec<OpdsEntryDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteImportResultDto {
    pub media_kind: String,
    pub imported_id: String,
    pub title: String,
}

#[derive(Debug)]
pub enum OpdsError {
    BadRequest(String),
    Upstream(String),
    Unsupported(String),
    TooLarge(String),
    Internal(String),
}

impl OpdsError {
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

impl fmt::Display for OpdsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadRequest(m)
            | Self::Upstream(m)
            | Self::Unsupported(m)
            | Self::TooLarge(m)
            | Self::Internal(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for OpdsError {}

#[derive(Clone)]
pub struct OpdsClient {
    secure: reqwest::Client,
    insecure: reqwest::Client,
    temp_dir: PathBuf,
}

impl OpdsClient {
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

    fn apply_auth(
        &self,
        req: reqwest::RequestBuilder,
        username: &str,
        password: &str,
    ) -> reqwest::RequestBuilder {
        if !username.trim().is_empty() {
            req.basic_auth(username.trim(), Some(password))
        } else {
            req
        }
    }

    pub async fn test_opds(
        &self,
        raw_url: &str,
        username: &str,
        password: &str,
        allow_insecure_tls: bool,
    ) -> OpdsTestResult {
        let normalized_url = match normalize_remote_url(raw_url) {
            Ok(u) => u,
            Err(err) => {
                return OpdsTestResult {
                    ok: false,
                    latency_ms: 0,
                    title: None,
                    opds_version: None,
                    entry_count: None,
                    error: Some(err.to_string()),
                };
            }
        };

        let started = Instant::now();
        let req = self
            .client_for(allow_insecure_tls)
            .get(&normalized_url)
            .header(
                reqwest::header::ACCEPT,
                "application/atom+xml, application/opds+json, application/xml, application/json;q=0.9, */*;q=0.8",
            )
            .timeout(TEST_TIMEOUT);
        let req = self.apply_auth(req, username, password);

        let resp = match req.send().await {
            Ok(r) => r,
            Err(err) => {
                return OpdsTestResult {
                    ok: false,
                    latency_ms: started.elapsed().as_millis() as u64,
                    title: None,
                    opds_version: None,
                    entry_count: None,
                    error: Some(format!("Connection failed: {}", err.without_url())),
                };
            }
        };

        let latency_ms = started.elapsed().as_millis() as u64;
        if !resp.status().is_success() {
            return OpdsTestResult {
                ok: false,
                latency_ms,
                title: None,
                opds_version: None,
                entry_count: None,
                error: Some(format!(
                    "OPDS server returned HTTP {}",
                    resp.status().as_u16()
                )),
            };
        }

        let final_url = resp.url().clone();
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let body = match read_response_text_limited(resp, MAX_METADATA_BYTES).await {
            Ok(t) => t,
            Err(ReadLimitError::TooLarge(limit)) => {
                return OpdsTestResult {
                    ok: false,
                    latency_ms,
                    title: None,
                    opds_version: None,
                    entry_count: None,
                    error: Some(format!(
                        "OPDS response exceeded {limit} byte metadata limit"
                    )),
                };
            }
            Err(ReadLimitError::Read(err)) => {
                return OpdsTestResult {
                    ok: false,
                    latency_ms,
                    title: None,
                    opds_version: None,
                    entry_count: None,
                    error: Some(format!("Failed to read OPDS response body: {err}")),
                };
            }
        };

        match parse_opds_feed("probe", &final_url, &content_type, &body) {
            Ok((feed, _)) => OpdsTestResult {
                ok: true,
                latency_ms,
                title: Some(feed.title),
                opds_version: Some(feed.opds_version),
                entry_count: Some(feed.entries.len() + feed.nav_links.len()),
                error: None,
            },
            Err(err) => OpdsTestResult {
                ok: false,
                latency_ms,
                title: None,
                opds_version: None,
                entry_count: None,
                error: Some(err.to_string()),
            },
        }
    }

    pub async fn browse_opds(
        &self,
        source: &RemoteSource,
        target_url_opt: Option<&str>,
        query_opt: Option<&str>,
    ) -> Result<OpdsFeedDto, OpdsError> {
        let target_url = resolve_target_url(&source.url, target_url_opt)?;
        let query = query_opt.map(str::trim).filter(|q| !q.is_empty());

        if let Some(q) = query {
            if target_url.contains('{') {
                let expanded = expand_search_template(&target_url, q);
                let search_target = resolve_target_url(&source.url, Some(&expanded))?;
                let (feed, _) = self.fetch_and_parse(source, &search_target).await?;
                return Ok(feed);
            }

            let (initial_feed, search_link_is_opensearch) =
                self.fetch_and_parse(source, &target_url).await?;
            let Some(search_href) = initial_feed.search_url.as_deref() else {
                return Err(OpdsError::BadRequest(
                    "This OPDS catalog does not advertise a search endpoint".to_string(),
                ));
            };

            let raw_search_target = if search_href.contains('{') {
                expand_search_template(search_href, q)
            } else if search_link_is_opensearch
                || search_href.to_ascii_lowercase().ends_with(".xml")
                || search_href.to_ascii_lowercase().contains("opensearch")
            {
                let safe_descriptor = resolve_target_url(&source.url, Some(search_href))?;
                let template = self
                    .fetch_opensearch_template(source, &safe_descriptor)
                    .await?;
                expand_search_template(&template, q)
            } else {
                append_query_param(search_href, "q", q)
            };

            let search_target = resolve_target_url(&source.url, Some(&raw_search_target))?;
            let (search_feed, _) = self.fetch_and_parse(source, &search_target).await?;
            return Ok(search_feed);
        }

        let (feed, _) = self.fetch_and_parse(source, &target_url).await?;
        Ok(feed)
    }

    async fn fetch_and_parse(
        &self,
        source: &RemoteSource,
        url: &str,
    ) -> Result<(OpdsFeedDto, bool), OpdsError> {
        let safe_url = resolve_target_url(&source.url, Some(url))?;
        let req = self
            .client_for(source.allow_insecure_tls)
            .get(&safe_url)
            .header(
                reqwest::header::ACCEPT,
                "application/atom+xml, application/opds+json, application/xml, application/json;q=0.9, */*;q=0.8",
            )
            .timeout(REQUEST_TIMEOUT);
        let req = self.apply_auth(req, &source.username, &source.password);

        let resp = req.send().await.map_err(|e| {
            OpdsError::Upstream(format!("OPDS request failed: {}", e.without_url()))
        })?;

        if !resp.status().is_success() {
            return Err(OpdsError::Upstream(format!(
                "OPDS server returned HTTP {}",
                resp.status().as_u16()
            )));
        }

        let final_url = resp.url().clone();
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let body = read_response_text_limited(resp, MAX_METADATA_BYTES)
            .await
            .map_err(|e| match e {
                ReadLimitError::TooLarge(limit) => OpdsError::TooLarge(format!(
                    "OPDS feed response exceeded {limit} byte metadata limit"
                )),
                ReadLimitError::Read(msg) => {
                    OpdsError::Upstream(format!("Failed to read OPDS body: {msg}"))
                }
            })?;

        parse_opds_feed(&source.id, &final_url, &content_type, &body)
    }

    async fn fetch_opensearch_template(
        &self,
        source: &RemoteSource,
        descriptor_url: &str,
    ) -> Result<String, OpdsError> {
        let safe_descriptor = resolve_target_url(&source.url, Some(descriptor_url))?;
        let req = self
            .client_for(source.allow_insecure_tls)
            .get(&safe_descriptor)
            .timeout(REQUEST_TIMEOUT);
        let req = self.apply_auth(req, &source.username, &source.password);

        let resp = req.send().await.map_err(|e| {
            OpdsError::Upstream(format!("OpenSearch request failed: {}", e.without_url()))
        })?;
        if !resp.status().is_success() {
            return Err(OpdsError::Upstream(format!(
                "OpenSearch descriptor returned HTTP {}",
                resp.status().as_u16()
            )));
        }

        let base_url = resp.url().clone();
        let xml = read_response_text_limited(resp, MAX_METADATA_BYTES)
            .await
            .map_err(|e| match e {
                ReadLimitError::TooLarge(limit) => OpdsError::TooLarge(format!(
                    "OpenSearch descriptor exceeded {limit} byte metadata limit"
                )),
                ReadLimitError::Read(msg) => {
                    OpdsError::Upstream(format!("Failed to read OpenSearch XML: {msg}"))
                }
            })?;

        let template = parse_opensearch_template(&xml).ok_or_else(|| {
            OpdsError::Upstream(
                "OpenSearch descriptor did not contain a valid <Url template>".into(),
            )
        })?;
        Ok(resolve_link_url(&base_url, &template))
    }

    pub async fn proxy_image(
        &self,
        source: &RemoteSource,
        raw_url: &str,
    ) -> Result<Response, OpdsError> {
        let target_url = resolve_target_url(&source.url, Some(raw_url))?;
        let req = self
            .client_for(source.allow_insecure_tls)
            .get(&target_url)
            .timeout(REQUEST_TIMEOUT);
        let req = self.apply_auth(req, &source.username, &source.password);

        let resp = req.send().await.map_err(|e| {
            OpdsError::Upstream(format!("Failed to fetch OPDS image: {}", e.without_url()))
        })?;

        if !resp.status().is_success() {
            return Err(OpdsError::Upstream(format!(
                "OPDS image endpoint returned HTTP {}",
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

        if !is_safe_raster_image_content_type(&content_type) {
            return Err(OpdsError::Unsupported(format!(
                "Remote image returned disallowed Content-Type '{content_type}'"
            )));
        }

        if let Some(len) = resp.content_length()
            && len > MAX_IMAGE_BYTES
        {
            return Err(OpdsError::TooLarge(format!(
                "Remote image exceeds maximum allowed size of {} bytes",
                MAX_IMAGE_BYTES
            )));
        }

        let content_length_header = resp
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
        if let Some(cl) = content_length_header {
            response.headers_mut().insert(CONTENT_LENGTH, cl);
        }
        response.headers_mut().insert(
            CACHE_CONTROL,
            HeaderValue::from_static("private, max-age=3600"),
        );
        apply_proxy_security_headers(response.headers_mut());
        Ok(response)
    }

    pub async fn import_to_local(
        &self,
        source: &RemoteSource,
        download_url: &str,
        filename_hint: Option<&str>,
        local_gateway_url: &str,
        local_token: &str,
    ) -> Result<RemoteImportResultDto, OpdsError> {
        let target_url = resolve_target_url(&source.url, Some(download_url))?;
        let req = self.client_for(source.allow_insecure_tls).get(&target_url);
        let req = self.apply_auth(req, &source.username, &source.password);

        let resp = req.send().await.map_err(|e| {
            OpdsError::Upstream(format!(
                "Failed to download remote OPDS file: {}",
                e.without_url()
            ))
        })?;

        if !resp.status().is_success() {
            return Err(OpdsError::Upstream(format!(
                "Remote OPDS download returned HTTP {}",
                resp.status().as_u16()
            )));
        }

        if let Some(len) = resp.content_length()
            && len > MAX_IMPORT_BYTES
        {
            return Err(OpdsError::TooLarge(format!(
                "Remote file exceeds 512MB import limit ({len} bytes)"
            )));
        }

        let final_url = resp.url().clone();
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let content_disposition = resp
            .headers()
            .get(reqwest::header::CONTENT_DISPOSITION)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);

        let temp_guard =
            stream_response_to_temp_file(resp, MAX_IMPORT_BYTES, &self.temp_dir).await?;
        let (filename, format) = resolve_import_filename_and_format(
            filename_hint,
            content_disposition.as_deref(),
            &content_type,
            &final_url,
        )?;

        let (media_kind, upload_endpoint) = match format.as_str() {
            "cbz" | "zip" => ("comic", format!("{local_gateway_url}/api/v1/comic/upload")),
            "epub" | "pdf" | "txt" => (
                "book",
                format!("{local_gateway_url}/api/v1/book/books/upload"),
            ),
            other => {
                return Err(OpdsError::Unsupported(format!(
                    "Unsupported OPDS import format '{other}'"
                )));
            }
        };

        let file = tokio::fs::File::open(&temp_guard.path).await.map_err(|e| {
            OpdsError::Internal(format!("Failed to open downloaded temp file: {e}"))
        })?;
        let file_len = temp_guard.size;
        let stream = tokio_util_file_stream(file);
        let part = reqwest::multipart::Part::stream_with_length(
            reqwest::Body::wrap_stream(stream),
            file_len,
        )
        .file_name(filename.clone());
        let form = reqwest::multipart::Form::new().part("file", part);

        let upload_resp = self
            .secure
            .post(&upload_endpoint)
            .bearer_auth(local_token)
            .multipart(form)
            .send()
            .await
            .map_err(|e| {
                OpdsError::Internal(format!(
                    "Failed to upload into local library: {}",
                    e.without_url()
                ))
            })?;

        let status = upload_resp.status();
        let body_bytes = upload_resp.bytes().await.map_err(|e| {
            OpdsError::Internal(format!(
                "Failed to read local upload response: {}",
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
                        "Local library rejected uploaded file".to_string()
                    } else {
                        trimmed.chars().take(200).collect()
                    }
                });
            if status == StatusCode::PAYLOAD_TOO_LARGE {
                return Err(OpdsError::TooLarge(format!(
                    "Local import rejected (HTTP 413): {msg}"
                )));
            }
            return Err(OpdsError::Upstream(format!(
                "Local import failed (HTTP {}): {msg}",
                status.as_u16()
            )));
        }

        let resp_json = parsed_json.ok_or_else(|| {
            OpdsError::Internal("Local upload succeeded but returned invalid JSON".into())
        })?;

        let first = resp_json
            .get("uploaded")
            .and_then(Value::as_array)
            .and_then(|arr| arr.first())
            .ok_or_else(|| {
                OpdsError::Internal(
                    "Local upload succeeded but returned empty uploaded array".into(),
                )
            })?;

        let (imported_id, title) = if media_kind == "comic" {
            let id = first
                .get("series_id")
                .or_else(|| first.get("chapter_id"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let title = first
                .get("series_title")
                .or_else(|| first.get("chapter_title"))
                .and_then(Value::as_str)
                .unwrap_or(&filename)
                .to_string();
            (id, title)
        } else {
            let id = first
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let title = first
                .pointer("/metadata/title")
                .and_then(Value::as_str)
                .unwrap_or(&filename)
                .to_string();
            (id, title)
        };

        Ok(RemoteImportResultDto {
            media_kind: media_kind.to_string(),
            imported_id,
            title,
        })
    }
}

struct TempDownloadFile {
    path: PathBuf,
    size: u64,
}

impl Drop for TempDownloadFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

async fn stream_response_to_temp_file(
    resp: reqwest::Response,
    max_bytes: u64,
    temp_dir: &Path,
) -> Result<TempDownloadFile, OpdsError> {
    tokio::fs::create_dir_all(temp_dir)
        .await
        .map_err(|e| OpdsError::Internal(format!("Failed to prepare temp directory: {e}")))?;
    let path = temp_dir.join(format!("xmedia-opds-import-{}.tmp", Uuid::new_v4()));
    let mut file = tokio::fs::File::create(&path)
        .await
        .map_err(|e| OpdsError::Internal(format!("Failed to create temp file: {e}")))?;
    // Immediately attach RAII guard so stream errors, flush failures, or task cancellation clean up.
    let mut guard = TempDownloadFile { path, size: 0 };

    let mut stream = resp.bytes_stream();
    while let Some(chunk_res) = stream.next().await {
        let chunk = chunk_res.map_err(|e| {
            OpdsError::Upstream(format!("Error reading remote stream: {}", e.without_url()))
        })?;
        guard.size = guard.size.saturating_add(chunk.len() as u64);
        if guard.size > max_bytes {
            drop(file);
            return Err(OpdsError::TooLarge(format!(
                "Remote download exceeded maximum limit of {max_bytes} bytes"
            )));
        }
        file.write_all(&chunk).await.map_err(|err| {
            OpdsError::Internal(format!(
                "Failed writing downloaded data to temp file: {err}"
            ))
        })?;
    }
    file.flush()
        .await
        .map_err(|e| OpdsError::Internal(format!("Failed to flush temp file: {e}")))?;

    Ok(guard)
}

pub(crate) fn tokio_util_file_stream(
    file: tokio::fs::File,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static {
    futures_util::stream::unfold(Some(file), |state| async move {
        let mut f = state?;
        let mut buf = vec![0u8; 64 * 1024];
        match tokio::io::AsyncReadExt::read(&mut f, &mut buf).await {
            Ok(0) => None,
            Ok(n) => {
                buf.truncate(n);
                Some((Ok(Bytes::from(buf)), Some(f)))
            }
            Err(err) => Some((Err(err), None)),
        }
    })
}

pub(crate) struct LimitedByteStream {
    inner: Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>,
    seen: u64,
    max_bytes: u64,
}

impl LimitedByteStream {
    pub(crate) fn new<S>(stream: S, max_bytes: u64) -> Self
    where
        S: Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
    {
        Self {
            inner: Box::pin(stream),
            seen: 0,
            max_bytes,
        }
    }
}

impl Stream for LimitedByteStream {
    type Item = Result<Bytes, std::io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.inner.as_mut().poll_next(cx) {
            Poll::Ready(Some(Ok(chunk))) => {
                self.seen = self.seen.saturating_add(chunk.len() as u64);
                if self.seen > self.max_bytes {
                    Poll::Ready(Some(Err(std::io::Error::other(format!(
                        "remote image exceeded {} byte size limit",
                        self.max_bytes
                    )))))
                } else {
                    Poll::Ready(Some(Ok(chunk)))
                }
            }
            Poll::Ready(Some(Err(err))) => {
                Poll::Ready(Some(Err(std::io::Error::other(err.without_url()))))
            }
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

fn resolve_target_url(source_base: &str, target_opt: Option<&str>) -> Result<String, OpdsError> {
    let base = Url::parse(source_base)
        .map_err(|e| OpdsError::BadRequest(format!("Invalid source URL: {e}")))?;
    if !matches!(base.scheme(), "http" | "https") {
        return Err(OpdsError::BadRequest(format!(
            "Unsupported source URL scheme '{}'",
            base.scheme()
        )));
    }

    let resolved = match target_opt.map(str::trim).filter(|s| !s.is_empty()) {
        Some(raw) => {
            if raw.starts_with("//") {
                return Err(OpdsError::BadRequest(
                    "Protocol-relative URLs are not allowed".to_string(),
                ));
            }
            if raw.contains("://") && !raw.starts_with("http://") && !raw.starts_with("https://") {
                return Err(OpdsError::BadRequest(
                    "Unsupported target URL scheme".to_string(),
                ));
            }
            let parsed = if raw.starts_with("http://") || raw.starts_with("https://") {
                Url::parse(raw)
                    .map_err(|e| OpdsError::BadRequest(format!("Invalid target URL: {e}")))?
            } else {
                base.join(raw)
                    .map_err(|e| OpdsError::BadRequest(format!("Invalid relative URL: {e}")))?
            };
            if !matches!(parsed.scheme(), "http" | "https") {
                return Err(OpdsError::BadRequest(format!(
                    "Unsupported URL scheme '{}'",
                    parsed.scheme()
                )));
            }
            if !is_same_origin(&base, &parsed) {
                return Err(OpdsError::BadRequest(
                    "Target URL must match the configured OPDS source origin".to_string(),
                ));
            }
            parsed
        }
        None => base,
    };

    Ok(resolved.to_string())
}

fn resolve_link_url(base: &Url, href: &str) -> String {
    let trimmed = href.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return trimmed.to_string();
    }
    if let Some(brace_idx) = trimmed.find('{') {
        let (prefix, suffix) = trimmed.split_at(brace_idx);
        if let Ok(resolved_prefix) = base.join(prefix) {
            return format!("{resolved_prefix}{suffix}");
        }
    }
    base.join(trimmed)
        .map(|u| u.to_string())
        .unwrap_or_else(|_| trimmed.to_string())
}

fn encode_query_component(raw: &str) -> String {
    form_urlencoded::byte_serialize(raw.as_bytes()).collect()
}

fn proxy_image_path(source_id: &str, abs_url: &str) -> String {
    format!(
        "/_app/opds/{source_id}/proxy-image?url={}",
        encode_query_component(abs_url)
    )
}

fn strip_optional_template_params(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{'
            && let Some(rel_end) = raw[i..].find('}')
        {
            let token = &raw[i + 1..i + rel_end];
            if token.ends_with('?') {
                // If preceded by "&key=" or "?key=", trim the dangling empty parameter.
                if let Some(eq_idx) = out.rfind('=') {
                    let prefix = &out[..eq_idx];
                    let sep_idx = prefix.rfind(['&', '?']);
                    let key_start = sep_idx.map(|idx| idx + 1).unwrap_or(0);
                    let key_slice = &out[key_start..eq_idx];
                    if !key_slice.is_empty()
                        && key_slice
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
                    {
                        match sep_idx {
                            Some(idx) if out.as_bytes()[idx] == b'&' => {
                                out.truncate(idx);
                            }
                            Some(idx) if out.as_bytes()[idx] == b'?' => {
                                // Keep '?' if more query parameters follow.
                                out.truncate(idx + 1);
                            }
                            _ => {}
                        }
                    }
                }
                i += rel_end + 1;
                // Avoid "?&" when the first query param was removed.
                if out.ends_with('?') && i < bytes.len() && bytes[i] == b'&' {
                    i += 1;
                }
                continue;
            }
        }
        let ch = raw[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    while out.ends_with('&') || out.ends_with('?') {
        out.pop();
    }
    out
}

fn expand_search_template(template: &str, query: &str) -> String {
    let encoded = encode_query_component(query);
    if template.contains("{searchTerms}") || template.contains("{searchTerms?}") {
        let replaced = template
            .replace("{searchTerms}", &encoded)
            .replace("{searchTerms?}", &encoded);
        return strip_optional_template_params(&replaced);
    }
    // Support RFC 6570 OPDS 2.0 templates like "/v2/search{?query}", "/v2/search{?q}", "/v2/search?lang=en{&query}"
    if let Some((start, marker_len)) = template
        .find("{?")
        .map(|i| (i, 2))
        .or_else(|| template.find("{&").map(|i| (i, 2)))
        && let Some(rel_end) = template[start..].find('}')
    {
        let end = start + rel_end;
        let vars = &template[start + marker_len..end];
        let param_name = vars
            .split(',')
            .map(str::trim)
            .map(|v| v.trim_end_matches('*').split(':').next().unwrap_or(v))
            .find(|v| !v.is_empty())
            .unwrap_or("query");
        let prefix = &template[..start];
        let suffix = &template[end + 1..];
        let sep = if prefix.contains('?') { '&' } else { '?' };
        let combined = format!("{prefix}{sep}{param_name}={encoded}{suffix}");
        return strip_optional_template_params(&combined);
    }
    append_query_param(template, "q", query)
}

fn append_query_param(url: &str, key: &str, val: &str) -> String {
    let sep = if url.contains('?') { '&' } else { '?' };
    format!("{url}{sep}{key}={}", encode_query_component(val))
}

fn parse_opds_feed(
    source_id: &str,
    base_url: &Url,
    content_type: &str,
    body: &str,
) -> Result<(OpdsFeedDto, bool), OpdsError> {
    let trimmed = body.trim_start();
    let lower_ct = content_type.to_ascii_lowercase();
    if lower_ct.contains("json") || trimmed.starts_with('{') {
        let feed = parse_opds2_json(source_id, base_url, trimmed)?;
        Ok((feed, false))
    } else if lower_ct.contains("xml") || trimmed.starts_with('<') {
        parse_opds1_atom(source_id, base_url, trimmed)
    } else {
        Err(OpdsError::Upstream(
            "Response is neither valid OPDS 1.2 Atom XML nor OPDS 2.0 JSON".to_string(),
        ))
    }
}

fn detect_format_from_mime_or_href(media_type: &str, href: &str) -> Option<String> {
    let mime = media_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    match mime.as_str() {
        "application/epub+zip" | "application/epub" => return Some("epub".to_string()),
        "application/pdf" => return Some("pdf".to_string()),
        "text/plain" => return Some("txt".to_string()),
        "application/vnd.comicbook+zip" | "application/x-cbz" | "application/cbz" => {
            return Some("cbz".to_string());
        }
        "application/zip" | "application/x-zip-compressed" => return Some("zip".to_string()),
        _ => {}
    }

    let path_part = Url::parse(href)
        .ok()
        .map(|u| u.path().to_string())
        .unwrap_or_else(|| href.split('?').next().unwrap_or(href).to_string());
    let ext = std::path::Path::new(&path_part)
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase());
    match ext.as_deref() {
        Some("epub") => Some("epub".to_string()),
        Some("pdf") => Some("pdf".to_string()),
        Some("txt") => Some("txt".to_string()),
        Some("cbz") => Some("cbz".to_string()),
        Some("zip") => Some("zip".to_string()),
        _ => None,
    }
}

/// Decodes RFC 3986 / RFC 5987 percent-encoded UTF-8 strings WITHOUT turning `+` into spaces.
pub(crate) fn percent_decode_utf8(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (hex_nibble(bytes[i + 1]), hex_nibble(bytes[i + 2]))
        {
            out.push((hi << 4) | lo);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

pub(crate) fn parse_filename_from_content_disposition(header: &str) -> Option<String> {
    let mut fallback: Option<String> = None;
    for part in header.split(';').map(str::trim) {
        let lower = part.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("filename*=") {
            let orig_val = &part[part.len() - rest.len()..];
            let encoded = orig_val
                .split_once("''")
                .map(|(_, v)| v)
                .unwrap_or(orig_val)
                .trim_matches('"');
            let decoded = percent_decode_utf8(encoded);
            if !decoded.trim().is_empty() {
                return Some(decoded);
            }
        }
        if fallback.is_none()
            && let Some(rest) = lower.strip_prefix("filename=")
        {
            let orig_val = &part[part.len() - rest.len()..];
            let cleaned = orig_val.trim().trim_matches('"').trim();
            if !cleaned.is_empty() {
                fallback = Some(cleaned.to_string());
            }
        }
    }
    fallback
}

fn sanitize_filename(raw: &str) -> String {
    let base = raw
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(raw)
        .trim()
        .replace(['\0', '\r', '\n'], "");
    if base.is_empty() {
        "import".to_string()
    } else {
        base
    }
}

fn resolve_import_filename_and_format(
    filename_hint: Option<&str>,
    content_disposition: Option<&str>,
    content_type: &str,
    final_url: &Url,
) -> Result<(String, String), OpdsError> {
    let candidate = filename_hint
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(sanitize_filename)
        .or_else(|| {
            content_disposition
                .and_then(parse_filename_from_content_disposition)
                .map(|s| sanitize_filename(&s))
        })
        .or_else(|| {
            final_url
                .path_segments()
                .and_then(|mut segs| segs.next_back())
                .filter(|s| !s.is_empty() && *s != "download")
                .map(percent_decode_utf8)
                .map(|s| sanitize_filename(&s))
        })
        .unwrap_or_else(|| "remote-import".to_string());

    let ext_from_name = std::path::Path::new(&candidate)
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .filter(|e| matches!(e.as_str(), "epub" | "pdf" | "txt" | "cbz" | "zip"));

    let format = ext_from_name
        .clone()
        .or_else(|| detect_format_from_mime_or_href(content_type, final_url.as_str()))
        .ok_or_else(|| {
            OpdsError::Unsupported(format!(
                "Could not determine supported book/comic format from filename '{candidate}' or Content-Type '{content_type}'"
            ))
        })?;

    let final_filename = if ext_from_name.is_some() {
        candidate
    } else {
        format!("{candidate}.{format}")
    };

    Ok((final_filename, format))
}

fn xml_attr(e: &BytesStart<'_>, local_key: &str) -> Option<String> {
    for attr in e.attributes().flatten() {
        if attr.key.local_name().as_ref() == local_key
            && let Ok(val) = attr.normalized_value(quick_xml::XmlVersion::Implicit1_0)
        {
            return Some(val.into_owned());
        }
    }
    None
}

fn append_xml_ref(out: &mut String, r: &quick_xml::events::BytesRef<'_>) {
    if let Ok(Some(ch)) = r.resolve_char_ref() {
        out.push(ch);
        return;
    }
    if let Some(resolved) = quick_xml::escape::resolve_xml_entity(r.as_ref()) {
        out.push_str(resolved);
    } else {
        out.push('&');
        out.push_str(r.as_ref());
        out.push(';');
    }
}

fn parse_opensearch_template(xml: &str) -> Option<String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut fallback: Option<String> = None;
    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e) | Event::Empty(ref e))
                if e.local_name().as_ref().eq_ignore_ascii_case("url") =>
            {
                if let Some(template) = xml_attr(e, "template") {
                    let mime = xml_attr(e, "type").unwrap_or_default().to_ascii_lowercase();
                    if mime.contains("atom") || mime.contains("opds") {
                        return Some(template);
                    }
                    if fallback.is_none() {
                        fallback = Some(template);
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }
    fallback
}

fn is_disallowed_acquisition_rel(rel: &str) -> bool {
    let lower = rel.to_ascii_lowercase();
    lower.contains("acquisition/buy")
        || lower.contains("acquisition/borrow")
        || lower.contains("acquisition/subscribe")
}

fn is_direct_acquisition_rel(rel: &str) -> bool {
    let lower = rel.to_ascii_lowercase();
    (lower.starts_with("http://opds-spec.org/acquisition") || lower.contains("acquisition"))
        && !is_disallowed_acquisition_rel(&lower)
}

#[derive(Default)]
struct AtomEntryBuilder {
    id: String,
    title: String,
    authors: Vec<String>,
    summary: Option<String>,
    content: Option<String>,
    updated: Option<String>,
    cover_raw: Option<String>,
    thumbnail_raw: Option<String>,
    pse_stream_url: Option<String>,
    page_count: Option<u64>,
    downloads: Vec<OpdsDownloadLinkDto>,
    nav_href: Option<String>,
    nav_count: Option<u64>,
}

fn stack_matches(stack: &[String], expected: &[&str]) -> bool {
    stack.len() == expected.len() && stack.iter().zip(expected.iter()).all(|(a, b)| a == b)
}

fn parse_opds1_atom(
    source_id: &str,
    base_url: &Url,
    xml: &str,
) -> Result<(OpdsFeedDto, bool), OpdsError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);

    let mut saw_feed = false;
    let mut feed_title = String::new();
    let mut search_url: Option<String> = None;
    let mut search_is_opensearch = false;
    let mut prev_url: Option<String> = None;
    let mut next_url: Option<String> = None;
    let mut nav_links: Vec<OpdsNavLinkDto> = Vec::new();
    let mut entries: Vec<OpdsEntryDto> = Vec::new();

    let mut tag_stack: Vec<String> = Vec::new();
    let mut capture_depth: Option<usize> = None;
    let mut text_buf = String::new();
    let mut cur_entry = AtomEntryBuilder::default();

    let handle_link = |e: &BytesStart<'_>,
                       in_entry: bool,
                       cur_entry: &mut AtomEntryBuilder,
                       search_url: &mut Option<String>,
                       search_is_opensearch: &mut bool,
                       prev_url: &mut Option<String>,
                       next_url: &mut Option<String>| {
        let Some(raw_href) = xml_attr(e, "href").filter(|h| !h.trim().is_empty()) else {
            return;
        };
        let abs_href = resolve_link_url(base_url, &raw_href);
        let rel = xml_attr(e, "rel").unwrap_or_default().to_ascii_lowercase();
        let media_type = xml_attr(e, "type").unwrap_or_default();
        let lower_type = media_type.to_ascii_lowercase();
        let title = xml_attr(e, "title").filter(|t| !t.trim().is_empty());
        let pse_count = xml_attr(e, "count").and_then(|c| c.trim().parse::<u64>().ok());

        if !in_entry {
            if rel == "next" && next_url.is_none() {
                *next_url = Some(abs_href);
            } else if (rel == "previous" || rel == "prev") && prev_url.is_none() {
                *prev_url = Some(abs_href);
            } else if rel == "search" && search_url.is_none() {
                *search_is_opensearch = lower_type.contains("opensearchdescription");
                *search_url = Some(abs_href);
            }
            return;
        }

        if is_disallowed_acquisition_rel(&rel) {
            return;
        }

        if is_direct_acquisition_rel(&rel) {
            let format = detect_format_from_mime_or_href(&media_type, &abs_href)
                .unwrap_or_else(|| "file".to_string());
            cur_entry.downloads.push(OpdsDownloadLinkDto {
                href: abs_href,
                title,
                format,
                media_type: if media_type.is_empty() {
                    "application/octet-stream".to_string()
                } else {
                    media_type
                },
            });
            return;
        }

        if rel == "http://vaemendis.net/opds-pse/stream" {
            cur_entry.pse_stream_url = Some(abs_href);
            if let Some(count) = pse_count {
                cur_entry.page_count = Some(count);
            }
            return;
        }

        if rel == "http://opds-spec.org/image/thumbnail" || rel == "thumbnail" {
            if cur_entry.thumbnail_raw.is_none() {
                cur_entry.thumbnail_raw = Some(abs_href);
            }
            return;
        }

        if rel == "http://opds-spec.org/image"
            || rel == "cover"
            || rel == "http://opds-spec.org/cover"
        {
            if cur_entry.cover_raw.is_none() {
                cur_entry.cover_raw = Some(abs_href);
            }
            return;
        }

        if rel == "subsection"
            || rel == "http://opds-spec.org/sort/new"
            || rel == "http://opds-spec.org/sort/popular"
            || rel == "http://opds-spec.org/featured"
            || lower_type.contains("profile=opds-catalog")
            || lower_type.contains("application/atom+xml")
        {
            if cur_entry.nav_href.is_none() {
                cur_entry.nav_href = Some(abs_href);
            }
            if let Some(count) = pse_count {
                cur_entry.nav_count = Some(count);
            }
        }
    };

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let tag = e.local_name().as_ref().to_ascii_lowercase();
                tag_stack.push(tag);

                if stack_matches(&tag_stack, &["feed"]) {
                    saw_feed = true;
                } else if stack_matches(&tag_stack, &["feed", "entry"]) {
                    cur_entry = AtomEntryBuilder::default();
                } else if stack_matches(&tag_stack, &["feed", "link"]) {
                    handle_link(
                        e,
                        false,
                        &mut cur_entry,
                        &mut search_url,
                        &mut search_is_opensearch,
                        &mut prev_url,
                        &mut next_url,
                    );
                } else if stack_matches(&tag_stack, &["feed", "entry", "link"]) {
                    handle_link(
                        e,
                        true,
                        &mut cur_entry,
                        &mut search_url,
                        &mut search_is_opensearch,
                        &mut prev_url,
                        &mut next_url,
                    );
                } else if capture_depth.is_none()
                    && (stack_matches(&tag_stack, &["feed", "title"])
                        || stack_matches(&tag_stack, &["feed", "entry", "id"])
                        || stack_matches(&tag_stack, &["feed", "entry", "title"])
                        || stack_matches(&tag_stack, &["feed", "entry", "summary"])
                        || stack_matches(&tag_stack, &["feed", "entry", "content"])
                        || stack_matches(&tag_stack, &["feed", "entry", "updated"])
                        || stack_matches(&tag_stack, &["feed", "entry", "author", "name"]))
                {
                    capture_depth = Some(tag_stack.len());
                    text_buf.clear();
                }
            }
            Ok(Event::Empty(ref e)) => {
                let tag = e.local_name().as_ref().to_ascii_lowercase();
                if tag_stack.is_empty() && tag == "feed" {
                    saw_feed = true;
                } else if tag == "link" {
                    if stack_matches(&tag_stack, &["feed"]) {
                        handle_link(
                            e,
                            false,
                            &mut cur_entry,
                            &mut search_url,
                            &mut search_is_opensearch,
                            &mut prev_url,
                            &mut next_url,
                        );
                    } else if stack_matches(&tag_stack, &["feed", "entry"]) {
                        handle_link(
                            e,
                            true,
                            &mut cur_entry,
                            &mut search_url,
                            &mut search_is_opensearch,
                            &mut prev_url,
                            &mut next_url,
                        );
                    }
                }
            }
            Ok(Event::Text(ref t)) => {
                if capture_depth.is_some() {
                    text_buf.push_str(&t.xml_content(quick_xml::XmlVersion::Implicit1_0));
                }
            }
            Ok(Event::CData(ref c)) => {
                if capture_depth.is_some() {
                    text_buf.push_str(c.as_ref());
                }
            }
            Ok(Event::GeneralRef(ref r)) => {
                if capture_depth.is_some() {
                    append_xml_ref(&mut text_buf, r);
                }
            }
            Ok(Event::End(ref _e)) => {
                if capture_depth == Some(tag_stack.len()) {
                    let value = text_buf.trim().to_string();
                    if !value.is_empty() {
                        if stack_matches(&tag_stack, &["feed", "title"]) && feed_title.is_empty() {
                            feed_title = value;
                        } else if stack_matches(&tag_stack, &["feed", "entry", "author", "name"]) {
                            cur_entry.authors.push(value);
                        } else if stack_matches(&tag_stack, &["feed", "entry", "id"]) {
                            cur_entry.id = value;
                        } else if stack_matches(&tag_stack, &["feed", "entry", "title"]) {
                            cur_entry.title = value;
                        } else if stack_matches(&tag_stack, &["feed", "entry", "summary"]) {
                            cur_entry.summary = Some(value);
                        } else if stack_matches(&tag_stack, &["feed", "entry", "content"]) {
                            cur_entry.content = Some(value);
                        } else if stack_matches(&tag_stack, &["feed", "entry", "updated"]) {
                            cur_entry.updated = Some(value);
                        }
                    }
                    capture_depth = None;
                    text_buf.clear();
                } else if stack_matches(&tag_stack, &["feed", "entry"]) {
                    let finished = std::mem::take(&mut cur_entry);
                    if finished.downloads.is_empty()
                        && finished.pse_stream_url.is_none()
                        && let Some(href) = finished.nav_href
                    {
                        nav_links.push(OpdsNavLinkDto {
                            title: if finished.title.is_empty() {
                                href.clone()
                            } else {
                                finished.title
                            },
                            href,
                            count: finished.nav_count,
                        });
                    } else if !finished.title.is_empty()
                        || !finished.downloads.is_empty()
                        || finished.pse_stream_url.is_some()
                    {
                        let cover_url = finished
                            .thumbnail_raw
                            .or(finished.cover_raw)
                            .map(|raw| proxy_image_path(source_id, &raw));
                        entries.push(OpdsEntryDto {
                            id: if finished.id.is_empty() {
                                Uuid::new_v4().to_string()
                            } else {
                                finished.id
                            },
                            title: finished.title,
                            authors: finished.authors,
                            summary: finished.summary.or(finished.content),
                            cover_url,
                            updated: finished.updated,
                            pse_stream_url: finished.pse_stream_url,
                            page_count: finished.page_count,
                            downloads: finished.downloads,
                        });
                    }
                }
                tag_stack.pop();
            }
            Ok(Event::Eof) => break,
            Err(err) => {
                return Err(OpdsError::Upstream(format!(
                    "Failed to parse OPDS 1.2 Atom XML: {err}"
                )));
            }
            _ => {}
        }
    }

    if !saw_feed {
        return Err(OpdsError::Upstream(
            "XML document does not contain an Atom <feed> root element".to_string(),
        ));
    }

    Ok((
        OpdsFeedDto {
            title: if feed_title.is_empty() {
                "OPDS Catalog".to_string()
            } else {
                feed_title
            },
            url: base_url.to_string(),
            opds_version: "1.2".to_string(),
            search_url,
            prev_url,
            next_url,
            nav_links,
            entries,
        },
        search_is_opensearch,
    ))
}

fn link_has_rel(link: &Value, expected: &str) -> bool {
    match link.get("rel") {
        Some(Value::String(s)) => s.eq_ignore_ascii_case(expected),
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(Value::as_str)
            .any(|s| s.eq_ignore_ascii_case(expected)),
        _ => false,
    }
}

fn link_rel_matches_any<F>(link: &Value, pred: F) -> bool
where
    F: Fn(&str) -> bool,
{
    match link.get("rel") {
        Some(Value::String(s)) => pred(s),
        Some(Value::Array(arr)) => arr.iter().filter_map(Value::as_str).any(pred),
        _ => false,
    }
}

/// Extracts a human-readable string from either a plain JSON string or a Readium/OPDS 2.0
/// localized string map (e.g. `{"en": "Title", "zh": "标题"}`).
fn extract_localized_string(val: &Value) -> Option<String> {
    match val {
        Value::String(s) => {
            let trimmed = s.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }
        Value::Object(map) => {
            for preferred in ["zh-CN", "zh", "en", "en-US", "und"] {
                if let Some(s) = map.get(preferred).and_then(Value::as_str) {
                    let trimmed = s.trim();
                    if !trimmed.is_empty() {
                        return Some(trimmed.to_string());
                    }
                }
            }
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            for key in keys {
                if let Some(s) = map.get(key).and_then(Value::as_str) {
                    let trimmed = s.trim();
                    if !trimmed.is_empty() {
                        return Some(trimmed.to_string());
                    }
                }
            }
            None
        }
        _ => None,
    }
}

fn extract_opds2_authors(meta: &Value) -> Vec<String> {
    let mut out = Vec::new();
    match meta.get("author") {
        Some(Value::String(s)) if !s.trim().is_empty() => out.push(s.trim().to_string()),
        Some(Value::Object(map)) => {
            if let Some(name_val) = map.get("name")
                && let Some(name) = extract_localized_string(name_val)
            {
                out.push(name);
            }
        }
        Some(Value::Array(arr)) => {
            for item in arr {
                match item {
                    Value::String(s) if !s.trim().is_empty() => out.push(s.trim().to_string()),
                    Value::Object(map) => {
                        if let Some(name_val) = map.get("name")
                            && let Some(name) = extract_localized_string(name_val)
                        {
                            out.push(name);
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
    out
}

fn parse_opds2_publication(
    source_id: &str,
    base_url: &Url,
    pub_val: &Value,
) -> Option<OpdsEntryDto> {
    let meta = pub_val.get("metadata")?;
    let title = meta
        .get("title")
        .and_then(extract_localized_string)
        .unwrap_or_default();
    let id = meta
        .get("identifier")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let authors = extract_opds2_authors(meta);
    let summary = meta.get("description").and_then(extract_localized_string);
    let updated = meta
        .get("modified")
        .or_else(|| meta.get("published"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let mut page_count = meta.get("numberOfPages").and_then(Value::as_u64);
    let mut pse_stream_url: Option<String> = None;

    let cover_url = pub_val
        .get("images")
        .and_then(Value::as_array)
        .and_then(|imgs| imgs.first())
        .and_then(|img| img.get("href"))
        .and_then(Value::as_str)
        .map(|href| resolve_link_url(base_url, href))
        .filter(|u| !u.is_empty())
        .map(|abs| proxy_image_path(source_id, &abs));

    let mut downloads = Vec::new();
    if let Some(links) = pub_val.get("links").and_then(Value::as_array) {
        for link in links {
            let Some(raw_href) = link.get("href").and_then(Value::as_str) else {
                continue;
            };
            if link_has_rel(link, "http://vaemendis.net/opds-pse/stream") {
                pse_stream_url = Some(resolve_link_url(base_url, raw_href));
                if let Some(count) = link
                    .pointer("/properties/numberOfPages")
                    .or_else(|| link.pointer("/properties/count"))
                    .and_then(Value::as_u64)
                {
                    page_count = Some(count);
                }
                continue;
            }
            if link_rel_matches_any(link, is_disallowed_acquisition_rel) {
                continue;
            }
            let media_type = link
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let is_acq = link_rel_matches_any(link, is_direct_acquisition_rel)
                || detect_format_from_mime_or_href(&media_type, raw_href).is_some();
            if !is_acq {
                continue;
            }
            let abs_href = resolve_link_url(base_url, raw_href);
            let format = detect_format_from_mime_or_href(&media_type, &abs_href)
                .unwrap_or_else(|| "file".to_string());
            downloads.push(OpdsDownloadLinkDto {
                href: abs_href,
                title: link.get("title").and_then(extract_localized_string),
                format,
                media_type: if media_type.is_empty() {
                    "application/octet-stream".to_string()
                } else {
                    media_type
                },
            });
        }
    }

    Some(OpdsEntryDto {
        id,
        title,
        authors,
        summary,
        cover_url,
        updated,
        pse_stream_url,
        page_count,
        downloads,
    })
}

fn parse_opds2_json(source_id: &str, base_url: &Url, raw: &str) -> Result<OpdsFeedDto, OpdsError> {
    let root: Value = serde_json::from_str(raw)
        .map_err(|e| OpdsError::Upstream(format!("Failed to parse OPDS 2.0 JSON: {e}")))?;

    let meta = root
        .get("metadata")
        .ok_or_else(|| OpdsError::Upstream("OPDS 2.0 feed is missing 'metadata' object".into()))?;
    let title = meta
        .get("title")
        .and_then(extract_localized_string)
        .unwrap_or_else(|| "OPDS 2.0 Catalog".to_string());

    let mut search_url: Option<String> = None;
    let mut prev_url: Option<String> = None;
    let mut next_url: Option<String> = None;
    if let Some(links) = root.get("links").and_then(Value::as_array) {
        for link in links {
            let Some(href) = link.get("href").and_then(Value::as_str) else {
                continue;
            };
            if link_has_rel(link, "next") && next_url.is_none() {
                next_url = Some(resolve_link_url(base_url, href));
            } else if (link_has_rel(link, "previous") || link_has_rel(link, "prev"))
                && prev_url.is_none()
            {
                prev_url = Some(resolve_link_url(base_url, href));
            } else if link_has_rel(link, "search") && search_url.is_none() {
                search_url = Some(resolve_link_url(base_url, href));
            }
        }
    }

    let mut nav_links = Vec::new();
    let collect_nav =
        |items: &[Value], group_prefix: Option<&str>, out: &mut Vec<OpdsNavLinkDto>| {
            for item in items {
                let Some(href) = item.get("href").and_then(Value::as_str) else {
                    continue;
                };
                let abs = resolve_link_url(base_url, href);
                let raw_title = item
                    .get("title")
                    .and_then(extract_localized_string)
                    .unwrap_or_else(|| abs.clone());
                let nav_title = match group_prefix {
                    Some(prefix) if !prefix.is_empty() && raw_title != prefix => {
                        format!("{prefix} · {raw_title}")
                    }
                    _ => raw_title,
                };
                let count = item
                    .pointer("/properties/numberOfItems")
                    .and_then(Value::as_u64);
                out.push(OpdsNavLinkDto {
                    title: nav_title,
                    href: abs,
                    count,
                });
            }
        };

    if let Some(nav) = root.get("navigation").and_then(Value::as_array) {
        collect_nav(nav, None, &mut nav_links);
    }

    let mut entries = Vec::new();
    if let Some(pubs) = root.get("publications").and_then(Value::as_array) {
        for p in pubs {
            if let Some(entry) = parse_opds2_publication(source_id, base_url, p) {
                entries.push(entry);
            }
        }
    }

    if let Some(groups) = root.get("groups").and_then(Value::as_array) {
        for group in groups {
            let group_title = group
                .get("metadata")
                .and_then(|m| m.get("title"))
                .and_then(extract_localized_string);
            let group_count = group
                .pointer("/metadata/numberOfItems")
                .and_then(Value::as_u64);

            if let Some(ref gtitle) = group_title
                && let Some(links) = group.get("links").and_then(Value::as_array)
                && let Some(link) = links
                    .iter()
                    .find(|l| link_has_rel(l, "self"))
                    .or_else(|| links.first())
                && let Some(href) = link.get("href").and_then(Value::as_str)
            {
                nav_links.push(OpdsNavLinkDto {
                    title: gtitle.clone(),
                    href: resolve_link_url(base_url, href),
                    count: group_count,
                });
            }

            if let Some(nav) = group.get("navigation").and_then(Value::as_array) {
                collect_nav(nav, group_title.as_deref(), &mut nav_links);
            }
            if let Some(pubs) = group.get("publications").and_then(Value::as_array) {
                for p in pubs {
                    if let Some(entry) = parse_opds2_publication(source_id, base_url, p) {
                        entries.push(entry);
                    }
                }
            }
        }
    }

    Ok(OpdsFeedDto {
        title,
        url: base_url.to_string(),
        opds_version: "2.0".to_string(),
        search_url,
        prev_url,
        next_url,
        nav_links,
        entries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_opds12_atom_navigation_and_acquisition_feeds() {
        let base = Url::parse("http://example.com/opds/book/v1.2").unwrap();
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom" xmlns:opds="http://opds-spec.org/2010/catalog" xmlns:pse="http://vaemendis.net/ns/opds-pse/ns">
  <title>Test Books</title>
  <link rel="search" href="/opds/book/v1.2/opensearch.xml" type="application/opensearchdescription+xml"/>
  <link rel="previous" href="/opds/book/v1.2/all?page=1"/>
  <link rel="next" href="/opds/book/v1.2/all?page=3"/>
  <entry>
    <id>nav-1</id>
    <title>All Books</title>
    <link rel="subsection" href="/opds/book/v1.2/all" type="application/atom+xml;profile=opds-catalog;kind=acquisition"/>
  </entry>
  <entry>
    <id>urn:uuid:book-1</id>
    <title>Foundation</title>
    <source>
      <id>urn:uuid:source-should-not-overwrite</id>
      <title>Source Title Should Not Overwrite</title>
    </source>
    <updated>2026-01-01T00:00:00Z</updated>
    <author><name>Isaac Asimov</name></author>
    <summary>A classic sci-fi novel.</summary>
    <link rel="http://opds-spec.org/image/thumbnail" href="/opds/book/books/1/cover" type="image/jpeg"/>
    <link rel="http://opds-spec.org/acquisition/buy" href="/buy/1" type="text/html"/>
    <link rel="http://opds-spec.org/acquisition/borrow" href="/borrow/1" type="text/html"/>
    <link rel="http://opds-spec.org/acquisition" href="/opds/book/books/1/download" type="application/epub+zip"/>
  </entry>
  <entry>
    <id>urn:uuid:comic-1</id>
    <title>Folder Manga</title>
    <link rel="http://vaemendis.net/opds-pse/stream" href="/opds/comic/chapters/1/page?page={pageNumber}" type="image/jpeg" pse:count="24"/>
  </entry>
</feed>"#;

        let (feed, is_opensearch) =
            parse_opds_feed("src-1", &base, "application/atom+xml", xml).unwrap();
        assert_eq!(feed.opds_version, "1.2");
        assert!(is_opensearch);
        assert_eq!(feed.title, "Test Books");
        assert_eq!(
            feed.prev_url.as_deref(),
            Some("http://example.com/opds/book/v1.2/all?page=1")
        );
        assert_eq!(
            feed.next_url.as_deref(),
            Some("http://example.com/opds/book/v1.2/all?page=3")
        );
        assert_eq!(feed.nav_links.len(), 1);
        assert_eq!(feed.nav_links[0].title, "All Books");
        assert_eq!(feed.entries.len(), 2);

        let book = &feed.entries[0];
        assert_eq!(book.id, "urn:uuid:book-1");
        assert_eq!(book.title, "Foundation");
        assert_eq!(book.authors, vec!["Isaac Asimov"]);
        assert_eq!(
            book.downloads.len(),
            1,
            "buy/borrow links must be filtered out"
        );
        assert_eq!(book.downloads[0].format, "epub");
        assert!(
            book.cover_url
                .as_deref()
                .unwrap()
                .starts_with("/_app/opds/src-1/proxy-image?url=")
        );

        let comic = &feed.entries[1];
        assert_eq!(comic.title, "Folder Manga");
        assert_eq!(
            comic.pse_stream_url.as_deref(),
            Some("http://example.com/opds/comic/chapters/1/page?page={pageNumber}")
        );
        assert_eq!(comic.page_count, Some(24));
    }

    #[test]
    fn parses_opds2_localized_objects_groups_and_pagination() {
        let base = Url::parse("http://example.com/opds/v2").unwrap();
        let json = r#"{
            "metadata": { "title": { "en": "World Library", "zh": "世界书库" } },
            "links": [
                { "rel": "previous", "href": "/opds/v2?page=1" },
                { "rel": "next", "href": "/opds/v2?page=3" },
                { "rel": "search", "href": "/opds/v2/search{?query}", "templated": true }
            ],
            "groups": [
                {
                    "metadata": { "title": { "en": "Featured" }, "numberOfItems": 12 },
                    "links": [ { "rel": "self", "href": "/opds/v2/featured" } ],
                    "navigation": [ { "title": "Sci-Fi", "href": "/opds/v2/scifi" } ],
                    "publications": [
                        {
                            "metadata": {
                                "identifier": "pub-1",
                                "title": { "en": "Dune" },
                                "description": { "en": "Desert planet" },
                                "author": { "name": { "en": "Frank Herbert" } },
                                "numberOfPages": 412
                            },
                            "links": [
                                { "rel": "http://opds-spec.org/acquisition/buy", "href": "/buy/dune", "type": "text/html" },
                                { "rel": "http://opds-spec.org/acquisition", "href": "/dl/dune.epub", "type": "application/epub+zip" }
                            ]
                        }
                    ]
                }
            ]
        }"#;

        let (feed, _) = parse_opds_feed("src-2", &base, "application/opds+json", json).unwrap();
        assert_eq!(feed.opds_version, "2.0");
        assert_eq!(feed.title, "世界书库");
        assert_eq!(
            feed.prev_url.as_deref(),
            Some("http://example.com/opds/v2?page=1")
        );
        assert_eq!(
            feed.next_url.as_deref(),
            Some("http://example.com/opds/v2?page=3")
        );
        assert!(
            feed.nav_links
                .iter()
                .any(|n| n.title == "Featured" && n.count == Some(12))
        );
        assert!(
            feed.nav_links
                .iter()
                .any(|n| n.title == "Featured · Sci-Fi")
        );
        assert_eq!(feed.entries.len(), 1);
        assert_eq!(feed.entries[0].title, "Dune");
        assert_eq!(feed.entries[0].authors, vec!["Frank Herbert"]);
        assert_eq!(feed.entries[0].summary.as_deref(), Some("Desert planet"));
        assert_eq!(feed.entries[0].page_count, Some(412));
        assert_eq!(feed.entries[0].downloads.len(), 1);
    }

    #[test]
    fn expands_opensearch_and_opds2_templates() {
        assert_eq!(
            expand_search_template("http://h/search?q={searchTerms}", "sci fi"),
            "http://h/search?q=sci+fi"
        );
        assert_eq!(
            expand_search_template(
                "http://h/search?q={searchTerms?}&page={startPage?}&count={count?}",
                "rust"
            ),
            "http://h/search?q=rust"
        );
        assert_eq!(
            expand_search_template("http://h/v2/search{?query}", "Foundation"),
            "http://h/v2/search?query=Foundation"
        );
        assert_eq!(
            expand_search_template("http://h/v2/search?lang=zh{&query}", "三体"),
            "http://h/v2/search?lang=zh&query=%E4%B8%89%E4%BD%93"
        );
    }

    #[test]
    fn resolves_only_same_origin_http_urls_and_blocks_protocol_relative_bypass() {
        let base = "http://127.0.0.1:8080/opds/book/v1.2";
        assert_eq!(
            resolve_target_url(base, Some("/opds/book/v1.2/all")).unwrap(),
            "http://127.0.0.1:8080/opds/book/v1.2/all"
        );
        assert!(resolve_target_url(base, Some("http://evil.example/steal")).is_err());
        assert!(resolve_target_url(base, Some("https://127.0.0.1:8080/opds")).is_err());
        assert!(resolve_target_url(base, Some("http://127.0.0.1:9999/opds")).is_err());
        assert!(resolve_target_url(base, Some("//evil.example/steal")).is_err());
        assert!(resolve_target_url(base, Some("file:///etc/passwd")).is_err());
        assert!(resolve_target_url(base, Some("javascript:alert(1)")).is_err());
    }

    #[test]
    fn rfc5987_content_disposition_preserves_plus_signs() {
        let hdr =
            r#"attachment; filename="fallback.epub"; filename*=UTF-8''C%2B%2B+Guide%202026.epub"#;
        assert_eq!(
            parse_filename_from_content_disposition(hdr).as_deref(),
            Some("C+++Guide 2026.epub")
        );
        assert_eq!(percent_decode_utf8("C%2B%2B.epub"), "C++.epub");
    }

    #[test]
    fn raster_image_content_type_whitelist_blocks_svg_and_html() {
        assert!(is_safe_raster_image_content_type("image/jpeg"));
        assert!(is_safe_raster_image_content_type(
            "image/png; charset=binary"
        ));
        assert!(is_safe_raster_image_content_type("image/webp"));
        assert!(is_safe_raster_image_content_type("image/gif"));
        assert!(is_safe_raster_image_content_type("image/avif"));
        assert!(!is_safe_raster_image_content_type("image/svg+xml"));
        assert!(!is_safe_raster_image_content_type("text/html"));
        assert!(!is_safe_raster_image_content_type("application/javascript"));
    }
}

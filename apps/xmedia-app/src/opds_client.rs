use std::fmt;
use std::path::PathBuf;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::header::{CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE};
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

use crate::remote_sources::{RemoteSource, normalize_remote_url};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const TEST_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_IMAGE_BYTES: u64 = 12 * 1024 * 1024;
const MAX_IMPORT_BYTES: u64 = 512 * 1024 * 1024;

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
}

impl OpdsClient {
    pub fn new(secure: reqwest::Client, insecure: reqwest::Client) -> Self {
        Self { secure, insecure }
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
                    error: Some(format!("Connection failed: {err}")),
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
        let body = match resp.text().await {
            Ok(t) => t,
            Err(err) => {
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
                let search_target = expand_search_template(&target_url, q);
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

            let search_target = if search_href.contains('{') {
                expand_search_template(search_href, q)
            } else if search_link_is_opensearch
                || search_href.to_ascii_lowercase().ends_with(".xml")
                || search_href.to_ascii_lowercase().contains("opensearch")
            {
                let template = self.fetch_opensearch_template(source, search_href).await?;
                expand_search_template(&template, q)
            } else {
                append_query_param(search_href, "q", q)
            };

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
        let req = self
            .client_for(source.allow_insecure_tls)
            .get(url)
            .header(
                reqwest::header::ACCEPT,
                "application/atom+xml, application/opds+json, application/xml, application/json;q=0.9, */*;q=0.8",
            )
            .timeout(REQUEST_TIMEOUT);
        let req = self.apply_auth(req, &source.username, &source.password);

        let resp = req
            .send()
            .await
            .map_err(|e| OpdsError::Upstream(format!("OPDS request failed: {e}")))?;

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
        let body = resp
            .text()
            .await
            .map_err(|e| OpdsError::Upstream(format!("Failed to read OPDS body: {e}")))?;

        parse_opds_feed(&source.id, &final_url, &content_type, &body)
    }

    async fn fetch_opensearch_template(
        &self,
        source: &RemoteSource,
        descriptor_url: &str,
    ) -> Result<String, OpdsError> {
        let req = self
            .client_for(source.allow_insecure_tls)
            .get(descriptor_url)
            .timeout(REQUEST_TIMEOUT);
        let req = self.apply_auth(req, &source.username, &source.password);

        let resp = req
            .send()
            .await
            .map_err(|e| OpdsError::Upstream(format!("OpenSearch request failed: {e}")))?;
        if !resp.status().is_success() {
            return Err(OpdsError::Upstream(format!(
                "OpenSearch descriptor returned HTTP {}",
                resp.status().as_u16()
            )));
        }

        let base_url = resp.url().clone();
        let xml = resp
            .text()
            .await
            .map_err(|e| OpdsError::Upstream(format!("Failed to read OpenSearch XML: {e}")))?;

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

        let resp = req
            .send()
            .await
            .map_err(|e| OpdsError::Upstream(format!("Failed to fetch OPDS image: {e}")))?;

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

        if !content_type.to_ascii_lowercase().starts_with("image/") {
            return Err(OpdsError::Unsupported(format!(
                "Remote image returned non-image Content-Type '{content_type}'"
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

        let limited = LimitedByteStream {
            inner: Box::pin(resp.bytes_stream()),
            seen: 0,
            max_bytes: MAX_IMAGE_BYTES,
        };

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
            OpdsError::Upstream(format!("Failed to download remote OPDS file: {e}"))
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

        let temp_guard = stream_response_to_temp_file(resp, MAX_IMPORT_BYTES).await?;
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
                OpdsError::Internal(format!("Failed to upload into local library: {e}"))
            })?;

        let status = upload_resp.status();
        let resp_json: Value = upload_resp
            .json()
            .await
            .map_err(|e| OpdsError::Internal(format!("Invalid local upload response: {e}")))?;

        if !status.is_success() {
            let msg = resp_json
                .pointer("/error/message")
                .and_then(Value::as_str)
                .unwrap_or("Local library rejected uploaded file");
            return Err(OpdsError::Upstream(format!(
                "Local import failed (HTTP {}): {msg}",
                status.as_u16()
            )));
        }

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
) -> Result<TempDownloadFile, OpdsError> {
    let path = std::env::temp_dir().join(format!("xmedia-opds-import-{}.tmp", Uuid::new_v4()));
    let mut file = tokio::fs::File::create(&path)
        .await
        .map_err(|e| OpdsError::Internal(format!("Failed to create temp file: {e}")))?;

    let mut total = 0u64;
    let mut stream = resp.bytes_stream();
    while let Some(chunk_res) = stream.next().await {
        let chunk = chunk_res
            .map_err(|e| OpdsError::Upstream(format!("Error reading remote stream: {e}")))?;
        total = total.saturating_add(chunk.len() as u64);
        if total > max_bytes {
            drop(file);
            let _ = tokio::fs::remove_file(&path).await;
            return Err(OpdsError::TooLarge(format!(
                "Remote download exceeded maximum limit of {max_bytes} bytes"
            )));
        }
        if let Err(err) = file.write_all(&chunk).await {
            drop(file);
            let _ = tokio::fs::remove_file(&path).await;
            return Err(OpdsError::Internal(format!(
                "Failed writing downloaded data to temp file: {err}"
            )));
        }
    }
    file.flush()
        .await
        .map_err(|e| OpdsError::Internal(format!("Failed to flush temp file: {e}")))?;

    Ok(TempDownloadFile { path, size: total })
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

struct LimitedByteStream {
    inner: Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>,
    seen: u64,
    max_bytes: u64,
}

impl Stream for LimitedByteStream {
    type Item = Result<Bytes, std::io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.inner.as_mut().poll_next(cx) {
            Poll::Ready(Some(Ok(chunk))) => {
                self.seen = self.seen.saturating_add(chunk.len() as u64);
                if self.seen > self.max_bytes {
                    Poll::Ready(Some(Err(std::io::Error::other(
                        "remote image exceeded 12MB size limit",
                    ))))
                } else {
                    Poll::Ready(Some(Ok(chunk)))
                }
            }
            Poll::Ready(Some(Err(err))) => Poll::Ready(Some(Err(std::io::Error::other(err)))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

fn resolve_target_url(source_base: &str, target_opt: Option<&str>) -> Result<String, OpdsError> {
    let base = Url::parse(source_base)
        .map_err(|e| OpdsError::BadRequest(format!("Invalid source URL: {e}")))?;
    let resolved = match target_opt.map(str::trim).filter(|s| !s.is_empty()) {
        Some(raw) => {
            if raw.starts_with("http://") || raw.starts_with("https://") {
                Url::parse(raw)
                    .map_err(|e| OpdsError::BadRequest(format!("Invalid target URL: {e}")))?
            } else {
                base.join(raw)
                    .map_err(|e| OpdsError::BadRequest(format!("Invalid relative URL: {e}")))?
            }
        }
        None => base,
    };

    match resolved.scheme() {
        "http" | "https" => Ok(resolved.to_string()),
        scheme => Err(OpdsError::BadRequest(format!(
            "Unsupported URL scheme '{scheme}'"
        ))),
    }
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

fn expand_search_template(template: &str, query: &str) -> String {
    let encoded = encode_query_component(query);
    if template.contains("{searchTerms}") {
        return template.replace("{searchTerms}", &encoded);
    }
    // Support RFC 6570 OPDS 2.0 templates like "/v2/search{?query}" or "/v2/search{?q}"
    if let Some(start) = template.find("{?")
        && let Some(rel_end) = template[start..].find('}')
    {
        let end = start + rel_end;
        let vars = &template[start + 2..end];
        let param_name = vars
            .split(',')
            .map(str::trim)
            .find(|v| !v.is_empty())
            .unwrap_or("query");
        let prefix = &template[..start];
        let suffix = &template[end + 1..];
        let sep = if prefix.contains('?') { '&' } else { '?' };
        return format!("{prefix}{sep}{param_name}={encoded}{suffix}");
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

fn parse_filename_from_content_disposition(header: &str) -> Option<String> {
    for part in header.split(';').map(str::trim) {
        let lower = part.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("filename*=") {
            let orig_val = &part[part.len() - rest.len()..];
            let encoded = orig_val
                .split_once("''")
                .map(|(_, v)| v)
                .unwrap_or(orig_val)
                .trim_matches('"');
            let decoded: String = form_urlencoded::parse(format!("f={encoded}").as_bytes())
                .find(|(k, _)| k == "f")
                .map(|(_, v)| v.into_owned())
                .unwrap_or_else(|| encoded.to_string());
            if !decoded.trim().is_empty() {
                return Some(decoded);
            }
        }
        if let Some(rest) = lower.strip_prefix("filename=") {
            let orig_val = &part[part.len() - rest.len()..];
            let cleaned = orig_val.trim().trim_matches('"').trim();
            if !cleaned.is_empty() {
                return Some(cleaned.to_string());
            }
        }
    }
    None
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
                .map(sanitize_filename)
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
    downloads: Vec<OpdsDownloadLinkDto>,
    nav_href: Option<String>,
    nav_count: Option<u64>,
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
    let mut next_url: Option<String> = None;
    let mut nav_links: Vec<OpdsNavLinkDto> = Vec::new();
    let mut entries: Vec<OpdsEntryDto> = Vec::new();

    let mut in_entry = false;
    let mut in_author = false;
    let mut current_tag: Option<String> = None;
    let mut text_buf = String::new();
    let mut cur_entry = AtomEntryBuilder::default();

    let handle_link = |e: &BytesStart<'_>,
                       in_entry: bool,
                       cur_entry: &mut AtomEntryBuilder,
                       search_url: &mut Option<String>,
                       search_is_opensearch: &mut bool,
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
            } else if rel == "search" && search_url.is_none() {
                *search_is_opensearch = lower_type.contains("opensearchdescription");
                *search_url = Some(abs_href);
            }
            return;
        }

        if rel.starts_with("http://opds-spec.org/acquisition") || rel.contains("acquisition") {
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
            if let Some(count) = pse_count {
                cur_entry.nav_count = Some(count);
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
                let local = e.local_name();
                let tag = local.as_ref().to_ascii_lowercase();
                match tag.as_str() {
                    "feed" => saw_feed = true,
                    "entry" => {
                        in_entry = true;
                        in_author = false;
                        cur_entry = AtomEntryBuilder::default();
                    }
                    "author" if in_entry => {
                        in_author = true;
                    }
                    "link" => {
                        handle_link(
                            e,
                            in_entry,
                            &mut cur_entry,
                            &mut search_url,
                            &mut search_is_opensearch,
                            &mut next_url,
                        );
                    }
                    "title" | "id" | "summary" | "content" | "updated" | "name" => {
                        current_tag = Some(tag);
                        text_buf.clear();
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(ref e)) => {
                let local = e.local_name();
                let tag = local.as_ref().to_ascii_lowercase();
                if tag == "feed" {
                    saw_feed = true;
                } else if tag == "link" {
                    handle_link(
                        e,
                        in_entry,
                        &mut cur_entry,
                        &mut search_url,
                        &mut search_is_opensearch,
                        &mut next_url,
                    );
                }
            }
            Ok(Event::Text(ref t)) => {
                if current_tag.is_some() {
                    text_buf.push_str(&t.xml_content(quick_xml::XmlVersion::Implicit1_0));
                }
            }
            Ok(Event::CData(ref c)) => {
                if current_tag.is_some() {
                    text_buf.push_str(c.as_ref());
                }
            }
            Ok(Event::GeneralRef(ref r)) => {
                if current_tag.is_some() {
                    append_xml_ref(&mut text_buf, r);
                }
            }
            Ok(Event::End(ref e)) => {
                let local = e.local_name();
                let tag = local.as_ref().to_ascii_lowercase();
                match tag.as_str() {
                    "entry" if in_entry => {
                        in_entry = false;
                        in_author = false;
                        let finished = std::mem::take(&mut cur_entry);
                        if finished.downloads.is_empty()
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
                        } else if !finished.title.is_empty() || !finished.downloads.is_empty() {
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
                                downloads: finished.downloads,
                            });
                        }
                    }
                    "author" if in_entry => {
                        in_author = false;
                    }
                    _ => {
                        if current_tag.as_deref() == Some(tag.as_str()) {
                            let value = text_buf.trim().to_string();
                            if !value.is_empty() {
                                if !in_entry {
                                    if tag == "title" && feed_title.is_empty() {
                                        feed_title = value;
                                    }
                                } else if in_author {
                                    if tag == "name" {
                                        cur_entry.authors.push(value);
                                    }
                                } else {
                                    match tag.as_str() {
                                        "id" => cur_entry.id = value,
                                        "title" => cur_entry.title = value,
                                        "summary" => cur_entry.summary = Some(value),
                                        "content" => cur_entry.content = Some(value),
                                        "updated" => cur_entry.updated = Some(value),
                                        _ => {}
                                    }
                                }
                            }
                            current_tag = None;
                            text_buf.clear();
                        }
                    }
                }
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

fn link_rel_starts_with(link: &Value, prefix: &str) -> bool {
    match link.get("rel") {
        Some(Value::String(s)) => s.to_ascii_lowercase().starts_with(prefix),
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(Value::as_str)
            .any(|s| s.to_ascii_lowercase().starts_with(prefix)),
        _ => false,
    }
}

fn extract_opds2_authors(meta: &Value) -> Vec<String> {
    let mut out = Vec::new();
    match meta.get("author") {
        Some(Value::String(s)) if !s.trim().is_empty() => out.push(s.trim().to_string()),
        Some(Value::Object(map)) => {
            if let Some(name) = map.get("name").and_then(Value::as_str)
                && !name.trim().is_empty()
            {
                out.push(name.trim().to_string());
            }
        }
        Some(Value::Array(arr)) => {
            for item in arr {
                match item {
                    Value::String(s) if !s.trim().is_empty() => out.push(s.trim().to_string()),
                    Value::Object(map) => {
                        if let Some(name) = map.get("name").and_then(Value::as_str)
                            && !name.trim().is_empty()
                        {
                            out.push(name.trim().to_string());
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
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let id = meta
        .get("identifier")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let authors = extract_opds2_authors(meta);
    let summary = meta
        .get("description")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let updated = meta
        .get("modified")
        .or_else(|| meta.get("published"))
        .and_then(Value::as_str)
        .map(str::to_string);

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
            let media_type = link
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let is_acq = link_rel_starts_with(link, "http://opds-spec.org/acquisition")
                || detect_format_from_mime_or_href(&media_type, raw_href).is_some();
            if !is_acq {
                continue;
            }
            let abs_href = resolve_link_url(base_url, raw_href);
            let format = detect_format_from_mime_or_href(&media_type, &abs_href)
                .unwrap_or_else(|| "file".to_string());
            downloads.push(OpdsDownloadLinkDto {
                href: abs_href,
                title: link
                    .get("title")
                    .and_then(Value::as_str)
                    .map(str::to_string),
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
        .and_then(Value::as_str)
        .unwrap_or("OPDS 2.0 Catalog")
        .to_string();

    let mut search_url: Option<String> = None;
    let mut next_url: Option<String> = None;
    if let Some(links) = root.get("links").and_then(Value::as_array) {
        for link in links {
            let Some(href) = link.get("href").and_then(Value::as_str) else {
                continue;
            };
            if link_has_rel(link, "next") && next_url.is_none() {
                next_url = Some(resolve_link_url(base_url, href));
            } else if link_has_rel(link, "search") && search_url.is_none() {
                search_url = Some(resolve_link_url(base_url, href));
            }
        }
    }

    let mut nav_links = Vec::new();
    let collect_nav = |items: &[Value], out: &mut Vec<OpdsNavLinkDto>| {
        for item in items {
            let Some(href) = item.get("href").and_then(Value::as_str) else {
                continue;
            };
            let abs = resolve_link_url(base_url, href);
            let nav_title = item
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or(&abs)
                .to_string();
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
        collect_nav(nav, &mut nav_links);
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
            if let Some(nav) = group.get("navigation").and_then(Value::as_array) {
                collect_nav(nav, &mut nav_links);
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
<feed xmlns="http://www.w3.org/2005/Atom" xmlns:opds="http://opds-spec.org/2010/catalog">
  <title>Test Books</title>
  <link rel="search" href="/opds/book/v1.2/opensearch.xml" type="application/opensearchdescription+xml"/>
  <link rel="next" href="/opds/book/v1.2/all?page=2"/>
  <entry>
    <id>nav-1</id>
    <title>All Books</title>
    <link rel="subsection" href="/opds/book/v1.2/all" type="application/atom+xml;profile=opds-catalog;kind=acquisition"/>
  </entry>
  <entry>
    <id>urn:uuid:book-1</id>
    <title>Foundation</title>
    <updated>2026-01-01T00:00:00Z</updated>
    <author><name>Isaac Asimov</name></author>
    <summary>A classic sci-fi novel.</summary>
    <link rel="http://opds-spec.org/image/thumbnail" href="/opds/book/books/1/cover" type="image/jpeg"/>
    <link rel="http://opds-spec.org/acquisition" href="/opds/book/books/1/download" type="application/epub+zip"/>
  </entry>
</feed>"#;

        let (feed, is_opensearch) =
            parse_opds_feed("src-1", &base, "application/atom+xml", xml).unwrap();
        assert_eq!(feed.opds_version, "1.2");
        assert!(is_opensearch);
        assert_eq!(feed.title, "Test Books");
        assert_eq!(
            feed.next_url.as_deref(),
            Some("http://example.com/opds/book/v1.2/all?page=2")
        );
        assert_eq!(feed.nav_links.len(), 1);
        assert_eq!(feed.nav_links[0].title, "All Books");
        assert_eq!(feed.entries.len(), 1);
        assert_eq!(feed.entries[0].title, "Foundation");
        assert_eq!(feed.entries[0].authors, vec!["Isaac Asimov"]);
        assert_eq!(feed.entries[0].downloads[0].format, "epub");
        assert!(
            feed.entries[0]
                .cover_url
                .as_deref()
                .unwrap()
                .starts_with("/_app/opds/src-1/proxy-image?url=")
        );
    }

    #[test]
    fn expands_opensearch_and_opds2_templates() {
        assert_eq!(
            expand_search_template("http://h/search?q={searchTerms}", "sci fi"),
            "http://h/search?q=sci+fi"
        );
        assert_eq!(
            expand_search_template("http://h/v2/search{?query}", "Foundation"),
            "http://h/v2/search?query=Foundation"
        );
    }
}

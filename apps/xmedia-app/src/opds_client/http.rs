use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Instant;

use axum::body::Body;
use axum::http::header::{
    CACHE_CONTROL, CONTENT_LENGTH, CONTENT_SECURITY_POLICY, CONTENT_TYPE, X_CONTENT_TYPE_OPTIONS,
};
use axum::http::{HeaderValue, StatusCode};
use axum::response::Response;
use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use serde_json::Value;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use super::helpers::{
    append_query_param, expand_search_template, resolve_import_filename_and_format,
    resolve_link_url, resolve_target_url,
};
use super::parser::{parse_opds_feed, parse_opensearch_template};
use super::{
    MAX_IMAGE_BYTES, MAX_IMPORT_BYTES, MAX_METADATA_BYTES, MAX_REDIRECTS, OpdsClient, OpdsError,
    OpdsFeedDto, OpdsTestResult, REQUEST_TIMEOUT, RemoteImportResultDto, TEST_TIMEOUT,
};
use crate::remote_sources::{RemoteSource, is_same_origin, normalize_remote_url};

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

impl OpdsClient {
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
        let _import_permit = self
            .import_semaphore
            .acquire()
            .await
            .map_err(|_| OpdsError::Internal("Import concurrency limiter closed".into()))?;

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
                "Remote file exceeds 511MB import limit ({len} bytes)"
            )));
        }

        let final_url = resp.url().clone();
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        if is_disallowed_import_content_type(&content_type) {
            return Err(OpdsError::Unsupported(format!(
                "Remote OPDS download returned disallowed Content-Type '{content_type}'"
            )));
        }
        let content_disposition = resp
            .headers()
            .get(reqwest::header::CONTENT_DISPOSITION)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);

        // Validate format and destination route before streaming potentially large payloads to disk.
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

        let temp_guard =
            stream_response_to_temp_file(resp, MAX_IMPORT_BYTES, &self.temp_dir).await?;
        if temp_guard.size == 0 {
            return Err(OpdsError::Upstream(
                "Remote OPDS download returned an empty file".into(),
            ));
        }

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
            .local_upload
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

fn is_disallowed_import_content_type(content_type: &str) -> bool {
    let essence = content_type
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

async fn create_private_temp_file(path: &Path) -> std::io::Result<tokio::fs::File> {
    let mut opts = tokio::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        opts.mode(0o600);
    }
    opts.open(path).await
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
    // Declare RAII guard BEFORE opening the file handle so `file` drops first (Windows unlink safety).
    let mut guard = TempDownloadFile {
        path: path.clone(),
        size: 0,
    };
    let mut file = create_private_temp_file(&path)
        .await
        .map_err(|e| OpdsError::Internal(format!("Failed to create temp file: {e}")))?;

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

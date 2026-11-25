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

use super::helpers::{
    append_query_param, expand_search_template, resolve_import_filename_and_format,
    resolve_link_url, resolve_target_url,
};
use super::parser::{parse_opds_feed, parse_opensearch_template};
use super::{
    MAX_IMAGE_BYTES, MAX_IMPORT_BYTES, MAX_METADATA_BYTES, MAX_REDIRECTS, OpdsClient, OpdsError,
    OpdsFeedDto, OpdsTestResult, REQUEST_TIMEOUT, RemoteImportResultDto, TEST_TIMEOUT,
};
use crate::remote_common::{stream_response_to_temp_file, upload_temp_file_to_local};
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
        "image/jpeg" | "image/png" | "image/webp" | "image/gif" | "image/avif" | "image/bmp"
    )
}

pub(crate) fn is_disallowed_non_raster_header(content_type: &str) -> bool {
    let essence = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    matches!(
        essence.as_str(),
        "image/svg+xml"
            | "text/html"
            | "application/xhtml+xml"
            | "text/xml"
            | "application/xml"
            | "application/javascript"
            | "text/javascript"
    )
}

#[derive(Debug)]
pub(crate) enum SniffImageError {
    TooLarge(String),
    Unsupported(String),
    Upstream(String),
}

const IMAGE_MAGIC_PEEK_BYTES: usize = 32;

pub(crate) async fn build_sniffed_raster_image_response(
    resp: reqwest::Response,
    max_bytes: u64,
    label: &str,
) -> Result<Response, SniffImageError> {
    if let Some(len) = resp.content_length()
        && len > max_bytes
    {
        return Err(SniffImageError::TooLarge(format!(
            "{label} image exceeds maximum allowed size of {max_bytes} bytes"
        )));
    }

    let content_length_header = resp
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| HeaderValue::from_str(s).ok());

    let mut stream = resp.bytes_stream();
    let mut prefix = Vec::with_capacity(IMAGE_MAGIC_PEEK_BYTES);
    while prefix.len() < IMAGE_MAGIC_PEEK_BYTES {
        let Some(chunk_res) = stream.next().await else {
            break;
        };
        let chunk = chunk_res.map_err(|e| {
            SniffImageError::Upstream(format!(
                "Failed reading {label} image bytes: {}",
                e.without_url()
            ))
        })?;
        if (prefix.len() as u64).saturating_add(chunk.len() as u64) > max_bytes {
            return Err(SniffImageError::TooLarge(format!(
                "{label} image exceeds maximum allowed size of {max_bytes} bytes"
            )));
        }
        prefix.extend_from_slice(&chunk);
    }

    let sniffed_mime = xmedia_core::infra::thumbnail::sniff_raster_image_mime(&prefix)
        .filter(|mime| is_safe_raster_image_content_type(mime))
        .ok_or_else(|| {
            SniffImageError::Unsupported(format!(
                "{label} image payload failed raster magic-byte verification"
            ))
        })?;

    let chained = futures_util::stream::once(async move { Ok(Bytes::from(prefix)) }).chain(stream);
    let limited = LimitedByteStream::new(chained, max_bytes);

    let mut response = Response::new(Body::from_stream(limited));
    *response.status_mut() = StatusCode::OK;
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static(sniffed_mime));
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

        if is_disallowed_non_raster_header(&content_type)
            || (!content_type.is_empty()
                && !content_type
                    .to_ascii_lowercase()
                    .starts_with("application/octet-stream")
                && !content_type.to_ascii_lowercase().starts_with("image/jpg")
                && !is_safe_raster_image_content_type(&content_type))
        {
            return Err(OpdsError::Unsupported(format!(
                "Remote image returned disallowed Content-Type '{content_type}'"
            )));
        }

        build_sniffed_raster_image_response(resp, MAX_IMAGE_BYTES, "Remote OPDS")
            .await
            .map_err(|err| match err {
                SniffImageError::TooLarge(m) => OpdsError::TooLarge(m),
                SniffImageError::Unsupported(m) => OpdsError::Unsupported(m),
                SniffImageError::Upstream(m) => OpdsError::Upstream(m),
            })
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

        let temp_guard = stream_response_to_temp_file(
            resp,
            MAX_IMPORT_BYTES,
            &self.temp_dir,
            "xmedia-opds-import",
            "OPDS download",
        )
        .await?;

        let resp_json = upload_temp_file_to_local(
            &self.local_upload,
            &temp_guard,
            &filename,
            &upload_endpoint,
            local_token,
            "OPDS",
        )
        .await?;

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

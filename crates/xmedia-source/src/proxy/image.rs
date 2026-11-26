use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, Weak};
use std::time::{Duration, Instant};

use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderValue, RETRY_AFTER, USER_AGENT};
use tokio::sync::Semaphore;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::infra::thumbnail::sniff_raster_image_mime;

use crate::http::proxy::filter_headers_for_target_origin;
use crate::http::{
    DEFAULT_WEBVIEW_UA, HttpClientConfig, build_sanitized_headers, get_or_build_async_client,
    has_ssrf_io_error_in_chain, validate_target_url_async,
};

pub const DEFAULT_IMAGE_PROXY_MAX_BYTES: usize = 8 * 1024 * 1024;
pub const DEFAULT_IMAGE_PROXY_TIMEOUT_SECS: u64 = 20;
pub const IMAGE_PROXY_CSP: &str = "sandbox; default-src 'none'; img-src 'self' data:";
pub const IMAGE_PROXY_CACHE_CONTROL: &str = "private, max-age=86400";

const IMAGE_PROXY_MAX_CONCURRENT_PER_HOST: usize = 6;
const IMAGE_PROXY_MAX_ATTEMPTS: usize = 2;
const IMAGE_PROXY_DEFAULT_RETRY_DELAY: Duration = Duration::from_millis(300);
const IMAGE_PROXY_MIN_RETRY_DELAY: Duration = Duration::from_millis(250);
const IMAGE_PROXY_MAX_RETRY_DELAY: Duration = Duration::from_secs(2);
const IMAGE_PROXY_RETRY_BUDGET: Duration = Duration::from_secs(14);

static HOST_SEMAPHORES: LazyLock<Mutex<HashMap<String, Weak<Semaphore>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn host_semaphore(host: &str) -> Arc<Semaphore> {
    let key = host.trim().to_ascii_lowercase();
    let mut guard = HOST_SEMAPHORES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(existing) = guard.get(&key).and_then(Weak::upgrade) {
        return existing;
    }
    if guard.len() >= 128 {
        guard.retain(|_, w| w.strong_count() > 0);
    }
    let sem = Arc::new(Semaphore::new(IMAGE_PROXY_MAX_CONCURRENT_PER_HOST));
    guard.insert(key, Arc::downgrade(&sem));
    sem
}

fn is_transient_reqwest_error(err: &reqwest::Error) -> bool {
    if err.is_redirect() || err.is_builder() {
        return false;
    }
    if has_ssrf_io_error_in_chain(err) {
        return false;
    }
    err.is_timeout() || err.is_connect() || err.is_body() || err.is_decode() || err.is_request()
}

fn is_transient_image_status(status: reqwest::StatusCode) -> bool {
    matches!(status.as_u16(), 408 | 429 | 500 | 502 | 503 | 504)
}

fn parse_retry_after_delay(headers: &HeaderMap) -> Option<Duration> {
    let raw = headers.get(RETRY_AFTER)?.to_str().ok()?.trim();
    let secs = raw.parse::<u64>().ok()?;
    Some(Duration::from_secs(secs).clamp(IMAGE_PROXY_MIN_RETRY_DELAY, IMAGE_PROXY_MAX_RETRY_DELAY))
}

/// Validates that upstream `Content-Type` (lowercased, without parameters) is either empty,
/// generic octet-stream, or a non-SVG `image/*` type.
pub fn validate_image_header_mime(ct_lower: &str) -> DomainResult<()> {
    if ct_lower.is_empty()
        || ct_lower == "application/octet-stream"
        || ct_lower == "binary/octet-stream"
    {
        return Ok(());
    }
    if !ct_lower.starts_with("image/") || ct_lower.contains("svg") {
        return Err(DomainError::Validation(format!(
            "不支持的图片内容类型: {ct_lower}"
        )));
    }
    Ok(())
}

/// Validates optional `Content-Type` header and sniffs magic bytes via
/// `xmedia_core::infra::thumbnail::sniff_raster_image_mime`.
/// Rejects HTML, XML, SVG, empty, or unknown binary payloads.
pub fn validate_safe_raster_image(
    header_ct: Option<&str>,
    bytes: &[u8],
) -> DomainResult<&'static str> {
    if let Some(ct) = header_ct {
        let normalized = ct
            .split(';')
            .next()
            .unwrap_or(ct)
            .trim()
            .to_ascii_lowercase();
        validate_image_header_mime(&normalized)?;
    }
    sniff_raster_image_mime(bytes)
        .ok_or_else(|| DomainError::Validation("图片内容不是受支持的安全格式".into()))
}

enum ImageAttemptOutcome {
    Done((Vec<u8>, &'static str)),
    Retry(Duration),
}

async fn attempt_fetch_validated_image_bytes(
    client: &reqwest::Client,
    validated: reqwest::Url,
    header_map: HeaderMap,
    max_bytes: usize,
    can_retry: bool,
    started: Instant,
) -> DomainResult<ImageAttemptOutcome> {
    let mut resp = match client.get(validated).headers(header_map).send().await {
        Ok(r) => r,
        Err(e) => {
            if can_retry
                && is_transient_reqwest_error(&e)
                && started.elapsed() < IMAGE_PROXY_RETRY_BUDGET
            {
                return Ok(ImageAttemptOutcome::Retry(IMAGE_PROXY_DEFAULT_RETRY_DELAY));
            }
            return Err(DomainError::Validation(format!("拉取图片失败: {e}")));
        }
    };

    if !resp.status().is_success() {
        let status = resp.status();
        if can_retry
            && is_transient_image_status(status)
            && started.elapsed() < IMAGE_PROXY_RETRY_BUDGET
        {
            let delay =
                parse_retry_after_delay(resp.headers()).unwrap_or(IMAGE_PROXY_DEFAULT_RETRY_DELAY);
            return Ok(ImageAttemptOutcome::Retry(delay));
        }
        return Err(DomainError::Validation(format!(
            "图片响应状态异常: {status}"
        )));
    }

    let header_ct = resp
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(';').next().unwrap_or(s).trim().to_ascii_lowercase());

    if let Some(ref ct) = header_ct {
        validate_image_header_mime(ct)?;
    }

    if let Some(content_len) = resp.content_length()
        && content_len > max_bytes as u64
    {
        return Err(DomainError::Validation(format!(
            "图片体积超过 {} 字节限制",
            max_bytes
        )));
    }

    let mut bytes = Vec::new();
    loop {
        match resp.chunk().await {
            Ok(Some(chunk)) => {
                if bytes.len().saturating_add(chunk.len()) > max_bytes {
                    return Err(DomainError::Validation(format!(
                        "图片体积超过 {} 字节限制",
                        max_bytes
                    )));
                }
                bytes.extend_from_slice(&chunk);
            }
            Ok(None) => break,
            Err(e) => {
                if can_retry
                    && is_transient_reqwest_error(&e)
                    && started.elapsed() < IMAGE_PROXY_RETRY_BUDGET
                {
                    return Ok(ImageAttemptOutcome::Retry(IMAGE_PROXY_DEFAULT_RETRY_DELAY));
                }
                return Err(DomainError::Validation(format!("读取图片失败: {e}")));
            }
        }
    }

    let safe_mime = validate_safe_raster_image(header_ct.as_deref(), &bytes)?;
    Ok(ImageAttemptOutcome::Done((bytes, safe_mime)))
}

/// Fetches and validates a raster image with an explicit `reqwest::Client`, enforcing
/// origin-affinity credential filtering whenever `source_url` is provided.
pub(super) async fn fetch_client_image(
    client: &reqwest::Client,
    source_url: Option<&str>,
    raw_url: &str,
    headers: Option<&HashMap<String, String>>,
    max_bytes: usize,
    allow_private_ips: bool,
) -> DomainResult<(Vec<u8>, &'static str)> {
    let validated = validate_target_url_async(raw_url.trim(), allow_private_ips)
        .await
        .map_err(DomainError::Validation)?;

    let mut header_map = headers.map(build_sanitized_headers).unwrap_or_default();
    if let Some(src) = source_url {
        header_map = filter_headers_for_target_origin(src, validated.as_str(), &header_map);
    }
    if !header_map.contains_key(USER_AGENT) {
        header_map.insert(USER_AGENT, HeaderValue::from_static(DEFAULT_WEBVIEW_UA));
    }

    let sem = host_semaphore(validated.host_str().unwrap_or("_unknown"));
    let started = Instant::now();

    for attempt in 1..=IMAGE_PROXY_MAX_ATTEMPTS {
        let can_retry = attempt < IMAGE_PROXY_MAX_ATTEMPTS;
        let outcome = {
            let _permit = sem
                .acquire()
                .await
                .map_err(|_| DomainError::Storage("图片代理并发信号量已关闭".into()))?;
            attempt_fetch_validated_image_bytes(
                client,
                validated.clone(),
                header_map.clone(),
                max_bytes,
                can_retry,
                started,
            )
            .await?
        };
        match outcome {
            ImageAttemptOutcome::Done(result) => return Ok(result),
            ImageAttemptOutcome::Retry(delay) => {
                tokio::time::sleep(delay).await;
            }
        }
    }

    Err(DomainError::Validation(
        "拉取图片失败: 重试次数已耗尽".into(),
    ))
}

pub async fn fetch_image(
    raw_url: &str,
    headers: Option<&HashMap<String, String>>,
    max_bytes: usize,
    config: &HttpClientConfig,
) -> DomainResult<(Vec<u8>, &'static str)> {
    fetch_source_image(None, raw_url, headers, max_bytes, config).await
}

pub async fn fetch_source_image(
    source_url: Option<&str>,
    raw_url: &str,
    headers: Option<&HashMap<String, String>>,
    max_bytes: usize,
    config: &HttpClientConfig,
) -> DomainResult<(Vec<u8>, &'static str)> {
    let client = get_or_build_async_client(
        config.timeout_secs,
        config.allow_private_ips,
        config.proxy_url.as_deref(),
    )
    .map_err(DomainError::Storage)?;
    fetch_client_image(
        &client,
        source_url,
        raw_url,
        headers,
        max_bytes,
        config.allow_private_ips,
    )
    .await
}

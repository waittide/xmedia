use std::collections::HashMap;
use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use encoding_rs::Encoding;
use regex::Regex;
use reqwest::header::{COOKIE, HeaderMap, HeaderName, HeaderValue, SET_COOKIE, USER_AGENT};

use super::ssrf::{
    SsrfSafeResolver, has_ssrf_io_error_in_chain, ssrf_blocking_redirect_policy,
    validate_target_url, validate_target_url_static,
};
use super::url_cookie::{
    HttpClientConfig, HttpFetchResult, merge_cookie_string, normalize_cookie_key, parse_legado_url,
    resolve_relative_url, try_resolve_data_url,
};
use super::{
    DEFAULT_WEBVIEW_UA, MAX_COOKIE_ENTRIES, MAX_COOKIE_TOTAL_BYTES, MAX_REDIRECT_HOPS,
    MAX_RESPONSE_BODY_BYTES,
};

fn is_timeout_error_chain(err: &std::io::Error) -> bool {
    if err.kind() == std::io::ErrorKind::TimedOut {
        return true;
    }
    let mut stack: Vec<&(dyn std::error::Error + 'static)> = Vec::new();
    if let Some(inner) = err.get_ref() {
        stack.push(inner);
    }
    if let Some(src) = std::error::Error::source(err) {
        stack.push(src);
    }
    let mut visited = 0usize;
    while let Some(cur) = stack.pop() {
        visited += 1;
        if visited > 32 {
            break;
        }
        if let Some(req_err) = cur.downcast_ref::<reqwest::Error>()
            && req_err.is_timeout()
        {
            return true;
        }
        if let Some(io_err) = cur.downcast_ref::<std::io::Error>() {
            if io_err.kind() == std::io::ErrorKind::TimedOut {
                return true;
            }
            if let Some(inner) = io_err.get_ref() {
                stack.push(inner);
            }
        }
        if let Some(src) = cur.source() {
            stack.push(src);
        }
    }
    false
}

pub fn is_forbidden_custom_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "host"
            | "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
            | "content-length"
    )
}

pub fn build_sanitized_headers(headers: &HashMap<String, String>) -> HeaderMap {
    let mut header_map = HeaderMap::new();
    for (k, v) in headers {
        if is_forbidden_custom_header(k) {
            continue;
        }
        if let (Ok(hn), Ok(hv)) = (
            HeaderName::from_bytes(k.as_bytes()),
            HeaderValue::from_str(v),
        ) {
            header_map.insert(hn, hv);
        }
    }
    header_map
}

pub fn decode_response_bytes(
    bytes: &[u8],
    explicit_charset: Option<&str>,
    content_type_header: Option<&str>,
) -> String {
    if let Some(cs) = explicit_charset.and_then(|s| Encoding::for_label(s.trim().as_bytes())) {
        let (cow, _, _) = cs.decode(bytes);
        return cow.into_owned();
    }

    if let Some(ct) = content_type_header
        && let Some(cs_name) = extract_charset_from_content_type(ct)
        && let Some(enc) = Encoding::for_label(cs_name.as_bytes())
    {
        let (cow, _, _) = enc.decode(bytes);
        return cow.into_owned();
    }

    // Sniff HTML <meta ... charset=...> in first 2 KiB only for absent or HTML/XML Content-Type
    if should_sniff_html_meta(content_type_header) {
        let sniff_len = bytes.len().min(2048);
        let head_ascii = String::from_utf8_lossy(&bytes[..sniff_len]).to_ascii_lowercase();
        if let Some(meta_cs) = extract_meta_charset(&head_ascii)
            && let Some(enc) = Encoding::for_label(meta_cs.as_bytes())
        {
            let (cow, _, _) = enc.decode(bytes);
            return cow.into_owned();
        }
    }

    if let Ok(utf8) = std::str::from_utf8(bytes) {
        return utf8.to_string();
    }

    let mut detector = chardetng::EncodingDetector::new(chardetng::Iso2022JpDetection::Allow);
    detector.feed(bytes, true);
    let enc = detector.guess(None, chardetng::Utf8Detection::Allow);
    let (cow, _, _) = enc.decode(bytes);
    cow.into_owned()
}

fn extract_charset_from_content_type(ct: &str) -> Option<String> {
    for part in ct.split(';') {
        let trimmed = part.trim();
        if let Some((k, v)) = trimmed.split_once('=')
            && k.trim().eq_ignore_ascii_case("charset")
        {
            let clean = v.trim().trim_matches(|c| c == '"' || c == '\'');
            if !clean.is_empty() {
                return Some(clean.to_string());
            }
        }
    }
    None
}

static META_CHARSET_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<meta(?:\s[^>]*?)?[\s;]charset\s*=\s*['"]?\s*([a-z0-9_-]+)"#)
        .expect("valid meta charset regex")
});

fn should_sniff_html_meta(content_type_header: Option<&str>) -> bool {
    let Some(ct) = content_type_header else {
        return true;
    };
    let mime = ct
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if mime.is_empty() {
        return true;
    }
    matches!(
        mime.as_str(),
        "text/html" | "application/xhtml+xml" | "application/xml" | "text/xml"
    )
}

fn extract_meta_charset(head_lower: &str) -> Option<String> {
    for caps in META_CHARSET_RE.captures_iter(head_lower) {
        if let Some(m) = caps.get(1) {
            let label = m.as_str().trim();
            if !label.is_empty() && Encoding::for_label(label.as_bytes()).is_some() {
                return Some(label.to_string());
            }
        }
    }
    None
}

const MAX_CACHED_CLIENTS: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ClientCacheKey {
    timeout_secs: u64,
    allow_private_ips: bool,
    proxy_url: Option<String>,
}

impl ClientCacheKey {
    fn from_config(config: &HttpClientConfig) -> Self {
        Self {
            timeout_secs: config.timeout_secs.max(3),
            allow_private_ips: config.allow_private_ips,
            proxy_url: config
                .proxy_url
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
        }
    }
}

static BLOCKING_CLIENT_CACHE: LazyLock<Mutex<HashMap<ClientCacheKey, reqwest::blocking::Client>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

static ASYNC_CLIENT_CACHE: LazyLock<Mutex<HashMap<ClientCacheKey, reqwest::Client>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Returns a pooled, cached `reqwest::blocking::Client` shared across calls with identical config.
pub fn get_or_build_blocking_client(
    config: &HttpClientConfig,
) -> Result<reqwest::blocking::Client, String> {
    let key = ClientCacheKey::from_config(config);
    {
        let guard = BLOCKING_CLIENT_CACHE
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(client) = guard.get(&key) {
            return Ok(client.clone());
        }
    }

    let has_proxy = key.proxy_url.is_some();
    let mut builder = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(key.timeout_secs))
        .connect_timeout(Duration::from_secs(key.timeout_secs.min(10)))
        .pool_max_idle_per_host(16)
        .pool_idle_timeout(Duration::from_secs(90))
        .redirect(ssrf_blocking_redirect_policy(
            key.allow_private_ips,
            has_proxy,
            MAX_REDIRECT_HOPS,
        ));
    if let Some(ref proxy) = key.proxy_url {
        let p = reqwest::Proxy::all(proxy.as_str()).map_err(|e| format!("代理配置无效: {e}"))?;
        builder = builder.proxy(p);
    } else {
        builder = builder.no_proxy();
        if !key.allow_private_ips {
            builder = builder.dns_resolver(Arc::new(SsrfSafeResolver));
        }
    }
    let client = builder
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {e}"))?;

    let mut guard = BLOCKING_CLIENT_CACHE
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if guard.len() >= MAX_CACHED_CLIENTS && !guard.contains_key(&key) {
        guard.clear();
    }
    Ok(guard.entry(key).or_insert(client).clone())
}

/// Returns a pooled, cached async `reqwest::Client` for cover proxy & async fetches.
pub fn get_or_build_async_client(
    timeout_secs: u64,
    allow_private_ips: bool,
    proxy_url: Option<&str>,
) -> Result<reqwest::Client, String> {
    let key = ClientCacheKey {
        timeout_secs: timeout_secs.max(3),
        allow_private_ips,
        proxy_url: proxy_url
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
    };
    {
        let guard = ASYNC_CLIENT_CACHE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(client) = guard.get(&key) {
            return Ok(client.clone());
        }
    }

    let has_proxy = key.proxy_url.is_some();
    let mut builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(key.timeout_secs))
        .connect_timeout(Duration::from_secs(key.timeout_secs.min(10)))
        .user_agent(DEFAULT_WEBVIEW_UA)
        .pool_max_idle_per_host(16)
        .pool_idle_timeout(Duration::from_secs(90))
        .redirect(ssrf_blocking_redirect_policy(
            key.allow_private_ips,
            has_proxy,
            MAX_REDIRECT_HOPS,
        ));
    if let Some(ref proxy) = key.proxy_url {
        let p = reqwest::Proxy::all(proxy.as_str()).map_err(|e| format!("代理配置无效: {e}"))?;
        builder = builder.proxy(p);
    } else {
        builder = builder.no_proxy();
        if !key.allow_private_ips {
            builder = builder.dns_resolver(Arc::new(SsrfSafeResolver));
        }
    }
    let client = builder
        .build()
        .map_err(|e| format!("创建异步 HTTP 客户端失败: {e}"))?;

    let mut guard = ASYNC_CLIENT_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if guard.len() >= MAX_CACHED_CLIENTS && !guard.contains_key(&key) {
        guard.clear();
    }
    Ok(guard.entry(key).or_insert(client).clone())
}

#[derive(Clone)]
pub struct LegadoHttpClient {
    client: reqwest::blocking::Client,
    config: HttpClientConfig,
    cookies: Arc<Mutex<HashMap<String, String>>>,
    default_headers: HashMap<String, String>,
    enabled_cookie_jar: bool,
}

impl LegadoHttpClient {
    pub fn new(
        config: HttpClientConfig,
        cookies: Arc<Mutex<HashMap<String, String>>>,
        default_header_json: Option<&str>,
        enabled_cookie_jar: bool,
    ) -> Result<Self, String> {
        let client = get_or_build_blocking_client(&config)?;

        let mut default_headers = HashMap::new();
        if let Some(raw) = default_header_json
            && let Ok(serde_json::Value::Object(map)) = serde_json::from_str(raw.trim())
        {
            for (k, v) in map {
                if let Some(s) = v.as_str() {
                    default_headers.insert(k, s.to_string());
                } else {
                    default_headers.insert(k, v.to_string());
                }
            }
        }

        Ok(Self {
            client,
            config,
            cookies,
            default_headers,
            enabled_cookie_jar,
        })
    }

    #[cfg(test)]
    pub(super) fn with_blocking_client_for_test(
        client: reqwest::blocking::Client,
        config: HttpClientConfig,
    ) -> Self {
        Self {
            client,
            config,
            cookies: Arc::new(Mutex::new(HashMap::new())),
            default_headers: HashMap::new(),
            enabled_cookie_jar: false,
        }
    }

    pub fn get_cookie_header(&self, url_or_domain: &str) -> String {
        let key = normalize_cookie_key(url_or_domain);
        if key.is_empty() {
            return String::new();
        }
        let guard = self.cookies.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(v) = guard.get(&key) {
            return v.clone();
        }
        // Only allow parent-domain cookies to match subdomains (never child -> parent),
        // picking the most specific (longest) matching suffix deterministically.
        let mut best: Option<(&str, &str)> = None;
        for (stored_host, val) in guard.iter() {
            let clean_stored = stored_host.trim_start_matches('.');
            if clean_stored.is_empty() {
                continue;
            }
            if key.len() > clean_stored.len()
                && key.ends_with(clean_stored)
                && key.as_bytes()[key.len() - clean_stored.len() - 1] == b'.'
            {
                let replace = match best {
                    None => true,
                    Some((prev_host, _)) => {
                        clean_stored.len() > prev_host.len()
                            || (clean_stored.len() == prev_host.len() && clean_stored < prev_host)
                    }
                };
                if replace {
                    best = Some((clean_stored, val.as_str()));
                }
            }
        }
        best.map(|(_, v)| v.to_string()).unwrap_or_default()
    }

    pub fn set_cookie(&self, url_or_domain: &str, cookie_val: &str) -> bool {
        let key = normalize_cookie_key(url_or_domain);
        if key.is_empty() {
            return false;
        }
        let mut guard = self.cookies.lock().unwrap_or_else(|e| e.into_inner());
        let existing = guard.get(&key).cloned().unwrap_or_default();
        let merged = merge_cookie_string(&existing, cookie_val);

        let is_new = !guard.contains_key(&key);
        if is_new && guard.len() >= MAX_COOKIE_ENTRIES {
            return false;
        }
        let current_bytes: usize = guard.iter().map(|(k, v)| k.len() + v.len()).sum();
        let old_entry_bytes = if is_new {
            0
        } else {
            key.len() + existing.len()
        };
        let new_total = current_bytes
            .saturating_sub(old_entry_bytes)
            .saturating_add(key.len())
            .saturating_add(merged.len());
        if new_total > MAX_COOKIE_TOTAL_BYTES {
            return false;
        }

        guard.insert(key, merged);
        true
    }

    pub fn remove_cookie(&self, url_or_domain: &str) {
        let key = normalize_cookie_key(url_or_domain);
        let mut guard = self.cookies.lock().unwrap_or_else(|e| e.into_inner());
        guard.remove(&key);
    }

    pub fn fetch(&self, raw_url: &str, base_url: &str) -> Result<HttpFetchResult, String> {
        self.fetch_with_timeout(raw_url, base_url, None)
    }

    pub fn fetch_with_timeout(
        &self,
        raw_url: &str,
        base_url: &str,
        timeout_override: Option<Duration>,
    ) -> Result<HttpFetchResult, String> {
        self.fetch_once(raw_url, base_url, timeout_override)
            .map_err(|(msg, _)| msg)
    }

    pub fn fetch_with_timeout_resilient(
        &self,
        raw_url: &str,
        base_url: &str,
        timeout_override: Option<Duration>,
        max_retries: usize,
        cancel_flag: Option<&AtomicBool>,
    ) -> Result<HttpFetchResult, String> {
        let parsed = parse_legado_url(raw_url);
        let method_str = parsed
            .options
            .method
            .as_deref()
            .unwrap_or("GET")
            .trim()
            .to_ascii_uppercase();
        let is_idempotent = matches!(method_str.as_str(), "GET" | "HEAD")
            && !parsed.url.trim().starts_with("data:");
        let allowed_retries = if is_idempotent { max_retries } else { 0 };
        let start = Instant::now();

        let mut attempt = 0usize;
        loop {
            if cancel_flag.is_some_and(|f| f.load(Ordering::Relaxed)) {
                return Err("执行已取消 (interrupted)".into());
            }
            let remaining_override = timeout_override.map(|t| t.saturating_sub(start.elapsed()));
            if let Some(rem) = remaining_override
                && rem.is_zero()
            {
                return Err("执行超过单次操作总超时上限 (interrupted)".into());
            }

            match self.fetch_once(raw_url, base_url, remaining_override) {
                Ok(res) => {
                    let transient_status = matches!(res.status, 408 | 429 | 500 | 502 | 503 | 504);
                    if transient_status && attempt < allowed_retries {
                        let post_remaining =
                            timeout_override.map(|t| t.saturating_sub(start.elapsed()));
                        let has_budget =
                            post_remaining.is_none_or(|rem| rem >= Duration::from_secs(4));
                        if has_budget {
                            attempt += 1;
                            if !sleep_with_cancel(Duration::from_millis(300), cancel_flag) {
                                return Err("执行已取消 (interrupted)".into());
                            }
                            continue;
                        }
                    }
                    return Ok(res);
                }
                Err((msg, retriable)) => {
                    if retriable && attempt < allowed_retries {
                        let post_remaining =
                            timeout_override.map(|t| t.saturating_sub(start.elapsed()));
                        let has_budget =
                            post_remaining.is_none_or(|rem| rem >= Duration::from_secs(4));
                        if has_budget {
                            attempt += 1;
                            if !sleep_with_cancel(Duration::from_millis(300), cancel_flag) {
                                return Err("执行已取消 (interrupted)".into());
                            }
                            continue;
                        }
                    }
                    return Err(msg);
                }
            }
        }
    }

    fn fetch_once(
        &self,
        raw_url: &str,
        base_url: &str,
        timeout_override: Option<Duration>,
    ) -> Result<HttpFetchResult, (String, bool)> {
        let parsed = parse_legado_url(raw_url);
        if let Some(res) = try_resolve_data_url(&parsed) {
            return res.map_err(|m| (m, false));
        }

        let resolved_url = resolve_relative_url(&parsed.url, base_url);
        let has_proxy = self
            .config
            .proxy_url
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty());
        // When no proxy is configured, static validation runs upfront and DNS resolution & IP
        // checks execute once inside `SsrfSafeResolver` to prevent DNS rebinding TOCTOU.
        // When `proxy_url` is configured, `reqwest` delegates connection setup to the proxy, so
        // full DNS-resolving SSRF validation must run upfront before dispatching to the proxy.
        let validated = if has_proxy {
            validate_target_url(&resolved_url, self.config.allow_private_ips)
        } else {
            validate_target_url_static(&resolved_url, self.config.allow_private_ips)
        }
        .map_err(|m| (m, false))?;

        let method_str = parsed
            .options
            .method
            .as_deref()
            .unwrap_or("GET")
            .trim()
            .to_ascii_uppercase();
        let method =
            reqwest::Method::from_bytes(method_str.as_bytes()).unwrap_or(reqwest::Method::GET);

        let mut header_map = build_sanitized_headers(&self.default_headers);
        let mut has_ua = header_map.contains_key(USER_AGENT);
        let mut has_cookie = header_map.contains_key(COOKIE);

        if let Some(serde_json::Value::Object(map)) = &parsed.options.headers {
            for (k, v) in map {
                if is_forbidden_custom_header(k) {
                    continue;
                }
                let val_str = match v {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                if let (Ok(hn), Ok(hv)) = (
                    HeaderName::from_bytes(k.as_bytes()),
                    HeaderValue::from_str(&val_str),
                ) {
                    if hn == USER_AGENT {
                        has_ua = true;
                    }
                    if hn == COOKIE {
                        has_cookie = true;
                    }
                    header_map.insert(hn, hv);
                }
            }
        }

        if !has_ua {
            header_map.insert(USER_AGENT, HeaderValue::from_static(DEFAULT_WEBVIEW_UA));
        }

        if self.enabled_cookie_jar && !has_cookie {
            let ck = self.get_cookie_header(validated.as_str());
            if !ck.is_empty()
                && let Ok(hv) = HeaderValue::from_str(&ck)
            {
                header_map.insert(COOKIE, hv);
            }
        }

        let mut req = self
            .client
            .request(method.clone(), validated.clone())
            .headers(header_map);
        if let Some(override_timeout) = timeout_override {
            let clamped = override_timeout
                .min(Duration::from_secs(self.config.timeout_secs.max(1)))
                .max(Duration::from_millis(100));
            req = req.timeout(clamped);
        }

        if method != reqwest::Method::GET
            && method != reqwest::Method::HEAD
            && let Some(ref body_val) = parsed.options.body
        {
            match body_val {
                serde_json::Value::String(s) => {
                    req = req.body(s.clone());
                }
                serde_json::Value::Null => {}
                other => {
                    req = req.body(other.to_string());
                }
            }
        }

        let mut resp = req.send().map_err(|e| {
            let retriable = is_transient_blocking_send_error(&e);
            let msg = if e.is_timeout() {
                format!("请求 '{resolved_url}' 超时: {e}")
            } else {
                format!("请求 '{resolved_url}' 失败: {e}")
            };
            (msg, retriable)
        })?;

        let final_url = resp.url().to_string();
        let status = resp.status().as_u16();
        let ct_header = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);

        let mut resp_headers = HashMap::new();
        for (k, v) in resp.headers() {
            if let Ok(s) = v.to_str() {
                if self.enabled_cookie_jar && k == SET_COOKIE {
                    if let Some((nv, _)) = s.split_once(';') {
                        self.set_cookie(&final_url, nv);
                    } else {
                        self.set_cookie(&final_url, s);
                    }
                }
                resp_headers.insert(k.as_str().to_ascii_lowercase(), s.to_string());
            }
        }

        if let Some(content_len) = resp.content_length()
            && content_len > MAX_RESPONSE_BODY_BYTES as u64
        {
            return Err((
                format!(
                    "响应体超过最大限制 ({}MB): '{resolved_url}'",
                    MAX_RESPONSE_BODY_BYTES / (1024 * 1024)
                ),
                false,
            ));
        }

        let mut raw_bytes = Vec::new();
        resp.by_ref()
            .take((MAX_RESPONSE_BODY_BYTES as u64) + 1)
            .read_to_end(&mut raw_bytes)
            .map_err(|e| {
                let retriable = !has_ssrf_io_error_in_chain(&e);
                let msg = if is_timeout_error_chain(&e) {
                    format!("读取响应体超时 '{resolved_url}': {e}")
                } else {
                    format!("读取响应体失败 '{resolved_url}': {e}")
                };
                (msg, retriable)
            })?;
        if raw_bytes.len() > MAX_RESPONSE_BODY_BYTES {
            return Err((
                format!(
                    "响应体超过最大限制 ({}MB): '{resolved_url}'",
                    MAX_RESPONSE_BODY_BYTES / (1024 * 1024)
                ),
                false,
            ));
        }

        let body = decode_response_bytes(
            &raw_bytes,
            parsed.options.charset.as_deref(),
            ct_header.as_deref(),
        );

        Ok(HttpFetchResult {
            final_url,
            status,
            body,
            headers: resp_headers,
        })
    }
}

fn is_transient_blocking_send_error(err: &reqwest::Error) -> bool {
    if err.is_redirect() || err.is_builder() {
        return false;
    }
    if has_ssrf_io_error_in_chain(err) {
        return false;
    }
    err.is_timeout() || err.is_connect() || err.is_body() || err.is_request()
}

fn sleep_with_cancel(total: Duration, cancel_flag: Option<&AtomicBool>) -> bool {
    let step = Duration::from_millis(50);
    let mut elapsed = Duration::ZERO;
    while elapsed < total {
        if cancel_flag.is_some_and(|f| f.load(Ordering::Relaxed)) {
            return false;
        }
        let d = step.min(total - elapsed);
        std::thread::sleep(d);
        elapsed += d;
    }
    !cancel_flag.is_some_and(|f| f.load(Ordering::Relaxed))
}

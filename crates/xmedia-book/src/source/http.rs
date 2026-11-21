use std::collections::HashMap;
use std::io::Read;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::{STANDARD as BASE64_STD, STANDARD_NO_PAD as BASE64_NO_PAD};
use encoding_rs::Encoding;
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::header::{COOKIE, HeaderMap, HeaderName, HeaderValue, SET_COOKIE, USER_AGENT};
use serde::{Deserialize, Serialize};
use url::Url;

pub const DEFAULT_WEBVIEW_UA: &str = "Mozilla/5.0 (Linux; Android 13; Pixel 7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Mobile Safari/537.36";
pub const MAX_RESPONSE_BODY_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_REDIRECT_HOPS: usize = 6;
pub const MAX_COOKIE_ENTRIES: usize = 256;
pub const MAX_COOKIE_TOTAL_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct LegadoUrlOptions {
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub headers: Option<serde_json::Value>,
    #[serde(default)]
    pub body: Option<serde_json::Value>,
    #[serde(default)]
    pub charset: Option<String>,
    #[serde(rename = "type", default)]
    pub content_type: Option<String>,
    #[serde(rename = "webView", default)]
    pub web_view: Option<serde_json::Value>,
}

#[derive(Debug, Clone)]
pub struct ParsedLegadoUrl {
    pub url: String,
    pub options: LegadoUrlOptions,
}

#[derive(Debug, Clone)]
pub struct HttpFetchResult {
    pub final_url: String,
    pub status: u16,
    pub body: String,
    pub headers: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct HttpClientConfig {
    pub timeout_secs: u64,
    pub allow_private_ips: bool,
    pub proxy_url: Option<String>,
}

impl Default for HttpClientConfig {
    fn default() -> Self {
        Self {
            timeout_secs: 12,
            allow_private_ips: false,
            proxy_url: None,
        }
    }
}

/// Parses Legado's `url,{...json options...}` convention safely even when URL contains commas.
pub fn parse_legado_url(raw: &str) -> ParsedLegadoUrl {
    let trimmed = raw.trim();
    if let Some((url_part, opt_json)) = split_url_and_options(trimmed)
        && let Ok(options) = serde_json::from_str::<LegadoUrlOptions>(opt_json)
    {
        return ParsedLegadoUrl {
            url: url_part.trim().to_string(),
            options,
        };
    }
    ParsedLegadoUrl {
        url: trimmed.to_string(),
        options: LegadoUrlOptions::default(),
    }
}

fn split_url_and_options(input: &str) -> Option<(&str, &str)> {
    if !input.ends_with('}') {
        return None;
    }
    // Walk backwards from the end matching balanced `{ ... }` respecting JSON strings.
    let bytes = input.as_bytes();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape = false;
    let mut brace_start = None;

    for (idx, &b) in bytes.iter().enumerate() {
        if in_string {
            if escape {
                escape = false;
            } else if b == b'\\' {
                escape = true;
            } else if b == b'"' {
                in_string = false;
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'{' => {
                if depth == 0 {
                    // Check if preceded by `,` (ignoring whitespace)
                    let before = input[..idx].trim_end();
                    if before.ends_with(',') {
                        brace_start = Some(idx);
                    }
                }
                depth += 1;
            }
            b'}' if depth > 0 => {
                depth -= 1;
                if depth == 0
                    && idx == bytes.len() - 1
                    && let Some(start) = brace_start
                {
                    let url_part = input[..start].trim_end().strip_suffix(',')?;
                    return Some((url_part, &input[start..]));
                }
            }
            _ => {}
        }
    }
    None
}

/// Handles Legado `data:` pseudo-URLs (e.g. `data:;base64,<b64>,{"type":"gysearch"}`).
///
/// Matches Legado `AnalyzeUrl`: when `options.content_type` is present and not `"image"`,
/// the decoded bytes are returned as lowercase hex so `java.hexDecodeToString(result)` works.
pub fn try_resolve_data_url(parsed: &ParsedLegadoUrl) -> Option<Result<HttpFetchResult, String>> {
    let rest = parsed.url.strip_prefix("data:")?;
    let Some((meta, payload)) = rest.split_once(',') else {
        return Some(Err("无效的 data: URL（缺少逗号分隔符）".into()));
    };
    let is_base64 = meta
        .split(';')
        .any(|seg| seg.trim().eq_ignore_ascii_case("base64"));

    let bytes = if is_base64 {
        let cleaned: String = payload.chars().filter(|c| !c.is_whitespace()).collect();
        BASE64_STD
            .decode(&cleaned)
            .or_else(|_| BASE64_NO_PAD.decode(cleaned.trim_end_matches('=')))
            .map_err(|e| format!("data URL base64 解码失败: {e}"))
    } else {
        Ok(percent_decode_bytes(payload.as_bytes()))
    };

    let raw_bytes = match bytes {
        Ok(b) => b,
        Err(e) => return Some(Err(e)),
    };

    let want_hex = parsed
        .options
        .content_type
        .as_deref()
        .is_some_and(|t| !t.trim().is_empty() && !t.trim().eq_ignore_ascii_case("image"));

    let body = if want_hex {
        hex::encode(&raw_bytes)
    } else {
        String::from_utf8_lossy(&raw_bytes).into_owned()
    };

    Some(Ok(HttpFetchResult {
        final_url: parsed.url.clone(),
        status: 200,
        body,
        headers: HashMap::new(),
    }))
}

pub fn resolve_relative_url(raw_url: &str, base_url: &str) -> String {
    let trimmed = raw_url.trim();
    if trimmed.is_empty()
        || trimmed.starts_with("http://")
        || trimmed.starts_with("https://")
        || trimmed.starts_with("data:")
    {
        return trimmed.to_string();
    }
    if let Ok(base) = Url::parse(base_url)
        && matches!(base.scheme(), "http" | "https")
        && let Ok(joined) = base.join(trimmed)
    {
        return joined.to_string();
    }
    trimmed.to_string()
}

pub fn normalize_cookie_key(raw_url: &str) -> String {
    let trimmed = raw_url.trim();
    if let Ok(u) = Url::parse(trimmed)
        && let Some(host) = u.host_str()
    {
        return host.trim_end_matches('.').to_ascii_lowercase();
    }
    trimmed
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or(trimmed)
        .split(':')
        .next()
        .unwrap_or(trimmed)
        .trim_end_matches('.')
        .to_ascii_lowercase()
}

pub fn merge_cookie_string(existing: &str, incoming: &str) -> String {
    let mut pairs: Vec<(String, String)> = Vec::new();
    let mut upsert = |raw: &str| {
        for part in raw.split(';') {
            let trimmed = part.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Some((k, v)) = trimmed.split_once('=') {
                let key = k.trim().to_string();
                let val = v.trim().to_string();
                if key.is_empty() {
                    continue;
                }
                if let Some(slot) = pairs.iter_mut().find(|(ek, _)| ek == &key) {
                    slot.1 = val;
                } else {
                    pairs.push((key, val));
                }
            }
        }
    };
    upsert(existing);
    upsert(incoming);
    pairs
        .into_iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("; ")
}

fn percent_decode_bytes(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        if input[i] == b'%'
            && i + 2 < input.len()
            && let (Some(hi), Some(lo)) = (hex_nibble(input[i + 1]), hex_nibble(input[i + 2]))
        {
            out.push((hi << 4) | lo);
            i += 3;
            continue;
        }
        out.push(input[i]);
        i += 1;
    }
    out
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Checks whether an IP address is private, loopback, link-local, transition, or otherwise non-public.
pub fn is_restricted_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_restricted_ipv4(v4),
        IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return is_restricted_ipv4(mapped);
            }
            let s = v6.segments();
            let is_ipv4_compatible =
                s[0] == 0 && s[1] == 0 && s[2] == 0 && s[3] == 0 && s[4] == 0 && s[5] == 0;
            let is_ipv4_translated =
                s[0] == 0 && s[1] == 0 && s[2] == 0 && s[3] == 0 && s[4] == 0xffff && s[5] == 0;
            let is_nat64_well_known = s[0] == 0x0064
                && s[1] == 0xff9b
                && s[2] == 0
                && s[3] == 0
                && s[4] == 0
                && s[5] == 0;
            let is_nat64_local = s[0] == 0x0064 && s[1] == 0xff9b && s[2] == 0x0001;
            let is_discard = s[0] == 0x0100 && s[1] == 0 && s[2] == 0 && s[3] == 0; // 100::/64
            let is_benchmarking = s[0] == 0x2001 && s[1] == 0x0002 && s[2] == 0; // 2001:2::/48
            let is_orchid = s[0] == 0x2001 && (0x0010..=0x002f).contains(&s[1]); // 2001:10::/28 & 2001:20::/28
            let is_6to4 = s[0] == 0x2002;
            let is_teredo = s[0] == 0x2001 && s[1] == 0x0000;
            let is_doc = s[0] == 0x2001 && s[1] == 0x0db8;
            let is_site_local = (s[0] & 0xffc0) == 0xfec0;
            let is_ula = (s[0] & 0xfe00) == 0xfc00;
            let is_link_local = (s[0] & 0xffc0) == 0xfe80;

            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || is_ipv4_compatible
                || is_ipv4_translated
                || is_nat64_well_known
                || is_nat64_local
                || is_discard
                || is_benchmarking
                || is_orchid
                || is_6to4
                || is_teredo
                || is_doc
                || is_site_local
                || is_ula
                || is_link_local
        }
    }
}

fn is_restricted_ipv4(v4: Ipv4Addr) -> bool {
    let [a, b, c, _] = v4.octets();
    v4.is_loopback()
        || v4.is_private()
        || v4.is_link_local()
        || v4.is_broadcast()
        || v4.is_documentation()
        || v4.is_multicast()
        || v4.is_unspecified()
        || (a == 100 && (64..=127).contains(&b)) // CGNAT 100.64.0.0/10
        || (a == 192 && b == 0 && c == 0) // IETF protocol assignments 192.0.0.0/24
        || (a == 192 && b == 88 && c == 99) // 6to4 relay anycast 192.88.99.0/24
        || (a == 198 && (b == 18 || b == 19)) // Benchmarking 198.18.0.0/15
        || (a == 0)
        || (a >= 240)
}

fn is_restricted_hostname(host: &str) -> bool {
    let normalized = host.trim_end_matches('.').to_ascii_lowercase();
    normalized.is_empty()
        || normalized == "localhost"
        || normalized.ends_with(".localhost")
        || normalized.ends_with(".local")
}

/// Validates URL scheme, hostname, and literal IP without performing DNS resolution.
/// Used by async clients & redirect policies where DNS is resolved once via `SsrfSafeResolver`.
pub fn validate_target_url_static(raw_url: &str, allow_private_ips: bool) -> Result<Url, String> {
    let parsed = Url::parse(raw_url).map_err(|e| format!("无效的 URL '{raw_url}': {e}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(format!("不支持的 URL 协议: {}", parsed.scheme()));
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| format!("URL 缺少主机名: {raw_url}"))?;

    if allow_private_ips {
        return Ok(parsed);
    }

    if is_restricted_hostname(host) {
        return Err(format!("安全策略拦截了对本地主机 '{host}' 的请求"));
    }

    let ip_candidate = host
        .trim_matches(|c| c == '[' || c == ']')
        .trim_end_matches('.');
    if let Ok(ip) = ip_candidate.parse::<IpAddr>() {
        if is_restricted_ip(ip) {
            return Err(format!("安全策略拦截了对内网/保留地址 '{ip}' 的请求"));
        }
        return Ok(parsed);
    }

    Ok(parsed)
}

/// Validates URL scheme, hostname, literal IP, and (when `!allow_private_ips`) DNS records fail-closed.
pub fn validate_target_url(raw_url: &str, allow_private_ips: bool) -> Result<Url, String> {
    let parsed = validate_target_url_static(raw_url, allow_private_ips)?;
    if allow_private_ips {
        return Ok(parsed);
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| format!("URL 缺少主机名: {raw_url}"))?;
    let ip_candidate = host
        .trim_matches(|c| c == '[' || c == ']')
        .trim_end_matches('.');
    if ip_candidate.parse::<IpAddr>().is_ok() {
        return Ok(parsed);
    }

    let port = parsed.port_or_known_default().unwrap_or(80);
    let addrs: Vec<SocketAddr> = (host, port)
        .to_socket_addrs()
        .map_err(|e| format!("安全策略拒绝未解析目标 '{host}': {e}"))?
        .collect();
    if addrs.is_empty() {
        return Err(format!("安全策略拒绝无 DNS 解析结果的目标 '{host}'"));
    }
    for addr in addrs {
        if is_restricted_ip(addr.ip()) {
            return Err(format!(
                "安全策略拦截了目标 '{host}' 解析到的内网/保留地址 '{}'",
                addr.ip()
            ));
        }
    }
    Ok(parsed)
}

/// Async custom DNS resolver injected into `reqwest` clients when `allow_private_ips == false`
/// and no proxy is configured. Validates every resolved IP and returns the validated addresses
/// directly to `reqwest`, eliminating DNS Rebinding TOCTOU and blocking DNS on Tokio workers.
#[derive(Debug, Default)]
struct SsrfSafeResolver;

impl Resolve for SsrfSafeResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_string();
        Box::pin(async move {
            if is_restricted_hostname(&host) {
                return Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    format!("安全策略拦截了对本地主机 '{host}' 的请求"),
                ))
                    as Box<dyn std::error::Error + Send + Sync>);
            }

            let ip_candidate = host
                .trim_matches(|c| c == '[' || c == ']')
                .trim_end_matches('.');
            if let Ok(ip) = ip_candidate.parse::<IpAddr>() {
                if is_restricted_ip(ip) {
                    return Err(Box::new(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        format!("安全策略拦截了对内网/保留地址 '{ip}' 的请求"),
                    ))
                        as Box<dyn std::error::Error + Send + Sync>);
                }
                let addrs: Addrs = Box::new(std::iter::once(SocketAddr::new(ip, 0)));
                return Ok(addrs);
            }

            let resolved: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), 0))
                .await
                .map_err(|e| {
                    Box::new(std::io::Error::new(
                        std::io::ErrorKind::AddrNotAvailable,
                        format!("安全策略拒绝未解析目标 '{host}': {e}"),
                    )) as Box<dyn std::error::Error + Send + Sync>
                })?
                .collect();

            if resolved.is_empty() {
                return Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::AddrNotAvailable,
                    format!("安全策略拒绝无 DNS 解析结果的目标 '{host}'"),
                ))
                    as Box<dyn std::error::Error + Send + Sync>);
            }

            for addr in &resolved {
                if is_restricted_ip(addr.ip()) {
                    return Err(Box::new(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        format!(
                            "安全策略拦截了目标 '{host}' 解析到的内网/保留地址 '{}'",
                            addr.ip()
                        ),
                    ))
                        as Box<dyn std::error::Error + Send + Sync>);
                }
            }

            let addrs: Addrs = Box::new(resolved.into_iter());
            Ok(addrs)
        })
    }
}

/// Builds a redirect policy that enforces hop limit and static SSRF validation on every redirect target.
/// Actual DNS resolution and IP enforcement run asynchronously inside `SsrfSafeResolver` (no blocking DNS).
pub fn ssrf_redirect_policy(allow_private_ips: bool, max_hops: usize) -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(move |attempt| {
        if attempt.previous().len() >= max_hops {
            return attempt.error(std::io::Error::other(format!(
                "重定向次数超过上限 ({max_hops})"
            )));
        }
        if let Some(prev) = attempt.previous().last()
            && prev.scheme().eq_ignore_ascii_case("https")
            && attempt.url().scheme().eq_ignore_ascii_case("http")
        {
            return attempt.error(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "禁止从 HTTPS 降级重定向至 HTTP",
            ));
        }
        if let Err(reason) = validate_target_url_static(attempt.url().as_str(), allow_private_ips) {
            return attempt.error(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                reason,
            ));
        }
        attempt.follow()
    })
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

    // Sniff HTML <meta charset="..."> in first 2 KiB
    let sniff_len = bytes.len().min(2048);
    let head_ascii = String::from_utf8_lossy(&bytes[..sniff_len]).to_ascii_lowercase();
    if let Some(meta_cs) = extract_meta_charset(&head_ascii)
        && let Some(enc) = Encoding::for_label(meta_cs.as_bytes())
    {
        let (cow, _, _) = enc.decode(bytes);
        return cow.into_owned();
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

fn extract_meta_charset(head_lower: &str) -> Option<String> {
    if let Some(idx) = head_lower.find("charset=") {
        let rest = &head_lower[idx + 8..];
        let rest = rest.trim_start_matches(['"', '\'', ' ']);
        let end = rest
            .find(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_')
            .unwrap_or(rest.len());
        let label = &rest[..end];
        if !label.is_empty() {
            return Some(label.to_string());
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

    let mut builder = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(key.timeout_secs))
        .connect_timeout(Duration::from_secs(key.timeout_secs.min(10)))
        .pool_max_idle_per_host(16)
        .pool_idle_timeout(Duration::from_secs(90))
        .redirect(ssrf_redirect_policy(
            key.allow_private_ips,
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

    let mut builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(key.timeout_secs))
        .connect_timeout(Duration::from_secs(key.timeout_secs.min(10)))
        .user_agent(DEFAULT_WEBVIEW_UA)
        .pool_max_idle_per_host(16)
        .pool_idle_timeout(Duration::from_secs(90))
        .redirect(ssrf_redirect_policy(
            key.allow_private_ips,
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
        let parsed = parse_legado_url(raw_url);
        if let Some(res) = try_resolve_data_url(&parsed) {
            return res;
        }

        let resolved_url = resolve_relative_url(&parsed.url, base_url);
        // Static validation runs upfront; DNS resolution & IP checks execute once inside
        // `SsrfSafeResolver` (when no proxy is configured) to prevent DNS rebinding TOCTOU.
        let validated = validate_target_url_static(&resolved_url, self.config.allow_private_ips)?;

        let method_str = parsed
            .options
            .method
            .as_deref()
            .unwrap_or("GET")
            .trim()
            .to_ascii_uppercase();
        let method =
            reqwest::Method::from_bytes(method_str.as_bytes()).unwrap_or(reqwest::Method::GET);

        let mut header_map = HeaderMap::new();
        let mut has_ua = false;
        let mut has_cookie = false;

        let is_forbidden_custom_header = |name: &str| {
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
        };

        for (k, v) in &self.default_headers {
            if is_forbidden_custom_header(k) {
                continue;
            }
            if let (Ok(hn), Ok(hv)) = (
                HeaderName::from_bytes(k.as_bytes()),
                HeaderValue::from_str(v),
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

        let mut resp = req
            .send()
            .map_err(|e| format!("请求 '{resolved_url}' 失败: {e}"))?;

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
            return Err(format!(
                "响应体超过最大限制 ({}MB): '{resolved_url}'",
                MAX_RESPONSE_BODY_BYTES / (1024 * 1024)
            ));
        }

        let mut raw_bytes = Vec::new();
        resp.by_ref()
            .take((MAX_RESPONSE_BODY_BYTES as u64) + 1)
            .read_to_end(&mut raw_bytes)
            .map_err(|e| format!("读取响应体失败 '{resolved_url}': {e}"))?;
        if raw_bytes.len() > MAX_RESPONSE_BODY_BYTES {
            return Err(format!(
                "响应体超过最大限制 ({}MB): '{resolved_url}'",
                MAX_RESPONSE_BODY_BYTES / (1024 * 1024)
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;

    #[test]
    fn restricted_ip_blocks_private_loopback_cgnat_and_ipv6_ranges() {
        for blocked in [
            "127.0.0.1",
            "10.0.0.1",
            "172.16.0.1",
            "172.31.255.254",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "192.0.0.1",
            "198.18.0.1",
            "198.19.255.254",
            "0.0.0.0",
            "240.0.0.1",
            "::1",
            "::",
            "fc00::1",
            "fd12:3456::1",
            "fe80::1",
            "fec0::1",
            "::ffff:127.0.0.1",
            "::ffff:172.16.5.4",
            "::127.0.0.1",
            "::ffff:0:127.0.0.1",
            "64:ff9b::127.0.0.1",
            "64:ff9b::1.1.1.1",
            "64:ff9b:1::1",
            "2002:7f00:1::1",
            "2001:0000::1",
            "2001:db8::1",
        ] {
            let ip: IpAddr = blocked.parse().unwrap();
            assert!(is_restricted_ip(ip), "expected {blocked} to be restricted");
        }

        for allowed in [
            "1.1.1.1",
            "8.8.8.8",
            "172.15.0.1",
            "172.32.0.1",
            "2606:4700::1",
        ] {
            let ip: IpAddr = allowed.parse().unwrap();
            assert!(!is_restricted_ip(ip), "expected {allowed} to be public");
        }
    }

    #[test]
    fn validate_target_url_rejects_local_private_and_non_http() {
        assert!(validate_target_url("http://127.0.0.1/admin", false).is_err());
        assert!(validate_target_url("http://localhost:8080/a", false).is_err());
        assert!(validate_target_url("http://localhost./a", false).is_err());
        assert!(validate_target_url("http://sub.localhost./a", false).is_err());
        assert!(validate_target_url("http://printer.local./a", false).is_err());
        assert!(validate_target_url("http://172.16.1.2/secret", false).is_err());
        assert!(validate_target_url("http://[::ffff:10.0.0.1]/x", false).is_err());
        assert!(validate_target_url("http://[64:ff9b::127.0.0.1]/x", false).is_err());
        assert!(validate_target_url("http://[2002:7f00:1::1]/x", false).is_err());
        assert!(validate_target_url("file:///etc/passwd", false).is_err());
        // Fail-closed when DNS resolution fails for a nonexistent domain
        assert!(
            validate_target_url("http://definitely-nonexistent-host.invalid/", false).is_err(),
            "DNS failure must fail-closed"
        );
        assert!(validate_target_url("https://1.1.1.1/ok", false).is_ok());
        assert!(validate_target_url("http://127.0.0.1/ok", true).is_ok());
    }

    #[test]
    fn ssrf_redirect_policy_blocks_302_to_loopback_and_localhost() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);
                let resp = "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:9/secret\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                let _ = stream.write_all(resp.as_bytes());
            }
        });

        // Build a blocking client that permits connecting to the initial test server IP
        // ONLY by bypassing initial check, while testing ssrf_redirect_policy(false, 6).
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(5))
            .redirect(ssrf_redirect_policy(false, MAX_REDIRECT_HOPS))
            .build()
            .unwrap();
        let err = client
            .get(format!("http://{addr}/start"))
            .send()
            .unwrap_err();
        assert!(
            err.is_redirect(),
            "expected redirect policy error, got: {err}"
        );
        let _ = handle.join();
    }

    #[test]
    fn cookie_matching_only_allows_parent_to_child_with_longest_suffix() {
        let cookies = Arc::new(Mutex::new(HashMap::new()));
        let client = LegadoHttpClient::new(
            HttpClientConfig {
                allow_private_ips: true,
                ..Default::default()
            },
            cookies,
            None,
            true,
        )
        .unwrap();

        assert!(client.set_cookie("https://api.example.com", "secret=child"));
        assert!(client.set_cookie("https://example.com", "base=root"));
        assert!(client.set_cookie("https://sub.example.com", "sub=mid"));

        // Exact matches
        assert_eq!(client.get_cookie_header("https://example.com"), "base=root");
        assert_eq!(
            client.get_cookie_header("https://api.example.com"),
            "secret=child"
        );
        // Subdomain inherits longest parent suffix
        assert_eq!(
            client.get_cookie_header("https://deep.sub.example.com"),
            "sub=mid"
        );
        assert_eq!(
            client.get_cookie_header("https://other.example.com"),
            "base=root"
        );

        // Child cookie must NEVER leak upward to parent domain
        client.remove_cookie("https://example.com");
        assert_eq!(
            client.get_cookie_header("https://example.com"),
            "",
            "child domain cookie must not leak to parent domain"
        );
        assert_eq!(
            client.get_cookie_header("https://evil-example.com"),
            "",
            "unrelated domain with matching string suffix must not receive cookie"
        );

        // Cookie quota enforcement
        let oversized = format!("big={}", "a".repeat(MAX_COOKIE_TOTAL_BYTES + 16));
        assert!(
            !client.set_cookie("https://quota.example.com", &oversized),
            "oversized cookie must be rejected by quota"
        );
    }

    #[test]
    fn legado_http_client_enforces_response_size_limit() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);
                let oversz = MAX_RESPONSE_BODY_BYTES + 1024;
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {oversz}\r\nConnection: close\r\n\r\n"
                );
                let _ = stream.write_all(header.as_bytes());
            }
        });

        let client = LegadoHttpClient::new(
            HttpClientConfig {
                timeout_secs: 5,
                allow_private_ips: true,
                proxy_url: None,
            },
            Arc::new(Mutex::new(HashMap::new())),
            None,
            false,
        )
        .unwrap();

        let err = client
            .fetch(&format!("http://{addr}/huge"), "")
            .unwrap_err();
        assert!(
            err.contains("响应体超过最大限制"),
            "unexpected error: {err}"
        );
        let _ = handle.join();
    }
}

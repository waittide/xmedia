use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, ToSocketAddrs};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::{STANDARD as BASE64_STD, STANDARD_NO_PAD as BASE64_NO_PAD};
use encoding_rs::Encoding;
use reqwest::header::{COOKIE, HeaderMap, HeaderName, HeaderValue, SET_COOKIE, USER_AGENT};
use serde::{Deserialize, Serialize};
use url::Url;

pub const DEFAULT_WEBVIEW_UA: &str = "Mozilla/5.0 (Linux; Android 13; Pixel 7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Mobile Safari/537.36";

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
            timeout_secs: 20,
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
        Ok(form_urlencoded::parse(payload.as_bytes())
            .next()
            .map(|(k, _)| k.into_owned().into_bytes())
            .unwrap_or_else(|| payload.as_bytes().to_vec()))
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
        return host.to_ascii_lowercase();
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

/// Checks whether an IP address is private, loopback, link-local, or otherwise non-public.
pub fn is_restricted_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_restricted_ipv4(v4),
        IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return is_restricted_ipv4(mapped);
            }
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (v6.segments()[0] & 0xfe00) == 0xfc00 // fc00::/7 ULA
                || (v6.segments()[0] & 0xffc0) == 0xfe80 // fe80::/10 link-local
        }
    }
}

fn is_restricted_ipv4(v4: Ipv4Addr) -> bool {
    let [a, b, _, _] = v4.octets();
    v4.is_loopback()
        || v4.is_private()
        || v4.is_link_local()
        || v4.is_broadcast()
        || v4.is_documentation()
        || v4.is_multicast()
        || v4.is_unspecified()
        || (a == 100 && (64..=127).contains(&b)) // CGNAT 100.64.0.0/10
        || (a == 0)
        || (a >= 240)
}

pub fn validate_target_url(raw_url: &str, allow_private_ips: bool) -> Result<Url, String> {
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

    let lower = host.to_ascii_lowercase();
    if lower == "localhost" || lower.ends_with(".localhost") || lower.ends_with(".local") {
        return Err(format!("安全策略拦截了对本地主机 '{host}' 的请求"));
    }

    if let Ok(ip) = host
        .trim_matches(|c| c == '[' || c == ']')
        .parse::<IpAddr>()
    {
        if is_restricted_ip(ip) {
            return Err(format!("安全策略拦截了对内网/保留地址 '{ip}' 的请求"));
        }
        return Ok(parsed);
    }

    let port = parsed.port_or_known_default().unwrap_or(80);
    if let Ok(addrs) = (host, port).to_socket_addrs() {
        for addr in addrs {
            if is_restricted_ip(addr.ip()) {
                return Err(format!(
                    "安全策略拦截了目标 '{host}' 解析到的内网/保留地址 '{}'",
                    addr.ip()
                ));
            }
        }
    }
    Ok(parsed)
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
        let mut builder = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs.max(3)))
            .redirect(reqwest::redirect::Policy::limited(6));
        if let Some(ref proxy) = config.proxy_url
            && !proxy.trim().is_empty()
        {
            let p = reqwest::Proxy::all(proxy.trim()).map_err(|e| format!("代理配置无效: {e}"))?;
            builder = builder.proxy(p);
        }
        let client = builder
            .build()
            .map_err(|e| format!("创建 HTTP 客户端失败: {e}"))?;

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
        let guard = self.cookies.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(v) = guard.get(&key) {
            return v.clone();
        }
        // Fallback: suffix match (e.g. "fanqienovel.com" matching "api.fanqienovel.com")
        for (stored_host, val) in guard.iter() {
            if key.ends_with(&format!(".{stored_host}"))
                || stored_host.ends_with(&format!(".{key}"))
            {
                return val.clone();
            }
        }
        String::new()
    }

    pub fn set_cookie(&self, url_or_domain: &str, cookie_val: &str) {
        let key = normalize_cookie_key(url_or_domain);
        if key.is_empty() {
            return;
        }
        let mut guard = self.cookies.lock().unwrap_or_else(|e| e.into_inner());
        let existing = guard.get(&key).cloned().unwrap_or_default();
        let merged = merge_cookie_string(&existing, cookie_val);
        guard.insert(key, merged);
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
        let validated = validate_target_url(&resolved_url, self.config.allow_private_ips)?;

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

        for (k, v) in &self.default_headers {
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

        let resp = req
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

        let raw_bytes = resp
            .bytes()
            .map_err(|e| format!("读取响应体失败 '{resolved_url}': {e}"))?;

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

#[allow(dead_code)]
fn _keep_ipv6_ref(_: Ipv6Addr) {}

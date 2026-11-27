use std::collections::HashMap;

use base64::Engine;
use base64::engine::general_purpose::{STANDARD as BASE64_STD, STANDARD_NO_PAD as BASE64_NO_PAD};
use serde::{Deserialize, Serialize};
use url::Url;

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
    pub raw_bytes: Vec<u8>,
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

    for idx in (0..bytes.len()).rev() {
        let b = bytes[idx];
        if b == b'"' {
            // A quote is escaped iff preceded by an odd number of consecutive backslashes.
            let mut backslashes = 0usize;
            let mut k = idx;
            while k > 0 && bytes[k - 1] == b'\\' {
                backslashes += 1;
                k -= 1;
            }
            if backslashes.is_multiple_of(2) {
                in_string = !in_string;
            }
            continue;
        }
        if in_string {
            continue;
        }
        match b {
            b'}' => depth += 1,
            b'{' => {
                depth -= 1;
                if depth == 0 {
                    let url_part = input[..idx].trim_end().strip_suffix(',')?;
                    return Some((url_part, &input[idx..]));
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
        raw_bytes,
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

use std::collections::BTreeMap;
use std::io::Read;
use std::sync::LazyLock;

use encoding_rs::{Encoding, GB18030, UTF_8};
use flate2::read::GzDecoder;
use regex::Regex;
use reqwest::Client;
use serde::de::DeserializeOwned;
use xmedia_core::domain::error::{DomainError, DomainResult};

use crate::domain::config::ProviderCredentials;
use crate::infra::scrapers::http_client::{SsrfRedirectGuard, get_or_build_http_client};

pub(super) const DESKTOP_BROWSER_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";

pub(super) const MOBILE_BROWSER_UA: &str = "Mozilla/5.0 (Linux; Android 13; Pixel 7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Mobile Safari/537.36";

static RE_BLOCK_BREAKS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)<(?:br\s*/?|/p|/div|/li)\s*>").unwrap());
static RE_ANY_TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]+>").unwrap());
static RE_META_TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?is)<meta\b[^>]*>").unwrap());
static RE_ATTR_PAIR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"([a-zA-Z_:][-a-zA-Z0-9_:.]*)\s*=\s*(?:"([^"]*)"|'([^']*)')"#).unwrap()
});
static RE_NUMERIC_ENTITY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"&#(x[0-9a-fA-F]+|\d+);").unwrap());
static RE_DATE_YMD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b((?:19|20)\d{2})[-/.](\d{1,2})[-/.](\d{1,2})\b").unwrap());
static RE_YEAR_ONLY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b((?:19|20)\d{2})\b").unwrap());

pub(super) fn build_browser_http_client(
    creds: &ProviderCredentials,
    mobile: bool,
) -> DomainResult<Client> {
    let default_ua = if mobile {
        MOBILE_BROWSER_UA
    } else {
        DESKTOP_BROWSER_UA
    };
    let ua = creds
        .user_agent
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(default_ua);
    let timeout_secs = creds.timeout_secs.map(|s| s.clamp(5, 120)).unwrap_or(15);

    get_or_build_http_client(
        creds.http_proxy.as_deref(),
        ua,
        timeout_secs,
        SsrfRedirectGuard::PublicOnly,
    )
}

const MAX_SCRAPER_BODY_BYTES: usize = 8 * 1024 * 1024;
const MAX_SCRAPER_DECOMPRESSED_BYTES: u64 = 16 * 1024 * 1024;

pub(super) async fn read_response_text(
    mut resp: reqwest::Response,
    fallback_encoding: &'static Encoding,
) -> DomainResult<String> {
    let header_enc = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(detect_charset_from_content_type);

    if let Some(cl) = resp.content_length()
        && cl > MAX_SCRAPER_BODY_BYTES as u64
    {
        return Err(DomainError::Validation(
            "upstream response body exceeds 8 MiB safety limit".into(),
        ));
    }

    let mut raw = Vec::new();
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| DomainError::Storage(format!("failed reading HTTP body: {e}")))?
    {
        if raw.len().saturating_add(chunk.len()) > MAX_SCRAPER_BODY_BYTES {
            return Err(DomainError::Validation(
                "upstream response body exceeds 8 MiB safety limit".into(),
            ));
        }
        raw.extend_from_slice(&chunk);
    }

    let decompressed: Vec<u8> = if raw.starts_with(&[0x1f, 0x8b]) {
        let decoder = GzDecoder::new(&raw[..]);
        let mut out = Vec::new();
        if decoder
            .take(MAX_SCRAPER_DECOMPRESSED_BYTES + 1)
            .read_to_end(&mut out)
            .is_ok()
        {
            if (out.len() as u64) > MAX_SCRAPER_DECOMPRESSED_BYTES {
                return Err(DomainError::Validation(
                    "decompressed upstream body exceeds 16 MiB safety limit".into(),
                ));
            }
            out
        } else {
            raw
        }
    } else {
        raw
    };

    let enc = header_enc.unwrap_or(fallback_encoding);
    let (cow, _, _) = enc.decode(&decompressed);
    Ok(cow.into_owned())
}

pub(super) async fn read_json_response<T: DeserializeOwned>(
    resp: reqwest::Response,
    label: &str,
) -> DomainResult<T> {
    let text = read_response_text(resp, UTF_8).await?;
    serde_json::from_str(text.trim())
        .map_err(|e| DomainError::Storage(format!("invalid {label} JSON: {e}")))
}

fn detect_charset_from_content_type(ct: &str) -> Option<&'static Encoding> {
    let lower = ct.to_ascii_lowercase();
    let idx = lower.find("charset=")?;
    let trimmed = lower[idx + 8..].trim_start_matches([' ', '\t', '"', '\'']);
    let rest = trimmed.split([';', ' ', '\t', '"', '\'']).next()?.trim();
    if rest.is_empty() {
        return None;
    }
    if rest.contains("gb") {
        Some(GB18030)
    } else if rest.contains("utf-8") || rest.contains("utf8") {
        Some(UTF_8)
    } else {
        Encoding::for_label(rest.as_bytes())
    }
}

pub(super) fn percent_encode_utf8(input: &str) -> String {
    form_urlencoded::byte_serialize(input.as_bytes()).collect()
}

pub(super) fn percent_encode_gb18030(input: &str) -> String {
    let (bytes, _, _) = GB18030.encode(input);
    let mut out = String::with_capacity(bytes.len() * 3);
    for &b in bytes.as_ref() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub(super) fn decode_html_entities(raw: &str) -> String {
    let named = raw
        .replace("&nbsp;", " ")
        .replace("&#160;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&ldquo;", "“")
        .replace("&rdquo;", "”")
        .replace("&lsquo;", "‘")
        .replace("&rsquo;", "’")
        .replace("&hellip;", "…")
        .replace("&mdash;", "—")
        .replace("&middot;", "·")
        .replace("&amp;", "&");

    RE_NUMERIC_ENTITY
        .replace_all(&named, |caps: &regex::Captures| {
            let token = &caps[1];
            let code =
                if let Some(hex) = token.strip_prefix('x').or_else(|| token.strip_prefix('X')) {
                    u32::from_str_radix(hex, 16).ok()
                } else {
                    token.parse::<u32>().ok()
                };
            code.and_then(char::from_u32)
                .map(|c| c.to_string())
                .unwrap_or_default()
        })
        .into_owned()
}

pub(super) fn strip_html_to_text(raw: &str) -> String {
    if raw.trim().is_empty() {
        return String::new();
    }
    let unescaped_once = decode_html_entities(raw)
        .replace("\\r\\n", "\n")
        .replace("\\n", "\n")
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let with_breaks = RE_BLOCK_BREAKS.replace_all(&unescaped_once, "\n");
    let no_tags = RE_ANY_TAG.replace_all(&with_breaks, "");
    let decoded = decode_html_entities(&no_tags);

    let mut lines: Vec<String> = Vec::new();
    let mut prev_empty = false;
    for line in decoded.lines() {
        let trimmed = line
            .trim_matches(|c: char| c.is_whitespace() || c == '\u{3000}')
            .to_string();
        if trimmed.is_empty() {
            if !lines.is_empty() && !prev_empty {
                lines.push(String::new());
                prev_empty = true;
            }
        } else {
            lines.push(trimmed);
            prev_empty = false;
        }
    }
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines.join("\n")
}

pub(super) fn strip_inline_html(raw: &str) -> String {
    let unescaped = decode_html_entities(raw);
    let no_tags = RE_ANY_TAG.replace_all(&unescaped, "");
    let decoded = decode_html_entities(&no_tags);
    decoded
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_matches('\u{3000}')
        .trim()
        .to_string()
}

pub(super) fn normalize_cover_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim().replace("\\/", "/");
    if trimmed.is_empty()
        || trimmed.contains("nocover")
        || trimmed.contains("default_cover")
        || trimmed.contains("defaultNew")
    {
        return None;
    }
    if let Some(rest) = trimmed.strip_prefix("//") {
        return Some(format!("https://{rest}"));
    }
    if let Some(rest) = trimmed.strip_prefix("http://") {
        return Some(format!("https://{rest}"));
    }
    if trimmed.starts_with("https://") {
        return Some(trimmed);
    }
    None
}

pub(super) fn extract_meta_map(html: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for tag_match in RE_META_TAG.find_iter(html) {
        let tag = tag_match.as_str();
        let mut key: Option<String> = None;
        let mut content: Option<String> = None;
        for caps in RE_ATTR_PAIR.captures_iter(tag) {
            let attr_name = caps[1].to_ascii_lowercase();
            let attr_val = caps
                .get(2)
                .or_else(|| caps.get(3))
                .map(|m| m.as_str())
                .unwrap_or("");
            match attr_name.as_str() {
                "property" | "name" => {
                    if key.is_none() && !attr_val.trim().is_empty() {
                        key = Some(attr_val.trim().to_ascii_lowercase());
                    }
                }
                "content" => {
                    content = Some(attr_val.to_string());
                }
                _ => {}
            }
        }
        if let (Some(k), Some(v)) = (key, content) {
            let cleaned = v.trim().to_string();
            if !cleaned.is_empty() {
                out.entry(k).or_insert(cleaned);
            }
        }
    }
    out
}

pub(super) fn parse_year_and_date(raw: &str) -> (Option<i32>, Option<String>) {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return (None, None);
    }
    if let Some(caps) = RE_DATE_YMD.captures(trimmed)
        && let (Ok(y), Ok(m), Ok(d)) = (
            caps[1].parse::<i32>(),
            caps[2].parse::<u32>(),
            caps[3].parse::<u32>(),
        )
        && (1900..=2100).contains(&y)
        && (1..=12).contains(&m)
        && (1..=31).contains(&d)
    {
        return (Some(y), Some(format!("{y:04}-{m:02}-{d:02}")));
    }
    if let Some(caps) = RE_YEAR_ONLY.captures(trimmed)
        && let Ok(y) = caps[1].parse::<i32>()
        && (1900..=2100).contains(&y)
    {
        return (Some(y), Some(y.to_string()));
    }
    (None, None)
}

pub(super) fn is_antibot_or_captcha_html(html: &str) -> bool {
    let trimmed = html.trim();
    if trimmed.is_empty() {
        return true;
    }
    trimmed.contains("<title>验证码")
        || trimmed.contains("<title>验证</title>")
        || trimmed.contains("acw_sc__v2")
        || (trimmed.len() < 8000 && trimmed.contains("geetest") && !trimmed.contains("og:novel"))
}

pub(super) fn extract_numeric_id_from_query(raw: &str, url_markers: &[&str]) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.len() >= 4 && trimmed.len() <= 22 && trimmed.bytes().all(|b| b.is_ascii_digit()) {
        return Some(trimmed.to_string());
    }
    for marker in url_markers {
        if let Some(idx) = trimmed.find(marker) {
            let after = &trimmed[idx + marker.len()..];
            let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
            if digits.len() >= 4 {
                return Some(digits);
            }
        }
    }
    None
}

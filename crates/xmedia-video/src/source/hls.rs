use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use bytes::Bytes;
use regex::Regex;
use tokio::sync::mpsc;
use url::Url;
use xmedia_core::application::ports::ByteChunkStream;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::infra::stream::MpscByteStream;
use xmedia_source::http::{
    DEFAULT_WEBVIEW_UA, MAX_REDIRECT_HOPS, SsrfSafeResolver, ssrf_redirect_policy,
};

pub const MAX_M3U8_BYTES: usize = 8 * 1024 * 1024;
pub const STREAM_CONNECT_TIMEOUT_SECS: u64 = 10;
pub const STREAM_READ_TIMEOUT_SECS: u64 = 30;
const MAX_CACHED_STREAMING_CLIENTS: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct StreamingClientKey {
    allow_private_ips: bool,
    proxy_url: Option<String>,
}

static STREAMING_ASYNC_CLIENT_CACHE: LazyLock<Mutex<HashMap<StreamingClientKey, reqwest::Client>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

static RE_M3U8_URI_ATTR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)\b(URI=")([^"]*)(")"#).expect("valid m3u8 URI attr regex"));

/// Builds or returns a pooled async `reqwest::Client` specifically for long-lived video streaming.
/// Uses `connect_timeout` + per-chunk `read_timeout` WITHOUT a short total-body `.timeout(...)`
/// so active MP4 / HLS playback is never killed mid-stream.
pub fn get_or_build_streaming_async_client(
    allow_private_ips: bool,
    proxy_url: Option<&str>,
) -> DomainResult<reqwest::Client> {
    let key = StreamingClientKey {
        allow_private_ips,
        proxy_url: proxy_url
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
    };
    {
        let guard = STREAMING_ASYNC_CLIENT_CACHE
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(client) = guard.get(&key) {
            return Ok(client.clone());
        }
    }

    let mut builder = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(STREAM_CONNECT_TIMEOUT_SECS))
        .read_timeout(Duration::from_secs(STREAM_READ_TIMEOUT_SECS))
        .user_agent(DEFAULT_WEBVIEW_UA)
        .pool_max_idle_per_host(16)
        .pool_idle_timeout(Duration::from_secs(90))
        .redirect(ssrf_redirect_policy(
            key.allow_private_ips,
            MAX_REDIRECT_HOPS,
        ));
    if let Some(ref proxy) = key.proxy_url {
        let p = reqwest::Proxy::all(proxy.as_str())
            .map_err(|e| DomainError::Storage(format!("代理配置无效: {e}")))?;
        builder = builder.proxy(p);
    } else {
        builder = builder.no_proxy();
        if !key.allow_private_ips {
            builder = builder.dns_resolver(Arc::new(SsrfSafeResolver));
        }
    }

    let client = builder
        .build()
        .map_err(|e| DomainError::Storage(format!("创建流媒体 HTTP 客户端失败: {e}")))?;

    let mut guard = STREAMING_ASYNC_CLIENT_CACHE
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if guard.len() >= MAX_CACHED_STREAMING_CLIENTS && !guard.contains_key(&key) {
        guard.clear();
    }
    Ok(guard.entry(key).or_insert(client).clone())
}

pub fn spawn_chunk_stream(
    mut resp: reqwest::Response,
    initial_chunk: Option<Bytes>,
) -> ByteChunkStream {
    let (tx, rx) = mpsc::channel::<DomainResult<Bytes>>(16);
    tokio::spawn(async move {
        if let Some(first) = initial_chunk
            && !first.is_empty()
            && tx.send(Ok(first)).await.is_err()
        {
            return;
        }
        loop {
            match resp.chunk().await {
                Ok(Some(chunk)) => {
                    if chunk.is_empty() {
                        continue;
                    }
                    if tx.send(Ok(chunk)).await.is_err() {
                        break;
                    }
                }
                Ok(None) => break,
                Err(err) => {
                    let _ = tx
                        .send(Err(DomainError::Storage(format!(
                            "upstream media stream error: {err}"
                        ))))
                        .await;
                    break;
                }
            }
        }
    });
    MpscByteStream::into_boxed(rx)
}

pub fn is_forbidden_media_ct(ct_lower: &str) -> bool {
    ct_lower.contains("text/html")
        || ct_lower.contains("application/xhtml")
        || ct_lower.contains("image/svg")
}

pub fn looks_like_html_bytes(bytes: &[u8]) -> bool {
    let trimmed = strip_bom_and_ascii_ws(bytes);
    if trimmed.len() < 5 {
        return false;
    }
    let head: Vec<u8> = trimmed
        .iter()
        .take(16)
        .map(|b| b.to_ascii_lowercase())
        .collect();
    head.starts_with(b"<html")
        || head.starts_with(b"<!doctype")
        || head.starts_with(b"<head")
        || head.starts_with(b"<body")
        || head.starts_with(b"<svg")
}

fn strip_bom_and_ascii_ws(mut bytes: &[u8]) -> &[u8] {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        bytes = &bytes[3..];
    }
    while let Some((&b, rest)) = bytes.split_first() {
        if b.is_ascii_whitespace() {
            bytes = rest;
        } else {
            break;
        }
    }
    bytes
}

pub fn is_hls_playlist_response(ct_lower: &str, final_url: &Url, first_chunk: &[u8]) -> bool {
    let stripped = strip_bom_and_ascii_ws(first_chunk);
    if stripped.starts_with(b"#EXTM3U") {
        return true;
    }
    if looks_like_html_bytes(first_chunk) {
        return false;
    }
    let ct_is_m3u8 = ct_lower.contains("mpegurl");
    let path_is_m3u8 = final_url.path().to_ascii_lowercase().ends_with(".m3u8")
        || final_url.path().to_ascii_lowercase().ends_with(".m3u");
    ct_is_m3u8 || path_is_m3u8
}

pub fn infer_segment_content_type(upstream_ct: Option<&str>, final_url: &Url) -> String {
    if let Some(ct) = upstream_ct {
        let base = ct
            .split(';')
            .next()
            .unwrap_or(ct)
            .trim()
            .to_ascii_lowercase();
        if !base.is_empty() && base != "text/plain" && !is_forbidden_media_ct(&base) {
            return ct.trim().to_string();
        }
    }
    let path_lower = final_url.path().to_ascii_lowercase();
    if path_lower.ends_with(".ts") || path_lower.ends_with(".m2ts") {
        "video/mp2t".to_string()
    } else if path_lower.ends_with(".m4s")
        || path_lower.ends_with(".mp4")
        || path_lower.ends_with(".m4v")
    {
        "video/mp4".to_string()
    } else if path_lower.ends_with(".m4a") {
        "audio/mp4".to_string()
    } else if path_lower.ends_with(".aac") {
        "audio/aac".to_string()
    } else if path_lower.ends_with(".vtt") {
        "text/vtt; charset=utf-8".to_string()
    } else if path_lower.ends_with(".webm") {
        "video/webm".to_string()
    } else {
        "application/octet-stream".to_string()
    }
}

pub fn encode_query_component(val: &str) -> String {
    form_urlencoded::byte_serialize(val.as_bytes()).collect()
}

pub fn build_poster_proxy_url(target_url: &str, source_id: Option<uuid::Uuid>) -> String {
    let mut out = format!(
        "/api/v1/video/online/poster-proxy?url={}",
        encode_query_component(target_url)
    );
    if let Some(sid) = source_id {
        out.push_str("&source_id=");
        out.push_str(&sid.to_string());
    }
    out
}

pub fn build_stream_proxy_url(
    target_url: &str,
    source_id: Option<uuid::Uuid>,
    token: Option<&str>,
) -> String {
    let mut out = format!(
        "/api/v1/video/online/stream-proxy?url={}",
        encode_query_component(target_url)
    );
    if let Some(sid) = source_id {
        out.push_str("&source_id=");
        out.push_str(&sid.to_string());
    }
    if let Some(tok) = token.map(str::trim).filter(|t| !t.is_empty()) {
        out.push_str("&token=");
        out.push_str(&encode_query_component(tok));
    }
    out
}

pub fn build_hls_proxy_url(
    target_url: &str,
    source_id: Option<uuid::Uuid>,
    token: Option<&str>,
) -> String {
    let mut out = format!(
        "/api/v1/video/online/hls-proxy?url={}",
        encode_query_component(target_url)
    );
    if let Some(sid) = source_id {
        out.push_str("&source_id=");
        out.push_str(&sid.to_string());
    }
    if let Some(tok) = token.map(str::trim).filter(|t| !t.is_empty()) {
        out.push_str("&token=");
        out.push_str(&encode_query_component(tok));
    }
    out
}

fn resolve_m3u8_uri(raw_uri: &str, base_url: &Url) -> Option<String> {
    let trimmed = raw_uri.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("data:") || lower.starts_with("skd:") {
        return None;
    }
    if let Ok(joined) = base_url.join(trimmed)
        && matches!(joined.scheme(), "http" | "https")
    {
        return Some(joined.to_string());
    }
    None
}

/// Rewrites all HLS playlist URIs:
/// - `URI="..."` attributes on directives (`#EXT-X-KEY`, `#EXT-X-MAP`, `#EXT-X-MEDIA`,
///   `#EXT-X-I-FRAME-STREAM-INF`, `#EXT-X-SESSION-KEY`, `#EXT-X-SESSION-DATA`,
///   `#EXT-X-PART`, `#EXT-X-PRELOAD-HINT`, `#EXT-X-RENDITION-REPORT`, etc.)
/// - All non-comment URI lines (variant sub-playlists, `.ts`, `.m4s`, `.mp4`, `.vtt`, etc.)
///
/// Relative URIs are resolved against `base_url` (which must be the post-redirect final URL).
/// `token` is preserved ONLY when the caller itself authenticated via `?token=`.
pub fn rewrite_m3u8_playlist(
    playlist_text: &str,
    base_url: &Url,
    source_id: Option<uuid::Uuid>,
    token: Option<&str>,
) -> String {
    let clean_text = playlist_text.trim_start_matches('\u{feff}');
    let mut out = String::with_capacity(clean_text.len() + 256);

    for line in clean_text.lines() {
        let line_no_cr = line.trim_end_matches('\r');
        let trimmed = line_no_cr.trim();

        if trimmed.is_empty() {
            out.push('\n');
            continue;
        }

        if trimmed.starts_with('#') {
            if RE_M3U8_URI_ATTR.is_match(line_no_cr) {
                let replaced =
                    RE_M3U8_URI_ATTR.replace_all(line_no_cr, |caps: &regex::Captures| {
                        let prefix = &caps[1];
                        let raw_uri = &caps[2];
                        let suffix = &caps[3];
                        if let Some(resolved) = resolve_m3u8_uri(raw_uri, base_url) {
                            let proxied = build_hls_proxy_url(&resolved, source_id, token);
                            format!("{prefix}{proxied}{suffix}")
                        } else {
                            caps[0].to_string()
                        }
                    });
                out.push_str(&replaced);
            } else {
                out.push_str(line_no_cr);
            }
            out.push('\n');
            continue;
        }

        if let Some(resolved) = resolve_m3u8_uri(trimmed, base_url) {
            out.push_str(&build_hls_proxy_url(&resolved, source_id, token));
        } else {
            out.push_str(line_no_cr);
        }
        out.push('\n');
    }

    out
}

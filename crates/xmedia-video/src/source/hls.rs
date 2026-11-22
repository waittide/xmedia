use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use bytes::{Bytes, BytesMut};
use regex::Regex;
use tokio::sync::mpsc;
use url::Url;
use xmedia_core::application::ports::ByteChunkStream;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::infra::stream::MpscByteStream;
use xmedia_source::http::{
    DEFAULT_WEBVIEW_UA, MAX_REDIRECT_HOPS, SsrfSafeResolver, ssrf_redirect_policy,
};
use xmedia_source::proxy::{
    MEDIA_PROLOG_SNIFF_MAX_BYTES, has_enough_media_sniff_bytes,
    has_unclosed_leading_xml_or_comment, is_only_bom_or_whitespace, looks_like_html_or_svg_bytes,
    strip_bom_and_ascii_ws,
};

pub const MAX_M3U8_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_SUBTITLE_BYTES: usize = 4 * 1024 * 1024;
pub const SNIFF_PREFIX_MAX_BYTES: usize = 4096;
pub const SNIFF_MIN_NON_WS_BYTES: usize = 16;
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
static RE_SRT_TIMESTAMP_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^\s*((?:\d{1,3}:)?\d{2}:\d{2})[,.](\d{3})\s*-->\s*((?:\d{1,3}:)?\d{2}:\d{2})[,.](\d{3})(.*)$",
    )
    .expect("valid srt timestamp regex")
});

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

fn is_utf16_bom_prefix(buf: &[u8]) -> bool {
    buf.starts_with(&[0xFF, 0xFE]) || buf.starts_with(&[0xFE, 0xFF])
}

fn has_binary_media_magic_or_null(buf: &[u8]) -> bool {
    if !is_utf16_bom_prefix(buf) && buf.contains(&0) {
        return true;
    }
    if buf.get(4..8) == Some(b"ftyp")
        || buf.starts_with(b"\x1A\x45\xDF\xA3")
        || buf.starts_with(b"FLV")
        || buf.starts_with(b"ID3")
        || buf.starts_with(b"RIFF")
        || buf.starts_with(b"OggS")
        || buf.starts_with(b"fLaC")
    {
        return true;
    }
    if buf.len() >= 2 && buf[0] == 0xFF && (buf[1] & 0xE0) == 0xE0 && buf[1] != 0xFE {
        return true;
    }
    // MPEG-TS sync byte 0x47 ('G'): verify either a second sync byte at +188 or non-text binary bytes
    if buf.first() == Some(&0x47)
        && ((buf.len() >= 189 && buf[188] == 0x47)
            || buf
                .iter()
                .any(|&b| b < 0x09 || (b > 0x0D && b < 0x20) || b >= 0x80))
    {
        return true;
    }
    false
}

/// Reads a bounded prefix from `resp` across one or more chunks so prefix sniffing
/// (HTML/SVG detection, UTF-16 BOM / large leading comments, and partial `#EXTM3U` / `#EXTINF:`
/// playlists) is immune to chunk-boundary splits and 206 Range offsets.
pub async fn read_sniff_prefix(
    resp: &mut reqwest::Response,
    max_sniff_bytes: usize,
) -> DomainResult<Bytes> {
    let fallback_url = Url::parse("http://localhost/").expect("valid static fallback url");
    read_sniff_prefix_with_context(resp, max_sniff_bytes, false, "", &fallback_url).await
}

/// Context-aware sniff prefix reader: on `206 Partial Content` or generic/text/empty Content-Type,
/// avoids stopping at `16` non-whitespace bytes unless binary media magic/NUL, HTML/SVG, or HLS
/// tags are observed, and extends up to `MEDIA_PROLOG_SNIFF_MAX_BYTES` for unclosed `<!--` / `<?`.
pub async fn read_sniff_prefix_with_context(
    resp: &mut reqwest::Response,
    max_sniff_bytes: usize,
    is_partial: bool,
    ct_lower: &str,
    final_url: &Url,
) -> DomainResult<Bytes> {
    let mut buf = BytesMut::new();
    let might_be_text_or_partial_hls = is_partial
        || ct_lower.is_empty()
        || ct_lower == "text/plain"
        || is_generic_binary_ct(ct_lower);

    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| DomainError::Validation(format!("读取上游媒体响应失败: {e}")))?
    {
        if chunk.is_empty() {
            continue;
        }
        buf.extend_from_slice(&chunk);
        if has_unclosed_leading_xml_or_comment(&buf) || is_only_bom_or_whitespace(&buf) {
            if buf.len() >= max_sniff_bytes.max(MEDIA_PROLOG_SNIFF_MAX_BYTES) {
                break;
            }
            continue;
        }
        if buf.len() >= max_sniff_bytes {
            break;
        }
        if might_be_text_or_partial_hls {
            let hls_ready = if is_partial {
                is_partial_hls_playlist_range_candidate(ct_lower, final_url, &buf)
            } else {
                let stripped = strip_bom_and_ascii_ws(&buf);
                let has_full_tag = stripped.starts_with(b"#EXTM3U")
                    || stripped.starts_with(b"#EXT-X-")
                    || stripped.starts_with(b"#EXTINF:");
                let ct_or_path_hls = ct_lower.contains("mpegurl")
                    || final_url.path().to_ascii_lowercase().ends_with(".m3u8")
                    || final_url.path().to_ascii_lowercase().ends_with(".m3u");
                has_full_tag
                    || (ct_or_path_hls
                        && has_enough_media_sniff_bytes(&buf, SNIFF_MIN_NON_WS_BYTES))
            };
            if has_binary_media_magic_or_null(&buf)
                || looks_like_html_bytes(&buf)
                || hls_ready
                || (is_utf16_bom_prefix(&buf)
                    && has_enough_media_sniff_bytes(&buf, SNIFF_MIN_NON_WS_BYTES))
            {
                break;
            }
        } else if has_enough_media_sniff_bytes(&buf, SNIFF_MIN_NON_WS_BYTES) {
            break;
        }
    }
    Ok(buf.freeze())
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
    looks_like_html_or_svg_bytes(bytes)
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

pub fn is_partial_hls_playlist_range_candidate(
    ct_lower: &str,
    final_url: &Url,
    prefix: &[u8],
) -> bool {
    if is_hls_playlist_response(ct_lower, final_url, prefix) {
        return true;
    }
    let allowed_ct =
        ct_lower.is_empty() || ct_lower == "text/plain" || is_generic_binary_ct(ct_lower);
    if !allowed_ct || looks_like_html_bytes(prefix) || prefix.contains(&0) {
        return false;
    }
    let stripped = strip_bom_and_ascii_ws(prefix);
    stripped.starts_with(b"#EXT")
        || prefix.windows(7).any(|w| w == b"#EXT-X-")
        || prefix.windows(8).any(|w| w == b"#EXTINF:")
}

fn is_generic_binary_ct(base: &str) -> bool {
    matches!(
        base,
        "application/octet-stream"
            | "binary/octet-stream"
            | "application/force-download"
            | "application/x-binary"
            | "application/download"
    )
}

pub fn infer_segment_content_type(upstream_ct: Option<&str>, final_url: &Url) -> String {
    let path_lower = final_url.path().to_ascii_lowercase();
    if let Some(ct) = upstream_ct {
        let trimmed = ct.trim();
        let (raw_base, params) = match trimmed.split_once(';') {
            Some((b, p)) => (b.trim(), Some(p.trim()).filter(|s| !s.is_empty())),
            None => (trimmed, None),
        };
        let base = raw_base.to_ascii_lowercase();
        if base == "application/mp4" {
            return if path_lower.ends_with(".m4a") {
                "audio/mp4".to_string()
            } else {
                "video/mp4".to_string()
            };
        }
        if !base.is_empty()
            && base != "text/plain"
            && !is_forbidden_media_ct(&base)
            && !is_generic_binary_ct(&base)
        {
            return match params {
                Some(p) => format!("{base}; {p}"),
                None => base,
            };
        }
    }
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

pub fn build_subtitle_proxy_url(
    target_url: &str,
    source_id: Option<uuid::Uuid>,
    token: Option<&str>,
) -> String {
    let mut out = format!(
        "/api/v1/video/online/subtitle-proxy?url={}",
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

pub fn is_forbidden_subtitle_ct(ct_lower: &str) -> bool {
    let base = ct_lower
        .split(';')
        .next()
        .unwrap_or(ct_lower)
        .trim()
        .to_ascii_lowercase();
    base.contains("text/html")
        || base.contains("application/xhtml")
        || base.contains("image/svg")
        || base.contains("javascript")
        || base == "application/xml"
        || base == "text/xml"
}

pub fn validate_and_normalize_subtitle_to_vtt(
    raw_bytes: &[u8],
    header_ct: Option<&str>,
) -> DomainResult<String> {
    if raw_bytes.len() > MAX_SUBTITLE_BYTES {
        return Err(DomainError::Validation(format!(
            "字幕文件体积超过 {} 字节限制",
            MAX_SUBTITLE_BYTES
        )));
    }
    if let Some(ct) = header_ct {
        let ct_lower = ct.to_ascii_lowercase();
        if is_forbidden_subtitle_ct(&ct_lower) {
            return Err(DomainError::Validation(format!(
                "拒绝转发不安全的字幕内容类型: {ct}"
            )));
        }
    }
    if raw_bytes.contains(&0) {
        return Err(DomainError::Validation(
            "字幕文件包含非法空字节，拒绝转发".into(),
        ));
    }
    if looks_like_html_bytes(raw_bytes) {
        return Err(DomainError::Validation(
            "拒绝转发伪装为字幕的 HTML/SVG/XML 内容".into(),
        ));
    }

    let utf8_text = std::str::from_utf8(raw_bytes)
        .map_err(|_| DomainError::Validation("字幕内容不是有效的 UTF-8 文本".into()))?;
    let without_bom = utf8_text.trim_start_matches('\u{feff}');
    let normalized = without_bom.replace("\r\n", "\n").replace('\r', "\n");
    let trimmed = normalized.trim();
    if trimmed.is_empty() {
        return Err(DomainError::Validation("字幕内容为空".into()));
    }

    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("<?")
        || lower.contains("<script")
        || lower.contains("<!doctype")
        || lower.contains("<html")
        || lower.contains("<svg")
        || lower.contains("<?xml")
    {
        return Err(DomainError::Validation(
            "拒绝包含 HTML/SVG/XML 脚本标签的字幕内容".into(),
        ));
    }

    let is_webvtt = trimmed
        .strip_prefix("WEBVTT")
        .is_some_and(|rest| rest.is_empty() || rest.starts_with([' ', '\t', '\n']));
    if is_webvtt {
        return Ok(format!("{}\n", normalized.trim_end_matches('\n')));
    }

    let mut cue_count = 0usize;
    let mut out_lines = Vec::new();
    for line in normalized.lines() {
        if let Some(caps) = RE_SRT_TIMESTAMP_LINE.captures(line) {
            cue_count += 1;
            out_lines.push(format!(
                "{}.{} --> {}.{}{}",
                &caps[1], &caps[2], &caps[3], &caps[4], &caps[5]
            ));
        } else {
            out_lines.push(line.to_string());
        }
    }
    if cue_count == 0 {
        return Err(DomainError::Validation(
            "不支持的字幕格式，仅支持 WebVTT 或 SRT".into(),
        ));
    }

    let mut vtt = String::from("WEBVTT\n\n");
    vtt.push_str(out_lines.join("\n").trim_end_matches('\n'));
    vtt.push('\n');
    Ok(vtt)
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
    let trimmed_start =
        playlist_text.trim_start_matches(|c: char| c == '\u{feff}' || c.is_ascii_whitespace());
    let clean_text = if trimmed_start.starts_with("#EXTM3U") {
        trimmed_start
    } else {
        playlist_text.trim_start_matches('\u{feff}')
    };
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

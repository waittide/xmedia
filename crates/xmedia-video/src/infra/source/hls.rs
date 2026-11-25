use std::borrow::Cow;
use std::sync::LazyLock;

use regex::Regex;
use url::Url;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_source::proxy::{
    DEFAULT_SUBTITLE_PROXY_MAX_BYTES, looks_like_html_or_svg_bytes, strip_bom_and_ascii_ws,
};

pub const MAX_M3U8_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_SUBTITLE_BYTES: usize = DEFAULT_SUBTITLE_PROXY_MAX_BYTES as usize;
pub const SNIFF_PREFIX_MAX_BYTES: usize = 4096;
pub const SNIFF_MIN_NON_WS_BYTES: usize = 16;

static RE_M3U8_URI_ATTR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)\bURI\s*=\s*(?:"([^"]*)"|'([^']*)'|([^,\s"']+))"#)
        .expect("valid m3u8 URI attr regex")
});
static RE_SRT_TIMESTAMP_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^\s*((?:\d{1,3}:)?\d{2}:\d{2})[,.](\d{3})\s*-->\s*((?:\d{1,3}:)?\d{2}:\d{2})[,.](\d{3})(.*)$",
    )
    .expect("valid srt timestamp regex")
});

pub(crate) fn is_utf16_bom_prefix(buf: &[u8]) -> bool {
    buf.starts_with(&[0xFF, 0xFE]) || buf.starts_with(&[0xFE, 0xFF])
}

pub(crate) fn has_binary_media_magic_or_null(buf: &[u8]) -> bool {
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

pub(crate) fn is_generic_binary_ct(base: &str) -> bool {
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

fn pad_vtt_timestamp_hms(ts: &str) -> Cow<'_, str> {
    if let Some((hours, rest)) = ts.split_once(':')
        && rest.contains(':')
        && hours.len() == 1
    {
        return Cow::Owned(format!("0{hours}:{rest}"));
    }
    Cow::Borrowed(ts)
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
        return Ok(format!("{trimmed}\n"));
    }

    let mut cue_count = 0usize;
    let mut out_lines = Vec::new();
    for line in normalized.lines() {
        if let Some(caps) = RE_SRT_TIMESTAMP_LINE.captures(line) {
            cue_count += 1;
            out_lines.push(format!(
                "{}.{} --> {}.{}{}",
                pad_vtt_timestamp_hms(&caps[1]),
                &caps[2],
                pad_vtt_timestamp_hms(&caps[3]),
                &caps[4],
                &caps[5]
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

/// Rewrites all HLS playlist URIs (`URI="..."` directive attributes and non-comment URI lines)
/// using the caller-supplied `rewrite_uri` callback so transport/infra stays decoupled from HTTP routes.
pub fn rewrite_m3u8_playlist_with<F>(
    playlist_text: &str,
    base_url: &Url,
    mut rewrite_uri: F,
) -> String
where
    F: FnMut(&str) -> String,
{
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
                        let raw_uri = caps
                            .get(1)
                            .or_else(|| caps.get(2))
                            .or_else(|| caps.get(3))
                            .map(|m| m.as_str())
                            .unwrap_or("");
                        if let Some(resolved) = resolve_m3u8_uri(raw_uri, base_url) {
                            let proxied = rewrite_uri(&resolved);
                            format!("URI=\"{proxied}\"")
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
            out.push_str(&rewrite_uri(&resolved));
        } else {
            out.push_str(line_no_cr);
        }
        out.push('\n');
    }

    out
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn rewrite_m3u8_playlist_handles_single_double_and_unquoted_uris() {
        let base = Url::parse("https://cdn.example.com/hls/index.m3u8").unwrap();
        let raw = "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI='enc.key'\n#EXT-X-MAP:URI=init.mp4\n#EXTINF:4.0,\nseg0.ts\n";
        let rewritten = rewrite_m3u8_playlist_with(raw, &base, |u| format!("PROXY({u})"));
        assert!(rewritten.contains("URI=\"PROXY(https://cdn.example.com/hls/enc.key)\""));
        assert!(rewritten.contains("URI=\"PROXY(https://cdn.example.com/hls/init.mp4)\""));
        assert!(rewritten.contains("PROXY(https://cdn.example.com/hls/seg0.ts)"));
    }
}

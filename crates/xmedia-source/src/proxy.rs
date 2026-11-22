use std::collections::HashMap;

use reqwest::header::{CONTENT_TYPE, HeaderValue, USER_AGENT};
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::infra::thumbnail::sniff_raster_image_mime;

use crate::http::{
    DEFAULT_WEBVIEW_UA, HttpClientConfig, build_sanitized_headers, get_or_build_async_client,
    validate_target_url_static,
};

pub const DEFAULT_IMAGE_PROXY_MAX_BYTES: usize = 8 * 1024 * 1024;
pub const DEFAULT_IMAGE_PROXY_TIMEOUT_SECS: u64 = 12;
pub const IMAGE_PROXY_CSP: &str = "sandbox; default-src 'none'; img-src 'self' data:";
pub const IMAGE_PROXY_CACHE_CONTROL: &str = "private, max-age=86400";

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

/// Fetches an image via `client` after static SSRF validation, attaches sanitized
/// source headers (defaulting `User-Agent` to `DEFAULT_WEBVIEW_UA` if omitted),
/// enforces `max_bytes` on both `Content-Length` and streamed chunks, validates
/// `Content-Type`, and sniffs magic bytes before returning `(bytes, mime_type)`.
pub async fn fetch_validated_image_bytes(
    client: &reqwest::Client,
    raw_url: &str,
    headers: Option<&HashMap<String, String>>,
    max_bytes: usize,
    allow_private_ips: bool,
) -> DomainResult<(Vec<u8>, &'static str)> {
    let validated = validate_target_url_static(raw_url.trim(), allow_private_ips)
        .map_err(DomainError::Validation)?;

    let mut header_map = headers.map(build_sanitized_headers).unwrap_or_default();
    if !header_map.contains_key(USER_AGENT) {
        header_map.insert(USER_AGENT, HeaderValue::from_static(DEFAULT_WEBVIEW_UA));
    }

    let mut resp = client
        .get(validated)
        .headers(header_map)
        .send()
        .await
        .map_err(|e| DomainError::Validation(format!("拉取图片失败: {e}")))?;

    if !resp.status().is_success() {
        return Err(DomainError::Validation(format!(
            "图片响应状态异常: {}",
            resp.status()
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
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| DomainError::Validation(format!("读取图片失败: {e}")))?
    {
        if bytes.len().saturating_add(chunk.len()) > max_bytes {
            return Err(DomainError::Validation(format!(
                "图片体积超过 {} 字节限制",
                max_bytes
            )));
        }
        bytes.extend_from_slice(&chunk);
    }

    let safe_mime = validate_safe_raster_image(header_ct.as_deref(), &bytes)?;
    Ok((bytes, safe_mime))
}

/// Convenience wrapper that resolves a pooled async client from `config`
/// (with `SsrfSafeResolver` + `ssrf_redirect_policy` when `!config.allow_private_ips`).
pub async fn fetch_validated_image_bytes_with_config(
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
    fetch_validated_image_bytes(
        &client,
        raw_url,
        headers,
        max_bytes,
        config.allow_private_ips,
    )
    .await
}

const MAX_UTF16_SNIFF_DECODE_BYTES: usize = 64 * 1024;
/// Extended sniff ceiling when leading bytes contain an unclosed XML prolog or HTML comment.
pub const MEDIA_PROLOG_SNIFF_MAX_BYTES: usize = 32 * 1024;

pub fn strip_bom_and_ascii_ws(mut bytes: &[u8]) -> &[u8] {
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

fn decode_utf16_bom_prefix(bytes: &[u8]) -> Option<Vec<u8>> {
    let (is_le, rest) = if bytes.starts_with(&[0xFF, 0xFE]) {
        (true, &bytes[2..])
    } else if bytes.starts_with(&[0xFE, 0xFF]) {
        (false, &bytes[2..])
    } else {
        return None;
    };
    let capped_len = rest.len().min(MAX_UTF16_SNIFF_DECODE_BYTES) & !1;
    let mut units = Vec::with_capacity(capped_len / 2);
    let (pairs, _) = rest[..capped_len].as_chunks::<2>();
    for &pair in pairs {
        let code = if is_le {
            u16::from_le_bytes(pair)
        } else {
            u16::from_be_bytes(pair)
        };
        units.push(code);
    }
    Some(String::from_utf16_lossy(&units).into_bytes())
}

fn strip_leading_xml_prolog_and_comments(mut cur: &[u8]) -> (&[u8], bool) {
    loop {
        if cur.starts_with(b"<?") {
            let Some(end) = cur.windows(2).position(|w| w == b"?>") else {
                return (cur, true);
            };
            cur = strip_bom_and_ascii_ws(&cur[end + 2..]);
            continue;
        }
        if cur.starts_with(b"<!--") {
            let Some(end) = cur.windows(3).position(|w| w == b"-->") else {
                return (cur, true);
            };
            cur = strip_bom_and_ascii_ws(&cur[end + 3..]);
            continue;
        }
        return (cur, false);
    }
}

/// Returns `true` when `buf` begins with an unclosed XML processing instruction or HTML comment
/// (including after UTF-8/UTF-16 BOM and leading ASCII whitespace).
pub fn has_unclosed_leading_xml_or_comment(buf: &[u8]) -> bool {
    let decoded = decode_utf16_bom_prefix(buf);
    let view = decoded.as_deref().unwrap_or(buf);
    let trimmed = strip_bom_and_ascii_ws(view);
    let (after_prolog, unclosed_prolog) = strip_leading_xml_prolog_and_comments(trimmed);
    unclosed_prolog && (after_prolog.starts_with(b"<!--") || after_prolog.starts_with(b"<?"))
}

/// Returns `true` when `buf` already has enough non-whitespace bytes past any leading
/// XML processing instructions (`<?xml ...?>`) or comments (`<!-- ... -->`) to reliably
/// sniff HTML/SVG/HLS headers.
pub fn has_enough_media_sniff_bytes(buf: &[u8], min_non_ws_bytes: usize) -> bool {
    let decoded = decode_utf16_bom_prefix(buf);
    let view = decoded.as_deref().unwrap_or(buf);
    let trimmed = strip_bom_and_ascii_ws(view);
    let (after_prolog, unclosed_prolog) = strip_leading_xml_prolog_and_comments(trimmed);
    !unclosed_prolog && after_prolog.len() >= min_non_ws_bytes
}

/// Returns `true` when `bytes` begins with HTML or SVG markup (including after leading
/// UTF-8/UTF-16 BOM, whitespace, `<?xml ...?>` declarations, or `<!-- ... -->` comments).
pub fn looks_like_html_or_svg_bytes(bytes: &[u8]) -> bool {
    let decoded = decode_utf16_bom_prefix(bytes);
    let view = decoded.as_deref().unwrap_or(bytes);
    let trimmed = strip_bom_and_ascii_ws(view);
    let (cur, unclosed_prolog) = strip_leading_xml_prolog_and_comments(trimmed);
    if unclosed_prolog {
        return cur.starts_with(b"<!--") || cur.starts_with(b"<?");
    }
    if cur.len() < 4 {
        return false;
    }
    let head: Vec<u8> = cur
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

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use axum::Router;
    use axum::body::Body;
    use axum::http::{HeaderMap, StatusCode};
    use axum::response::Response;
    use axum::routing::get;

    use super::*;

    const PNG_BYTES: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0];

    #[test]
    fn validate_safe_raster_image_accepts_all_7_raster_formats_and_rejects_svg_html() {
        let jpeg = [0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        let mut webp = b"RIFF\x00\x00\x00\x00WEBP".to_vec();
        webp.extend_from_slice(&[0u8; 4]);
        let gif = b"GIF89a\x01\x00\x01\x00";
        let mut bmp = b"BM".to_vec();
        bmp.extend_from_slice(&[0u8; 12]);
        let ico = [0x00, 0x00, 0x01, 0x00, 0x01, 0x00];
        let avif = b"\x00\x00\x00\x18ftypavif\x00\x00\x00\x00";

        assert_eq!(
            validate_safe_raster_image(Some("image/png"), PNG_BYTES).unwrap(),
            "image/png"
        );
        assert_eq!(
            validate_safe_raster_image(Some("application/octet-stream"), &jpeg).unwrap(),
            "image/jpeg"
        );
        assert_eq!(
            validate_safe_raster_image(None, &webp).unwrap(),
            "image/webp"
        );
        assert_eq!(
            validate_safe_raster_image(Some("image/gif"), gif).unwrap(),
            "image/gif"
        );
        assert_eq!(
            validate_safe_raster_image(Some("image/bmp"), &bmp).unwrap(),
            "image/bmp"
        );
        assert_eq!(
            validate_safe_raster_image(Some("image/x-icon"), &ico).unwrap(),
            "image/x-icon"
        );
        assert_eq!(
            validate_safe_raster_image(Some("image/avif"), avif).unwrap(),
            "image/avif"
        );

        let svg = b"<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>";
        let html = b"<html><body>xss</body></html>";
        assert!(validate_safe_raster_image(Some("image/svg+xml"), svg).is_err());
        assert!(validate_safe_raster_image(Some("image/jpeg"), svg).is_err());
        assert!(validate_safe_raster_image(Some("image/png"), html).is_err());
        assert!(validate_safe_raster_image(Some("text/html"), PNG_BYTES).is_err());
        assert!(validate_safe_raster_image(Some("image/svg+xml"), PNG_BYTES).is_err());
        assert!(validate_safe_raster_image(None, b"").is_err());
    }

    #[tokio::test]
    async fn fetch_validated_image_bytes_enforces_ssrf_size_cap_and_header_sanitization() {
        let captured: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));
        let captured_c = captured.clone();

        let app = Router::new()
            .route(
                "/ok.png",
                get(move |headers: HeaderMap| {
                    let cap = captured_c.clone();
                    async move {
                        let mut g = cap.lock().unwrap();
                        for (k, v) in &headers {
                            if let Ok(s) = v.to_str() {
                                g.insert(k.as_str().to_ascii_lowercase(), s.to_string());
                            }
                        }
                        Response::builder()
                            .status(StatusCode::OK)
                            .header("content-type", "image/png")
                            .body(Body::from(PNG_BYTES.to_vec()))
                            .unwrap()
                    }
                }),
            )
            .route(
                "/big-header.png",
                get(|| async {
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "image/png")
                        .header("content-length", "200")
                        .body(Body::from(vec![0u8; 200]))
                        .unwrap()
                }),
            )
            .route(
                "/big-stream.png",
                get(|| async {
                    let mut payload = PNG_BYTES.to_vec();
                    payload.extend_from_slice(&vec![1u8; 256]);
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "image/png")
                        .body(Body::from(payload))
                        .unwrap()
                }),
            )
            .route(
                "/evil.svg",
                get(|| async {
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "image/svg+xml")
                        .body(Body::from("<svg></svg>"))
                        .unwrap()
                }),
            )
            .route(
                "/missing.png",
                get(|| async { (StatusCode::NOT_FOUND, "not found") }),
            );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        // 1. SSRF rejection when allow_private_ips = false
        let ssrf_cfg = HttpClientConfig {
            timeout_secs: 5,
            allow_private_ips: false,
            proxy_url: None,
        };
        assert!(
            fetch_validated_image_bytes_with_config(
                &format!("http://{addr}/ok.png"),
                None,
                1024,
                &ssrf_cfg,
            )
            .await
            .is_err()
        );

        // 2. Allowed private IP + custom Referer forwarded + forbidden headers stripped
        let ok_cfg = HttpClientConfig {
            timeout_secs: 5,
            allow_private_ips: true,
            proxy_url: None,
        };
        let custom_headers = HashMap::from([
            (
                "Referer".to_string(),
                "https://comic.example.com/".to_string(),
            ),
            ("Host".to_string(), "spoofed.example.com".to_string()),
            ("Connection".to_string(), "close".to_string()),
        ]);
        let (bytes, mime) = fetch_validated_image_bytes_with_config(
            &format!("http://{addr}/ok.png"),
            Some(&custom_headers),
            1024,
            &ok_cfg,
        )
        .await
        .unwrap();
        assert_eq!(mime, "image/png");
        assert_eq!(bytes, PNG_BYTES);
        {
            let cap = captured.lock().unwrap();
            assert_eq!(
                cap.get("referer").map(String::as_str),
                Some("https://comic.example.com/")
            );
            assert_ne!(
                cap.get("host").map(String::as_str),
                Some("spoofed.example.com")
            );
        }

        // 3. Size cap (Content-Length & streamed body)
        assert!(
            fetch_validated_image_bytes_with_config(
                &format!("http://{addr}/big-header.png"),
                None,
                64,
                &ok_cfg,
            )
            .await
            .is_err()
        );
        assert!(
            fetch_validated_image_bytes_with_config(
                &format!("http://{addr}/big-stream.png"),
                None,
                64,
                &ok_cfg,
            )
            .await
            .is_err()
        );

        // 4. Non-200 status and SVG rejection
        assert!(
            fetch_validated_image_bytes_with_config(
                &format!("http://{addr}/missing.png"),
                None,
                1024,
                &ok_cfg,
            )
            .await
            .is_err()
        );
        assert!(
            fetch_validated_image_bytes_with_config(
                &format!("http://{addr}/evil.svg"),
                None,
                1024,
                &ok_cfg,
            )
            .await
            .is_err()
        );

        server.abort();
    }

    #[test]
    fn html_or_svg_sniffing_catches_xml_prologs_comments_and_split_chunks() {
        assert!(looks_like_html_or_svg_bytes(
            b"<?xml version=\"1.0\" encoding=\"UTF-8\"?><svg xmlns=\"http://www.w3.org/2000/svg\"/>"
        ));
        assert!(looks_like_html_or_svg_bytes(
            b"\xEF\xBB\xBF \n<?xml version=\"1.0\"?>\n<!-- c --><!DOCTYPE html><html></html>"
        ));
        assert!(!has_enough_media_sniff_bytes(
            b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>",
            16
        ));
        assert!(has_enough_media_sniff_bytes(
            b"<?xml version=\"1.0\" encoding=\"UTF-8\"?><svg xmlns=\"http://www.w3.org/2000/svg\">",
            16
        ));
        assert!(has_enough_media_sniff_bytes(
            b"ID3\x03\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00",
            16
        ));
        assert!(!looks_like_html_or_svg_bytes(
            b"ID3\x03\x00\x00\x00\x00\x00\x00"
        ));
    }

    #[test]
    fn html_or_svg_sniffing_catches_utf16_bom_and_large_leading_comments() {
        let mut utf16_le = vec![0xFF, 0xFE];
        for unit in "<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>".encode_utf16() {
            utf16_le.extend_from_slice(&unit.to_le_bytes());
        }
        assert!(looks_like_html_or_svg_bytes(&utf16_le));
        assert!(has_enough_media_sniff_bytes(&utf16_le, 16));

        let mut utf16_be = vec![0xFE, 0xFF];
        for unit in "  <?xml version=\"1.0\"?><html></html>".encode_utf16() {
            utf16_be.extend_from_slice(&unit.to_be_bytes());
        }
        assert!(looks_like_html_or_svg_bytes(&utf16_be));

        let mut huge_comment = b"<!-- ".to_vec();
        huge_comment.extend_from_slice(&vec![b'X'; 4600]);
        assert!(has_unclosed_leading_xml_or_comment(&huge_comment));
        assert!(!has_enough_media_sniff_bytes(&huge_comment, 16));
        assert!(looks_like_html_or_svg_bytes(&huge_comment));

        huge_comment.extend_from_slice(b" --><svg xmlns=\"http://www.w3.org/2000/svg\"/>");
        assert!(!has_unclosed_leading_xml_or_comment(&huge_comment));
        assert!(has_enough_media_sniff_bytes(&huge_comment, 16));
        assert!(looks_like_html_or_svg_bytes(&huge_comment));

        // Valid media headers must never trigger false positives.
        assert!(!looks_like_html_or_svg_bytes(
            b"\xFF\xFB\x90\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00"
        ));
        assert!(!looks_like_html_or_svg_bytes(
            b"\x00\x00\x00\x18ftypisom\x00\x00\x02\x00isom"
        ));
        assert!(!looks_like_html_or_svg_bytes(
            b"OggS\x00\x02\x00\x00\x00\x00\x00\x00\x00\x00"
        ));
        assert!(!looks_like_html_or_svg_bytes(
            b"fLaC\x00\x00\x00\x22\x10\x00\x10\x00"
        ));
        assert!(!looks_like_html_or_svg_bytes(
            b"RIFF\x24\x00\x00\x00WAVEfmt "
        ));
    }
}

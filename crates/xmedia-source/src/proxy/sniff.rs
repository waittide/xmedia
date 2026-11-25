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

pub fn is_only_bom_or_whitespace(buf: &[u8]) -> bool {
    if matches!(buf, [0xEF] | [0xEF, 0xBB] | [0xFF] | [0xFE]) {
        return true;
    }
    let decoded = decode_utf16_bom_prefix(buf);
    let view = decoded.as_deref().unwrap_or(buf);
    strip_bom_and_ascii_ws(view).is_empty()
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

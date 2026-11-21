use url::Url;

use super::{MAX_SANITIZED_FILENAME_CHARS, OpdsError};
use crate::remote_sources::is_same_origin;

pub(super) fn resolve_target_url(
    source_base: &str,
    target_opt: Option<&str>,
) -> Result<String, OpdsError> {
    let base = Url::parse(source_base)
        .map_err(|e| OpdsError::BadRequest(format!("Invalid source URL: {e}")))?;
    if !matches!(base.scheme(), "http" | "https") {
        return Err(OpdsError::BadRequest(format!(
            "Unsupported source URL scheme '{}'",
            base.scheme()
        )));
    }
    if !base.username().is_empty() || base.password().is_some() {
        return Err(OpdsError::BadRequest(
            "Source URL must not contain embedded credentials".to_string(),
        ));
    }

    let resolved = match target_opt.map(str::trim).filter(|s| !s.is_empty()) {
        Some(raw) => {
            if raw.starts_with("//") {
                return Err(OpdsError::BadRequest(
                    "Protocol-relative URLs are not allowed".to_string(),
                ));
            }
            if raw.contains("://") && !raw.starts_with("http://") && !raw.starts_with("https://") {
                return Err(OpdsError::BadRequest(
                    "Unsupported target URL scheme".to_string(),
                ));
            }
            let parsed = if raw.starts_with("http://") || raw.starts_with("https://") {
                Url::parse(raw)
                    .map_err(|e| OpdsError::BadRequest(format!("Invalid target URL: {e}")))?
            } else {
                base.join(raw)
                    .map_err(|e| OpdsError::BadRequest(format!("Invalid relative URL: {e}")))?
            };
            if !matches!(parsed.scheme(), "http" | "https") {
                return Err(OpdsError::BadRequest(format!(
                    "Unsupported URL scheme '{}'",
                    parsed.scheme()
                )));
            }
            if !parsed.username().is_empty() || parsed.password().is_some() {
                return Err(OpdsError::BadRequest(
                    "Target URL must not contain embedded credentials".to_string(),
                ));
            }
            if !is_same_origin(&base, &parsed) {
                return Err(OpdsError::BadRequest(
                    "Target URL must match the configured OPDS source origin".to_string(),
                ));
            }
            parsed
        }
        None => base,
    };

    Ok(resolved.to_string())
}

pub(super) fn resolve_link_url(base: &Url, href: &str) -> String {
    let trimmed = href.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return trimmed.to_string();
    }
    if let Some(brace_idx) = trimmed.find('{') {
        let (prefix, suffix) = trimmed.split_at(brace_idx);
        if let Ok(resolved_prefix) = base.join(prefix) {
            return format!("{resolved_prefix}{suffix}");
        }
    }
    base.join(trimmed)
        .map(|u| u.to_string())
        .unwrap_or_else(|_| trimmed.to_string())
}

fn encode_query_component(raw: &str) -> String {
    form_urlencoded::byte_serialize(raw.as_bytes()).collect()
}

pub(super) fn proxy_image_path(source_id: &str, abs_url: &str) -> String {
    format!(
        "/_app/opds/{source_id}/proxy-image?url={}",
        encode_query_component(abs_url)
    )
}

fn strip_optional_template_params(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{'
            && let Some(rel_end) = raw[i..].find('}')
        {
            let token = &raw[i + 1..i + rel_end];
            if token.ends_with('?') {
                // Only trim a dangling "&key=" or "?key=" immediately preceding the optional token.
                if out.ends_with('=') {
                    let eq_idx = out.len() - 1;
                    let prefix = &out[..eq_idx];
                    let sep_idx = prefix.rfind(['&', '?']);
                    let key_start = sep_idx.map(|idx| idx + 1).unwrap_or(0);
                    let key_slice = &out[key_start..eq_idx];
                    if !key_slice.is_empty()
                        && key_slice
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
                    {
                        match sep_idx {
                            Some(idx) if out.as_bytes()[idx] == b'&' => {
                                out.truncate(idx);
                            }
                            Some(idx) if out.as_bytes()[idx] == b'?' => {
                                // Keep '?' if more query parameters follow.
                                out.truncate(idx + 1);
                            }
                            _ => {}
                        }
                    }
                }
                i += rel_end + 1;
                // Avoid "?&" when the first query param was removed.
                if out.ends_with('?') && i < bytes.len() && bytes[i] == b'&' {
                    i += 1;
                }
                continue;
            }
        }
        let ch = raw[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    while out.ends_with('&') || out.ends_with('?') {
        out.pop();
    }
    out
}

pub(super) fn expand_search_template(template: &str, query: &str) -> String {
    let encoded = encode_query_component(query);
    if template.contains("{searchTerms}") || template.contains("{searchTerms?}") {
        let replaced = template
            .replace("{searchTerms}", &encoded)
            .replace("{searchTerms?}", &encoded);
        return strip_optional_template_params(&replaced);
    }
    // Support RFC 6570 OPDS 2.0 templates like "/v2/search{?query}", "/v2/search{?q}", "/v2/search?lang=en{&query}"
    if let Some((start, marker_len)) = template
        .find("{?")
        .map(|i| (i, 2))
        .or_else(|| template.find("{&").map(|i| (i, 2)))
        && let Some(rel_end) = template[start..].find('}')
    {
        let end = start + rel_end;
        let vars = &template[start + marker_len..end];
        let param_name = vars
            .split(',')
            .map(str::trim)
            .map(|v| v.trim_end_matches('*').split(':').next().unwrap_or(v))
            .find(|v| !v.is_empty())
            .unwrap_or("query");
        let prefix = &template[..start];
        let suffix = &template[end + 1..];
        let sep = if prefix.contains('?') { '&' } else { '?' };
        let combined = format!("{prefix}{sep}{param_name}={encoded}{suffix}");
        return strip_optional_template_params(&combined);
    }
    append_query_param(template, "q", query)
}

pub(super) fn append_query_param(url: &str, key: &str, val: &str) -> String {
    let sep = if url.contains('?') { '&' } else { '?' };
    format!("{url}{sep}{key}={}", encode_query_component(val))
}

pub(super) fn detect_format_from_mime_or_href(media_type: &str, href: &str) -> Option<String> {
    let mime = media_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    match mime.as_str() {
        "application/epub+zip" | "application/epub" => return Some("epub".to_string()),
        "application/pdf" => return Some("pdf".to_string()),
        "text/plain" => return Some("txt".to_string()),
        "application/vnd.comicbook+zip" | "application/x-cbz" | "application/cbz" => {
            return Some("cbz".to_string());
        }
        "application/zip" | "application/x-zip-compressed" => return Some("zip".to_string()),
        _ => {}
    }

    let path_part = Url::parse(href)
        .ok()
        .map(|u| u.path().to_string())
        .unwrap_or_else(|| href.split('?').next().unwrap_or(href).to_string());
    let ext = std::path::Path::new(&path_part)
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase());
    match ext.as_deref() {
        Some("epub") => Some("epub".to_string()),
        Some("pdf") => Some("pdf".to_string()),
        Some("txt") => Some("txt".to_string()),
        Some("cbz") => Some("cbz".to_string()),
        Some("zip") => Some("zip".to_string()),
        _ => None,
    }
}

/// Decodes RFC 3986 / RFC 5987 percent-encoded UTF-8 strings WITHOUT turning `+` into spaces.
pub(crate) fn percent_decode_utf8(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (hex_nibble(bytes[i + 1]), hex_nibble(bytes[i + 2]))
        {
            out.push((hi << 4) | lo);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

pub(crate) fn parse_filename_from_content_disposition(header: &str) -> Option<String> {
    let mut fallback: Option<String> = None;
    for part in header.split(';').map(str::trim) {
        let lower = part.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("filename*=") {
            let orig_val = &part[part.len() - rest.len()..];
            let encoded = orig_val
                .split_once("''")
                .map(|(_, v)| v)
                .unwrap_or(orig_val)
                .trim_matches('"');
            let decoded = percent_decode_utf8(encoded);
            if !decoded.trim().is_empty() {
                return Some(decoded);
            }
        }
        if fallback.is_none()
            && let Some(rest) = lower.strip_prefix("filename=")
        {
            let orig_val = &part[part.len() - rest.len()..];
            let cleaned = orig_val.trim().trim_matches('"').trim();
            if !cleaned.is_empty() {
                fallback = Some(cleaned.to_string());
            }
        }
    }
    fallback
}

pub(crate) fn is_windows_reserved_stem(stem: &str) -> bool {
    let upper = stem.trim().to_ascii_uppercase();
    matches!(
        upper.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}

pub(crate) fn sanitize_filename_component(raw: &str) -> String {
    let leaf = raw.rsplit(['/', '\\']).next().unwrap_or(raw);
    let mut cleaned = String::with_capacity(leaf.len());
    for ch in leaf.chars() {
        if ch.is_control() {
            continue;
        }
        if matches!(ch, ':' | '*' | '?' | '"' | '<' | '>' | '|') {
            cleaned.push('_');
        } else {
            cleaned.push(ch);
        }
    }
    let trimmed = cleaned.trim().trim_matches('.');
    if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
        return String::new();
    }

    let mut result = trimmed.to_string();
    let stem = std::path::Path::new(&result)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(&result);
    if is_windows_reserved_stem(stem) {
        result = format!("_{result}");
    }

    if result.chars().count() > MAX_SANITIZED_FILENAME_CHARS {
        let ext = std::path::Path::new(&result)
            .extension()
            .and_then(|s| s.to_str())
            .filter(|e| e.len() <= 16)
            .map(str::to_string);
        if let Some(ext) = ext {
            let suffix = format!(".{ext}");
            let max_stem_chars =
                MAX_SANITIZED_FILENAME_CHARS.saturating_sub(suffix.chars().count());
            let stem_trunc: String = result
                .strip_suffix(&suffix)
                .unwrap_or(&result)
                .chars()
                .take(max_stem_chars)
                .collect();
            result = format!("{}{}", stem_trunc.trim_end_matches([' ', '.']), suffix);
        } else {
            result = result
                .chars()
                .take(MAX_SANITIZED_FILENAME_CHARS)
                .collect::<String>()
                .trim_end_matches([' ', '.'])
                .to_string();
        }
    }

    result
}

fn sanitize_filename(raw: &str) -> String {
    let cleaned = sanitize_filename_component(raw);
    if cleaned.is_empty() {
        "import".to_string()
    } else {
        cleaned
    }
}

pub(super) fn resolve_import_filename_and_format(
    filename_hint: Option<&str>,
    content_disposition: Option<&str>,
    content_type: &str,
    final_url: &Url,
) -> Result<(String, String), OpdsError> {
    let candidate = filename_hint
        .map(sanitize_filename_component)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            content_disposition
                .and_then(parse_filename_from_content_disposition)
                .map(|s| sanitize_filename_component(&s))
                .filter(|s| !s.is_empty())
        })
        .or_else(|| {
            final_url
                .path_segments()
                .and_then(|mut segs| segs.next_back())
                .filter(|s| !s.is_empty() && *s != "download")
                .map(percent_decode_utf8)
                .map(|s| sanitize_filename_component(&s))
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| sanitize_filename("remote-import"));

    let ext_from_name = std::path::Path::new(&candidate)
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .filter(|e| matches!(e.as_str(), "epub" | "pdf" | "txt" | "cbz" | "zip"));

    let format = ext_from_name
        .clone()
        .or_else(|| detect_format_from_mime_or_href(content_type, final_url.as_str()))
        .ok_or_else(|| {
            OpdsError::Unsupported(format!(
                "Could not determine supported book/comic format from filename '{candidate}' or Content-Type '{content_type}'"
            ))
        })?;

    let final_filename = if ext_from_name.is_some() {
        candidate
    } else {
        format!("{candidate}.{format}")
    };

    Ok((final_filename, format))
}

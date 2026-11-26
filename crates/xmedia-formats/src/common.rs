use std::cmp::Ordering;
use std::io::{Cursor, Read};
use std::path::{Component, Path, PathBuf};

use blake3::Hasher as Blake3Hasher;
use chardetng::{EncodingDetector, Iso2022JpDetection, Utf8Detection};
use encoding_rs::{GB18030, UTF_8, UTF_16BE, UTF_16LE};
use image::ImageReader;
use quick_xml::escape::escape;
use quick_xml::events::{BytesRef, BytesStart};
use regex::{Regex, RegexBuilder};
use sha2::{Digest, Sha256};

use crate::error::{FormatError, FormatResult};

/// Raw binary media/cover/resource payload with MIME type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinaryAsset {
    pub bytes: Vec<u8>,
    pub mime_type: String,
}

impl BinaryAsset {
    pub fn new(bytes: Vec<u8>, mime_type: impl Into<String>) -> Self {
        Self {
            bytes,
            mime_type: mime_type.into(),
        }
    }
}

/// Decodes arbitrary text bytes (UTF-8, UTF-16 LE/BE with BOM, GBK/GB18030, Big5, Shift-JIS, etc.)
/// into UTF-8 along with the detected encoding label.
pub fn decode_text_smart(bytes: &[u8]) -> (String, &'static str) {
    if bytes.is_empty() {
        return (String::new(), "UTF-8");
    }
    if let Some(stripped) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        let (cow, _, _) = UTF_8.decode(stripped);
        return (cow.into_owned(), "UTF-8");
    }
    if let Some(stripped) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let (cow, _, _) = UTF_16LE.decode(stripped);
        return (cow.into_owned(), "UTF-16LE");
    }
    if let Some(stripped) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        let (cow, _, _) = UTF_16BE.decode(stripped);
        return (cow.into_owned(), "UTF-16BE");
    }
    if let Ok(utf8) = std::str::from_utf8(bytes) {
        return (utf8.to_owned(), "UTF-8");
    }

    let mut detector = EncodingDetector::new(Iso2022JpDetection::Allow);
    let sample_len = bytes.len().min(64 * 1024);
    detector.feed(&bytes[..sample_len], sample_len == bytes.len());
    let guessed = detector.guess(Some(b"cn"), Utf8Detection::Allow);
    let effective = if guessed == UTF_8 || guessed == encoding_rs::GBK {
        GB18030
    } else {
        guessed
    };
    let (cow, used, _) = effective.decode(bytes);
    (cow.into_owned(), used.name())
}

/// Sniffs safe raster image formats (JPEG, PNG, WebP, GIF, BMP, ICO, AVIF) by magic bytes.
/// Explicitly excludes SVG/XML/HTML.
pub fn sniff_raster_image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("image/jpeg");
    }
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Some("image/png");
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    if bytes.len() >= 14 && &bytes[0..2] == b"BM" {
        return Some("image/bmp");
    }
    if bytes.len() >= 6
        && bytes[0..4] == [0x00, 0x00, 0x01, 0x00]
        && (bytes[4] != 0 || bytes[5] != 0)
    {
        return Some("image/x-icon");
    }
    if bytes.len() >= 12
        && &bytes[4..8] == b"ftyp"
        && matches!(&bytes[8..12], b"avif" | b"avis" | b"mif1")
    {
        return Some("image/avif");
    }
    None
}

/// Sniffs image MIME type with `image/jpeg` fallback for legacy artwork slots.
pub fn sniff_image_mime(bytes: &[u8]) -> &'static str {
    sniff_raster_image_mime(bytes).unwrap_or("image/jpeg")
}

/// Canonical file extension for a raster image MIME type.
pub fn ext_for_raster_mime(mime: &str) -> Option<&'static str> {
    let base = mime
        .split(';')
        .next()
        .unwrap_or(mime)
        .trim()
        .to_ascii_lowercase();
    match base.as_str() {
        "image/jpeg" | "image/jpg" => Some("jpg"),
        "image/png" => Some("png"),
        "image/webp" => Some("webp"),
        "image/gif" => Some("gif"),
        "image/avif" => Some("avif"),
        "image/bmp" => Some("bmp"),
        _ => None,
    }
}

/// Probes `(width, height)` from image header bytes without decoding the full pixel buffer.
pub fn probe_image_dimensions_bytes(bytes: &[u8]) -> Option<(u32, u32)> {
    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    reader.into_dimensions().ok()
}

/// Probes `(width, height)` from a `Read` stream by reading at most the image header
/// (256 KiB fast path, 4 MiB fallback for large EXIF/ICC blocks) without decompressing full pages.
pub fn probe_reader_dimensions<R: Read>(mut reader: R, size_hint: u64) -> Option<(u32, u32)> {
    const HEADER_PROBE_BYTES: u64 = 256 * 1024;
    const EXIF_FALLBACK_BYTES: u64 = 4 * 1024 * 1024;

    let mut buf = Vec::with_capacity(size_hint.min(64 * 1024) as usize);
    if reader
        .by_ref()
        .take(HEADER_PROBE_BYTES)
        .read_to_end(&mut buf)
        .is_err()
    {
        return None;
    }
    if let Some(dims) = probe_image_dimensions_bytes(&buf) {
        return Some(dims);
    }
    if (buf.len() as u64) == HEADER_PROBE_BYTES
        && reader
            .take(EXIF_FALLBACK_BYTES - HEADER_PROBE_BYTES)
            .read_to_end(&mut buf)
            .is_ok()
    {
        return probe_image_dimensions_bytes(&buf);
    }
    None
}

/// Escapes XML special characters while stripping illegal XML 1.0 control characters.
pub fn xml_escape_clean(raw: &str) -> String {
    if raw
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        let cleaned: String = raw
            .chars()
            .filter(|&c| !c.is_control() || matches!(c, '\n' | '\r' | '\t'))
            .collect();
        escape(&cleaned).into_owned()
    } else {
        escape(raw).into_owned()
    }
}

/// Resolves a `quick_xml` general entity / character reference into `out`.
pub fn append_xml_ref(out: &mut String, r: &BytesRef<'_>) {
    if let Ok(Some(ch)) = r.resolve_char_ref() {
        out.push(ch);
        return;
    }
    if let Some(resolved) = quick_xml::escape::resolve_xml_entity(r.as_ref()) {
        out.push_str(resolved);
    } else {
        out.push('&');
        out.push_str(r.as_ref());
        out.push(';');
    }
}

/// Reads an XML attribute value matching `local_key` (case-sensitively by local name).
pub fn xml_attr_value(e: &BytesStart<'_>, local_key: &str) -> Option<String> {
    for attr in e.attributes().flatten() {
        if attr.key.local_name().as_ref() == local_key
            && let Ok(val) = attr.normalized_value(quick_xml::XmlVersion::Implicit1_0)
        {
            return Some(val.into_owned());
        }
    }
    None
}

/// Reads an XML attribute value matching `local_key` case-insensitively.
pub fn xml_attr_value_ci(e: &BytesStart<'_>, local_key: &str) -> Option<String> {
    for attr in e.attributes().flatten() {
        if attr
            .key
            .local_name()
            .as_ref()
            .eq_ignore_ascii_case(local_key)
            && let Ok(val) = attr.normalized_value(quick_xml::XmlVersion::Implicit1_0)
        {
            return Some(val.into_owned());
        }
    }
    None
}

/// Compiles a user-provided regular expression with strict DFA/NFA memory caps (`1 MiB`).
pub fn compile_bounded_regex(pattern: &str, label: &str) -> FormatResult<Regex> {
    RegexBuilder::new(pattern)
        .size_limit(1 << 20)
        .dfa_size_limit(1 << 20)
        .build()
        .map_err(|e| FormatError::Validation(format!("{label} 无效或复杂度过高: {e}")))
}

/// Formats a floating-point index/volume (`1.0` -> `"1"`, `1.5` -> `"1.5"`).
pub fn format_clean_float(val: f64) -> String {
    if !val.is_finite() {
        return String::new();
    }
    if (val - val.round()).abs() < 1e-6 {
        format!("{}", val.round() as i64)
    } else {
        let formatted = format!("{val:.4}");
        formatted
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
}

/// Computes lowercase hex SHA-256 digest of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

/// Computes lowercase hex BLAKE3 digest of `bytes`.
pub fn blake3_hex(bytes: &[u8]) -> String {
    let mut hasher = Blake3Hasher::new();
    hasher.update(bytes);
    hasher.finalize().to_hex().to_string()
}

fn decode_percent_path(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (
                (bytes[i + 1] as char).to_digit(16),
                (bytes[i + 2] as char).to_digit(16),
            )
        {
            out.push(((hi << 4) | lo) as u8);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Normalizes a relative archive path (stripping query/fragment, percent-decoding, and resolving `..` safely).
pub fn normalize_zip_path(base_dir: &Path, relative: &str) -> String {
    let without_frag = relative.split('#').next().unwrap_or(relative);
    let without_query = without_frag.split('?').next().unwrap_or(without_frag);
    let decoded = decode_percent_path(without_query);
    let joined = base_dir.join(decoded);
    let mut out = PathBuf::new();
    for comp in joined.components() {
        match comp {
            Component::Normal(part) => out.push(part),
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir | Component::RootDir | Component::Prefix(_) => {}
        }
    }
    out.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

#[derive(Debug, PartialEq, Eq)]
struct NumberToken {
    int_part: u128,
    frac_part: String,
    raw_len: usize,
}

fn take_number_tokens<I>(iter: &mut std::iter::Peekable<I>) -> NumberToken
where
    I: Iterator<Item = char> + Clone,
{
    let mut raw_int = String::new();
    while let Some(&c) = iter.peek() {
        if c.is_ascii_digit() {
            raw_int.push(c);
            iter.next();
        } else {
            break;
        }
    }

    let mut frac_part = String::new();
    let mut clone = iter.clone();
    if clone.next() == Some('.') && clone.peek().is_some_and(|c| c.is_ascii_digit()) {
        iter.next();
        while let Some(&c) = iter.peek() {
            if c.is_ascii_digit() {
                frac_part.push(c);
                iter.next();
            } else {
                break;
            }
        }
    }

    NumberToken {
        int_part: raw_int.parse::<u128>().unwrap_or(u128::MAX),
        frac_part,
        raw_len: raw_int.len(),
    }
}

fn cmp_number_tokens(a: &NumberToken, b: &NumberToken) -> Ordering {
    match a.int_part.cmp(&b.int_part) {
        Ordering::Equal => {}
        non_eq => return non_eq,
    }
    let fa = a.frac_part.trim_end_matches('0');
    let fb = b.frac_part.trim_end_matches('0');
    match fa.cmp(fb) {
        Ordering::Equal => a.raw_len.cmp(&b.raw_len),
        non_eq => non_eq,
    }
}

/// Fractional-aware natural string comparator (`1 < 1.5 < 2 < 10`, case-insensitive).
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let mut ia = a.chars().peekable();
    let mut ib = b.chars().peekable();

    loop {
        match (ia.peek(), ib.peek()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(&ca), Some(&cb)) => {
                if ca.is_ascii_digit() && cb.is_ascii_digit() {
                    let na = take_number_tokens(&mut ia);
                    let nb = take_number_tokens(&mut ib);
                    match cmp_number_tokens(&na, &nb) {
                        Ordering::Equal => continue,
                        non_eq => return non_eq,
                    }
                } else {
                    let la = ca.to_lowercase().next().unwrap_or(ca);
                    let lb = cb.to_lowercase().next().unwrap_or(cb);
                    ia.next();
                    ib.next();
                    match la.cmp(&lb) {
                        Ordering::Equal => continue,
                        non_eq => return non_eq,
                    }
                }
            }
        }
    }
}

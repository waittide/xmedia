use std::sync::LazyLock;

use encoding_rs::{UTF_8, WINDOWS_1252};
use regex::Regex;

use super::txt::segment_txt_structure;
use super::{BookFormatExt, BookTextChapter, BookTocEntry, ParsedBookMeta, ParsedBookPackage};
use crate::common::{BinaryAsset, decode_text_smart, sniff_raster_image_mime};
use crate::error::{FormatError, FormatResult};

const MAX_MOBI_DECOMPRESSED_BYTES: usize = 64 * 1024 * 1024;

static RE_HTML_BLOCK_BREAK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)<\s*(?:/\s*(?:p|div|h[1-6]|li|tr|blockquote)|br\s*/?|hr\s*/?)\s*>")
        .expect("valid html block regex")
});
static RE_MOBI_PAGEBREAK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)<\s*mbp:pagebreak[^>]*/?\s*>").expect("valid mbp pagebreak regex")
});
static RE_HTML_HEADING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?is)<\s*h([1-3])\b[^>]*>(.*?)</\s*h[1-3]\s*>").expect("valid heading regex")
});
static RE_SCRIPT_STYLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?is)<\s*(?:script|style|head)\b[^>]*>.*?</\s*(?:script|style|head)\s*>")
        .expect("valid script/style regex")
});

fn be_u16(buf: &[u8], off: usize) -> Option<u16> {
    let bytes: [u8; 2] = buf.get(off..off + 2)?.try_into().ok()?;
    Some(u16::from_be_bytes(bytes))
}

fn be_u32(buf: &[u8], off: usize) -> Option<u32> {
    let bytes: [u8; 4] = buf.get(off..off + 4)?.try_into().ok()?;
    Some(u32::from_be_bytes(bytes))
}

fn parse_pdb_records(data: &[u8]) -> FormatResult<Vec<&[u8]>> {
    if data.len() < 78 {
        return Err(FormatError::Validation(
            "invalid MOBI/AZW3 file: header shorter than 78 bytes".into(),
        ));
    }
    let magic = &data[60..68];
    if magic != b"BOOKMOBI" && magic != b"TEXtREAd" {
        return Err(FormatError::Validation(
            "invalid MOBI/AZW3 file: missing BOOKMOBI/TEXtREAd signature".into(),
        ));
    }
    let num_records = be_u16(data, 76).unwrap_or(0) as usize;
    let table_end = 78usize.saturating_add(num_records.saturating_mul(8));
    if num_records == 0 || data.len() < table_end {
        return Err(FormatError::Validation(
            "invalid MOBI/AZW3 file: truncated PalmDB record table".into(),
        ));
    }
    let mut offsets = Vec::with_capacity(num_records);
    for i in 0..num_records {
        let off = be_u32(data, 78 + i * 8).unwrap_or(0) as usize;
        if off > data.len() {
            return Err(FormatError::Validation(
                "invalid MOBI/AZW3 file: out-of-bounds record offset".into(),
            ));
        }
        offsets.push(off);
    }
    let mut records = Vec::with_capacity(num_records);
    for i in 0..num_records {
        let start = offsets[i];
        let end = offsets
            .get(i + 1)
            .copied()
            .unwrap_or(data.len())
            .max(start)
            .min(data.len());
        records.push(&data[start..end]);
    }
    Ok(records)
}

fn trim_mobi_trailing_data(mut rec: &[u8], extra_flags: u16) -> &[u8] {
    for bit in (1..16).rev() {
        if (extra_flags & (1 << bit)) != 0 {
            let mut size = 0usize;
            let mut shift = 0u32;
            let mut idx = rec.len();
            while idx > 0 {
                idx -= 1;
                let b = rec[idx];
                size |= ((b & 0x7F) as usize) << shift;
                if (b & 0x80) != 0 || shift >= 21 {
                    break;
                }
                shift += 7;
            }
            if size > 0 && size <= rec.len() {
                rec = &rec[..rec.len() - size];
            }
        }
    }
    if (extra_flags & 1) != 0 && !rec.is_empty() {
        let skip = ((rec[rec.len() - 1] & 0x03) as usize) + 1;
        if skip <= rec.len() {
            rec = &rec[..rec.len() - skip];
        }
    }
    rec
}

/// Decompresses a PalmDOC LZ77-compressed record into `out`.
pub fn decompress_palmdoc_lz77(src: &[u8], out: &mut Vec<u8>) -> FormatResult<()> {
    let mut i = 0usize;
    while i < src.len() {
        if out.len() >= MAX_MOBI_DECOMPRESSED_BYTES {
            return Err(FormatError::Validation(
                "MOBI/AZW3 text exceeds maximum decompressed size".into(),
            ));
        }
        let b = src[i];
        i += 1;
        match b {
            0x00 => out.push(0),
            0x01..=0x08 => {
                let count = b as usize;
                let end = (i + count).min(src.len());
                out.extend_from_slice(&src[i..end]);
                i = end;
            }
            0x09..=0x7F => out.push(b),
            0x80..=0xBF => {
                if i >= src.len() {
                    break;
                }
                let next = src[i];
                i += 1;
                let pair = (((b as u16) << 8) | (next as u16)) & 0x3FFF;
                let dist = (pair >> 3) as usize;
                let len = ((pair & 0x07) as usize) + 3;
                if dist == 0 || dist > out.len() {
                    continue;
                }
                for _ in 0..len {
                    let copied = out[out.len() - dist];
                    out.push(copied);
                }
            }
            0xC0..=0xFF => {
                out.push(b' ');
                out.push(b ^ 0x80);
            }
        }
    }
    Ok(())
}

fn decode_mobi_str(bytes: &[u8], text_encoding: u32) -> String {
    let clean_bytes = bytes.split(|&b| b == 0).next().unwrap_or(bytes);
    let s = match text_encoding {
        65001 => UTF_8.decode(clean_bytes).0.into_owned(),
        1252 => WINDOWS_1252.decode(clean_bytes).0.into_owned(),
        _ => decode_text_smart(clean_bytes).0,
    };
    s.trim().to_string()
}

fn strip_html_entities_and_tags(raw_html: &str) -> String {
    let without_scripts = RE_SCRIPT_STYLE.replace_all(raw_html, "");
    let with_breaks = RE_HTML_BLOCK_BREAK.replace_all(&without_scripts, "\n");
    let mut out = String::with_capacity(with_breaks.len());
    let mut in_tag = false;
    for ch in with_breaks.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.replace("&nbsp;", " ")
        .replace("&#160;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
}

struct MobiHeaderInfo {
    compression: u16,
    record_count: usize,
    text_encoding: u32,
    first_image_index: Option<usize>,
    extra_data_flags: u16,
    cover_offset: Option<usize>,
    thumb_offset: Option<usize>,
    kf8_boundary_rec: Option<usize>,
}

fn parse_record0_header(
    rec0: &[u8],
    pdb_name_bytes: &[u8],
    meta: &mut ParsedBookMeta,
) -> FormatResult<MobiHeaderInfo> {
    if rec0.len() < 16 {
        return Err(FormatError::Validation(
            "invalid MOBI/AZW3 record 0: shorter than 16 bytes".into(),
        ));
    }
    let compression = be_u16(rec0, 0).unwrap_or(1);
    let record_count = be_u16(rec0, 8).unwrap_or(0) as usize;
    let mut text_encoding = 65001u32;
    let mut first_image_index = None;
    let mut extra_data_flags = 0u16;
    let mut cover_offset = None;
    let mut thumb_offset = None;
    let mut kf8_boundary_rec = None;

    if rec0.len() >= 24 && &rec0[16..20] == b"MOBI" {
        let mobi_header_len = be_u32(rec0, 20).unwrap_or(0) as usize;
        text_encoding = be_u32(rec0, 28).unwrap_or(65001);
        if let Some(full_off) = be_u32(rec0, 84).map(|v| v as usize)
            && let Some(full_len) = be_u32(rec0, 88).map(|v| v as usize)
            && full_len > 0
            && let Some(slice) = rec0.get(full_off..full_off.saturating_add(full_len))
        {
            let full_title = decode_mobi_str(slice, text_encoding);
            if !full_title.is_empty() {
                meta.title = full_title;
            }
        }
        if let Some(first_img) = be_u32(rec0, 108)
            && first_img != u32::MAX
            && first_img > 0
        {
            first_image_index = Some(first_img as usize);
        }
        if mobi_header_len >= 0xE4
            && let Some(flags) = be_u16(rec0, 0xF2)
        {
            extra_data_flags = flags;
        }

        let exth_flags = be_u32(rec0, 128).unwrap_or(0);
        let exth_start = 16usize.saturating_add(mobi_header_len);
        if (exth_flags & 0x40) != 0
            && let Some(exth_hdr) = rec0.get(exth_start..exth_start + 12)
            && &exth_hdr[0..4] == b"EXTH"
        {
            let exth_count = be_u32(exth_hdr, 8).unwrap_or(0) as usize;
            let mut pos = exth_start + 12;
            for _ in 0..exth_count {
                let Some(rec_type) = be_u32(rec0, pos) else {
                    break;
                };
                let Some(rec_len) = be_u32(rec0, pos + 4).map(|v| v as usize) else {
                    break;
                };
                if rec_len < 8 || pos + rec_len > rec0.len() {
                    break;
                }
                let payload = &rec0[pos + 8..pos + rec_len];
                match rec_type {
                    100 => {
                        let author = decode_mobi_str(payload, text_encoding);
                        for part in author
                            .split([';', '&'])
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                        {
                            if !meta.authors.iter().any(|a| a == part) {
                                meta.authors.push(part.to_string());
                            }
                        }
                    }
                    101 => {
                        let p = decode_mobi_str(payload, text_encoding);
                        if !p.is_empty() {
                            meta.publisher = Some(p);
                        }
                    }
                    103 => {
                        let desc =
                            strip_html_entities_and_tags(&decode_mobi_str(payload, text_encoding))
                                .trim()
                                .to_string();
                        if !desc.is_empty() {
                            meta.description = Some(desc);
                        }
                    }
                    104 => {
                        let isbn = decode_mobi_str(payload, text_encoding);
                        if !isbn.is_empty() {
                            meta.isbn = Some(isbn);
                        }
                    }
                    105 => {
                        let tag = decode_mobi_str(payload, text_encoding);
                        if !tag.is_empty() && !meta.tags.contains(&tag) {
                            meta.tags.push(tag);
                        }
                    }
                    106 => {
                        let dt = decode_mobi_str(payload, text_encoding);
                        if !dt.is_empty() {
                            meta.published_at = Some(dt);
                        }
                    }
                    121 => {
                        if let Some(b_idx) = be_u32(payload, 0)
                            && b_idx != u32::MAX
                            && b_idx > 0
                        {
                            kf8_boundary_rec = Some(b_idx as usize);
                        }
                    }
                    201 => {
                        if let Some(off) = be_u32(payload, 0)
                            && off != u32::MAX
                        {
                            cover_offset = Some(off as usize);
                        }
                    }
                    202 => {
                        if let Some(off) = be_u32(payload, 0)
                            && off != u32::MAX
                        {
                            thumb_offset = Some(off as usize);
                        }
                    }
                    503 => {
                        let updated_title = decode_mobi_str(payload, text_encoding);
                        if !updated_title.is_empty() {
                            meta.title = updated_title;
                        }
                    }
                    524 => {
                        let lang = decode_mobi_str(payload, text_encoding);
                        if !lang.is_empty() {
                            meta.language = Some(lang);
                        }
                    }
                    _ => {}
                }
                pos += rec_len;
            }
        }
    } else {
        let pdb_title = decode_mobi_str(pdb_name_bytes, text_encoding);
        if !pdb_title.is_empty() && pdb_title != "Untitled" {
            meta.title = pdb_title;
        }
    }

    Ok(MobiHeaderInfo {
        compression,
        record_count,
        text_encoding,
        first_image_index,
        extra_data_flags,
        cover_offset,
        thumb_offset,
        kf8_boundary_rec,
    })
}

fn extract_text_from_section(
    records: &[&[u8]],
    base_rec_idx: usize,
    info: &MobiHeaderInfo,
) -> FormatResult<String> {
    let mut raw_bytes = Vec::new();
    let max_rec = (base_rec_idx + 1 + info.record_count).min(records.len());
    for rec in records.iter().take(max_rec).skip(base_rec_idx + 1) {
        let trimmed = trim_mobi_trailing_data(rec, info.extra_data_flags);
        match info.compression {
            1 => {
                if raw_bytes.len().saturating_add(trimmed.len()) > MAX_MOBI_DECOMPRESSED_BYTES {
                    return Err(FormatError::Validation(
                        "MOBI/AZW3 text exceeds maximum size".into(),
                    ));
                }
                raw_bytes.extend_from_slice(trimmed);
            }
            2 => decompress_palmdoc_lz77(trimmed, &mut raw_bytes)?,
            other => {
                return Err(FormatError::UnsupportedFormat(format!(
                    "unsupported MOBI/AZW3 compression scheme {other} (DRM/Huffdic not supported)"
                )));
            }
        }
    }
    Ok(match info.text_encoding {
        65001 => UTF_8.decode(&raw_bytes).0.into_owned(),
        1252 => WINDOWS_1252.decode(&raw_bytes).0.into_owned(),
        _ => decode_text_smart(&raw_bytes).0,
    })
}

fn split_mobi_html_chapters(decoded_html: &str, fallback_title: &str) -> Vec<BookTextChapter> {
    let chunks: Vec<&str> = RE_MOBI_PAGEBREAK.split(decoded_html).collect();
    if chunks.len() > 1 {
        let mut out = Vec::new();
        for chunk in chunks {
            let heading = RE_HTML_HEADING
                .captures(chunk)
                .and_then(|c| c.get(2))
                .map(|m| strip_html_entities_and_tags(m.as_str()).trim().to_string())
                .filter(|s| !s.is_empty());
            let plain = strip_html_entities_and_tags(chunk)
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .collect::<Vec<_>>()
                .join("\n");
            if plain.is_empty() && heading.is_none() {
                continue;
            }
            let idx = u32::try_from(out.len()).unwrap_or(0);
            let title = heading
                .or_else(|| {
                    plain
                        .lines()
                        .next()
                        .filter(|l| l.chars().count() <= 60)
                        .map(str::to_owned)
                })
                .unwrap_or_else(|| format!("Chapter {}", idx + 1));
            out.push(BookTextChapter {
                index: idx,
                volume_title: None,
                title,
                content: plain,
                level: 0,
            });
        }
        if !out.is_empty() {
            return out;
        }
    }

    let plain = strip_html_entities_and_tags(decoded_html);
    if let Ok(seg) = segment_txt_structure(&plain, None, None)
        && !seg.chapters.is_empty()
    {
        return seg.chapters;
    }

    vec![BookTextChapter {
        index: 0,
        volume_title: None,
        title: fallback_title.to_string(),
        content: plain.trim().to_string(),
        level: 0,
    }]
}

struct ParsedMobiInternal {
    meta: ParsedBookMeta,
    cover: Option<BinaryAsset>,
    chapters: Vec<BookTextChapter>,
    toc: Vec<BookTocEntry>,
}

fn parse_mobi_internal(
    bytes: &[u8],
    format: BookFormatExt,
    fallback_title: &str,
) -> FormatResult<ParsedMobiInternal> {
    let records = parse_pdb_records(bytes)?;
    let mut meta = ParsedBookMeta::with_fallback(format, fallback_title);
    let primary_info = parse_record0_header(records[0], &bytes[0..32], &mut meta)?;

    let mut html = extract_text_from_section(&records, 0, &primary_info)?;
    if let Some(boundary_idx) = primary_info.kf8_boundary_rec
        && let Some(kf8_rec0_idx) = boundary_idx.checked_add(1)
        && kf8_rec0_idx < records.len()
    {
        let mut kf8_meta = meta.clone();
        if let Ok(kf8_info) =
            parse_record0_header(records[kf8_rec0_idx], &bytes[0..32], &mut kf8_meta)
            && let Ok(kf8_html) = extract_text_from_section(&records, kf8_rec0_idx, &kf8_info)
            && kf8_html.trim().len() > html.trim().len()
        {
            html = kf8_html;
            if meta.title == fallback_title && !kf8_meta.title.is_empty() {
                meta.title = kf8_meta.title;
            }
        }
    }

    let mut cover = None;
    if let Some(first_img) = primary_info.first_image_index {
        for off_opt in [
            primary_info.cover_offset,
            primary_info.thumb_offset,
            Some(0),
        ] {
            if let Some(off) = off_opt
                && let Some(rec_bytes) = records.get(first_img.saturating_add(off))
                && let Some(mime) = sniff_raster_image_mime(rec_bytes)
            {
                cover = Some(BinaryAsset::new(rec_bytes.to_vec(), mime));
                break;
            }
        }
    }

    let chapters = split_mobi_html_chapters(&html, &meta.title);
    let toc: Vec<BookTocEntry> = chapters
        .iter()
        .map(|ch| BookTocEntry {
            index: ch.index,
            title: ch.title.clone(),
            href: format!("chapter:{}", ch.index),
            level: ch.level,
        })
        .collect();

    meta.has_cover = cover.is_some();
    meta.chapter_count = u32::try_from(chapters.len()).ok();

    Ok(ParsedMobiInternal {
        meta,
        cover,
        chapters,
        toc,
    })
}

pub fn parse_mobi_bytes(
    bytes: &[u8],
    format: BookFormatExt,
    fallback_title: &str,
) -> FormatResult<ParsedBookPackage> {
    let parsed = parse_mobi_internal(bytes, format, fallback_title)?;
    Ok(ParsedBookPackage {
        metadata: parsed.meta,
        cover: parsed.cover,
        toc: parsed.toc,
    })
}

pub fn list_mobi_toc_bytes(bytes: &[u8]) -> FormatResult<Vec<BookTocEntry>> {
    Ok(parse_mobi_internal(bytes, BookFormatExt::Mobi, "Untitled")?.toc)
}

pub fn read_mobi_chapter_bytes(
    bytes: &[u8],
    chapter_index: u32,
) -> FormatResult<(BookTextChapter, u32)> {
    let parsed = parse_mobi_internal(bytes, BookFormatExt::Mobi, "Untitled")?;
    let total = u32::try_from(parsed.chapters.len()).unwrap_or(0);
    let ch = parsed
        .chapters
        .into_iter()
        .nth(chapter_index as usize)
        .ok_or_else(|| {
            FormatError::NotFound(format!("MOBI/AZW3 chapter {chapter_index} not found"))
        })?;
    Ok((ch, total))
}

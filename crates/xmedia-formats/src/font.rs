#[cfg(test)]
mod tests;

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{FormatError, FormatResult};

const MAX_FONT_NAME_CHARS: usize = 128;
const MAX_SFNT_TABLES: usize = 512;
const MAX_NAME_RECORDS: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FontFormat {
    Ttf,
    Otf,
    Ttc,
    Woff,
    Woff2,
}

impl FontFormat {
    pub const fn extension(&self) -> &'static str {
        match self {
            Self::Ttf => "ttf",
            Self::Otf => "otf",
            Self::Ttc => "ttc",
            Self::Woff => "woff",
            Self::Woff2 => "woff2",
        }
    }

    pub const fn mime_type(&self) -> &'static str {
        match self {
            Self::Ttf => "font/ttf",
            Self::Otf => "font/otf",
            Self::Ttc => "font/collection",
            Self::Woff => "font/woff",
            Self::Woff2 => "font/woff2",
        }
    }

    pub const fn css_format(&self) -> &'static str {
        match self {
            Self::Ttf => "truetype",
            Self::Otf => "opentype",
            Self::Ttc => "truetype",
            Self::Woff => "woff",
            Self::Woff2 => "woff2",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetectedFontInfo {
    pub format: FontFormat,
    pub family_name: String,
}

pub fn sniff_font_format(bytes: &[u8]) -> FormatResult<FontFormat> {
    if bytes.is_empty() {
        return Err(FormatError::Validation(
            "font payload cannot be empty".into(),
        ));
    }
    let Some(magic) = bytes.get(..4) else {
        return Err(FormatError::UnsupportedFormat(
            "unrecognized font file header (too short)".into(),
        ));
    };
    match magic {
        [0x00, 0x01, 0x00, 0x00] | b"true" => Ok(FontFormat::Ttf),
        b"OTTO" => Ok(FontFormat::Otf),
        b"ttcf" => Ok(FontFormat::Ttc),
        b"wOFF" => Ok(FontFormat::Woff),
        b"wOF2" => Ok(FontFormat::Woff2),
        _ => Err(FormatError::UnsupportedFormat(
            "unsupported or unrecognized font magic bytes".into(),
        )),
    }
}

pub fn inspect_font_bytes(file_name: &str, bytes: &[u8]) -> FormatResult<DetectedFontInfo> {
    let format = sniff_font_format(bytes)?;
    let family_name = extract_sfnt_family_name(format, bytes)
        .unwrap_or_else(|| fallback_family_from_filename(file_name));
    Ok(DetectedFontInfo {
        format,
        family_name,
    })
}

fn sanitize_font_display_name(raw: &str) -> Option<String> {
    let filtered: String = raw
        .chars()
        .filter(|&c| {
            !c.is_control() && !matches!(c, ';' | '{' | '}' | '<' | '>' | '\\' | '"' | '\'')
        })
        .collect();
    let trimmed = filtered.trim();
    if trimmed.is_empty() {
        return None;
    }
    let capped: String = trimmed.chars().take(MAX_FONT_NAME_CHARS).collect();
    let final_trimmed = capped.trim_end();
    if final_trimmed.is_empty() {
        None
    } else {
        Some(final_trimmed.to_string())
    }
}

fn fallback_family_from_filename(file_name: &str) -> String {
    let base = file_name.rsplit(['/', '\\']).next().unwrap_or(file_name);
    let stem = Path::new(base)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(base);
    sanitize_font_display_name(stem).unwrap_or_else(|| "Custom Font".to_string())
}

fn extract_sfnt_family_name(format: FontFormat, bytes: &[u8]) -> Option<String> {
    let sfnt_offset = match format {
        FontFormat::Ttf | FontFormat::Otf => 0usize,
        FontFormat::Ttc => {
            let header = bytes.get(..16)?;
            let num_fonts = u32::from_be_bytes([header[8], header[9], header[10], header[11]]);
            if num_fonts == 0 {
                return None;
            }
            u32::from_be_bytes([header[12], header[13], header[14], header[15]]) as usize
        }
        FontFormat::Woff | FontFormat::Woff2 => return None,
    };

    let dir_header = bytes.get(sfnt_offset..sfnt_offset.checked_add(12)?)?;
    let num_tables =
        u16::from_be_bytes([dir_header[4], dir_header[5]]).min(MAX_SFNT_TABLES as u16) as usize;
    let records_base = sfnt_offset.checked_add(12)?;

    for idx in 0..num_tables {
        let rec_start = records_base.checked_add(idx.checked_mul(16)?)?;
        let Some(rec) = bytes.get(rec_start..rec_start.checked_add(16)?) else {
            break;
        };
        if &rec[0..4] == b"name" {
            let tbl_offset = u32::from_be_bytes([rec[8], rec[9], rec[10], rec[11]]) as usize;
            let tbl_len = u32::from_be_bytes([rec[12], rec[13], rec[14], rec[15]]) as usize;
            if tbl_len < 6 || tbl_offset >= bytes.len() {
                return None;
            }
            let tbl_end = bytes.len().min(tbl_offset.saturating_add(tbl_len));
            let name_table = bytes.get(tbl_offset..tbl_end)?;
            return parse_name_table(name_table);
        }
    }
    None
}

fn parse_name_table(name_table: &[u8]) -> Option<String> {
    let header = name_table.get(..6)?;
    let count = u16::from_be_bytes([header[2], header[3]]).min(MAX_NAME_RECORDS as u16) as usize;
    let string_offset = u16::from_be_bytes([header[4], header[5]]) as usize;

    let mut best: Option<(u32, String)> = None;

    for idx in 0..count {
        let Some(rec_start) = 6usize.checked_add(idx.checked_mul(12)?) else {
            break;
        };
        let Some(rec) = name_table.get(rec_start..rec_start + 12) else {
            break;
        };

        let platform_id = u16::from_be_bytes([rec[0], rec[1]]);
        let encoding_id = u16::from_be_bytes([rec[2], rec[3]]);
        let language_id = u16::from_be_bytes([rec[4], rec[5]]);
        let name_id = u16::from_be_bytes([rec[6], rec[7]]);
        if name_id != 16 && name_id != 1 {
            continue;
        }

        let length = u16::from_be_bytes([rec[8], rec[9]]) as usize;
        let offset = u16::from_be_bytes([rec[10], rec[11]]) as usize;
        let Some(str_start) = string_offset.checked_add(offset) else {
            continue;
        };
        let Some(str_end) = str_start.checked_add(length) else {
            continue;
        };
        let Some(raw) = name_table.get(str_start..str_end) else {
            continue;
        };

        let Some(decoded) = decode_name_record_bytes(platform_id, encoding_id, raw) else {
            continue;
        };
        let Some(cleaned) = sanitize_font_display_name(&decoded) else {
            continue;
        };

        let score = score_name_record(name_id, platform_id, encoding_id, language_id);
        if best
            .as_ref()
            .is_none_or(|(best_score, _)| score > *best_score)
        {
            best = Some((score, cleaned));
        }
    }

    best.map(|(_, name)| name)
}

fn decode_name_record_bytes(platform_id: u16, encoding_id: u16, raw: &[u8]) -> Option<String> {
    if raw.is_empty() {
        return None;
    }
    match platform_id {
        0 | 3 => {
            if raw.len() < 2 {
                return None;
            }
            let units = raw
                .as_chunks::<2>()
                .0
                .iter()
                .copied()
                .map(u16::from_be_bytes);
            let decoded: String = char::decode_utf16(units).filter_map(Result::ok).collect();
            Some(decoded)
        }
        1 => {
            if let Ok(utf8) = std::str::from_utf8(raw) {
                return Some(utf8.to_string());
            }
            if encoding_id == 0 {
                let (cow, _) = encoding_rs::MACINTOSH.decode_without_bom_handling(raw);
                return Some(cow.into_owned());
            }
            None
        }
        _ => std::str::from_utf8(raw).ok().map(str::to_string),
    }
}

fn score_name_record(name_id: u16, platform_id: u16, encoding_id: u16, language_id: u16) -> u32 {
    let name_rank = if name_id == 16 { 1000 } else { 500 };
    let platform_rank = match (platform_id, encoding_id) {
        (3, 1 | 10) => 40,
        (3, _) => 30,
        (0, _) => 25,
        (1, 0) => 15,
        (1, _) => 10,
        _ => 5,
    };
    let lang_bonus = match (platform_id, language_id) {
        (3, 0x0409) | (0, 0) | (1, 0) => 2,
        (3, 0x0804 | 0x0404) => 1,
        _ => 0,
    };
    name_rank + platform_rank + lang_bonus
}

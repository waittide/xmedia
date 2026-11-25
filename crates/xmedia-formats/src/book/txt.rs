use std::sync::LazyLock;

use regex::Regex;

use super::{BookFormatExt, BookTextChapter, BookTocEntry, ParsedBookMeta, ParsedBookPackage};
use crate::common::{compile_bounded_regex, decode_text_smart};
use crate::error::{FormatError, FormatResult};

static DEFAULT_VOLUME_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^\s*(?:第[零一二三四五六七八九十百千万两〇廿卅卌壹贰叁肆伍陆柒捌玖拾佰仟0-9０-９]+[卷部集册篇](?:[\s：:、—\-].*)?|卷[零一二三四五六七八九十百千万两〇廿卅卌壹贰叁肆伍陆柒捌玖拾佰仟0-9０-９]+(?:[\s：:、—\-].*)?|(?:volume|vol\.?|book|part)\s+[0-9ivxlcdm]+(?:\b.*)?)$",
    )
    .expect("valid TXT volume heading regex")
});

static DEFAULT_CHAPTER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)^\s*(?:第[零一二三四五六七八九十百千万两〇廿卅卌壹贰叁肆伍陆柒捌玖拾佰仟0-9０-９]+[\s章节回话折幕][^\n。．！？；]*|(?:chapter|section|episode)\s+[0-9ivxlcdm]+(?:\b[^\n,;"]*)?|(?:楔子|序[章言幕曲]?|引子|前言|番外|尾声|后记|终章|prologue|epilogue)(?:[\s\u{3000}：:、—\-].*)?)$"#,
    )
    .expect("valid TXT chapter heading regex")
});

static PROSE_FALSE_CHAPTER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^\s*第[零一二三四五六七八九十百千万两〇廿卅卌壹贰叁肆伍陆柒捌玖拾佰仟0-9０-９]+[章节回话折幕][中内裏里上下前后後的了说說道云]",
    )
    .expect("valid prose false chapter regex")
});

static AUTHOR_LINE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^\s*(?:作者|著|author)\s*[:：]\s*(.+?)\s*$").expect("valid TXT author regex")
});

static BOOK_BRACKET_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*《([^》]{1,80})》\s*$").expect("valid TXT bracket title regex")
});

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HeadingKind {
    Volume,
    Chapter,
}

#[derive(Debug, Clone)]
struct RawBlock {
    kind: HeadingKind,
    title: String,
    volume_title: Option<String>,
    body_start: usize,
    end_byte: usize,
}

#[derive(Debug, Clone)]
pub struct SegmentedTextStructure {
    pub chapters: Vec<BookTextChapter>,
    pub toc: Vec<BookTocEntry>,
}

pub fn decode_txt_bytes(bytes: &[u8]) -> String {
    decode_text_smart(bytes).0
}

pub fn clean_plain_paragraphs(raw: &str) -> String {
    raw.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn chunk_unstructured_text(text: &str, approx_chars: usize) -> Vec<BookTextChapter> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return vec![BookTextChapter {
            index: 0,
            volume_title: None,
            title: "Section 1".to_string(),
            content: String::new(),
            level: 0,
        }];
    }
    let mut out = Vec::new();
    let mut acc = String::new();
    let mut acc_chars = 0usize;

    for line in trimmed.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if !acc.is_empty() {
            acc.push('\n');
        }
        acc.push_str(t);
        acc_chars += t.chars().count();
        if acc_chars >= approx_chars {
            let idx = out.len();
            out.push(BookTextChapter {
                index: u32::try_from(idx).unwrap_or(0),
                volume_title: None,
                title: format!("Section {}", idx + 1),
                content: std::mem::take(&mut acc),
                level: 0,
            });
            acc_chars = 0;
        }
    }
    if !acc.is_empty() {
        let idx = out.len();
        out.push(BookTextChapter {
            index: u32::try_from(idx).unwrap_or(0),
            volume_title: None,
            title: format!("Section {}", idx + 1),
            content: acc,
            level: 0,
        });
    }
    out
}

/// Segments plain text into structured chapters and TOC entries using either default CN/EN
/// volume+chapter regexes or bounded custom regexes.
pub fn segment_txt_structure(
    raw_text: &str,
    custom_chapter_pattern: Option<&str>,
    custom_volume_pattern: Option<&str>,
) -> FormatResult<SegmentedTextStructure> {
    let custom_chap = match custom_chapter_pattern
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(pat) => Some(compile_bounded_regex(pat, "章节正则表达式")?),
        None => None,
    };
    let custom_vol = match custom_volume_pattern
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(pat) => Some(compile_bounded_regex(pat, "分卷正则表达式")?),
        None => None,
    };

    let chap_re = custom_chap.as_ref().unwrap_or(&DEFAULT_CHAPTER_RE);
    let vol_re_opt = if custom_chap.is_some() && custom_vol.is_none() {
        None
    } else {
        Some(custom_vol.as_ref().unwrap_or(&DEFAULT_VOLUME_RE))
    };

    let text = raw_text.replace("\r\n", "\n").replace('\r', "\n");
    let mut blocks: Vec<RawBlock> = Vec::new();
    let mut active_volume: Option<String> = None;
    let mut current: Option<(HeadingKind, String, Option<String>, usize)> = None;
    let mut cursor = 0usize;

    for raw_line in text.split_inclusive('\n') {
        let line_end = cursor + raw_line.len();
        let trimmed = raw_line.trim();
        let char_len = trimmed.chars().count();

        let matched = if (1..=80).contains(&char_len) {
            if vol_re_opt.is_some_and(|re| re.is_match(trimmed)) {
                Some(HeadingKind::Volume)
            } else if chap_re.is_match(trimmed)
                && (custom_chap.is_some() || !PROSE_FALSE_CHAPTER_RE.is_match(trimmed))
            {
                Some(HeadingKind::Chapter)
            } else {
                None
            }
        } else {
            None
        };

        if let Some(kind) = matched {
            if let Some((prev_kind, prev_title, prev_vol, prev_body_start)) = current.take() {
                blocks.push(RawBlock {
                    kind: prev_kind,
                    title: prev_title,
                    volume_title: prev_vol,
                    body_start: prev_body_start,
                    end_byte: cursor,
                });
            } else if cursor > 0 && !text[..cursor].trim().is_empty() {
                blocks.push(RawBlock {
                    kind: HeadingKind::Chapter,
                    title: "Prologue / 序".to_string(),
                    volume_title: None,
                    body_start: 0,
                    end_byte: cursor,
                });
            }

            let heading_text = trimmed.to_string();
            if kind == HeadingKind::Volume {
                active_volume = Some(heading_text.clone());
                current = Some((HeadingKind::Volume, heading_text, None, line_end));
            } else {
                current = Some((
                    HeadingKind::Chapter,
                    heading_text,
                    active_volume.clone(),
                    line_end,
                ));
            }
        }
        cursor = line_end;
    }

    if let Some((last_kind, last_title, last_vol, last_body_start)) = current {
        blocks.push(RawBlock {
            kind: last_kind,
            title: last_title,
            volume_title: last_vol,
            body_start: last_body_start,
            end_byte: text.len(),
        });
    }

    if blocks.is_empty() {
        let chapters = chunk_unstructured_text(&text, 6000);
        let toc = chapters
            .iter()
            .enumerate()
            .map(|(i, c)| BookTocEntry {
                index: u32::try_from(i).unwrap_or(0),
                title: c.title.clone(),
                href: format!("chapter:{i}"),
                level: 0,
            })
            .collect();
        return Ok(SegmentedTextStructure { chapters, toc });
    }

    let has_any_volume = blocks.iter().any(|b| b.kind == HeadingKind::Volume);
    let mut chapters: Vec<BookTextChapter> = Vec::with_capacity(blocks.len());
    let mut toc: Vec<BookTocEntry> = Vec::with_capacity(blocks.len());
    let mut pending_volume_toc_indices: Vec<usize> = Vec::new();

    for block in blocks {
        let body = text[block.body_start..block.end_byte].trim();
        match block.kind {
            HeadingKind::Volume => {
                if !body.is_empty() {
                    let ch_idx = chapters.len();
                    chapters.push(BookTextChapter {
                        index: u32::try_from(ch_idx).unwrap_or(0),
                        volume_title: None,
                        title: block.title.clone(),
                        content: body.to_string(),
                        level: 0,
                    });
                    for pending_idx in pending_volume_toc_indices.drain(..) {
                        toc[pending_idx].href = format!("chapter:{ch_idx}");
                    }
                    let toc_idx = u32::try_from(toc.len()).unwrap_or(0);
                    toc.push(BookTocEntry {
                        index: toc_idx,
                        title: block.title,
                        href: format!("chapter:{ch_idx}"),
                        level: 0,
                    });
                } else {
                    let toc_pos = toc.len();
                    toc.push(BookTocEntry {
                        index: u32::try_from(toc_pos).unwrap_or(0),
                        title: block.title,
                        href: String::new(),
                        level: 0,
                    });
                    pending_volume_toc_indices.push(toc_pos);
                }
            }
            HeadingKind::Chapter => {
                let ch_idx = chapters.len();
                let href = format!("chapter:{ch_idx}");
                for pending_idx in pending_volume_toc_indices.drain(..) {
                    toc[pending_idx].href = href.clone();
                }
                let level = if has_any_volume && block.volume_title.is_some() {
                    1
                } else {
                    0
                };
                chapters.push(BookTextChapter {
                    index: u32::try_from(ch_idx).unwrap_or(0),
                    volume_title: block.volume_title,
                    title: block.title.clone(),
                    content: body.to_string(),
                    level,
                });
                let toc_idx = u32::try_from(toc.len()).unwrap_or(0);
                toc.push(BookTocEntry {
                    index: toc_idx,
                    title: block.title,
                    href,
                    level,
                });
            }
        }
    }

    if !pending_volume_toc_indices.is_empty() {
        if chapters.is_empty() {
            chapters.push(BookTextChapter {
                index: 0,
                volume_title: None,
                title: toc[0].title.clone(),
                content: text.trim().to_string(),
                level: 0,
            });
        }
        let fallback_href = format!("chapter:{}", chapters.len() - 1);
        for pending_idx in pending_volume_toc_indices {
            toc[pending_idx].href = fallback_href.clone();
        }
    }

    Ok(SegmentedTextStructure { chapters, toc })
}

/// Convenience helper for editor chapter splitting where empty volume headings are kept as
/// standalone `level = 0` entries.
pub fn segment_txt_chapters_flat(
    raw_text: &str,
    custom_chapter_pattern: Option<&str>,
    custom_volume_pattern: Option<&str>,
) -> FormatResult<Vec<BookTextChapter>> {
    let custom_chap = match custom_chapter_pattern
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(pat) => Some(compile_bounded_regex(pat, "章节正则表达式")?),
        None => None,
    };
    let custom_vol = match custom_volume_pattern
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(pat) => Some(compile_bounded_regex(pat, "分卷正则表达式")?),
        None => None,
    };
    let chap_re = custom_chap.as_ref().unwrap_or(&DEFAULT_CHAPTER_RE);
    let vol_re_opt = if custom_chap.is_some() && custom_vol.is_none() {
        None
    } else {
        Some(custom_vol.as_ref().unwrap_or(&DEFAULT_VOLUME_RE))
    };

    let normalized = raw_text.replace("\r\n", "\n").replace('\r', "\n");
    let mut sections: Vec<(HeadingKind, String, Option<String>, usize, usize)> = Vec::new();
    let mut active_volume: Option<String> = None;
    let mut current: Option<(HeadingKind, String, Option<String>, usize)> = None;
    let mut cursor = 0usize;

    for raw_line in normalized.split_inclusive('\n') {
        let next_cursor = cursor + raw_line.len();
        let trimmed = raw_line.trim();
        let char_len = trimmed.chars().count();
        let matched = if (1..=80).contains(&char_len) {
            if vol_re_opt.is_some_and(|re| re.is_match(trimmed)) {
                Some(HeadingKind::Volume)
            } else if chap_re.is_match(trimmed)
                && (custom_chap.is_some() || !PROSE_FALSE_CHAPTER_RE.is_match(trimmed))
            {
                Some(HeadingKind::Chapter)
            } else {
                None
            }
        } else {
            None
        };

        if let Some(kind) = matched {
            if let Some((pk, pt, pv, p_start)) = current.take() {
                sections.push((pk, pt, pv, p_start, cursor));
            } else if cursor > 0 && !normalized[..cursor].trim().is_empty() {
                sections.push((
                    HeadingKind::Chapter,
                    "前言 / 序".to_string(),
                    None,
                    0,
                    cursor,
                ));
            }
            if kind == HeadingKind::Volume {
                active_volume = Some(trimmed.to_string());
                current = Some((HeadingKind::Volume, trimmed.to_string(), None, next_cursor));
            } else {
                current = Some((
                    HeadingKind::Chapter,
                    trimmed.to_string(),
                    active_volume.clone(),
                    next_cursor,
                ));
            }
        }
        cursor = next_cursor;
    }
    if let Some((pk, pt, pv, p_start)) = current {
        sections.push((pk, pt, pv, p_start, normalized.len()));
    }
    if sections.is_empty() {
        return Ok(chunk_unstructured_text(&normalized, 6000));
    }
    Ok(sections
        .into_iter()
        .enumerate()
        .map(|(idx, (kind, title, vol, s, e))| BookTextChapter {
            index: u32::try_from(idx).unwrap_or(0),
            level: if kind == HeadingKind::Volume || vol.is_none() {
                0
            } else {
                1
            },
            volume_title: vol,
            title,
            content: clean_plain_paragraphs(&normalized[s..e]),
        })
        .collect())
}

pub fn parse_txt_bytes(file_bytes: &[u8], fallback_title: &str) -> FormatResult<ParsedBookPackage> {
    let decoded = decode_txt_bytes(file_bytes);
    let structure = segment_txt_structure(&decoded, None, None)?;

    let mut detected_title: Option<String> = None;
    let mut detected_author: Option<String> = None;
    for line in decoded.lines().take(25) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if detected_title.is_none()
            && let Some(caps) = BOOK_BRACKET_RE.captures(trimmed)
        {
            detected_title = Some(caps[1].trim().to_string());
        }
        if detected_author.is_none()
            && let Some(caps) = AUTHOR_LINE_RE.captures(trimmed)
        {
            detected_author = Some(caps[1].trim().to_string());
        }
    }

    let title = detected_title.unwrap_or_else(|| fallback_title.to_string());
    let mut metadata = ParsedBookMeta::with_fallback(BookFormatExt::Txt, &title);
    if let Some(author) = detected_author {
        metadata.authors = vec![author];
    }
    metadata.chapter_count = u32::try_from(structure.chapters.len()).ok();
    let preview: String = decoded
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .take(4)
        .collect::<Vec<_>>()
        .join("\n")
        .chars()
        .take(280)
        .collect();
    if !preview.is_empty() {
        metadata.description = Some(preview);
    }

    Ok(ParsedBookPackage {
        metadata,
        cover: None,
        toc: structure.toc,
    })
}

pub fn list_txt_toc_bytes(file_bytes: &[u8]) -> Vec<BookTocEntry> {
    let decoded = decode_txt_bytes(file_bytes);
    segment_txt_structure(&decoded, None, None)
        .map(|s| s.toc)
        .unwrap_or_default()
}

pub fn read_txt_chapter_bytes(
    file_bytes: &[u8],
    chapter_index: u32,
) -> FormatResult<(BookTextChapter, u32)> {
    let decoded = decode_txt_bytes(file_bytes);
    let structure = segment_txt_structure(&decoded, None, None)?;
    let total = u32::try_from(structure.chapters.len()).unwrap_or(0);
    let ch = structure
        .chapters
        .into_iter()
        .nth(chapter_index as usize)
        .ok_or_else(|| FormatError::NotFound(format!("chapter {chapter_index} not found")))?;
    Ok((ch, total))
}

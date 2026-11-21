use std::sync::LazyLock;

use regex::Regex;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::infra::stream::decode_text_smart;

use crate::application::ports::{ParsedBook, TocItem, TxtChapterContent};
use crate::domain::book::BookMetadata;

use super::svg_cover::generate_placeholder_svg_cover;

static VOLUME_HEADING_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^\s*(?:第[零一二三四五六七八九十百千万两〇廿卅卌壹贰叁肆伍陆柒捌玖拾佰仟0-9０-９]+[卷部集册篇](?:[\s：:、—\-].*)?|卷[零一二三四五六七八九十百千万两〇廿卅卌壹贰叁肆伍陆柒捌玖拾佰仟0-9０-９]+(?:[\s：:、—\-].*)?|(?:volume|vol\.?|book|part)\s+[0-9ivxlcdm]+(?:\b.*)?)$",
    )
    .expect("valid TXT volume heading regex")
});

static CHAPTER_HEADING_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^\s*(?:第[零一二三四五六七八九十百千万两〇廿卅卌壹贰叁肆伍陆柒捌玖拾佰仟0-9０-９]+[\s章节回话折幕].*|(?:chapter|section|episode)\s+[0-9ivxlcdm]+(?:\b.*)?|(?:楔子|序[章言幕曲]?|引子|前言|番外|尾声|后记|终章|prologue|epilogue)(?:[\s：:、—\-].*)?)$",
    )
    .expect("valid TXT chapter heading regex")
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
struct ChapterSlice {
    volume_title: Option<String>,
    title: String,
    body_start: usize,
    end_byte: usize,
}

#[derive(Debug, Clone)]
struct TxtStructure {
    chapters: Vec<ChapterSlice>,
    toc: Vec<TocItem>,
}

pub fn decode_txt_bytes(bytes: &[u8]) -> String {
    decode_text_smart(bytes).0
}

fn classify_heading(trimmed: &str) -> Option<HeadingKind> {
    let char_count = trimmed.chars().count();
    if !(1..=80).contains(&char_count) {
        return None;
    }
    if VOLUME_HEADING_RE.is_match(trimmed) {
        return Some(HeadingKind::Volume);
    }
    if CHAPTER_HEADING_RE.is_match(trimmed) {
        return Some(HeadingKind::Chapter);
    }
    None
}

fn build_structure(text: &str) -> TxtStructure {
    let mut blocks: Vec<RawBlock> = Vec::new();
    let mut active_volume: Option<String> = None;
    let mut current: Option<(HeadingKind, String, Option<String>, usize)> = None;
    let mut cursor = 0usize;

    for raw_line in text.split_inclusive('\n') {
        let line_len = raw_line.len();
        let line_end = cursor + line_len;
        let trimmed = raw_line.trim();

        if let Some(kind) = classify_heading(trimmed) {
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
        let chapters = chunk_plain_text_by_paragraphs(text, 6000);
        let toc = chapters
            .iter()
            .enumerate()
            .map(|(i, c)| TocItem {
                index: u32::try_from(i).unwrap_or(0),
                title: c.title.clone(),
                href: format!("chapter:{i}"),
                level: 0,
            })
            .collect();
        return TxtStructure { chapters, toc };
    }

    let has_any_volume = blocks.iter().any(|b| b.kind == HeadingKind::Volume);
    let mut chapters: Vec<ChapterSlice> = Vec::with_capacity(blocks.len());
    let mut toc: Vec<TocItem> = Vec::with_capacity(blocks.len());
    let mut pending_volume_toc_indices: Vec<usize> = Vec::new();

    for block in blocks {
        let has_body = !text[block.body_start..block.end_byte].trim().is_empty();
        match block.kind {
            HeadingKind::Volume => {
                if has_body {
                    let ch_idx = chapters.len();
                    chapters.push(ChapterSlice {
                        volume_title: None,
                        title: block.title.clone(),
                        body_start: block.body_start,
                        end_byte: block.end_byte,
                    });
                    for pending_idx in pending_volume_toc_indices.drain(..) {
                        toc[pending_idx].href = format!("chapter:{ch_idx}");
                    }
                    let toc_idx = u32::try_from(toc.len()).unwrap_or(0);
                    toc.push(TocItem {
                        index: toc_idx,
                        title: block.title,
                        href: format!("chapter:{ch_idx}"),
                        level: 0,
                    });
                } else {
                    let toc_pos = toc.len();
                    toc.push(TocItem {
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

                chapters.push(ChapterSlice {
                    volume_title: block.volume_title,
                    title: block.title.clone(),
                    body_start: block.body_start,
                    end_byte: block.end_byte,
                });

                let toc_idx = u32::try_from(toc.len()).unwrap_or(0);
                toc.push(TocItem {
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
            chapters.push(ChapterSlice {
                volume_title: None,
                title: toc[0].title.clone(),
                body_start: 0,
                end_byte: text.len(),
            });
        }
        let fallback_href = format!("chapter:{}", chapters.len() - 1);
        for pending_idx in pending_volume_toc_indices {
            toc[pending_idx].href = fallback_href.clone();
        }
    }

    TxtStructure { chapters, toc }
}

fn chunk_plain_text_by_paragraphs(text: &str, approx_chars_per_chunk: usize) -> Vec<ChapterSlice> {
    if text.trim().is_empty() {
        return vec![ChapterSlice {
            volume_title: None,
            title: "Section 1".to_string(),
            body_start: 0,
            end_byte: text.len(),
        }];
    }

    let mut slices = Vec::new();
    let mut chunk_start = 0usize;
    let mut chunk_chars = 0usize;
    let mut cursor = 0usize;

    for line in text.split_inclusive('\n') {
        chunk_chars += line.chars().count();
        cursor += line.len();
        if chunk_chars >= approx_chars_per_chunk {
            let idx = slices.len() + 1;
            slices.push(ChapterSlice {
                volume_title: None,
                title: format!("Section {idx}"),
                body_start: chunk_start,
                end_byte: cursor,
            });
            chunk_start = cursor;
            chunk_chars = 0;
        }
    }

    if chunk_start < text.len() {
        let idx = slices.len() + 1;
        slices.push(ChapterSlice {
            volume_title: None,
            title: format!("Section {idx}"),
            body_start: chunk_start,
            end_byte: text.len(),
        });
    }

    slices
}

pub fn parse_txt(file_bytes: &[u8], fallback_title: &str) -> DomainResult<ParsedBook> {
    let decoded = decode_txt_bytes(file_bytes);
    let structure = build_structure(&decoded);

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
    let mut metadata = BookMetadata::new(&title)?;
    if let Some(author) = detected_author {
        metadata.authors = vec![author];
    }
    metadata.chapter_count = u32::try_from(structure.chapters.len()).ok();
    metadata.description = decoded
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .take(4)
        .collect::<Vec<_>>()
        .join("\n")
        .chars()
        .take(280)
        .collect::<String>()
        .into();

    let cover = Some(generate_placeholder_svg_cover(
        &metadata.title,
        metadata.authors.first().map(String::as_str),
        "TXT",
    ));

    Ok(ParsedBook {
        metadata,
        cover,
        toc: structure.toc,
    })
}

pub fn list_txt_toc(file_bytes: &[u8]) -> Vec<TocItem> {
    let decoded = decode_txt_bytes(file_bytes);
    build_structure(&decoded).toc
}

pub fn read_txt_chapter(file_bytes: &[u8], chapter_index: u32) -> DomainResult<TxtChapterContent> {
    let decoded = decode_txt_bytes(file_bytes);
    let structure = build_structure(&decoded);
    let total_chapters = u32::try_from(structure.chapters.len()).unwrap_or(0);
    let slice = structure
        .chapters
        .get(chapter_index as usize)
        .ok_or_else(|| DomainError::NotFound(format!("chapter {chapter_index} not found")))?;

    let content = decoded[slice.body_start..slice.end_byte].trim().to_string();
    Ok(TxtChapterContent {
        index: chapter_index,
        volume_title: slice.volume_title.clone(),
        title: slice.title.clone(),
        content,
        total_chapters,
    })
}

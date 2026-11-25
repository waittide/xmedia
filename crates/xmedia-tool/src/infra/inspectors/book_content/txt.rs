use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::infra::stream::decode_text_smart;

use crate::infra::atomic_fs::atomic_write_bytes;

use super::xhtml::{plain_to_xhtml_body, xhtml_body_to_plain};
use super::{
    ChapterEditMode, EditableBookChapterDto, SaveBookChapterInput, compile_bounded_user_regex,
};

static DEFAULT_VOLUME_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^\s*(?:第[零一二三四五六七八九十百千万两〇廿卅卌壹贰叁肆伍陆柒捌玖拾佰仟0-9０-９]+[卷部集册篇](?:[\s：:、—\-].*)?|卷[零一二三四五六七八九十百千万两〇廿卅卌壹贰叁肆伍陆柒捌玖拾佰仟0-9０-９]+(?:[\s：:、—\-].*)?|(?:volume|vol\.?|book|part)\s+[0-9ivxlcdm]+(?:\b.*)?)$",
    )
    .expect("valid default volume heading regex")
});

static DEFAULT_CHAPTER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)^\s*(?:第[零一二三四五六七八九十百千万两〇廿卅卌壹贰叁肆伍陆柒捌玖拾佰仟0-9０-９]+[\s章节回话折幕][^\n。．！？；]*|(?:chapter|episode)\s+[0-9ivxlcdm]+(?:\b[^\n,;"]*)?|(?:楔子|序[章言幕曲]?|引子|前言|番外|尾声|后记|终章|prologue|epilogue)(?:[\s\u{3000}：:、—\-].*)?)$"#,
    )
    .expect("valid default chapter heading regex")
});

static DEFAULT_PROSE_FALSE_CHAPTER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^\s*第[零一二三四五六七八九十百千万两〇廿卅卌壹贰叁肆伍陆柒捌玖拾佰仟0-9０-９]+[章节回话折幕][中内裏里上下前后後的了说說道云]",
    )
    .expect("valid prose false chapter regex")
});

pub fn resegment_txt_file(
    path: &Path,
    chapter_pattern: Option<&str>,
    volume_pattern: Option<&str>,
) -> DomainResult<Vec<EditableBookChapterDto>> {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if ext != "txt" {
        return Err(DomainError::Validation(
            "regex re-segmentation is only applicable to TXT files".into(),
        ));
    }
    let bytes = std::fs::read(path)
        .map_err(|e| DomainError::Storage(format!("failed reading txt file: {e}")))?;
    let (decoded, _) = decode_text_smart(&bytes);
    segment_txt_chapters(&decoded, chapter_pattern, volume_pattern)
}

pub fn segment_txt_chapters(
    raw_text: &str,
    custom_chapter_pattern: Option<&str>,
    custom_volume_pattern: Option<&str>,
) -> DomainResult<Vec<EditableBookChapterDto>> {
    let custom_chap = match custom_chapter_pattern
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(pat) => Some(compile_bounded_user_regex(pat, "章节正则表达式")?),
        None => None,
    };
    let custom_vol = match custom_volume_pattern
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(pat) => Some(compile_bounded_user_regex(pat, "分卷正则表达式")?),
        None => None,
    };

    let chap_re = custom_chap.as_ref().unwrap_or(&DEFAULT_CHAPTER_RE);
    let vol_re_opt = if custom_chap.is_some() && custom_vol.is_none() {
        None
    } else {
        Some(custom_vol.as_ref().unwrap_or(&DEFAULT_VOLUME_RE))
    };

    let normalized = raw_text.replace("\r\n", "\n").replace('\r', "\n");

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum HKind {
        Volume,
        Chapter,
    }

    struct RawSec {
        kind: HKind,
        title: String,
        under_volume: bool,
        body_start: usize,
        body_end: usize,
    }

    let mut sections: Vec<RawSec> = Vec::new();
    let mut seen_volume = false;
    let mut current: Option<(HKind, String, bool, usize)> = None;
    let mut cursor = 0usize;

    for raw_line in normalized.split_inclusive('\n') {
        let next_cursor = cursor + raw_line.len();
        let trimmed = raw_line.trim();
        let char_len = trimmed.chars().count();

        let matched_kind = if (1..=80).contains(&char_len) {
            if vol_re_opt.is_some_and(|re| re.is_match(trimmed)) {
                Some(HKind::Volume)
            } else if chap_re.is_match(trimmed)
                && (custom_chap.is_some() || !DEFAULT_PROSE_FALSE_CHAPTER_RE.is_match(trimmed))
            {
                Some(HKind::Chapter)
            } else {
                None
            }
        } else {
            None
        };

        if let Some(kind) = matched_kind {
            if let Some((pk, pt, p_under, p_start)) = current.take() {
                sections.push(RawSec {
                    kind: pk,
                    title: pt,
                    under_volume: p_under,
                    body_start: p_start,
                    body_end: cursor,
                });
            } else if cursor > 0 && !normalized[..cursor].trim().is_empty() {
                sections.push(RawSec {
                    kind: HKind::Chapter,
                    title: "前言 / 序".to_string(),
                    under_volume: false,
                    body_start: 0,
                    body_end: cursor,
                });
            }

            if kind == HKind::Volume {
                seen_volume = true;
                current = Some((HKind::Volume, trimmed.to_string(), false, next_cursor));
            } else {
                current = Some((
                    HKind::Chapter,
                    trimmed.to_string(),
                    seen_volume,
                    next_cursor,
                ));
            }
        }

        cursor = next_cursor;
    }

    if let Some((pk, pt, p_under, p_start)) = current {
        sections.push(RawSec {
            kind: pk,
            title: pt,
            under_volume: p_under,
            body_start: p_start,
            body_end: normalized.len(),
        });
    }

    if sections.is_empty() {
        let chunks = chunk_unstructured_text(&normalized, 6000);
        return Ok(chunks
            .into_iter()
            .enumerate()
            .map(|(idx, (title, plain))| {
                let xhtml = plain_to_xhtml_body(&title, &plain);
                EditableBookChapterDto {
                    id: format!("ch_{}", idx + 1),
                    title,
                    level: 0,
                    plain_text: plain,
                    xhtml_body: xhtml,
                    has_rich_media: false,
                    source_href: None,
                }
            })
            .collect());
    }

    let mut out = Vec::with_capacity(sections.len());
    for (idx, sec) in sections.into_iter().enumerate() {
        let plain = clean_plain_paragraphs(&normalized[sec.body_start..sec.body_end]);
        let level = if sec.kind == HKind::Volume || !sec.under_volume {
            0
        } else {
            1
        };
        let xhtml = plain_to_xhtml_body(&sec.title, &plain);
        out.push(EditableBookChapterDto {
            id: format!("ch_{}", idx + 1),
            title: sec.title,
            level,
            plain_text: plain,
            xhtml_body: xhtml,
            has_rich_media: false,
            source_href: None,
        });
    }
    Ok(out)
}

fn chunk_unstructured_text(text: &str, approx_chars: usize) -> Vec<(String, String)> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return vec![("第 1 章".to_string(), String::new())];
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
            let idx = out.len() + 1;
            out.push((format!("第 {idx} 节"), std::mem::take(&mut acc)));
            acc_chars = 0;
        }
    }
    if !acc.is_empty() {
        let idx = out.len() + 1;
        out.push((format!("第 {idx} 节"), acc));
    }
    out
}

pub(super) fn clean_plain_paragraphs(raw: &str) -> String {
    let mut lines = Vec::new();
    for line in raw.lines() {
        let t = line.trim();
        if !t.is_empty() {
            lines.push(t);
        }
    }
    lines.join("\n")
}

pub(super) fn save_txt_content(path: &Path, chapters: &[SaveBookChapterInput]) -> DomainResult<()> {
    let mut blocks = Vec::with_capacity(chapters.len());
    for (idx, ch) in chapters.iter().enumerate() {
        let title = if ch.title.trim().is_empty() {
            format!("第 {} 章", idx + 1)
        } else {
            ch.title.trim().to_string()
        };
        let body = match ch.mode {
            ChapterEditMode::Plain => clean_plain_paragraphs(&ch.plain_text),
            ChapterEditMode::Xhtml => xhtml_body_to_plain(&ch.xhtml_body, Some(&title)),
        };
        if body.is_empty() {
            blocks.push(title);
        } else {
            blocks.push(format!("{title}\n\n{body}"));
        }
    }
    let joined = format!("{}\n", blocks.join("\n\n\n"));
    atomic_write_bytes(path, joined.as_bytes())
}

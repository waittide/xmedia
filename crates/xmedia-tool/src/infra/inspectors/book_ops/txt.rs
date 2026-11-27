use std::path::Path;

use xmedia_core::domain::error::{DomainError, DomainResult};
pub(super) use xmedia_formats::book::txt::clean_plain_paragraphs;
use xmedia_formats::book::txt::{decode_txt_bytes, segment_txt_structure};

use crate::infra::atomic_fs::atomic_write_bytes;
use crate::infra::inspectors::map_format_err;

use super::xhtml::{plain_to_xhtml_body, xhtml_body_to_plain};
use super::{ChapterEditMode, EditableBookChapterDto, SaveBookChapterInput};

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
    let decoded = decode_txt_bytes(&bytes);
    segment_txt_chapters(&decoded, chapter_pattern, volume_pattern)
}

pub fn segment_txt_chapters(
    raw_text: &str,
    custom_chapter_pattern: Option<&str>,
    custom_volume_pattern: Option<&str>,
) -> DomainResult<Vec<EditableBookChapterDto>> {
    let segmented = segment_txt_structure(raw_text, custom_chapter_pattern, custom_volume_pattern)
        .map_err(map_format_err)?;

    let mut out = Vec::with_capacity(segmented.chapters.len() + 4);
    let mut emitted_volume: Option<String> = None;

    for ch in segmented.chapters {
        if let Some(ref vol) = ch.volume_title
            && emitted_volume.as_deref() != Some(vol.as_str())
            && ch.title != *vol
        {
            let vol_idx = out.len() + 1;
            let vol_xhtml = plain_to_xhtml_body(vol, "");
            out.push(EditableBookChapterDto {
                id: format!("ch_{vol_idx}"),
                title: vol.clone(),
                level: 0,
                plain_text: String::new(),
                xhtml_body: vol_xhtml,
                has_rich_media: false,
                source_href: None,
            });
            emitted_volume = Some(vol.clone());
        } else if ch.level == 0 && ch.volume_title.is_none() {
            emitted_volume = Some(ch.title.clone());
        }

        let plain = clean_plain_paragraphs(&ch.content);
        let level = ch.level.min(1);
        let xhtml = plain_to_xhtml_body(&ch.title, &plain);
        let next_idx = out.len() + 1;
        out.push(EditableBookChapterDto {
            id: format!("ch_{next_idx}"),
            title: ch.title,
            level,
            plain_text: plain,
            xhtml_body: xhtml,
            has_rich_media: false,
            source_href: None,
        });
    }

    Ok(out)
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

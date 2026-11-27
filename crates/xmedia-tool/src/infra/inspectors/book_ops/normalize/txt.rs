use std::path::Path;

use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_formats::book::{BookFormatExt, parse_book_bytes, read_book_text_chapter_bytes};
use xmedia_formats::decode_text_smart;

use super::image_compress::{compress_image_bytes, load_and_compress_sidecar_cover};
use super::{
    GeneratedChapter, NormalizeBookOutcome, NormalizeBookToEpubOptions,
    remove_source_and_stem_sidecars, write_fresh_normalized_epub,
};
use crate::infra::inspectors::book::read_book_meta_any;
use crate::infra::inspectors::book_ops::DEFAULT_EPUB_CSS;
use crate::infra::inspectors::book_ops::txt::segment_txt_chapters;
use crate::infra::inspectors::book_ops::xhtml::plain_to_xhtml_body;
use crate::infra::inspectors::map_format_err;

pub(super) fn normalize_txt_to_epub(
    txt_path: &Path,
    opts: &NormalizeBookToEpubOptions,
) -> DomainResult<NormalizeBookOutcome> {
    let before_bytes = std::fs::metadata(txt_path).map(|m| m.len()).unwrap_or(0);
    let epub_path = txt_path.with_extension("epub");
    if epub_path.exists() {
        return Err(DomainError::Conflict(format!(
            "目标文件 '{}' 已存在，请先重命名或移除已有同名 EPUB",
            epub_path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        )));
    }

    let meta = read_book_meta_any(txt_path)?;
    let bytes =
        std::fs::read(txt_path).map_err(|e| DomainError::Storage(format!("读取 TXT 失败: {e}")))?;
    let (decoded, _) = decode_text_smart(&bytes);
    let segmented = segment_txt_chapters(&decoded, None, None)?;
    let chapters: Vec<GeneratedChapter> = segmented
        .into_iter()
        .map(|c| GeneratedChapter {
            title: c.title,
            level: c.level,
            body_xhtml: c.xhtml_body,
        })
        .collect();

    let (cover, compressed_images) = load_and_compress_sidecar_cover(txt_path, opts);

    write_fresh_normalized_epub(
        &epub_path,
        &meta,
        &chapters,
        &[],
        cover,
        DEFAULT_EPUB_CSS,
        opts.generate_jacket,
    )?;

    let after_bytes = std::fs::metadata(&epub_path).map(|m| m.len()).unwrap_or(0);
    let deleted_source = if !opts.keep_source {
        remove_source_and_stem_sidecars(txt_path);
        true
    } else {
        false
    };

    Ok(NormalizeBookOutcome {
        output_path: epub_path,
        backup_path: None,
        source_format: "txt".to_string(),
        before_bytes,
        after_bytes,
        chapter_count: chapters.len(),
        compressed_images,
        removed_orphans: 0,
        deleted_source,
        skipped: false,
        skip_reason: None,
    })
}

pub(super) fn normalize_structured_text_book_to_epub(
    source_path: &Path,
    fmt: BookFormatExt,
    opts: &NormalizeBookToEpubOptions,
) -> DomainResult<NormalizeBookOutcome> {
    let before_bytes = std::fs::metadata(source_path).map(|m| m.len()).unwrap_or(0);
    let epub_path = source_path.with_extension("epub");
    if epub_path.exists() {
        return Err(DomainError::Conflict(format!(
            "目标文件 '{}' 已存在，请先重命名或移除已有同名 EPUB",
            epub_path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        )));
    }

    let meta = read_book_meta_any(source_path)?;
    let bytes = std::fs::read(source_path)
        .map_err(|e| DomainError::Storage(format!("读取电子书失败: {e}")))?;
    let pkg = parse_book_bytes(fmt, &bytes, &meta.title).map_err(map_format_err)?;
    let total = pkg.metadata.chapter_count.unwrap_or(1).max(1);
    let mut chapters = Vec::with_capacity(total as usize);
    for idx in 0..total {
        if let Ok((ch, _)) = read_book_text_chapter_bytes(fmt, &bytes, idx) {
            let xhtml = plain_to_xhtml_body(&ch.title, &ch.content);
            chapters.push(GeneratedChapter {
                title: ch.title,
                level: ch.level.min(1),
                body_xhtml: xhtml,
            });
        }
    }
    if chapters.is_empty() {
        chapters.push(GeneratedChapter {
            title: meta.title.clone(),
            level: 0,
            body_xhtml: plain_to_xhtml_body(&meta.title, ""),
        });
    }

    let (mut cover, mut compressed_images) = load_and_compress_sidecar_cover(source_path, opts);
    if cover.is_none()
        && let Some(emb) = pkg.cover
    {
        if opts.compress_images {
            let comp = compress_image_bytes(emb.bytes, &emb.mime_type, opts);
            if comp.changed {
                compressed_images += 1;
            }
            cover = Some((comp.bytes, comp.mime, comp.ext));
        } else {
            let ext = if emb.mime_type.contains("png") {
                "png"
            } else if emb.mime_type.contains("webp") {
                "webp"
            } else {
                "jpg"
            };
            let mime = if ext == "png" {
                "image/png"
            } else if ext == "webp" {
                "image/webp"
            } else {
                "image/jpeg"
            };
            cover = Some((emb.bytes, mime, ext));
        }
    }

    write_fresh_normalized_epub(
        &epub_path,
        &meta,
        &chapters,
        &[],
        cover,
        DEFAULT_EPUB_CSS,
        opts.generate_jacket,
    )?;

    let after_bytes = std::fs::metadata(&epub_path).map(|m| m.len()).unwrap_or(0);
    let deleted_source = if !opts.keep_source {
        remove_source_and_stem_sidecars(source_path);
        true
    } else {
        false
    };

    Ok(NormalizeBookOutcome {
        output_path: epub_path,
        backup_path: None,
        source_format: fmt.as_str().to_string(),
        before_bytes,
        after_bytes,
        chapter_count: chapters.len(),
        compressed_images,
        removed_orphans: 0,
        deleted_source,
        skipped: false,
        skip_reason: None,
    })
}

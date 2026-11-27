mod epub;
mod image_compress;
mod pdf;
mod txt;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};

use super::epub_ops::{
    FreshEpubBuildOptions, FreshEpubChapter, FreshEpubImage, write_fresh_epub_package,
};
use crate::application::ops::{
    BookImageCompressFormat, NormalizeBookOutcome, NormalizeBookToEpubOptions,
};
use crate::domain::media::BookFileMeta;

pub(super) fn is_orig_epub_backup_path(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
        return false;
    };
    let lower = name.to_ascii_lowercase();
    let Some(stem) = lower.strip_suffix(".epub") else {
        return false;
    };
    if stem.ends_with(".orig") {
        return true;
    }
    if let Some((prefix, suffix)) = stem.rsplit_once(".orig.") {
        return !prefix.is_empty() && !suffix.is_empty();
    }
    false
}

pub(super) fn unique_orig_epub_backup_path(epub_path: &Path) -> PathBuf {
    let parent = epub_path.parent().unwrap_or_else(|| Path::new(""));
    let stem = epub_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("book");
    let first = parent.join(format!("{stem}.orig.epub"));
    if !first.exists() {
        return first;
    }
    for idx in 2..1000 {
        let candidate = parent.join(format!("{stem}.orig.{idx}.epub"));
        if !candidate.exists() {
            return candidate;
        }
    }
    parent.join(format!("{stem}.orig.{}.epub", Uuid::now_v7()))
}

pub(super) fn remove_source_and_stem_sidecars(source_path: &Path) {
    let _ = std::fs::remove_file(source_path);
    // Clean exact-stem sidecar .opf and cover only (never touch shared directory metadata.opf/cover.jpg if other books exist)
    let stem_opf = source_path.with_extension("opf");
    if stem_opf.is_file() {
        let _ = std::fs::remove_file(stem_opf);
    }
    for ext in ["jpg", "jpeg", "png", "webp"] {
        let stem_cov = source_path.with_extension(ext);
        if stem_cov.is_file() {
            let _ = std::fs::remove_file(stem_cov);
        }
    }
}

/// Entry point: converts or normalizes a `.txt`, `.epub`, or `.pdf` book into a
/// clean, compressed EPUB 3.0 file.
pub(super) fn normalize_book_to_epub(
    source_path: &Path,
    opts: &NormalizeBookToEpubOptions,
) -> DomainResult<NormalizeBookOutcome> {
    if !source_path.is_file() {
        return Err(DomainError::NotFound(format!(
            "源文件不存在: {}",
            source_path.display()
        )));
    }
    let ext = source_path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    match ext.as_str() {
        "txt" => txt::normalize_txt_to_epub(source_path, opts),
        "epub" => epub::normalize_existing_epub(source_path, opts),
        "pdf" => pdf::normalize_pdf_to_epub(source_path, opts),
        "fb2" | "mobi" | "azw3" => {
            let fmt = xmedia_formats::book::BookFormatExt::from_extension(&ext)
                .expect("checked fb2/mobi/azw3");
            txt::normalize_structured_text_book_to_epub(source_path, fmt, opts)
        }
        other => Err(DomainError::Validation(format!(
            "暂不支持将 '.{other}' 转为规范化 EPUB（支持 txt / epub / pdf / fb2 / mobi / azw3）"
        ))),
    }
}

pub(super) type GeneratedChapter = FreshEpubChapter;
pub(super) type GeneratedImageAsset = FreshEpubImage;

#[allow(clippy::too_many_arguments)]
pub(super) fn write_fresh_normalized_epub(
    epub_path: &Path,
    meta: &BookFileMeta,
    chapters: &[GeneratedChapter],
    extra_images: &[GeneratedImageAsset],
    cover: Option<(Vec<u8>, &'static str, &'static str)>,
    css: &str,
    generate_jacket: bool,
) -> DomainResult<()> {
    write_fresh_epub_package(
        epub_path,
        meta,
        chapters,
        extra_images,
        cover.as_ref(),
        &FreshEpubBuildOptions {
            css,
            generate_jacket,
            mark_normalized: true,
            best_compression: true,
            cover_wrapper_class: "cover-page",
            cover_alt: &meta.title,
        },
    )
}

mod epub;
mod image_compress;
mod pdf;
mod txt;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};

use crate::domain::media::BookFileMeta;
use crate::infra::inspectors::book_content::{
    FreshEpubBuildOptions, FreshEpubChapter, FreshEpubImage, write_fresh_epub_package,
};

fn default_true() -> bool {
    true
}

fn default_max_image_dim() -> Option<u32> {
    Some(1600)
}

fn default_image_quality() -> u8 {
    82
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BookImageCompressFormat {
    #[default]
    Webp,
    Jpeg,
    Keep,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NormalizeBookToEpubOptions {
    /// Keep original source file (default: true).
    /// - For TXT/PDF: keeps source `.txt`/`.pdf` alongside newly created `.epub`.
    /// - For EPUB: backs up original to `<stem>.orig.epub` and writes normalized EPUB to `<stem>.epub`.
    #[serde(default = "default_true")]
    pub keep_source: bool,
    /// Compress cover & embedded illustrations.
    #[serde(default = "default_true")]
    pub compress_images: bool,
    /// Target format for compressed images: "webp" (default) | "jpeg" | "keep".
    #[serde(default)]
    pub image_format: BookImageCompressFormat,
    /// Maximum longest dimension in pixels (e.g. 1600; None or < 320 disables resize).
    #[serde(default = "default_max_image_dim")]
    pub max_image_dimension: Option<u32>,
    /// Quality for lossy JPEG compression/fallback (50..=95, default 82).
    #[serde(default = "default_image_quality")]
    pub image_quality: u8,
    /// Insert or refresh standardized book jacket page (`xmedia_jacket.xhtml`).
    #[serde(default = "default_true")]
    pub generate_jacket: bool,
    /// Remove unreferenced orphan files & OS junk (`__MACOSX`, `.DS_Store`, `Thumbs.db`).
    #[serde(default = "default_true")]
    pub remove_orphans: bool,
    /// For scanned/image-only PDFs without extractable text, convert page images into image-page EPUB.
    #[serde(default = "default_true")]
    pub pdf_scanned_fallback: bool,
    /// Skip EPUBs already tagged with `xmedia:normalized` and `.orig.epub` backup files (default: true).
    #[serde(default = "default_true")]
    pub skip_normalized: bool,
}

impl Default for NormalizeBookToEpubOptions {
    fn default() -> Self {
        Self {
            keep_source: true,
            compress_images: true,
            image_format: BookImageCompressFormat::Webp,
            max_image_dimension: default_max_image_dim(),
            image_quality: default_image_quality(),
            generate_jacket: true,
            remove_orphans: true,
            pdf_scanned_fallback: true,
            skip_normalized: true,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct NormalizeBookOutcome {
    pub output_path: PathBuf,
    pub backup_path: Option<PathBuf>,
    pub source_format: String,
    pub before_bytes: u64,
    pub after_bytes: u64,
    pub chapter_count: usize,
    pub compressed_images: usize,
    pub removed_orphans: usize,
    pub deleted_source: bool,
    pub skipped: bool,
    pub skip_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NormalizeBookItemReportDto {
    pub source_rel_path: String,
    pub output_rel_path: Option<String>,
    pub backup_rel_path: Option<String>,
    pub source_format: String,
    pub before_bytes: u64,
    pub after_bytes: u64,
    pub chapter_count: usize,
    pub compressed_images: usize,
    pub removed_orphans: usize,
    pub deleted_source: bool,
    pub skipped: bool,
    pub skip_reason: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NormalizeBooksBatchReportDto {
    pub total: usize,
    pub succeeded: usize,
    pub skipped: usize,
    pub failed: usize,
    pub total_before_bytes: u64,
    pub total_after_bytes: u64,
    pub items: Vec<NormalizeBookItemReportDto>,
}

/// Returns true if the filename matches an original EPUB backup produced by normalization
/// (e.g. `book.orig.epub` or `book.orig.2.epub`).
pub fn is_orig_epub_backup_path(path: &Path) -> bool {
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
pub fn normalize_book_to_epub(
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
        other => Err(DomainError::Validation(format!(
            "暂不支持将 '.{other}' 转为规范化 EPUB（支持 txt / epub / pdf）"
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

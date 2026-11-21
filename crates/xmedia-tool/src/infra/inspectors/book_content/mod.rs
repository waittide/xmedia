mod epub_io;
mod epub_ops;
mod txt;
mod xhtml;

#[cfg(test)]
mod tests;

use std::path::Path;

use regex::Regex;
use serde::{Deserialize, Serialize};
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::infra::stream::decode_text_smart;

use crate::infra::inspectors::book::read_book_meta_any;
use crate::infra::inspectors::zh_text::{
    BookTextTransformOptions, apply_text_transforms_to_chapter, rewrite_css_vertical_to_horizontal,
};

pub use epub_io::read_epub_internal_asset;
#[allow(unused_imports)]
pub(crate) use epub_io::{
    EpubStructureInfo, ManifestNode, epub_deflated_zip_options, epub_entry_zip_options,
    epub_stored_zip_options, ext_for_image_mime, find_cover_manifest_node, is_nav_manifest_node,
    parse_epub_structure, parse_nav_toc_map, parse_ncx_toc_map, percent_decode_path,
    read_epub_toc_map, read_zip_bytes, rebuild_opf_manifest_and_spine, relative_href_from_dir,
    render_nav_xhtml, render_toc_ncx, write_zip_entry,
};
pub(crate) use epub_ops::{
    FreshEpubBuildOptions, FreshEpubChapter, FreshEpubImage, write_fresh_epub_package,
};
pub use epub_ops::{convert_txt_to_epub, insert_epub_image, polish_epub_book};
pub use txt::{resegment_txt_file, segment_txt_chapters};
pub(crate) use xhtml::{extract_heading_from_xhtml, render_book_jacket_xhtml};
pub use xhtml::{plain_to_xhtml_body, xhtml_body_to_plain};

pub(crate) const DEFAULT_EPUB_CSS: &str = r#"body {
  font-family: -apple-system, BlinkMacSystemFont, "PingFang SC", "Noto Serif CJK SC", "Microsoft YaHei", serif;
  line-height: 1.8;
  margin: 0;
  padding: 1.2em 1.4em;
  word-wrap: break-word;
}
h1, h2, h3 {
  line-height: 1.35;
  margin: 1.2em 0 0.8em;
  font-weight: 700;
}
p {
  margin: 0 0 0.85em;
  text-indent: 2em;
  text-align: justify;
}
.cover-wrap {
  text-align: center;
  margin: 0;
  padding: 0;
}
.cover-wrap img {
  max-width: 100%;
  max-height: 98vh;
  object-fit: contain;
}
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ChapterEditMode {
    #[default]
    Plain,
    Xhtml,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditableBookChapterDto {
    pub id: String,
    pub title: String,
    /// `0` = Volume / Top-level chapter, `1` = Sub-chapter nested under preceding `0`
    pub level: u8,
    pub plain_text: String,
    pub xhtml_body: String,
    pub has_rich_media: bool,
    pub source_href: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookContentDocumentDto {
    pub format: String,
    pub title: String,
    pub authors: Vec<String>,
    #[serde(default)]
    pub stylesheets: Vec<String>,
    pub chapters: Vec<EditableBookChapterDto>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SaveBookChapterInput {
    pub title: String,
    #[serde(default)]
    pub level: u8,
    #[serde(default)]
    pub mode: ChapterEditMode,
    #[serde(default)]
    pub plain_text: String,
    #[serde(default)]
    pub xhtml_body: String,
    #[serde(default)]
    pub source_href: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformBookContentResultDto {
    pub chapters: Vec<EditableBookChapterDto>,
    pub stylesheets: Vec<String>,
    pub changed_chapters: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InsertedEpubImageDto {
    pub asset_path: String,
    pub chapter_rel_href: String,
    pub mime_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolishBookReportDto {
    pub jacket_updated: bool,
    pub removed_orphan_files: usize,
}

pub fn transform_book_content_batch(
    mut chapters: Vec<EditableBookChapterDto>,
    mut stylesheets: Vec<String>,
    opts: &BookTextTransformOptions,
) -> DomainResult<TransformBookContentResultDto> {
    let mut compiled_custom = Vec::new();
    for pat in &opts.custom_strip_patterns {
        let trimmed = pat.trim();
        if trimmed.is_empty() {
            continue;
        }
        let re = Regex::new(trimmed).map_err(|e| {
            DomainError::Validation(format!("自定义净化正则表达式无效 '{trimmed}': {e}"))
        })?;
        compiled_custom.push(re);
    }

    let mut changed_chapters = 0usize;
    for ch in &mut chapters {
        let (new_title, new_plain, new_xhtml) = apply_text_transforms_to_chapter(
            &ch.title,
            &ch.plain_text,
            &ch.xhtml_body,
            opts,
            &compiled_custom,
        );
        if new_title != ch.title || new_plain != ch.plain_text || new_xhtml != ch.xhtml_body {
            changed_chapters += 1;
            ch.title = new_title;
            ch.plain_text = new_plain;
            ch.xhtml_body = new_xhtml;
        }
    }

    if opts.vertical_to_horizontal {
        for css in &mut stylesheets {
            *css = rewrite_css_vertical_to_horizontal(css);
        }
    }

    Ok(TransformBookContentResultDto {
        chapters,
        stylesheets,
        changed_chapters,
    })
}

pub fn read_book_content(path: &Path) -> DomainResult<BookContentDocumentDto> {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let meta = read_book_meta_any(path)?;

    match ext.as_str() {
        "txt" => {
            let bytes = std::fs::read(path)
                .map_err(|e| DomainError::Storage(format!("failed reading txt file: {e}")))?;
            let (decoded, _) = decode_text_smart(&bytes);
            let chapters = segment_txt_chapters(&decoded, None, None)?;
            Ok(BookContentDocumentDto {
                format: "txt".to_string(),
                title: meta.title,
                authors: meta.authors,
                stylesheets: Vec::new(),
                chapters,
            })
        }
        "epub" => {
            let (chapters, stylesheets) = epub_io::read_epub_chapters_and_styles(path)?;
            Ok(BookContentDocumentDto {
                format: "epub".to_string(),
                title: meta.title,
                authors: meta.authors,
                stylesheets,
                chapters,
            })
        }
        _ => Err(DomainError::UnsupportedFormat(format!(
            "book content editing only supports TXT and EPUB (got '.{ext}')"
        ))),
    }
}

pub fn save_book_content(
    path: &Path,
    chapters: &[SaveBookChapterInput],
    stylesheets: Option<&[String]>,
    force_horizontal_spine: bool,
) -> DomainResult<()> {
    if chapters.is_empty() {
        return Err(DomainError::Validation("图书至少需要保留一个章节".into()));
    }
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    match ext.as_str() {
        "txt" => txt::save_txt_content(path, chapters),
        "epub" => epub_ops::save_epub_content(path, chapters, stylesheets, force_horizontal_spine),
        _ => Err(DomainError::UnsupportedFormat(format!(
            "book content saving only supports TXT and EPUB (got '.{ext}')"
        ))),
    }
}

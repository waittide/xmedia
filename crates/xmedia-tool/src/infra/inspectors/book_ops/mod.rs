mod epub_io;
mod epub_ops;
mod normalize;
mod txt;
mod xhtml;

#[cfg(test)]
mod tests;

use std::path::Path;

use regex::{Regex, RegexBuilder};
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_formats::decode_text_smart;

use crate::application::ops::{
    BookContentDocumentDto, BookToolCommand, BookToolOutput, ChapterEditMode,
    EditableBookChapterDto, InsertedEpubImageDto, PolishBookReportDto, SaveBookChapterInput,
    TransformBookContentResultDto,
};
use crate::domain::zh_text::{
    BookTextTransformOptions, apply_text_transforms_to_chapter, rewrite_css_vertical_to_horizontal,
};
use crate::infra::inspectors::book::{
    dir_has_single_book, is_generic_book_folder, read_book_meta_any,
};

pub(super) const DEFAULT_EPUB_CSS: &str = r#"body {
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

const USER_REGEX_SIZE_LIMIT: usize = 2 * 1024 * 1024;

pub(super) fn compile_bounded_user_regex(pattern: &str, label: &str) -> DomainResult<Regex> {
    RegexBuilder::new(pattern)
        .size_limit(USER_REGEX_SIZE_LIMIT)
        .dfa_size_limit(USER_REGEX_SIZE_LIMIT)
        .build()
        .map_err(|e| DomainError::Validation(format!("{label}无效 '{pattern}': {e}")))
}

pub fn run_book_tool(cmd: BookToolCommand) -> DomainResult<BookToolOutput> {
    match cmd {
        BookToolCommand::ReadEpubAsset { path, asset_path } => {
            epub_io::read_epub_internal_asset(&path, &asset_path).map(BookToolOutput::Binary)
        }
        BookToolCommand::ReadContent { path } => {
            read_book_content(&path).map(BookToolOutput::Document)
        }
        BookToolCommand::ResegmentTxt {
            path,
            chapter_pattern,
            volume_pattern,
        } => txt::resegment_txt_file(&path, chapter_pattern.as_deref(), volume_pattern.as_deref())
            .map(BookToolOutput::Chapters),
        BookToolCommand::SaveContent {
            path,
            chapters,
            stylesheets,
            force_horizontal_spine,
        } => {
            save_book_content(
                &path,
                &chapters,
                stylesheets.as_deref(),
                force_horizontal_spine,
            )?;
            Ok(BookToolOutput::Unit)
        }
        BookToolCommand::ConvertTxtToEpub {
            path,
            chapters,
            stylesheets,
            delete_source_txt,
        } => epub_ops::convert_txt_to_epub(
            &path,
            chapters.as_deref(),
            stylesheets.as_deref(),
            delete_source_txt,
        )
        .map(BookToolOutput::Path),
        BookToolCommand::TransformContent {
            chapters,
            stylesheets,
            options,
        } => transform_book_content_batch(chapters, stylesheets, &options)
            .map(BookToolOutput::Transform),
        BookToolCommand::InsertEpubImage {
            path,
            chapter_source_href,
            bytes,
            mime_hint,
        } => epub_ops::insert_epub_image(
            &path,
            chapter_source_href.as_deref(),
            &bytes,
            mime_hint.as_deref(),
        )
        .map(BookToolOutput::InsertedImage),
        BookToolCommand::PolishEpub { path } => {
            epub_ops::polish_epub_book(&path).map(BookToolOutput::Polished)
        }
        BookToolCommand::NormalizeToEpub { path, options } => {
            normalize::normalize_book_to_epub(&path, &options).map(BookToolOutput::Normalized)
        }
        BookToolCommand::PromoteCalibreSidecars {
            root_path,
            book_path,
        } => {
            promote_calibre_sidecars(&root_path, &book_path);
            Ok(BookToolOutput::Unit)
        }
    }
}

fn promote_calibre_sidecars(root_path: &Path, dst_abs: &Path) {
    if let Some(dst_dir) = dst_abs.parent()
        && dst_dir != root_path
        && dst_dir
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|n| !is_generic_book_folder(n))
        && dir_has_single_book(dst_dir)
    {
        let stem_opf = dst_abs.with_extension("opf");
        let calibre_opf = dst_dir.join("metadata.opf");
        if stem_opf.is_file() && !calibre_opf.exists() {
            let _ = std::fs::rename(&stem_opf, &calibre_opf);
        }
        for ext in ["jpg", "jpeg", "png", "webp"] {
            let stem_cov = dst_abs.with_extension(ext);
            let calibre_cov = dst_dir.join(format!("cover.{ext}"));
            if stem_cov.is_file() && !calibre_cov.exists() {
                let _ = std::fs::rename(&stem_cov, &calibre_cov);
            }
        }
    }
}

fn transform_book_content_batch(
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
        let re = compile_bounded_user_regex(trimmed, "自定义净化正则表达式")?;
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

fn read_book_content(path: &Path) -> DomainResult<BookContentDocumentDto> {
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
            let chapters = txt::segment_txt_chapters(&decoded, None, None)?;
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

fn save_book_content(
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

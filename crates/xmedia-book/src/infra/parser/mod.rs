mod svg_cover;

use std::path::Path;

use bytes::Bytes;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_formats::book::{
    BookFormatExt, BookTocEntry, ParsedBookMeta, epub, list_book_toc_bytes, opf, parse_book_bytes,
    read_book_text_chapter_bytes,
};
use xmedia_formats::error::FormatError;
use xmedia_source::proxy::validate_safe_raster_image;

use crate::application::ports::{
    BookAsset, BookParserPort, CoverPayload, ParsedBook, TocItem, TxtChapterContent,
};
use crate::domain::book::{BookFormat, BookMetadata};

use svg_cover::generate_placeholder_svg_cover;

pub(crate) fn to_format_ext(format: BookFormat) -> BookFormatExt {
    match format {
        BookFormat::Epub => BookFormatExt::Epub,
        BookFormat::Txt => BookFormatExt::Txt,
        BookFormat::Pdf => BookFormatExt::Pdf,
        BookFormat::Mobi => BookFormatExt::Mobi,
        BookFormat::Azw3 => BookFormatExt::Azw3,
        BookFormat::Fb2 => BookFormatExt::Fb2,
    }
}

pub(crate) fn map_format_err(err: FormatError) -> DomainError {
    match err {
        FormatError::Validation(msg) => DomainError::Validation(msg),
        FormatError::UnsupportedFormat(msg) => DomainError::UnsupportedFormat(msg),
        FormatError::NotFound(msg) => DomainError::NotFound(msg),
        FormatError::Conflict(msg) => DomainError::Conflict(msg),
        FormatError::Io(msg) => DomainError::Storage(msg),
    }
}

fn map_toc_entries(entries: Vec<BookTocEntry>) -> Vec<TocItem> {
    entries
        .into_iter()
        .map(|e| TocItem {
            index: e.index,
            title: e.title,
            href: e.href,
            level: e.level,
        })
        .collect()
}

fn map_parsed_meta(meta: ParsedBookMeta, fallback_title: &str) -> DomainResult<BookMetadata> {
    let title = if meta.title.trim().is_empty() {
        let fb = fallback_title.trim();
        if fb.is_empty() { "Untitled" } else { fb }
    } else {
        meta.title.trim()
    };
    let mut out = BookMetadata::new(title)?;
    out.authors = meta.authors;
    out.description = meta.description;
    out.isbn = meta.isbn;
    out.language = meta.language;
    out.publisher = meta.publisher;
    out.published_at = meta.published_at;
    out.tags = meta.tags;
    out.series = meta.series.or(meta.subseries);
    out.series_index = meta.series_index.filter(|v| v.is_finite() && *v >= 0.0);
    out.rating = meta
        .rating
        .filter(|r| r.is_finite() && (0.0..=10.0).contains(r));
    out.page_count = meta.page_count;
    out.chapter_count = meta.chapter_count;
    out.normalized()
}

#[derive(Default, Clone)]
pub struct CompositeBookParser;

impl BookParserPort for CompositeBookParser {
    fn parse_book(
        &self,
        format: BookFormat,
        file_bytes: &[u8],
        fallback_title: &str,
    ) -> DomainResult<ParsedBook> {
        self.parse_book_with_sidecars(format, file_bytes, fallback_title, None, None)
    }

    fn parse_book_with_sidecars(
        &self,
        format: BookFormat,
        file_bytes: &[u8],
        fallback_title: &str,
        sidecar_opf: Option<&[u8]>,
        sidecar_cover: Option<CoverPayload>,
    ) -> DomainResult<ParsedBook> {
        let mut pkg = parse_book_bytes(to_format_ext(format), file_bytes, fallback_title)
            .map_err(map_format_err)?;
        if let Some(opf_bytes) = sidecar_opf {
            opf::overlay_opf_sidecar_bytes(opf_bytes, &mut pkg.metadata);
        }

        let metadata = map_parsed_meta(pkg.metadata, fallback_title)?;

        let valid_sidecar_cover = sidecar_cover.and_then(|cov| {
            if cov.trusted_generated_svg {
                Some(cov)
            } else {
                validate_safe_raster_image(Some(&cov.mime_type), &cov.bytes)
                    .ok()
                    .map(|safe_mime| CoverPayload::new(cov.bytes, safe_mime))
            }
        });

        let cover = valid_sidecar_cover
            .or_else(|| {
                pkg.cover.and_then(|asset| {
                    validate_safe_raster_image(Some(&asset.mime_type), &asset.bytes)
                        .ok()
                        .map(|safe_mime| CoverPayload::new(Bytes::from(asset.bytes), safe_mime))
                })
            })
            .or_else(|| {
                Some(generate_placeholder_svg_cover(
                    &metadata.title,
                    metadata.authors.first().map(String::as_str),
                    &format.as_ref().to_ascii_uppercase(),
                ))
            });

        Ok(ParsedBook {
            metadata,
            cover,
            toc: map_toc_entries(pkg.toc),
        })
    }

    fn list_toc(&self, format: BookFormat, file_bytes: &[u8]) -> DomainResult<Vec<TocItem>> {
        let toc = list_book_toc_bytes(to_format_ext(format), file_bytes).map_err(map_format_err)?;
        Ok(map_toc_entries(toc))
    }

    fn list_toc_from_path(&self, format: BookFormat, path: &Path) -> DomainResult<Vec<TocItem>> {
        if format == BookFormat::Epub {
            let toc = epub::list_epub_toc_from_path(path).map_err(map_format_err)?;
            return Ok(map_toc_entries(toc));
        }
        let bytes = std::fs::read(path).map_err(|e| {
            DomainError::Storage(format!("failed to read book {}: {e}", path.display()))
        })?;
        self.list_toc(format, &bytes)
    }

    fn read_text_chapter(
        &self,
        format: BookFormat,
        file_bytes: &[u8],
        chapter_index: u32,
    ) -> DomainResult<TxtChapterContent> {
        let (ch, total_chapters) =
            read_book_text_chapter_bytes(to_format_ext(format), file_bytes, chapter_index)
                .map_err(map_format_err)?;
        Ok(TxtChapterContent {
            index: ch.index,
            volume_title: ch.volume_title,
            title: ch.title,
            content: ch.content,
            total_chapters,
        })
    }

    fn read_epub_resource(
        &self,
        file_bytes: &[u8],
        resource_path: &str,
    ) -> DomainResult<BookAsset> {
        let asset =
            epub::read_epub_resource_bytes(file_bytes, resource_path).map_err(map_format_err)?;
        Ok(BookAsset {
            bytes: Bytes::from(asset.bytes),
            mime_type: asset.mime_type,
        })
    }

    fn read_epub_resource_from_path(
        &self,
        path: &Path,
        resource_path: &str,
    ) -> DomainResult<BookAsset> {
        let asset =
            epub::read_epub_resource_from_path(path, resource_path).map_err(map_format_err)?;
        Ok(BookAsset {
            bytes: Bytes::from(asset.bytes),
            mime_type: asset.mime_type,
        })
    }
}

pub mod epub;
pub mod pdf;
mod svg_cover;
pub mod txt;

use xmedia_core::domain::error::DomainResult;

use crate::application::ports::{
    BookAsset, BookParserPort, CoverPayload, ParsedBook, TocItem, TxtChapterContent,
};
use crate::domain::book::BookFormat;

#[derive(Default, Clone)]
pub struct CompositeBookParser;

impl BookParserPort for CompositeBookParser {
    fn parse_book(
        &self,
        format: BookFormat,
        file_bytes: &[u8],
        fallback_title: &str,
    ) -> DomainResult<ParsedBook> {
        match format {
            BookFormat::Epub => epub::parse_epub(file_bytes, fallback_title),
            BookFormat::Txt => txt::parse_txt(file_bytes, fallback_title),
            BookFormat::Pdf => pdf::parse_pdf(file_bytes, fallback_title),
        }
    }

    fn parse_book_with_sidecars(
        &self,
        format: BookFormat,
        file_bytes: &[u8],
        fallback_title: &str,
        sidecar_opf: Option<&[u8]>,
        sidecar_cover: Option<CoverPayload>,
    ) -> DomainResult<ParsedBook> {
        let mut parsed = self.parse_book(format, file_bytes, fallback_title)?;
        if let Some(opf) = sidecar_opf {
            epub::overlay_opf_sidecar(opf, &mut parsed);
        }
        if let Some(cov) = sidecar_cover
            && (cov.trusted_generated_svg
                || crate::application::online::validate_safe_cover_image(
                    Some(&cov.mime_type),
                    &cov.bytes,
                )
                .is_ok())
        {
            parsed.cover = Some(cov);
        }
        Ok(parsed)
    }

    fn list_toc(&self, format: BookFormat, file_bytes: &[u8]) -> DomainResult<Vec<TocItem>> {
        match format {
            BookFormat::Epub => epub::list_epub_toc(file_bytes),
            BookFormat::Txt => Ok(txt::list_txt_toc(file_bytes)),
            BookFormat::Pdf => pdf::list_pdf_toc(file_bytes),
        }
    }

    fn list_toc_from_path(
        &self,
        format: BookFormat,
        path: &std::path::Path,
    ) -> DomainResult<Vec<TocItem>> {
        match format {
            BookFormat::Epub => epub::list_epub_toc_from_path(path),
            BookFormat::Txt | BookFormat::Pdf => {
                let bytes = std::fs::read(path).map_err(|e| {
                    xmedia_core::domain::error::DomainError::Storage(format!(
                        "failed to read book {}: {e}",
                        path.display()
                    ))
                })?;
                self.list_toc(format, &bytes)
            }
        }
    }

    fn read_txt_chapter(
        &self,
        file_bytes: &[u8],
        chapter_index: u32,
    ) -> DomainResult<TxtChapterContent> {
        txt::read_txt_chapter(file_bytes, chapter_index)
    }

    fn read_epub_resource(
        &self,
        file_bytes: &[u8],
        resource_path: &str,
    ) -> DomainResult<BookAsset> {
        epub::read_epub_resource(file_bytes, resource_path)
    }

    fn read_epub_resource_from_path(
        &self,
        path: &std::path::Path,
        resource_path: &str,
    ) -> DomainResult<BookAsset> {
        epub::read_epub_resource_from_path(path, resource_path)
    }
}

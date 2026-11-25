use std::path::{Path, PathBuf};

use xmedia_core::domain::error::DomainResult;
use xmedia_formats::book::pdf::apply_pdf_info_from_path;
use xmedia_formats::book::sidecar as fmt_sidecar;
use xmedia_formats::book::{BookFormatExt, ParsedBookMeta, parse_book_bytes};

use super::epub_zip::parse_epub_archive;
use super::opf::{parse_opf_bytes, parsed_book_to_file_meta};
use crate::domain::media::BookFileMeta;

pub(super) const BOOK_EXTENSIONS: &[&str] = BookFormatExt::ALL_EXTENSIONS;

pub(crate) use fmt_sidecar::{dir_has_single_book, is_generic_book_folder};

pub(super) fn is_dedicated_calibre_book_dir(dir: &Path) -> bool {
    fmt_sidecar::is_dedicated_book_dir(dir, None)
}

pub(super) fn find_existing_sidecar_opf(book_path: &Path) -> Option<PathBuf> {
    fmt_sidecar::find_existing_sidecar_opf(book_path, None)
}

pub(super) fn preferred_write_opf_path(book_path: &Path) -> PathBuf {
    fmt_sidecar::preferred_write_opf_path(book_path, None)
}

pub(crate) fn find_sidecar_cover(book_path: &Path) -> Option<PathBuf> {
    fmt_sidecar::find_sidecar_cover(book_path, None)
}

fn merge_sidecar_into_book_meta(meta: &mut BookFileMeta, sidecar: BookFileMeta) {
    if !sidecar.title.trim().is_empty() {
        meta.title = sidecar.title;
    }
    if !sidecar.authors.is_empty() {
        meta.authors = sidecar.authors;
    }
    if sidecar.series.is_some() {
        meta.series = sidecar.series;
    }
    if sidecar.subseries.is_some() {
        meta.subseries = sidecar.subseries;
    }
    if sidecar.series_index.is_some() {
        meta.series_index = sidecar.series_index;
    }
    if sidecar.rating.is_some() {
        meta.rating = sidecar.rating;
    }
    if sidecar.description.is_some() {
        meta.description = sidecar.description;
    }
    if sidecar.publisher.is_some() {
        meta.publisher = sidecar.publisher;
    }
    if sidecar.published_at.is_some() {
        meta.published_at = sidecar.published_at;
    }
    if sidecar.language.is_some() {
        meta.language = sidecar.language;
    }
    if sidecar.isbn.is_some() {
        meta.isbn = sidecar.isbn;
    }
    if !sidecar.tags.is_empty() {
        meta.tags = sidecar.tags;
    }
    if !sidecar.locked_fields.is_empty() {
        meta.locked_fields = sidecar.locked_fields;
    }
}

pub(crate) fn read_book_meta_any(path: &Path) -> DomainResult<BookFileMeta> {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("epub")
        .to_ascii_lowercase();
    let fallback_title = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Untitled")
        .to_string();

    if ext == "epub" {
        let mut meta = parse_epub_archive(path)?.meta;
        if let Some(opf_path) = find_existing_sidecar_opf(path)
            && let Ok(bytes) = std::fs::read(&opf_path)
        {
            let sidecar = parse_opf_bytes(&bytes, &ext, &meta.title).meta;
            merge_sidecar_into_book_meta(&mut meta, sidecar);
        }
        meta.has_cover = meta.has_cover || find_sidecar_cover(path).is_some();
        return Ok(meta);
    }

    let fmt_ext = BookFormatExt::from_extension(&ext).unwrap_or(BookFormatExt::Txt);
    let mut meta = match fmt_ext {
        BookFormatExt::Pdf => {
            let mut pm = ParsedBookMeta::with_fallback(BookFormatExt::Pdf, &fallback_title);
            apply_pdf_info_from_path(path, &mut pm);
            parsed_book_to_file_meta(pm, &ext)
        }
        BookFormatExt::Fb2 | BookFormatExt::Mobi | BookFormatExt::Azw3 => std::fs::read(path)
            .ok()
            .and_then(|bytes| parse_book_bytes(fmt_ext, &bytes, &fallback_title).ok())
            .map(|pkg| parsed_book_to_file_meta(pkg.metadata, &ext))
            .unwrap_or_else(|| BookFileMeta {
                format: ext.clone(),
                title: fallback_title.clone(),
                writable: true,
                ..Default::default()
            }),
        BookFormatExt::Txt | BookFormatExt::Epub => BookFileMeta {
            format: ext.clone(),
            title: fallback_title.clone(),
            writable: true,
            ..Default::default()
        },
    };

    let embedded_has_cover = meta.has_cover;
    if let Some(opf_path) = find_existing_sidecar_opf(path)
        && let Ok(bytes) = std::fs::read(&opf_path)
    {
        let sidecar = parse_opf_bytes(&bytes, &ext, &meta.title).meta;
        merge_sidecar_into_book_meta(&mut meta, sidecar);
    }

    meta.is_normalized = false;
    meta.has_cover = embedded_has_cover || find_sidecar_cover(path).is_some();
    Ok(meta)
}

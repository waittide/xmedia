use xmedia_formats::book::opf::{
    parse_opf_xml, render_sidecar_opf as fmt_render_sidecar_opf,
    replace_opf_metadata_block as fmt_replace_opf_metadata_block,
};
pub(crate) use xmedia_formats::book::opf::{swap_href_extension, update_manifest_cover_item};
use xmedia_formats::book::{BookFormatExt, ParsedBookMeta};
use xmedia_formats::common::format_clean_float;

use crate::domain::media::BookFileMeta;

pub(crate) fn parsed_book_to_file_meta(parsed: ParsedBookMeta, format_str: &str) -> BookFileMeta {
    BookFileMeta {
        format: format_str.to_string(),
        title: parsed.title,
        authors: parsed.authors,
        series: parsed.series,
        subseries: parsed.subseries,
        series_index: parsed
            .series_index
            .map(format_clean_float)
            .filter(|s| !s.is_empty()),
        rating: parsed.rating,
        description: parsed.description,
        publisher: parsed.publisher,
        published_at: parsed.published_at,
        language: parsed.language,
        isbn: parsed.isbn,
        tags: parsed.tags,
        locked_fields: parsed.locked_fields,
        is_normalized: parsed.is_normalized,
        has_cover: parsed.has_cover,
        writable: true,
    }
}

pub(crate) fn file_meta_to_parsed_book(meta: &BookFileMeta) -> ParsedBookMeta {
    let format = BookFormatExt::from_extension(&meta.format).unwrap_or(BookFormatExt::Epub);
    let series_index = meta
        .series_index
        .as_deref()
        .and_then(|s| s.trim().parse::<f64>().ok())
        .filter(|v| v.is_finite() && *v >= 0.0);
    ParsedBookMeta {
        format,
        title: meta.title.clone(),
        authors: meta.authors.clone(),
        description: meta.description.clone(),
        isbn: meta.isbn.clone(),
        language: meta.language.clone(),
        publisher: meta.publisher.clone(),
        published_at: meta.published_at.clone(),
        tags: meta.tags.clone(),
        series: meta.series.clone(),
        subseries: meta.subseries.clone(),
        series_index,
        rating: meta.rating,
        page_count: None,
        chapter_count: None,
        locked_fields: meta.locked_fields.clone(),
        is_normalized: meta.is_normalized,
        has_cover: meta.has_cover,
    }
}

pub(super) struct ParsedOpfData {
    pub(super) meta: BookFileMeta,
}

pub(super) fn parse_opf_bytes(
    opf_bytes: &[u8],
    format: &str,
    fallback_title: &str,
) -> ParsedOpfData {
    let fmt = BookFormatExt::from_extension(format).unwrap_or(BookFormatExt::Epub);
    let doc = parse_opf_xml(opf_bytes, fmt, fallback_title);
    ParsedOpfData {
        meta: parsed_book_to_file_meta(doc.meta, format),
    }
}

pub(crate) fn replace_opf_metadata_block(
    original_opf: &str,
    meta: &BookFileMeta,
    cover_id_hint: Option<&str>,
    unique_id_hint: &str,
    package_version_hint: &str,
) -> String {
    let parsed = file_meta_to_parsed_book(meta);
    fmt_replace_opf_metadata_block(
        original_opf,
        &parsed,
        cover_id_hint,
        unique_id_hint,
        package_version_hint,
    )
}

pub(super) fn render_sidecar_opf(meta: &BookFileMeta) -> String {
    let parsed = file_meta_to_parsed_book(meta);
    fmt_render_sidecar_opf(&parsed)
}

use std::path::Path;

use quick_xml::events::BytesStart;
use xmedia_core::domain::error::DomainResult;
use xmedia_formats::book::epub::{
    find_opf_path_in_container as fmt_find_opf_path_in_container, inspect_epub_path,
    normalize_zip_path,
};
use xmedia_formats::book::epub_write::rewrite_epub_file_entries;
use xmedia_formats::common::xml_attr_value;

use super::opf::parsed_book_to_file_meta;
use crate::domain::media::BookFileMeta;
use crate::infra::inspectors::map_format_err;

pub(crate) fn attr_value(e: &BytesStart<'_>, local_key: &str) -> Option<String> {
    xml_attr_value(e, local_key)
}

pub(crate) fn normalize_zip_rel(base_dir: &Path, href: &str) -> String {
    normalize_zip_path(base_dir, href)
}

pub(crate) fn find_opf_path_in_container(container_xml: &[u8]) -> Option<String> {
    fmt_find_opf_path_in_container(container_xml)
}

#[derive(Default)]
pub(super) struct ParsedEpubPackage {
    pub(super) opf_path: String,
    pub(super) package_version: String,
    pub(super) unique_identifier_id: String,
    pub(super) cover_entry_path: Option<String>,
    pub(super) cover_item_id: Option<String>,
    pub(super) cover_media_type: Option<String>,
    pub(super) cover_raw_href: Option<String>,
    pub(super) meta: BookFileMeta,
}

pub(super) fn parse_epub_archive(path: &Path) -> DomainResult<ParsedEpubPackage> {
    let inspected = inspect_epub_path(path).map_err(map_format_err)?;
    Ok(ParsedEpubPackage {
        opf_path: inspected.opf_path,
        package_version: inspected.package_version,
        unique_identifier_id: inspected.unique_identifier_id,
        cover_entry_path: inspected.cover_entry_path,
        cover_item_id: inspected.cover_item_id,
        cover_media_type: inspected.cover_media_type,
        cover_raw_href: inspected.cover_raw_href,
        meta: parsed_book_to_file_meta(inspected.meta, "epub"),
    })
}

pub(crate) fn rewrite_epub_entries(
    epub_path: &Path,
    replacements: Vec<(String, Vec<u8>)>,
    skip_paths: &[String],
) -> DomainResult<()> {
    rewrite_epub_file_entries(epub_path, replacements, skip_paths).map_err(map_format_err)
}

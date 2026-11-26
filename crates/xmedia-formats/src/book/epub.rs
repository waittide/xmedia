use std::fs::File;
use std::io::{BufReader, Cursor, Read, Seek};
use std::path::Path;

use quick_xml::Reader;
use quick_xml::events::Event;
use zip::ZipArchive;

pub use super::epub_toc::{parse_nav_xhtml_toc, parse_ncx_toc};
use super::opf::{OpfManifestItem, ParsedOpfDocument, parse_opf_xml};
use super::{BookFormatExt, BookTocEntry, ParsedBookMeta, ParsedBookPackage};
use crate::common::{BinaryAsset, normalize_zip_path, sniff_raster_image_mime, xml_attr_value};
use crate::error::{FormatError, FormatResult};

const MAX_EPUB_METADATA_BYTES: u64 = 4 * 1024 * 1024;
const MAX_EPUB_RESOURCE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_ZIP_PREALLOC_BYTES: usize = 1024 * 1024;

enum BoundedZipRead {
    Ok(Vec<u8>),
    NotFound,
    TooLarge,
}

#[derive(Debug, Clone)]
pub struct InspectedEpubArchive {
    pub opf_path: String,
    pub package_version: String,
    pub unique_identifier_id: String,
    pub cover_entry_path: Option<String>,
    pub cover_item_id: Option<String>,
    pub cover_media_type: Option<String>,
    pub cover_raw_href: Option<String>,
    pub meta: ParsedBookMeta,
}

fn guess_epub_mime(path: &str) -> &'static str {
    let ext = Path::new(path)
        .extension()
        .and_then(|s| s.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    match ext.as_str() {
        "xhtml" | "xht" => "application/xhtml+xml",
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "application/javascript",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "avif" => "image/avif",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "xml" | "opf" => "application/xml",
        "ncx" => "application/x-dtbncx+xml",
        "json" => "application/json",
        _ => "application/octet-stream",
    }
}

fn read_bounded_zip_file<R: Read>(
    file: &mut R,
    declared_size: u64,
    max_bytes: u64,
) -> BoundedZipRead {
    let cap = usize::try_from(declared_size)
        .unwrap_or(0)
        .min(MAX_ZIP_PREALLOC_BYTES)
        .min(usize::try_from(max_bytes).unwrap_or(MAX_ZIP_PREALLOC_BYTES));
    let mut buf = Vec::with_capacity(cap);
    if file
        .by_ref()
        .take(max_bytes.saturating_add(1))
        .read_to_end(&mut buf)
        .is_err()
    {
        return BoundedZipRead::NotFound;
    }
    if buf.len() as u64 > max_bytes {
        return BoundedZipRead::TooLarge;
    }
    BoundedZipRead::Ok(buf)
}

fn read_zip_entry_bounded<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    name: &str,
    max_bytes: u64,
) -> BoundedZipRead {
    if let Ok(mut file) = archive.by_name(name) {
        let size = file.size();
        return read_bounded_zip_file(&mut file, size, max_bytes);
    }
    const MAX_CASE_FALLBACK_SCAN: usize = 8192;
    let target_lower = name.to_lowercase();
    let scan_limit = archive.len().min(MAX_CASE_FALLBACK_SCAN);
    let mut matched_idx = None;
    for idx in 0..scan_limit {
        if let Ok(entry) = archive.by_index(idx)
            && let Ok(entry_name) = entry.name()
            && entry_name.to_lowercase() == target_lower
        {
            matched_idx = Some(idx);
            break;
        }
    }
    let Some(idx) = matched_idx else {
        return BoundedZipRead::NotFound;
    };
    let Ok(mut file) = archive.by_index(idx) else {
        return BoundedZipRead::NotFound;
    };
    let size = file.size();
    read_bounded_zip_file(&mut file, size, max_bytes)
}

fn read_zip_entry<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    name: &str,
    max_bytes: u64,
) -> Option<Vec<u8>> {
    match read_zip_entry_bounded(archive, name, max_bytes) {
        BoundedZipRead::Ok(bytes) => Some(bytes),
        BoundedZipRead::NotFound | BoundedZipRead::TooLarge => None,
    }
}

pub fn find_opf_path_in_container(container_xml: &[u8]) -> Option<String> {
    let xml_str = String::from_utf8_lossy(container_xml);
    let mut reader = Reader::from_str(&xml_str);
    reader.config_mut().trim_text(true);
    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e) | Event::Empty(ref e))
                if e.local_name().as_ref() == "rootfile" =>
            {
                if let Some(full_path) = xml_attr_value(e, "full-path") {
                    return Some(full_path);
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    None
}

fn select_cover_manifest_item(opf: &ParsedOpfDocument) -> Option<&OpfManifestItem> {
    opf.manifest
        .iter()
        .find(|item| {
            item.media_type.starts_with("image/")
                && item
                    .properties
                    .split_whitespace()
                    .any(|p| p == "cover-image")
        })
        .or_else(|| {
            opf.cover_meta_id.as_ref().and_then(|cid| {
                opf.manifest
                    .iter()
                    .find(|item| &item.id == cid && item.media_type.starts_with("image/"))
            })
        })
        .or_else(|| {
            opf.manifest.iter().find(|item| {
                item.media_type.starts_with("image/")
                    && (item.id.to_ascii_lowercase().contains("cover")
                        || item.href.to_ascii_lowercase().contains("cover"))
            })
        })
}

fn extract_safe_cover<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    opf_dir: &Path,
    opf: &ParsedOpfDocument,
) -> Option<BinaryAsset> {
    let candidate = select_cover_manifest_item(opf)?;
    if candidate.media_type.to_ascii_lowercase().contains("svg") {
        return None;
    }
    let zip_path = normalize_zip_path(opf_dir, &candidate.href);
    let bytes = read_zip_entry(archive, &zip_path, MAX_EPUB_RESOURCE_BYTES)?;
    let safe_mime = sniff_raster_image_mime(&bytes)?;
    Some(BinaryAsset::new(bytes, safe_mime))
}

fn read_archive_opf<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    fallback_title: &str,
) -> FormatResult<(String, ParsedOpfDocument)> {
    let container_bytes =
        read_zip_entry(archive, "META-INF/container.xml", MAX_EPUB_METADATA_BYTES)
            .ok_or_else(|| FormatError::Validation("EPUB missing META-INF/container.xml".into()))?;
    let opf_path = find_opf_path_in_container(&container_bytes)
        .ok_or_else(|| FormatError::Validation("EPUB container.xml missing rootfile".into()))?;
    let opf_bytes = read_zip_entry(archive, &opf_path, MAX_EPUB_METADATA_BYTES)
        .ok_or_else(|| FormatError::Validation(format!("EPUB missing OPF package: {opf_path}")))?;
    let opf = parse_opf_xml(&opf_bytes, BookFormatExt::Epub, fallback_title);
    Ok((opf_path, opf))
}

fn extract_toc<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    opf_dir: &Path,
    opf: &ParsedOpfDocument,
) -> Vec<BookTocEntry> {
    let by_id = opf.manifest_by_id();
    let mut nav_toc = Vec::new();
    if let Some(nav_item) = opf
        .manifest
        .iter()
        .find(|item| item.properties.split_whitespace().any(|p| p == "nav"))
    {
        let nav_path = normalize_zip_path(opf_dir, &nav_item.href);
        let nav_dir = Path::new(&nav_path).parent().unwrap_or(Path::new(""));
        if let Some(bytes) = read_zip_entry(archive, &nav_path, MAX_EPUB_METADATA_BYTES) {
            nav_toc = parse_nav_xhtml_toc(&bytes, nav_dir);
        }
    }

    let mut ncx_toc = Vec::new();
    let ncx_item = opf
        .spine_toc_id
        .as_deref()
        .and_then(|id| by_id.get(id).copied())
        .or_else(|| {
            opf.manifest
                .iter()
                .find(|i| i.media_type.contains("dtbncx"))
        });
    if let Some(ncx) = ncx_item {
        let ncx_path = normalize_zip_path(opf_dir, &ncx.href);
        let ncx_dir = Path::new(&ncx_path).parent().unwrap_or(Path::new(""));
        if let Some(bytes) = read_zip_entry(archive, &ncx_path, MAX_EPUB_METADATA_BYTES) {
            ncx_toc = parse_ncx_toc(&bytes, ncx_dir);
        }
    }

    if ncx_toc.len() > nav_toc.len() && nav_toc.len() <= 1 {
        return ncx_toc;
    }
    if !nav_toc.is_empty() {
        return nav_toc;
    }
    if !ncx_toc.is_empty() {
        return ncx_toc;
    }

    opf.spine_ids
        .iter()
        .filter_map(|id| by_id.get(id.as_str()).copied())
        .enumerate()
        .map(|(i, item)| BookTocEntry {
            index: u32::try_from(i).unwrap_or(0),
            title: format!("Section {}", i + 1),
            href: normalize_zip_path(opf_dir, &item.href),
            level: 0,
        })
        .collect()
}

pub fn parse_epub_reader<R: Read + Seek>(
    reader: R,
    fallback_title: &str,
) -> FormatResult<ParsedBookPackage> {
    let mut archive = ZipArchive::new(reader)
        .map_err(|e| FormatError::Validation(format!("invalid EPUB archive: {e}")))?;
    let (opf_path, mut opf) = read_archive_opf(&mut archive, fallback_title)?;
    let opf_dir = Path::new(&opf_path).parent().unwrap_or(Path::new(""));
    let toc = extract_toc(&mut archive, opf_dir, &opf);
    opf.meta.chapter_count = u32::try_from(toc.len().max(opf.spine_ids.len())).ok();
    let cover = extract_safe_cover(&mut archive, opf_dir, &opf);
    opf.meta.has_cover = cover.is_some();

    Ok(ParsedBookPackage {
        metadata: opf.meta,
        cover,
        toc,
    })
}

pub fn parse_epub_bytes(bytes: &[u8], fallback_title: &str) -> FormatResult<ParsedBookPackage> {
    parse_epub_reader(Cursor::new(bytes), fallback_title)
}

pub fn inspect_epub_path(path: &Path) -> FormatResult<InspectedEpubArchive> {
    let file = File::open(path)
        .map_err(|e| FormatError::Io(format!("failed opening '{}': {e}", path.display())))?;
    let mut archive = ZipArchive::new(BufReader::new(file))
        .map_err(|e| FormatError::Validation(format!("invalid epub archive: {e}")))?;
    let fallback_title = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Untitled");
    let (opf_path, mut opf) = read_archive_opf(&mut archive, fallback_title)?;
    let opf_dir = Path::new(&opf_path).parent().unwrap_or(Path::new(""));
    let cover_item = select_cover_manifest_item(&opf).cloned();
    let cover_href = cover_item
        .as_ref()
        .map(|item| normalize_zip_path(opf_dir, &item.href));
    opf.meta.has_cover = cover_href
        .as_ref()
        .is_some_and(|cp| archive.by_name(cp).is_ok());

    Ok(InspectedEpubArchive {
        opf_path,
        package_version: opf.package_version,
        unique_identifier_id: opf.unique_identifier_id,
        cover_entry_path: cover_href,
        cover_item_id: cover_item.as_ref().map(|i| i.id.clone()),
        cover_media_type: cover_item.as_ref().map(|i| i.media_type.clone()),
        cover_raw_href: cover_item.map(|i| i.href),
        meta: opf.meta,
    })
}

pub fn list_epub_toc_from_reader<R: Read + Seek>(reader: R) -> FormatResult<Vec<BookTocEntry>> {
    let mut archive = ZipArchive::new(reader)
        .map_err(|e| FormatError::Validation(format!("invalid EPUB archive: {e}")))?;
    let (opf_path, opf) = read_archive_opf(&mut archive, "Untitled")?;
    let opf_dir = Path::new(&opf_path).parent().unwrap_or(Path::new(""));
    Ok(extract_toc(&mut archive, opf_dir, &opf))
}

pub fn list_epub_toc_bytes(bytes: &[u8]) -> FormatResult<Vec<BookTocEntry>> {
    list_epub_toc_from_reader(Cursor::new(bytes))
}

pub fn list_epub_toc_from_path(path: &Path) -> FormatResult<Vec<BookTocEntry>> {
    let file = File::open(path)
        .map_err(|e| FormatError::Io(format!("failed to open book {}: {e}", path.display())))?;
    list_epub_toc_from_reader(BufReader::new(file))
}

pub fn read_epub_resource_from_reader<R: Read + Seek>(
    reader: R,
    resource_path: &str,
) -> FormatResult<BinaryAsset> {
    let normalized = normalize_zip_path(Path::new(""), resource_path.trim_start_matches('/'));
    if normalized.is_empty() {
        return Err(FormatError::Validation("empty EPUB resource path".into()));
    }
    let mut archive = ZipArchive::new(reader)
        .map_err(|e| FormatError::Validation(format!("invalid EPUB archive: {e}")))?;
    let bytes = match read_zip_entry_bounded(&mut archive, &normalized, MAX_EPUB_RESOURCE_BYTES) {
        BoundedZipRead::Ok(bytes) => bytes,
        BoundedZipRead::TooLarge => {
            return Err(FormatError::Validation(format!(
                "epub resource {normalized} exceeds maximum allowed size"
            )));
        }
        BoundedZipRead::NotFound => {
            return Err(FormatError::NotFound(format!(
                "epub resource {normalized} not found"
            )));
        }
    };
    Ok(BinaryAsset::new(bytes, guess_epub_mime(&normalized)))
}

pub fn read_epub_resource_bytes(bytes: &[u8], resource_path: &str) -> FormatResult<BinaryAsset> {
    read_epub_resource_from_reader(Cursor::new(bytes), resource_path)
}

pub fn read_epub_resource_from_path(path: &Path, resource_path: &str) -> FormatResult<BinaryAsset> {
    let file = File::open(path)
        .map_err(|e| FormatError::Io(format!("failed to open book {}: {e}", path.display())))?;
    read_epub_resource_from_reader(BufReader::new(file), resource_path)
}

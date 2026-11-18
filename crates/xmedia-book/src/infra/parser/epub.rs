use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::path::{Component, Path, PathBuf};

use bytes::Bytes;
use quick_xml::Reader;
use quick_xml::escape::unescape;
use quick_xml::events::{BytesStart, Event};
use xmedia_core::domain::error::{DomainError, DomainResult};
use zip::ZipArchive;

use crate::application::ports::{BookAsset, CoverPayload, ParsedBook, TocItem};
use crate::domain::book::BookMetadata;

use super::svg_cover::generate_placeholder_svg_cover;

#[derive(Debug, Clone)]
struct ManifestItem {
    href: String,
    media_type: String,
    properties: String,
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

fn normalize_zip_path(base_dir: &Path, relative: &str) -> String {
    let clean_rel = relative.split('#').next().unwrap_or(relative);
    let joined = base_dir.join(clean_rel);
    let mut out = PathBuf::new();
    for comp in joined.components() {
        match comp {
            Component::Normal(part) => out.push(part),
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir | Component::RootDir | Component::Prefix(_) => {}
        }
    }
    out.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

fn read_zip_entry(archive: &mut ZipArchive<Cursor<&[u8]>>, name: &str) -> Option<Vec<u8>> {
    if let Ok(mut file) = archive.by_name(name) {
        let mut buf = Vec::with_capacity(usize::try_from(file.size()).unwrap_or(0));
        if file.read_to_end(&mut buf).is_ok() {
            return Some(buf);
        }
    }
    let target_lower = name.to_lowercase();
    let mut matched_idx = None;
    for idx in 0..archive.len() {
        if let Ok(entry) = archive.by_index(idx)
            && let Ok(entry_name) = entry.name()
            && entry_name.to_lowercase() == target_lower
        {
            matched_idx = Some(idx);
            break;
        }
    }
    let idx = matched_idx?;
    let mut file = archive.by_index(idx).ok()?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf).ok()?;
    Some(buf)
}

fn attr_value(e: &BytesStart<'_>, local_key: &str) -> Option<String> {
    for attr in e.attributes().flatten() {
        if attr.key.local_name().as_ref() == local_key
            && let Ok(val) = attr.normalized_value(quick_xml::XmlVersion::Implicit1_0)
        {
            return Some(val.into_owned());
        }
    }
    None
}

fn decode_xml_text(raw: &str) -> String {
    unescape(raw)
        .map(|cow| cow.into_owned())
        .unwrap_or_else(|_| raw.to_string())
}

fn find_opf_path(container_xml: &[u8]) -> Option<String> {
    let xml_str = String::from_utf8_lossy(container_xml);
    let mut reader = Reader::from_str(&xml_str);
    reader.config_mut().trim_text(true);
    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e) | Event::Empty(ref e))
                if e.local_name().as_ref() == "rootfile" =>
            {
                if let Some(full_path) = attr_value(e, "full-path") {
                    return Some(full_path);
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    None
}

struct OpfData {
    title: Option<String>,
    authors: Vec<String>,
    description: Option<String>,
    isbn: Option<String>,
    language: Option<String>,
    publisher: Option<String>,
    published_at: Option<String>,
    subjects: Vec<String>,
    cover_meta_id: Option<String>,
    manifest: HashMap<String, ManifestItem>,
    spine_ids: Vec<String>,
    spine_toc_id: Option<String>,
}

fn parse_opf(opf_xml: &[u8]) -> OpfData {
    let xml_str = String::from_utf8_lossy(opf_xml);
    let mut reader = Reader::from_str(&xml_str);
    reader.config_mut().trim_text(true);

    let mut data = OpfData {
        title: None,
        authors: Vec::new(),
        description: None,
        isbn: None,
        language: None,
        publisher: None,
        published_at: None,
        subjects: Vec::new(),
        cover_meta_id: None,
        manifest: HashMap::new(),
        spine_ids: Vec::new(),
        spine_toc_id: None,
    };

    let mut active_tag: Option<String> = None;

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let local = e.local_name().as_ref().to_string();
                match local.as_str() {
                    "meta" => {
                        if attr_value(e, "name").as_deref() == Some("cover") {
                            data.cover_meta_id = attr_value(e, "content");
                        }
                    }
                    "item" => {
                        if let (Some(id), Some(href)) = (attr_value(e, "id"), attr_value(e, "href"))
                        {
                            data.manifest.insert(
                                id,
                                ManifestItem {
                                    href,
                                    media_type: attr_value(e, "media-type").unwrap_or_default(),
                                    properties: attr_value(e, "properties").unwrap_or_default(),
                                },
                            );
                        }
                    }
                    "spine" => {
                        data.spine_toc_id = attr_value(e, "toc");
                    }
                    "itemref" => {
                        if let Some(idref) = attr_value(e, "idref") {
                            data.spine_ids.push(idref);
                        }
                    }
                    _ => active_tag = Some(local),
                }
            }
            Ok(Event::Empty(ref e)) => match e.local_name().as_ref() {
                "meta" => {
                    if attr_value(e, "name").as_deref() == Some("cover") {
                        data.cover_meta_id = attr_value(e, "content");
                    }
                }
                "item" => {
                    if let (Some(id), Some(href)) = (attr_value(e, "id"), attr_value(e, "href")) {
                        data.manifest.insert(
                            id,
                            ManifestItem {
                                href,
                                media_type: attr_value(e, "media-type").unwrap_or_default(),
                                properties: attr_value(e, "properties").unwrap_or_default(),
                            },
                        );
                    }
                }
                "itemref" => {
                    if let Some(idref) = attr_value(e, "idref") {
                        data.spine_ids.push(idref);
                    }
                }
                _ => {}
            },
            Ok(Event::Text(ref t)) => {
                if let Some(ref tag) = active_tag {
                    let val = decode_xml_text(t).trim().to_string();
                    if !val.is_empty() {
                        match tag.as_str() {
                            "title" if data.title.is_none() => data.title = Some(val),
                            "creator" => data.authors.push(val),
                            "description" if data.description.is_none() => {
                                data.description = Some(strip_html_tags(&val))
                            }
                            "language" if data.language.is_none() => data.language = Some(val),
                            "publisher" if data.publisher.is_none() => data.publisher = Some(val),
                            "date" if data.published_at.is_none() => data.published_at = Some(val),
                            "subject" => data.subjects.push(val),
                            "identifier" if data.isbn.is_none() => {
                                let clean = val.trim_start_matches("urn:isbn:").trim().to_string();
                                if clean
                                    .chars()
                                    .filter(|c| c.is_ascii_digit() || *c == 'X')
                                    .count()
                                    >= 10
                                {
                                    data.isbn = Some(clean);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            Ok(Event::End(_)) => {
                active_tag = None;
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }

    data
}

pub(crate) fn overlay_opf_sidecar(opf_xml: &[u8], parsed: &mut ParsedBook) {
    let opf = parse_opf(opf_xml);
    if let Some(t) = opf.title
        && !t.trim().is_empty()
    {
        parsed.metadata.title = t;
    }
    if !opf.authors.is_empty() {
        parsed.metadata.authors = opf.authors;
    }
    if opf.description.is_some() {
        parsed.metadata.description = opf.description;
    }
    if opf.isbn.is_some() {
        parsed.metadata.isbn = opf.isbn;
    }
    if opf.language.is_some() {
        parsed.metadata.language = opf.language;
    }
    if opf.publisher.is_some() {
        parsed.metadata.publisher = opf.publisher;
    }
    if opf.published_at.is_some() {
        parsed.metadata.published_at = opf.published_at;
    }
    if !opf.subjects.is_empty() {
        parsed.metadata.tags = opf.subjects;
    }
    if let Ok(normalized) = parsed.metadata.clone().normalized() {
        parsed.metadata = normalized;
    }
}

fn strip_html_tags(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_tag = false;
    for ch in input.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.trim().to_string()
}

fn parse_ncx_toc(ncx_xml: &[u8], ncx_dir: &Path) -> Vec<TocItem> {
    let xml_str = String::from_utf8_lossy(ncx_xml);
    let mut reader = Reader::from_str(&xml_str);
    reader.config_mut().trim_text(true);
    let mut items = Vec::new();
    let mut depth: u8 = 0;
    let mut in_text = false;
    let mut current_title = String::new();
    let mut current_src: Option<String> = None;

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => match e.local_name().as_ref() {
                "navPoint" => {
                    depth = depth.saturating_add(1);
                    current_title.clear();
                    current_src = None;
                }
                "text" if depth > 0 => {
                    in_text = true;
                }
                "content" if depth > 0 => {
                    if let Some(src) = attr_value(e, "src") {
                        current_src = Some(normalize_zip_path(ncx_dir, &src));
                    }
                }
                _ => {}
            },
            Ok(Event::Empty(ref e)) if e.local_name().as_ref() == "content" && depth > 0 => {
                if let Some(src) = attr_value(e, "src") {
                    let href = normalize_zip_path(ncx_dir, &src);
                    if !current_title.trim().is_empty() {
                        let idx = u32::try_from(items.len()).unwrap_or(0);
                        items.push(TocItem {
                            index: idx,
                            title: current_title.trim().to_string(),
                            href,
                            level: depth.saturating_sub(1),
                        });
                        current_title.clear();
                    } else {
                        current_src = Some(href);
                    }
                }
            }
            Ok(Event::Text(ref t)) if in_text => {
                current_title.push_str(&decode_xml_text(t));
            }
            Ok(Event::End(ref e)) => match e.local_name().as_ref() {
                "text" => {
                    in_text = false;
                    if let Some(href) = current_src.take()
                        && !current_title.trim().is_empty()
                    {
                        let idx = u32::try_from(items.len()).unwrap_or(0);
                        items.push(TocItem {
                            index: idx,
                            title: current_title.trim().to_string(),
                            href,
                            level: depth.saturating_sub(1),
                        });
                        current_title.clear();
                    }
                }
                "content" => {
                    if let Some(href) = current_src.take()
                        && !current_title.trim().is_empty()
                    {
                        let idx = u32::try_from(items.len()).unwrap_or(0);
                        items.push(TocItem {
                            index: idx,
                            title: current_title.trim().to_string(),
                            href,
                            level: depth.saturating_sub(1),
                        });
                        current_title.clear();
                    }
                }
                "navPoint" => {
                    depth = depth.saturating_sub(1);
                }
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }

    items
}

fn is_toc_nav(e: &BytesStart<'_>) -> bool {
    if let Some(t) = attr_value(e, "type")
        && t.split_whitespace()
            .any(|tok| tok.eq_ignore_ascii_case("toc"))
    {
        return true;
    }
    if let Some(r) = attr_value(e, "role")
        && r.split_whitespace()
            .any(|tok| tok.eq_ignore_ascii_case("doc-toc"))
    {
        return true;
    }
    false
}

fn parse_nav_xhtml_toc(nav_xml: &[u8], nav_dir: &Path) -> Vec<TocItem> {
    let xml_str = String::from_utf8_lossy(nav_xml);
    let mut reader = Reader::from_str(&xml_str);
    reader.config_mut().trim_text(true);

    let mut preferred_toc: Vec<TocItem> = Vec::new();
    let mut fallback_toc: Vec<TocItem> = Vec::new();

    let mut current_items: Vec<TocItem> = Vec::new();
    let mut pending_group_indices: Vec<usize> = Vec::new();
    let mut nav_depth: usize = 0;
    let mut current_nav_is_toc = false;
    let mut list_depth: u8 = 0;
    let mut active_href: Option<String> = None;
    let mut active_span_heading = false;
    let mut active_text = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => match e.local_name().as_ref() {
                "nav" => {
                    if nav_depth == 0 {
                        current_nav_is_toc = is_toc_nav(e);
                        current_items.clear();
                        pending_group_indices.clear();
                        list_depth = 0;
                    }
                    nav_depth += 1;
                }
                "ol" | "ul" if nav_depth > 0 => {
                    list_depth = list_depth.saturating_add(1);
                }
                "a" if nav_depth > 0 && list_depth > 0 => {
                    active_href = attr_value(e, "href").map(|h| normalize_zip_path(nav_dir, &h));
                    active_span_heading = false;
                    active_text.clear();
                }
                "span" if nav_depth > 0 && list_depth > 0 && active_href.is_none() => {
                    active_span_heading = true;
                    active_text.clear();
                }
                _ => {}
            },
            Ok(Event::Text(ref t)) if active_href.is_some() || active_span_heading => {
                active_text.push_str(&decode_xml_text(t));
            }
            Ok(Event::End(ref e)) => match e.local_name().as_ref() {
                "a" if nav_depth > 0 => {
                    if let Some(href) = active_href.take() {
                        let title = active_text.trim().to_string();
                        if !title.is_empty() {
                            for pending_idx in pending_group_indices.drain(..) {
                                current_items[pending_idx].href = href.clone();
                            }
                            let idx = u32::try_from(current_items.len()).unwrap_or(0);
                            current_items.push(TocItem {
                                index: idx,
                                title,
                                href,
                                level: list_depth.saturating_sub(1),
                            });
                        }
                    }
                    active_text.clear();
                }
                "span" if nav_depth > 0 && active_span_heading => {
                    active_span_heading = false;
                    let title = active_text.trim().to_string();
                    if !title.is_empty() {
                        let pos = current_items.len();
                        current_items.push(TocItem {
                            index: u32::try_from(pos).unwrap_or(0),
                            title,
                            href: String::new(),
                            level: list_depth.saturating_sub(1),
                        });
                        pending_group_indices.push(pos);
                    }
                    active_text.clear();
                }
                "ol" | "ul" if nav_depth > 0 => {
                    list_depth = list_depth.saturating_sub(1);
                }
                "nav" if nav_depth > 0 => {
                    nav_depth -= 1;
                    if nav_depth == 0 {
                        current_items.retain(|i| !i.href.is_empty());
                        for (idx, item) in current_items.iter_mut().enumerate() {
                            item.index = u32::try_from(idx).unwrap_or(0);
                        }
                        if !current_items.is_empty() {
                            if current_nav_is_toc {
                                preferred_toc = std::mem::take(&mut current_items);
                                break;
                            } else if fallback_toc.is_empty() {
                                fallback_toc = std::mem::take(&mut current_items);
                            }
                        }
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }

    if !preferred_toc.is_empty() {
        preferred_toc
    } else {
        fallback_toc
    }
}

fn extract_cover(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    opf_dir: &Path,
    opf: &OpfData,
) -> Option<CoverPayload> {
    let candidate = opf
        .manifest
        .values()
        .find(|item| {
            item.properties
                .split_whitespace()
                .any(|p| p == "cover-image")
        })
        .or_else(|| {
            opf.cover_meta_id
                .as_ref()
                .and_then(|id| opf.manifest.get(id))
                .filter(|item| item.media_type.starts_with("image/"))
        })
        .or_else(|| {
            opf.manifest.values().find(|item| {
                item.media_type.starts_with("image/") && item.href.to_lowercase().contains("cover")
            })
        })?;

    let zip_path = normalize_zip_path(opf_dir, &candidate.href);
    let bytes = read_zip_entry(archive, &zip_path)?;
    if bytes.is_empty() {
        return None;
    }
    let mime_type = if candidate.media_type.starts_with("image/") {
        candidate.media_type.clone()
    } else {
        guess_epub_mime(&zip_path).to_string()
    };
    Some(CoverPayload {
        bytes: Bytes::from(bytes),
        mime_type,
    })
}

fn extract_toc(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    opf_dir: &Path,
    opf: &OpfData,
) -> Vec<TocItem> {
    let mut nav_toc = Vec::new();
    if let Some(nav_item) = opf
        .manifest
        .values()
        .find(|item| item.properties.split_whitespace().any(|p| p == "nav"))
    {
        let nav_path = normalize_zip_path(opf_dir, &nav_item.href);
        let nav_dir = Path::new(&nav_path)
            .parent()
            .unwrap_or_else(|| Path::new(""));
        if let Some(bytes) = read_zip_entry(archive, &nav_path) {
            nav_toc = parse_nav_xhtml_toc(&bytes, nav_dir);
        }
    }

    let mut ncx_toc = Vec::new();
    let ncx_item = opf
        .spine_toc_id
        .as_ref()
        .and_then(|id| opf.manifest.get(id))
        .or_else(|| {
            opf.manifest
                .values()
                .find(|i| i.media_type.contains("dtbncx"))
        });
    if let Some(ncx) = ncx_item {
        let ncx_path = normalize_zip_path(opf_dir, &ncx.href);
        let ncx_dir = Path::new(&ncx_path)
            .parent()
            .unwrap_or_else(|| Path::new(""));
        if let Some(bytes) = read_zip_entry(archive, &ncx_path) {
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
        .filter_map(|id| opf.manifest.get(id))
        .enumerate()
        .map(|(i, item)| TocItem {
            index: u32::try_from(i).unwrap_or(0),
            title: format!("Section {}", i + 1),
            href: normalize_zip_path(opf_dir, &item.href),
            level: 0,
        })
        .collect()
}

pub fn parse_epub(file_bytes: &[u8], fallback_title: &str) -> DomainResult<ParsedBook> {
    let cursor = Cursor::new(file_bytes);
    let mut archive = ZipArchive::new(cursor)
        .map_err(|e| DomainError::Validation(format!("invalid EPUB archive: {e}")))?;

    let container_bytes = read_zip_entry(&mut archive, "META-INF/container.xml")
        .ok_or_else(|| DomainError::Validation("EPUB missing META-INF/container.xml".into()))?;
    let opf_path = find_opf_path(&container_bytes)
        .ok_or_else(|| DomainError::Validation("EPUB container.xml missing rootfile".into()))?;
    let opf_bytes = read_zip_entry(&mut archive, &opf_path)
        .ok_or_else(|| DomainError::Validation(format!("EPUB missing OPF package: {opf_path}")))?;

    let opf_dir = Path::new(&opf_path)
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let opf = parse_opf(&opf_bytes);

    let title = opf
        .title
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| fallback_title.to_string());

    let mut metadata = BookMetadata::new(&title)?;
    metadata.authors = opf.authors.clone();
    metadata.description = opf.description.clone();
    metadata.isbn = opf.isbn.clone();
    metadata.language = opf.language.clone();
    metadata.publisher = opf.publisher.clone();
    metadata.published_at = opf.published_at.clone();
    metadata.tags = opf.subjects.clone();
    metadata = metadata.normalized()?;

    let toc = extract_toc(&mut archive, opf_dir, &opf);
    metadata.chapter_count = u32::try_from(toc.len().max(opf.spine_ids.len())).ok();

    let cover = extract_cover(&mut archive, opf_dir, &opf).or_else(|| {
        Some(generate_placeholder_svg_cover(
            &metadata.title,
            metadata.authors.first().map(String::as_str),
            "EPUB",
        ))
    });

    Ok(ParsedBook {
        metadata,
        cover,
        toc,
    })
}

pub fn list_epub_toc(file_bytes: &[u8]) -> DomainResult<Vec<TocItem>> {
    let cursor = Cursor::new(file_bytes);
    let mut archive = ZipArchive::new(cursor)
        .map_err(|e| DomainError::Validation(format!("invalid EPUB archive: {e}")))?;
    let container_bytes = read_zip_entry(&mut archive, "META-INF/container.xml")
        .ok_or_else(|| DomainError::Validation("EPUB missing META-INF/container.xml".into()))?;
    let opf_path = find_opf_path(&container_bytes)
        .ok_or_else(|| DomainError::Validation("EPUB container.xml missing rootfile".into()))?;
    let opf_bytes = read_zip_entry(&mut archive, &opf_path)
        .ok_or_else(|| DomainError::Validation(format!("EPUB missing OPF package: {opf_path}")))?;
    let opf_dir = Path::new(&opf_path)
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let opf = parse_opf(&opf_bytes);
    Ok(extract_toc(&mut archive, opf_dir, &opf))
}

pub fn read_epub_resource(file_bytes: &[u8], resource_path: &str) -> DomainResult<BookAsset> {
    let normalized = normalize_zip_path(Path::new(""), resource_path.trim_start_matches('/'));
    if normalized.is_empty() {
        return Err(DomainError::Validation("empty EPUB resource path".into()));
    }
    let cursor = Cursor::new(file_bytes);
    let mut archive = ZipArchive::new(cursor)
        .map_err(|e| DomainError::Validation(format!("invalid EPUB archive: {e}")))?;
    let bytes = read_zip_entry(&mut archive, &normalized)
        .ok_or_else(|| DomainError::NotFound(format!("epub resource {normalized} not found")))?;
    let mime_type = guess_epub_mime(&normalized).to_string();
    Ok(BookAsset {
        bytes: Bytes::from(bytes),
        mime_type,
    })
}

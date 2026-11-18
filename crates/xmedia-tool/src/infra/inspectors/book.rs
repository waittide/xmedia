use std::collections::HashSet;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Component, Path, PathBuf};

use async_trait::async_trait;
use jiff::Timestamp;
use lopdf::{Document, Object, decode_text_string};
use quick_xml::Reader;
use quick_xml::escape::escape;
use quick_xml::events::{BytesStart, Event};
use regex::Regex;
use xmedia_core::application::ports::BinaryPayload;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;
use xmedia_core::infra::thumbnail::sniff_image_mime;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::domain::media::{
    BookFileMeta, BookMetaPatch, CompletenessFlags, MediaMetadataPatch, MediaMetadataPayload,
};
use crate::domain::ports::{MediaInspectorPort, QuickFileSummary};
use crate::infra::atomic_fs::{atomic_write_bytes, atomic_write_via_temp};
use crate::infra::inspectors::video::write_image_replacing_opposite_ext;

pub struct EpubBookInspector;

const BOOK_EXTENSIONS: &[&str] = &["epub", "pdf", "txt"];

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

fn normalize_zip_rel(base_dir: &Path, href: &str) -> String {
    let clean = href.split('#').next().unwrap_or(href);
    let joined = base_dir.join(clean);
    let mut out = PathBuf::new();
    for comp in joined.components() {
        match comp {
            Component::Normal(p) => out.push(p),
            Component::ParentDir => {
                out.pop();
            }
            _ => {}
        }
    }
    out.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

fn find_opf_path_in_container(container_xml: &[u8]) -> Option<String> {
    let s = String::from_utf8_lossy(container_xml);
    let mut reader = Reader::from_str(&s);
    reader.config_mut().trim_text(true);
    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e) | Event::Empty(ref e))
                if e.local_name().as_ref() == "rootfile" =>
            {
                if let Some(fp) = attr_value(e, "full-path") {
                    return Some(fp);
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    None
}

#[derive(Default)]
struct ParsedEpubPackage {
    opf_path: String,
    package_version: String,
    unique_identifier_id: String,
    cover_entry_path: Option<String>,
    cover_item_id: Option<String>,
    cover_media_type: Option<String>,
    cover_raw_href: Option<String>,
    meta: BookFileMeta,
}

type OpfManifestEntry = (String, String, String, String);

struct ParsedOpfData {
    meta: BookFileMeta,
    package_version: String,
    unique_identifier_id: String,
    cover_meta_id: Option<String>,
    manifest_items: Vec<OpfManifestEntry>,
}

fn parse_opf_bytes(opf_bytes: &[u8], format: &str, fallback_title: &str) -> ParsedOpfData {
    let opf_str = String::from_utf8_lossy(opf_bytes);
    let mut reader = Reader::from_str(&opf_str);
    reader.config_mut().trim_text(false);

    let mut meta = BookFileMeta {
        format: format.to_string(),
        title: fallback_title.to_string(),
        writable: true,
        ..Default::default()
    };

    let mut package_version = "2.0".to_string();
    let mut unique_identifier_id = "bookid".to_string();
    let mut in_metadata = false;
    let mut cur_tag: Option<String> = None;
    let mut cur_meta_id: Option<String> = None;
    let mut cur_meta_prop: Option<String> = None;
    let mut cur_meta_refines: Option<String> = None;
    let mut text_acc = String::new();
    let mut cover_meta_id: Option<String> = None;
    let mut calibre_series: Option<String> = None;
    let mut calibre_subseries: Option<String> = None;
    let mut calibre_series_index: Option<String> = None;
    let mut collections: Vec<(Option<String>, String)> = Vec::new();
    let mut collection_types: Vec<(String, String)> = Vec::new();
    let mut group_positions: Vec<(String, String)> = Vec::new();
    let mut manifest_items: Vec<(String, String, String, String)> = Vec::new();

    let mut apply_meta_name_attr = |e: &BytesStart<'_>| {
        if let Some(name_val) = attr_value(e, "name") {
            let content = attr_value(e, "content")
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            match name_val.trim().to_ascii_lowercase().as_str() {
                "cover" => cover_meta_id = content,
                "calibre:series" => calibre_series = content,
                "calibre:subseries" => calibre_subseries = content,
                "calibre:series_index" => calibre_series_index = content,
                _ => {}
            }
        }
    };

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let local = e.local_name().as_ref().to_ascii_lowercase();
                if local == "package" {
                    if let Some(v) = attr_value(e, "version") {
                        package_version = v;
                    }
                    if let Some(uid) = attr_value(e, "unique-identifier")
                        && !uid.trim().is_empty()
                    {
                        unique_identifier_id = uid.trim().to_string();
                    }
                } else if local == "metadata" {
                    in_metadata = true;
                } else if in_metadata {
                    if local == "meta" {
                        apply_meta_name_attr(e);
                        cur_meta_id = attr_value(e, "id");
                        cur_meta_prop = attr_value(e, "property");
                        cur_meta_refines = attr_value(e, "refines");
                    }
                    cur_tag = Some(local);
                    text_acc.clear();
                } else if local == "item" {
                    manifest_items.push((
                        attr_value(e, "id").unwrap_or_default(),
                        attr_value(e, "href").unwrap_or_default(),
                        attr_value(e, "media-type").unwrap_or_default(),
                        attr_value(e, "properties").unwrap_or_default(),
                    ));
                }
            }
            Ok(Event::Empty(ref e)) => {
                let local = e.local_name().as_ref().to_ascii_lowercase();
                if in_metadata && local == "meta" {
                    apply_meta_name_attr(e);
                } else if local == "item" {
                    manifest_items.push((
                        attr_value(e, "id").unwrap_or_default(),
                        attr_value(e, "href").unwrap_or_default(),
                        attr_value(e, "media-type").unwrap_or_default(),
                        attr_value(e, "properties").unwrap_or_default(),
                    ));
                }
            }
            Ok(Event::Text(t)) if in_metadata => {
                if cur_tag.is_some() {
                    text_acc.push_str(&t.xml10_content());
                }
            }
            Ok(Event::GeneralRef(r)) if in_metadata => {
                if cur_tag.is_some() {
                    super::append_xml_ref(&mut text_acc, &r);
                }
            }
            Ok(Event::CData(c)) if in_metadata => {
                if cur_tag.is_some() {
                    text_acc.push_str(c.as_ref());
                }
            }
            Ok(Event::End(ref e)) => {
                let local = e.local_name().as_ref().to_ascii_lowercase();
                if local == "metadata" {
                    in_metadata = false;
                } else if in_metadata && let Some(tag) = cur_tag.take() {
                    let trimmed = text_acc.trim();
                    if !trimmed.is_empty() {
                        match tag.as_str() {
                            "title" => meta.title = trimmed.to_string(),
                            "creator" if !meta.authors.iter().any(|a| a == trimmed) => {
                                meta.authors.push(trimmed.to_string());
                            }
                            "description" => meta.description = Some(trimmed.to_string()),
                            "publisher" => meta.publisher = Some(trimmed.to_string()),
                            "date" => meta.published_at = Some(trimmed.to_string()),
                            "language" => meta.language = Some(trimmed.to_string()),
                            "identifier" => {
                                let lower = trimmed.to_ascii_lowercase();
                                let stripped = if lower.starts_with("urn:isbn:") {
                                    &trimmed[9..]
                                } else if lower.starts_with("isbn:") {
                                    &trimmed[5..]
                                } else {
                                    trimmed
                                }
                                .trim();
                                let digits: String = stripped
                                    .chars()
                                    .filter(|c| c.is_ascii_digit() || *c == 'X' || *c == 'x')
                                    .collect();
                                if meta.isbn.is_none() && (digits.len() == 10 || digits.len() == 13)
                                {
                                    meta.isbn = Some(stripped.to_string());
                                }
                            }
                            "subject"
                                if !meta.tags.iter().any(|s| s.eq_ignore_ascii_case(trimmed)) =>
                            {
                                meta.tags.push(trimmed.to_string());
                            }
                            "meta" => {
                                if let Some(prop) = cur_meta_prop.as_deref() {
                                    let prop_lower = prop.trim().to_ascii_lowercase();
                                    if prop_lower == "belongs-to-collection" {
                                        collections.push((
                                            cur_meta_id.as_ref().map(|s| s.trim().to_string()),
                                            trimmed.to_string(),
                                        ));
                                    } else if prop_lower == "collection-type" {
                                        if let Some(ref_id) = cur_meta_refines
                                            .as_deref()
                                            .and_then(|r| r.trim().strip_prefix('#'))
                                        {
                                            collection_types.push((
                                                ref_id.to_string(),
                                                trimmed.to_ascii_lowercase(),
                                            ));
                                        }
                                    } else if prop_lower == "group-position"
                                        && let Some(ref_id) = cur_meta_refines
                                            .as_deref()
                                            .and_then(|r| r.trim().strip_prefix('#'))
                                    {
                                        group_positions
                                            .push((ref_id.to_string(), trimmed.to_string()));
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                cur_tag = None;
                cur_meta_id = None;
                cur_meta_prop = None;
                cur_meta_refines = None;
                text_acc.clear();
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }

    // Resolve EPUB 3 belongs-to-collection (set + series + group-position)
    if !collections.is_empty() {
        let type_for = |cid: Option<&str>| -> Option<&str> {
            let id = cid?;
            collection_types
                .iter()
                .find(|(k, _)| k == id)
                .map(|(_, v)| v.as_str())
        };
        let pos_for = |cid: Option<&str>| -> Option<String> {
            let id = cid?;
            group_positions
                .iter()
                .find(|(k, _)| k == id)
                .map(|(_, v)| v.clone())
        };

        if collections.len() >= 2 {
            let set_entry = collections
                .iter()
                .find(|(id, _)| type_for(id.as_deref()) == Some("set"));
            let ser_entry = collections
                .iter()
                .find(|(id, _)| type_for(id.as_deref()) == Some("series"));
            if let (Some((_, s_name)), Some((sub_id, sub_name))) = (set_entry, ser_entry) {
                meta.series = Some(s_name.clone());
                meta.subseries = Some(sub_name.clone());
                meta.series_index = pos_for(sub_id.as_deref());
            } else {
                meta.series = Some(collections[0].1.clone());
                meta.subseries = Some(collections[1].1.clone());
                meta.series_index = pos_for(collections[1].0.as_deref())
                    .or_else(|| pos_for(collections[0].0.as_deref()));
            }
        } else {
            let (cid, cname) = &collections[0];
            meta.series = Some(cname.clone());
            meta.series_index =
                pos_for(cid.as_deref()).or_else(|| group_positions.first().map(|(_, v)| v.clone()));
        }
    }

    if meta.series.is_none() {
        meta.series = calibre_series;
    }
    if meta.subseries.is_none() {
        meta.subseries = calibre_subseries;
    }
    if meta.series_index.is_none() {
        meta.series_index = calibre_series_index;
    }

    ParsedOpfData {
        meta,
        package_version,
        unique_identifier_id,
        cover_meta_id,
        manifest_items,
    }
}

fn parse_epub_archive(path: &Path) -> DomainResult<ParsedEpubPackage> {
    let file = File::open(path)
        .map_err(|e| DomainError::Storage(format!("failed opening '{}': {e}", path.display())))?;
    let mut archive = ZipArchive::new(BufReader::new(file))
        .map_err(|e| DomainError::Validation(format!("invalid epub archive: {e}")))?;

    let mut container_xml = Vec::new();
    archive
        .by_name("META-INF/container.xml")
        .map_err(|_| DomainError::Validation("missing META-INF/container.xml in epub".into()))?
        .read_to_end(&mut container_xml)
        .map_err(|e| DomainError::Validation(format!("failed reading container.xml: {e}")))?;

    let opf_path = find_opf_path_in_container(&container_xml)
        .ok_or_else(|| DomainError::Validation("rootfile missing in container.xml".into()))?;
    let mut opf_bytes = Vec::new();
    archive
        .by_name(&opf_path)
        .map_err(|_| DomainError::Validation(format!("OPF '{opf_path}' missing in epub")))?
        .read_to_end(&mut opf_bytes)
        .map_err(|e| DomainError::Validation(format!("failed reading OPF: {e}")))?;

    let opf_dir = Path::new(&opf_path).parent().unwrap_or(Path::new(""));
    let fallback_title = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Untitled");

    let ParsedOpfData {
        mut meta,
        package_version,
        unique_identifier_id,
        cover_meta_id,
        manifest_items,
    } = parse_opf_bytes(&opf_bytes, "epub", fallback_title);

    let cover_item = manifest_items
        .iter()
        .find(|(_, _, mt, props)| {
            mt.starts_with("image/") && props.split_whitespace().any(|p| p == "cover-image")
        })
        .or_else(|| {
            cover_meta_id.as_ref().and_then(|cid| {
                manifest_items
                    .iter()
                    .find(|(id, _, mt, _)| id == cid && mt.starts_with("image/"))
            })
        })
        .or_else(|| {
            manifest_items.iter().find(|(id, href, mt, _)| {
                mt.starts_with("image/")
                    && (id.to_ascii_lowercase().contains("cover")
                        || href.to_ascii_lowercase().contains("cover"))
            })
        });

    let cover_item_id = cover_item.map(|(id, _, _, _)| id.clone());
    let cover_raw_href = cover_item.map(|(_, href, _, _)| href.clone());
    let cover_media_type = cover_item.map(|(_, _, mt, _)| mt.clone());
    let cover_href = cover_raw_href
        .as_deref()
        .map(|href| normalize_zip_rel(opf_dir, href));

    meta.has_cover = cover_href
        .as_ref()
        .is_some_and(|cp| archive.by_name(cp).is_ok());

    Ok(ParsedEpubPackage {
        opf_path,
        package_version,
        unique_identifier_id,
        cover_entry_path: cover_href,
        cover_item_id,
        cover_media_type,
        cover_raw_href,
        meta,
    })
}

fn looks_like_isbn(text: &str, scheme_attr: Option<&str>) -> bool {
    if scheme_attr.is_some_and(|s| s.eq_ignore_ascii_case("isbn")) {
        return true;
    }
    let lower = text.trim().to_ascii_lowercase();
    if lower.starts_with("urn:isbn:") || lower.starts_with("isbn:") {
        return true;
    }
    let digits: String = text
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == 'X' || *c == 'x')
        .collect();
    !lower.starts_with("urn:uuid:") && (digits.len() == 10 || digits.len() == 13)
}

fn replace_opf_metadata_block(
    original_opf: &str,
    meta: &BookFileMeta,
    cover_id_hint: Option<&str>,
    unique_id_hint: &str,
    package_version_hint: &str,
) -> String {
    let re = Regex::new(
        r"(?is)<(?P<prefix>\w+:)?metadata\b(?P<attrs>[^>]*)>(?P<inner>.*?)</(?:\w+:)?metadata>",
    )
    .expect("valid opf metadata regex");

    let Some(caps) = re.captures(original_opf) else {
        return original_opf.to_string();
    };
    let prefix = caps.name("prefix").map(|m| m.as_str()).unwrap_or("");
    let open_attrs = caps.name("attrs").map(|m| m.as_str()).unwrap_or("");
    let inner = caps.name("inner").map(|m| m.as_str()).unwrap_or("");

    let mut preserved: Vec<(Option<String>, String)> = Vec::new();
    let mut removed_ids: HashSet<String> = HashSet::new();
    let mut has_primary_unique_id = false;
    let mut emitted_isbn = false;
    let mut had_dcterms_modified = false;

    let mut reader = Reader::from_str(inner);
    reader.config_mut().trim_text(false);
    let mut prev_pos = 0usize;
    let mut depth = 0usize;
    let mut elem_start = 0usize;
    let mut elem_local = String::new();
    let mut elem_id: Option<String> = None;
    let mut elem_name: Option<String> = None;
    let mut elem_prop: Option<String> = None;
    let mut elem_refines: Option<String> = None;
    let mut elem_scheme: Option<String> = None;
    let mut text_acc = String::new();

    let mut handle_top_element = |local: &str,
                                  id: Option<String>,
                                  name: Option<String>,
                                  prop: Option<String>,
                                  refines: Option<String>,
                                  scheme: Option<String>,
                                  inner_text: &str,
                                  raw_slice: &str| {
        let trimmed_raw = raw_slice.trim();
        if trimmed_raw.is_empty() {
            return;
        }
        match local {
            "title" | "creator" | "description" | "publisher" | "date" | "language" | "subject" => {
                if let Some(elem_id_val) = id {
                    removed_ids.insert(elem_id_val);
                }
            }
            "meta" => {
                if prop
                    .as_deref()
                    .is_some_and(|p| p.eq_ignore_ascii_case("dcterms:modified"))
                {
                    had_dcterms_modified = true;
                    return;
                }
                if prop.as_deref().is_some_and(|p| {
                    matches!(
                        p.trim().to_ascii_lowercase().as_str(),
                        "belongs-to-collection" | "collection-type" | "group-position"
                    )
                }) {
                    if let Some(elem_id_val) = id {
                        removed_ids.insert(elem_id_val);
                    }
                    return;
                }
                if name.as_deref().is_some_and(|n| {
                    matches!(
                        n.trim().to_ascii_lowercase().as_str(),
                        "calibre:series"
                            | "calibre:subseries"
                            | "calibre:series_index"
                            | "calibre:subseries_index"
                    )
                }) {
                    if let Some(elem_id_val) = id {
                        removed_ids.insert(elem_id_val);
                    }
                    return;
                }
                if name
                    .as_deref()
                    .is_some_and(|n| n.eq_ignore_ascii_case("cover"))
                    && cover_id_hint.is_some()
                {
                    return;
                }
                let refines_id = refines
                    .as_deref()
                    .and_then(|r| r.trim().strip_prefix('#'))
                    .map(str::to_owned);
                preserved.push((refines_id, trimmed_raw.to_string()));
            }
            "identifier" => {
                let is_primary = id.as_deref() == Some(unique_id_hint);
                let is_isbn = looks_like_isbn(inner_text, scheme.as_deref());
                if is_primary {
                    has_primary_unique_id = true;
                    if !is_isbn {
                        preserved.push((None, trimmed_raw.to_string()));
                    } else if let Some(ref isbn_val) = meta.isbn
                        && !isbn_val.trim().is_empty()
                    {
                        emitted_isbn = true;
                        preserved.push((
                            None,
                            format!(
                                "<dc:identifier id=\"{}\">urn:isbn:{}</dc:identifier>",
                                escape(unique_id_hint),
                                escape(isbn_val.trim())
                            ),
                        ));
                    } else {
                        preserved.push((
                            None,
                            format!(
                                "<dc:identifier id=\"{}\">urn:uuid:{}</dc:identifier>",
                                escape(unique_id_hint),
                                uuid::Uuid::now_v7()
                            ),
                        ));
                    }
                } else if !is_isbn {
                    preserved.push((None, trimmed_raw.to_string()));
                }
            }
            _ => {
                preserved.push((None, trimmed_raw.to_string()));
            }
        }
    };

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                if depth == 0 {
                    let rel = inner[prev_pos..].find('<').unwrap_or(0);
                    elem_start = prev_pos + rel;
                    elem_local = e.local_name().as_ref().to_ascii_lowercase();
                    elem_id = attr_value(e, "id");
                    elem_name = attr_value(e, "name");
                    elem_prop = attr_value(e, "property");
                    elem_refines = attr_value(e, "refines");
                    elem_scheme = attr_value(e, "scheme");
                    text_acc.clear();
                }
                depth += 1;
            }
            Ok(Event::Empty(ref e)) => {
                if depth == 0 {
                    let rel = inner[prev_pos..].find('<').unwrap_or(0);
                    let start = prev_pos + rel;
                    let end = reader.buffer_position() as usize;
                    let local = e.local_name().as_ref().to_ascii_lowercase();
                    handle_top_element(
                        &local,
                        attr_value(e, "id"),
                        attr_value(e, "name"),
                        attr_value(e, "property"),
                        attr_value(e, "refines"),
                        attr_value(e, "scheme"),
                        "",
                        &inner[start..end],
                    );
                }
            }
            Ok(Event::Text(t)) if depth > 0 => {
                text_acc.push_str(&t.xml10_content());
            }
            Ok(Event::GeneralRef(r)) if depth > 0 => {
                super::append_xml_ref(&mut text_acc, &r);
            }
            Ok(Event::CData(c)) if depth > 0 => {
                text_acc.push_str(c.as_ref());
            }
            Ok(Event::End(_)) => {
                if depth > 0 {
                    depth -= 1;
                    if depth == 0 {
                        let end = reader.buffer_position() as usize;
                        handle_top_element(
                            &elem_local,
                            elem_id.take(),
                            elem_name.take(),
                            elem_prop.take(),
                            elem_refines.take(),
                            elem_scheme.take(),
                            &text_acc,
                            &inner[elem_start..end],
                        );
                        text_acc.clear();
                    }
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        prev_pos = reader.buffer_position() as usize;
    }

    let mut attrs_out = open_attrs.to_string();
    if !original_opf.contains("xmlns:dc=") && !attrs_out.contains("xmlns:dc=") {
        attrs_out.push_str(" xmlns:dc=\"http://purl.org/dc/elements/1.1/\"");
    }

    let mut block = String::with_capacity(1024);
    block.push_str(&format!("<{prefix}metadata{attrs_out}>\n"));

    if !has_primary_unique_id {
        let book_id = if let Some(ref isbn_val) = meta.isbn
            && !isbn_val.trim().is_empty()
        {
            emitted_isbn = true;
            format!("urn:isbn:{}", isbn_val.trim())
        } else {
            format!("urn:uuid:{}", uuid::Uuid::now_v7())
        };
        block.push_str(&format!(
            "    <dc:identifier id=\"{}\">{}</dc:identifier>\n",
            escape(unique_id_hint),
            escape(&book_id)
        ));
    }

    if !emitted_isbn
        && let Some(ref isbn_val) = meta.isbn
        && !isbn_val.trim().is_empty()
    {
        block.push_str(&format!(
            "    <dc:identifier>urn:isbn:{}</dc:identifier>\n",
            escape(isbn_val.trim())
        ));
    }

    block.push_str(&format!(
        "    <dc:title>{}</dc:title>\n",
        escape(meta.title.trim())
    ));
    for author in &meta.authors {
        let clean = author.trim();
        if !clean.is_empty() {
            block.push_str(&format!("    <dc:creator>{}</dc:creator>\n", escape(clean)));
        }
    }
    let lang = meta
        .language
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("zh");
    block.push_str(&format!(
        "    <dc:language>{}</dc:language>\n",
        escape(lang)
    ));
    if let Some(ref desc) = meta.description
        && !desc.trim().is_empty()
    {
        block.push_str(&format!(
            "    <dc:description>{}</dc:description>\n",
            escape(desc.trim())
        ));
    }
    if let Some(ref pub_name) = meta.publisher
        && !pub_name.trim().is_empty()
    {
        block.push_str(&format!(
            "    <dc:publisher>{}</dc:publisher>\n",
            escape(pub_name.trim())
        ));
    }
    if let Some(ref date) = meta.published_at
        && !date.trim().is_empty()
    {
        block.push_str(&format!("    <dc:date>{}</dc:date>\n", escape(date.trim())));
    }
    for subject in &meta.tags {
        let clean = subject.trim();
        if !clean.is_empty() {
            block.push_str(&format!("    <dc:subject>{}</dc:subject>\n", escape(clean)));
        }
    }
    let clean_series = meta
        .series
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let clean_subseries = meta
        .subseries
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let clean_index = meta
        .series_index
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    if package_version_hint.trim().starts_with('3') {
        match (clean_series, clean_subseries) {
            (Some(ser), Some(sub)) => {
                block.push_str(&format!(
                    "    <meta property=\"belongs-to-collection\" id=\"xmedia-col-set\">{}</meta>\n",
                    escape(ser)
                ));
                block.push_str(
                    "    <meta refines=\"#xmedia-col-set\" property=\"collection-type\">set</meta>\n",
                );
                block.push_str(&format!(
                    "    <meta property=\"belongs-to-collection\" id=\"xmedia-col-series\">{}</meta>\n",
                    escape(sub)
                ));
                block.push_str(
                    "    <meta refines=\"#xmedia-col-series\" property=\"collection-type\">series</meta>\n",
                );
                if let Some(idx) = clean_index {
                    block.push_str(&format!(
                        "    <meta refines=\"#xmedia-col-series\" property=\"group-position\">{}</meta>\n",
                        escape(idx)
                    ));
                }
            }
            (Some(ser), None) | (None, Some(ser)) => {
                block.push_str(&format!(
                    "    <meta property=\"belongs-to-collection\" id=\"xmedia-col-series\">{}</meta>\n",
                    escape(ser)
                ));
                block.push_str(
                    "    <meta refines=\"#xmedia-col-series\" property=\"collection-type\">series</meta>\n",
                );
                if let Some(idx) = clean_index {
                    block.push_str(&format!(
                        "    <meta refines=\"#xmedia-col-series\" property=\"group-position\">{}</meta>\n",
                        escape(idx)
                    ));
                }
            }
            (None, None) => {}
        }
    } else {
        if let Some(ser) = clean_series {
            block.push_str(&format!(
                "    <meta name=\"calibre:series\" content=\"{}\"/>\n",
                escape(ser)
            ));
        }
        if let Some(sub) = clean_subseries {
            block.push_str(&format!(
                "    <meta name=\"calibre:subseries\" content=\"{}\"/>\n",
                escape(sub)
            ));
        }
        if let Some(idx) = clean_index {
            block.push_str(&format!(
                "    <meta name=\"calibre:series_index\" content=\"{}\"/>\n",
                escape(idx)
            ));
        }
    }

    if let Some(cid) = cover_id_hint {
        block.push_str(&format!(
            "    <meta name=\"cover\" content=\"{}\"/>\n",
            escape(cid)
        ));
    }

    for (refines_target, raw_elem) in preserved {
        if let Some(target_id) = refines_target
            && removed_ids.contains(&target_id)
        {
            continue;
        }
        block.push_str("    ");
        block.push_str(&raw_elem);
        block.push('\n');
    }

    if package_version_hint.trim().starts_with('3') || had_dcterms_modified {
        let modified = Timestamp::now().strftime("%Y-%m-%dT%H:%M:%SZ").to_string();
        block.push_str(&format!(
            "    <meta property=\"dcterms:modified\">{modified}</meta>\n"
        ));
    }

    block.push_str(&format!("  </{prefix}metadata>"));
    re.replace(original_opf, block.as_str()).into_owned()
}

fn rewrite_epub_entries(
    epub_path: &Path,
    replacements: Vec<(String, Vec<u8>)>,
    skip_paths: &[String],
) -> DomainResult<()> {
    atomic_write_via_temp(epub_path, |staging| {
        let src_file = File::open(epub_path)
            .map_err(|e| DomainError::Storage(format!("failed opening epub: {e}")))?;
        let mut src_archive = ZipArchive::new(BufReader::new(src_file))
            .map_err(|e| DomainError::Validation(format!("invalid epub zip: {e}")))?;
        let out_file = File::create(staging)
            .map_err(|e| DomainError::Storage(format!("failed creating staging epub: {e}")))?;
        let mut writer = ZipWriter::new(BufWriter::new(out_file));

        // Per EPUB OCF specification, `mimetype` MUST be the first entry and stored uncompressed
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        writer
            .start_file("mimetype", stored)
            .map_err(|e| DomainError::Storage(format!("epub start mimetype failed: {e}")))?;
        writer
            .write_all(b"application/epub+zip")
            .map_err(|e| DomainError::Storage(format!("epub write mimetype failed: {e}")))?;

        let mut written: HashSet<String> = HashSet::from(["mimetype".to_string()]);

        for idx in 0..src_archive.len() {
            let entry = src_archive
                .by_index(idx)
                .map_err(|e| DomainError::Storage(format!("epub entry error: {e}")))?;
            let Ok(name_ref) = entry.name() else {
                continue;
            };
            let name = name_ref.to_string();
            if entry.is_dir() || name == "mimetype" || skip_paths.iter().any(|s| s == &name) {
                continue;
            }

            if let Some((_, rep_bytes)) = replacements.iter().find(|(rn, _)| rn == &name) {
                drop(entry);
                let opts =
                    SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
                writer
                    .start_file(&name, opts)
                    .map_err(|e| DomainError::Storage(format!("epub replace start error: {e}")))?;
                writer
                    .write_all(rep_bytes)
                    .map_err(|e| DomainError::Storage(format!("epub replace write error: {e}")))?;
                written.insert(name);
            } else {
                writer
                    .raw_copy_file(entry)
                    .map_err(|e| DomainError::Storage(format!("epub raw copy error: {e}")))?;
                written.insert(name);
            }
        }

        for (rep_name, rep_bytes) in replacements {
            if !written.contains(&rep_name) {
                let opts =
                    SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
                writer
                    .start_file(rep_name, opts)
                    .map_err(|e| DomainError::Storage(format!("epub add entry error: {e}")))?;
                writer.write_all(&rep_bytes).map_err(|e| {
                    DomainError::Storage(format!("epub write new entry error: {e}"))
                })?;
            }
        }

        let buf_writer = writer
            .finish()
            .map_err(|e| DomainError::Storage(format!("epub finish error: {e}")))?;
        buf_writer
            .into_inner()
            .map_err(|e| DomainError::Storage(format!("epub flush error: {e}")))?;
        Ok(())
    })
}

const GENERIC_BOOK_FOLDER_NAMES: &[&str] = &[
    "book",
    "books",
    "ebook",
    "ebooks",
    "doc",
    "docs",
    "novel",
    "novels",
    "sample",
    "samples",
    "download",
    "downloads",
    "inbox",
    "library",
    "media",
    "图书",
    "书籍",
    "电子书",
    "小说",
];

pub(crate) fn is_generic_book_folder(name: &str) -> bool {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return true;
    }
    let lower = trimmed.to_ascii_lowercase();
    GENERIC_BOOK_FOLDER_NAMES
        .iter()
        .any(|g| lower == *g || trimmed == *g)
}

pub(crate) fn dir_has_single_book(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    let mut count = 0usize;
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_file()
            && p.extension()
                .and_then(|s| s.to_str())
                .is_some_and(|e| BOOK_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
        {
            count += 1;
            if count > 1 {
                return false;
            }
        }
    }
    count == 1
}

fn is_dedicated_calibre_book_dir(dir: &Path) -> bool {
    let Some(dir_name) = dir.file_name().and_then(|s| s.to_str()) else {
        return false;
    };
    !is_generic_book_folder(dir_name) && dir_has_single_book(dir)
}

fn find_existing_sidecar_opf(book_path: &Path) -> Option<PathBuf> {
    let same_stem = book_path.with_extension("opf");
    if same_stem.is_file() {
        return Some(same_stem);
    }
    if let Some(parent) = book_path.parent()
        && dir_has_single_book(parent)
    {
        let calibre_opf = parent.join("metadata.opf");
        if calibre_opf.is_file() {
            return Some(calibre_opf);
        }
    }
    None
}

fn preferred_write_opf_path(book_path: &Path) -> PathBuf {
    let same_stem = book_path.with_extension("opf");
    if same_stem.is_file() {
        return same_stem;
    }
    if let Some(parent) = book_path.parent() {
        let calibre_opf = parent.join("metadata.opf");
        if calibre_opf.is_file() || is_dedicated_calibre_book_dir(parent) {
            return calibre_opf;
        }
    }
    same_stem
}

fn find_sidecar_cover(book_path: &Path) -> Option<PathBuf> {
    if let Some(same_stem) = ["jpg", "jpeg", "png", "webp"]
        .into_iter()
        .map(|ext| book_path.with_extension(ext))
        .find(|p| p.is_file())
    {
        return Some(same_stem);
    }
    if let Some(parent) = book_path.parent()
        && dir_has_single_book(parent)
    {
        return ["jpg", "jpeg", "png", "webp"]
            .into_iter()
            .map(|ext| parent.join(format!("cover.{ext}")))
            .find(|p| p.is_file());
    }
    None
}

fn pdf_info_string(doc: &Document, info_dict: &lopdf::Dictionary, key: &[u8]) -> Option<String> {
    let obj = info_dict.get(key).ok()?;
    let resolved = match obj {
        Object::Reference(id) => doc.get_object(*id).ok()?,
        other => other,
    };
    decode_text_string(resolved)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn apply_pdf_info_meta(path: &Path, meta: &mut BookFileMeta) {
    let Ok(doc) = Document::load(path) else {
        return;
    };
    let Ok(info_ref) = doc.trailer.get(b"Info") else {
        return;
    };
    let Ok(info_obj) = (match info_ref {
        Object::Reference(id) => doc.get_object(*id),
        other => Ok(other),
    }) else {
        return;
    };
    let Ok(info_dict) = info_obj.as_dict() else {
        return;
    };

    if let Some(t) = pdf_info_string(&doc, info_dict, b"Title") {
        meta.title = t;
    }
    if let Some(a) = pdf_info_string(&doc, info_dict, b"Author") {
        meta.authors = a
            .split([';', '、'])
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
    }
    if let Some(subj) = pdf_info_string(&doc, info_dict, b"Subject") {
        meta.description = Some(subj);
    }
    if let Some(kw) = pdf_info_string(&doc, info_dict, b"Keywords") {
        meta.tags = kw
            .split([',', ';', '、'])
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
    }
}

fn render_sidecar_opf(meta: &BookFileMeta) -> String {
    let base = r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0" unique-identifier="bookid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
  </metadata>
</package>
"#;
    replace_opf_metadata_block(base, meta, None, "bookid", "2.0")
}

fn read_book_meta_any(path: &Path) -> DomainResult<BookFileMeta> {
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
        }
        meta.has_cover = meta.has_cover || find_sidecar_cover(path).is_some();
        return Ok(meta);
    }

    let mut meta = BookFileMeta {
        format: ext.clone(),
        title: fallback_title.clone(),
        writable: true,
        ..Default::default()
    };

    if ext == "pdf" {
        apply_pdf_info_meta(path, &mut meta);
    }

    if let Some(opf_path) = find_existing_sidecar_opf(path)
        && let Ok(bytes) = std::fs::read(&opf_path)
    {
        meta = parse_opf_bytes(&bytes, &ext, &meta.title).meta;
    }

    meta.has_cover = find_sidecar_cover(path).is_some();
    Ok(meta)
}

#[async_trait]
impl MediaInspectorPort for EpubBookInspector {
    fn media_kind(&self) -> MediaKind {
        MediaKind::Book
    }

    fn supports_extension(&self, ext_lower: &str) -> bool {
        BOOK_EXTENSIONS.contains(&ext_lower)
    }

    async fn summarize_file(&self, path: &Path, fallback_stem: &str) -> QuickFileSummary {
        let p = path.to_path_buf();
        let stem = fallback_stem.to_string();
        tokio::task::spawn_blocking(move || match read_book_meta_any(&p) {
            Ok(m) => {
                let year = m
                    .published_at
                    .as_deref()
                    .and_then(|d| d.get(..4))
                    .and_then(|y| y.parse::<i32>().ok());
                QuickFileSummary {
                    display_title: m.title.clone(),
                    display_subtitle: m.authors.join(", "),
                    year,
                    completeness: CompletenessFlags {
                        has_title: !m.title.trim().is_empty(),
                        has_creator: !m.authors.is_empty(),
                        has_cover: m.has_cover,
                        has_summary_or_lyrics: m
                            .description
                            .as_ref()
                            .is_some_and(|d| !d.trim().is_empty()),
                        has_nfo_or_tag_block: !m.tags.is_empty()
                            || m.isbn.is_some()
                            || m.publisher
                                .as_ref()
                                .is_some_and(|pub_name| !pub_name.trim().is_empty()),
                    },
                }
            }
            Err(_) => QuickFileSummary {
                display_title: stem,
                display_subtitle: String::new(),
                year: None,
                completeness: CompletenessFlags::default(),
            },
        })
        .await
        .unwrap_or_else(|_| QuickFileSummary {
            display_title: fallback_stem.to_string(),
            display_subtitle: String::new(),
            year: None,
            completeness: CompletenessFlags::default(),
        })
    }

    async fn inspect_file(&self, path: &Path) -> DomainResult<MediaMetadataPayload> {
        let p = path.to_path_buf();
        let meta = tokio::task::spawn_blocking(move || read_book_meta_any(&p))
            .await
            .map_err(|e| DomainError::Storage(format!("book inspect task failed: {e}")))??;
        Ok(MediaMetadataPayload::Book(meta))
    }

    async fn write_metadata(
        &self,
        path: &Path,
        patch: &MediaMetadataPatch,
    ) -> DomainResult<MediaMetadataPayload> {
        let MediaMetadataPatch::Book(bp) = patch else {
            return Err(DomainError::Validation(
                "expected book metadata patch".into(),
            ));
        };
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if !BOOK_EXTENSIONS.contains(&ext.as_str()) {
            return Err(DomainError::UnsupportedFormat(format!(
                "unsupported book format '.{ext}'"
            )));
        }

        let p = path.to_path_buf();
        let bp_cloned: BookMetaPatch = bp.clone();

        let updated = tokio::task::spawn_blocking(move || -> DomainResult<BookFileMeta> {
            let apply_patch = |meta: &mut BookFileMeta, patch: BookMetaPatch| {
                if let Some(title) = patch.title
                    && !title.trim().is_empty()
                {
                    meta.title = title.trim().to_string();
                }
                if let Some(authors) = patch.authors {
                    meta.authors = authors
                        .into_iter()
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                }
                if let Some(series) = patch.series {
                    meta.series = (!series.trim().is_empty()).then(|| series.trim().to_string());
                }
                if let Some(subseries) = patch.subseries {
                    meta.subseries =
                        (!subseries.trim().is_empty()).then(|| subseries.trim().to_string());
                }
                if let Some(idx) = patch.series_index {
                    meta.series_index = (!idx.trim().is_empty()).then(|| idx.trim().to_string());
                }
                if let Some(desc) = patch.description {
                    meta.description = (!desc.trim().is_empty()).then(|| desc.trim().to_string());
                }
                if let Some(pub_name) = patch.publisher {
                    meta.publisher =
                        (!pub_name.trim().is_empty()).then(|| pub_name.trim().to_string());
                }
                if let Some(pub_at) = patch.published_at {
                    meta.published_at =
                        (!pub_at.trim().is_empty()).then(|| pub_at.trim().to_string());
                }
                if let Some(lang) = patch.language {
                    meta.language = (!lang.trim().is_empty()).then(|| lang.trim().to_string());
                }
                if let Some(isbn) = patch.isbn {
                    meta.isbn = (!isbn.trim().is_empty()).then(|| isbn.trim().to_string());
                }
                if let Some(tags) = patch.tags {
                    meta.tags = tags
                        .into_iter()
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                }
            };

            if ext != "epub" {
                let mut meta = read_book_meta_any(&p)?;
                apply_patch(&mut meta, bp_cloned);
                let opf_path = preferred_write_opf_path(&p);
                let xml = if opf_path.is_file()
                    && let Ok(existing) = std::fs::read_to_string(&opf_path)
                {
                    replace_opf_metadata_block(&existing, &meta, None, "bookid", "2.0")
                } else {
                    render_sidecar_opf(&meta)
                };
                atomic_write_bytes(&opf_path, xml.as_bytes())?;
                return read_book_meta_any(&p);
            }

            let parsed = parse_epub_archive(&p)?;
            let mut meta = read_book_meta_any(&p)?;
            apply_patch(&mut meta, bp_cloned);

            let src_file = File::open(&p)
                .map_err(|e| DomainError::Storage(format!("failed reading epub: {e}")))?;
            let mut archive = ZipArchive::new(BufReader::new(src_file))
                .map_err(|e| DomainError::Validation(format!("invalid epub: {e}")))?;
            let mut opf_bytes = Vec::new();
            archive
                .by_name(&parsed.opf_path)
                .map_err(|_| DomainError::Validation("OPF missing".into()))?
                .read_to_end(&mut opf_bytes)
                .map_err(|e| DomainError::Storage(format!("read OPF error: {e}")))?;
            drop(archive);

            let opf_str = String::from_utf8_lossy(&opf_bytes);
            let next_opf = replace_opf_metadata_block(
                &opf_str,
                &meta,
                None,
                &parsed.unique_identifier_id,
                &parsed.package_version,
            );
            rewrite_epub_entries(&p, vec![(parsed.opf_path, next_opf.into_bytes())], &[])?;

            // Keep any existing sidecar OPF (e.g. Calibre metadata.opf) synchronized so it never shadows updated EPUB tags
            if let Some(sidecar_opf) = find_existing_sidecar_opf(&p) {
                let side_xml = if let Ok(existing) = std::fs::read_to_string(&sidecar_opf) {
                    replace_opf_metadata_block(&existing, &meta, None, "bookid", "2.0")
                } else {
                    render_sidecar_opf(&meta)
                };
                let _ = atomic_write_bytes(&sidecar_opf, side_xml.as_bytes());
            }

            read_book_meta_any(&p)
        })
        .await
        .map_err(|e| DomainError::Storage(format!("book write task failed: {e}")))??;

        Ok(MediaMetadataPayload::Book(updated))
    }

    async fn read_artwork(
        &self,
        path: &Path,
        _slot: Option<&str>,
    ) -> DomainResult<Option<BinaryPayload>> {
        let p = path.to_path_buf();
        tokio::task::spawn_blocking(move || {
            let ext = p
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            if ext == "epub" {
                let parsed = parse_epub_archive(&p)?;
                if let Some(cover_entry) = parsed.cover_entry_path {
                    let src_file = File::open(&p)
                        .map_err(|e| DomainError::Storage(format!("failed reading epub: {e}")))?;
                    let mut archive = ZipArchive::new(BufReader::new(src_file))
                        .map_err(|e| DomainError::Validation(format!("invalid epub: {e}")))?;
                    let mut entry = archive
                        .by_name(&cover_entry)
                        .map_err(|e| DomainError::Storage(format!("cover missing: {e}")))?;
                    let mut buf = Vec::new();
                    entry
                        .read_to_end(&mut buf)
                        .map_err(|e| DomainError::Storage(format!("cover read error: {e}")))?;
                    let mime = sniff_image_mime(&buf).to_string();
                    return Ok(Some(BinaryPayload {
                        bytes: buf,
                        mime_type: mime,
                    }));
                }
            }

            if let Some(sidecar) = find_sidecar_cover(&p) {
                let buf = std::fs::read(&sidecar)
                    .map_err(|e| DomainError::Storage(format!("sidecar read error: {e}")))?;
                let mime = sniff_image_mime(&buf).to_string();
                return Ok(Some(BinaryPayload {
                    bytes: buf,
                    mime_type: mime,
                }));
            }
            Ok(None)
        })
        .await
        .map_err(|e| DomainError::Storage(format!("book cover read failed: {e}")))?
    }

    async fn write_artwork(
        &self,
        path: &Path,
        bytes: &[u8],
        mime_type: &str,
        _slot: &str,
    ) -> DomainResult<()> {
        let p = path.to_path_buf();
        let raw = bytes.to_vec();
        let sniffed = sniff_image_mime(&raw).to_string();
        let detected = if sniffed.starts_with("image/") && sniffed != "image/jpeg" {
            sniffed
        } else if mime_type.trim().starts_with("image/") {
            mime_type.trim().to_string()
        } else {
            sniffed
        };

        tokio::task::spawn_blocking(move || {
            let ext = p
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            let img_ext = if detected.contains("png") {
                "png"
            } else if detected.contains("webp") {
                "webp"
            } else {
                "jpg"
            };

            if ext != "epub" {
                let has_same_stem = ["jpg", "jpeg", "png", "webp"]
                    .into_iter()
                    .any(|e| p.with_extension(e).is_file());
                if !has_same_stem
                    && let Some(parent) = p.parent()
                    && (is_dedicated_calibre_book_dir(parent)
                        || ["jpg", "jpeg", "png", "webp"]
                            .into_iter()
                            .any(|e| parent.join(format!("cover.{e}")).is_file()))
                {
                    return write_image_replacing_opposite_ext(parent, "cover", img_ext, &raw);
                }
                let sidecar = p.with_extension(img_ext);
                atomic_write_bytes(&sidecar, &raw)?;
                for old_ext in ["jpg", "jpeg", "png", "webp"] {
                    if old_ext != img_ext {
                        let stale = p.with_extension(old_ext);
                        if stale.is_file() {
                            let _ = std::fs::remove_file(stale);
                        }
                    }
                }
                return Ok(());
            }

            if let Some(parent) = p.parent()
                && dir_has_single_book(parent)
                && ["jpg", "jpeg", "png", "webp"]
                    .into_iter()
                    .any(|e| parent.join(format!("cover.{e}")).is_file())
            {
                let _ = write_image_replacing_opposite_ext(parent, "cover", img_ext, &raw);
            }

            let parsed = parse_epub_archive(&p)?;
            if let Some(existing_cover) = parsed.cover_entry_path {
                let same_mime = parsed
                    .cover_media_type
                    .as_deref()
                    .is_some_and(|m| m.eq_ignore_ascii_case(&detected));
                if same_mime {
                    return rewrite_epub_entries(&p, vec![(existing_cover, raw)], &[]);
                }

                let old_raw_href = parsed
                    .cover_raw_href
                    .as_deref()
                    .unwrap_or(&existing_cover);
                let new_raw_href = swap_href_extension(old_raw_href, img_ext);
                let opf_dir = Path::new(&parsed.opf_path).parent().unwrap_or(Path::new(""));
                let new_zip_cover = normalize_zip_rel(opf_dir, &new_raw_href);

                let src_file = File::open(&p)
                    .map_err(|e| DomainError::Storage(format!("failed reading epub: {e}")))?;
                let mut archive = ZipArchive::new(BufReader::new(src_file))
                    .map_err(|e| DomainError::Validation(format!("invalid epub: {e}")))?;
                let mut opf_bytes = Vec::new();
                archive
                    .by_name(&parsed.opf_path)
                    .map_err(|_| DomainError::Validation("OPF missing".into()))?
                    .read_to_end(&mut opf_bytes)
                    .map_err(|e| DomainError::Storage(format!("read OPF error: {e}")))?;
                drop(archive);

                let opf_str = String::from_utf8_lossy(&opf_bytes);
                let updated_opf = update_manifest_cover_item(
                    &opf_str,
                    parsed.cover_item_id.as_deref(),
                    old_raw_href,
                    &new_raw_href,
                    &detected,
                );
                let skip = if new_zip_cover != existing_cover {
                    vec![existing_cover]
                } else {
                    Vec::new()
                };
                return rewrite_epub_entries(
                    &p,
                    vec![
                        (new_zip_cover, raw),
                        (parsed.opf_path, updated_opf.into_bytes()),
                    ],
                    &skip,
                );
            }

            // Inject a new cover image entry + manifest item into OPF
            let opf_dir = Path::new(&parsed.opf_path).parent().unwrap_or(Path::new(""));
            let rel_href = format!("xmedia-cover.{img_ext}");
            let zip_cover_path = normalize_zip_rel(opf_dir, &rel_href);

            let src_file = File::open(&p)
                .map_err(|e| DomainError::Storage(format!("failed reading epub: {e}")))?;
            let mut archive = ZipArchive::new(BufReader::new(src_file))
                .map_err(|e| DomainError::Validation(format!("invalid epub: {e}")))?;
            let mut opf_bytes = Vec::new();
            archive
                .by_name(&parsed.opf_path)
                .map_err(|_| DomainError::Validation("OPF missing".into()))?
                .read_to_end(&mut opf_bytes)
                .map_err(|e| DomainError::Storage(format!("read OPF error: {e}")))?;
            drop(archive);

            let mut opf_str = String::from_utf8_lossy(&opf_bytes).into_owned();
            let item_tag = format!(
                "<item id=\"xmedia-cover-img\" href=\"{}\" media-type=\"{}\" properties=\"cover-image\"/>\n  </manifest>",
                escape(&rel_href),
                escape(&detected)
            );
            opf_str = opf_str.replacen("</manifest>", &item_tag, 1);
            opf_str = replace_opf_metadata_block(
                &opf_str,
                &parsed.meta,
                Some("xmedia-cover-img"),
                &parsed.unique_identifier_id,
                &parsed.package_version,
            );

            rewrite_epub_entries(
                &p,
                vec![
                    (zip_cover_path, raw),
                    (parsed.opf_path, opf_str.into_bytes()),
                ],
                &[],
            )
        })
        .await
        .map_err(|e| DomainError::Storage(format!("book cover write failed: {e}")))?
    }
}

fn swap_href_extension(href: &str, new_ext: &str) -> String {
    match href.rsplit_once('.') {
        Some((stem, _)) if !stem.ends_with('/') => format!("{stem}.{new_ext}"),
        _ => format!("{href}.{new_ext}"),
    }
}

fn update_manifest_cover_item(
    opf_str: &str,
    cover_item_id: Option<&str>,
    old_href: &str,
    new_href: &str,
    new_mime: &str,
) -> String {
    let item_re = Regex::new(r"(?is)<item\b[^>]*?/?>").expect("valid item regex");
    let href_re = Regex::new(r#"(?i)\bhref\s*=\s*(?:"[^"]*"|'[^']*')"#).expect("valid href regex");
    let mime_re =
        Regex::new(r#"(?i)\bmedia-type\s*=\s*(?:"[^"]*"|'[^']*')"#).expect("valid mime regex");
    let id_re = Regex::new(r#"(?i)\bid\s*=\s*(?:"([^"]*)"|'([^']*)')"#).expect("valid id regex");
    let href_val_re =
        Regex::new(r#"(?i)\bhref\s*=\s*(?:"([^"]*)"|'([^']*)')"#).expect("valid href val regex");

    item_re
        .replace_all(opf_str, |caps: &regex::Captures<'_>| {
            let tag = &caps[0];
            let matched_by_id = cover_item_id.is_some_and(|cid| {
                id_re
                    .captures(tag)
                    .and_then(|c| c.get(1).or_else(|| c.get(2)))
                    .is_some_and(|m| m.as_str() == cid)
            });
            let matched_by_href = href_val_re
                .captures(tag)
                .and_then(|c| c.get(1).or_else(|| c.get(2)))
                .is_some_and(|m| m.as_str() == old_href);
            if !matched_by_id && !matched_by_href {
                return tag.to_string();
            }

            let updated = href_re.replace(tag, format!("href=\"{}\"", escape(new_href)).as_str());
            if mime_re.is_match(&updated) {
                mime_re
                    .replace(
                        &updated,
                        format!("media-type=\"{}\"", escape(new_mime)).as_str(),
                    )
                    .into_owned()
            } else {
                updated.into_owned()
            }
        })
        .into_owned()
}

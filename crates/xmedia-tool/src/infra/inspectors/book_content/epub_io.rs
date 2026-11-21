use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::LazyLock;

use quick_xml::Reader;
use quick_xml::escape::escape;
use quick_xml::events::Event;
use regex::Regex;
use xmedia_core::application::ports::BinaryPayload;
use xmedia_core::domain::error::{DomainError, DomainResult};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::domain::media::BookFileMeta;
use crate::infra::inspectors::append_xml_ref;
use crate::infra::inspectors::book::{
    attr_value, find_opf_path_in_container, normalize_zip_rel, replace_opf_metadata_block,
};

use super::EditableBookChapterDto;
use super::xhtml::{
    detect_rich_media_in_xhtml, extract_heading_from_xhtml, split_xhtml_head_and_body,
    xhtml_body_to_plain,
};

#[derive(Clone, Debug)]
pub(crate) struct ManifestNode {
    pub(crate) id: String,
    pub(crate) href: String,
    pub(crate) full_path: String,
    pub(crate) media_type: String,
    pub(crate) properties: Option<String>,
}

pub(crate) struct EpubStructureInfo {
    pub(crate) opf_path: String,
    pub(crate) opf_dir: PathBuf,
    pub(crate) raw_opf: String,
    pub(crate) package_version: String,
    pub(crate) unique_id: String,
    pub(crate) toc_ncx_attr: Option<String>,
    pub(crate) spine_page_progression: Option<String>,
    pub(crate) cover_meta_id: Option<String>,
    pub(crate) manifest: Vec<ManifestNode>,
    pub(crate) spine_idrefs: Vec<String>,
}

pub(crate) fn percent_decode_path(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (
                (bytes[i + 1] as char).to_digit(16),
                (bytes[i + 2] as char).to_digit(16),
            )
        {
            out.push(((hi << 4) | lo) as u8);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| raw.to_string())
}

pub(crate) fn read_zip_bytes<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
    entry_path: &str,
) -> Option<Vec<u8>> {
    if let Ok(mut f) = archive.by_name(entry_path) {
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).ok()?;
        return Some(buf);
    }
    if entry_path.contains('%') {
        let decoded = percent_decode_path(entry_path);
        if let Ok(mut f) = archive.by_name(&decoded) {
            let mut buf = Vec::new();
            f.read_to_end(&mut buf).ok()?;
            return Some(buf);
        }
    }
    None
}

pub(crate) fn parse_epub_structure<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
) -> DomainResult<EpubStructureInfo> {
    let container_bytes = read_zip_bytes(archive, "META-INF/container.xml")
        .ok_or_else(|| DomainError::Validation("EPUB missing META-INF/container.xml".into()))?;
    let opf_path = find_opf_path_in_container(&container_bytes)
        .ok_or_else(|| DomainError::Validation("EPUB container.xml missing rootfile".into()))?;
    let opf_bytes = read_zip_bytes(archive, &opf_path)
        .ok_or_else(|| DomainError::Validation(format!("EPUB missing OPF '{opf_path}'")))?;
    let raw_opf = String::from_utf8_lossy(&opf_bytes).into_owned();
    let opf_dir = Path::new(&opf_path)
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();

    let mut reader = Reader::from_str(&raw_opf);
    reader.config_mut().trim_text(true);

    let mut package_version = "3.0".to_string();
    let mut unique_id = "bookid".to_string();
    let mut toc_ncx_attr = None;
    let mut spine_page_progression = None;
    let mut cover_meta_id = None;
    let mut manifest = Vec::new();
    let mut spine_idrefs = Vec::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e) | Event::Empty(ref e)) => {
                let local = e.local_name().as_ref().to_ascii_lowercase();
                match local.as_str() {
                    "package" => {
                        if let Some(v) = attr_value(e, "version") {
                            package_version = v;
                        }
                        if let Some(uid) = attr_value(e, "unique-identifier") {
                            unique_id = uid;
                        }
                    }
                    "meta" => {
                        if attr_value(e, "name")
                            .is_some_and(|n| n.trim().eq_ignore_ascii_case("cover"))
                        {
                            cover_meta_id = attr_value(e, "content")
                                .map(|s| s.trim().to_string())
                                .filter(|s| !s.is_empty());
                        }
                    }
                    "item" => {
                        if let (Some(id), Some(href)) = (attr_value(e, "id"), attr_value(e, "href"))
                        {
                            let media_type = attr_value(e, "media-type").unwrap_or_default();
                            let properties = attr_value(e, "properties");
                            let full_path = normalize_zip_rel(&opf_dir, &href);
                            manifest.push(ManifestNode {
                                id,
                                href,
                                full_path,
                                media_type,
                                properties,
                            });
                        }
                    }
                    "spine" => {
                        toc_ncx_attr = attr_value(e, "toc");
                        spine_page_progression = attr_value(e, "page-progression-direction");
                    }
                    "itemref" => {
                        if let Some(idref) = attr_value(e, "idref") {
                            spine_idrefs.push(idref);
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }

    Ok(EpubStructureInfo {
        opf_path,
        opf_dir,
        raw_opf,
        package_version,
        unique_id,
        toc_ncx_attr,
        spine_page_progression,
        cover_meta_id,
        manifest,
        spine_idrefs,
    })
}

pub(crate) fn parse_nav_toc_map(nav_bytes: &[u8], nav_dir: &Path) -> HashMap<String, (String, u8)> {
    let xml = String::from_utf8_lossy(nav_bytes);
    let mut reader = Reader::from_str(&xml);
    reader.config_mut().trim_text(true);

    let mut out: HashMap<String, (String, u8)> = HashMap::new();
    let mut in_toc_nav = false;
    let mut nav_depth = 0usize;
    let mut ol_depth = 0u8;
    let mut active_href: Option<String> = None;
    let mut text_buf = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let local = e.local_name().as_ref().to_ascii_lowercase();
                match local.as_str() {
                    "nav" => {
                        if nav_depth == 0 {
                            let is_toc = attr_value(e, "type")
                                .is_some_and(|v| v.split_whitespace().any(|t| t == "toc"))
                                || attr_value(e, "role")
                                    .is_some_and(|v| v.split_whitespace().any(|t| t == "doc-toc"));
                            in_toc_nav = is_toc || out.is_empty();
                            ol_depth = 0;
                        }
                        nav_depth += 1;
                    }
                    "ol" | "ul" if in_toc_nav => {
                        ol_depth = ol_depth.saturating_add(1);
                    }
                    "a" if in_toc_nav && ol_depth > 0 => {
                        active_href = attr_value(e, "href").map(|h| normalize_zip_rel(nav_dir, &h));
                        text_buf.clear();
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(ref t)) if active_href.is_some() => {
                text_buf.push_str(&t.xml_content(quick_xml::XmlVersion::Implicit1_0));
            }
            Ok(Event::GeneralRef(ref r)) if active_href.is_some() => {
                append_xml_ref(&mut text_buf, r);
            }
            Ok(Event::End(ref e)) => {
                let local = e.local_name().as_ref().to_ascii_lowercase();
                match local.as_str() {
                    "a" if in_toc_nav => {
                        if let Some(full) = active_href.take() {
                            let title = text_buf.trim().to_string();
                            if !title.is_empty() {
                                out.entry(full)
                                    .or_insert((title, ol_depth.saturating_sub(1).min(1)));
                            }
                        }
                        text_buf.clear();
                    }
                    "ol" | "ul" if in_toc_nav => {
                        ol_depth = ol_depth.saturating_sub(1);
                    }
                    "nav" if nav_depth > 0 => {
                        nav_depth -= 1;
                        if nav_depth == 0 {
                            in_toc_nav = false;
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }

    out
}

pub(crate) fn parse_ncx_toc_map(ncx_bytes: &[u8], ncx_dir: &Path) -> HashMap<String, (String, u8)> {
    let xml = String::from_utf8_lossy(ncx_bytes);
    let mut reader = Reader::from_str(&xml);
    reader.config_mut().trim_text(true);

    let mut out: HashMap<String, (String, u8)> = HashMap::new();
    let mut depth = 0u8;
    let mut in_text = false;
    let mut cur_title = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let local = e.local_name().as_ref().to_ascii_lowercase();
                match local.as_str() {
                    "navpoint" => {
                        depth = depth.saturating_add(1);
                        cur_title.clear();
                    }
                    "text" if depth > 0 => {
                        in_text = true;
                        cur_title.clear();
                    }
                    "content" if depth > 0 => {
                        if let Some(src) = attr_value(e, "src") {
                            let full = normalize_zip_rel(ncx_dir, &src);
                            let t = cur_title.trim().to_string();
                            if !t.is_empty() {
                                out.entry(full)
                                    .or_insert((t, depth.saturating_sub(1).min(1)));
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(ref e)) => {
                if depth > 0
                    && e.local_name().as_ref().eq_ignore_ascii_case("content")
                    && let Some(src) = attr_value(e, "src")
                {
                    let full = normalize_zip_rel(ncx_dir, &src);
                    let t = cur_title.trim().to_string();
                    if !t.is_empty() {
                        out.entry(full)
                            .or_insert((t, depth.saturating_sub(1).min(1)));
                    }
                }
            }
            Ok(Event::Text(ref t)) if in_text => {
                cur_title.push_str(&t.xml_content(quick_xml::XmlVersion::Implicit1_0));
            }
            Ok(Event::GeneralRef(ref r)) if in_text => {
                append_xml_ref(&mut cur_title, r);
            }
            Ok(Event::End(ref e)) => {
                let local = e.local_name().as_ref().to_ascii_lowercase();
                match local.as_str() {
                    "text" => in_text = false,
                    "navpoint" => depth = depth.saturating_sub(1),
                    _ => {}
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }

    out
}

pub(crate) fn is_nav_manifest_node(m: &ManifestNode) -> bool {
    m.properties
        .as_deref()
        .is_some_and(|p| p.split_whitespace().any(|tok| tok == "nav"))
}

pub(super) fn is_cover_wrapper_doc(
    m: &ManifestNode,
    toc_map: &HashMap<String, (String, u8)>,
    plain: &str,
    body_xhtml: &str,
) -> bool {
    if toc_map.contains_key(&m.full_path) {
        return false;
    }
    let id_lower = m.id.to_ascii_lowercase();
    let href_lower = m.href.to_ascii_lowercase();
    let looks_cover_named = id_lower == "cover"
        || id_lower == "cover-page"
        || href_lower.ends_with("cover.xhtml")
        || href_lower.ends_with("cover.html")
        || href_lower.ends_with("titlepage.xhtml");
    looks_cover_named && plain.chars().count() < 20 && detect_rich_media_in_xhtml(body_xhtml)
}

pub fn read_epub_internal_asset(
    epub_path: &Path,
    raw_asset_path: &str,
) -> DomainResult<BinaryPayload> {
    let normalized = normalize_zip_rel(Path::new(""), raw_asset_path.trim_start_matches('/'));
    if normalized.is_empty() {
        return Err(DomainError::Validation("empty epub asset path".into()));
    }
    let f = File::open(epub_path)
        .map_err(|e| DomainError::Storage(format!("failed opening epub: {e}")))?;
    let mut archive = ZipArchive::new(BufReader::new(f))
        .map_err(|e| DomainError::Validation(format!("invalid epub archive: {e}")))?;
    let bytes = read_zip_bytes(&mut archive, &normalized).ok_or_else(|| {
        DomainError::NotFound(format!("epub internal asset '{normalized}' not found"))
    })?;
    let mime_type =
        if let Some(raster) = xmedia_core::infra::thumbnail::sniff_raster_image_mime(&bytes) {
            raster.to_string()
        } else {
            let ext = Path::new(&normalized)
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_ascii_lowercase())
                .unwrap_or_default();
            match ext.as_str() {
                "woff" => "font/woff".to_string(),
                "woff2" => "font/woff2".to_string(),
                "ttf" => "font/ttf".to_string(),
                "otf" => "font/otf".to_string(),
                _ => {
                    return Err(DomainError::Validation(format!(
                        "epub asset '{normalized}' is not an allowed raster image or font"
                    )));
                }
            }
        };
    Ok(BinaryPayload { bytes, mime_type })
}

pub(super) fn read_epub_chapters_and_styles(
    epub_path: &Path,
) -> DomainResult<(Vec<EditableBookChapterDto>, Vec<String>)> {
    let f = File::open(epub_path)
        .map_err(|e| DomainError::Storage(format!("failed opening epub: {e}")))?;
    let mut archive = ZipArchive::new(BufReader::new(f))
        .map_err(|e| DomainError::Validation(format!("invalid epub archive: {e}")))?;
    let info = parse_epub_structure(&mut archive)?;

    let toc_map = read_epub_toc_map(&mut archive, &info);

    let manifest_by_id: HashMap<&str, &ManifestNode> =
        info.manifest.iter().map(|m| (m.id.as_str(), m)).collect();

    let mut chapters = Vec::new();
    for idref in &info.spine_idrefs {
        let Some(node) = manifest_by_id.get(idref.as_str()).copied() else {
            continue;
        };
        if is_nav_manifest_node(node) {
            continue;
        }
        if !node.media_type.contains("html") && !node.media_type.contains("xml") {
            continue;
        }
        let Some(bytes) = read_zip_bytes(&mut archive, &node.full_path) else {
            continue;
        };
        let full_str = String::from_utf8_lossy(&bytes);
        let (_, body_inner) = split_xhtml_head_and_body(&full_str);

        let (toc_title_opt, level) = toc_map
            .get(&node.full_path)
            .map(|(t, l)| (Some(t.clone()), *l))
            .unwrap_or((None, 0));

        let preliminary_plain = xhtml_body_to_plain(&body_inner, None);
        if is_cover_wrapper_doc(node, &toc_map, &preliminary_plain, &body_inner) {
            continue;
        }

        let resolved_title = toc_title_opt
            .or_else(|| extract_heading_from_xhtml(&full_str))
            .unwrap_or_else(|| format!("第 {} 章", chapters.len() + 1));

        let plain_text = xhtml_body_to_plain(&body_inner, Some(&resolved_title));
        let has_rich_media = detect_rich_media_in_xhtml(&body_inner);

        chapters.push(EditableBookChapterDto {
            id: node.id.clone(),
            title: resolved_title,
            level,
            plain_text,
            xhtml_body: body_inner,
            has_rich_media,
            source_href: Some(node.full_path.clone()),
        });
    }

    let mut stylesheets = Vec::new();
    for m in &info.manifest {
        if m.media_type.eq_ignore_ascii_case("text/css")
            && let Some(css_bytes) = read_zip_bytes(&mut archive, &m.full_path)
        {
            let css_str = String::from_utf8_lossy(&css_bytes).trim().to_string();
            if !css_str.is_empty() {
                stylesheets.push(css_str);
            }
        }
    }

    Ok((chapters, stylesheets))
}

pub(crate) fn relative_href_from_dir(from_dir: &Path, target_full_path: &str) -> String {
    let from_comps: Vec<String> = from_dir
        .components()
        .filter_map(|c| match c {
            Component::Normal(p) => Some(p.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    let target_comps: Vec<&str> = target_full_path
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();

    let mut common = 0usize;
    while common < from_comps.len()
        && common < target_comps.len()
        && from_comps[common] == target_comps[common]
    {
        common += 1;
    }

    let mut rel_parts: Vec<String> = Vec::new();
    for _ in common..from_comps.len() {
        rel_parts.push("..".to_string());
    }
    for seg in &target_comps[common..] {
        rel_parts.push((*seg).to_string());
    }
    rel_parts.join("/")
}

/// Renders an EPUB 3 Navigation Document (`nav.xhtml`) supporting 2-level Volume -> Chapter nesting.
pub(crate) fn render_nav_xhtml(
    book_title: &str,
    lang: &str,
    items: &[(String, String, u8)],
    nav_dir: &Path,
) -> String {
    let mut body = String::new();
    body.push_str("  <nav epub:type=\"toc\" id=\"toc\" role=\"doc-toc\">\n");
    body.push_str(&format!("    <h1>{}</h1>\n", escape(book_title)));
    body.push_str("    <ol>\n");

    let mut idx = 0usize;
    while idx < items.len() {
        let (title, full_path, _lvl) = &items[idx];
        let rel_href = relative_href_from_dir(nav_dir, full_path);

        // Collect immediately following level=1 children if this item is level=0
        let is_top = items[idx].2 == 0 || idx == 0;
        let mut children = Vec::new();
        if is_top {
            let mut next = idx + 1;
            while next < items.len() && items[next].2 > 0 {
                children.push(&items[next]);
                next += 1;
            }
        }

        if children.is_empty() {
            body.push_str(&format!(
                "      <li><a href=\"{}\">{}</a></li>\n",
                escape(&rel_href),
                escape(title)
            ));
            idx += 1;
        } else {
            body.push_str(&format!(
                "      <li>\n        <a href=\"{}\">{}</a>\n        <ol>\n",
                escape(&rel_href),
                escape(title)
            ));
            for (ct, cp, _) in &children {
                let crel = relative_href_from_dir(nav_dir, cp);
                body.push_str(&format!(
                    "          <li><a href=\"{}\">{}</a></li>\n",
                    escape(&crel),
                    escape(ct)
                ));
            }
            body.push_str("        </ol>\n      </li>\n");
            idx += 1 + children.len();
        }
    }

    body.push_str("    </ol>\n  </nav>");

    let clean_lang = if lang.trim().is_empty() {
        "zh-CN"
    } else {
        lang.trim()
    };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\" xml:lang=\"{l}\" lang=\"{l}\">\n<head>\n  <meta charset=\"utf-8\"/>\n  <title>{t}</title>\n</head>\n<body>\n{body}\n</body>\n</html>\n",
        l = escape(clean_lang),
        t = escape(book_title),
    )
}

/// Renders an EPUB 2 NCX Table of Contents (`toc.ncx`) for universal reader compatibility.
pub(crate) fn render_toc_ncx(
    book_uid: &str,
    book_title: &str,
    items: &[(String, String, u8)],
    ncx_dir: &Path,
) -> String {
    let has_sub = items.iter().any(|(_, _, lvl)| *lvl > 0);
    let depth = if has_sub { 2 } else { 1 };
    let mut nav_map = String::new();
    let mut play_order = 1usize;

    let mut idx = 0usize;
    while idx < items.len() {
        let (title, full_path, _) = &items[idx];
        let rel_href = relative_href_from_dir(ncx_dir, full_path);
        let cur_order = play_order;
        play_order += 1;

        let is_top = items[idx].2 == 0 || idx == 0;
        let mut children = Vec::new();
        if is_top {
            let mut next = idx + 1;
            while next < items.len() && items[next].2 > 0 {
                children.push(&items[next]);
                next += 1;
            }
        }

        nav_map.push_str(&format!(
            "    <navPoint id=\"navPoint-{cur_order}\" playOrder=\"{cur_order}\">\n      <navLabel><text>{}</text></navLabel>\n      <content src=\"{}\"/>\n",
            escape(title),
            escape(&rel_href)
        ));

        for (ct, cp, _) in &children {
            let c_order = play_order;
            play_order += 1;
            let crel = relative_href_from_dir(ncx_dir, cp);
            nav_map.push_str(&format!(
                "      <navPoint id=\"navPoint-{c_order}\" playOrder=\"{c_order}\">\n        <navLabel><text>{}</text></navLabel>\n        <content src=\"{}\"/>\n      </navPoint>\n",
                escape(ct),
                escape(&crel)
            ));
        }

        nav_map.push_str("    </navPoint>\n");
        idx += 1 + children.len();
    }

    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ncx xmlns=\"http://www.daisy.org/z3986/2005/ncx/\" version=\"2005-1\">\n  <head>\n    <meta name=\"dtb:uid\" content=\"{uid}\"/>\n    <meta name=\"dtb:depth\" content=\"{depth}\"/>\n    <meta name=\"dtb:totalPageCount\" content=\"0\"/>\n    <meta name=\"dtb:maxPageNumber\" content=\"0\"/>\n  </head>\n  <docTitle><text>{title}</text></docTitle>\n  <navMap>\n{nav_map}  </navMap>\n</ncx>\n",
        uid = escape(book_uid),
        title = escape(book_title),
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn rebuild_opf_manifest_and_spine(
    raw_opf: &str,
    manifest: &[ManifestNode],
    spine_ids: &[String],
    ncx_id: &str,
    spine_page_progression: Option<&str>,
    meta: &BookFileMeta,
    unique_id: &str,
    package_version: &str,
) -> String {
    static MANIFEST_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?is)<(?P<p>\w+:)?manifest\b[^>]*>.*?</(?:\w+:)?manifest>")
            .expect("valid manifest regex")
    });
    static SPINE_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?is)<(?P<p>\w+:)?spine\b[^>]*>.*?</(?:\w+:)?spine>")
            .expect("valid spine regex")
    });
    static PKG_VER_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"(<(?:\w+:)?package\b[^>]*?\bversion\s*=\s*["'])[^"']+(["'])"#)
            .expect("valid package version regex")
    });

    // Upgrade package version to 3.0 when nav property is present so EPUB3 validators pass
    let upgraded_opf = PKG_VER_RE.replace(raw_opf, "${1}3.0${2}").into_owned();
    let effective_ver = if package_version.starts_with('3') {
        package_version
    } else {
        "3.0"
    };

    let mut manifest_xml = String::from("<manifest>\n");
    for item in manifest {
        let props_attr = item
            .properties
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .map(|p| format!(" properties=\"{}\"", escape(p)))
            .unwrap_or_default();
        manifest_xml.push_str(&format!(
            "    <item id=\"{}\" href=\"{}\" media-type=\"{}\"{}/>\n",
            escape(&item.id),
            escape(&item.href),
            escape(&item.media_type),
            props_attr
        ));
    }
    manifest_xml.push_str("  </manifest>");

    let dir_attr = spine_page_progression
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|d| format!(" page-progression-direction=\"{}\"", escape(d)))
        .unwrap_or_default();
    let mut spine_xml = format!("<spine toc=\"{}\"{dir_attr}>\n", escape(ncx_id));
    for idref in spine_ids {
        spine_xml.push_str(&format!("    <itemref idref=\"{}\"/>\n", escape(idref)));
    }
    spine_xml.push_str("  </spine>");

    let with_manifest = MANIFEST_RE
        .replace(&upgraded_opf, manifest_xml.as_str())
        .into_owned();
    let with_spine = SPINE_RE
        .replace(&with_manifest, spine_xml.as_str())
        .into_owned();

    replace_opf_metadata_block(&with_spine, meta, None, unique_id, effective_ver)
}

pub(crate) fn ext_for_image_mime(mime: &str) -> &'static str {
    match mime.to_ascii_lowercase().as_str() {
        "image/webp" => "webp",
        "image/png" => "png",
        "image/gif" => "gif",
        "image/svg+xml" => "svg",
        _ => "jpg",
    }
}

pub(crate) fn epub_stored_zip_options() -> SimpleFileOptions {
    SimpleFileOptions::default().compression_method(CompressionMethod::Stored)
}

pub(crate) fn epub_deflated_zip_options(best: bool) -> SimpleFileOptions {
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    if best {
        opts.compression_level(Some(9))
    } else {
        opts
    }
}

pub(crate) fn epub_entry_zip_options(
    path: &str,
    media_type: &str,
    best: bool,
) -> SimpleFileOptions {
    if !best {
        return epub_deflated_zip_options(false);
    }
    let lower_mt = media_type.to_ascii_lowercase();
    let lower_path = path.to_ascii_lowercase();
    let precompressed = lower_mt == "image/jpeg"
        || lower_mt == "image/webp"
        || lower_mt == "font/woff2"
        || lower_mt == "application/font-woff2"
        || lower_path.ends_with(".jpg")
        || lower_path.ends_with(".jpeg")
        || lower_path.ends_with(".webp")
        || lower_path.ends_with(".woff2");
    if precompressed {
        epub_stored_zip_options()
    } else {
        epub_deflated_zip_options(true)
    }
}

pub(crate) fn write_zip_entry<W: Write + std::io::Seek>(
    writer: &mut ZipWriter<W>,
    path: &str,
    opts: SimpleFileOptions,
    bytes: &[u8],
    label: &str,
) -> DomainResult<()> {
    writer
        .start_file(path, opts)
        .map_err(|e| DomainError::Storage(format!("epub {label} start error: {e}")))?;
    writer
        .write_all(bytes)
        .map_err(|e| DomainError::Storage(format!("epub {label} write error: {e}")))?;
    Ok(())
}

pub(crate) fn read_epub_toc_map<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
    info: &EpubStructureInfo,
) -> HashMap<String, (String, u8)> {
    let mut toc_map: HashMap<String, (String, u8)> = HashMap::new();
    if let Some(nav_node) = info.manifest.iter().find(|m| is_nav_manifest_node(m))
        && let Some(nav_bytes) = read_zip_bytes(archive, &nav_node.full_path)
    {
        let nav_dir = Path::new(&nav_node.full_path)
            .parent()
            .unwrap_or_else(|| Path::new(""));
        toc_map = parse_nav_toc_map(&nav_bytes, nav_dir);
    }
    if toc_map.is_empty()
        && let Some(ncx_node) = info.manifest.iter().find(|m| {
            info.toc_ncx_attr.as_deref() == Some(m.id.as_str())
                || m.media_type
                    .eq_ignore_ascii_case("application/x-dtbncx+xml")
        })
        && let Some(ncx_bytes) = read_zip_bytes(archive, &ncx_node.full_path)
    {
        let ncx_dir = Path::new(&ncx_node.full_path)
            .parent()
            .unwrap_or_else(|| Path::new(""));
        toc_map = parse_ncx_toc_map(&ncx_bytes, ncx_dir);
    }
    toc_map
}

pub(crate) fn find_cover_manifest_node(info: &EpubStructureInfo) -> Option<&ManifestNode> {
    info.manifest
        .iter()
        .find(|m| {
            m.media_type.starts_with("image/")
                && (m
                    .properties
                    .as_deref()
                    .is_some_and(|p| p.split_whitespace().any(|t| t == "cover-image"))
                    || info.cover_meta_id.as_deref() == Some(m.id.as_str()))
        })
        .or_else(|| {
            info.manifest.iter().find(|m| {
                m.media_type.starts_with("image/")
                    && (m.id.to_ascii_lowercase().contains("cover")
                        || m.href.to_ascii_lowercase().contains("cover"))
            })
        })
}

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::LazyLock;

use quick_xml::escape::escape;
use regex::Regex;
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::infra::thumbnail::sniff_image_mime;
use zip::ZipArchive;

use crate::infra::inspectors::book::{normalize_zip_rel, read_book_meta_any, rewrite_epub_entries};

use super::super::epub_io::{
    ManifestNode, ext_for_image_mime, find_cover_manifest_node, is_cover_wrapper_doc,
    is_nav_manifest_node, parse_epub_structure, percent_decode_path, read_epub_toc_map,
    read_zip_bytes, rebuild_opf_manifest_and_spine, relative_href_from_dir, render_nav_xhtml,
    render_toc_ncx,
};
use super::super::xhtml::{
    extract_heading_from_xhtml, render_book_jacket_xhtml, split_xhtml_head_and_body,
    xhtml_body_to_plain,
};
use super::super::{InsertedEpubImageDto, PolishBookReportDto};

pub fn insert_epub_image(
    epub_path: &Path,
    chapter_source_href: Option<&str>,
    bytes: &[u8],
    mime_hint: Option<&str>,
) -> DomainResult<InsertedEpubImageDto> {
    if bytes.is_empty() {
        return Err(DomainError::Validation("上传的图片文件为空".into()));
    }
    let ext_check = epub_path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if ext_check != "epub" {
        return Err(DomainError::Validation(
            "仅 EPUB 图书支持章节内插入图片".into(),
        ));
    }

    let sniffed = sniff_image_mime(bytes);
    let mime_type = if sniffed.starts_with("image/") {
        sniffed.to_string()
    } else if let Some(hint) = mime_hint.filter(|m| m.starts_with("image/")) {
        hint.to_string()
    } else {
        return Err(DomainError::Validation(
            "不支持的图片格式，请上传 JPG/PNG/WebP/GIF/SVG".into(),
        ));
    };

    let img_ext = ext_for_image_mime(&mime_type);

    let f = File::open(epub_path)
        .map_err(|e| DomainError::Storage(format!("failed opening epub: {e}")))?;
    let mut archive = ZipArchive::new(BufReader::new(f))
        .map_err(|e| DomainError::Validation(format!("invalid epub archive: {e}")))?;
    let info = parse_epub_structure(&mut archive)?;
    drop(archive);

    let short_id = &Uuid::now_v7().simple().to_string()[..12];
    let rel_img = format!("Images/ximg_{short_id}.{img_ext}");
    let full_img = normalize_zip_rel(&info.opf_dir, &rel_img);
    let href_in_opf = relative_href_from_dir(&info.opf_dir, &full_img);
    let item_id = format!("ximg-{short_id}");

    static CLOSE_MANIFEST_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)</(?:\w+:)?manifest>").expect("valid manifest end regex")
    });

    let item_line = format!(
        "    <item id=\"{}\" href=\"{}\" media-type=\"{}\"/>\n  ",
        escape(&item_id),
        escape(&href_in_opf),
        escape(&mime_type)
    );
    let updated_opf = if let Some(m) = CLOSE_MANIFEST_RE.find(&info.raw_opf) {
        let mut s = info.raw_opf.clone();
        s.insert_str(m.start(), &item_line);
        s
    } else {
        return Err(DomainError::Validation(
            "EPUB OPF 缺少 <manifest> 节点".into(),
        ));
    };

    rewrite_epub_entries(
        epub_path,
        vec![
            (full_img.clone(), bytes.to_vec()),
            (info.opf_path, updated_opf.into_bytes()),
        ],
        &[],
    )?;

    let fallback_ch_full = normalize_zip_rel(&info.opf_dir, "Text/chapter.xhtml");
    let ch_ref_full = chapter_source_href
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(&fallback_ch_full);
    let ch_dir = Path::new(ch_ref_full)
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let chapter_rel_href = relative_href_from_dir(ch_dir, &full_img);

    Ok(InsertedEpubImageDto {
        asset_path: full_img,
        chapter_rel_href,
        mime_type,
    })
}

pub fn polish_epub_book(epub_path: &Path) -> DomainResult<PolishBookReportDto> {
    let ext_check = epub_path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if ext_check != "epub" {
        return Err(DomainError::Validation("仅 EPUB 图书支持一键抛光".into()));
    }

    let meta = read_book_meta_any(epub_path)?;
    let lang = meta.language.as_deref().unwrap_or("zh-CN");

    let f = File::open(epub_path)
        .map_err(|e| DomainError::Storage(format!("failed opening epub: {e}")))?;
    let mut archive = ZipArchive::new(BufReader::new(f))
        .map_err(|e| DomainError::Validation(format!("invalid epub archive: {e}")))?;
    let info = parse_epub_structure(&mut archive)?;

    let toc_map = read_epub_toc_map(&mut archive, &info);

    let cover_img_node = find_cover_manifest_node(&info);
    let cover_img_id = cover_img_node.map(|m| m.id.clone());
    let cover_img_full = cover_img_node.map(|m| m.full_path.clone());

    let primary_css_full = info
        .manifest
        .iter()
        .find(|m| m.media_type.eq_ignore_ascii_case("text/css"))
        .map(|m| m.full_path.clone());

    let mut extra_manifest_nodes: Vec<ManifestNode> = Vec::new();
    let mut replacements: Vec<(String, Vec<u8>)> = Vec::new();

    let (jacket_id, jacket_full_path) = if let Some(existing_jacket) = info
        .manifest
        .iter()
        .find(|m| m.id == "xmedia-jacket" || m.href.ends_with("xmedia_jacket.xhtml"))
    {
        (
            existing_jacket.id.clone(),
            existing_jacket.full_path.clone(),
        )
    } else {
        let jf = normalize_zip_rel(&info.opf_dir, "Text/xmedia_jacket.xhtml");
        let jh = relative_href_from_dir(&info.opf_dir, &jf);
        let jid = "xmedia-jacket".to_string();
        extra_manifest_nodes.push(ManifestNode {
            id: jid.clone(),
            href: jh,
            full_path: jf.clone(),
            media_type: "application/xhtml+xml".to_string(),
            properties: None,
        });
        (jid, jf)
    };

    let jacket_dir = Path::new(&jacket_full_path)
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let jacket_cover_rel = cover_img_full
        .as_deref()
        .map(|cf| relative_href_from_dir(jacket_dir, cf));
    let jacket_css_rel = primary_css_full
        .as_deref()
        .map(|sf| relative_href_from_dir(jacket_dir, sf));
    let jacket_xhtml = render_book_jacket_xhtml(
        &meta,
        jacket_cover_rel.as_deref(),
        jacket_css_rel.as_deref(),
    );
    replacements.push((jacket_full_path.clone(), jacket_xhtml.clone().into_bytes()));

    let manifest_by_id: HashMap<&str, &ManifestNode> =
        info.manifest.iter().map(|m| (m.id.as_str(), m)).collect();

    let mut leading_cover_wrapper_id: Option<String> = None;
    if let Some(first_id) = info.spine_idrefs.first()
        && let Some(first_node) = manifest_by_id.get(first_id.as_str()).copied()
        && first_node.id != jacket_id
        && let Some(bytes) = read_zip_bytes(&mut archive, &first_node.full_path)
    {
        let full_str = String::from_utf8_lossy(&bytes);
        let (_, body_inner) = split_xhtml_head_and_body(&full_str);
        let plain = xhtml_body_to_plain(&body_inner, None);
        if is_cover_wrapper_doc(first_node, &toc_map, &plain, &body_inner) {
            leading_cover_wrapper_id = Some(first_node.id.clone());
        }
    }

    let mut spine_ids: Vec<String> = Vec::with_capacity(info.spine_idrefs.len() + 1);
    if let Some(ref cov_id) = leading_cover_wrapper_id {
        spine_ids.push(cov_id.clone());
    }
    spine_ids.push(jacket_id.clone());
    for idref in &info.spine_idrefs {
        if idref == &jacket_id || leading_cover_wrapper_id.as_deref() == Some(idref.as_str()) {
            continue;
        }
        if manifest_by_id.contains_key(idref.as_str()) {
            spine_ids.push(idref.clone());
        }
    }

    let mut toc_triples: Vec<(String, String, u8)> = Vec::new();
    toc_triples.push(("书籍简介".to_string(), jacket_full_path.clone(), 0));

    for (idx, idref) in spine_ids.iter().enumerate() {
        if idref == &jacket_id || leading_cover_wrapper_id.as_deref() == Some(idref.as_str()) {
            continue;
        }
        let Some(node) = manifest_by_id.get(idref.as_str()).copied() else {
            continue;
        };
        if is_nav_manifest_node(node) {
            continue;
        }
        if let Some((t, lvl)) = toc_map.get(&node.full_path) {
            toc_triples.push((t.clone(), node.full_path.clone(), *lvl));
        } else if let Some(bytes) = read_zip_bytes(&mut archive, &node.full_path) {
            let s = String::from_utf8_lossy(&bytes);
            let title =
                extract_heading_from_xhtml(&s).unwrap_or_else(|| format!("第 {} 章", idx + 1));
            toc_triples.push((title, node.full_path.clone(), 0));
        }
    }

    let mut all_doc_sources = String::new();
    all_doc_sources.push_str(&jacket_xhtml);
    for m in &info.manifest {
        if m.full_path == jacket_full_path {
            continue;
        }
        let mt = m.media_type.to_ascii_lowercase();
        if (mt.contains("html") || mt.contains("xml") || mt.contains("css") || mt.contains("svg"))
            && let Some(bytes) = read_zip_bytes(&mut archive, &m.full_path)
        {
            all_doc_sources.push_str(&String::from_utf8_lossy(&bytes));
            all_doc_sources.push('\n');
        }
    }

    let mut all_zip_files = Vec::new();
    for i in 0..archive.len() {
        if let Ok(zf) = archive.by_index(i)
            && let Ok(name_ref) = zf.name()
        {
            let name = name_ref.into_owned();
            if !name.ends_with('/') {
                all_zip_files.push(name);
            }
        }
    }
    drop(archive);

    let nav_full_path =
        if let Some(existing_nav) = info.manifest.iter().find(|m| is_nav_manifest_node(m)) {
            existing_nav.full_path.clone()
        } else {
            let nf = normalize_zip_rel(&info.opf_dir, "nav.xhtml");
            let nh = relative_href_from_dir(&info.opf_dir, &nf);
            extra_manifest_nodes.push(ManifestNode {
                id: "xmedia-nav".to_string(),
                href: nh,
                full_path: nf.clone(),
                media_type: "application/xhtml+xml".to_string(),
                properties: Some("nav".to_string()),
            });
            nf
        };
    let nav_dir = Path::new(&nav_full_path)
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let nav_xml = render_nav_xhtml(&meta.title, lang, &toc_triples, nav_dir);
    replacements.push((nav_full_path, nav_xml.into_bytes()));

    let (ncx_id, ncx_full_path) = if let Some(existing_ncx) = info.manifest.iter().find(|m| {
        info.toc_ncx_attr.as_deref() == Some(m.id.as_str())
            || m.media_type
                .eq_ignore_ascii_case("application/x-dtbncx+xml")
    }) {
        (existing_ncx.id.clone(), existing_ncx.full_path.clone())
    } else {
        let nf = normalize_zip_rel(&info.opf_dir, "toc.ncx");
        let nh = relative_href_from_dir(&info.opf_dir, &nf);
        let nid = "xmedia-ncx".to_string();
        extra_manifest_nodes.push(ManifestNode {
            id: nid.clone(),
            href: nh,
            full_path: nf.clone(),
            media_type: "application/x-dtbncx+xml".to_string(),
            properties: None,
        });
        (nid, nf)
    };
    let ncx_dir = Path::new(&ncx_full_path)
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let ncx_xml = render_toc_ncx(&info.unique_id, &meta.title, &toc_triples, ncx_dir);
    replacements.push((ncx_full_path, ncx_xml.into_bytes()));

    let decoded_doc_sources = if all_doc_sources.contains('%') {
        Some(percent_decode_path(&all_doc_sources))
    } else {
        None
    };
    let mut unused_manifest_paths: HashSet<String> = HashSet::new();
    let mut merged_manifest: Vec<ManifestNode> =
        Vec::with_capacity(info.manifest.len() + extra_manifest_nodes.len());
    for m in info.manifest {
        let is_cover_img = cover_img_id.as_deref() == Some(m.id.as_str())
            || m.id.to_ascii_lowercase().contains("cover")
            || m.href.to_ascii_lowercase().contains("cover");
        if m.media_type.starts_with("image/") && !is_cover_img {
            let basename = Path::new(&m.full_path)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            let decoded_basename = percent_decode_path(basename);
            let is_referenced = basename.is_empty()
                || all_doc_sources.contains(basename)
                || all_doc_sources.contains(&decoded_basename)
                || decoded_doc_sources
                    .as_deref()
                    .is_some_and(|d| d.contains(&decoded_basename));
            if !is_referenced {
                unused_manifest_paths.insert(m.full_path.clone());
                let decoded_full = percent_decode_path(&m.full_path);
                if decoded_full != m.full_path {
                    unused_manifest_paths.insert(decoded_full);
                }
                continue;
            }
        }
        merged_manifest.push(m);
    }
    merged_manifest.extend(extra_manifest_nodes);

    let mut valid_zip_paths: HashSet<String> = HashSet::new();
    valid_zip_paths.insert("mimetype".to_string());
    valid_zip_paths.insert(info.opf_path.clone());
    for m in &merged_manifest {
        valid_zip_paths.insert(m.full_path.clone());
        if m.full_path.contains('%') {
            valid_zip_paths.insert(percent_decode_path(&m.full_path));
        }
    }

    let mut orphan_paths: Vec<String> = Vec::new();
    for zpath in all_zip_files {
        if zpath.starts_with("META-INF/") || zpath.eq_ignore_ascii_case("mimetype") {
            continue;
        }
        if !valid_zip_paths.contains(&zpath) || unused_manifest_paths.contains(&zpath) {
            orphan_paths.push(zpath);
        }
    }

    let updated_opf = rebuild_opf_manifest_and_spine(
        &info.raw_opf,
        &merged_manifest,
        &spine_ids,
        &ncx_id,
        info.spine_page_progression.as_deref(),
        &meta,
        &info.unique_id,
        &info.package_version,
    );
    replacements.push((info.opf_path, updated_opf.into_bytes()));

    let removed_orphan_files = orphan_paths.len();
    rewrite_epub_entries(epub_path, replacements, &orphan_paths)?;

    Ok(PolishBookReportDto {
        jacket_updated: true,
        removed_orphan_files,
    })
}

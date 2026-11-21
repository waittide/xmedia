mod package;
mod polish;

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use quick_xml::escape::escape;
use xmedia_core::domain::error::{DomainError, DomainResult};
use zip::ZipArchive;

use crate::infra::inspectors::book::{normalize_zip_rel, read_book_meta_any, rewrite_epub_entries};
use crate::infra::inspectors::zh_text::rewrite_css_vertical_to_horizontal;

use super::epub_io::{
    ManifestNode, is_cover_wrapper_doc, is_nav_manifest_node, parse_epub_structure,
    parse_nav_toc_map, read_zip_bytes, rebuild_opf_manifest_and_spine, relative_href_from_dir,
    render_nav_xhtml, render_toc_ncx,
};
use super::xhtml::{
    plain_to_xhtml_body, sanitize_xhtml_fragment, split_xhtml_head_and_body,
    wrap_full_xhtml_document, xhtml_body_to_plain,
};
use super::{ChapterEditMode, DEFAULT_EPUB_CSS, SaveBookChapterInput};

pub use self::package::convert_txt_to_epub;
pub(crate) use self::package::{
    FreshEpubBuildOptions, FreshEpubChapter, FreshEpubImage, write_fresh_epub_package,
};
pub use self::polish::{insert_epub_image, polish_epub_book};

pub(super) fn save_epub_content(
    epub_path: &Path,
    chapters: &[SaveBookChapterInput],
    stylesheets: Option<&[String]>,
    force_horizontal_spine: bool,
) -> DomainResult<()> {
    let meta = read_book_meta_any(epub_path)?;
    let lang = meta.language.as_deref().unwrap_or("zh-CN");

    let f = File::open(epub_path)
        .map_err(|e| DomainError::Storage(format!("failed opening epub: {e}")))?;
    let mut archive = ZipArchive::new(BufReader::new(f))
        .map_err(|e| DomainError::Validation(format!("invalid epub archive: {e}")))?;
    let info = parse_epub_structure(&mut archive)?;

    let mut old_toc_map: HashMap<String, (String, u8)> = HashMap::new();
    if let Some(nav_node) = info.manifest.iter().find(|m| is_nav_manifest_node(m))
        && let Some(nav_bytes) = read_zip_bytes(&mut archive, &nav_node.full_path)
    {
        let nav_dir = Path::new(&nav_node.full_path)
            .parent()
            .unwrap_or_else(|| Path::new(""));
        old_toc_map = parse_nav_toc_map(&nav_bytes, nav_dir);
    }

    let manifest_by_id: HashMap<&str, &ManifestNode> =
        info.manifest.iter().map(|m| (m.id.as_str(), m)).collect();
    let manifest_by_full: HashMap<&str, &ManifestNode> = info
        .manifest
        .iter()
        .map(|m| (m.full_path.as_str(), m))
        .collect();

    let mut old_editable_paths: HashSet<String> = HashSet::new();
    let mut preserved_cover_spine_id: Option<String> = None;

    for (pos, idref) in info.spine_idrefs.iter().enumerate() {
        let Some(node) = manifest_by_id.get(idref.as_str()).copied() else {
            continue;
        };
        if is_nav_manifest_node(node) {
            continue;
        }
        if !node.media_type.contains("html") && !node.media_type.contains("xml") {
            continue;
        }
        if let Some(bytes) = read_zip_bytes(&mut archive, &node.full_path) {
            let full_str = String::from_utf8_lossy(&bytes);
            let (_, body_inner) = split_xhtml_head_and_body(&full_str);
            let plain = xhtml_body_to_plain(&body_inner, None);
            if pos == 0 && is_cover_wrapper_doc(node, &old_toc_map, &plain, &body_inner) {
                preserved_cover_spine_id = Some(node.id.clone());
                continue;
            }
        }
        old_editable_paths.insert(node.full_path.clone());
    }

    let existing_css_nodes: Vec<&ManifestNode> = info
        .manifest
        .iter()
        .filter(|m| m.media_type.eq_ignore_ascii_case("text/css"))
        .collect();
    let had_no_existing_css = existing_css_nodes.is_empty();

    let mut replacements: Vec<(String, Vec<u8>)> = Vec::new();
    let mut extra_manifest_nodes: Vec<ManifestNode> = Vec::new();

    let effective_css_full = if let Some(first_css) = existing_css_nodes.first() {
        if let Some(edited_list) = stylesheets {
            for (idx, css_node) in existing_css_nodes.iter().enumerate() {
                if let Some(raw_css) = edited_list.get(idx) {
                    let final_css = if force_horizontal_spine {
                        rewrite_css_vertical_to_horizontal(raw_css)
                    } else {
                        raw_css.clone()
                    };
                    replacements.push((css_node.full_path.clone(), final_css.into_bytes()));
                } else if force_horizontal_spine
                    && let Some(orig_bytes) = read_zip_bytes(&mut archive, &css_node.full_path)
                {
                    let orig_str = String::from_utf8_lossy(&orig_bytes);
                    let rewritten = rewrite_css_vertical_to_horizontal(&orig_str);
                    replacements.push((css_node.full_path.clone(), rewritten.into_bytes()));
                }
            }
        } else if force_horizontal_spine {
            for css_node in &existing_css_nodes {
                if let Some(orig_bytes) = read_zip_bytes(&mut archive, &css_node.full_path) {
                    let orig_str = String::from_utf8_lossy(&orig_bytes);
                    let rewritten = rewrite_css_vertical_to_horizontal(&orig_str);
                    replacements.push((css_node.full_path.clone(), rewritten.into_bytes()));
                }
            }
        }
        first_css.full_path.clone()
    } else {
        let css_full = normalize_zip_rel(&info.opf_dir, "Styles/xmedia_style.css");
        let css_href = relative_href_from_dir(&info.opf_dir, &css_full);
        let initial_css = stylesheets
            .and_then(|s| s.first())
            .map(String::as_str)
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(DEFAULT_EPUB_CSS);
        let final_css = if force_horizontal_spine {
            rewrite_css_vertical_to_horizontal(initial_css)
        } else {
            initial_css.to_string()
        };
        replacements.push((css_full.clone(), final_css.into_bytes()));
        extra_manifest_nodes.push(ManifestNode {
            id: "xmedia-default-css".to_string(),
            href: css_href,
            full_path: css_full.clone(),
            media_type: "text/css".to_string(),
            properties: None,
        });
        css_full
    };

    let mut used_source_paths: HashSet<String> = HashSet::new();
    let mut used_ids: HashSet<String> = info.manifest.iter().map(|m| m.id.clone()).collect();
    let mut final_chapter_items: Vec<(String, String, u8, String)> =
        Vec::with_capacity(chapters.len());

    for (idx, ch) in chapters.iter().enumerate() {
        let title = if ch.title.trim().is_empty() {
            format!("第 {} 章", idx + 1)
        } else {
            ch.title.trim().to_string()
        };
        let level = ch.level.min(1);

        let reusable_source = ch
            .source_href
            .as_deref()
            .filter(|p| old_editable_paths.contains(*p) && !used_source_paths.contains(*p))
            .map(str::to_string);

        let (target_full_path, manifest_id, preserved_head) =
            if let Some(src_full) = reusable_source {
                used_source_paths.insert(src_full.clone());
                let head = read_zip_bytes(&mut archive, &src_full).and_then(|b| {
                    let s = String::from_utf8_lossy(&b);
                    split_xhtml_head_and_body(&s).0
                });
                let id = manifest_by_full
                    .get(src_full.as_str())
                    .map(|m| m.id.clone())
                    .unwrap_or_else(|| format!("ch-{}", idx + 1));
                (src_full, id, head)
            } else {
                let mut seq = idx + 1;
                let (new_full, new_id) = loop {
                    let cand_rel = format!("Text/chapter_edit_{seq:04}.xhtml");
                    let cand_full = normalize_zip_rel(&info.opf_dir, &cand_rel);
                    let cand_id = format!("xch-{seq:04}");
                    if !used_source_paths.contains(&cand_full)
                        && !manifest_by_full.contains_key(cand_full.as_str())
                        && !used_ids.contains(&cand_id)
                    {
                        used_ids.insert(cand_id.clone());
                        used_source_paths.insert(cand_full.clone());
                        break (cand_full, cand_id);
                    }
                    seq += 1;
                };
                let href = relative_href_from_dir(&info.opf_dir, &new_full);
                extra_manifest_nodes.push(ManifestNode {
                    id: new_id.clone(),
                    href,
                    full_path: new_full.clone(),
                    media_type: "application/xhtml+xml".to_string(),
                    properties: None,
                });
                (new_full, new_id, None)
            };

        let ch_dir = Path::new(&target_full_path)
            .parent()
            .unwrap_or_else(|| Path::new(""));
        let css_rel = relative_href_from_dir(ch_dir, &effective_css_full);

        let body_inner = match ch.mode {
            ChapterEditMode::Plain => plain_to_xhtml_body(&title, &ch.plain_text),
            ChapterEditMode::Xhtml => {
                if ch.xhtml_body.trim().is_empty() {
                    plain_to_xhtml_body(&title, &ch.plain_text)
                } else {
                    sanitize_xhtml_fragment(&ch.xhtml_body)
                }
            }
        };

        let effective_head = preserved_head.map(|mut h| {
            if force_horizontal_spine {
                h = rewrite_css_vertical_to_horizontal(&h);
            }
            if had_no_existing_css
                && stylesheets.is_some_and(|s| s.first().is_some_and(|c| !c.trim().is_empty()))
                && !h.to_ascii_lowercase().contains("stylesheet")
                && let Some(pos) = h.to_ascii_lowercase().rfind("</head>")
            {
                let link_tag = format!(
                    "  <link rel=\"stylesheet\" type=\"text/css\" href=\"{}\"/>\n",
                    escape(&css_rel)
                );
                h.insert_str(pos, &link_tag);
            }
            h
        });

        let full_doc = wrap_full_xhtml_document(
            &title,
            lang,
            effective_head.as_deref(),
            Some(&css_rel),
            &body_inner,
        );
        replacements.push((target_full_path.clone(), full_doc.into_bytes()));
        final_chapter_items.push((title, target_full_path, level, manifest_id));
    }
    drop(archive);

    let removed_paths: Vec<String> = old_editable_paths
        .difference(&used_source_paths)
        .cloned()
        .collect();
    let removed_set: HashSet<&str> = removed_paths.iter().map(String::as_str).collect();

    let toc_triples: Vec<(String, String, u8)> = final_chapter_items
        .iter()
        .map(|(t, p, l, _)| (t.clone(), p.clone(), *l))
        .collect();

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

    let mut merged_manifest: Vec<ManifestNode> = info
        .manifest
        .into_iter()
        .filter(|m| !removed_set.contains(m.full_path.as_str()))
        .collect();
    merged_manifest.extend(extra_manifest_nodes);

    let mut spine_ids = Vec::with_capacity(final_chapter_items.len() + 1);
    if let Some(cov_id) = preserved_cover_spine_id {
        spine_ids.push(cov_id);
    }
    for (_, _, _, mid) in &final_chapter_items {
        spine_ids.push(mid.clone());
    }

    let effective_spine_dir = if force_horizontal_spine {
        None
    } else {
        info.spine_page_progression.as_deref()
    };

    let updated_opf = rebuild_opf_manifest_and_spine(
        &info.raw_opf,
        &merged_manifest,
        &spine_ids,
        &ncx_id,
        effective_spine_dir,
        &meta,
        &info.unique_id,
        &info.package_version,
    );
    replacements.push((info.opf_path, updated_opf.into_bytes()));

    rewrite_epub_entries(epub_path, replacements, &removed_paths)
}

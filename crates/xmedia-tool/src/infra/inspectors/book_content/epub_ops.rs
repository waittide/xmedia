use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use quick_xml::escape::escape;
use regex::Regex;
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::infra::stream::decode_text_smart;
use xmedia_core::infra::thumbnail::sniff_image_mime;
use zip::{ZipArchive, ZipWriter};

use crate::domain::media::BookFileMeta;
use crate::infra::atomic_fs::atomic_write_via_temp;
use crate::infra::inspectors::book::{
    find_sidecar_cover, normalize_zip_rel, read_book_meta_any, replace_opf_metadata_block,
    rewrite_epub_entries,
};
use crate::infra::inspectors::zh_text::rewrite_css_vertical_to_horizontal;

use super::epub_io::{
    ManifestNode, epub_deflated_zip_options, epub_entry_zip_options, epub_stored_zip_options,
    ext_for_image_mime, find_cover_manifest_node, is_cover_wrapper_doc, is_nav_manifest_node,
    parse_epub_structure, parse_nav_toc_map, percent_decode_path, read_epub_toc_map,
    read_zip_bytes, rebuild_opf_manifest_and_spine, relative_href_from_dir, render_nav_xhtml,
    render_toc_ncx, write_zip_entry,
};
use super::txt::segment_txt_chapters;
use super::xhtml::{
    extract_heading_from_xhtml, plain_to_xhtml_body, render_book_jacket_xhtml,
    sanitize_xhtml_fragment, split_xhtml_head_and_body, wrap_full_xhtml_document,
    xhtml_body_to_plain,
};
use super::{
    ChapterEditMode, DEFAULT_EPUB_CSS, InsertedEpubImageDto, PolishBookReportDto,
    SaveBookChapterInput,
};

pub(crate) struct FreshEpubChapter {
    pub title: String,
    pub level: u8,
    pub body_xhtml: String,
}

pub(crate) struct FreshEpubImage {
    pub rel_href: String,
    pub media_type: &'static str,
    pub bytes: Vec<u8>,
}

pub(crate) struct FreshEpubBuildOptions<'a> {
    pub css: &'a str,
    pub generate_jacket: bool,
    pub mark_normalized: bool,
    pub best_compression: bool,
    pub cover_wrapper_class: &'a str,
    pub cover_alt: &'a str,
}

const DEFAULT_CONTAINER_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>
"#;

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

    // Build existing TOC map solely to detect preserved leading cover wrapper in spine
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

    // Identify old editable chapter paths & optional preserved leading cover spine id
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

    // Locate or create stylesheet for newly inserted chapters & persist edited CSS stylesheets
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
        Vec::with_capacity(chapters.len()); // (title, full_path, level, manifest_id)

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

    // Deleted chapter files to strip from zip & manifest
    let removed_paths: Vec<String> = old_editable_paths
        .difference(&used_source_paths)
        .cloned()
        .collect();
    let removed_set: HashSet<&str> = removed_paths.iter().map(String::as_str).collect();

    // Ensure nav.xhtml and toc.ncx exist in manifest & replacements
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

    // Rebuild <manifest> and <spine> inside OPF while preserving <metadata> and package version
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

pub(crate) fn write_fresh_epub_package(
    epub_path: &Path,
    meta: &BookFileMeta,
    chapters: &[FreshEpubChapter],
    extra_images: &[FreshEpubImage],
    cover: Option<&(Vec<u8>, &'static str, &'static str)>,
    opts: &FreshEpubBuildOptions<'_>,
) -> DomainResult<()> {
    let lang = meta.language.as_deref().unwrap_or("zh-CN");
    let book_uuid = format!("urn:uuid:{}", Uuid::now_v7());

    atomic_write_via_temp(epub_path, |staging| {
        let out_file = File::create(staging)
            .map_err(|e| DomainError::Storage(format!("failed creating epub staging: {e}")))?;
        let mut writer = ZipWriter::new(BufWriter::new(out_file));

        let stored = epub_stored_zip_options();
        let deflated = epub_deflated_zip_options(opts.best_compression);

        // 1. OCF `mimetype` uncompressed first entry
        write_zip_entry(
            &mut writer,
            "mimetype",
            stored,
            b"application/epub+zip",
            "mimetype",
        )?;

        // 2. META-INF/container.xml
        write_zip_entry(
            &mut writer,
            "META-INF/container.xml",
            deflated,
            DEFAULT_CONTAINER_XML.as_bytes(),
            "container",
        )?;

        // 3. Stylesheet
        write_zip_entry(
            &mut writer,
            "OEBPS/Styles/style.css",
            deflated,
            opts.css.as_bytes(),
            "css",
        )?;

        let mut manifest_nodes = vec![
            ManifestNode {
                id: "style-css".to_string(),
                href: "Styles/style.css".to_string(),
                full_path: "OEBPS/Styles/style.css".to_string(),
                media_type: "text/css".to_string(),
                properties: None,
            },
            ManifestNode {
                id: "nav".to_string(),
                href: "nav.xhtml".to_string(),
                full_path: "OEBPS/nav.xhtml".to_string(),
                media_type: "application/xhtml+xml".to_string(),
                properties: Some("nav".to_string()),
            },
            ManifestNode {
                id: "ncx".to_string(),
                href: "toc.ncx".to_string(),
                full_path: "OEBPS/toc.ncx".to_string(),
                media_type: "application/x-dtbncx+xml".to_string(),
                properties: None,
            },
        ];
        let mut spine_ids = Vec::new();
        let mut toc_triples: Vec<(String, String, u8)> = Vec::with_capacity(chapters.len() + 1);

        // 4. Optional Cover Image & Cover Page
        let mut cover_item_id: Option<&str> = None;
        let mut cover_rel_for_jacket: Option<String> = None;
        if let Some((cov_bytes, cov_mime, cov_ext)) = cover {
            let img_rel = format!("Images/cover.{cov_ext}");
            let img_full = format!("OEBPS/{img_rel}");
            write_zip_entry(
                &mut writer,
                &img_full,
                epub_entry_zip_options(&img_full, cov_mime, opts.best_compression),
                cov_bytes,
                "cover image",
            )?;

            let cover_xhtml = wrap_full_xhtml_document(
                "封面",
                lang,
                None,
                Some("../Styles/style.css"),
                &format!(
                    "  <div class=\"{}\"><img src=\"../{img_rel}\" alt=\"{}\"/></div>",
                    opts.cover_wrapper_class,
                    escape(opts.cover_alt)
                ),
            );
            write_zip_entry(
                &mut writer,
                "OEBPS/Text/cover.xhtml",
                deflated,
                cover_xhtml.as_bytes(),
                "cover xhtml",
            )?;

            manifest_nodes.push(ManifestNode {
                id: "cover-image".to_string(),
                href: img_rel.clone(),
                full_path: img_full,
                media_type: (*cov_mime).to_string(),
                properties: Some("cover-image".to_string()),
            });
            manifest_nodes.push(ManifestNode {
                id: "cover-page".to_string(),
                href: "Text/cover.xhtml".to_string(),
                full_path: "OEBPS/Text/cover.xhtml".to_string(),
                media_type: "application/xhtml+xml".to_string(),
                properties: None,
            });
            spine_ids.push("cover-page".to_string());
            cover_item_id = Some("cover-image");
            cover_rel_for_jacket = Some(format!("../{img_rel}"));
        }

        // 5. Optional Jacket Metadata Page
        if opts.generate_jacket {
            let jacket_xhtml = render_book_jacket_xhtml(
                meta,
                cover_rel_for_jacket.as_deref(),
                Some("../Styles/style.css"),
            );
            write_zip_entry(
                &mut writer,
                "OEBPS/Text/xmedia_jacket.xhtml",
                deflated,
                jacket_xhtml.as_bytes(),
                "jacket",
            )?;

            manifest_nodes.push(ManifestNode {
                id: "xmedia-jacket".to_string(),
                href: "Text/xmedia_jacket.xhtml".to_string(),
                full_path: "OEBPS/Text/xmedia_jacket.xhtml".to_string(),
                media_type: "application/xhtml+xml".to_string(),
                properties: None,
            });
            spine_ids.push("xmedia-jacket".to_string());
            toc_triples.push((
                "书籍简介".to_string(),
                "OEBPS/Text/xmedia_jacket.xhtml".to_string(),
                0,
            ));
        }

        // 6. Optional Extra Images (e.g. PDF extracted page images)
        for (idx, asset) in extra_images.iter().enumerate() {
            let full_path = format!("OEBPS/{}", asset.rel_href);
            write_zip_entry(
                &mut writer,
                &full_path,
                epub_entry_zip_options(&full_path, asset.media_type, opts.best_compression),
                &asset.bytes,
                "image",
            )?;
            manifest_nodes.push(ManifestNode {
                id: format!("img-{:04}", idx + 1),
                href: asset.rel_href.clone(),
                full_path,
                media_type: asset.media_type.to_string(),
                properties: None,
            });
        }

        // 7. Chapters
        for (idx, ch) in chapters.iter().enumerate() {
            let seq = idx + 1;
            let title = if ch.title.trim().is_empty() {
                format!("第 {seq} 章")
            } else {
                ch.title.trim().to_string()
            };
            let level = ch.level.min(1);
            let rel_href = format!("Text/chapter_{seq:04}.xhtml");
            let full_path = format!("OEBPS/{rel_href}");
            let item_id = format!("ch-{seq:04}");

            let doc = wrap_full_xhtml_document(
                &title,
                lang,
                None,
                Some("../Styles/style.css"),
                &ch.body_xhtml,
            );
            write_zip_entry(&mut writer, &full_path, deflated, doc.as_bytes(), "chapter")?;

            manifest_nodes.push(ManifestNode {
                id: item_id.clone(),
                href: rel_href,
                full_path: full_path.clone(),
                media_type: "application/xhtml+xml".to_string(),
                properties: None,
            });
            spine_ids.push(item_id);
            toc_triples.push((title, full_path, level));
        }

        // 8. nav.xhtml + toc.ncx
        let oebps_dir = Path::new("OEBPS");
        let nav_xml = render_nav_xhtml(&meta.title, lang, &toc_triples, oebps_dir);
        write_zip_entry(
            &mut writer,
            "OEBPS/nav.xhtml",
            deflated,
            nav_xml.as_bytes(),
            "nav",
        )?;

        let ncx_xml = render_toc_ncx(&book_uuid, &meta.title, &toc_triples, oebps_dir);
        write_zip_entry(
            &mut writer,
            "OEBPS/toc.ncx",
            deflated,
            ncx_xml.as_bytes(),
            "ncx",
        )?;

        // 9. OEBPS/content.opf
        let base_opf = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"bookid\">\n  <metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n    <dc:identifier id=\"bookid\">{}</dc:identifier>\n  </metadata>\n  <manifest></manifest>\n  <spine toc=\"ncx\"></spine>\n</package>\n",
            escape(&book_uuid)
        );
        let mut effective_meta = meta.clone();
        if opts.mark_normalized {
            effective_meta.is_normalized = true;
        }
        let with_meta =
            replace_opf_metadata_block(&base_opf, &effective_meta, cover_item_id, "bookid", "3.0");
        let final_opf = rebuild_opf_manifest_and_spine(
            &with_meta,
            &manifest_nodes,
            &spine_ids,
            "ncx",
            None,
            &effective_meta,
            "bookid",
            "3.0",
        );

        write_zip_entry(
            &mut writer,
            "OEBPS/content.opf",
            deflated,
            final_opf.as_bytes(),
            "opf",
        )?;

        writer
            .finish()
            .map_err(|e| DomainError::Storage(format!("epub zip finalize error: {e}")))?;
        Ok(())
    })
}

pub fn convert_txt_to_epub(
    txt_path: &Path,
    chapters_override: Option<&[SaveBookChapterInput]>,
    stylesheets_override: Option<&[String]>,
    delete_source_txt: bool,
) -> DomainResult<PathBuf> {
    let ext = txt_path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if ext != "txt" {
        return Err(DomainError::Validation(
            "only TXT files can be converted to EPUB".into(),
        ));
    }

    let epub_path = txt_path.with_extension("epub");
    if epub_path.exists() {
        return Err(DomainError::Conflict(format!(
            "目标文件 '{}' 已存在，请先重命名或移除已有同名 EPUB",
            epub_path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        )));
    }

    let meta = read_book_meta_any(txt_path)?;

    let resolved_chapters: Vec<SaveBookChapterInput> = if let Some(list) = chapters_override {
        if list.is_empty() {
            return Err(DomainError::Validation(
                "至少需要包含一个章节才能生成 EPUB".into(),
            ));
        }
        list.to_vec()
    } else {
        let bytes = std::fs::read(txt_path)
            .map_err(|e| DomainError::Storage(format!("failed reading txt: {e}")))?;
        let (decoded, _) = decode_text_smart(&bytes);
        segment_txt_chapters(&decoded, None, None)?
            .into_iter()
            .map(|c| SaveBookChapterInput {
                title: c.title,
                level: c.level,
                mode: ChapterEditMode::Plain,
                plain_text: c.plain_text,
                xhtml_body: c.xhtml_body,
                source_href: None,
            })
            .collect()
    };

    let cover_payload: Option<(Vec<u8>, &'static str, &'static str)> = find_sidecar_cover(txt_path)
        .and_then(|cov_path| {
            let bytes = std::fs::read(cov_path).ok()?;
            if bytes.is_empty() {
                return None;
            }
            let mime = sniff_image_mime(&bytes);
            Some((bytes, mime, ext_for_image_mime(mime)))
        });

    let chosen_css = stylesheets_override
        .and_then(|s| s.first())
        .map(String::as_str)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(DEFAULT_EPUB_CSS);

    let fresh_chapters: Vec<FreshEpubChapter> = resolved_chapters
        .iter()
        .enumerate()
        .map(|(idx, ch)| {
            let seq = idx + 1;
            let title = if ch.title.trim().is_empty() {
                format!("第 {seq} 章")
            } else {
                ch.title.trim().to_string()
            };
            let body_xhtml = match ch.mode {
                ChapterEditMode::Plain => plain_to_xhtml_body(&title, &ch.plain_text),
                ChapterEditMode::Xhtml => {
                    if ch.xhtml_body.trim().is_empty() {
                        plain_to_xhtml_body(&title, &ch.plain_text)
                    } else {
                        sanitize_xhtml_fragment(&ch.xhtml_body)
                    }
                }
            };
            FreshEpubChapter {
                title,
                level: ch.level,
                body_xhtml,
            }
        })
        .collect();

    write_fresh_epub_package(
        &epub_path,
        &meta,
        &fresh_chapters,
        &[],
        cover_payload.as_ref(),
        &FreshEpubBuildOptions {
            css: chosen_css,
            generate_jacket: false,
            mark_normalized: false,
            best_compression: false,
            cover_wrapper_class: "cover-wrap",
            cover_alt: "Cover",
        },
    )?;

    if delete_source_txt {
        let _ = std::fs::remove_file(txt_path);
    }

    Ok(epub_path)
}

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

    // Existing TOC map for preserving TOC hierarchy & titles
    let toc_map = read_epub_toc_map(&mut archive, &info);

    // Locate cover image in manifest if available
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

    // Create or update xmedia_jacket.xhtml
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

    // Determine whether spine[0] is a cover wrapper page
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

    // Build normalized spine_ids with jacket right after cover wrapper (or at index 0)
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

    // Build synchronized TOC triples for nav.xhtml and toc.ncx
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

    // Collect all text/xhtml/svg/css content to detect unreferenced non-cover images
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

    // Identify all entries currently in the ZIP archive
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

    // Ensure nav.xhtml and toc.ncx exist and are updated
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

    // Prune unused non-cover images from manifest (accounting for percent-encoded vs UTF-8 hrefs)
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

    // Compute orphan files in ZIP archive (unmanifested junk + pruned unused images)
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

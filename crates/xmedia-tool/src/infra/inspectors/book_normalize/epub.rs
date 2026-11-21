use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufReader, BufWriter, Read};
use std::path::Path;

use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use zip::{ZipArchive, ZipWriter};

use super::image_compress::{compress_image_bytes, is_raster_compressible_mime};
use super::{
    NormalizeBookOutcome, NormalizeBookToEpubOptions, is_orig_epub_backup_path,
    unique_orig_epub_backup_path,
};
use crate::infra::atomic_fs::atomic_write_via_temp;
use crate::infra::inspectors::book::{normalize_zip_rel, read_book_meta_any};
use crate::infra::inspectors::book_content::{
    ManifestNode, epub_entry_zip_options, epub_stored_zip_options, extract_heading_from_xhtml,
    find_cover_manifest_node, is_nav_manifest_node, parse_epub_structure, percent_decode_path,
    read_epub_toc_map, rebuild_opf_manifest_and_spine, relative_href_from_dir,
    render_book_jacket_xhtml, render_nav_xhtml, render_toc_ncx, write_zip_entry,
};

fn is_os_junk_entry(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.starts_with("__macosx/")
        || lower.ends_with("/.ds_store")
        || lower == ".ds_store"
        || lower.ends_with("/thumbs.db")
        || lower == "thumbs.db"
        || lower.ends_with("/desktop.ini")
        || lower == "desktop.ini"
}

fn rewrite_image_refs_in_text(
    doc_full_path: &str,
    content: &str,
    renames: &[(String, String)],
) -> String {
    if renames.is_empty() {
        return content.to_string();
    }
    let doc_dir = Path::new(doc_full_path)
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let mut updated = content.to_string();
    for (old_full, new_full) in renames {
        let old_rel = relative_href_from_dir(doc_dir, old_full);
        let new_rel = relative_href_from_dir(doc_dir, new_full);
        if old_rel != new_rel && updated.contains(&old_rel) {
            updated = updated.replace(&old_rel, &new_rel);
        }
        if old_full != new_full && updated.contains(old_full.as_str()) {
            updated = updated.replace(old_full.as_str(), new_full.as_str());
        }
        // Handle percent-decoded relative paths if source used raw Unicode or encoded paths
        let old_rel_dec = percent_decode_path(&old_rel);
        let new_rel_dec = percent_decode_path(&new_rel);
        if old_rel_dec != old_rel && old_rel_dec != new_rel_dec && updated.contains(&old_rel_dec) {
            updated = updated.replace(&old_rel_dec, &new_rel_dec);
        }
    }
    updated
}

fn replace_path_extension(path: &str, new_ext: &str) -> String {
    if let Some(dot) = path.rfind('.') {
        let slash = path.rfind('/').unwrap_or(0);
        if dot > slash {
            return format!("{}.{new_ext}", &path[..dot]);
        }
    }
    format!("{path}.{new_ext}")
}

pub(super) fn normalize_existing_epub(
    epub_path: &Path,
    opts: &NormalizeBookToEpubOptions,
) -> DomainResult<NormalizeBookOutcome> {
    let before_bytes = std::fs::metadata(epub_path).map(|m| m.len()).unwrap_or(0);

    if opts.skip_normalized && is_orig_epub_backup_path(epub_path) {
        return Ok(NormalizeBookOutcome {
            output_path: epub_path.to_path_buf(),
            backup_path: None,
            source_format: "epub".to_string(),
            before_bytes,
            after_bytes: before_bytes,
            chapter_count: 0,
            compressed_images: 0,
            removed_orphans: 0,
            deleted_source: false,
            skipped: true,
            skip_reason: Some("原件备份文件 (.orig.epub)".to_string()),
        });
    }

    let mut meta = read_book_meta_any(epub_path)?;
    if opts.skip_normalized && meta.is_normalized {
        return Ok(NormalizeBookOutcome {
            output_path: epub_path.to_path_buf(),
            backup_path: None,
            source_format: "epub".to_string(),
            before_bytes,
            after_bytes: before_bytes,
            chapter_count: 0,
            compressed_images: 0,
            removed_orphans: 0,
            deleted_source: false,
            skipped: true,
            skip_reason: Some("已是规范化 EPUB 3.0".to_string()),
        });
    }
    meta.is_normalized = true;
    let lang = meta.language.as_deref().unwrap_or("zh-CN");

    let f =
        File::open(epub_path).map_err(|e| DomainError::Storage(format!("打开 EPUB 失败: {e}")))?;
    let mut archive = ZipArchive::new(BufReader::new(f))
        .map_err(|e| DomainError::Validation(format!("无效的 EPUB 压缩包: {e}")))?;
    let info = parse_epub_structure(&mut archive)?;

    // Existing TOC map
    let toc_map = read_epub_toc_map(&mut archive, &info);

    // Locate cover image node in manifest
    let cover_img_id = find_cover_manifest_node(&info).map(|m| m.id.clone());

    let primary_css_full = info
        .manifest
        .iter()
        .find(|m| m.media_type.eq_ignore_ascii_case("text/css"))
        .map(|m| m.full_path.clone());

    // Read all existing archive files into memory (skipping OS junk)
    const MAX_EPUB_ENTRY_BYTES: u64 = 64 * 1024 * 1024;
    const MAX_EPUB_TOTAL_BYTES: u64 = 512 * 1024 * 1024;
    let mut raw_entries: HashMap<String, Vec<u8>> = HashMap::new();
    let mut entry_order: Vec<String> = Vec::new();
    let mut junk_removed = 0usize;
    let mut total_bytes_read: u64 = 0;
    for i in 0..archive.len() {
        let Ok(zf) = archive.by_index(i) else {
            continue;
        };
        let Ok(name_ref) = zf.name() else {
            continue;
        };
        let name = name_ref.into_owned();
        if name.ends_with('/') {
            continue;
        }
        if opts.remove_orphans && is_os_junk_entry(&name) {
            junk_removed += 1;
            continue;
        }
        let prealloc = zf.size().min(1024 * 1024) as usize;
        let mut buf = Vec::with_capacity(prealloc);
        if zf
            .take(MAX_EPUB_ENTRY_BYTES + 1)
            .read_to_end(&mut buf)
            .is_ok()
        {
            if (buf.len() as u64) > MAX_EPUB_ENTRY_BYTES {
                return Err(DomainError::Validation(format!(
                    "EPUB entry '{name}' exceeds 64 MiB safety limit"
                )));
            }
            total_bytes_read = total_bytes_read.saturating_add(buf.len() as u64);
            if total_bytes_read > MAX_EPUB_TOTAL_BYTES {
                return Err(DomainError::Validation(
                    "decompressed EPUB archive exceeds 512 MiB safety limit".into(),
                ));
            }
            entry_order.push(name.clone());
            raw_entries.insert(name, buf);
        }
    }
    drop(archive);

    // Helper to look up bytes by manifest full_path (with percent-decode fallback)
    let get_entry_bytes =
        |entries: &HashMap<String, Vec<u8>>, full_path: &str| -> Option<(String, Vec<u8>)> {
            if let Some(b) = entries.get(full_path) {
                return Some((full_path.to_string(), b.clone()));
            }
            if full_path.contains('%') {
                let dec = percent_decode_path(full_path);
                if let Some(b) = entries.get(&dec) {
                    return Some((dec, b.clone()));
                }
            }
            None
        };

    // Prune unreferenced manifest images BEFORE compressing so we don't waste CPU on orphans
    let mut removed_orphans = junk_removed;
    let initial_manifest: Vec<ManifestNode> = if opts.remove_orphans {
        let mut all_doc_sources = String::new();
        for m in &info.manifest {
            let mt = m.media_type.to_ascii_lowercase();
            if (mt.contains("html")
                || mt.contains("xml")
                || mt.contains("css")
                || mt.contains("svg"))
                && let Some((_, bytes)) = get_entry_bytes(&raw_entries, &m.full_path)
            {
                all_doc_sources.push_str(&String::from_utf8_lossy(&bytes));
                all_doc_sources.push('\n');
            }
        }
        let all_doc_decoded = percent_decode_path(&all_doc_sources);

        let mut kept = Vec::with_capacity(info.manifest.len());
        for m in info.manifest.clone() {
            if m.media_type.starts_with("image/") && cover_img_id.as_deref() != Some(m.id.as_str())
            {
                let fname = Path::new(&m.full_path)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default();
                let fname_dec = percent_decode_path(fname);
                let referenced = !fname.is_empty()
                    && (all_doc_sources.contains(fname)
                        || all_doc_decoded.contains(&fname_dec)
                        || all_doc_sources.contains(&m.href));
                if !referenced {
                    raw_entries.remove(&m.full_path);
                    raw_entries.remove(&percent_decode_path(&m.full_path));
                    removed_orphans += 1;
                    continue;
                }
            }
            kept.push(m);
        }
        kept
    } else {
        info.manifest.clone()
    };

    // Compress raster images in manifest & track renames
    let mut updated_manifest: Vec<ManifestNode> = Vec::with_capacity(initial_manifest.len() + 3);
    let mut image_renames: Vec<(String, String)> = Vec::new();
    let mut renamed_source_keys: HashSet<String> = HashSet::new();
    let mut compressed_images = 0usize;
    let mut used_full_paths: HashSet<String> = initial_manifest
        .iter()
        .map(|m| m.full_path.clone())
        .collect();

    for mut m in initial_manifest {
        if opts.compress_images
            && is_raster_compressible_mime(&m.media_type, &m.full_path)
            && let Some((actual_key, bytes)) = get_entry_bytes(&raw_entries, &m.full_path)
        {
            let comp = compress_image_bytes(bytes, &m.media_type, opts);
            if comp.changed {
                compressed_images += 1;
                let mut new_full = if comp.mime != m.media_type {
                    replace_path_extension(&m.full_path, comp.ext)
                } else {
                    m.full_path.clone()
                };
                if new_full != m.full_path && used_full_paths.contains(&new_full) {
                    // Avoid collision with another existing file
                    new_full = m.full_path.clone();
                }
                if new_full != m.full_path {
                    used_full_paths.insert(new_full.clone());
                    image_renames.push((m.full_path.clone(), new_full.clone()));
                    if actual_key != m.full_path {
                        image_renames.push((actual_key.clone(), new_full.clone()));
                    }
                    renamed_source_keys.insert(actual_key);
                    m.full_path = new_full.clone();
                    m.href = relative_href_from_dir(&info.opf_dir, &new_full);
                    m.media_type = comp.mime.to_string();
                } else {
                    m.media_type = comp.mime.to_string();
                }
                raw_entries.insert(m.full_path.clone(), comp.bytes);
            }
        }
        updated_manifest.push(m);
    }

    for old_key in &renamed_source_keys {
        raw_entries.remove(old_key);
    }

    // Rewrite references inside XHTML / HTML / CSS / SVG files when image extensions changed
    if !image_renames.is_empty() {
        for m in &updated_manifest {
            let mt = m.media_type.to_ascii_lowercase();
            if (mt.contains("html")
                || mt.contains("xml")
                || mt.contains("css")
                || mt.contains("svg"))
                && let Some((actual_key, bytes)) = get_entry_bytes(&raw_entries, &m.full_path)
            {
                let text = String::from_utf8_lossy(&bytes);
                let rewritten = rewrite_image_refs_in_text(&m.full_path, &text, &image_renames);
                if rewritten != text {
                    raw_entries.insert(actual_key, rewritten.into_bytes());
                }
            }
        }
    }

    // Optional jacket page generation/refresh
    let mut spine_ids = info.spine_idrefs.clone();
    let mut jacket_id_opt: Option<String> = None;
    let mut jacket_full_opt: Option<String> = None;

    if opts.generate_jacket {
        let cover_img_full = cover_img_id.as_deref().and_then(|cid| {
            updated_manifest
                .iter()
                .find(|m| m.id == cid)
                .map(|m| m.full_path.clone())
        });

        let (jacket_id, jacket_full_path) = if let Some(existing_jacket) = updated_manifest
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
            updated_manifest.push(ManifestNode {
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
        raw_entries.insert(jacket_full_path.clone(), jacket_xhtml.into_bytes());

        if !spine_ids.iter().any(|id| id == &jacket_id) {
            let insert_pos = if spine_ids
                .first()
                .and_then(|fid| updated_manifest.iter().find(|m| &m.id == fid))
                .is_some_and(|n| {
                    let stem = Path::new(&n.href)
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or_default()
                        .to_ascii_lowercase();
                    matches!(
                        stem.as_str(),
                        "cover" | "coverpage" | "cover-page" | "titlepage" | "title-page"
                    )
                }) {
                1
            } else {
                0
            };
            spine_ids.insert(insert_pos.min(spine_ids.len()), jacket_id.clone());
        }
        jacket_id_opt = Some(jacket_id);
        jacket_full_opt = Some(jacket_full_path);
    }

    // Build TOC triples from spine
    let manifest_by_id: HashMap<String, ManifestNode> = updated_manifest
        .iter()
        .map(|m| (m.id.clone(), m.clone()))
        .collect();

    let mut toc_triples: Vec<(String, String, u8)> = Vec::new();
    if let Some(ref jf) = jacket_full_opt {
        toc_triples.push(("书籍简介".to_string(), jf.clone(), 0));
    }
    let mut chapter_count = 0usize;
    for (idx, idref) in spine_ids.iter().enumerate() {
        if jacket_id_opt.as_deref() == Some(idref.as_str()) {
            continue;
        }
        let Some(node) = manifest_by_id.get(idref) else {
            continue;
        };
        if is_nav_manifest_node(node) {
            continue;
        }
        let hl = node.href.to_ascii_lowercase();
        if idx == 0 && (hl.contains("cover") || hl.ends_with("titlepage.xhtml")) {
            continue;
        }
        chapter_count += 1;
        if let Some((t, lvl)) = toc_map.get(&node.full_path) {
            toc_triples.push((t.clone(), node.full_path.clone(), *lvl));
        } else if let Some((_, bytes)) = get_entry_bytes(&raw_entries, &node.full_path) {
            let s = String::from_utf8_lossy(&bytes);
            let title =
                extract_heading_from_xhtml(&s).unwrap_or_else(|| format!("第 {chapter_count} 章"));
            toc_triples.push((title, node.full_path.clone(), 0));
        }
    }

    // Ensure nav.xhtml & toc.ncx exist and are synced
    let (nav_full_path, _) =
        if let Some(n) = updated_manifest.iter().find(|m| is_nav_manifest_node(m)) {
            (n.full_path.clone(), n.id.clone())
        } else {
            let nf = normalize_zip_rel(&info.opf_dir, "nav.xhtml");
            let nh = relative_href_from_dir(&info.opf_dir, &nf);
            updated_manifest.push(ManifestNode {
                id: "nav".to_string(),
                href: nh,
                full_path: nf.clone(),
                media_type: "application/xhtml+xml".to_string(),
                properties: Some("nav".to_string()),
            });
            (nf, "nav".to_string())
        };
    let nav_dir = Path::new(&nav_full_path)
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let nav_xml = render_nav_xhtml(&meta.title, lang, &toc_triples, nav_dir);
    raw_entries.insert(nav_full_path, nav_xml.into_bytes());

    let (ncx_full_path, ncx_id) = if let Some(n) = updated_manifest.iter().find(|m| {
        info.toc_ncx_attr.as_deref() == Some(m.id.as_str())
            || m.media_type
                .eq_ignore_ascii_case("application/x-dtbncx+xml")
    }) {
        (n.full_path.clone(), n.id.clone())
    } else {
        let nf = normalize_zip_rel(&info.opf_dir, "toc.ncx");
        let nh = relative_href_from_dir(&info.opf_dir, &nf);
        updated_manifest.push(ManifestNode {
            id: "ncx".to_string(),
            href: nh,
            full_path: nf.clone(),
            media_type: "application/x-dtbncx+xml".to_string(),
            properties: None,
        });
        (nf, "ncx".to_string())
    };
    let ncx_dir = Path::new(&ncx_full_path)
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let ncx_xml = render_toc_ncx(&info.unique_id, &meta.title, &toc_triples, ncx_dir);
    raw_entries.insert(ncx_full_path, ncx_xml.into_bytes());

    // Build final normalized EPUB 3.0 OPF
    let updated_opf = rebuild_opf_manifest_and_spine(
        &info.raw_opf,
        &updated_manifest,
        &spine_ids,
        &ncx_id,
        info.spine_page_progression.as_deref(),
        &meta,
        &info.unique_id,
        "3.0",
    );
    raw_entries.insert(info.opf_path.clone(), updated_opf.into_bytes());

    // Determine valid zip paths
    let mut manifest_mime_by_path: HashMap<String, String> = HashMap::new();
    let mut valid_zip_paths: HashSet<String> = HashSet::new();
    valid_zip_paths.insert("mimetype".to_string());
    valid_zip_paths.insert(info.opf_path.clone());
    for m in &updated_manifest {
        valid_zip_paths.insert(m.full_path.clone());
        manifest_mime_by_path.insert(m.full_path.clone(), m.media_type.clone());
        if m.full_path.contains('%') {
            let dec = percent_decode_path(&m.full_path);
            valid_zip_paths.insert(dec.clone());
            manifest_mime_by_path.insert(dec, m.media_type.clone());
        }
    }

    if opts.remove_orphans {
        let extra_unmanifested: Vec<String> = raw_entries
            .keys()
            .filter(|k| {
                !k.starts_with("META-INF/")
                    && !k.eq_ignore_ascii_case("mimetype")
                    && !valid_zip_paths.contains(*k)
            })
            .cloned()
            .collect();
        for k in extra_unmanifested {
            raw_entries.remove(&k);
            removed_orphans += 1;
        }
    }

    // Write re-packed compressed EPUB into staging path, then apply Option B backup if keep_source
    let write_epub_to = |target: &Path| -> DomainResult<()> {
        let out_file = File::create(target)
            .map_err(|e| DomainError::Storage(format!("写入规范化 EPUB 失败: {e}")))?;
        let mut writer = ZipWriter::new(BufWriter::new(out_file));

        // Entry #0: uncompressed mimetype
        write_zip_entry(
            &mut writer,
            "mimetype",
            epub_stored_zip_options(),
            b"application/epub+zip",
            "mimetype",
        )?;

        let mut written: HashSet<String> = HashSet::new();
        written.insert("mimetype".to_string());

        // Write existing entries in deterministic order first, then newly added entries
        let mut ordered_keys: Vec<String> = entry_order
            .iter()
            .filter(|k| !written.contains(*k) && raw_entries.contains_key(*k))
            .cloned()
            .collect();
        let mut extra_keys: Vec<String> = raw_entries
            .keys()
            .filter(|k| !written.contains(*k) && !ordered_keys.contains(k))
            .cloned()
            .collect();
        extra_keys.sort();
        ordered_keys.extend(extra_keys);

        for key in ordered_keys {
            if written.contains(&key) {
                continue;
            }
            let Some(bytes) = raw_entries.get(&key) else {
                continue;
            };
            let mt = manifest_mime_by_path
                .get(&key)
                .map(String::as_str)
                .unwrap_or("application/octet-stream");
            write_zip_entry(
                &mut writer,
                &key,
                epub_entry_zip_options(&key, mt, true),
                bytes,
                &key,
            )?;
            written.insert(key);
        }

        let mut buf_writer = writer
            .finish()
            .map_err(|e| DomainError::Storage(format!("epub finalize error: {e}")))?;
        std::io::Write::flush(&mut buf_writer)
            .map_err(|e| DomainError::Storage(format!("epub flush error: {e}")))?;
        buf_writer
            .get_ref()
            .sync_all()
            .map_err(|e| DomainError::Storage(format!("epub sync error: {e}")))?;
        Ok(())
    };

    let backup_path = if opts.keep_source {
        let orig_perms = std::fs::metadata(epub_path).ok().map(|m| m.permissions());
        let backup = unique_orig_epub_backup_path(epub_path);
        let parent = epub_path.parent().unwrap_or_else(|| Path::new("."));
        let staging = parent.join(format!(".xmedia_norm_{}.tmp", Uuid::now_v7()));
        if let Err(err) = write_epub_to(&staging) {
            let _ = std::fs::remove_file(&staging);
            return Err(err);
        }
        if let Some(perms) = orig_perms {
            let _ = std::fs::set_permissions(&staging, perms);
        }
        std::fs::rename(epub_path, &backup).map_err(|e| {
            let _ = std::fs::remove_file(&staging);
            DomainError::Storage(format!("备份原 EPUB 失败: {e}"))
        })?;
        if let Err(e) = std::fs::rename(&staging, epub_path) {
            // Rollback original file name
            let _ = std::fs::rename(&backup, epub_path);
            let _ = std::fs::remove_file(&staging);
            return Err(DomainError::Storage(format!("替换规范化 EPUB 失败: {e}")));
        }
        Some(backup)
    } else {
        atomic_write_via_temp(epub_path, write_epub_to)?;
        None
    };

    let after_bytes = std::fs::metadata(epub_path).map(|m| m.len()).unwrap_or(0);

    Ok(NormalizeBookOutcome {
        output_path: epub_path.to_path_buf(),
        backup_path,
        source_format: "epub".to_string(),
        before_bytes,
        after_bytes,
        chapter_count: chapter_count.max(1),
        compressed_images,
        removed_orphans,
        deleted_source: false,
        skipped: false,
        skip_reason: None,
    })
}

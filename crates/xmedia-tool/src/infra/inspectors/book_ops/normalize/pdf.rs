use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::path::Path;
use std::time::Duration;

use image::{DynamicImage, ImageBuffer, ImageFormat};
use lopdf::{Document, Object, ObjectId, decode_text_string};
use quick_xml::escape::escape;
use xmedia_core::domain::error::{DomainError, DomainResult};

use super::image_compress::{compress_image_bytes, load_and_compress_sidecar_cover};
use super::{
    GeneratedChapter, GeneratedImageAsset, NormalizeBookOutcome, NormalizeBookToEpubOptions,
    remove_source_and_stem_sidecars, write_fresh_normalized_epub,
};
use crate::infra::inspectors::book::read_book_meta_any;
use crate::infra::inspectors::book_ops::DEFAULT_EPUB_CSS;
use crate::infra::inspectors::book_ops::txt::segment_txt_chapters;
use crate::infra::inspectors::book_ops::xhtml::plain_to_xhtml_body;

fn try_extract_pdf_pages_with_pdftotext(pdf_path: &Path) -> Option<Vec<String>> {
    let path_str = pdf_path.to_str()?;
    let output = crate::infra::inspectors::process::run_command_bounded(
        "pdftotext",
        &["-enc", "UTF-8", "--", path_str, "-"],
        Duration::from_secs(60),
        32 * 1024 * 1024,
    )
    .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    let mut pages: Vec<String> = text.split('\u{000c}').map(|s| s.to_string()).collect();
    if pages.last().is_some_and(|s| s.trim().is_empty()) && pages.len() > 1 {
        pages.pop();
    }
    Some(pages)
}

fn extract_pdf_pages_with_lopdf(doc: &Document) -> Vec<String> {
    let page_map = doc.get_pages();
    let mut page_nums: Vec<u32> = page_map.keys().copied().collect();
    page_nums.sort_unstable();
    let mut out = Vec::with_capacity(page_nums.len());
    for num in page_nums {
        let text = doc.extract_text(&[num]).unwrap_or_default();
        out.push(text);
    }
    out
}

fn resolve_pdf_dest_page(
    doc: &Document,
    dest_obj: &Object,
    page_id_to_num: &HashMap<ObjectId, u32>,
) -> Option<u32> {
    let resolved = match dest_obj {
        Object::Reference(id) => doc.get_object(*id).ok()?,
        other => other,
    };
    match resolved {
        Object::Array(arr) => {
            if let Some(Object::Reference(pid)) = arr.first() {
                return page_id_to_num.get(pid).copied();
            }
            None
        }
        Object::Dictionary(d) => {
            if let Ok(inner) = d.get(b"D") {
                return resolve_pdf_dest_page(doc, inner, page_id_to_num);
            }
            None
        }
        _ => None,
    }
}

fn collect_pdf_outline_items(
    doc: &Document,
    first_id: ObjectId,
    level: u8,
    page_id_to_num: &HashMap<ObjectId, u32>,
    out: &mut Vec<(String, u8, u32)>,
    visited: &mut HashSet<ObjectId>,
) {
    let mut current = Some(first_id);
    while let Some(oid) = current {
        if !visited.insert(oid) || out.len() >= 1000 {
            break;
        }
        let Ok(dict) = doc.get_dictionary(oid) else {
            break;
        };

        let title = dict
            .get(b"Title")
            .ok()
            .and_then(|o| match o {
                Object::Reference(id) => doc.get_object(*id).ok(),
                other => Some(other),
            })
            .and_then(|o| decode_text_string(o).ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        let page_num = dict
            .get(b"Dest")
            .ok()
            .and_then(|d| resolve_pdf_dest_page(doc, d, page_id_to_num))
            .or_else(|| {
                dict.get(b"A")
                    .ok()
                    .and_then(|a| resolve_pdf_dest_page(doc, a, page_id_to_num))
            });

        if let (Some(t), Some(pn)) = (title, page_num) {
            out.push((t, level.min(1), pn));
        }

        if let Ok(Object::Reference(child_id)) = dict.get(b"First") {
            collect_pdf_outline_items(
                doc,
                *child_id,
                level.saturating_add(1),
                page_id_to_num,
                out,
                visited,
            );
        }

        current = match dict.get(b"Next") {
            Ok(Object::Reference(next_id)) => Some(*next_id),
            _ => None,
        };
    }
}

fn extract_pdf_outlines(doc: &Document) -> Vec<(String, u8, u32)> {
    let page_id_to_num: HashMap<ObjectId, u32> = doc
        .get_pages()
        .into_iter()
        .map(|(num, oid)| (oid, num))
        .collect();

    let Ok(catalog) = doc.catalog() else {
        return Vec::new();
    };
    let Ok(Object::Reference(outlines_id)) = catalog.get(b"Outlines") else {
        return Vec::new();
    };
    let Ok(outlines_dict) = doc.get_dictionary(*outlines_id) else {
        return Vec::new();
    };
    let Ok(Object::Reference(first_id)) = outlines_dict.get(b"First") else {
        return Vec::new();
    };

    let mut out = Vec::new();
    let mut visited = HashSet::new();
    collect_pdf_outline_items(doc, *first_id, 0, &page_id_to_num, &mut out, &mut visited);
    out
}

fn extract_pdf_stream_image(
    doc: &Document,
    stream_id: ObjectId,
) -> Option<(Vec<u8>, &'static str)> {
    let Ok(Object::Stream(stream)) = doc.get_object(stream_id) else {
        return None;
    };
    let dict = &stream.dict;
    let subtype = dict
        .get(b"Subtype")
        .ok()
        .and_then(|o| o.as_name().ok())
        .unwrap_or_default();
    if subtype != b"Image" {
        return None;
    }

    let width = dict
        .get(b"Width")
        .ok()
        .and_then(|o| o.as_i64().ok())
        .unwrap_or(0) as u32;
    let height = dict
        .get(b"Height")
        .ok()
        .and_then(|o| o.as_i64().ok())
        .unwrap_or(0) as u32;
    if width < 80 || height < 80 {
        return None;
    }

    let filter_name: Option<Vec<u8>> = match dict.get(b"Filter") {
        Ok(Object::Name(n)) => Some(n.clone()),
        Ok(Object::Array(arr)) => arr
            .last()
            .and_then(|o| o.as_name().ok())
            .map(|n| n.to_vec()),
        _ => None,
    };

    if filter_name.as_deref() == Some(b"DCTDecode") {
        return Some((stream.content.clone(), "image/jpeg"));
    }

    if filter_name.as_deref() == Some(b"FlateDecode") || filter_name.is_none() {
        let mut cloned = stream.clone();
        if cloned.decompress().is_ok() {
            let bpc = dict
                .get(b"BitsPerComponent")
                .ok()
                .and_then(|o| o.as_i64().ok())
                .unwrap_or(8);
            if bpc != 8 {
                return None;
            }
            let cs_name = dict.get(b"ColorSpace").ok().and_then(|o| match o {
                Object::Name(n) => Some(n.as_slice()),
                Object::Array(arr) => arr.first().and_then(|x| x.as_name().ok()),
                _ => None,
            });
            let pixel_count = (width as usize).checked_mul(height as usize)?;
            let dynamic = if cs_name == Some(b"DeviceGray") && cloned.content.len() >= pixel_count {
                let buf = ImageBuffer::<image::Luma<u8>, Vec<u8>>::from_raw(
                    width,
                    height,
                    cloned.content[..pixel_count].to_vec(),
                )?;
                DynamicImage::ImageLuma8(buf)
            } else if cloned.content.len() >= pixel_count.saturating_mul(3) {
                let rgb_len = pixel_count * 3;
                let buf = ImageBuffer::<image::Rgb<u8>, Vec<u8>>::from_raw(
                    width,
                    height,
                    cloned.content[..rgb_len].to_vec(),
                )?;
                DynamicImage::ImageRgb8(buf)
            } else {
                return None;
            };
            let mut cursor = Cursor::new(Vec::new());
            if dynamic.write_to(&mut cursor, ImageFormat::Png).is_ok() {
                return Some((cursor.into_inner(), "image/png"));
            }
        }
    }

    None
}

fn extract_largest_image_from_pdf_page(
    doc: &Document,
    page_id: ObjectId,
) -> Option<(Vec<u8>, &'static str)> {
    let Ok((res_opt, _)) = doc.get_page_resources(page_id) else {
        return None;
    };
    let res_dict = res_opt?;
    let xobj_obj = res_dict.get(b"XObject").ok()?;
    let xobj_dict = match xobj_obj {
        Object::Reference(id) => doc.get_dictionary(*id).ok()?,
        Object::Dictionary(d) => d,
        _ => return None,
    };

    let mut best: Option<(usize, Vec<u8>, &'static str)> = None;
    for (_, val) in xobj_dict.iter() {
        if let Object::Reference(sid) = val
            && let Some((bytes, mime)) = extract_pdf_stream_image(doc, *sid)
        {
            let score = bytes.len();
            if best.as_ref().is_none_or(|(bs, _, _)| score > *bs) {
                best = Some((score, bytes, mime));
            }
        }
    }
    best.map(|(_, bytes, mime)| (bytes, mime))
}

fn clean_pdf_page_text(raw: &str) -> String {
    let mut paragraphs: Vec<String> = Vec::new();
    let mut current = String::new();
    for line in raw.replace("\r\n", "\n").lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !current.is_empty() {
                paragraphs.push(std::mem::take(&mut current));
            }
            continue;
        }
        if current.is_empty() {
            current.push_str(trimmed);
        } else if current.ends_with('-')
            && trimmed
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_lowercase())
        {
            current.pop();
            current.push_str(trimmed);
        } else {
            let last_cjk = current
                .chars()
                .last()
                .is_some_and(|c| ('\u{4e00}'..='\u{9fff}').contains(&c));
            let next_cjk = trimmed
                .chars()
                .next()
                .is_some_and(|c| ('\u{4e00}'..='\u{9fff}').contains(&c));
            if !(last_cjk && next_cjk) {
                current.push(' ');
            }
            current.push_str(trimmed);
        }
    }
    if !current.is_empty() {
        paragraphs.push(current);
    }
    paragraphs.join("\n\n")
}

pub(super) fn normalize_pdf_to_epub(
    pdf_path: &Path,
    opts: &NormalizeBookToEpubOptions,
) -> DomainResult<NormalizeBookOutcome> {
    let before_bytes = std::fs::metadata(pdf_path).map(|m| m.len()).unwrap_or(0);
    let epub_path = pdf_path.with_extension("epub");
    if epub_path.exists() {
        return Err(DomainError::Conflict(format!(
            "目标文件 '{}' 已存在，请先重命名或移除已有同名 EPUB",
            epub_path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        )));
    }

    let meta = read_book_meta_any(pdf_path)?;
    let doc = Document::load(pdf_path)
        .map_err(|e| DomainError::Validation(format!("解析 PDF 失败（可能已加密或损坏）: {e}")))?;

    let pages_map = doc.get_pages();
    let mut sorted_pages: Vec<(u32, ObjectId)> = pages_map.into_iter().collect();
    sorted_pages.sort_by_key(|(num, _)| *num);

    let (mut cover, mut compressed_images) = load_and_compress_sidecar_cover(pdf_path, opts);
    if cover.is_none()
        && let Some((_, first_page_id)) = sorted_pages.first()
        && let Some((raw_cov, raw_mime)) = extract_largest_image_from_pdf_page(&doc, *first_page_id)
    {
        let comp = compress_image_bytes(raw_cov, raw_mime, opts);
        if comp.changed {
            compressed_images += 1;
        }
        cover = Some((comp.bytes, comp.mime, comp.ext));
    }

    // Extract page texts (prefer `pdftotext` when installed on host, fallback to pure-Rust `lopdf`)
    let page_texts = try_extract_pdf_pages_with_pdftotext(pdf_path)
        .unwrap_or_else(|| extract_pdf_pages_with_lopdf(&doc));

    let total_non_ws: usize = page_texts
        .iter()
        .map(|p| p.chars().filter(|c| !c.is_whitespace()).count())
        .sum();
    let page_count = sorted_pages.len().max(1);
    let has_usable_text = total_non_ws >= 40 && (page_count == 1 || total_non_ws / page_count >= 8);

    let mut chapters: Vec<GeneratedChapter> = Vec::new();
    let mut extra_images: Vec<GeneratedImageAsset> = Vec::new();

    if has_usable_text {
        let outlines = extract_pdf_outlines(&doc);
        let distinct_outline_pages: HashSet<u32> = outlines.iter().map(|(_, _, p)| *p).collect();

        if outlines.len() >= 2 && distinct_outline_pages.len() >= 2 {
            // Group pages using PDF bookmarks/outlines
            let mut sorted_outlines = outlines;
            sorted_outlines.sort_by_key(|(_, _, p)| *p);
            for (idx, (title, level, start_page)) in sorted_outlines.iter().enumerate() {
                let start_idx = start_page.saturating_sub(1) as usize;
                let end_idx = sorted_outlines
                    .get(idx + 1)
                    .map(|(_, _, next_p)| next_p.saturating_sub(1) as usize)
                    .unwrap_or(page_texts.len())
                    .max(start_idx.saturating_add(1))
                    .min(page_texts.len());
                if start_idx >= page_texts.len() {
                    continue;
                }
                let joined = page_texts[start_idx..end_idx]
                    .iter()
                    .map(|p| clean_pdf_page_text(p))
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n\n");
                let body = plain_to_xhtml_body(title, &joined);
                chapters.push(GeneratedChapter {
                    title: title.clone(),
                    level: *level,
                    body_xhtml: body,
                });
            }
        }

        if chapters.is_empty() {
            let full_text = page_texts
                .iter()
                .map(|p| clean_pdf_page_text(p))
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("\n\n");
            let segmented = segment_txt_chapters(&full_text, None, None)?;
            chapters = segmented
                .into_iter()
                .map(|c| GeneratedChapter {
                    title: c.title,
                    level: c.level,
                    body_xhtml: c.xhtml_body,
                })
                .collect();
        }
    } else {
        if !opts.pdf_scanned_fallback {
            return Err(DomainError::Validation(
                "该 PDF 未提取到有效文字层（疑似扫描版），请开启「扫描版 PDF 自动转图页」后重试"
                    .into(),
            ));
        }

        for (idx, (_, page_id)) in sorted_pages.iter().enumerate() {
            if let Some((raw_img, raw_mime)) = extract_largest_image_from_pdf_page(&doc, *page_id) {
                let comp = compress_image_bytes(raw_img, raw_mime, opts);
                if comp.changed {
                    compressed_images += 1;
                }
                let rel_href = format!("Images/page_{:04}.{}", idx + 1, comp.ext);
                let page_title = format!("第 {} 页", idx + 1);
                let body = format!(
                    "  <div class=\"pdf-page\" style=\"text-align:center;\"><img src=\"../{rel_href}\" alt=\"{}\" style=\"max-width:100%;height:auto;\"/></div>",
                    escape(&page_title)
                );
                extra_images.push(GeneratedImageAsset {
                    rel_href,
                    media_type: comp.mime,
                    bytes: comp.bytes,
                });
                chapters.push(GeneratedChapter {
                    title: page_title,
                    level: 0,
                    body_xhtml: body,
                });
            }
        }

        if chapters.is_empty() {
            return Err(DomainError::Validation(
                "该 PDF 既无有效文字层也未提取到可转换的位图页面".into(),
            ));
        }
    }

    write_fresh_normalized_epub(
        &epub_path,
        &meta,
        &chapters,
        &extra_images,
        cover,
        DEFAULT_EPUB_CSS,
        opts.generate_jacket,
    )?;

    let after_bytes = std::fs::metadata(&epub_path).map(|m| m.len()).unwrap_or(0);
    let deleted_source = if !opts.keep_source {
        remove_source_and_stem_sidecars(pdf_path);
        true
    } else {
        false
    };

    Ok(NormalizeBookOutcome {
        output_path: epub_path,
        backup_path: None,
        source_format: "pdf".to_string(),
        before_bytes,
        after_bytes,
        chapter_count: chapters.len(),
        compressed_images,
        removed_orphans: 0,
        deleted_source,
        skipped: false,
        skip_reason: None,
    })
}

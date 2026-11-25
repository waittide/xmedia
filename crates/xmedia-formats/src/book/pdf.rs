use std::path::Path;

use lopdf::{Document, Object, decode_text_string};

use super::{BookFormatExt, BookTocEntry, ParsedBookMeta, ParsedBookPackage};
use crate::error::{FormatError, FormatResult};

fn dict_string(doc: &Document, info_dict: &lopdf::Dictionary, key: &[u8]) -> Option<String> {
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

fn populate_from_doc(doc: &Document, meta: &mut ParsedBookMeta) -> Vec<BookTocEntry> {
    let pages = doc.get_pages().len();
    meta.page_count = u32::try_from(pages).ok();

    if let Ok(info_ref) = doc.trailer.get(b"Info")
        && let Ok(info_obj) = match info_ref {
            Object::Reference(id) => doc.get_object(*id),
            other => Ok(other),
        }
        && let Ok(info_dict) = info_obj.as_dict()
    {
        if let Some(t) = dict_string(doc, info_dict, b"Title") {
            meta.title = t;
        }
        if let Some(a) = dict_string(doc, info_dict, b"Author") {
            meta.authors = a
                .split([',', ';', '、'])
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
        if let Some(subj) = dict_string(doc, info_dict, b"Subject") {
            meta.description = Some(subj);
        }
        if let Some(kw) = dict_string(doc, info_dict, b"Keywords") {
            meta.tags = kw
                .split([',', ';', '、', ' '])
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
    }

    if let Ok(pdf_toc) = doc.get_toc() {
        pdf_toc
            .toc
            .into_iter()
            .take(5000)
            .enumerate()
            .map(|(i, item)| BookTocEntry {
                index: u32::try_from(i).unwrap_or(0),
                title: item.title,
                href: format!("page:{}", item.page.max(1)),
                level: u8::try_from(item.level.saturating_sub(1)).unwrap_or(0),
            })
            .collect()
    } else {
        Vec::new()
    }
}

pub fn parse_pdf_bytes(file_bytes: &[u8], fallback_title: &str) -> FormatResult<ParsedBookPackage> {
    if !file_bytes.starts_with(b"%PDF-")
        && !file_bytes
            .windows(5)
            .take(1024)
            .any(|window| window == b"%PDF-")
    {
        return Err(FormatError::Validation(
            "invalid PDF file: missing %PDF- header".into(),
        ));
    }

    let doc = Document::load_mem(file_bytes)
        .map_err(|e| FormatError::Validation(format!("invalid or corrupt PDF file: {e}")))?;
    let mut metadata = ParsedBookMeta::with_fallback(BookFormatExt::Pdf, fallback_title);
    let toc = populate_from_doc(&doc, &mut metadata);

    Ok(ParsedBookPackage {
        metadata,
        cover: None,
        toc,
    })
}

pub fn apply_pdf_info_from_path(path: &Path, meta: &mut ParsedBookMeta) {
    let Ok(doc) = Document::load(path) else {
        return;
    };
    let _ = populate_from_doc(&doc, meta);
}

pub fn list_pdf_toc_bytes(file_bytes: &[u8]) -> FormatResult<Vec<BookTocEntry>> {
    let Ok(doc) = Document::load_mem(file_bytes) else {
        return Ok(Vec::new());
    };
    let Ok(pdf_toc) = doc.get_toc() else {
        return Ok(Vec::new());
    };
    Ok(pdf_toc
        .toc
        .into_iter()
        .take(5000)
        .enumerate()
        .map(|(i, item)| BookTocEntry {
            index: u32::try_from(i).unwrap_or(0),
            title: item.title,
            href: format!("page:{}", item.page.max(1)),
            level: u8::try_from(item.level.saturating_sub(1)).unwrap_or(0),
        })
        .collect())
}

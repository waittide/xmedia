use lopdf::{Document, Object, decode_text_string};
use xmedia_core::domain::error::{DomainError, DomainResult};

use crate::application::ports::{ParsedBook, TocItem};
use crate::domain::book::BookMetadata;

use super::svg_cover::generate_placeholder_svg_cover;

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

pub fn parse_pdf(file_bytes: &[u8], fallback_title: &str) -> DomainResult<ParsedBook> {
    if !file_bytes.starts_with(b"%PDF-")
        && !file_bytes
            .windows(5)
            .take(1024)
            .any(|window| window == b"%PDF-")
    {
        return Err(DomainError::Validation(
            "invalid PDF file: missing %PDF- header".into(),
        ));
    }

    let mut title = fallback_title.to_string();
    let mut authors = Vec::new();
    let mut description = None;
    let mut tags = Vec::new();
    let mut toc = Vec::new();
    let doc = Document::load_mem(file_bytes)
        .map_err(|e| DomainError::Validation(format!("invalid or corrupt PDF file: {e}")))?;

    let pages = doc.get_pages().len();
    let page_count = u32::try_from(pages).ok();

    if let Ok(info_ref) = doc.trailer.get(b"Info")
        && let Ok(info_obj) = match info_ref {
            Object::Reference(id) => doc.get_object(*id),
            other => Ok(other),
        }
        && let Ok(info_dict) = info_obj.as_dict()
    {
        if let Some(t) = dict_string(&doc, info_dict, b"Title") {
            title = t;
        }
        if let Some(a) = dict_string(&doc, info_dict, b"Author") {
            authors = a
                .split([',', ';', '、'])
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
        if let Some(subj) = dict_string(&doc, info_dict, b"Subject") {
            description = Some(subj);
        }
        if let Some(kw) = dict_string(&doc, info_dict, b"Keywords") {
            tags = kw
                .split([',', ';', ' '])
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
    }

    if let Ok(pdf_toc) = doc.get_toc() {
        toc = pdf_toc
            .toc
            .into_iter()
            .take(5000)
            .enumerate()
            .map(|(i, item)| TocItem {
                index: u32::try_from(i).unwrap_or(0),
                title: item.title,
                href: format!("page:{}", item.page.max(1)),
                level: u8::try_from(item.level.saturating_sub(1)).unwrap_or(0),
            })
            .collect();
    }

    let mut metadata = BookMetadata::new(&title)?;
    metadata.authors = authors;
    metadata.description = description;
    metadata.tags = tags;
    metadata.page_count = page_count;
    metadata = metadata.normalized()?;

    let cover = Some(generate_placeholder_svg_cover(
        &metadata.title,
        metadata.authors.first().map(String::as_str),
        "PDF",
    ));

    Ok(ParsedBook {
        metadata,
        cover,
        toc,
    })
}

pub fn list_pdf_toc(file_bytes: &[u8]) -> DomainResult<Vec<TocItem>> {
    let Ok(doc) = Document::load_mem(file_bytes) else {
        return Ok(Vec::new());
    };
    let Ok(pdf_toc) = doc.get_toc() else {
        return Ok(Vec::new());
    };
    Ok(pdf_toc
        .toc
        .into_iter()
        .enumerate()
        .map(|(i, item)| TocItem {
            index: u32::try_from(i).unwrap_or(0),
            title: item.title,
            href: format!("page:{}", item.page.max(1)),
            level: u8::try_from(item.level.saturating_sub(1)).unwrap_or(0),
        })
        .collect())
}

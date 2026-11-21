use std::path::{Path, PathBuf};

use lopdf::{Document, Object, decode_text_string};
use xmedia_core::domain::error::DomainResult;

use super::epub_zip::parse_epub_archive;
use super::opf::parse_opf_bytes;
use crate::domain::media::BookFileMeta;

pub(super) const BOOK_EXTENSIONS: &[&str] = &["epub", "pdf", "txt"];

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

pub(super) fn is_dedicated_calibre_book_dir(dir: &Path) -> bool {
    let Some(dir_name) = dir.file_name().and_then(|s| s.to_str()) else {
        return false;
    };
    !is_generic_book_folder(dir_name) && dir_has_single_book(dir)
}

pub(super) fn find_existing_sidecar_opf(book_path: &Path) -> Option<PathBuf> {
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

pub(super) fn preferred_write_opf_path(book_path: &Path) -> PathBuf {
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

pub(crate) fn find_sidecar_cover(book_path: &Path) -> Option<PathBuf> {
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

pub(crate) fn read_book_meta_any(path: &Path) -> DomainResult<BookFileMeta> {
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
            if !sidecar.locked_fields.is_empty() {
                meta.locked_fields = sidecar.locked_fields;
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

    meta.is_normalized = false;
    meta.has_cover = find_sidecar_cover(path).is_some();
    Ok(meta)
}

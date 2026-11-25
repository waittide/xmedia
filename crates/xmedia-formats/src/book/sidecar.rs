use std::path::{Path, PathBuf};

use super::BookFormatExt;

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

pub const SIDECAR_COVER_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp"];

pub fn is_generic_book_folder(name: &str) -> bool {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return true;
    }
    let lower = trimmed.to_ascii_lowercase();
    GENERIC_BOOK_FOLDER_NAMES
        .iter()
        .any(|g| lower == *g || trimmed == *g)
}

/// Returns `true` when `dir` contains exactly one recognized book file (`epub/pdf/txt/mobi/azw3/fb2`).
pub fn dir_has_single_book(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    let mut count = 0usize;
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_file() && BookFormatExt::from_path(&p).is_some() {
            count += 1;
            if count > 1 {
                return false;
            }
        }
    }
    count == 1
}

/// Returns `true` when `dir` is a dedicated single-book folder (e.g., Calibre `Author/Title (123)/`)
/// rather than a generic top-level container folder (`Books/`, `Downloads/`, etc.) or `library_root`.
pub fn is_dedicated_book_dir(dir: &Path, library_root: Option<&Path>) -> bool {
    if library_root.is_some_and(|root| root == dir) {
        return false;
    }
    let Some(dir_name) = dir.file_name().and_then(|s| s.to_str()) else {
        return false;
    };
    !is_generic_book_folder(dir_name) && dir_has_single_book(dir)
}

/// Finds an existing sidecar OPF file (`<stem>.opf` or `metadata.opf` in a dedicated single-book directory).
pub fn find_existing_sidecar_opf(book_path: &Path, library_root: Option<&Path>) -> Option<PathBuf> {
    let same_stem = book_path.with_extension("opf");
    if same_stem.is_file() {
        return Some(same_stem);
    }
    if let Some(parent) = book_path.parent()
        && library_root != Some(parent)
        && dir_has_single_book(parent)
    {
        let calibre_opf = parent.join("metadata.opf");
        if calibre_opf.is_file() {
            return Some(calibre_opf);
        }
    }
    None
}

/// Determines the preferred sidecar OPF path when writing metadata for non-EPUB books.
pub fn preferred_write_opf_path(book_path: &Path, library_root: Option<&Path>) -> PathBuf {
    let same_stem = book_path.with_extension("opf");
    if same_stem.is_file() {
        return same_stem;
    }
    if let Some(parent) = book_path.parent() {
        let calibre_opf = parent.join("metadata.opf");
        if calibre_opf.is_file() || is_dedicated_book_dir(parent, library_root) {
            return calibre_opf;
        }
    }
    same_stem
}

/// Finds a sidecar cover image (`<stem>.{jpg,jpeg,png,webp}` or `cover.{jpg,jpeg,png,webp}` in a
/// single-book directory).
pub fn find_sidecar_cover(book_path: &Path, library_root: Option<&Path>) -> Option<PathBuf> {
    if let Some(same_stem) = SIDECAR_COVER_EXTENSIONS
        .iter()
        .map(|ext| book_path.with_extension(ext))
        .find(|p| p.is_file())
    {
        return Some(same_stem);
    }
    if let Some(parent) = book_path.parent()
        && library_root != Some(parent)
        && dir_has_single_book(parent)
    {
        return SIDECAR_COVER_EXTENSIONS
            .iter()
            .map(|ext| parent.join(format!("cover.{ext}")))
            .find(|p| p.is_file());
    }
    None
}

/// Returns candidate sidecar paths whose modification timestamps affect incremental re-scan hashing.
pub fn tracked_book_sidecar_paths(book_path: &Path, is_single_book_dir: bool) -> Vec<PathBuf> {
    let mut tracked = vec![
        book_path.with_extension("opf"),
        book_path.with_extension("jpg"),
        book_path.with_extension("jpeg"),
        book_path.with_extension("png"),
        book_path.with_extension("webp"),
    ];
    if is_single_book_dir && let Some(parent) = book_path.parent() {
        tracked.extend([
            parent.join("metadata.opf"),
            parent.join("cover.jpg"),
            parent.join("cover.jpeg"),
            parent.join("cover.png"),
            parent.join("cover.webp"),
        ]);
    }
    tracked
}

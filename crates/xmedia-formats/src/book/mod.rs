pub mod epub;
pub mod epub_toc;
pub mod epub_write;
pub mod fb2;
pub mod mobi;
pub mod opf;
pub mod opf_write;
pub mod pdf;
pub mod sidecar;
pub mod txt;

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::common::BinaryAsset;
use crate::error::{FormatError, FormatResult};

/// Canonical supported book file extensions across main server and `xmedia-tool`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BookFormatExt {
    Epub,
    Pdf,
    Txt,
    Mobi,
    Azw3,
    Fb2,
}

impl BookFormatExt {
    pub const ALL_EXTENSIONS: &'static [&'static str] =
        &["epub", "pdf", "txt", "mobi", "azw3", "fb2"];

    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext
            .trim()
            .trim_start_matches('.')
            .to_ascii_lowercase()
            .as_str()
        {
            "epub" => Some(Self::Epub),
            "pdf" => Some(Self::Pdf),
            "txt" => Some(Self::Txt),
            "mobi" | "prc" => Some(Self::Mobi),
            "azw3" | "azw" | "kf8" => Some(Self::Azw3),
            "fb2" => Some(Self::Fb2),
            _ => None,
        }
    }

    pub fn from_path(path: &Path) -> Option<Self> {
        path.extension()
            .and_then(|s| s.to_str())
            .and_then(Self::from_extension)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Epub => "epub",
            Self::Pdf => "pdf",
            Self::Txt => "txt",
            Self::Mobi => "mobi",
            Self::Azw3 => "azw3",
            Self::Fb2 => "fb2",
        }
    }

    pub fn mime_type(self) -> &'static str {
        match self {
            Self::Epub => "application/epub+zip",
            Self::Pdf => "application/pdf",
            Self::Txt => "text/plain; charset=utf-8",
            Self::Mobi => "application/x-mobipocket-ebook",
            Self::Azw3 => "application/vnd.amazon.ebook",
            Self::Fb2 => "application/x-fictionbook+xml",
        }
    }

    /// Returns `true` for formats whose chapters are read as decoded plain-text slices
    /// (`txt`, `mobi`, `azw3`, `fb2`).
    pub fn is_text_chapter_based(self) -> bool {
        matches!(self, Self::Txt | Self::Mobi | Self::Azw3 | Self::Fb2)
    }
}

/// Unified rich book metadata shared by main-server scanners and `xmedia-tool` inspectors.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParsedBookMeta {
    pub format: BookFormatExt,
    pub title: String,
    pub authors: Vec<String>,
    pub description: Option<String>,
    pub isbn: Option<String>,
    pub language: Option<String>,
    pub publisher: Option<String>,
    pub published_at: Option<String>,
    pub tags: Vec<String>,
    pub series: Option<String>,
    pub subseries: Option<String>,
    pub series_index: Option<f64>,
    pub rating: Option<f64>,
    pub page_count: Option<u32>,
    pub chapter_count: Option<u32>,
    pub locked_fields: Vec<String>,
    pub is_normalized: bool,
    pub has_cover: bool,
}

impl Default for ParsedBookMeta {
    fn default() -> Self {
        Self {
            format: BookFormatExt::Epub,
            title: String::new(),
            authors: Vec::new(),
            description: None,
            isbn: None,
            language: None,
            publisher: None,
            published_at: None,
            tags: Vec::new(),
            series: None,
            subseries: None,
            series_index: None,
            rating: None,
            page_count: None,
            chapter_count: None,
            locked_fields: Vec::new(),
            is_normalized: false,
            has_cover: false,
        }
    }
}

impl ParsedBookMeta {
    pub fn with_fallback(format: BookFormatExt, fallback_title: &str) -> Self {
        let clean = fallback_title.trim();
        Self {
            format,
            title: if clean.is_empty() {
                "Untitled".to_string()
            } else {
                clean.to_string()
            },
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BookTocEntry {
    pub index: u32,
    pub title: String,
    pub href: String,
    pub level: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BookTextChapter {
    pub index: u32,
    pub volume_title: Option<String>,
    pub title: String,
    pub content: String,
    pub level: u8,
}

#[derive(Debug, Clone)]
pub struct ParsedBookPackage {
    pub metadata: ParsedBookMeta,
    pub cover: Option<BinaryAsset>,
    pub toc: Vec<BookTocEntry>,
}

/// Parses any supported book format from in-memory bytes.
pub fn parse_book_bytes(
    format: BookFormatExt,
    bytes: &[u8],
    fallback_title: &str,
) -> FormatResult<ParsedBookPackage> {
    match format {
        BookFormatExt::Epub => epub::parse_epub_bytes(bytes, fallback_title),
        BookFormatExt::Pdf => pdf::parse_pdf_bytes(bytes, fallback_title),
        BookFormatExt::Txt => txt::parse_txt_bytes(bytes, fallback_title),
        BookFormatExt::Fb2 => fb2::parse_fb2_bytes(bytes, fallback_title),
        BookFormatExt::Mobi | BookFormatExt::Azw3 => {
            mobi::parse_mobi_bytes(bytes, format, fallback_title)
        }
    }
}

/// Lists TOC entries for any supported book format from bytes.
pub fn list_book_toc_bytes(format: BookFormatExt, bytes: &[u8]) -> FormatResult<Vec<BookTocEntry>> {
    match format {
        BookFormatExt::Epub => epub::list_epub_toc_bytes(bytes),
        BookFormatExt::Pdf => pdf::list_pdf_toc_bytes(bytes),
        BookFormatExt::Txt => Ok(txt::list_txt_toc_bytes(bytes)),
        BookFormatExt::Fb2 => fb2::list_fb2_toc_bytes(bytes),
        BookFormatExt::Mobi | BookFormatExt::Azw3 => mobi::list_mobi_toc_bytes(bytes),
    }
}

/// Reads a single decoded text chapter (`(chapter, total_chapters)`) for `Txt`, `Fb2`, `Mobi`, or `Azw3`.
pub fn read_book_text_chapter_bytes(
    format: BookFormatExt,
    bytes: &[u8],
    chapter_index: u32,
) -> FormatResult<(BookTextChapter, u32)> {
    match format {
        BookFormatExt::Txt => txt::read_txt_chapter_bytes(bytes, chapter_index),
        BookFormatExt::Fb2 => fb2::read_fb2_chapter_bytes(bytes, chapter_index),
        BookFormatExt::Mobi | BookFormatExt::Azw3 => {
            mobi::read_mobi_chapter_bytes(bytes, chapter_index)
        }
        BookFormatExt::Epub | BookFormatExt::Pdf => Err(FormatError::UnsupportedFormat(format!(
            "format '{}' does not use plain-text chapter slices",
            format.as_str()
        ))),
    }
}

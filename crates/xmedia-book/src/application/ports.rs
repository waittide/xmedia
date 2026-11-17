use async_trait::async_trait;
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use xmedia_core::application::ports::{ByteRangeSpec, ByteRangeStream};
use xmedia_core::domain::error::DomainResult;
use xmedia_core::domain::library::Library;

use crate::domain::book::{Book, BookFormat, BookId, BookMetadata};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverPayload {
    pub bytes: Bytes,
    pub mime_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TocItem {
    pub index: u32,
    pub title: String,
    pub href: String,
    pub level: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TxtChapterContent {
    pub index: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume_title: Option<String>,
    pub title: String,
    pub content: String,
    pub total_chapters: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookAsset {
    pub bytes: Bytes,
    pub mime_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedBook {
    pub metadata: BookMetadata,
    pub cover: Option<CoverPayload>,
    pub toc: Vec<TocItem>,
}

pub trait BookParserPort: Send + Sync {
    fn parse_book(
        &self,
        format: BookFormat,
        file_bytes: &[u8],
        fallback_title: &str,
    ) -> DomainResult<ParsedBook>;

    fn list_toc(&self, format: BookFormat, file_bytes: &[u8]) -> DomainResult<Vec<TocItem>>;

    fn read_txt_chapter(
        &self,
        file_bytes: &[u8],
        chapter_index: u32,
    ) -> DomainResult<TxtChapterContent>;

    fn read_epub_resource(&self, file_bytes: &[u8], resource_path: &str)
    -> DomainResult<BookAsset>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredManagedBook {
    pub storage_key: String,
    pub file_hash: String,
    pub file_size: u64,
}

#[async_trait]
pub trait BookStoragePort: Send + Sync {
    async fn save_managed_book(
        &self,
        book_id: BookId,
        format: BookFormat,
        data: Bytes,
    ) -> DomainResult<StoredManagedBook>;

    async fn delete_managed_book(&self, storage_key: &str) -> DomainResult<()>;

    async fn save_cover(&self, book_id: BookId, cover: &CoverPayload) -> DomainResult<String>;
    async fn read_cover(&self, cover_key: &str) -> DomainResult<CoverPayload>;
    async fn delete_cover(&self, cover_key: &str) -> DomainResult<()>;

    async fn read_book_bytes(&self, library: &Library, book: &Book) -> DomainResult<Bytes>;

    async fn open_book_stream(
        &self,
        library: &Library,
        book: &Book,
        spec: ByteRangeSpec,
    ) -> DomainResult<ByteRangeStream>;
}

#[derive(Debug, Clone)]
pub struct ScannedBookEntry {
    pub relative_path: String,
    pub format: BookFormat,
    pub file_size: u64,
    pub file_mtime: i64,
    pub fallback_title: String,
}

#[derive(Debug, Clone)]
pub struct LoadedMountedBook {
    pub bytes: Bytes,
    pub file_hash: String,
}

#[async_trait]
pub trait BookScannerPort: Send + Sync {
    async fn scan_entries(&self, root_path: &str) -> DomainResult<Vec<ScannedBookEntry>>;
    async fn read_entry(
        &self,
        root_path: &str,
        relative_path: &str,
    ) -> DomainResult<LoadedMountedBook>;
}

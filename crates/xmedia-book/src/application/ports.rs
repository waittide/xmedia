use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use xmedia_core::application::ports::{ByteRangeSpec, ByteRangeStream};
use xmedia_core::domain::error::DomainResult;
use xmedia_core::domain::library::Library;

use crate::domain::book::{Book, BookFormat, BookId, BookMetadata};
use crate::domain::online::{
    BookJsContext, ChapterJsContext, LegadoBookSource, OnlineBookDetail, OnlineChapterContent,
    OnlineChapterItem, PackBookMetadata, PackChapterInput, SearchBookResult, SourceCallError,
    SourceCallOutput, SourceRuntimeState, TrackedChapterFile, TrackedManifest,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverPayload {
    pub bytes: Bytes,
    pub mime_type: String,
    pub(crate) trusted_generated_svg: bool,
}

impl CoverPayload {
    pub fn new(bytes: impl Into<Bytes>, mime_type: impl Into<String>) -> Self {
        Self {
            bytes: bytes.into(),
            mime_type: mime_type.into(),
            trusted_generated_svg: false,
        }
    }

    pub(crate) fn trusted_generated_svg(bytes: impl Into<Bytes>) -> Self {
        Self {
            bytes: bytes.into(),
            mime_type: "image/svg+xml".to_string(),
            trusted_generated_svg: true,
        }
    }
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

#[derive(Debug, Clone, PartialEq)]
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

    fn parse_book_with_sidecars(
        &self,
        format: BookFormat,
        file_bytes: &[u8],
        fallback_title: &str,
        _sidecar_opf: Option<&[u8]>,
        _sidecar_cover: Option<CoverPayload>,
    ) -> DomainResult<ParsedBook> {
        self.parse_book(format, file_bytes, fallback_title)
    }

    fn list_toc(&self, format: BookFormat, file_bytes: &[u8]) -> DomainResult<Vec<TocItem>>;

    fn list_toc_from_path(&self, format: BookFormat, path: &Path) -> DomainResult<Vec<TocItem>> {
        let bytes = std::fs::read(path).map_err(|e| {
            xmedia_core::domain::error::DomainError::Storage(format!(
                "failed to read book {}: {e}",
                path.display()
            ))
        })?;
        self.list_toc(format, &bytes)
    }

    fn read_text_chapter(
        &self,
        format: BookFormat,
        file_bytes: &[u8],
        chapter_index: u32,
    ) -> DomainResult<TxtChapterContent>;

    fn read_txt_chapter(
        &self,
        file_bytes: &[u8],
        chapter_index: u32,
    ) -> DomainResult<TxtChapterContent> {
        self.read_text_chapter(BookFormat::Txt, file_bytes, chapter_index)
    }

    fn read_epub_resource(&self, file_bytes: &[u8], resource_path: &str)
    -> DomainResult<BookAsset>;

    fn read_epub_resource_from_path(
        &self,
        path: &Path,
        resource_path: &str,
    ) -> DomainResult<BookAsset> {
        let bytes = std::fs::read(path).map_err(|e| {
            xmedia_core::domain::error::DomainError::Storage(format!(
                "failed to read book {}: {e}",
                path.display()
            ))
        })?;
        self.read_epub_resource(&bytes, resource_path)
    }
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

    fn resolve_book_fs_path(
        &self,
        _library: &Library,
        _book: &Book,
    ) -> DomainResult<Option<PathBuf>> {
        Ok(None)
    }

    async fn open_book_stream(
        &self,
        library: &Library,
        book: &Book,
        spec: ByteRangeSpec,
    ) -> DomainResult<ByteRangeStream>;

    async fn open_path_stream(
        &self,
        path: &Path,
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
    pub legacy_sha256: String,
    pub sidecar_opf: Option<Vec<u8>>,
    pub sidecar_cover: Option<CoverPayload>,
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

pub trait BookPackagerPort: Send + Sync {
    fn package_epub(
        &self,
        meta: &PackBookMetadata,
        chapters: &[PackChapterInput],
    ) -> Result<Vec<u8>, String>;

    fn package_txt(&self, meta: &PackBookMetadata, chapters: &[PackChapterInput]) -> Vec<u8>;
}

pub trait TrackedBookStorePort: Send + Sync {
    fn remove_book_lock(&self, book_id: BookId);
    fn load_manifest(&self, book_id: BookId) -> TrackedManifest;
    fn save_manifest(&self, book_id: BookId, manifest: &TrackedManifest) -> Result<(), String>;
    fn update_manifest_from_toc(
        &self,
        book_id: BookId,
        toc: &[OnlineChapterItem],
        preserve_existing_cache: bool,
    ) -> Result<TrackedManifest, String>;
    fn load_chapter(&self, book_id: BookId, index: usize) -> Option<TrackedChapterFile>;
    fn load_chapter_matching_url(
        &self,
        book_id: BookId,
        index: usize,
        expected_url: &str,
    ) -> Option<TrackedChapterFile>;
    fn save_chapter(&self, book_id: BookId, chapter: &TrackedChapterFile) -> Result<u32, String>;
    fn collect_cached_chapters_for_epub(
        &self,
        book_id: BookId,
    ) -> (Vec<PackChapterInput>, u32, u32);
    fn clear_chapters(&self, book_id: BookId);
    fn delete_book_dir(&self, book_id: BookId);
    fn read_cached_online_chapter(
        &self,
        source_id: Uuid,
        chapter_url: &str,
    ) -> Option<OnlineChapterContent>;
    fn write_cached_online_chapter(
        &self,
        source_id: Uuid,
        chapter_url: &str,
        content: &OnlineChapterContent,
    );
}

pub trait BookBatchWorkerPort {
    fn reset_chapter_budget(&mut self);
    fn fetch_chapter_content(
        &mut self,
        chapter_url: &str,
        book_ctx: &BookJsContext,
        chapter_ctx: &ChapterJsContext,
    ) -> Result<OnlineChapterContent, String>;
}

pub trait BookBatchContextPort: Send + Sync {
    fn snapshot_state(&self) -> SourceRuntimeState;
    fn create_worker_session(&self) -> Box<dyn BookBatchWorkerPort>;
    fn fetch_toc_sync(
        &self,
        toc_url: &str,
        book_ctx: &BookJsContext,
    ) -> Result<Vec<OnlineChapterItem>, String>;
}

pub trait BookSourceEnginePort: Send + Sync {
    fn prepare_batch_context(
        &self,
        source: Arc<LegadoBookSource>,
        initial_state: SourceRuntimeState,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<Arc<dyn BookBatchContextPort>, String>;

    fn search_sync_with_control(
        &self,
        source: &LegadoBookSource,
        state: SourceRuntimeState,
        keyword: &str,
        page: u32,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<Vec<SearchBookResult>>, SourceCallError>;

    fn fetch_book_info_sync_with_control(
        &self,
        source: &LegadoBookSource,
        state: SourceRuntimeState,
        book_url: &str,
        fallback_hint: Option<&BookJsContext>,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<OnlineBookDetail>, SourceCallError>;

    fn fetch_toc_sync_with_control(
        &self,
        source: &LegadoBookSource,
        state: SourceRuntimeState,
        toc_url: &str,
        book_ctx: &BookJsContext,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<Vec<OnlineChapterItem>>, SourceCallError>;

    #[allow(clippy::too_many_arguments)]
    fn fetch_chapter_content_sync_with_control(
        &self,
        source: &LegadoBookSource,
        state: SourceRuntimeState,
        chapter_url: &str,
        book_ctx: &BookJsContext,
        chapter_ctx: &ChapterJsContext,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<OnlineChapterContent>, SourceCallError>;

    fn run_login_action_sync(
        &self,
        source: &LegadoBookSource,
        state: SourceRuntimeState,
        login_info: HashMap<String, String>,
        action_js: Option<&str>,
    ) -> Result<SourceCallOutput<String>, SourceCallError>;

    fn http_config(&self) -> xmedia_source::http::HttpClientConfig {
        xmedia_source::http::HttpClientConfig::default()
    }
}

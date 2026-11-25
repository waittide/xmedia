use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use async_trait::async_trait;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use xmedia_core::application::ports::{ByteRangeSpec, ByteRangeStream};
use xmedia_core::domain::error::DomainResult;

use crate::domain::chapter::{ChapterKind, ChapterSourceKind};
use crate::domain::online::{
    ComicChapterJsContext, ComicJsContext, ComicSourceSpec, ExploreCategoryItem,
    OnlineComicChapter, OnlineComicDetail, OnlineComicItem, SourceCallError, SourceCallOutput,
    SourceRuntimeState,
};
use crate::domain::series::ReadingDirection;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ComicInfoMetadata {
    pub series: Option<String>,
    pub series_group: Option<String>,
    pub title: Option<String>,
    pub number: Option<f64>,
    pub volume: Option<f64>,
    pub summary: Option<String>,
    pub writers: Vec<String>,
    pub pencillers: Vec<String>,
    pub publisher: Option<String>,
    pub genres: Vec<String>,
    pub tags: Vec<String>,
    pub age_rating: Option<String>,
    pub language_iso: Option<String>,
    pub year: Option<i32>,
    pub reading_direction: Option<ReadingDirection>,
    pub page_count: Option<u32>,
}

impl From<xmedia_formats::comic::comicinfo::ComicInfoMeta> for ComicInfoMetadata {
    fn from(ci: xmedia_formats::comic::comicinfo::ComicInfoMeta) -> Self {
        let mut pencillers = ci.pencillers;
        for artist in ci.cover_artists {
            if !pencillers
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(&artist))
            {
                pencillers.push(artist);
            }
        }
        Self {
            series: ci.series,
            series_group: ci.series_group,
            title: ci.title,
            number: ci.number,
            volume: ci.volume,
            summary: ci.summary,
            writers: ci.writers,
            pencillers,
            publisher: ci.publisher,
            genres: ci.genres,
            tags: ci.tags,
            age_rating: ci.age_rating,
            language_iso: ci.language_iso,
            year: ci.year,
            reading_direction: ci.reading_direction.map(ReadingDirection::from),
            page_count: ci.page_count,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageDescriptor {
    pub page_number: u32,
    pub entry_name: String,
    pub mime_type: String,
    pub file_size: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub page_type: Option<String>,
    pub double_page: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct PageContent {
    pub bytes: Vec<u8>,
    pub mime_type: String,
    pub etag: String,
}

#[derive(Debug, Clone)]
pub struct ArchiveInspection {
    pub page_count: u32,
    pub pages: Vec<PageDescriptor>,
    pub comic_info: Option<ComicInfoMetadata>,
    pub file_size: u64,
    pub file_hash: String,
}

#[derive(Debug, Clone)]
pub struct ExistingChapterDigest {
    pub file_size: u64,
    pub file_hash: String,
    pub page_count: u32,
    pub updated_at: Timestamp,
    pub series_title: Option<String>,
    pub series_source_dir: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ScannedChapterEntry {
    pub kind: ChapterKind,
    pub number: f64,
    pub volume_number: Option<f64>,
    pub title: String,
    pub page_count: u32,
    pub source_kind: ChapterSourceKind,
    pub source_path: String,
    pub file_size: u64,
    pub file_hash: String,
    pub comic_info: Option<ComicInfoMetadata>,
    pub unchanged: bool,
    pub preserved_series_title: Option<String>,
    pub preserved_series_source_dir: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ScannedSeriesEntry {
    pub title: String,
    pub source_dir: String,
    pub chapters: Vec<ScannedChapterEntry>,
    pub series_cover_bytes: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub struct StoredUploadFile {
    pub source_dir: String,
    pub source_path: String,
    pub file_size: u64,
    pub file_hash: String,
}

#[async_trait]
pub trait ArchiveReaderPort: Send + Sync {
    async fn inspect_source(
        &self,
        source_kind: ChapterSourceKind,
        source_path: &str,
    ) -> DomainResult<ArchiveInspection>;

    async fn inspect_bytes(&self, filename: &str, bytes: &[u8]) -> DomainResult<ArchiveInspection>;

    async fn read_page(
        &self,
        source_kind: ChapterSourceKind,
        source_path: &str,
        page_number: u32,
        file_hash_hint: &str,
    ) -> DomainResult<PageContent>;

    async fn open_raw_archive_stream(
        &self,
        source_path: &str,
        spec: ByteRangeSpec,
    ) -> DomainResult<(ByteRangeStream, String, String)>;
}

#[async_trait]
pub trait ThumbnailGeneratorPort: Send + Sync {
    async fn generate_from_bytes(&self, key: &str, image_bytes: &[u8]) -> DomainResult<String>;
    async fn read_thumbnail(&self, cover_key: &str) -> DomainResult<PageContent>;
    async fn read_thumbnail_sized(
        &self,
        cover_key: &str,
        max_size: Option<u32>,
    ) -> DomainResult<PageContent> {
        let _ = max_size;
        self.read_thumbnail(cover_key).await
    }
}

#[async_trait]
pub trait ComicScannerPort: Send + Sync {
    async fn scan_directory(
        &self,
        root_path: &str,
        known_chapters: &HashMap<String, ExistingChapterDigest>,
    ) -> DomainResult<Vec<ScannedSeriesEntry>>;
}

#[async_trait]
pub trait ComicFileStoragePort: Send + Sync {
    async fn ensure_managed_root(&self) -> DomainResult<String>;
    async fn store_uploaded_archive(
        &self,
        series_folder: &str,
        filename: &str,
        bytes: &[u8],
    ) -> DomainResult<StoredUploadFile>;
    async fn store_uploaded_archive_from_path(
        &self,
        series_folder: &str,
        filename: &str,
        src_path: &Path,
    ) -> DomainResult<StoredUploadFile> {
        let bytes = tokio::fs::read(src_path).await.map_err(|e| {
            xmedia_core::domain::error::DomainError::Storage(format!(
                "failed reading temp comic archive: {e}"
            ))
        })?;
        self.store_uploaded_archive(series_folder, filename, &bytes)
            .await
    }
    async fn delete_managed_file(&self, path: &str) -> DomainResult<()>;
}

pub trait ComicSourceEnginePort: Send + Sync {
    fn parse_explore_categories_with_control(
        &self,
        source: &ComicSourceSpec,
        state: SourceRuntimeState,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<Vec<ExploreCategoryItem>>, SourceCallError>;

    fn search_sync_with_control(
        &self,
        source: &ComicSourceSpec,
        state: SourceRuntimeState,
        keyword: &str,
        page: u32,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<Vec<OnlineComicItem>>, SourceCallError>;

    #[allow(clippy::too_many_arguments)]
    fn explore_sync_with_categories_and_control(
        &self,
        source: &ComicSourceSpec,
        state: SourceRuntimeState,
        explore_url: &str,
        page: u32,
        precomputed_categories: Option<&[ExploreCategoryItem]>,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<Vec<OnlineComicItem>>, SourceCallError>;

    fn fetch_detail_sync_with_control(
        &self,
        source: &ComicSourceSpec,
        state: SourceRuntimeState,
        comic_url: &str,
        fallback_hint: Option<&ComicJsContext>,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<OnlineComicDetail>, SourceCallError>;

    fn fetch_chapters_sync_with_control(
        &self,
        source: &ComicSourceSpec,
        state: SourceRuntimeState,
        toc_url: &str,
        comic_ctx: &ComicJsContext,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<Vec<OnlineComicChapter>>, SourceCallError>;

    #[allow(clippy::too_many_arguments)]
    fn fetch_pages_sync_with_control(
        &self,
        source: &ComicSourceSpec,
        state: SourceRuntimeState,
        chapter_url: &str,
        comic_ctx: &ComicJsContext,
        chapter_ctx: &ComicChapterJsContext,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<Vec<String>>, SourceCallError>;

    fn run_login_action_sync(
        &self,
        source: &ComicSourceSpec,
        state: SourceRuntimeState,
        login_info: HashMap<String, String>,
        action_js: Option<&str>,
    ) -> Result<SourceCallOutput<String>, SourceCallError>;
}

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::book::BookId;
pub use crate::domain::online::SourceTestMode;
use crate::domain::online::{
    ExecutionDiagnostics, OnlineBookDetail, OnlineChapterContent, OnlineChapterItem,
    SearchBookResult, TrackedChapterMeta,
};

pub type BookSourceSummaryDto = xmedia_source::crud::SourceSummaryDto;
pub type ImportBookSourcesReportDto =
    xmedia_source::crud::ImportSourcesReportDto<BookSourceSummaryDto>;
pub type UpdateBookSourceInput = xmedia_source::crud::UpdateSourceInput;
pub type BatchBookSourceAction = xmedia_source::crud::BatchSourceAction;
pub type BatchBookSourcesInput = xmedia_source::crud::BatchSourcesInput;
pub type BatchBookSourcesReportDto = xmedia_source::crud::BatchSourcesReportDto;
pub type LoginActionResponseDto = xmedia_source::crud::LoginActionResponseDto<BookSourceSummaryDto>;
pub type SourceDebugStepDto = xmedia_source::crud::SourceTestStepDto;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SourceTestInput {
    pub keyword: Option<String>,
    pub mode: SourceTestMode,
    pub book_url: Option<String>,
    pub toc_url: Option<String>,
    pub chapter_url: Option<String>,
    pub chapter_index: Option<usize>,
    pub chapter_title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceTestReportDto {
    pub ok: bool,
    pub mode: SourceTestMode,
    pub keyword: String,
    pub total_elapsed_ms: u64,
    pub result_count: usize,
    pub sample_titles: Vec<String>,
    pub error: Option<String>,
    pub diagnostics: ExecutionDiagnostics,
    pub steps: Vec<SourceDebugStepDto>,
    pub search_items: Vec<SearchBookResult>,
    pub book_detail: Option<OnlineBookDetail>,
    pub toc_chapters: Vec<OnlineChapterItem>,
    pub toc_total_count: usize,
    pub content_chapter_index: Option<usize>,
    pub content_chapter_title: Option<String>,
    pub content_chapter_url: Option<String>,
    pub content_text: Option<String>,
    pub content_char_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregatedSearchItemDto {
    pub source_id: Uuid,
    pub source_name: String,
    pub book: SearchBookResult,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceSearchErrorDto {
    pub source_id: Uuid,
    pub source_name: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregatedSearchResponseDto {
    pub keyword: String,
    pub page: u32,
    pub items: Vec<AggregatedSearchItemDto>,
    pub errors: Vec<SourceSearchErrorDto>,
    pub toasts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineBookDetailResponseDto {
    pub source_id: Uuid,
    pub source_name: String,
    pub detail: OnlineBookDetail,
    pub diagnostics: ExecutionDiagnostics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineTocResponseDto {
    pub source_id: Uuid,
    pub chapters: Vec<OnlineChapterItem>,
    pub diagnostics: ExecutionDiagnostics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineContentResponseDto {
    pub source_id: Uuid,
    pub chapter_index: usize,
    pub content: OnlineChapterContent,
    pub cached: bool,
    pub diagnostics: ExecutionDiagnostics,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OnlineDownloadFormat {
    Epub,
    Txt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DownloadJobStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TrackOnlineBookInput {
    pub source_id: Uuid,
    pub name: String,
    #[serde(default)]
    pub author: String,
    pub book_url: String,
    #[serde(default)]
    pub toc_url: String,
    #[serde(default)]
    pub cover_url: Option<String>,
    #[serde(default)]
    pub intro: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub last_chapter: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TrackedTocResponseDto {
    pub book_id: BookId,
    pub source_id: Uuid,
    pub source_name: String,
    pub chapters: Vec<TrackedChapterMeta>,
    pub cached_chapters: u32,
    pub total_chapters: u32,
    pub last_chapter: Option<String>,
    pub epub_dirty: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct TrackedChapterResponseDto {
    pub book_id: BookId,
    pub source_id: Uuid,
    pub chapter_index: usize,
    pub title: String,
    pub content: String,
    pub cached: bool,
    pub cached_chapters: u32,
    pub total_chapters: u32,
    pub epub_dirty: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SwitchTrackedSourceInput {
    pub source_id: Uuid,
    pub book_url: String,
    pub toc_url: String,
    #[serde(default)]
    pub cover_url: Option<String>,
    #[serde(default)]
    pub intro: Option<String>,
    #[serde(default)]
    pub last_chapter: Option<String>,
    #[serde(default)]
    pub clear_cache: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StartDownloadInput {
    #[serde(default)]
    pub book_id: Option<Uuid>,
    pub source_id: Uuid,
    pub book_name: String,
    #[serde(default)]
    pub author: String,
    pub book_url: String,
    pub toc_url: String,
    #[serde(default)]
    pub cover_url: Option<String>,
    #[serde(default)]
    pub intro: Option<String>,
    pub format: OnlineDownloadFormat,
    #[serde(default)]
    pub start_chapter: Option<usize>,
    #[serde(default)]
    pub end_chapter: Option<usize>,
    #[serde(default)]
    pub import_to_library: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineDownloadJobDto {
    pub id: Uuid,
    pub user_id: Uuid,
    pub source_id: Uuid,
    pub source_name: String,
    pub book_name: String,
    pub author: String,
    pub format: OnlineDownloadFormat,
    pub status: DownloadJobStatus,
    pub total_chapters: usize,
    pub processed_chapters: usize,
    pub failed_chapters: usize,
    #[serde(default)]
    pub chapters_per_sec: f64,
    #[serde(default)]
    pub elapsed_ms: u64,
    pub current_chapter_title: Option<String>,
    pub output_file_name: Option<String>,
    pub file_size: Option<u64>,
    pub import_to_library: bool,
    pub imported_book_id: Option<Uuid>,
    pub error: Option<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

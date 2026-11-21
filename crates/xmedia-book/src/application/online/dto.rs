use std::collections::HashMap;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::book::BookId;
use crate::source::model::{
    ExecutionDiagnostics, LoginUiItem, OnlineBookDetail, OnlineChapterContent, OnlineChapterItem,
    SearchBookResult,
};
use crate::source::tracked_store::TrackedChapterMeta;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookSourceSummaryDto {
    pub id: Uuid,
    pub name: String,
    pub source_url: String,
    pub group_name: String,
    pub comment: Option<String>,
    pub check_keyword: Option<String>,
    pub enabled: bool,
    pub weight: i64,
    pub source_type: i64,
    pub has_login_url: bool,
    pub login_ui: Vec<LoginUiItem>,
    pub login_info: HashMap<String, String>,
    pub has_variable: bool,
    pub variable_preview: String,
    pub cookie_hosts: Vec<String>,
    pub last_test_ok: Option<bool>,
    pub last_test_msg: Option<String>,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportBookSourcesReportDto {
    pub imported_count: usize,
    pub updated_count: usize,
    pub sources: Vec<BookSourceSummaryDto>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct UpdateBookSourceInput {
    pub enabled: Option<bool>,
    pub weight: Option<i64>,
    pub group_name: Option<String>,
    pub raw_json: Option<String>,
    pub variable: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum BatchBookSourceAction {
    Enable,
    Disable,
    SetGroup { group_name: String },
    Delete,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BatchBookSourcesInput {
    pub ids: Vec<Uuid>,
    #[serde(flatten)]
    pub action: BatchBookSourceAction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchBookSourcesReportDto {
    pub affected_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginActionResponseDto {
    pub result: String,
    pub diagnostics: ExecutionDiagnostics,
    pub source: BookSourceSummaryDto,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceTestMode {
    #[default]
    Full,
    Search,
    BookInfo,
    Toc,
    Content,
}

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
pub struct SourceDebugStepDto {
    pub stage: String,
    pub label: String,
    pub ok: bool,
    pub elapsed_ms: u64,
    pub summary: String,
    pub error: Option<String>,
    pub diagnostics: ExecutionDiagnostics,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineShelfBookDto {
    pub id: Uuid,
    pub source_id: Uuid,
    pub source_name: String,
    pub name: String,
    pub author: String,
    pub book_url: String,
    pub toc_url: String,
    pub cover_url: Option<String>,
    pub intro: Option<String>,
    pub kind: Option<String>,
    pub last_chapter: Option<String>,
    pub total_chapters: i64,
    pub current_chapter_index: i64,
    pub current_chapter_title: Option<String>,
    pub scroll_ratio: f64,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AddOnlineShelfInput {
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
    #[serde(default)]
    pub total_chapters: Option<i64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct UpdateOnlineShelfInput {
    pub current_chapter_index: Option<i64>,
    pub current_chapter_title: Option<String>,
    pub scroll_ratio: Option<f64>,
    pub total_chapters: Option<i64>,
    pub last_chapter: Option<String>,
    pub toc_url: Option<String>,
    // Optional source switch
    pub source_id: Option<Uuid>,
    pub book_url: Option<String>,
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

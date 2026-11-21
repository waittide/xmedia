use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::series::ReadingDirection;
use crate::source::model::{
    ComicSearchResult, ExecutionDiagnostics, ExploreCategoryItem, OnlineComicChapter,
    OnlineComicDetail,
};

pub type ComicSourceSummaryDto = xmedia_source::crud::SourceSummaryDto;
pub type ImportComicSourcesReportDto =
    xmedia_source::crud::ImportSourcesReportDto<ComicSourceSummaryDto>;
pub type UpdateComicSourceInput = xmedia_source::crud::UpdateSourceInput;
pub type BatchComicSourceAction = xmedia_source::crud::BatchSourceAction;
pub type BatchComicSourcesInput = xmedia_source::crud::BatchSourcesInput;
pub type BatchComicSourcesReportDto = xmedia_source::crud::BatchSourcesReportDto;
pub type LoginActionResponseDto =
    xmedia_source::crud::LoginActionResponseDto<ComicSourceSummaryDto>;
pub type SourceDebugStepDto = xmedia_source::crud::SourceTestStepDto;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComicSourceTestMode {
    #[default]
    Full,
    Search,
    Explore,
    #[serde(alias = "book_info")]
    Detail,
    #[serde(alias = "toc")]
    Chapters,
    #[serde(alias = "content")]
    Pages,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ComicSourceTestInput {
    pub keyword: Option<String>,
    pub mode: ComicSourceTestMode,
    pub target_url: Option<String>,
    pub explore_url: Option<String>,
    #[serde(alias = "book_url")]
    pub comic_url: Option<String>,
    pub toc_url: Option<String>,
    pub chapter_url: Option<String>,
    pub chapter_index: Option<usize>,
    pub chapter_title: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ComicSourceTestPayload {
    pub search_items: Vec<ComicSearchResult>,
    pub explore_categories: Vec<ExploreCategoryItem>,
    pub comic_detail: Option<OnlineComicDetail>,
    pub chapters: Vec<OnlineComicChapter>,
    pub chapters_total_count: usize,
    pub chapter_index: Option<usize>,
    pub chapter_title: Option<String>,
    pub chapter_url: Option<String>,
    pub pages: Vec<OnlineComicPageDto>,
    pub page_count: usize,
}

pub type ComicSourceTestReportDto =
    xmedia_source::crud::SourceTestReportDto<ComicSourceTestMode, ComicSourceTestPayload>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComicSearchResultItem {
    pub source_id: Uuid,
    pub source_name: String,
    pub title: String,
    pub author: String,
    pub comic_url: String,
    pub cover_url: Option<String>,
    pub proxy_cover_url: Option<String>,
    pub intro: Option<String>,
    pub kind: Option<String>,
    pub last_chapter: Option<String>,
    pub word_count: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceSearchErrorDto {
    pub source_id: Uuid,
    pub source_name: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineComicSearchResponseDto {
    pub keyword: String,
    pub page: u32,
    pub results: Vec<ComicSearchResultItem>,
    pub errors: Vec<SourceSearchErrorDto>,
    pub toasts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineComicExploreResponseDto {
    pub source_id: Uuid,
    pub explore_url: String,
    pub page: u32,
    pub items: Vec<ComicSearchResultItem>,
    pub categories: Vec<ExploreCategoryItem>,
    pub diagnostics: ExecutionDiagnostics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineComicDetailDto {
    pub title: String,
    pub author: String,
    pub comic_url: String,
    pub toc_url: String,
    pub cover_url: Option<String>,
    pub proxy_cover_url: Option<String>,
    pub intro: Option<String>,
    pub kind: Option<String>,
    pub status: Option<String>,
    pub last_chapter: Option<String>,
    pub word_count: Option<String>,
    pub reading_direction: ReadingDirection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineComicDetailResponseDto {
    pub source_id: Uuid,
    pub source_name: String,
    pub detail: OnlineComicDetailDto,
    pub chapters: Vec<OnlineComicChapter>,
    pub diagnostics: ExecutionDiagnostics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineComicChaptersResponseDto {
    pub source_id: Uuid,
    pub source_name: String,
    pub toc_url: String,
    pub chapters: Vec<OnlineComicChapter>,
    pub diagnostics: ExecutionDiagnostics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineComicPageDto {
    pub index: usize,
    pub image_url: String,
    pub proxy_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineComicPagesResponseDto {
    pub source_id: Uuid,
    pub source_name: String,
    pub chapter_url: String,
    pub reading_direction: ReadingDirection,
    pub pages: Vec<OnlineComicPageDto>,
    pub diagnostics: ExecutionDiagnostics,
}

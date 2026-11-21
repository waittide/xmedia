use std::path::PathBuf;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use xmedia_core::domain::library::LibraryId;
use xmedia_core::domain::user::{ComicFitMode, ComicReaderMode};

use crate::domain::chapter::{Chapter, ChapterKind, ChapterSourceKind};
use crate::domain::id::{ChapterId, ComicBookmarkId, ComicShelfId, SeriesId};
use crate::domain::reading::{
    ComicBookmark, ComicShelf, ContinueReadingPointer, SeriesReaderPref, format_chapter_number,
};
use crate::domain::series::{ReadingDirection, Series, SeriesStatus};

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SeriesQueryFilter {
    pub search: Option<String>,
    pub library_id: Option<LibraryId>,
    pub status: Option<SeriesStatus>,
    pub writer: Option<String>,
    pub penciller: Option<String>,
    pub tag: Option<String>,
    pub genre: Option<String>,
    pub read_status: Option<String>,
    #[serde(default)]
    pub favorites_only: bool,
    pub shelf_id: Option<ComicShelfId>,
    pub sort_by: Option<String>,
    pub sort_dir: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SeriesSummaryDto {
    pub id: SeriesId,
    pub library_id: LibraryId,
    pub title: String,
    pub writers: Vec<String>,
    pub pencillers: Vec<String>,
    pub status: SeriesStatus,
    pub reading_direction: ReadingDirection,
    pub age_rating: Option<String>,
    pub genres: Vec<String>,
    pub tags: Vec<String>,
    pub cover_url: Option<String>,
    pub chapter_count: u32,
    pub completed_chapters: u32,
    pub is_favorite: bool,
    pub continue_reading: Option<ContinueReadingPointer>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContinueReadingItemDto {
    pub series: SeriesSummaryDto,
    pub pointer: ContinueReadingPointer,
}

#[derive(Debug, Clone, Serialize)]
pub struct ComicLibraryStatsDto {
    pub series_count: u32,
    pub chapter_count: u32,
    pub completed_chapters: u32,
    pub in_progress_count: u32,
    pub bookmarks_count: u32,
    pub favorites_count: u32,
    pub shelves_count: u32,
    pub tags_count: u32,
    pub writers_count: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct HomeOverviewDto {
    pub continue_reading: Vec<ContinueReadingItemDto>,
    pub recently_updated: Vec<SeriesSummaryDto>,
    pub recently_added: Vec<SeriesSummaryDto>,
    pub stats: ComicLibraryStatsDto,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChapterItemDto {
    pub id: ChapterId,
    pub series_id: SeriesId,
    pub kind: ChapterKind,
    pub number: f64,
    pub number_label: String,
    pub volume_number: Option<f64>,
    pub title: String,
    pub page_count: u32,
    pub source_kind: ChapterSourceKind,
    pub file_size: u64,
    pub cover_url: Option<String>,
    pub summary: String,
    pub writers: Vec<String>,
    pub pencillers: Vec<String>,
    pub is_missing: bool,
    pub current_page: u32,
    pub is_completed: bool,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl ChapterItemDto {
    pub fn from_domain(ch: Chapter, current_page: u32, is_completed: bool) -> Self {
        let cover_url = ch
            .cover_path
            .as_ref()
            .map(|key| format!("/api/v1/comic/covers/{key}"));
        let number_label = format_chapter_number(ch.number);
        Self {
            id: ch.id,
            series_id: ch.series_id,
            kind: ch.kind,
            number: ch.number,
            number_label,
            volume_number: ch.volume_number,
            title: ch.title,
            page_count: ch.page_count,
            source_kind: ch.source_kind,
            file_size: ch.file_size,
            cover_url,
            summary: ch.summary,
            writers: ch.writers,
            pencillers: ch.pencillers,
            is_missing: ch.is_missing,
            current_page,
            is_completed,
            created_at: ch.created_at,
            updated_at: ch.updated_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SeriesDetailDto {
    pub series: Series,
    pub cover_url: Option<String>,
    pub is_favorite: bool,
    pub shelf_ids: Vec<ComicShelfId>,
    pub completed_chapters: u32,
    pub continue_reading: Option<ContinueReadingPointer>,
    pub reader_pref: Option<SeriesReaderPref>,
    pub chapters: Vec<ChapterItemDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectiveReaderPrefDto {
    pub reader_mode: ComicReaderMode,
    pub fit_mode: ComicFitMode,
    pub offset_cover_page: bool,
    pub dim_images_in_dark: bool,
    pub preload_pages: u8,
    pub webtoon_width: u16,
}

#[derive(Debug, Clone, Serialize)]
pub struct AdjacentChapterDto {
    pub id: ChapterId,
    pub kind: ChapterKind,
    pub number: f64,
    pub number_label: String,
    pub volume_number: Option<f64>,
    pub title: String,
    pub page_count: u32,
    pub is_completed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReaderContextDto {
    pub series_id: SeriesId,
    pub series_title: String,
    pub reading_direction: ReadingDirection,
    pub chapter: ChapterItemDto,
    pub effective_pref: EffectiveReaderPrefDto,
    pub prev_chapter: Option<AdjacentChapterDto>,
    pub next_chapter: Option<AdjacentChapterDto>,
    pub all_chapters: Vec<AdjacentChapterDto>,
}

#[derive(Debug, Clone)]
pub struct UploadArchiveInput {
    pub filename: String,
    pub bytes: Vec<u8>,
    pub target_series_id: Option<SeriesId>,
    pub series_title_override: Option<String>,
}

#[derive(Debug, Clone)]
pub struct UploadArchivePathInput {
    pub filename: String,
    pub temp_path: PathBuf,
    pub target_series_id: Option<SeriesId>,
    pub series_title_override: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UploadArchiveResultDto {
    pub series_id: SeriesId,
    pub series_title: String,
    pub chapter_id: ChapterId,
    pub chapter_title: String,
    pub page_count: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaxonomyCountDto {
    pub name: String,
    pub count: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaxonomyOverviewDto {
    pub tags: Vec<TaxonomyCountDto>,
    pub genres: Vec<TaxonomyCountDto>,
    pub writers: Vec<TaxonomyCountDto>,
    pub pencillers: Vec<TaxonomyCountDto>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateProgressInput {
    pub current_page: u32,
    pub is_completed: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SaveSeriesReaderPrefInput {
    pub reader_mode: ComicReaderMode,
    pub fit_mode: ComicFitMode,
    pub offset_cover_page: bool,
    pub webtoon_width: u16,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpsertShelfInput {
    pub name: String,
    pub description: Option<String>,
    pub series_ids: Option<Vec<SeriesId>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ShelfDetailDto {
    pub shelf: ComicShelf,
    pub series: Vec<SeriesSummaryDto>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateComicBookmarkInput {
    pub page_number: u32,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ComicBookmarkItemDto {
    pub id: ComicBookmarkId,
    pub bookmark: ComicBookmark,
    pub series_title: String,
    pub series_cover_url: Option<String>,
    pub chapter_number_label: String,
    pub chapter_title: String,
    pub total_pages: u32,
}

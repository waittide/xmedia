use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use xmedia_core::domain::library::LibraryId;

use crate::domain::book::{
    Book, BookFormat, BookId, BookMetadata, BookSortField, BookSource, SortOrder, TagMatchMode,
};
use crate::domain::reading::{Bookmark, ReadingLocation, ReadingProgress, Shelf, ShelfId};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookOnlineTrackingDto {
    pub source_id: Uuid,
    pub book_url: String,
    pub toc_url: String,
    pub last_chapter: Option<String>,
    pub cached_chapters: u32,
    pub total_chapters: u32,
    pub epub_dirty: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookSummaryDto {
    pub id: BookId,
    pub library_id: LibraryId,
    pub library_name: Option<String>,
    pub format: BookFormat,
    pub is_managed: bool,
    pub file_size: u64,
    pub has_cover: bool,
    pub cover_mime: Option<String>,
    pub metadata: BookMetadata,
    pub is_missing: bool,
    pub is_favorite: bool,
    pub progress: Option<ReadingLocation>,
    pub last_read_at: Option<Timestamp>,
    pub online_tracking: Option<BookOnlineTrackingDto>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

fn infer_cover_mime(cover_key: &str) -> &'static str {
    let lower = cover_key.to_ascii_lowercase();
    if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".webp") {
        "image/webp"
    } else if lower.ends_with(".gif") {
        "image/gif"
    } else if lower.ends_with(".svg") {
        "image/svg+xml"
    } else {
        "image/jpeg"
    }
}

impl BookSummaryDto {
    pub fn from_domain(
        book: &Book,
        library_name: Option<String>,
        is_favorite: bool,
        progress: Option<&ReadingProgress>,
    ) -> Self {
        let online_tracking = match &book.source {
            BookSource::OnlineTracked {
                source_id,
                book_url,
                toc_url,
                last_chapter,
                cached_chapters,
                total_chapters,
                epub_dirty,
                ..
            } => Some(BookOnlineTrackingDto {
                source_id: *source_id,
                book_url: book_url.clone(),
                toc_url: toc_url.clone(),
                last_chapter: last_chapter.clone(),
                cached_chapters: *cached_chapters,
                total_chapters: *total_chapters,
                epub_dirty: *epub_dirty,
            }),
            _ => None,
        };

        Self {
            id: book.id,
            library_id: book.library_id,
            library_name,
            format: book.format,
            is_managed: book.source.is_managed(),
            file_size: book.file_size,
            has_cover: book.cover_key.is_some(),
            cover_mime: book
                .cover_key
                .as_deref()
                .map(|k| infer_cover_mime(k).to_string()),
            metadata: book.metadata.clone(),
            is_missing: book.is_missing,
            is_favorite,
            progress: progress.map(|p| p.location.clone()),
            last_read_at: progress.map(|p| p.last_read_at),
            online_tracking,
            created_at: book.created_at,
            updated_at: book.updated_at,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CatalogSearchFilter {
    pub keyword: Option<String>,
    pub author: Option<String>,
    #[serde(default)]
    pub authors: Vec<String>,
    pub tag: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub tag_mode: TagMatchMode,
    pub format: Option<BookFormat>,
    #[serde(default)]
    pub formats: Vec<BookFormat>,
    pub library_id: Option<LibraryId>,
    #[serde(default)]
    pub library_ids: Vec<LibraryId>,
    pub favorites_only: bool,
    pub shelf_id: Option<ShelfId>,
    pub include_missing: bool,
    pub sort_by: Option<BookSortField>,
    pub sort_order: Option<SortOrder>,
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateMetadataPatch {
    pub title: Option<String>,
    pub authors: Option<Vec<String>>,
    pub description: Option<String>,
    pub isbn: Option<String>,
    pub language: Option<String>,
    pub publisher: Option<String>,
    pub published_at: Option<String>,
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShelfDto {
    pub id: ShelfId,
    pub name: String,
    pub is_favorite: bool,
    pub book_ids: Vec<BookId>,
    pub book_count: usize,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl From<&Shelf> for ShelfDto {
    fn from(shelf: &Shelf) -> Self {
        Self {
            id: shelf.id,
            name: shelf.name.clone(),
            is_favorite: shelf.is_favorite,
            book_ids: shelf.book_ids.clone(),
            book_count: shelf.book_ids.len(),
            created_at: shelf.created_at,
            updated_at: shelf.updated_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookBookmarkItemDto {
    pub bookmark: Bookmark,
    pub book_title: String,
    pub book_authors: Vec<String>,
    pub book_format: BookFormat,
    pub has_cover: bool,
    pub book_updated_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookLibraryStatsDto {
    pub total_books: u64,
    pub reading_count: u64,
    pub bookmarks_count: u64,
    pub favorites_count: u64,
    pub shelves_count: u64,
    pub tags_count: u64,
    pub authors_count: u64,
    pub libraries_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookHomeOverviewDto {
    pub stats: BookLibraryStatsDto,
    pub continue_reading: Vec<BookSummaryDto>,
    pub recently_added: Vec<BookSummaryDto>,
}

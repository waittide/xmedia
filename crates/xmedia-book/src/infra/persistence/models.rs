use jiff::Timestamp;
use uuid::Uuid;

use xmedia_core::domain::library::LibraryId;

use crate::domain::book::{Book, BookFormat, BookId, BookMetadata, BookSource};
use crate::domain::online::SourceRuntimeState;
use crate::domain::reading::ReadingLocation;

#[derive(Debug, toasty::Model)]
#[table = "book_books"]
pub(crate) struct BookModel {
    #[key]
    pub id: Uuid,

    #[index]
    pub library_id: Uuid,

    #[index]
    pub format: String,

    pub title: String,
    pub title_lower: String,

    #[column(type = text)]
    pub source: toasty::Json<BookSource>,

    pub file_size: i64,
    pub file_hash: String,
    pub cover_key: Option<String>,
    pub series: Option<String>,
    pub series_index: Option<f64>,
    pub rating: Option<f64>,

    #[column(type = text)]
    pub metadata: toasty::Json<BookMetadata>,

    #[index]
    pub is_missing: bool,

    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl BookModel {
    pub fn parsed_format(&self) -> BookFormat {
        self.format.parse().unwrap_or(BookFormat::Epub)
    }

    pub fn into_domain(self) -> Book {
        let format = self.parsed_format();
        let mut metadata = self.metadata.0;
        if metadata.series.is_none() {
            metadata.series = self.series;
        }
        if metadata.series_index.is_none() {
            metadata.series_index = self.series_index;
        }
        if metadata.rating.is_none() {
            metadata.rating = self.rating;
        }
        Book {
            id: BookId(self.id),
            library_id: LibraryId(self.library_id),
            format,
            source: self.source.0,
            file_size: u64::try_from(self.file_size.max(0)).unwrap_or(0),
            file_hash: self.file_hash,
            cover_key: self.cover_key,
            metadata,
            is_missing: self.is_missing,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

#[derive(Debug, toasty::Model)]
#[table = "book_reading_progresses"]
pub(crate) struct BookReadingProgressModel {
    #[key]
    pub progress_key: String,

    #[index]
    pub user_id: Uuid,

    #[index]
    pub book_id: Uuid,

    #[column(type = text)]
    pub location: toasty::Json<ReadingLocation>,

    pub last_read_at: Timestamp,
}

#[derive(Debug, toasty::Model)]
#[table = "book_bookmarks"]
pub(crate) struct BookBookmarkModel {
    #[key]
    pub id: Uuid,

    #[index]
    pub user_id: Uuid,

    #[index]
    pub book_id: Uuid,

    pub title: String,
    pub note: Option<String>,

    #[column(type = text)]
    pub location: toasty::Json<ReadingLocation>,

    pub created_at: Timestamp,
}

#[derive(Debug, toasty::Model)]
#[table = "book_favorites"]
pub(crate) struct BookFavoriteModel {
    #[key]
    pub fav_key: String,

    #[index]
    pub user_id: Uuid,

    #[index]
    pub book_id: Uuid,

    pub created_at: Timestamp,
}

#[derive(Debug, toasty::Model)]
#[table = "book_shelves"]
pub(crate) struct BookShelfModel {
    #[key]
    pub id: Uuid,

    #[index]
    pub user_id: Uuid,

    pub name: String,
    pub is_favorite: bool,

    #[column(type = text)]
    pub book_ids: toasty::Json<Vec<Uuid>>,

    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, toasty::Model)]
#[table = "book_sources"]
pub(crate) struct BookSourceRecordModel {
    #[key]
    pub id: Uuid,

    pub name: String,
    pub source_url: String,
    pub group_name: String,
    pub enabled: bool,
    pub weight: i64,
    pub source_type: i64,

    #[column(type = text)]
    pub raw_json: String,

    #[column(type = text)]
    pub runtime_state: toasty::Json<SourceRuntimeState>,

    pub last_test_ok: Option<bool>,
    pub last_test_msg: Option<String>,

    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

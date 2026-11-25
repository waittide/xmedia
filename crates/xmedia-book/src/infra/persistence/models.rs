use jiff::Timestamp;
use uuid::Uuid;

use crate::domain::book::{BookFormat, BookMetadata, BookSource};
use crate::domain::reading::ReadingLocation;
use crate::source::model::SourceRuntimeState;

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
        BookFormat::parse(&self.format).unwrap_or(BookFormat::Epub)
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

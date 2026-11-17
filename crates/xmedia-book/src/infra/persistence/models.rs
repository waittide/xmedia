use jiff::Timestamp;
use uuid::Uuid;

use crate::domain::book::{BookFormat, BookMetadata, BookSource};
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

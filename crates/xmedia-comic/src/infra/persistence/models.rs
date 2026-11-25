use jiff::Timestamp;
use uuid::Uuid;

use crate::domain::chapter::OnlineTrackedChapterMeta;
use crate::domain::reading::SeriesReaderPref;
use crate::domain::series::OnlineTrackedSeriesMeta;
use crate::source::model::SourceRuntimeState;

#[derive(Debug, toasty::Model)]
#[table = "comic_series"]
pub(crate) struct ComicSeriesModel {
    #[key]
    pub id: Uuid,

    #[index]
    pub library_id: Uuid,

    pub title: String,
    pub sort_title: String,

    #[column(type = text)]
    pub alt_titles: toasty::Json<Vec<String>>,

    #[column(type = text)]
    pub summary: String,

    #[column(type = text)]
    pub writers: toasty::Json<Vec<String>>,

    #[column(type = text)]
    pub pencillers: toasty::Json<Vec<String>>,

    pub publisher: Option<String>,
    pub status: String,
    pub reading_direction: String,
    pub age_rating: Option<String>,

    #[column(type = text)]
    pub genres: toasty::Json<Vec<String>>,

    #[column(type = text)]
    pub tags: toasty::Json<Vec<String>>,

    pub language: Option<String>,
    pub year: Option<i64>,

    #[index]
    pub cover_path: Option<String>,

    pub cover_is_custom: bool,
    pub chapter_count: i64,
    pub source_kind: String,

    #[column(type = text)]
    pub source_dir: String,

    #[index]
    pub online_source_id: Option<Uuid>,

    pub online_comic_url: Option<String>,

    #[column(type = text)]
    pub online_meta: Option<toasty::Json<OnlineTrackedSeriesMeta>>,

    pub metadata_locked: bool,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, toasty::Model)]
#[table = "comic_chapters"]
pub(crate) struct ComicChapterModel {
    #[key]
    pub id: Uuid,

    #[index]
    pub series_id: Uuid,

    #[index]
    pub library_id: Uuid,

    pub kind: String,
    pub number: f64,
    pub volume_number: Option<f64>,
    pub title: String,
    pub sort_key: String,
    pub page_count: i64,
    pub source_kind: String,

    #[column(type = text)]
    pub source_path: String,

    #[index]
    pub online_source_id: Option<Uuid>,

    pub online_chapter_url: Option<String>,
    pub cached_archive_path: Option<String>,

    #[column(type = text)]
    pub online_meta: Option<toasty::Json<OnlineTrackedChapterMeta>>,

    pub file_size: i64,
    pub file_hash: String,

    #[index]
    pub cover_path: Option<String>,

    #[column(type = text)]
    pub summary: String,

    #[column(type = text)]
    pub writers: toasty::Json<Vec<String>>,

    #[column(type = text)]
    pub pencillers: toasty::Json<Vec<String>>,

    pub is_missing: bool,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, toasty::Model)]
#[table = "comic_chapter_progresses"]
pub(crate) struct ComicChapterProgressModel {
    #[key]
    pub composite_key: String,

    #[index]
    pub user_id: Uuid,

    #[index]
    pub series_id: Uuid,

    #[index]
    pub chapter_id: Uuid,

    pub current_page: i64,
    pub total_pages: i64,
    pub is_completed: bool,
    pub updated_at: Timestamp,
}

#[derive(Debug, toasty::Model)]
#[table = "comic_series_reader_prefs"]
pub(crate) struct ComicSeriesReaderPrefModel {
    #[key]
    pub composite_key: String,

    #[index]
    pub user_id: Uuid,

    #[index]
    pub series_id: Uuid,

    #[column(type = text)]
    pub pref: toasty::Json<SeriesReaderPref>,

    pub updated_at: Timestamp,
}

#[derive(Debug, toasty::Model)]
#[table = "comic_favorite_series"]
pub(crate) struct ComicFavoriteSeriesModel {
    #[key]
    pub composite_key: String,

    #[index]
    pub user_id: Uuid,

    #[index]
    pub series_id: Uuid,

    pub created_at: Timestamp,
}

#[derive(Debug, toasty::Model)]
#[table = "comic_shelves"]
pub(crate) struct ComicShelfModel {
    #[key]
    pub id: Uuid,

    #[index]
    pub user_id: Uuid,

    pub name: String,
    pub description: String,

    #[column(type = text)]
    pub series_ids: toasty::Json<Vec<Uuid>>,

    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, toasty::Model)]
#[table = "comic_bookmarks"]
pub(crate) struct ComicBookmarkModel {
    #[key]
    pub id: Uuid,

    #[index]
    pub user_id: Uuid,

    #[index]
    pub series_id: Uuid,

    #[index]
    pub chapter_id: Uuid,

    pub page_number: i64,
    pub title: String,
    pub created_at: Timestamp,
}

#[derive(Debug, toasty::Model)]
#[table = "comic_sources"]
pub(crate) struct ComicSourceRecordModel {
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

use async_trait::async_trait;
use jiff::Timestamp;
use uuid::Uuid;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::domain::library::LibraryId;
use xmedia_core::domain::user::UserId;

use crate::domain::chapter::Chapter;
use crate::domain::id::{ChapterId, ComicBookmarkId, ComicShelfId, SeriesId};
use crate::domain::online::{ComicSourceMetadataUpdate, ComicSourceRecord};
use crate::domain::reading::{
    ChapterProgress, ComicBookmark, ComicShelf, FavoriteSeries, SeriesReaderPref,
};
use crate::domain::series::Series;
use crate::source::model::SourceRuntimeState;

#[async_trait]
pub trait SeriesRepository: Send + Sync {
    async fn find_by_id(&self, id: SeriesId) -> DomainResult<Option<Series>>;
    async fn find_by_library_and_dir(
        &self,
        library_id: LibraryId,
        source_dir: &str,
    ) -> DomainResult<Option<Series>>;
    async fn list_all(&self) -> DomainResult<Vec<Series>>;
    async fn list_by_library(&self, library_id: LibraryId) -> DomainResult<Vec<Series>>;
    async fn count_by_library(&self, library_id: LibraryId) -> DomainResult<u64>;
    async fn save(&self, series: &Series) -> DomainResult<()>;
    async fn delete(&self, id: SeriesId) -> DomainResult<()>;
}

#[async_trait]
pub trait ChapterRepository: Send + Sync {
    async fn find_by_id(&self, id: ChapterId) -> DomainResult<Option<Chapter>>;
    async fn find_by_ids(&self, ids: &[ChapterId]) -> DomainResult<Vec<Chapter>>;
    async fn list_by_series(&self, series_id: SeriesId) -> DomainResult<Vec<Chapter>>;
    async fn list_by_series_ids(&self, series_ids: &[SeriesId]) -> DomainResult<Vec<Chapter>>;
    async fn list_by_library(&self, library_id: LibraryId) -> DomainResult<Vec<Chapter>>;
    async fn save(&self, chapter: &Chapter) -> DomainResult<()>;
    async fn delete(&self, id: ChapterId) -> DomainResult<()>;
    async fn delete_by_series(&self, series_id: SeriesId) -> DomainResult<()>;
}

#[async_trait]
pub trait ComicReadingRepository: Send + Sync {
    async fn get_chapter_progress(
        &self,
        user_id: UserId,
        chapter_id: ChapterId,
    ) -> DomainResult<Option<ChapterProgress>>;
    async fn list_series_progress(
        &self,
        user_id: UserId,
        series_id: SeriesId,
    ) -> DomainResult<Vec<ChapterProgress>>;
    async fn list_user_progress(&self, user_id: UserId) -> DomainResult<Vec<ChapterProgress>>;
    async fn save_chapter_progress(&self, progress: &ChapterProgress) -> DomainResult<()>;
    async fn delete_chapter_progress(
        &self,
        user_id: UserId,
        chapter_id: ChapterId,
    ) -> DomainResult<()>;
    async fn delete_by_chapter(&self, chapter_id: ChapterId) -> DomainResult<()>;
    async fn delete_by_series(&self, series_id: SeriesId) -> DomainResult<()>;
    async fn delete_by_user(&self, user_id: UserId) -> DomainResult<()>;

    async fn get_series_pref(
        &self,
        user_id: UserId,
        series_id: SeriesId,
    ) -> DomainResult<Option<SeriesReaderPref>>;
    async fn save_series_pref(&self, pref: &SeriesReaderPref) -> DomainResult<()>;

    async fn is_favorite(&self, user_id: UserId, series_id: SeriesId) -> DomainResult<bool>;
    async fn list_favorites(&self, user_id: UserId) -> DomainResult<Vec<FavoriteSeries>>;
    async fn add_favorite(&self, fav: &FavoriteSeries) -> DomainResult<()>;
    async fn remove_favorite(&self, user_id: UserId, series_id: SeriesId) -> DomainResult<()>;

    async fn find_shelf(&self, id: ComicShelfId) -> DomainResult<Option<ComicShelf>>;
    async fn list_shelves(&self, user_id: UserId) -> DomainResult<Vec<ComicShelf>>;
    async fn save_shelf(&self, shelf: &ComicShelf) -> DomainResult<()>;
    async fn delete_shelf(&self, id: ComicShelfId) -> DomainResult<()>;

    async fn list_chapter_bookmarks(
        &self,
        user_id: UserId,
        chapter_id: ChapterId,
    ) -> DomainResult<Vec<ComicBookmark>>;
    async fn list_user_bookmarks(&self, user_id: UserId) -> DomainResult<Vec<ComicBookmark>>;
    async fn save_bookmark(&self, bookmark: &ComicBookmark) -> DomainResult<()>;
    async fn delete_bookmark(
        &self,
        user_id: UserId,
        bookmark_id: ComicBookmarkId,
    ) -> DomainResult<()>;
}

#[async_trait]
pub trait ComicSourceRepository: Send + Sync {
    async fn list_all(&self) -> DomainResult<Vec<ComicSourceRecord>>;
    async fn find_by_id(&self, id: Uuid) -> DomainResult<Option<ComicSourceRecord>>;
    async fn save(&self, source: &ComicSourceRecord) -> DomainResult<()>;
    async fn update_existing(&self, source: &ComicSourceRecord) -> DomainResult<bool>;
    async fn update_metadata_if_exists(
        &self,
        id: Uuid,
        patch: ComicSourceMetadataUpdate,
    ) -> DomainResult<Option<ComicSourceRecord>>;
    async fn update_runtime_state(
        &self,
        id: Uuid,
        runtime_state: SourceRuntimeState,
        updated_at: Timestamp,
    ) -> DomainResult<()>;
    async fn update_test_result(
        &self,
        id: Uuid,
        runtime_state: SourceRuntimeState,
        last_test_ok: bool,
        last_test_msg: Option<String>,
        updated_at: Timestamp,
    ) -> DomainResult<bool>;
    async fn update_weight_if_exists(
        &self,
        id: Uuid,
        weight: i64,
        updated_at: Timestamp,
    ) -> DomainResult<Option<ComicSourceRecord>>;
    async fn update_batch_fields_if_exists(
        &self,
        id: Uuid,
        enabled: Option<bool>,
        group_name: Option<String>,
        updated_at: Timestamp,
    ) -> DomainResult<bool>;
    async fn delete(&self, id: Uuid) -> DomainResult<()>;
}

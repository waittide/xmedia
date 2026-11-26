use async_trait::async_trait;
use jiff::Timestamp;
use uuid::Uuid;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::domain::library::LibraryId;
use xmedia_core::domain::user::UserId;

use crate::domain::chapter::Chapter;
use crate::domain::id::{ChapterId, ComicBookmarkId, ComicShelfId, SeriesId};
use crate::domain::reading::{
    ChapterProgress, ComicBookmark, ComicShelf, FavoriteSeries, SeriesReaderPref,
};
use crate::domain::series::Series;

#[derive(Debug, Clone)]
pub enum SeriesQuery {
    Id(SeriesId),
    LocalDir {
        library_id: LibraryId,
        source_dir: String,
    },
    OnlineTracked {
        source_id: Uuid,
        comic_url: String,
    },
    CoverKey(String),
    Library(LibraryId),
    All,
}

#[async_trait]
pub trait SeriesRepository: Send + Sync {
    async fn query(&self, scope: SeriesQuery) -> DomainResult<Vec<Series>>;
    async fn save(&self, series: &Series) -> DomainResult<()>;
    async fn delete(&self, id: SeriesId) -> DomainResult<()>;
}

#[derive(Debug, Clone)]
pub enum ChapterQuery {
    Id(ChapterId),
    Ids(Vec<ChapterId>),
    CoverKey(String),
    Series(SeriesId),
    SeriesIds(Vec<SeriesId>),
    Library(LibraryId),
}

#[derive(Debug, Clone, Copy)]
pub enum ChapterDeleteScope {
    Id(ChapterId),
    Series(SeriesId),
}

#[async_trait]
pub trait ChapterRepository: Send + Sync {
    async fn query(&self, scope: ChapterQuery) -> DomainResult<Vec<Chapter>>;
    async fn save(&self, chapter: &Chapter) -> DomainResult<()>;
    async fn delete_scope(&self, scope: ChapterDeleteScope) -> DomainResult<()>;
}

#[derive(Debug, Clone, Copy)]
pub enum ComicProgressScope {
    Chapter(ChapterId),
    Series(SeriesId),
    All,
}

#[derive(Debug, Clone)]
pub enum ComicReadingMutation<'a> {
    Progress(&'a ChapterProgress),
    SeriesPref(&'a SeriesReaderPref),
    Favorite {
        user_id: UserId,
        series_id: SeriesId,
        favorite: bool,
        now: Timestamp,
    },
    Shelf(&'a ComicShelf),
    Bookmark(&'a ComicBookmark),
}

#[derive(Debug, Clone, Copy)]
pub enum ComicReadingDeleteScope {
    ChapterProgress {
        user_id: UserId,
        chapter_id: ChapterId,
    },
    Chapter(ChapterId),
    Series(SeriesId),
    User(UserId),
    UserSeries {
        user_id: UserId,
        series_id: SeriesId,
    },
    Shelf(ComicShelfId),
    Bookmark {
        user_id: UserId,
        bookmark_id: ComicBookmarkId,
    },
}

#[async_trait]
pub trait ComicReadingRepository: Send + Sync {
    async fn query_progress(
        &self,
        user_id: UserId,
        scope: ComicProgressScope,
    ) -> DomainResult<Vec<ChapterProgress>>;
    async fn get_series_pref(
        &self,
        user_id: UserId,
        series_id: SeriesId,
    ) -> DomainResult<Option<SeriesReaderPref>>;
    async fn list_favorites(&self, user_id: UserId) -> DomainResult<Vec<FavoriteSeries>>;
    async fn query_shelves(
        &self,
        user_id: Option<UserId>,
        shelf_id: Option<ComicShelfId>,
    ) -> DomainResult<Vec<ComicShelf>>;
    async fn list_bookmarks(
        &self,
        user_id: UserId,
        chapter_id: Option<ChapterId>,
    ) -> DomainResult<Vec<ComicBookmark>>;
    async fn save(&self, mutation: ComicReadingMutation<'_>) -> DomainResult<()>;
    async fn delete_scope(&self, scope: ComicReadingDeleteScope) -> DomainResult<()>;
}

use async_trait::async_trait;
use jiff::Timestamp;
use toasty::Db;
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::LibraryId;
use xmedia_core::domain::user::UserId;

use crate::domain::chapter::{Chapter, ChapterKind, ChapterSourceKind};
use crate::domain::id::{ChapterId, ComicBookmarkId, ComicShelfId, SeriesId};
use crate::domain::reading::{
    ChapterProgress, ComicBookmark, ComicShelf, FavoriteSeries, SeriesReaderPref,
};
use crate::domain::repository::{ChapterRepository, ComicReadingRepository, SeriesRepository};
use crate::domain::series::{ReadingDirection, Series, SeriesStatus};

use super::models::{
    ComicBookmarkModel, ComicChapterModel, ComicChapterProgressModel, ComicFavoriteSeriesModel,
    ComicSeriesModel, ComicSeriesReaderPrefModel, ComicShelfModel,
};

fn db_err(err: impl std::fmt::Display) -> DomainError {
    DomainError::Persistence(err.to_string())
}

#[derive(Clone)]
pub struct ToastySeriesRepository {
    db: Db,
}

impl ToastySeriesRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    fn to_domain(m: ComicSeriesModel) -> Series {
        Series {
            id: SeriesId(m.id),
            library_id: LibraryId(m.library_id),
            title: m.title,
            sort_title: m.sort_title,
            alt_titles: m.alt_titles.0,
            summary: m.summary,
            writers: m.writers.0,
            pencillers: m.pencillers.0,
            publisher: m.publisher,
            status: SeriesStatus::parse(&m.status),
            reading_direction: ReadingDirection::parse(&m.reading_direction),
            age_rating: m.age_rating,
            genres: m.genres.0,
            tags: m.tags.0,
            language: m.language,
            year: m.year.and_then(|y| i32::try_from(y).ok()),
            cover_path: m.cover_path,
            chapter_count: u32::try_from(m.chapter_count.max(0)).unwrap_or(0),
            source_dir: m.source_dir,
            metadata_locked: m.metadata_locked,
            created_at: m.created_at,
            updated_at: m.updated_at,
        }
    }
}

#[async_trait]
impl SeriesRepository for ToastySeriesRepository {
    async fn find_by_id(&self, id: SeriesId) -> DomainResult<Option<Series>> {
        let mut db = self.db.clone();
        let row = ComicSeriesModel::filter_by_id(id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(row.map(Self::to_domain))
    }

    async fn find_by_library_and_dir(
        &self,
        library_id: LibraryId,
        source_dir: &str,
    ) -> DomainResult<Option<Series>> {
        let mut db = self.db.clone();
        let rows = ComicSeriesModel::filter_by_library_id(library_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(rows
            .into_iter()
            .find(|r| r.source_dir == source_dir)
            .map(Self::to_domain))
    }

    async fn list_all(&self) -> DomainResult<Vec<Series>> {
        let mut db = self.db.clone();
        let rows = ComicSeriesModel::all()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(rows.into_iter().map(Self::to_domain).collect())
    }

    async fn list_by_library(&self, library_id: LibraryId) -> DomainResult<Vec<Series>> {
        let mut db = self.db.clone();
        let rows = ComicSeriesModel::filter_by_library_id(library_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(rows.into_iter().map(Self::to_domain).collect())
    }

    async fn count_by_library(&self, library_id: LibraryId) -> DomainResult<u64> {
        let mut db = self.db.clone();
        let rows = ComicSeriesModel::filter_by_library_id(library_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(rows.len() as u64)
    }

    async fn save(&self, s: &Series) -> DomainResult<()> {
        let mut db = self.db.clone();
        let existing = ComicSeriesModel::filter_by_id(s.id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;

        let year_i64 = s.year.map(i64::from);
        let ch_count = i64::from(s.chapter_count);

        if let Some(mut record) = existing {
            record
                .update()
                .library_id(s.library_id.0)
                .title(s.title.clone())
                .sort_title(s.sort_title.clone())
                .alt_titles(toasty::Json(s.alt_titles.clone()))
                .summary(s.summary.clone())
                .writers(toasty::Json(s.writers.clone()))
                .pencillers(toasty::Json(s.pencillers.clone()))
                .publisher(s.publisher.clone())
                .status(s.status.as_str().to_string())
                .reading_direction(s.reading_direction.as_str().to_string())
                .age_rating(s.age_rating.clone())
                .genres(toasty::Json(s.genres.clone()))
                .tags(toasty::Json(s.tags.clone()))
                .language(s.language.clone())
                .year(year_i64)
                .cover_path(s.cover_path.clone())
                .chapter_count(ch_count)
                .source_dir(s.source_dir.clone())
                .metadata_locked(s.metadata_locked)
                .updated_at(s.updated_at)
                .exec(&mut db)
                .await
                .map_err(db_err)?;
        } else {
            toasty::create!(ComicSeriesModel {
                id: s.id.0,
                library_id: s.library_id.0,
                title: s.title.clone(),
                sort_title: s.sort_title.clone(),
                alt_titles: toasty::Json(s.alt_titles.clone()),
                summary: s.summary.clone(),
                writers: toasty::Json(s.writers.clone()),
                pencillers: toasty::Json(s.pencillers.clone()),
                publisher: s.publisher.clone(),
                status: s.status.as_str().to_string(),
                reading_direction: s.reading_direction.as_str().to_string(),
                age_rating: s.age_rating.clone(),
                genres: toasty::Json(s.genres.clone()),
                tags: toasty::Json(s.tags.clone()),
                language: s.language.clone(),
                year: year_i64,
                cover_path: s.cover_path.clone(),
                chapter_count: ch_count,
                source_dir: s.source_dir.clone(),
                metadata_locked: s.metadata_locked,
                created_at: s.created_at,
                updated_at: s.updated_at,
            })
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        }
        Ok(())
    }

    async fn delete(&self, id: SeriesId) -> DomainResult<()> {
        let mut db = self.db.clone();
        ComicSeriesModel::filter_by_id(id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(())
    }
}

#[derive(Clone)]
pub struct ToastyChapterRepository {
    db: Db,
}

impl ToastyChapterRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    fn to_domain(m: ComicChapterModel) -> Chapter {
        Chapter {
            id: ChapterId(m.id),
            series_id: SeriesId(m.series_id),
            library_id: LibraryId(m.library_id),
            kind: ChapterKind::parse(&m.kind),
            number: m.number,
            volume_number: m.volume_number,
            title: m.title,
            sort_key: m.sort_key,
            page_count: u32::try_from(m.page_count.max(0)).unwrap_or(0),
            source_kind: ChapterSourceKind::parse(&m.source_kind),
            source_path: m.source_path,
            file_size: u64::try_from(m.file_size.max(0)).unwrap_or(0),
            file_hash: m.file_hash,
            cover_path: m.cover_path,
            summary: m.summary,
            writers: m.writers.0,
            pencillers: m.pencillers.0,
            is_missing: m.is_missing,
            created_at: m.created_at,
            updated_at: m.updated_at,
        }
    }
}

#[async_trait]
impl ChapterRepository for ToastyChapterRepository {
    async fn find_by_id(&self, id: ChapterId) -> DomainResult<Option<Chapter>> {
        let mut db = self.db.clone();
        let row = ComicChapterModel::filter_by_id(id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(row.map(Self::to_domain))
    }

    async fn list_by_series(&self, series_id: SeriesId) -> DomainResult<Vec<Chapter>> {
        let mut db = self.db.clone();
        let rows = ComicChapterModel::filter_by_series_id(series_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(rows.into_iter().map(Self::to_domain).collect())
    }

    async fn list_by_library(&self, library_id: LibraryId) -> DomainResult<Vec<Chapter>> {
        let mut db = self.db.clone();
        let rows = ComicChapterModel::filter_by_library_id(library_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(rows.into_iter().map(Self::to_domain).collect())
    }

    async fn save(&self, c: &Chapter) -> DomainResult<()> {
        let mut db = self.db.clone();
        let existing = ComicChapterModel::filter_by_id(c.id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;

        let page_count = i64::from(c.page_count);
        let file_size = i64::try_from(c.file_size).unwrap_or(i64::MAX);

        if let Some(mut record) = existing {
            record
                .update()
                .series_id(c.series_id.0)
                .library_id(c.library_id.0)
                .kind(c.kind.as_str().to_string())
                .number(c.number)
                .volume_number(c.volume_number)
                .title(c.title.clone())
                .sort_key(c.sort_key.clone())
                .page_count(page_count)
                .source_kind(c.source_kind.as_str().to_string())
                .source_path(c.source_path.clone())
                .file_size(file_size)
                .file_hash(c.file_hash.clone())
                .cover_path(c.cover_path.clone())
                .summary(c.summary.clone())
                .writers(toasty::Json(c.writers.clone()))
                .pencillers(toasty::Json(c.pencillers.clone()))
                .is_missing(c.is_missing)
                .updated_at(c.updated_at)
                .exec(&mut db)
                .await
                .map_err(db_err)?;
        } else {
            toasty::create!(ComicChapterModel {
                id: c.id.0,
                series_id: c.series_id.0,
                library_id: c.library_id.0,
                kind: c.kind.as_str().to_string(),
                number: c.number,
                volume_number: c.volume_number,
                title: c.title.clone(),
                sort_key: c.sort_key.clone(),
                page_count: page_count,
                source_kind: c.source_kind.as_str().to_string(),
                source_path: c.source_path.clone(),
                file_size: file_size,
                file_hash: c.file_hash.clone(),
                cover_path: c.cover_path.clone(),
                summary: c.summary.clone(),
                writers: toasty::Json(c.writers.clone()),
                pencillers: toasty::Json(c.pencillers.clone()),
                is_missing: c.is_missing,
                created_at: c.created_at,
                updated_at: c.updated_at,
            })
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        }
        Ok(())
    }

    async fn delete(&self, id: ChapterId) -> DomainResult<()> {
        let mut db = self.db.clone();
        ComicChapterModel::filter_by_id(id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    async fn delete_by_series(&self, series_id: SeriesId) -> DomainResult<()> {
        let mut db = self.db.clone();
        let rows = ComicChapterModel::filter_by_series_id(series_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        for row in rows {
            row.delete().exec(&mut db).await.map_err(db_err)?;
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct ToastyComicReadingRepository {
    db: Db,
}

impl ToastyComicReadingRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    fn ch_key(user_id: UserId, chapter_id: ChapterId) -> String {
        format!("{}:{}", user_id.0, chapter_id.0)
    }

    fn series_key(user_id: UserId, series_id: SeriesId) -> String {
        format!("{}:{}", user_id.0, series_id.0)
    }

    fn progress_to_domain(m: ComicChapterProgressModel) -> ChapterProgress {
        ChapterProgress {
            user_id: UserId(m.user_id),
            series_id: SeriesId(m.series_id),
            chapter_id: ChapterId(m.chapter_id),
            current_page: u32::try_from(m.current_page.max(0)).unwrap_or(1),
            total_pages: u32::try_from(m.total_pages.max(1)).unwrap_or(1),
            is_completed: m.is_completed,
            updated_at: m.updated_at,
        }
    }

    fn shelf_to_domain(m: ComicShelfModel) -> ComicShelf {
        ComicShelf {
            id: ComicShelfId(m.id),
            user_id: UserId(m.user_id),
            name: m.name,
            description: m.description,
            series_ids: m.series_ids.0.into_iter().map(SeriesId).collect(),
            created_at: m.created_at,
            updated_at: m.updated_at,
        }
    }

    fn bookmark_to_domain(m: ComicBookmarkModel) -> ComicBookmark {
        ComicBookmark {
            id: ComicBookmarkId(m.id),
            user_id: UserId(m.user_id),
            series_id: SeriesId(m.series_id),
            chapter_id: ChapterId(m.chapter_id),
            page_number: u32::try_from(m.page_number.max(1)).unwrap_or(1),
            title: m.title,
            created_at: m.created_at,
        }
    }
}

#[async_trait]
impl ComicReadingRepository for ToastyComicReadingRepository {
    async fn get_chapter_progress(
        &self,
        user_id: UserId,
        chapter_id: ChapterId,
    ) -> DomainResult<Option<ChapterProgress>> {
        let mut db = self.db.clone();
        let key = Self::ch_key(user_id, chapter_id);
        let row = ComicChapterProgressModel::filter_by_composite_key(&key)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(row.map(Self::progress_to_domain))
    }

    async fn list_series_progress(
        &self,
        user_id: UserId,
        series_id: SeriesId,
    ) -> DomainResult<Vec<ChapterProgress>> {
        let mut db = self.db.clone();
        let rows = ComicChapterProgressModel::filter_by_user_id(user_id.0)
            .filter(
                ComicChapterProgressModel::fields()
                    .series_id()
                    .eq(series_id.0),
            )
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(rows.into_iter().map(Self::progress_to_domain).collect())
    }

    async fn list_user_progress(&self, user_id: UserId) -> DomainResult<Vec<ChapterProgress>> {
        let mut db = self.db.clone();
        let rows = ComicChapterProgressModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(rows.into_iter().map(Self::progress_to_domain).collect())
    }

    async fn save_chapter_progress(&self, p: &ChapterProgress) -> DomainResult<()> {
        let mut db = self.db.clone();
        let key = Self::ch_key(p.user_id, p.chapter_id);
        let existing = ComicChapterProgressModel::filter_by_composite_key(&key)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;

        if let Some(mut record) = existing {
            record
                .update()
                .current_page(i64::from(p.current_page))
                .total_pages(i64::from(p.total_pages))
                .is_completed(p.is_completed)
                .updated_at(p.updated_at)
                .exec(&mut db)
                .await
                .map_err(db_err)?;
        } else {
            toasty::create!(ComicChapterProgressModel {
                composite_key: key,
                user_id: p.user_id.0,
                series_id: p.series_id.0,
                chapter_id: p.chapter_id.0,
                current_page: i64::from(p.current_page),
                total_pages: i64::from(p.total_pages),
                is_completed: p.is_completed,
                updated_at: p.updated_at,
            })
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        }
        Ok(())
    }

    async fn delete_chapter_progress(
        &self,
        user_id: UserId,
        chapter_id: ChapterId,
    ) -> DomainResult<()> {
        let mut db = self.db.clone();
        let key = Self::ch_key(user_id, chapter_id);
        ComicChapterProgressModel::filter_by_composite_key(&key)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    async fn delete_by_series(&self, series_id: SeriesId) -> DomainResult<()> {
        let mut db = self.db.clone();
        for row in ComicChapterProgressModel::filter_by_series_id(series_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?
        {
            row.delete().exec(&mut db).await.map_err(db_err)?;
        }
        for row in ComicSeriesReaderPrefModel::filter_by_series_id(series_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?
        {
            row.delete().exec(&mut db).await.map_err(db_err)?;
        }
        for row in ComicFavoriteSeriesModel::filter_by_series_id(series_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?
        {
            row.delete().exec(&mut db).await.map_err(db_err)?;
        }
        for row in ComicBookmarkModel::filter_by_series_id(series_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?
        {
            row.delete().exec(&mut db).await.map_err(db_err)?;
        }
        for mut shelf in ComicShelfModel::all().exec(&mut db).await.map_err(db_err)? {
            if shelf.series_ids.0.contains(&series_id.0) {
                let filtered: Vec<Uuid> = shelf
                    .series_ids
                    .0
                    .iter()
                    .copied()
                    .filter(|&id| id != series_id.0)
                    .collect();
                shelf
                    .update()
                    .series_ids(toasty::Json(filtered))
                    .updated_at(Timestamp::now())
                    .exec(&mut db)
                    .await
                    .map_err(db_err)?;
            }
        }
        Ok(())
    }

    async fn delete_by_user(&self, user_id: UserId) -> DomainResult<()> {
        let mut db = self.db.clone();
        for row in ComicChapterProgressModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?
        {
            row.delete().exec(&mut db).await.map_err(db_err)?;
        }
        for row in ComicSeriesReaderPrefModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?
        {
            row.delete().exec(&mut db).await.map_err(db_err)?;
        }
        for row in ComicFavoriteSeriesModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?
        {
            row.delete().exec(&mut db).await.map_err(db_err)?;
        }
        for row in ComicShelfModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?
        {
            row.delete().exec(&mut db).await.map_err(db_err)?;
        }
        for row in ComicBookmarkModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?
        {
            row.delete().exec(&mut db).await.map_err(db_err)?;
        }
        Ok(())
    }

    async fn get_series_pref(
        &self,
        user_id: UserId,
        series_id: SeriesId,
    ) -> DomainResult<Option<SeriesReaderPref>> {
        let mut db = self.db.clone();
        let key = Self::series_key(user_id, series_id);
        let row = ComicSeriesReaderPrefModel::filter_by_composite_key(&key)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(row.map(|r| r.pref.0))
    }

    async fn save_series_pref(&self, pref: &SeriesReaderPref) -> DomainResult<()> {
        let mut db = self.db.clone();
        let key = Self::series_key(pref.user_id, pref.series_id);
        let existing = ComicSeriesReaderPrefModel::filter_by_composite_key(&key)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;

        if let Some(mut record) = existing {
            record
                .update()
                .pref(toasty::Json(pref.clone()))
                .updated_at(pref.updated_at)
                .exec(&mut db)
                .await
                .map_err(db_err)?;
        } else {
            toasty::create!(ComicSeriesReaderPrefModel {
                composite_key: key,
                user_id: pref.user_id.0,
                series_id: pref.series_id.0,
                pref: toasty::Json(pref.clone()),
                updated_at: pref.updated_at,
            })
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        }
        Ok(())
    }

    async fn is_favorite(&self, user_id: UserId, series_id: SeriesId) -> DomainResult<bool> {
        let mut db = self.db.clone();
        let key = Self::series_key(user_id, series_id);
        let row = ComicFavoriteSeriesModel::filter_by_composite_key(&key)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(row.is_some())
    }

    async fn list_favorites(&self, user_id: UserId) -> DomainResult<Vec<FavoriteSeries>> {
        let mut db = self.db.clone();
        let mut rows = ComicFavoriteSeriesModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        rows.sort_by_key(|a| std::cmp::Reverse(a.created_at));
        Ok(rows
            .into_iter()
            .map(|r| FavoriteSeries {
                user_id: UserId(r.user_id),
                series_id: SeriesId(r.series_id),
                created_at: r.created_at,
            })
            .collect())
    }

    async fn add_favorite(&self, fav: &FavoriteSeries) -> DomainResult<()> {
        let mut db = self.db.clone();
        let key = Self::series_key(fav.user_id, fav.series_id);
        if ComicFavoriteSeriesModel::filter_by_composite_key(&key)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?
            .is_none()
        {
            toasty::create!(ComicFavoriteSeriesModel {
                composite_key: key,
                user_id: fav.user_id.0,
                series_id: fav.series_id.0,
                created_at: fav.created_at,
            })
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        }
        Ok(())
    }

    async fn remove_favorite(&self, user_id: UserId, series_id: SeriesId) -> DomainResult<()> {
        let mut db = self.db.clone();
        let key = Self::series_key(user_id, series_id);
        ComicFavoriteSeriesModel::filter_by_composite_key(&key)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    async fn find_shelf(&self, id: ComicShelfId) -> DomainResult<Option<ComicShelf>> {
        let mut db = self.db.clone();
        let row = ComicShelfModel::filter_by_id(id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(row.map(Self::shelf_to_domain))
    }

    async fn list_shelves(&self, user_id: UserId) -> DomainResult<Vec<ComicShelf>> {
        let mut db = self.db.clone();
        let mut rows = ComicShelfModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        rows.sort_by_key(|a| a.created_at);
        Ok(rows.into_iter().map(Self::shelf_to_domain).collect())
    }

    async fn save_shelf(&self, shelf: &ComicShelf) -> DomainResult<()> {
        let mut db = self.db.clone();
        let existing = ComicShelfModel::filter_by_id(shelf.id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        let raw_ids: Vec<Uuid> = shelf.series_ids.iter().map(|s| s.0).collect();

        if let Some(mut record) = existing {
            record
                .update()
                .name(shelf.name.clone())
                .description(shelf.description.clone())
                .series_ids(toasty::Json(raw_ids))
                .updated_at(shelf.updated_at)
                .exec(&mut db)
                .await
                .map_err(db_err)?;
        } else {
            toasty::create!(ComicShelfModel {
                id: shelf.id.0,
                user_id: shelf.user_id.0,
                name: shelf.name.clone(),
                description: shelf.description.clone(),
                series_ids: toasty::Json(raw_ids),
                created_at: shelf.created_at,
                updated_at: shelf.updated_at,
            })
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        }
        Ok(())
    }

    async fn delete_shelf(&self, id: ComicShelfId) -> DomainResult<()> {
        let mut db = self.db.clone();
        ComicShelfModel::filter_by_id(id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    async fn list_chapter_bookmarks(
        &self,
        user_id: UserId,
        chapter_id: ChapterId,
    ) -> DomainResult<Vec<ComicBookmark>> {
        let mut db = self.db.clone();
        let mut rows = ComicBookmarkModel::filter_by_user_id(user_id.0)
            .filter(ComicBookmarkModel::fields().chapter_id().eq(chapter_id.0))
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        rows.sort_by_key(|a| a.page_number);
        Ok(rows.into_iter().map(Self::bookmark_to_domain).collect())
    }

    async fn list_user_bookmarks(&self, user_id: UserId) -> DomainResult<Vec<ComicBookmark>> {
        let mut db = self.db.clone();
        let mut rows = ComicBookmarkModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        rows.sort_by_key(|a| std::cmp::Reverse(a.created_at));
        Ok(rows.into_iter().map(Self::bookmark_to_domain).collect())
    }

    async fn save_bookmark(&self, bookmark: &ComicBookmark) -> DomainResult<()> {
        let mut db = self.db.clone();
        toasty::create!(ComicBookmarkModel {
            id: bookmark.id.0,
            user_id: bookmark.user_id.0,
            series_id: bookmark.series_id.0,
            chapter_id: bookmark.chapter_id.0,
            page_number: i64::from(bookmark.page_number),
            title: bookmark.title.clone(),
            created_at: bookmark.created_at,
        })
        .exec(&mut db)
        .await
        .map_err(db_err)?;
        Ok(())
    }

    async fn delete_bookmark(
        &self,
        user_id: UserId,
        bookmark_id: ComicBookmarkId,
    ) -> DomainResult<()> {
        let mut db = self.db.clone();
        if let Some(record) = ComicBookmarkModel::filter_by_id(bookmark_id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?
            && record.user_id == user_id.0
        {
            record.delete().exec(&mut db).await.map_err(db_err)?;
        }
        Ok(())
    }
}

use async_trait::async_trait;
use jiff::Timestamp;
use toasty::Db;
use uuid::Uuid;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::domain::user::UserId;

use crate::domain::id::{ChapterId, ComicBookmarkId, ComicShelfId, SeriesId};
use crate::domain::reading::{
    ChapterProgress, ComicBookmark, ComicShelf, FavoriteSeries, SeriesReaderPref,
};
use crate::domain::repository::ComicReadingRepository;

use super::super::models::{
    ComicBookmarkModel, ComicChapterProgressModel, ComicFavoriteSeriesModel,
    ComicSeriesReaderPrefModel, ComicShelfModel,
};
use super::db_err;

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

    async fn delete_by_chapter(&self, chapter_id: ChapterId) -> DomainResult<()> {
        let mut db = self.db.clone();
        ComicChapterProgressModel::filter_by_chapter_id(chapter_id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        ComicBookmarkModel::filter_by_chapter_id(chapter_id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    async fn delete_by_series(&self, series_id: SeriesId) -> DomainResult<()> {
        let mut db = self.db.clone();
        ComicChapterProgressModel::filter_by_series_id(series_id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        ComicSeriesReaderPrefModel::filter_by_series_id(series_id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        ComicFavoriteSeriesModel::filter_by_series_id(series_id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        ComicBookmarkModel::filter_by_series_id(series_id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
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
        ComicChapterProgressModel::filter_by_user_id(user_id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        ComicSeriesReaderPrefModel::filter_by_user_id(user_id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        ComicFavoriteSeriesModel::filter_by_user_id(user_id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        ComicShelfModel::filter_by_user_id(user_id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        ComicBookmarkModel::filter_by_user_id(user_id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    async fn delete_by_user_and_series(
        &self,
        user_id: UserId,
        series_id: SeriesId,
    ) -> DomainResult<()> {
        let mut db = self.db.clone();
        let progs = ComicChapterProgressModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        for p in progs {
            if p.series_id == series_id.0 {
                p.delete().exec(&mut db).await.map_err(db_err)?;
            }
        }
        let pref_key = Self::series_key(user_id, series_id);
        ComicSeriesReaderPrefModel::filter_by_composite_key(&pref_key)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        ComicFavoriteSeriesModel::filter_by_composite_key(&pref_key)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        let bookmarks = ComicBookmarkModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        for bm in bookmarks {
            if bm.series_id == series_id.0 {
                bm.delete().exec(&mut db).await.map_err(db_err)?;
            }
        }
        for mut shelf in ComicShelfModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?
        {
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

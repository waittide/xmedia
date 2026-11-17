use std::collections::HashMap;
use std::sync::Arc;

use jiff::Timestamp;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::user::{User, UserId};

use crate::application::catalog::ComicCatalogService;
use crate::application::dto::{
    ComicBookmarkItemDto, CreateComicBookmarkInput, SaveSeriesReaderPrefInput, SeriesQueryFilter,
    ShelfDetailDto, UpdateProgressInput, UpsertShelfInput,
};
use crate::domain::chapter::Chapter;
use crate::domain::id::{ChapterId, ComicBookmarkId, ComicShelfId, SeriesId};
use crate::domain::reading::{
    ChapterProgress, ComicBookmark, ComicShelf, FavoriteSeries, SeriesReaderPref,
    format_chapter_number,
};
use crate::domain::repository::{ChapterRepository, ComicReadingRepository, SeriesRepository};
use crate::domain::series::Series;

pub struct ComicReadingService {
    series: Arc<dyn SeriesRepository>,
    chapters: Arc<dyn ChapterRepository>,
    reading: Arc<dyn ComicReadingRepository>,
}

impl ComicReadingService {
    pub fn new(
        series: Arc<dyn SeriesRepository>,
        chapters: Arc<dyn ChapterRepository>,
        reading: Arc<dyn ComicReadingRepository>,
    ) -> Self {
        Self {
            series,
            chapters,
            reading,
        }
    }

    pub async fn update_progress(
        &self,
        user_id: UserId,
        chapter_id: ChapterId,
        input: UpdateProgressInput,
    ) -> DomainResult<ChapterProgress> {
        let chapter = self
            .chapters
            .find_by_id(chapter_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("chapter not found".into()))?;

        let progress = ChapterProgress::new(
            user_id,
            chapter.series_id,
            chapter.id,
            input.current_page,
            chapter.page_count,
            input.is_completed,
        );
        self.reading.save_chapter_progress(&progress).await?;
        Ok(progress)
    }

    pub async fn set_chapter_completed(
        &self,
        user_id: UserId,
        chapter_id: ChapterId,
        completed: bool,
    ) -> DomainResult<()> {
        let chapter = self
            .chapters
            .find_by_id(chapter_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("chapter not found".into()))?;
        if completed {
            let progress = ChapterProgress::new(
                user_id,
                chapter.series_id,
                chapter.id,
                chapter.page_count.max(1),
                chapter.page_count.max(1),
                Some(true),
            );
            self.reading.save_chapter_progress(&progress).await
        } else {
            self.reading
                .delete_chapter_progress(user_id, chapter_id)
                .await
        }
    }

    pub async fn mark_previous_chapters_read(
        &self,
        user_id: UserId,
        chapter_id: ChapterId,
    ) -> DomainResult<u32> {
        let target = self
            .chapters
            .find_by_id(chapter_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("chapter not found".into()))?;
        let mut chapters = self.chapters.list_by_series(target.series_id).await?;
        chapters.sort_by(|a, b| a.cmp_order(b));

        let Some(target_idx) = chapters.iter().position(|c| c.id == target.id) else {
            return Ok(0);
        };

        let mut count = 0_u32;
        for ch in chapters.into_iter().take(target_idx + 1) {
            if ch.is_missing {
                continue;
            }
            let progress = ChapterProgress::new(
                user_id,
                ch.series_id,
                ch.id,
                ch.page_count.max(1),
                ch.page_count.max(1),
                Some(true),
            );
            self.reading.save_chapter_progress(&progress).await?;
            count += 1;
        }
        Ok(count)
    }

    pub async fn mark_volume_completed(
        &self,
        user_id: UserId,
        series_id: SeriesId,
        volume_number: f64,
        completed: bool,
    ) -> DomainResult<u32> {
        let chapters = self.chapters.list_by_series(series_id).await?;
        let mut affected = 0_u32;

        for ch in chapters {
            let same_vol = ch
                .volume_number
                .is_some_and(|v| (v - volume_number).abs() < 1e-6);
            if !same_vol || ch.is_missing {
                continue;
            }
            if completed {
                let progress = ChapterProgress::new(
                    user_id,
                    ch.series_id,
                    ch.id,
                    ch.page_count.max(1),
                    ch.page_count.max(1),
                    Some(true),
                );
                self.reading.save_chapter_progress(&progress).await?;
            } else {
                self.reading.delete_chapter_progress(user_id, ch.id).await?;
            }
            affected += 1;
        }
        Ok(affected)
    }

    pub async fn mark_series_completed(
        &self,
        user_id: UserId,
        series_id: SeriesId,
        completed: bool,
    ) -> DomainResult<u32> {
        let chapters = self.chapters.list_by_series(series_id).await?;
        let mut affected = 0_u32;

        for ch in chapters {
            if ch.is_missing {
                continue;
            }
            if completed {
                let progress = ChapterProgress::new(
                    user_id,
                    ch.series_id,
                    ch.id,
                    ch.page_count.max(1),
                    ch.page_count.max(1),
                    Some(true),
                );
                self.reading.save_chapter_progress(&progress).await?;
            } else {
                self.reading.delete_chapter_progress(user_id, ch.id).await?;
            }
            affected += 1;
        }
        Ok(affected)
    }

    pub async fn save_series_reader_pref(
        &self,
        user_id: UserId,
        series_id: SeriesId,
        input: SaveSeriesReaderPrefInput,
    ) -> DomainResult<SeriesReaderPref> {
        if self.series.find_by_id(series_id).await?.is_none() {
            return Err(DomainError::NotFound("series not found".into()));
        }
        let pref = SeriesReaderPref {
            user_id,
            series_id,
            reader_mode: input.reader_mode,
            fit_mode: input.fit_mode,
            offset_cover_page: input.offset_cover_page,
            webtoon_width: input.webtoon_width.clamp(360, 2400),
            updated_at: Timestamp::now(),
        };
        self.reading.save_series_pref(&pref).await?;
        Ok(pref)
    }

    pub async fn set_favorite(
        &self,
        user_id: UserId,
        series_id: SeriesId,
        favorite: bool,
    ) -> DomainResult<()> {
        if self.series.find_by_id(series_id).await?.is_none() {
            return Err(DomainError::NotFound("series not found".into()));
        }
        if favorite {
            if !self.reading.is_favorite(user_id, series_id).await? {
                self.reading
                    .add_favorite(&FavoriteSeries {
                        user_id,
                        series_id,
                        created_at: Timestamp::now(),
                    })
                    .await?;
            }
        } else {
            self.reading.remove_favorite(user_id, series_id).await?;
        }
        Ok(())
    }

    pub async fn list_shelves(
        &self,
        user: &User,
        catalog: &ComicCatalogService,
    ) -> DomainResult<Vec<ShelfDetailDto>> {
        let mut shelves = self.reading.list_shelves(user.id).await?;
        shelves.sort_by_key(|s| s.created_at);
        let mut out = Vec::with_capacity(shelves.len());

        for shelf in shelves {
            let filter = SeriesQueryFilter {
                shelf_id: Some(shelf.id),
                ..Default::default()
            };
            let series = catalog.list_series(user, filter).await?;
            out.push(ShelfDetailDto { shelf, series });
        }
        Ok(out)
    }

    pub async fn create_shelf(
        &self,
        user_id: UserId,
        input: UpsertShelfInput,
    ) -> DomainResult<ComicShelf> {
        let mut shelf =
            ComicShelf::new(user_id, input.name, input.description.unwrap_or_default())?;
        if let Some(ids) = input.series_ids {
            shelf.series_ids = ids;
        }
        self.reading.save_shelf(&shelf).await?;
        Ok(shelf)
    }

    pub async fn update_shelf(
        &self,
        user_id: UserId,
        shelf_id: ComicShelfId,
        input: UpsertShelfInput,
    ) -> DomainResult<ComicShelf> {
        let mut shelf = self
            .reading
            .find_shelf(shelf_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("shelf not found".into()))?;
        if shelf.user_id != user_id {
            return Err(DomainError::Forbidden("access denied to shelf".into()));
        }
        let clean_name = input.name.trim();
        if clean_name.is_empty() {
            return Err(DomainError::Validation("shelf name cannot be empty".into()));
        }
        shelf.name = clean_name.to_owned();
        if let Some(desc) = input.description {
            shelf.description = desc.trim().to_owned();
        }
        if let Some(ids) = input.series_ids {
            shelf.series_ids = ids;
        }
        shelf.updated_at = Timestamp::now();
        self.reading.save_shelf(&shelf).await?;
        Ok(shelf)
    }

    pub async fn delete_shelf(&self, user_id: UserId, shelf_id: ComicShelfId) -> DomainResult<()> {
        let shelf = self
            .reading
            .find_shelf(shelf_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("shelf not found".into()))?;
        if shelf.user_id != user_id {
            return Err(DomainError::Forbidden("access denied to shelf".into()));
        }
        self.reading.delete_shelf(shelf_id).await
    }

    pub async fn set_series_shelves(
        &self,
        user_id: UserId,
        series_id: SeriesId,
        target_shelf_ids: &[ComicShelfId],
    ) -> DomainResult<()> {
        let shelves = self.reading.list_shelves(user_id).await?;
        let now = Timestamp::now();

        for mut shelf in shelves {
            let should_contain = target_shelf_ids.contains(&shelf.id);
            let currently_contains = shelf.series_ids.contains(&series_id);
            if should_contain && !currently_contains {
                shelf.series_ids.push(series_id);
                shelf.updated_at = now;
                self.reading.save_shelf(&shelf).await?;
            } else if !should_contain && currently_contains {
                shelf.series_ids.retain(|&s| s != series_id);
                shelf.updated_at = now;
                self.reading.save_shelf(&shelf).await?;
            }
        }
        Ok(())
    }

    pub async fn list_chapter_bookmarks(
        &self,
        user_id: UserId,
        chapter_id: ChapterId,
    ) -> DomainResult<Vec<ComicBookmark>> {
        self.reading
            .list_chapter_bookmarks(user_id, chapter_id)
            .await
    }

    pub async fn list_all_bookmarks(
        &self,
        user_id: UserId,
    ) -> DomainResult<Vec<ComicBookmarkItemDto>> {
        let bookmarks = self.reading.list_user_bookmarks(user_id).await?;
        if bookmarks.is_empty() {
            return Ok(Vec::new());
        }
        let all_series = self.series.list_all().await?;
        let series_map: HashMap<SeriesId, Series> =
            all_series.into_iter().map(|s| (s.id, s)).collect();

        let mut chapter_cache: HashMap<ChapterId, Option<Chapter>> = HashMap::new();
        let mut out = Vec::with_capacity(bookmarks.len());

        for bm in bookmarks {
            let Some(series) = series_map.get(&bm.series_id) else {
                continue;
            };
            let chapter_opt = if let Some(cached) = chapter_cache.get(&bm.chapter_id) {
                cached.clone()
            } else {
                let found = self.chapters.find_by_id(bm.chapter_id).await?;
                chapter_cache.insert(bm.chapter_id, found.clone());
                found
            };
            let Some(ch) = chapter_opt else {
                continue;
            };

            let cover_url = series
                .cover_path
                .as_ref()
                .or(ch.cover_path.as_ref())
                .map(|key| format!("/api/v1/comic/covers/{key}"));

            out.push(ComicBookmarkItemDto {
                id: bm.id,
                bookmark: bm,
                series_title: series.title.clone(),
                series_cover_url: cover_url,
                chapter_number_label: format_chapter_number(ch.number),
                chapter_title: ch.title,
                total_pages: ch.page_count.max(1),
            });
        }
        Ok(out)
    }

    pub async fn create_bookmark(
        &self,
        user_id: UserId,
        chapter_id: ChapterId,
        input: CreateComicBookmarkInput,
    ) -> DomainResult<ComicBookmark> {
        let chapter = self
            .chapters
            .find_by_id(chapter_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("chapter not found".into()))?;
        let page = input.page_number.clamp(1, chapter.page_count.max(1));
        let default_title = format!("#{} P.{}", format_chapter_number(chapter.number), page);
        let title = input
            .title
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .unwrap_or(default_title);
        let bookmark = ComicBookmark::new(user_id, chapter.series_id, chapter.id, page, title);
        self.reading.save_bookmark(&bookmark).await?;
        Ok(bookmark)
    }

    pub async fn delete_bookmark(
        &self,
        user_id: UserId,
        bookmark_id: ComicBookmarkId,
    ) -> DomainResult<()> {
        self.reading.delete_bookmark(user_id, bookmark_id).await
    }
}

use std::collections::HashMap;
use std::sync::Arc;

use jiff::Timestamp;
use xmedia_core::application::bindings::MediaBindingService;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;
use xmedia_core::domain::user::{User, UserId};

use crate::application::catalog::ComicCatalogService;
use crate::application::dto::{
    ComicBookmarkItemDto, CreateComicBookmarkInput, SaveSeriesReaderPrefInput, SeriesQueryFilter,
    ShelfDetailDto, UpdateProgressInput, UpsertShelfInput,
};
use crate::domain::chapter::Chapter;
use crate::domain::id::{ChapterId, ComicBookmarkId, ComicShelfId, SeriesId};
use crate::domain::reading::{
    BatchReadScope, ChapterProgress, ComicBookmark, ComicShelf, SeriesReaderPref,
};
use crate::domain::repository::{
    ChapterQuery, ChapterRepository, ComicReadingDeleteScope, ComicReadingMutation,
    ComicReadingRepository, SeriesQuery, SeriesRepository,
};
use crate::domain::series::Series;

pub struct ComicReadingService {
    series: Arc<dyn SeriesRepository>,
    chapters: Arc<dyn ChapterRepository>,
    reading: Arc<dyn ComicReadingRepository>,
    bindings: Arc<MediaBindingService>,
}

impl ComicReadingService {
    pub fn new(
        series: Arc<dyn SeriesRepository>,
        chapters: Arc<dyn ChapterRepository>,
        reading: Arc<dyn ComicReadingRepository>,
        bindings: Arc<MediaBindingService>,
    ) -> Self {
        Self {
            series,
            chapters,
            reading,
            bindings,
        }
    }

    async fn require_series_bound(&self, user_id: UserId, series_id: SeriesId) -> DomainResult<()> {
        self.bindings
            .require_bound(user_id, MediaKind::Comic, series_id.0, "series not found")
            .await
    }

    async fn load_chapter(&self, chapter_id: ChapterId) -> DomainResult<Chapter> {
        self.chapters
            .query(ChapterQuery::Id(chapter_id))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound("chapter not found".into()))
    }

    async fn require_series_exists(&self, series_id: SeriesId) -> DomainResult<()> {
        if self
            .series
            .query(SeriesQuery::Id(series_id))
            .await?
            .is_empty()
        {
            return Err(DomainError::NotFound("series not found".into()));
        }
        Ok(())
    }

    async fn load_owned_shelf(
        &self,
        user_id: UserId,
        shelf_id: ComicShelfId,
    ) -> DomainResult<ComicShelf> {
        let shelf = self
            .reading
            .query_shelves(None, Some(shelf_id))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound("shelf not found".into()))?;
        if shelf.user_id != user_id {
            return Err(DomainError::Forbidden("access denied to shelf".into()));
        }
        Ok(shelf)
    }

    async fn apply_batch_completion(
        &self,
        user_id: UserId,
        series_id: SeriesId,
        scope: BatchReadScope,
        completed: bool,
    ) -> DomainResult<u32> {
        self.require_series_bound(user_id, series_id).await?;
        let chapters = self.chapters.query(ChapterQuery::Series(series_id)).await?;
        let selected = ChapterProgress::select_batch(&chapters, scope);
        let mut affected = 0_u32;

        for ch in selected {
            if completed {
                let progress = ChapterProgress::new(
                    user_id,
                    ch.series_id,
                    ch.id,
                    ch.page_count.max(1),
                    ch.page_count.max(1),
                    Some(true),
                );
                self.reading
                    .save(ComicReadingMutation::Progress(&progress))
                    .await?;
            } else {
                self.reading
                    .delete_scope(ComicReadingDeleteScope::ChapterProgress {
                        user_id,
                        chapter_id: ch.id,
                    })
                    .await?;
            }
            affected += 1;
        }
        Ok(affected)
    }

    pub async fn update_progress(
        &self,
        user_id: UserId,
        chapter_id: ChapterId,
        input: UpdateProgressInput,
    ) -> DomainResult<ChapterProgress> {
        let chapter = self.load_chapter(chapter_id).await?;
        self.require_series_bound(user_id, chapter.series_id)
            .await?;

        let progress = ChapterProgress::new(
            user_id,
            chapter.series_id,
            chapter.id,
            input.current_page,
            chapter.page_count,
            input.is_completed,
        );
        self.reading
            .save(ComicReadingMutation::Progress(&progress))
            .await?;
        Ok(progress)
    }

    pub async fn set_chapter_completed(
        &self,
        user_id: UserId,
        chapter_id: ChapterId,
        completed: bool,
    ) -> DomainResult<()> {
        let chapter = self.load_chapter(chapter_id).await?;
        self.require_series_bound(user_id, chapter.series_id)
            .await?;
        if completed {
            let progress = ChapterProgress::new(
                user_id,
                chapter.series_id,
                chapter.id,
                chapter.page_count.max(1),
                chapter.page_count.max(1),
                Some(true),
            );
            self.reading
                .save(ComicReadingMutation::Progress(&progress))
                .await
        } else {
            self.reading
                .delete_scope(ComicReadingDeleteScope::ChapterProgress {
                    user_id,
                    chapter_id,
                })
                .await
        }
    }

    pub async fn mark_previous_chapters_read(
        &self,
        user_id: UserId,
        chapter_id: ChapterId,
    ) -> DomainResult<u32> {
        let target = self.load_chapter(chapter_id).await?;
        self.apply_batch_completion(
            user_id,
            target.series_id,
            BatchReadScope::UpToChapter(target.id),
            true,
        )
        .await
    }

    pub async fn mark_volume_completed(
        &self,
        user_id: UserId,
        series_id: SeriesId,
        volume_number: f64,
        completed: bool,
    ) -> DomainResult<u32> {
        self.apply_batch_completion(
            user_id,
            series_id,
            BatchReadScope::Volume(volume_number),
            completed,
        )
        .await
    }

    pub async fn mark_series_completed(
        &self,
        user_id: UserId,
        series_id: SeriesId,
        completed: bool,
    ) -> DomainResult<u32> {
        self.apply_batch_completion(user_id, series_id, BatchReadScope::All, completed)
            .await
    }

    pub async fn save_series_reader_pref(
        &self,
        user_id: UserId,
        series_id: SeriesId,
        input: SaveSeriesReaderPrefInput,
    ) -> DomainResult<SeriesReaderPref> {
        self.require_series_bound(user_id, series_id).await?;
        self.require_series_exists(series_id).await?;
        let pref = SeriesReaderPref {
            user_id,
            series_id,
            reader_mode: input.reader_mode,
            fit_mode: input.fit_mode,
            offset_cover_page: input.offset_cover_page,
            webtoon_width: input.webtoon_width.clamp(360, 2400),
            updated_at: Timestamp::now(),
        };
        self.reading
            .save(ComicReadingMutation::SeriesPref(&pref))
            .await?;
        Ok(pref)
    }

    pub async fn set_favorite(
        &self,
        user_id: UserId,
        series_id: SeriesId,
        favorite: bool,
    ) -> DomainResult<()> {
        self.require_series_bound(user_id, series_id).await?;
        self.require_series_exists(series_id).await?;
        self.reading
            .save(ComicReadingMutation::Favorite {
                user_id,
                series_id,
                favorite,
                now: Timestamp::now(),
            })
            .await
    }

    pub async fn list_shelves(
        &self,
        user: &User,
        catalog: &ComicCatalogService,
    ) -> DomainResult<Vec<ShelfDetailDto>> {
        let mut shelves = self.reading.query_shelves(Some(user.id), None).await?;
        if shelves.is_empty() {
            return Ok(Vec::new());
        }
        shelves.sort_by_key(|s| s.created_at);

        let all_summaries = catalog
            .list_series(user, SeriesQueryFilter::default())
            .await?;
        let by_id: HashMap<SeriesId, _> = all_summaries.into_iter().map(|s| (s.id, s)).collect();

        let mut out = Vec::with_capacity(shelves.len());
        for shelf in shelves {
            let mut series: Vec<_> = shelf
                .series_ids
                .iter()
                .filter_map(|sid| by_id.get(sid).cloned())
                .collect();
            series.sort_by_key(|s| std::cmp::Reverse(s.updated_at));
            out.push(ShelfDetailDto { shelf, series });
        }
        Ok(out)
    }

    pub async fn create_shelf(
        &self,
        user_id: UserId,
        input: UpsertShelfInput,
    ) -> DomainResult<ComicShelf> {
        let filtered_ids = if let Some(ids) = input.series_ids {
            let bound = self
                .bindings
                .list_bound_ids(user_id, MediaKind::Comic)
                .await?;
            ids.into_iter()
                .filter(|sid| bound.contains(&sid.0))
                .collect()
        } else {
            Vec::new()
        };
        let shelf = ComicShelf::new(
            user_id,
            input.name,
            input.description.unwrap_or_default(),
            filtered_ids,
        )?;
        self.reading
            .save(ComicReadingMutation::Shelf(&shelf))
            .await?;
        Ok(shelf)
    }

    pub async fn update_shelf(
        &self,
        user_id: UserId,
        shelf_id: ComicShelfId,
        input: UpsertShelfInput,
    ) -> DomainResult<ComicShelf> {
        let mut shelf = self.load_owned_shelf(user_id, shelf_id).await?;
        let filtered_ids = if let Some(ids) = input.series_ids {
            let bound = self
                .bindings
                .list_bound_ids(user_id, MediaKind::Comic)
                .await?;
            Some(
                ids.into_iter()
                    .filter(|sid| bound.contains(&sid.0))
                    .collect(),
            )
        } else {
            None
        };
        shelf.update(
            input.name,
            input.description,
            filtered_ids,
            Timestamp::now(),
        )?;
        self.reading
            .save(ComicReadingMutation::Shelf(&shelf))
            .await?;
        Ok(shelf)
    }

    pub async fn delete_shelf(&self, user_id: UserId, shelf_id: ComicShelfId) -> DomainResult<()> {
        let _ = self.load_owned_shelf(user_id, shelf_id).await?;
        self.reading
            .delete_scope(ComicReadingDeleteScope::Shelf(shelf_id))
            .await
    }

    pub async fn set_series_shelves(
        &self,
        user_id: UserId,
        series_id: SeriesId,
        target_shelf_ids: &[ComicShelfId],
    ) -> DomainResult<()> {
        self.require_series_bound(user_id, series_id).await?;
        let shelves = self.reading.query_shelves(Some(user_id), None).await?;
        let now = Timestamp::now();

        for mut shelf in shelves {
            let contains = target_shelf_ids.contains(&shelf.id);
            if shelf.set_contains(series_id, contains, now) {
                self.reading
                    .save(ComicReadingMutation::Shelf(&shelf))
                    .await?;
            }
        }
        Ok(())
    }

    pub async fn list_chapter_bookmarks(
        &self,
        user_id: UserId,
        chapter_id: ChapterId,
    ) -> DomainResult<Vec<ComicBookmark>> {
        let chapter = self.load_chapter(chapter_id).await?;
        self.require_series_bound(user_id, chapter.series_id)
            .await?;
        self.reading.list_bookmarks(user_id, Some(chapter_id)).await
    }

    pub async fn list_all_bookmarks(
        &self,
        user_id: UserId,
    ) -> DomainResult<Vec<ComicBookmarkItemDto>> {
        let bound = self
            .bindings
            .list_bound_ids(user_id, MediaKind::Comic)
            .await?;
        if bound.is_empty() {
            return Ok(Vec::new());
        }
        let bookmarks: Vec<_> = self
            .reading
            .list_bookmarks(user_id, None)
            .await?
            .into_iter()
            .filter(|bm| bound.contains(&bm.series_id.0))
            .collect();
        if bookmarks.is_empty() {
            return Ok(Vec::new());
        }
        let all_series = self.series.query(SeriesQuery::All).await?;
        let series_map: HashMap<SeriesId, Series> = all_series
            .into_iter()
            .filter(|s| bound.contains(&s.id.0))
            .map(|s| (s.id, s))
            .collect();

        let mut chapter_ids: Vec<ChapterId> = Vec::new();
        for bm in &bookmarks {
            if !chapter_ids.contains(&bm.chapter_id) {
                chapter_ids.push(bm.chapter_id);
            }
        }
        let fetched_chapters = self.chapters.query(ChapterQuery::Ids(chapter_ids)).await?;
        let chapter_map: HashMap<ChapterId, Chapter> =
            fetched_chapters.into_iter().map(|c| (c.id, c)).collect();

        let mut out = Vec::with_capacity(bookmarks.len());

        for bm in bookmarks {
            let Some(series) = series_map.get(&bm.series_id) else {
                continue;
            };
            let Some(ch) = chapter_map.get(&bm.chapter_id) else {
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
                chapter_number_label: ch.number_label(),
                chapter_title: ch.title.clone(),
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
        let chapter = self.load_chapter(chapter_id).await?;
        self.require_series_bound(user_id, chapter.series_id)
            .await?;
        let page = input.page_number.clamp(1, chapter.page_count.max(1));
        let default_title = format!("#{} P.{}", chapter.number_label(), page);
        let title = input
            .title
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .unwrap_or(default_title);
        let bookmark = ComicBookmark::new(user_id, chapter.series_id, chapter.id, page, title);
        self.reading
            .save(ComicReadingMutation::Bookmark(&bookmark))
            .await?;
        Ok(bookmark)
    }

    pub async fn delete_bookmark(
        &self,
        user_id: UserId,
        bookmark_id: ComicBookmarkId,
    ) -> DomainResult<()> {
        self.reading
            .delete_scope(ComicReadingDeleteScope::Bookmark {
                user_id,
                bookmark_id,
            })
            .await
    }
}

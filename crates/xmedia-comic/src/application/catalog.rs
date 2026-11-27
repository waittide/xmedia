use crate::application::dto::{
    AdjacentChapterDto, ChapterItemDto, ComicLibraryStatsDto, ContinueReadingItemDto,
    EffectiveReaderPrefDto, HomeOverviewDto, OnlineTrackedSeriesDto, PoolSeriesSummaryDto,
    ReaderContextDto, SeriesDetailDto, SeriesQueryFilter, SeriesSummaryDto, TaxonomyCountDto,
    TaxonomyOverviewDto, UploadArchiveInput, UploadArchivePathInput, UploadArchiveResultDto,
};
use crate::application::ports::{
    ArchiveInspection, ArchiveReaderPort, ComicFileStoragePort, PageContent, StoredUploadFile,
    ThumbnailGeneratorPort,
};
use crate::domain::chapter::{
    Chapter, ChapterMetadataPatch, ChapterScanUpdate, ChapterSource, ChapterSourceKind,
};
use crate::domain::id::{ChapterId, ComicShelfId, SeriesId};
use crate::domain::reading::{ChapterProgress, SeriesReadingSummary};
use crate::domain::repository::{
    ChapterDeleteScope, ChapterQuery, ChapterRepository, ComicProgressScope,
    ComicReadingDeleteScope, ComicReadingRepository, SeriesQuery, SeriesRepository,
};
use crate::domain::series::{
    CoverUpdate, ReadingDirection, Series, SeriesCriteria, SeriesMetadataPatch, SeriesSource,
};
use jiff::Timestamp;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use uuid::Uuid;
use xmedia_core::application::bindings::{
    BindMediaItemsInput, BindMediaItemsResultDto, MediaBindingService, UnbindMediaItemResultDto,
};
use xmedia_core::application::ports::{
    ByteRangeSpec, ByteRangeStream, RealtimeEventPort, SystemEvent,
};
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::{Library, LibraryId, MediaKind};
use xmedia_core::domain::repository::LibraryRepository;
use xmedia_core::domain::user::{User, UserId};
use xmedia_formats::comic::ComicFormatExt;
use xmedia_formats::comic::filename::parse_comic_chapter_filename;
use xmedia_formats::common::natural_cmp;

pub struct ComicCatalogService {
    libraries: Arc<dyn LibraryRepository>,
    series: Arc<dyn SeriesRepository>,
    chapters: Arc<dyn ChapterRepository>,
    reading: Arc<dyn ComicReadingRepository>,
    bindings: Arc<MediaBindingService>,
    archives: Arc<dyn ArchiveReaderPort>,
    thumbnails: Arc<dyn ThumbnailGeneratorPort>,
    storage: Arc<dyn ComicFileStoragePort>,
    events: Arc<dyn RealtimeEventPort>,
    managed_lib_lock: tokio::sync::Mutex<()>,
}

impl ComicCatalogService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        libraries: Arc<dyn LibraryRepository>,
        series: Arc<dyn SeriesRepository>,
        chapters: Arc<dyn ChapterRepository>,
        reading: Arc<dyn ComicReadingRepository>,
        bindings: Arc<MediaBindingService>,
        archives: Arc<dyn ArchiveReaderPort>,
        thumbnails: Arc<dyn ThumbnailGeneratorPort>,
        storage: Arc<dyn ComicFileStoragePort>,
        events: Arc<dyn RealtimeEventPort>,
    ) -> Self {
        Self {
            libraries,
            series,
            chapters,
            reading,
            bindings,
            archives,
            thumbnails,
            storage,
            events,
            managed_lib_lock: tokio::sync::Mutex::new(()),
        }
    }

    pub async fn list_all_series_ids_for_backfill(&self) -> DomainResult<Vec<Uuid>> {
        Ok(self
            .series
            .query(SeriesQuery::All)
            .await?
            .into_iter()
            .map(|s| s.id.0)
            .collect())
    }

    async fn load_series(&self, series_id: SeriesId) -> DomainResult<Series> {
        self.series
            .query(SeriesQuery::Id(series_id))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound("series not found".into()))
    }

    async fn load_chapter(&self, chapter_id: ChapterId) -> DomainResult<Chapter> {
        self.chapters
            .query(ChapterQuery::Id(chapter_id))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound("chapter not found".into()))
    }

    pub(crate) async fn bind_series_for_user(
        &self,
        user_id: UserId,
        series_id: SeriesId,
    ) -> DomainResult<bool> {
        self.bindings
            .bind(user_id, MediaKind::Comic, series_id.0)
            .await
    }

    pub(crate) async fn require_series_bound(
        &self,
        user: &User,
        series_id: SeriesId,
    ) -> DomainResult<()> {
        self.bindings
            .require_bound(user.id, MediaKind::Comic, series_id.0, "series not found")
            .await
    }

    pub(crate) async fn require_chapter_bound(
        &self,
        user: &User,
        chapter_id: ChapterId,
    ) -> DomainResult<Chapter> {
        let chapter = self.load_chapter(chapter_id).await?;
        self.require_series_bound(user, chapter.series_id).await?;
        Ok(chapter)
    }

    async fn load_sorted_chapters_map(
        &self,
        series_ids: &[SeriesId],
    ) -> DomainResult<HashMap<SeriesId, Vec<Chapter>>> {
        if series_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let chapters = self
            .chapters
            .query(ChapterQuery::SeriesIds(series_ids.to_vec()))
            .await?;
        let mut map: HashMap<SeriesId, Vec<Chapter>> = HashMap::new();
        for ch in chapters {
            map.entry(ch.series_id).or_default().push(ch);
        }
        for list in map.values_mut() {
            list.sort_by(|a, b| a.cmp_order(b));
        }
        Ok(map)
    }

    fn build_series_summary_with_chapters(
        series: Series,
        progress_list: &[ChapterProgress],
        sorted_chapters: &[Chapter],
        is_favorite: bool,
    ) -> SeriesSummaryDto {
        let reading = SeriesReadingSummary::compute(
            series.id,
            series.chapter_count,
            sorted_chapters,
            progress_list,
            None,
        )
        .unwrap_or_default();
        Self::into_series_summary(series, reading, is_favorite)
    }

    fn into_series_summary(
        series: Series,
        reading: SeriesReadingSummary,
        is_favorite: bool,
    ) -> SeriesSummaryDto {
        let cover_url = series
            .cover_path
            .as_ref()
            .map(|key| format!("/api/v1/comic/covers/{key}"));
        let online_tracking = series
            .source
            .online_tracking()
            .map(|info| OnlineTrackedSeriesDto {
                source_id: info.source_id,
                comic_url: info.comic_url.clone(),
                toc_url: info.toc_url.clone(),
            });

        SeriesSummaryDto {
            id: series.id,
            library_id: series.library_id,
            title: series.title,
            writers: series.writers,
            pencillers: series.pencillers,
            status: series.status,
            reading_direction: series.reading_direction,
            age_rating: series.age_rating,
            genres: series.genres,
            tags: series.tags,
            cover_url,
            chapter_count: series.chapter_count,
            completed_chapters: reading.completed_chapters,
            is_favorite,
            continue_reading: reading.continue_reading,
            online_tracking,
            created_at: series.created_at,
            updated_at: series.updated_at,
        }
    }

    pub(crate) async fn get_or_create_managed_library(&self) -> DomainResult<Library> {
        let _guard = self.managed_lib_lock.lock().await;
        if let Some(existing) = self
            .libraries
            .find_managed_by_media(MediaKind::Comic)
            .await?
        {
            return Ok(existing);
        }
        let _ = self.storage.ensure_managed_root().await?;
        let lib = Library::new_managed(MediaKind::Comic)?;
        self.libraries.save(&lib).await?;
        Ok(lib)
    }
}

fn sanitize_folder_name(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            other => other,
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.');
    if trimmed.is_empty() {
        "untitled_series".into()
    } else {
        trimmed.to_owned()
    }
}

fn to_taxonomy_dtos(items: Vec<(String, u32)>) -> Vec<TaxonomyCountDto> {
    items
        .into_iter()
        .map(|(name, count)| TaxonomyCountDto { name, count })
        .collect()
}

impl ComicCatalogService {
    pub async fn update_series_metadata(
        &self,
        actor: &User,
        series_id: SeriesId,
        patch: SeriesMetadataPatch,
    ) -> DomainResult<Series> {
        actor.ensure_admin()?;
        let mut series = self.load_series(series_id).await?;
        series.patch_metadata(patch)?;
        self.series.save(&series).await?;
        Ok(series)
    }

    pub async fn update_series_cover(
        &self,
        actor: &User,
        series_id: SeriesId,
        image_bytes: &[u8],
    ) -> DomainResult<Series> {
        actor.ensure_admin()?;
        let mut series = self.load_series(series_id).await?;
        let now = Timestamp::now();
        let key = format!("series_{}", series.id);
        let saved_key = self
            .thumbnails
            .generate_from_bytes(&key, image_bytes)
            .await?;
        series.apply_cover(CoverUpdate::Custom(saved_key), now);
        self.series.save(&series).await?;
        Ok(series)
    }

    pub async fn bind_series(
        &self,
        actor: &User,
        input: BindMediaItemsInput,
    ) -> DomainResult<BindMediaItemsResultDto> {
        let target_user_id = MediaBindingService::resolve_target_user(actor, input.user_id)?;
        let mut to_bind: Vec<Uuid> = Vec::new();
        if let Some(lib_uuid) = input.library_id {
            let lib_id = LibraryId(lib_uuid);
            for s in self.series.query(SeriesQuery::Library(lib_id)).await? {
                if s.source.online_tracking().is_none() {
                    to_bind.push(s.id.0);
                }
            }
        }
        for raw_id in input.item_ids {
            let Some(s) = self
                .series
                .query(SeriesQuery::Id(SeriesId(raw_id)))
                .await?
                .into_iter()
                .next()
            else {
                continue;
            };
            if s.source.online_tracking().is_some()
                && (target_user_id != actor.id
                    || !self
                        .bindings
                        .is_bound(actor.id, MediaKind::Comic, s.id.0)
                        .await?)
            {
                continue;
            }
            to_bind.push(s.id.0);
        }
        let total_requested = to_bind.len();
        let bound_count = self
            .bindings
            .bind_batch(target_user_id, MediaKind::Comic, &to_bind)
            .await?;
        Ok(BindMediaItemsResultDto {
            bound_count,
            total_requested,
        })
    }

    pub async fn unbind_series(
        &self,
        actor: &User,
        series_id: SeriesId,
        requested_user_id: Option<Uuid>,
    ) -> DomainResult<UnbindMediaItemResultDto> {
        let target_user_id = MediaBindingService::resolve_target_user(actor, requested_user_id)?;
        let (unbound, remaining_bindings, deleted_from_pool) = self
            .bindings
            .unbind_with_gc(target_user_id, MediaKind::Comic, series_id.0, || async {
                if let Some(s) = self
                    .series
                    .query(SeriesQuery::Id(series_id))
                    .await?
                    .into_iter()
                    .next()
                    && s.source.online_tracking().is_some()
                {
                    self.delete_series_without_bindings(s).await?;
                    return Ok(true);
                }
                Ok(false)
            })
            .await?;
        if unbound && !deleted_from_pool {
            self.reading
                .delete_scope(ComicReadingDeleteScope::UserSeries {
                    user_id: target_user_id,
                    series_id,
                })
                .await?;
        }
        Ok(UnbindMediaItemResultDto {
            unbound,
            remaining_bindings,
            deleted_from_pool,
        })
    }

    async fn delete_series_internal(&self, series: Series) -> DomainResult<()> {
        let _ = self
            .bindings
            .purge(xmedia_core::domain::repository::BindingDeleteScope::Item {
                media_kind: MediaKind::Comic,
                item_id: series.id.0,
            })
            .await?;
        self.delete_series_without_bindings(series).await
    }

    async fn delete_series_without_bindings(&self, series: Series) -> DomainResult<()> {
        let series_id = series.id;
        let lib = self.libraries.find_by_id(series.library_id).await?;
        let chapters = self.chapters.query(ChapterQuery::Series(series_id)).await?;

        if lib.is_some_and(|l| l.kind.is_managed()) {
            for ch in &chapters {
                if let Some((ChapterSourceKind::ArchiveFile, path)) = ch.source.resolved_local() {
                    let _ = self.storage.delete_managed_file(path).await;
                }
            }
        }
        self.reading
            .delete_scope(ComicReadingDeleteScope::Series(series_id))
            .await?;
        self.chapters
            .delete_scope(ChapterDeleteScope::Series(series_id))
            .await?;
        self.series.delete(series_id).await?;
        self.events.publish(SystemEvent::MediaChanged {
            media_kind: MediaKind::Comic,
        });
        Ok(())
    }

    pub async fn delete_series(&self, actor: &User, series_id: SeriesId) -> DomainResult<()> {
        actor.ensure_admin()?;
        let series = self.load_series(series_id).await?;
        self.delete_series_internal(series).await
    }

    pub async fn delete_chapter(&self, actor: &User, chapter_id: ChapterId) -> DomainResult<()> {
        actor.ensure_admin()?;
        let chapter = self.load_chapter(chapter_id).await?;
        let lib = self.libraries.find_by_id(chapter.library_id).await?;
        if lib.is_some_and(|l| l.kind.is_managed())
            && let Some((ChapterSourceKind::ArchiveFile, path)) = chapter.source.resolved_local()
        {
            let _ = self.storage.delete_managed_file(path).await;
        }
        self.reading
            .delete_scope(ComicReadingDeleteScope::Chapter(chapter_id))
            .await?;
        self.chapters
            .delete_scope(ChapterDeleteScope::Id(chapter_id))
            .await?;

        if let Some(mut series) = self
            .series
            .query(SeriesQuery::Id(chapter.series_id))
            .await?
            .into_iter()
            .next()
        {
            let mut remaining = self.chapters.query(ChapterQuery::Series(series.id)).await?;
            remaining.retain(|c| c.id != chapter_id);
            if remaining.is_empty() {
                let _ = self
                    .bindings
                    .purge(xmedia_core::domain::repository::BindingDeleteScope::Item {
                        media_kind: MediaKind::Comic,
                        item_id: series.id.0,
                    })
                    .await?;
                self.reading
                    .delete_scope(ComicReadingDeleteScope::Series(series.id))
                    .await?;
                self.series.delete(series.id).await?;
            } else {
                remaining.sort_by(|a, b| a.cmp_order(b));
                let now = Timestamp::now();
                let active_count = remaining.iter().filter(|c| !c.is_missing).count() as u32;
                let _ = series.sync_chapter_count(active_count, now);
                series.apply_cover(
                    CoverUpdate::ReconcileRemoved {
                        removed_cover: chapter.cover_path.as_deref(),
                        fallback_cover: remaining.iter().find_map(|c| c.cover_path.clone()),
                    },
                    now,
                );
                series.updated_at = now;
                self.series.save(&series).await?;
            }
        }

        self.events.publish(SystemEvent::MediaChanged {
            media_kind: MediaKind::Comic,
        });
        Ok(())
    }

    pub async fn get_reader_context(
        &self,
        user: &User,
        chapter_id: ChapterId,
    ) -> DomainResult<ReaderContextDto> {
        let target = self.load_chapter(chapter_id).await?;
        self.require_series_bound(user, target.series_id).await?;
        if target.is_missing {
            return Err(DomainError::NotFound(
                "chapter archive is missing on disk".into(),
            ));
        }

        let series = self
            .series
            .query(SeriesQuery::Id(target.series_id))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound("parent series not found".into()))?;

        let mut chapters = self.chapters.query(ChapterQuery::Series(series.id)).await?;
        chapters.retain(|c| !c.is_missing);
        chapters.sort_by(|a, b| a.cmp_order(b));

        let prog_list = self
            .reading
            .query_progress(user.id, ComicProgressScope::Series(series.id))
            .await?;
        let prog_map: HashMap<ChapterId, ChapterProgress> =
            prog_list.into_iter().map(|p| (p.chapter_id, p)).collect();

        let current_idx = chapters.iter().position(|c| c.id == target.id);

        let to_adjacent = |ch: &Chapter| AdjacentChapterDto {
            id: ch.id,
            kind: ch.kind,
            number: ch.number,
            number_label: ch.number_label(),
            volume_number: ch.volume_number,
            title: ch.title.clone(),
            page_count: ch.page_count,
            is_completed: prog_map.get(&ch.id).is_some_and(|p| p.is_completed),
        };

        let prev_chapter = current_idx
            .and_then(|idx| idx.checked_sub(1))
            .and_then(|prev_idx| chapters.get(prev_idx))
            .map(to_adjacent);
        let next_chapter = current_idx
            .and_then(|idx| chapters.get(idx + 1))
            .map(to_adjacent);
        let all_chapters: Vec<AdjacentChapterDto> = chapters.iter().map(to_adjacent).collect();

        let (cur_page, is_done) = prog_map
            .get(&target.id)
            .map(|p| (p.current_page, p.is_completed))
            .unwrap_or((1, false));

        let series_pref = self.reading.get_series_pref(user.id, series.id).await?;
        let resolved = user.preferences.resolve_comic_reader(
            series.reading_direction == ReadingDirection::Webtoon,
            series_pref.map(
                |sp| xmedia_core::domain::preferences::ComicSeriesReaderOverride {
                    reader_mode: sp.reader_mode,
                    fit_mode: sp.fit_mode,
                    offset_cover_page: sp.offset_cover_page,
                    webtoon_width: sp.webtoon_width,
                },
            ),
        );
        let effective_pref = EffectiveReaderPrefDto {
            reader_mode: resolved.reader_mode,
            fit_mode: resolved.fit_mode,
            offset_cover_page: resolved.offset_cover_page,
            dim_images_in_dark: resolved.dim_images_in_dark,
            preload_pages: resolved.preload_pages,
            webtoon_width: resolved.webtoon_width,
        };

        Ok(ReaderContextDto {
            series_id: series.id,
            series_title: series.title,
            reading_direction: series.reading_direction,
            chapter: ChapterItemDto::from_domain(target, cur_page.max(1), is_done),
            effective_pref,
            prev_chapter,
            next_chapter,
            all_chapters,
        })
    }

    pub async fn update_chapter_metadata(
        &self,
        actor: &User,
        chapter_id: ChapterId,
        patch: ChapterMetadataPatch,
    ) -> DomainResult<ChapterItemDto> {
        actor.ensure_admin()?;
        let mut chapter = self.load_chapter(chapter_id).await?;
        chapter.patch_metadata(patch)?;
        self.chapters.save(&chapter).await?;
        let (current_page, is_completed) = self
            .reading
            .query_progress(actor.id, ComicProgressScope::Chapter(chapter.id))
            .await?
            .into_iter()
            .next()
            .map(|p| (p.current_page, p.is_completed))
            .unwrap_or((0, false));
        Ok(ChapterItemDto::from_domain(
            chapter,
            current_page,
            is_completed,
        ))
    }

    pub async fn read_chapter_page(
        &self,
        user: &User,
        chapter_id: ChapterId,
        page_number: u32,
    ) -> DomainResult<PageContent> {
        let chapter = self.load_chapter(chapter_id).await?;
        self.require_series_bound(user, chapter.series_id).await?;
        if chapter.is_missing {
            return Err(DomainError::NotFound("chapter source is missing".into()));
        }
        if page_number == 0 || page_number > chapter.page_count {
            return Err(DomainError::Validation(format!(
                "page {page_number} out of range 1..={}",
                chapter.page_count
            )));
        }
        let Some((effective_kind, effective_path)) = chapter.source.resolved_local() else {
            return Err(DomainError::NotFound(
                "online tracked chapter is not cached yet".into(),
            ));
        };
        self.archives
            .read_page(
                effective_kind,
                effective_path,
                page_number,
                &chapter.file_hash,
            )
            .await
    }

    pub async fn open_chapter_archive_stream(
        &self,
        user: &User,
        chapter_id: ChapterId,
        spec: ByteRangeSpec,
    ) -> DomainResult<(ByteRangeStream, String, String)> {
        let chapter = self.load_chapter(chapter_id).await?;
        self.require_series_bound(user, chapter.series_id).await?;
        if chapter.is_missing {
            return Err(DomainError::NotFound("chapter source is missing".into()));
        }
        let Some((ChapterSourceKind::ArchiveFile, archive_path)) = chapter.source.resolved_local()
        else {
            return Err(DomainError::Validation(
                "direct download is only available for archive-backed chapters".into(),
            ));
        };
        self.archives
            .open_raw_archive_stream(archive_path, spec)
            .await
    }

    async fn ensure_cover_accessible(&self, user: &User, cover_key: &str) -> DomainResult<()> {
        let matched_series = self
            .series
            .query(SeriesQuery::CoverKey(cover_key.to_string()))
            .await?;
        let mut matched_tracked_series: Vec<SeriesId> = Vec::new();
        for s in &matched_series {
            if s.source.online_tracking().is_some() {
                matched_tracked_series.push(s.id);
            } else {
                return Ok(());
            }
        }
        if matched_tracked_series.is_empty() {
            let matched_chapters = self
                .chapters
                .query(ChapterQuery::CoverKey(cover_key.to_string()))
                .await?;
            let mut seen_series = std::collections::HashSet::new();
            for ch in matched_chapters {
                if !seen_series.insert(ch.series_id) {
                    continue;
                }
                if let Some(s) = self
                    .series
                    .query(SeriesQuery::Id(ch.series_id))
                    .await?
                    .into_iter()
                    .next()
                {
                    if s.source.online_tracking().is_some() {
                        matched_tracked_series.push(s.id);
                    } else {
                        return Ok(());
                    }
                }
            }
        }
        if !matched_tracked_series.is_empty() {
            let bound = self
                .bindings
                .list_bound_ids(user.id, MediaKind::Comic)
                .await?;
            if matched_tracked_series
                .iter()
                .any(|sid| bound.contains(&sid.0))
            {
                return Ok(());
            }
            return Err(DomainError::NotFound("cover not found".into()));
        }
        Ok(())
    }

    pub async fn read_cover(&self, user: &User, cover_key: &str) -> DomainResult<PageContent> {
        self.ensure_cover_accessible(user, cover_key).await?;
        self.thumbnails.read_thumbnail(cover_key).await
    }

    pub async fn read_cover_sized(
        &self,
        user: &User,
        cover_key: &str,
        max_size: Option<u32>,
    ) -> DomainResult<PageContent> {
        self.ensure_cover_accessible(user, cover_key).await?;
        match max_size {
            Some(size) => {
                self.thumbnails
                    .read_thumbnail_sized(cover_key, Some(size))
                    .await
            }
            None => self.thumbnails.read_thumbnail(cover_key).await,
        }
    }
}

impl ComicCatalogService {
    pub async fn home_overview(&self, user: &User) -> DomainResult<HomeOverviewDto> {
        let bound_ids = self
            .bindings
            .list_bound_ids(user.id, MediaKind::Comic)
            .await?;
        let all_series: Vec<_> = if bound_ids.is_empty() {
            Vec::new()
        } else {
            self.series
                .query(SeriesQuery::All)
                .await?
                .into_iter()
                .filter(|s| bound_ids.contains(&s.id.0))
                .collect()
        };
        let user_progress: Vec<_> = self
            .reading
            .query_progress(user.id, ComicProgressScope::All)
            .await?
            .into_iter()
            .filter(|p| bound_ids.contains(&p.series_id.0))
            .collect();
        let favorites: Vec<_> = self
            .reading
            .list_favorites(user.id)
            .await?
            .into_iter()
            .filter(|f| bound_ids.contains(&f.series_id.0))
            .collect();
        let fav_set: HashSet<SeriesId> = favorites.iter().map(|f| f.series_id).collect();

        let mut progress_by_series: HashMap<SeriesId, Vec<ChapterProgress>> = HashMap::new();
        for p in user_progress {
            progress_by_series.entry(p.series_id).or_default().push(p);
        }

        let series_ids_with_progress: Vec<SeriesId> = all_series
            .iter()
            .filter(|s| progress_by_series.contains_key(&s.id))
            .map(|s| s.id)
            .collect();
        let chapters_by_series = self
            .load_sorted_chapters_map(&series_ids_with_progress)
            .await?;

        let mut completed_chapters_total = 0_u32;
        for (sid, progs) in &progress_by_series {
            let active_chapter_ids: HashSet<ChapterId> = chapters_by_series
                .get(sid)
                .map(|list| {
                    list.iter()
                        .filter(|c| !c.is_missing)
                        .map(|c| c.id)
                        .collect()
                })
                .unwrap_or_default();
            for p in progs {
                if p.is_completed && active_chapter_ids.contains(&p.chapter_id) {
                    completed_chapters_total += 1;
                }
            }
        }

        let mut total_chapters = 0_u32;
        let mut distinct_tags: HashSet<String> = HashSet::new();
        let mut distinct_writers: HashSet<String> = HashSet::new();
        let mut summaries: Vec<SeriesSummaryDto> = Vec::with_capacity(all_series.len());
        let mut continue_items: Vec<ContinueReadingItemDto> = Vec::new();

        for s in all_series {
            total_chapters += s.chapter_count;
            for t in &s.tags {
                distinct_tags.insert(t.clone());
            }
            for w in &s.writers {
                distinct_writers.insert(w.clone());
            }
            let is_fav = fav_set.contains(&s.id);
            let s_prog = progress_by_series
                .get(&s.id)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            let s_chapters = chapters_by_series
                .get(&s.id)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            let summary = Self::build_series_summary_with_chapters(s, s_prog, s_chapters, is_fav);
            if let Some(pointer) = summary.continue_reading.clone() {
                continue_items.push(ContinueReadingItemDto {
                    series: summary.clone(),
                    pointer,
                });
            }
            summaries.push(summary);
        }

        continue_items.sort_by_key(|a| std::cmp::Reverse(a.pointer.last_read_at));
        let in_progress_count = continue_items.len() as u32;
        continue_items.truncate(16);

        let mut recently_updated = summaries.clone();
        recently_updated.sort_by_key(|a| std::cmp::Reverse(a.updated_at));
        recently_updated.truncate(18);

        let total_series = summaries.len() as u32;
        let bookmarks_count = self
            .reading
            .list_bookmarks(user.id, None)
            .await?
            .into_iter()
            .filter(|b| bound_ids.contains(&b.series_id.0))
            .count() as u32;
        let shelves_count = self.reading.query_shelves(Some(user.id), None).await?.len() as u32;
        let mut recently_added = summaries;
        recently_added.sort_by_key(|a| std::cmp::Reverse(a.created_at));
        recently_added.truncate(18);

        Ok(HomeOverviewDto {
            continue_reading: continue_items,
            recently_updated,
            recently_added,
            stats: ComicLibraryStatsDto {
                series_count: total_series,
                chapter_count: total_chapters,
                completed_chapters: completed_chapters_total,
                in_progress_count,
                bookmarks_count,
                favorites_count: favorites.len() as u32,
                shelves_count,
                tags_count: distinct_tags.len() as u32,
                writers_count: distinct_writers.len() as u32,
            },
        })
    }

    pub async fn list_series(
        &self,
        user: &User,
        filter: SeriesQueryFilter,
    ) -> DomainResult<Vec<SeriesSummaryDto>> {
        let bound_ids = self
            .bindings
            .list_bound_ids(user.id, MediaKind::Comic)
            .await?;
        if bound_ids.is_empty() {
            return Ok(Vec::new());
        }
        let scope = filter
            .library_id
            .map_or(SeriesQuery::All, SeriesQuery::Library);
        let base_list: Vec<_> = self
            .series
            .query(scope)
            .await?
            .into_iter()
            .filter(|s| bound_ids.contains(&s.id.0))
            .collect();

        let shelf_filter: Option<HashSet<SeriesId>> = if let Some(shelf_id) = filter.shelf_id {
            let shelf = self
                .reading
                .query_shelves(None, Some(shelf_id))
                .await?
                .into_iter()
                .next()
                .ok_or_else(|| DomainError::NotFound("shelf not found".into()))?;
            if shelf.user_id != user.id {
                return Err(DomainError::Forbidden("access denied to shelf".into()));
            }
            Some(shelf.series_ids.into_iter().collect())
        } else {
            None
        };

        let user_progress = self
            .reading
            .query_progress(user.id, ComicProgressScope::All)
            .await?;
        let mut progress_by_series: HashMap<SeriesId, Vec<ChapterProgress>> = HashMap::new();
        for p in user_progress {
            progress_by_series.entry(p.series_id).or_default().push(p);
        }

        let favorites = self.reading.list_favorites(user.id).await?;
        let fav_set: HashSet<SeriesId> = favorites.into_iter().map(|f| f.series_id).collect();

        let criteria = SeriesCriteria {
            allowed_ids: shelf_filter.as_ref(),
            favorites: Some(&fav_set),
            favorites_only: filter.favorites_only,
            status: filter.status,
            writer: filter.writer.as_deref(),
            penciller: filter.penciller.as_deref(),
            tag: filter.tag.as_deref(),
            genre: filter.genre.as_deref(),
            search: filter.search.as_deref(),
        };
        let matched_series: Vec<_> = base_list
            .into_iter()
            .filter(|s| s.matches_criteria(&criteria))
            .collect();

        let ids_with_progress: Vec<SeriesId> = matched_series
            .iter()
            .filter(|s| progress_by_series.contains_key(&s.id))
            .map(|s| s.id)
            .collect();
        let chapters_by_series = self.load_sorted_chapters_map(&ids_with_progress).await?;

        let mut results = Vec::with_capacity(matched_series.len());
        for s in matched_series {
            let is_fav = fav_set.contains(&s.id);
            let s_prog = progress_by_series
                .get(&s.id)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            let s_chapters = chapters_by_series
                .get(&s.id)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            let Some(reading) = SeriesReadingSummary::compute(
                s.id,
                s.chapter_count,
                s_chapters,
                s_prog,
                filter.read_status.as_deref(),
            ) else {
                continue;
            };

            results.push(Self::into_series_summary(s, reading, is_fav));
        }

        let sort_by = filter.sort_by.as_deref().unwrap_or("updated_at");
        let asc = filter.sort_dir.as_deref() == Some("asc");

        results.sort_by(|a, b| {
            let ord = match sort_by {
                "title" => natural_cmp(&a.title, &b.title),
                "created_at" => a.created_at.cmp(&b.created_at),
                "chapter_count" => a.chapter_count.cmp(&b.chapter_count),
                _ => a.updated_at.cmp(&b.updated_at),
            };
            if asc { ord } else { ord.reverse() }
        });

        Ok(results)
    }

    pub async fn list_pool_series(
        &self,
        user: &User,
        filter: SeriesQueryFilter,
    ) -> DomainResult<Vec<PoolSeriesSummaryDto>> {
        let bound_ids = self
            .bindings
            .list_bound_ids(user.id, MediaKind::Comic)
            .await?;
        let scope = filter
            .library_id
            .map_or(SeriesQuery::All, SeriesQuery::Library);
        let base_list = self.series.query(scope).await?;
        let criteria = SeriesCriteria {
            status: filter.status,
            search: filter.search.as_deref(),
            ..Default::default()
        };

        let mut results: Vec<PoolSeriesSummaryDto> = Vec::new();
        for s in base_list {
            if s.source.online_tracking().is_some() || !s.matches_criteria(&criteria) {
                continue;
            }
            let bound = bound_ids.contains(&s.id.0);
            let summary = Self::build_series_summary_with_chapters(s, &[], &[], false);
            results.push(PoolSeriesSummaryDto {
                series: summary,
                bound,
            });
        }

        let sort_by = filter.sort_by.as_deref().unwrap_or("updated_at");
        let asc = filter.sort_dir.as_deref() == Some("asc");
        results.sort_by(|a, b| {
            let ord = match sort_by {
                "title" => natural_cmp(&a.series.title, &b.series.title),
                "created_at" => a.series.created_at.cmp(&b.series.created_at),
                "chapter_count" => a.series.chapter_count.cmp(&b.series.chapter_count),
                _ => a.series.updated_at.cmp(&b.series.updated_at),
            };
            if asc { ord } else { ord.reverse() }
        });

        Ok(results)
    }

    pub(super) async fn count_bound_series_by_library(
        &self,
        user: &User,
    ) -> DomainResult<HashMap<LibraryId, u64>> {
        let bound_ids = self
            .bindings
            .list_bound_ids(user.id, MediaKind::Comic)
            .await?;
        if bound_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let mut counts: HashMap<LibraryId, u64> = HashMap::new();
        for s in self.series.query(SeriesQuery::All).await? {
            if bound_ids.contains(&s.id.0) {
                *counts.entry(s.library_id).or_insert(0) += 1;
            }
        }
        Ok(counts)
    }

    pub async fn get_series_detail(
        &self,
        user: &User,
        series_id: SeriesId,
    ) -> DomainResult<SeriesDetailDto> {
        self.require_series_bound(user, series_id).await?;
        let series = self.load_series(series_id).await?;

        let mut chapters = self.chapters.query(ChapterQuery::Series(series_id)).await?;
        chapters.sort_by(|a, b| a.cmp_order(b));

        let progress_list = self
            .reading
            .query_progress(user.id, ComicProgressScope::Series(series_id))
            .await?;
        let reading = SeriesReadingSummary::compute(
            series_id,
            series.chapter_count,
            &chapters,
            &progress_list,
            None,
        )
        .unwrap_or_default();
        let completed_chapters = reading.completed_chapters;
        let continue_reading = reading.continue_reading;

        let prog_map: HashMap<ChapterId, ChapterProgress> = progress_list
            .into_iter()
            .map(|p| (p.chapter_id, p))
            .collect();

        let mut chapter_dtos = Vec::with_capacity(chapters.len());
        for ch in chapters {
            let (cur_page, is_done) = prog_map
                .get(&ch.id)
                .map(|p| (p.current_page, p.is_completed))
                .unwrap_or((0, false));
            chapter_dtos.push(ChapterItemDto::from_domain(ch, cur_page, is_done));
        }

        let is_favorite = self
            .reading
            .list_favorites(user.id)
            .await?
            .iter()
            .any(|f| f.series_id == series_id);
        let shelves = self.reading.query_shelves(Some(user.id), None).await?;
        let shelf_ids: Vec<ComicShelfId> = shelves
            .into_iter()
            .filter(|sh| sh.series_ids.contains(&series_id))
            .map(|sh| sh.id)
            .collect();
        let reader_pref = self.reading.get_series_pref(user.id, series_id).await?;
        let cover_url = series
            .cover_path
            .as_ref()
            .map(|key| format!("/api/v1/comic/covers/{key}"));
        let online_tracking = series
            .source
            .online_tracking()
            .map(|info| OnlineTrackedSeriesDto {
                source_id: info.source_id,
                comic_url: info.comic_url.clone(),
                toc_url: info.toc_url.clone(),
            });

        Ok(SeriesDetailDto {
            series,
            cover_url,
            is_favorite,
            shelf_ids,
            completed_chapters,
            continue_reading,
            reader_pref,
            online_tracking,
            chapters: chapter_dtos,
        })
    }

    pub async fn taxonomy_overview(&self, user: &User) -> DomainResult<TaxonomyOverviewDto> {
        let bound_ids = self
            .bindings
            .list_bound_ids(user.id, MediaKind::Comic)
            .await?;
        let all: Vec<_> = if bound_ids.is_empty() {
            Vec::new()
        } else {
            self.series
                .query(SeriesQuery::All)
                .await?
                .into_iter()
                .filter(|s| bound_ids.contains(&s.id.0))
                .collect()
        };
        let buckets = Series::tally_taxonomies(&all);

        Ok(TaxonomyOverviewDto {
            tags: to_taxonomy_dtos(buckets.tags),
            genres: to_taxonomy_dtos(buckets.genres),
            writers: to_taxonomy_dtos(buckets.writers),
            pencillers: to_taxonomy_dtos(buckets.pencillers),
        })
    }
}

impl ComicCatalogService {
    pub async fn upload_archive(
        &self,
        actor: &User,
        input: UploadArchiveInput,
    ) -> DomainResult<UploadArchiveResultDto> {
        actor.ensure_admin()?;
        validate_archive_filename(&input.filename)?;

        let inspection = self
            .archives
            .inspect_bytes(&input.filename, &input.bytes)
            .await?;
        if inspection.page_count == 0 {
            return Err(DomainError::Validation(
                "uploaded archive contains no readable comic images".into(),
            ));
        }

        let managed_lib = self.get_or_create_managed_library().await?;
        let series = self
            .resolve_upload_series(
                &managed_lib,
                input.target_series_id,
                input.series_title_override.as_deref(),
                &input.filename,
                &inspection,
            )
            .await?;

        let folder_slug = sanitize_folder_name(&series.title);
        let stored = self
            .storage
            .store_uploaded_archive(&folder_slug, &input.filename, &input.bytes)
            .await?;

        self.finalize_uploaded_chapter(
            actor.id,
            managed_lib,
            series,
            &input.filename,
            inspection,
            stored,
        )
        .await
    }

    pub async fn upload_archive_from_path(
        &self,
        actor: &User,
        input: UploadArchivePathInput,
    ) -> DomainResult<UploadArchiveResultDto> {
        actor.ensure_admin()?;
        validate_archive_filename(&input.filename)?;

        let temp_path_str = input.temp_path.to_string_lossy().into_owned();
        let inspection = self
            .archives
            .inspect_source(ChapterSourceKind::ArchiveFile, &temp_path_str)
            .await?;
        if inspection.page_count == 0 {
            return Err(DomainError::Validation(
                "uploaded archive contains no readable comic images".into(),
            ));
        }

        let managed_lib = self.get_or_create_managed_library().await?;
        let series = self
            .resolve_upload_series(
                &managed_lib,
                input.target_series_id,
                input.series_title_override.as_deref(),
                &input.filename,
                &inspection,
            )
            .await?;

        let folder_slug = sanitize_folder_name(&series.title);
        let stored = self
            .storage
            .store_uploaded_archive_from_path(&folder_slug, &input.filename, &input.temp_path)
            .await?;

        self.finalize_uploaded_chapter(
            actor.id,
            managed_lib,
            series,
            &input.filename,
            inspection,
            stored,
        )
        .await
    }

    async fn resolve_upload_series(
        &self,
        managed_lib: &Library,
        target_series_id: Option<SeriesId>,
        series_title_override: Option<&str>,
        filename: &str,
        inspection: &ArchiveInspection,
    ) -> DomainResult<Series> {
        if let Some(target_id) = target_series_id {
            let existing = self
                .series
                .query(SeriesQuery::Id(target_id))
                .await?
                .into_iter()
                .next()
                .ok_or_else(|| DomainError::NotFound("target series not found".into()))?;
            if existing.library_id != managed_lib.id {
                return Err(DomainError::Validation(
                    "target series belongs to a read-only local library".into(),
                ));
            }
            if existing.source.online_tracking().is_some() {
                return Err(DomainError::Validation(
                    "cannot upload local archives into an online tracked series".into(),
                ));
            }
            return Ok(existing);
        }

        let parsed = parse_comic_chapter_filename(filename, 1);
        let desired_title = series_title_override
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .or_else(|| {
                inspection
                    .comic_info
                    .as_ref()
                    .and_then(|ci| ci.series.clone())
                    .filter(|s| !s.trim().is_empty())
            })
            .or_else(|| parsed.inferred_series.clone())
            .unwrap_or_else(|| parsed.clean_title.clone());

        let all_series = self
            .series
            .query(SeriesQuery::Library(managed_lib.id))
            .await?;
        if let Some(existing) = all_series.into_iter().find(|s| {
            s.source.online_tracking().is_none()
                && s.title.eq_ignore_ascii_case(desired_title.trim())
        }) {
            Ok(existing)
        } else {
            let folder_slug = sanitize_folder_name(&desired_title);
            let root = self.storage.ensure_managed_root().await?;
            let source_dir = format!("{root}/{folder_slug}");
            Series::new(
                managed_lib.id,
                desired_title,
                SeriesSource::Local { source_dir },
            )
        }
    }

    async fn finalize_uploaded_chapter(
        &self,
        actor_id: UserId,
        managed_lib: Library,
        mut series: Series,
        filename: &str,
        inspection: ArchiveInspection,
        stored: StoredUploadFile,
    ) -> DomainResult<UploadArchiveResultDto> {
        let existing_chapters = self.chapters.query(ChapterQuery::Series(series.id)).await?;
        let fallback_num = (existing_chapters.len() + 1) as u32;
        let parsed_with_index = parse_comic_chapter_filename(filename, fallback_num);

        let stored_path = stored.source_path;
        let mut chapter = Chapter::new(
            series.id,
            managed_lib.id,
            parsed_with_index.kind.into(),
            parsed_with_index.number,
            parsed_with_index.volume_number,
            parsed_with_index.clean_title,
            inspection.page_count,
            ChapterSource::Archive {
                path: stored_path.clone(),
            },
            stored.file_size,
            stored.file_hash,
        )?;

        if let Some(ci) = &inspection.comic_info {
            chapter.apply_comic_info(ci);
            if !series.metadata_locked {
                series.apply_comic_info(ci);
            }
        }

        let now = Timestamp::now();
        if let Ok(first_page) = self
            .archives
            .read_page(
                ChapterSourceKind::ArchiveFile,
                &stored_path,
                1,
                &chapter.file_hash,
            )
            .await
        {
            let thumb_key = format!("ch_{}", chapter.id);
            if let Ok(saved_key) = self
                .thumbnails
                .generate_from_bytes(&thumb_key, &first_page.bytes)
                .await
            {
                let _ = series.apply_cover(CoverUpdate::Fallback(saved_key.clone()), now);
                let _ = chapter.sync_scan(ChapterScanUpdate::Cover(saved_key), now);
            }
        }

        self.chapters.save(&chapter).await?;

        let active_count = self
            .chapters
            .query(ChapterQuery::Series(series.id))
            .await?
            .into_iter()
            .filter(|c| !c.is_missing)
            .count() as u32;
        let _ = series.sync_chapter_count(active_count, now);
        series.updated_at = now;
        self.series.save(&series).await?;
        self.bindings
            .bind(actor_id, MediaKind::Comic, series.id.0)
            .await?;

        self.events.publish(SystemEvent::MediaChanged {
            media_kind: MediaKind::Comic,
        });

        Ok(UploadArchiveResultDto {
            series_id: series.id,
            series_title: series.title,
            chapter_id: chapter.id,
            chapter_title: chapter.title,
            page_count: chapter.page_count,
        })
    }
}

pub(crate) fn validate_archive_filename(filename: &str) -> DomainResult<()> {
    match ComicFormatExt::from_path(Path::new(filename)) {
        Some(fmt) if fmt.is_zip_based() => Ok(()),
        Some(fmt) => Err(DomainError::UnsupportedFormat(format!(
            ".{} comic archives require an external extractor and are not supported",
            fmt.as_str()
        ))),
        None => Err(DomainError::UnsupportedFormat(
            "only .cbz, .zip, and .epub comic archives are supported".into(),
        )),
    }
}

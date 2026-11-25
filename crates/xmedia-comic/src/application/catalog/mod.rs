mod mutations;
mod queries;
mod upload;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use uuid::Uuid;
use xmedia_core::application::bindings::MediaBindingService;
use xmedia_core::application::ports::RealtimeEventPort;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::{Library, MediaKind};
use xmedia_core::domain::repository::LibraryRepository;
use xmedia_core::domain::user::{User, UserId};

use crate::application::dto::{OnlineTrackedSeriesDto, SeriesSummaryDto, TaxonomyCountDto};
use crate::application::ports::{ArchiveReaderPort, ComicFileStoragePort, ThumbnailGeneratorPort};
use crate::domain::chapter::Chapter;
use crate::domain::id::{ChapterId, SeriesId};
use crate::domain::natural::natural_cmp;
use crate::domain::reading::{ChapterProgress, compute_continue_reading};
use crate::domain::repository::{ChapterRepository, ComicReadingRepository, SeriesRepository};
use crate::domain::series::Series;

pub struct ComicCatalogService {
    pub(super) libraries: Arc<dyn LibraryRepository>,
    pub(super) series: Arc<dyn SeriesRepository>,
    pub(super) chapters: Arc<dyn ChapterRepository>,
    pub(super) reading: Arc<dyn ComicReadingRepository>,
    pub(super) bindings: Arc<MediaBindingService>,
    pub(super) archives: Arc<dyn ArchiveReaderPort>,
    pub(super) thumbnails: Arc<dyn ThumbnailGeneratorPort>,
    pub(super) storage: Arc<dyn ComicFileStoragePort>,
    pub(super) events: Arc<dyn RealtimeEventPort>,
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

    pub fn bindings(&self) -> &Arc<MediaBindingService> {
        &self.bindings
    }

    pub async fn list_all_series_ids_for_backfill(&self) -> DomainResult<Vec<Uuid>> {
        Ok(self
            .series
            .list_all()
            .await?
            .into_iter()
            .map(|s| s.id.0)
            .collect())
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

    pub async fn require_series_bound(&self, user: &User, series_id: SeriesId) -> DomainResult<()> {
        self.bindings
            .require_bound(user.id, MediaKind::Comic, series_id.0, "series not found")
            .await
    }

    pub async fn require_chapter_bound(
        &self,
        user: &User,
        chapter_id: ChapterId,
    ) -> DomainResult<Chapter> {
        let chapter = self
            .chapters
            .find_by_id(chapter_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("chapter not found".into()))?;
        self.require_series_bound(user, chapter.series_id).await?;
        Ok(chapter)
    }

    pub(super) async fn load_sorted_chapters_map(
        &self,
        series_ids: &[SeriesId],
    ) -> DomainResult<HashMap<SeriesId, Vec<Chapter>>> {
        if series_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let chapters = self.chapters.list_by_series_ids(series_ids).await?;
        let mut map: HashMap<SeriesId, Vec<Chapter>> = HashMap::new();
        for ch in chapters {
            map.entry(ch.series_id).or_default().push(ch);
        }
        for list in map.values_mut() {
            list.sort_by(|a, b| a.cmp_order(b));
        }
        Ok(map)
    }

    pub fn build_series_summary_with_chapters(
        series: Series,
        progress_list: &[ChapterProgress],
        sorted_chapters: &[Chapter],
        is_favorite: bool,
    ) -> SeriesSummaryDto {
        let (completed_chapters, continue_reading) = if progress_list.is_empty() {
            (0, None)
        } else {
            let active_ids: HashSet<ChapterId> = sorted_chapters
                .iter()
                .filter(|c| !c.is_missing)
                .map(|c| c.id)
                .collect();
            let done = progress_list
                .iter()
                .filter(|p| p.is_completed && active_ids.contains(&p.chapter_id))
                .count() as u32;
            let ptr = compute_continue_reading(series.id, sorted_chapters, progress_list);
            (done, ptr)
        };

        let cover_url = series
            .cover_path
            .as_ref()
            .map(|key| format!("/api/v1/comic/covers/{key}"));
        let online_tracking = series.online_tracking().map(|info| OnlineTrackedSeriesDto {
            source_id: info.source_id,
            comic_url: info.comic_url,
            toc_url: info.toc_url,
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
            completed_chapters,
            is_favorite,
            continue_reading,
            online_tracking,
            created_at: series.created_at,
            updated_at: series.updated_at,
        }
    }

    pub async fn build_series_summary(
        &self,
        series: Series,
        progress_list: &[ChapterProgress],
        is_favorite: bool,
    ) -> DomainResult<SeriesSummaryDto> {
        let sorted_chapters = if progress_list.is_empty() {
            Vec::new()
        } else {
            let mut chapters = self.chapters.list_by_series(series.id).await?;
            chapters.sort_by(|a, b| a.cmp_order(b));
            chapters
        };
        Ok(Self::build_series_summary_with_chapters(
            series,
            progress_list,
            &sorted_chapters,
            is_favorite,
        ))
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
        let lib = Library::new_managed(MediaKind::Comic, MediaKind::Comic.default_managed_name())?;
        self.libraries.save(&lib).await?;
        Ok(lib)
    }
}

pub(super) fn sanitize_folder_name(title: &str) -> String {
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

pub(super) fn to_sorted_counts(map: HashMap<String, u32>) -> Vec<TaxonomyCountDto> {
    let mut vec: Vec<TaxonomyCountDto> = map
        .into_iter()
        .map(|(name, count)| TaxonomyCountDto { name, count })
        .collect();
    vec.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| natural_cmp(&a.name, &b.name))
    });
    vec
}

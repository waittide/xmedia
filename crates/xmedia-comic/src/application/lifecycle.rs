use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use async_trait::async_trait;
use jiff::Timestamp;
use xmedia_core::application::bindings::MediaBindingService;
use xmedia_core::application::library::ScanProgressReporter;
use xmedia_core::application::ports::{MediaDomainLifecycle, RealtimeEventPort};
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::{Library, LibraryId, LibraryKind, MediaKind};
use xmedia_core::domain::repository::LibraryRepository;
use xmedia_core::domain::user::UserId;

use crate::application::ports::{
    ArchiveReaderPort, ComicFileStoragePort, ComicInfoMetadata, ComicScannerPort,
    ExistingChapterDigest, ThumbnailGeneratorPort,
};
use crate::domain::chapter::Chapter;
use crate::domain::id::SeriesId;
use crate::domain::natural::build_chapter_sort_key;
use crate::domain::repository::{ChapterRepository, ComicReadingRepository, SeriesRepository};
use crate::domain::series::{Series, normalize_list};

pub struct ComicDomainLifecycle {
    libraries: Arc<dyn LibraryRepository>,
    series: Arc<dyn SeriesRepository>,
    chapters: Arc<dyn ChapterRepository>,
    reading: Arc<dyn ComicReadingRepository>,
    bindings: Arc<MediaBindingService>,
    scanner: Arc<dyn ComicScannerPort>,
    archives: Arc<dyn ArchiveReaderPort>,
    thumbnails: Arc<dyn ThumbnailGeneratorPort>,
    storage: Arc<dyn ComicFileStoragePort>,
    events: Arc<dyn RealtimeEventPort>,
}

impl ComicDomainLifecycle {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        libraries: Arc<dyn LibraryRepository>,
        series: Arc<dyn SeriesRepository>,
        chapters: Arc<dyn ChapterRepository>,
        reading: Arc<dyn ComicReadingRepository>,
        bindings: Arc<MediaBindingService>,
        scanner: Arc<dyn ComicScannerPort>,
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
            scanner,
            archives,
            thumbnails,
            storage,
            events,
        }
    }
}

#[async_trait]
impl MediaDomainLifecycle for ComicDomainLifecycle {
    fn media_kind(&self) -> MediaKind {
        MediaKind::Comic
    }

    async fn scan_library(&self, library: Library) -> DomainResult<()> {
        let root_path = match &library.kind {
            LibraryKind::LocalMount { root_path } => root_path.clone(),
            LibraryKind::Managed => {
                return Err(DomainError::Validation(
                    "managed upload library cannot be scanned".into(),
                ));
            }
        };

        let library_id = library.id;
        let existing_series_list = self.series.list_by_library(library_id).await?;
        let series_info_by_id: HashMap<SeriesId, (String, String)> = existing_series_list
            .iter()
            .map(|s| (s.id, (s.title.clone(), s.source_dir.clone())))
            .collect();
        let existing_chapters = self.chapters.list_by_library(library_id).await?;
        let mut existing_by_path: HashMap<String, Chapter> = HashMap::new();
        let mut known_digests: HashMap<String, ExistingChapterDigest> = HashMap::new();

        for ch in existing_chapters {
            let (series_title, series_source_dir) = series_info_by_id
                .get(&ch.series_id)
                .map(|(t, d)| (Some(t.clone()), Some(d.clone())))
                .unwrap_or((None, None));
            known_digests.insert(
                ch.source_path.clone(),
                ExistingChapterDigest {
                    file_size: ch.file_size,
                    file_hash: ch.file_hash.clone(),
                    page_count: ch.page_count,
                    updated_at: ch.updated_at,
                    series_title,
                    series_source_dir,
                },
            );
            existing_by_path.insert(ch.source_path.clone(), ch);
        }

        let scanned_series = self
            .scanner
            .scan_directory(&root_path, &known_digests)
            .await?;
        let seen_on_disk: HashSet<String> = scanned_series
            .iter()
            .flat_map(|s| s.chapters.iter().map(|c| c.source_path.clone()))
            .collect();
        let total_chapters: u64 = scanned_series.iter().map(|s| s.chapters.len() as u64).sum();
        let mut reporter = ScanProgressReporter::new(
            library,
            self.libraries.clone(),
            self.events.clone(),
            Some(total_chapters),
        );
        reporter.start().await;

        let mut series_by_dir: HashMap<String, Series> = existing_series_list
            .into_iter()
            .map(|s| (s.source_dir.clone(), s))
            .collect();

        let mut seen_paths: HashSet<String> = HashSet::new();
        let mut affected_series: HashSet<SeriesId> = HashSet::new();
        let mut added = 0u64;
        let mut updated = 0u64;
        let now = Timestamp::now();

        for entry in scanned_series {
            if entry.chapters.is_empty() {
                continue;
            }

            let (mut series, is_new_series) = match series_by_dir.remove(&entry.source_dir) {
                Some(s) => (s, false),
                None => (
                    Series::new(library_id, entry.title.clone(), entry.source_dir.clone())?,
                    true,
                ),
            };
            let mut series_dirty = is_new_series;
            let orig_series_meta = (
                series.title.clone(),
                series.alt_titles.clone(),
                series.summary.clone(),
                series.writers.clone(),
                series.pencillers.clone(),
                series.publisher.clone(),
                series.genres.clone(),
                series.tags.clone(),
                series.age_rating.clone(),
                series.language.clone(),
                series.year,
                series.reading_direction,
            );

            let mut ordered_chapters: Vec<Chapter> = Vec::with_capacity(entry.chapters.len());

            for sc in entry.chapters {
                seen_paths.insert(sc.source_path.clone());

                let (mut chapter, refresh_cover, mut chapter_dirty) =
                    if let Some(mut existing) = existing_by_path.remove(&sc.source_path) {
                        let was_missing = existing.is_missing;
                        let mut dirty = false;
                        if !sc.unchanged {
                            existing.is_missing = false;
                            existing.page_count = sc.page_count;
                            existing.file_size = sc.file_size;
                            existing.file_hash = sc.file_hash.clone();
                            existing.title = sc.title.clone();
                            existing.number = sc.number;
                            existing.volume_number = sc.volume_number;
                            existing.kind = sc.kind;
                            if let Some(ci) = &sc.comic_info {
                                apply_comic_info_to_chapter(&mut existing, ci);
                            }
                            existing.updated_at = now;
                            updated += 1;
                            dirty = true;
                        } else if was_missing {
                            existing.is_missing = false;
                            existing.updated_at = now;
                            updated += 1;
                            dirty = true;
                        }
                        if existing.series_id != series.id {
                            affected_series.insert(existing.series_id);
                            existing.series_id = series.id;
                            existing.updated_at = now;
                            dirty = true;
                        }
                        let need_cov = existing.cover_path.is_none() || !sc.unchanged;
                        (existing, need_cov, dirty)
                    } else {
                        let matched_old_path = existing_by_path
                            .iter()
                            .find(|(old_path, old_ch)| {
                                !seen_on_disk.contains(*old_path)
                                    && !sc.file_hash.is_empty()
                                    && old_ch.file_hash == sc.file_hash
                            })
                            .map(|(k, _)| k.clone());

                        if let Some(old_path) = matched_old_path
                            && let Some(mut migrated) = existing_by_path.remove(&old_path)
                        {
                            if migrated.series_id != series.id {
                                affected_series.insert(migrated.series_id);
                                migrated.series_id = series.id;
                            }
                            migrated.source_kind = sc.source_kind;
                            migrated.source_path = sc.source_path.clone();
                            migrated.is_missing = false;
                            migrated.page_count = sc.page_count;
                            migrated.file_size = sc.file_size;
                            migrated.file_hash = sc.file_hash.clone();
                            if let Some(ci) = &sc.comic_info {
                                apply_comic_info_to_chapter(&mut migrated, ci);
                            }
                            migrated.updated_at = now;
                            updated += 1;
                            let need_cov = migrated.cover_path.is_none();
                            (migrated, need_cov, true)
                        } else {
                            let mut created = Chapter::new(
                                series.id,
                                library_id,
                                sc.kind,
                                sc.number,
                                sc.volume_number,
                                sc.title.clone(),
                                sc.page_count,
                                sc.source_kind,
                                sc.source_path.clone(),
                                sc.file_size,
                                sc.file_hash.clone(),
                            )?;
                            if let Some(ci) = &sc.comic_info {
                                apply_comic_info_to_chapter(&mut created, ci);
                            }
                            added += 1;
                            (created, true, true)
                        }
                    };

                if let Some(ci) = &sc.comic_info
                    && !series.metadata_locked
                {
                    apply_comic_info_to_series(&mut series, ci);
                }

                if refresh_cover
                    && chapter.page_count > 0
                    && let Ok(first_page) = self
                        .archives
                        .read_page(
                            chapter.source_kind,
                            &chapter.source_path,
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
                        && chapter.cover_path.as_deref() != Some(saved_key.as_str())
                    {
                        chapter.cover_path = Some(saved_key);
                        chapter_dirty = true;
                    }
                }

                if chapter_dirty {
                    self.chapters.save(&chapter).await?;
                    series_dirty = true;
                }
                ordered_chapters.push(chapter);
                reporter.advance().await;
            }

            ordered_chapters.sort_by(|a, b| a.cmp_order(b));
            let new_chapter_count = ordered_chapters.len() as u32;
            if series.chapter_count != new_chapter_count {
                series.chapter_count = new_chapter_count;
                series_dirty = true;
            }
            if let Some(ref cov_bytes) = entry.series_cover_bytes
                && (series.cover_path.is_none() || (!series.metadata_locked && series_dirty))
            {
                let thumb_key = format!("series_{}", series.id);
                if let Ok(saved_key) = self
                    .thumbnails
                    .generate_from_bytes(&thumb_key, cov_bytes)
                    .await
                    && series.cover_path.as_deref() != Some(saved_key.as_str())
                {
                    series.cover_path = Some(saved_key);
                    series_dirty = true;
                }
            }
            if series.cover_path.is_none()
                && let Some(fallback_cover) =
                    ordered_chapters.iter().find_map(|c| c.cover_path.clone())
            {
                series.cover_path = Some(fallback_cover);
                series_dirty = true;
            }
            let current_series_meta = (
                series.title.clone(),
                series.alt_titles.clone(),
                series.summary.clone(),
                series.writers.clone(),
                series.pencillers.clone(),
                series.publisher.clone(),
                series.genres.clone(),
                series.tags.clone(),
                series.age_rating.clone(),
                series.language.clone(),
                series.year,
                series.reading_direction,
            );
            if current_series_meta != orig_series_meta {
                series_dirty = true;
            }
            if series_dirty {
                series.updated_at = now;
                self.series.save(&series).await?;
            }
        }

        let mut missing_count = 0u64;
        for (path, mut leftover) in existing_by_path {
            if leftover.is_online_tracked() {
                continue;
            }
            if !seen_paths.contains(&path) && !leftover.is_missing {
                leftover.is_missing = true;
                leftover.updated_at = now;
                affected_series.insert(leftover.series_id);
                self.chapters.save(&leftover).await?;
                missing_count += 1;
            }
        }

        for sid in affected_series {
            let chapters_in_series = self.chapters.list_by_series(sid).await?;
            if chapters_in_series.is_empty() {
                let _ = self
                    .bindings
                    .delete_item_bindings(MediaKind::Comic, sid.0)
                    .await;
                let _ = self.reading.delete_by_series(sid).await;
                let _ = self.series.delete(sid).await;
            } else if let Some(mut s) = self.series.find_by_id(sid).await? {
                let active_count = chapters_in_series
                    .into_iter()
                    .filter(|c| !c.is_missing)
                    .count() as u32;
                s.chapter_count = active_count;
                s.updated_at = now;
                self.series.save(&s).await?;
            }
        }

        reporter.complete(added, updated, missing_count).await?;

        Ok(())
    }

    async fn count_library_items(&self, library_id: LibraryId) -> DomainResult<u64> {
        let series = self.series.list_by_library(library_id).await?;
        Ok(series
            .into_iter()
            .filter(|s| s.online_tracking().is_none())
            .count() as u64)
    }

    async fn on_library_deleted(&self, library_id: LibraryId) -> DomainResult<()> {
        let lib = self.libraries.find_by_id(library_id).await?;
        let is_managed = lib.is_some_and(|l| l.kind.is_managed());
        let lib_series = self.series.list_by_library(library_id).await?;
        for s in lib_series {
            if is_managed {
                for ch in self.chapters.list_by_series(s.id).await? {
                    if let Some(path) = ch.managed_file_path() {
                        let _ = self.storage.delete_managed_file(&path).await;
                    }
                }
            }
            self.bindings
                .delete_item_bindings(MediaKind::Comic, s.id.0)
                .await?;
            self.reading.delete_by_series(s.id).await?;
            self.chapters.delete_by_series(s.id).await?;
            self.series.delete(s.id).await?;
        }
        Ok(())
    }

    async fn on_user_deleted(&self, user_id: UserId) -> DomainResult<()> {
        self.reading.delete_by_user(user_id).await?;
        let unbound_ids = self
            .bindings
            .delete_user_bindings_for_media(user_id, MediaKind::Comic)
            .await?;
        for raw_id in unbound_ids {
            let sid = SeriesId(raw_id);
            self.bindings
                .gc_if_orphaned(MediaKind::Comic, raw_id, || async {
                    if let Some(series) = self.series.find_by_id(sid).await?
                        && series.online_tracking().is_some()
                    {
                        for ch in self.chapters.list_by_series(sid).await? {
                            if let Some(path) = ch.managed_file_path() {
                                let _ = self.storage.delete_managed_file(&path).await;
                            }
                        }
                        self.reading.delete_by_series(sid).await?;
                        self.chapters.delete_by_series(sid).await?;
                        self.series.delete(sid).await?;
                        return Ok(true);
                    }
                    Ok(false)
                })
                .await?;
        }
        Ok(())
    }
}

pub fn apply_comic_info_to_chapter(chapter: &mut Chapter, ci: &ComicInfoMetadata) {
    if let Some(title) = &ci.title
        && !title.trim().is_empty()
    {
        chapter.title = title.trim().to_owned();
    }
    if let Some(num) = ci.number
        && num.is_finite()
        && num >= 0.0
    {
        chapter.number = num;
    }
    if let Some(vol) = ci.volume
        && vol.is_finite()
        && vol >= 0.0
    {
        chapter.volume_number = Some(vol);
    }
    if let Some(summary) = &ci.summary
        && !summary.trim().is_empty()
    {
        chapter.summary = summary.trim().to_owned();
    }
    if !ci.writers.is_empty() {
        chapter.writers = normalize_list(ci.writers.clone());
    }
    if !ci.pencillers.is_empty() {
        chapter.pencillers = normalize_list(ci.pencillers.clone());
    }
    chapter.sort_key = build_chapter_sort_key(
        chapter.kind.sort_rank(),
        chapter.volume_number,
        chapter.number,
        &chapter.title,
    );
}

pub fn apply_comic_info_to_series(series: &mut Series, ci: &ComicInfoMetadata) {
    if let Some(series_title) = &ci.series
        && !series_title.trim().is_empty()
    {
        series.title = series_title.trim().to_owned();
        series.sort_title = series.title.clone();
    }
    if let Some(group) = &ci.series_group {
        let trimmed = group.trim();
        if !trimmed.is_empty() && !trimmed.eq_ignore_ascii_case(&series.title) {
            let mut combined = series.alt_titles.clone();
            combined.push(trimmed.to_owned());
            series.alt_titles = normalize_list(combined);
        }
    }
    if let Some(summary) = &ci.summary
        && series.summary.is_empty()
        && !summary.trim().is_empty()
    {
        series.summary = summary.trim().to_owned();
    }
    if !ci.writers.is_empty() {
        let mut combined = series.writers.clone();
        combined.extend(ci.writers.iter().cloned());
        series.writers = normalize_list(combined);
    }
    if !ci.pencillers.is_empty() {
        let mut combined = series.pencillers.clone();
        combined.extend(ci.pencillers.iter().cloned());
        series.pencillers = normalize_list(combined);
    }
    if let Some(publisher) = &ci.publisher
        && series.publisher.is_none()
        && !publisher.trim().is_empty()
    {
        series.publisher = Some(publisher.trim().to_owned());
    }
    if !ci.genres.is_empty() {
        let mut combined = series.genres.clone();
        combined.extend(ci.genres.iter().cloned());
        series.genres = normalize_list(combined);
    }
    if !ci.tags.is_empty() {
        let mut combined = series.tags.clone();
        combined.extend(ci.tags.iter().cloned());
        series.tags = normalize_list(combined);
    }
    if let Some(age) = &ci.age_rating
        && series.age_rating.is_none()
        && !age.trim().is_empty()
    {
        series.age_rating = Some(age.trim().to_owned());
    }
    if let Some(lang) = &ci.language_iso
        && series.language.is_none()
        && !lang.trim().is_empty()
    {
        series.language = Some(lang.trim().to_owned());
    }
    if let Some(year) = ci.year
        && series.year.is_none()
    {
        series.year = Some(year);
    }
    if let Some(dir) = ci.reading_direction {
        series.reading_direction = dir;
    }
}

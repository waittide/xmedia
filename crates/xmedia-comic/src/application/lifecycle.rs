use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use async_trait::async_trait;
use jiff::Timestamp;
use xmedia_core::application::ports::{MediaDomainLifecycle, RealtimeEventPort, SystemEvent};
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

    async fn scan_library(&self, mut library: Library) -> DomainResult<()> {
        let root_path = match &library.kind {
            LibraryKind::LocalMount { root_path } => root_path.clone(),
            LibraryKind::Managed => {
                return Err(DomainError::Validation(
                    "managed upload library cannot be scanned".into(),
                ));
            }
        };

        let existing_chapters = self.chapters.list_by_library(library.id).await?;
        let mut existing_by_path: HashMap<String, Chapter> = HashMap::new();
        let mut known_digests: HashMap<String, ExistingChapterDigest> = HashMap::new();

        for ch in existing_chapters {
            known_digests.insert(
                ch.source_path.clone(),
                ExistingChapterDigest {
                    file_size: ch.file_size,
                    file_hash: ch.file_hash.clone(),
                    page_count: ch.page_count,
                    updated_at: ch.updated_at,
                },
            );
            existing_by_path.insert(ch.source_path.clone(), ch);
        }

        let scanned_series = self
            .scanner
            .scan_directory(&root_path, &known_digests)
            .await?;
        let mut seen_paths: HashSet<String> = HashSet::new();
        let mut added = 0u64;
        let mut updated = 0u64;
        let mut scanned_chapters = 0u64;
        let now = Timestamp::now();

        for entry in scanned_series {
            if entry.chapters.is_empty() {
                continue;
            }

            let mut series = match self
                .series
                .find_by_library_and_dir(library.id, &entry.source_dir)
                .await?
            {
                Some(s) => s,
                None => Series::new(library.id, entry.title.clone(), entry.source_dir.clone())?,
            };

            let mut ordered_chapters: Vec<Chapter> = Vec::with_capacity(entry.chapters.len());

            for sc in entry.chapters {
                seen_paths.insert(sc.source_path.clone());
                scanned_chapters += 1;

                let mut chapter =
                    if let Some(mut existing) = existing_by_path.remove(&sc.source_path) {
                        let was_missing = existing.is_missing;
                        existing.is_missing = false;
                        if !sc.unchanged || was_missing {
                            existing.page_count = sc.page_count;
                            existing.file_size = sc.file_size;
                            existing.file_hash = sc.file_hash.clone();
                            existing.updated_at = now;
                            updated += 1;
                        }
                        existing
                    } else {
                        let mut created = Chapter::new(
                            series.id,
                            library.id,
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
                        created
                    };

                if let Some(ci) = &sc.comic_info
                    && !series.metadata_locked
                {
                    apply_comic_info_to_series(&mut series, ci);
                }

                if chapter.cover_path.is_none()
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
                    {
                        chapter.cover_path = Some(saved_key);
                    }
                }

                self.chapters.save(&chapter).await?;
                ordered_chapters.push(chapter);

                if scanned_chapters.is_multiple_of(10) {
                    library.mark_scanning(scanned_chapters);
                    let _ = self.libraries.save(&library).await;
                    self.events.publish(SystemEvent::LibraryScanProgress {
                        library_id: library.id,
                        media_kind: MediaKind::Comic,
                        scanned: scanned_chapters,
                    });
                }
            }

            ordered_chapters.sort_by(|a, b| a.cmp_order(b));
            series.chapter_count = ordered_chapters.len() as u32;
            if series.cover_path.is_none() {
                series.cover_path = ordered_chapters.iter().find_map(|c| c.cover_path.clone());
            }
            series.updated_at = now;
            self.series.save(&series).await?;
        }

        let mut missing_count = 0u64;
        let mut affected_series: HashSet<SeriesId> = HashSet::new();
        for (path, mut leftover) in existing_by_path {
            if !seen_paths.contains(&path) && !leftover.is_missing {
                leftover.is_missing = true;
                leftover.updated_at = now;
                affected_series.insert(leftover.series_id);
                self.chapters.save(&leftover).await?;
                missing_count += 1;
            }
        }

        for sid in affected_series {
            if let Some(mut s) = self.series.find_by_id(sid).await? {
                let active_count = self
                    .chapters
                    .list_by_series(sid)
                    .await?
                    .into_iter()
                    .filter(|c| !c.is_missing)
                    .count() as u32;
                s.chapter_count = active_count;
                s.updated_at = now;
                self.series.save(&s).await?;
            }
        }

        library.mark_scan_completed(scanned_chapters, added, updated, missing_count);
        self.libraries.save(&library).await?;

        self.events.publish(SystemEvent::LibraryScanCompleted {
            library_id: library.id,
            media_kind: MediaKind::Comic,
            scanned: scanned_chapters,
            added,
            updated,
            missing: missing_count,
        });
        if added > 0 || updated > 0 || missing_count > 0 {
            self.events.publish(SystemEvent::MediaChanged {
                media_kind: MediaKind::Comic,
            });
        }

        Ok(())
    }

    async fn count_library_items(&self, library_id: LibraryId) -> DomainResult<u64> {
        self.series.count_by_library(library_id).await
    }

    async fn on_library_deleted(&self, library_id: LibraryId) -> DomainResult<()> {
        let lib = self.libraries.find_by_id(library_id).await?;
        let is_managed = lib.is_some_and(|l| l.kind.is_managed());
        let lib_series = self.series.list_by_library(library_id).await?;
        for s in lib_series {
            if is_managed {
                for ch in self.chapters.list_by_series(s.id).await? {
                    let _ = self.storage.delete_managed_file(&ch.source_path).await;
                }
            }
            self.reading.delete_by_series(s.id).await?;
            self.chapters.delete_by_series(s.id).await?;
            self.series.delete(s.id).await?;
        }
        Ok(())
    }

    async fn on_user_deleted(&self, user_id: UserId) -> DomainResult<()> {
        self.reading.delete_by_user(user_id).await
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

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use async_trait::async_trait;
use jiff::Timestamp;
use xmedia_core::application::bindings::MediaBindingService;
use xmedia_core::application::library::ScanProgressReporter;
use xmedia_core::application::ports::{MediaDomainLifecycle, RealtimeEventPort};
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::{Library, LibraryId, LibraryKind, MediaKind};
use xmedia_core::domain::repository::{BindingDeleteScope, LibraryRepository};
use xmedia_core::domain::user::UserId;

use crate::application::ports::{
    ArchiveReaderPort, ComicFileStoragePort, ComicScannerPort, ExistingChapterDigest,
    ThumbnailGeneratorPort,
};
use crate::domain::chapter::{Chapter, ChapterScanUpdate, ChapterSource, ChapterSourceKind};
use crate::domain::id::SeriesId;
use crate::domain::repository::{
    ChapterDeleteScope, ChapterQuery, ChapterRepository, ComicReadingDeleteScope,
    ComicReadingRepository, SeriesQuery, SeriesRepository,
};
use crate::domain::series::{CoverUpdate, Series, SeriesSource};

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
        let existing_series_list = self.series.query(SeriesQuery::Library(library_id)).await?;
        let series_info_by_id: HashMap<SeriesId, (String, String)> = existing_series_list
            .iter()
            .filter_map(|s| match &s.source {
                SeriesSource::Local { source_dir } => {
                    Some((s.id, (s.title.clone(), source_dir.clone())))
                }
                SeriesSource::OnlineTracked(_) => None,
            })
            .collect();
        let existing_chapters = self
            .chapters
            .query(ChapterQuery::Library(library_id))
            .await?;
        let mut existing_by_path: HashMap<String, Chapter> = HashMap::new();
        let mut known_digests: HashMap<String, ExistingChapterDigest> = HashMap::new();

        for ch in existing_chapters {
            let local_path = match &ch.source {
                ChapterSource::Archive { path } | ChapterSource::ImageFolder { path } => {
                    path.clone()
                }
                ChapterSource::OnlineTracked(_) => continue,
            };
            let (series_title, series_source_dir) = series_info_by_id
                .get(&ch.series_id)
                .map(|(t, d)| (Some(t.clone()), Some(d.clone())))
                .unwrap_or((None, None));
            known_digests.insert(
                local_path.clone(),
                ExistingChapterDigest {
                    file_size: ch.file_size,
                    file_hash: ch.file_hash.clone(),
                    page_count: ch.page_count,
                    updated_at: ch.updated_at,
                    series_title,
                    series_source_dir,
                },
            );
            existing_by_path.insert(local_path, ch);
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
            .filter_map(|s| {
                let SeriesSource::Local { ref source_dir } = s.source else {
                    return None;
                };
                let dir = source_dir.clone();
                Some((dir, s))
            })
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
                    Series::new(
                        library_id,
                        entry.title.clone(),
                        SeriesSource::Local {
                            source_dir: entry.source_dir.clone(),
                        },
                    )?,
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
                let scanned_source = match sc.source_kind {
                    ChapterSourceKind::ImageDirectory => ChapterSource::ImageFolder {
                        path: sc.source_path.clone(),
                    },
                    ChapterSourceKind::ArchiveFile => ChapterSource::Archive {
                        path: sc.source_path.clone(),
                    },
                    ChapterSourceKind::OnlineTracked => {
                        return Err(DomainError::Validation(
                            "online_tracked requires OnlineTrackedChapterMeta".into(),
                        ));
                    }
                };

                let (mut chapter, refresh_cover, mut chapter_dirty) =
                    if let Some(mut existing) = existing_by_path.remove(&sc.source_path) {
                        let was_missing = existing.is_missing;
                        let mut dirty = false;
                        if !sc.unchanged {
                            existing.sync_scan(
                                ChapterScanUpdate::Metrics {
                                    kind: sc.kind,
                                    number: sc.number,
                                    volume_number: sc.volume_number,
                                    title: sc.title.clone(),
                                    page_count: sc.page_count,
                                    file_size: sc.file_size,
                                    file_hash: sc.file_hash.clone(),
                                },
                                now,
                            );
                            if let Some(ci) = &sc.comic_info {
                                existing.apply_comic_info(ci);
                            }
                            updated += 1;
                            dirty = true;
                        } else if was_missing {
                            existing.sync_scan(ChapterScanUpdate::Present, now);
                            updated += 1;
                            dirty = true;
                        }
                        if let (_, Some(prev_sid)) =
                            existing.sync_scan(ChapterScanUpdate::ReassignSeries(series.id), now)
                        {
                            affected_series.insert(prev_sid);
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
                            if let (_, Some(prev_sid)) = migrated.sync_scan(
                                ChapterScanUpdate::MigrateLocal {
                                    series_id: series.id,
                                    source: scanned_source,
                                    page_count: sc.page_count,
                                    file_size: sc.file_size,
                                    file_hash: sc.file_hash.clone(),
                                },
                                now,
                            ) {
                                affected_series.insert(prev_sid);
                            }
                            if let Some(ci) = &sc.comic_info {
                                migrated.apply_comic_info(ci);
                            }
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
                                scanned_source,
                                sc.file_size,
                                sc.file_hash.clone(),
                            )?;
                            if let Some(ci) = &sc.comic_info {
                                created.apply_comic_info(ci);
                            }
                            added += 1;
                            (created, true, true)
                        }
                    };

                if let Some(ci) = &sc.comic_info
                    && !series.metadata_locked
                {
                    series.apply_comic_info(ci);
                }

                if refresh_cover
                    && chapter.page_count > 0
                    && let Some((eff_kind, eff_path)) = chapter.source.resolved_local()
                    && let Ok(first_page) = self
                        .archives
                        .read_page(eff_kind, eff_path, 1, &chapter.file_hash)
                        .await
                {
                    let thumb_key = format!("ch_{}", chapter.id);
                    if let Ok(saved_key) = self
                        .thumbnails
                        .generate_from_bytes(&thumb_key, &first_page.bytes)
                        .await
                        && chapter
                            .sync_scan(ChapterScanUpdate::Cover(saved_key), now)
                            .0
                    {
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

            ordered_chapters.sort_by(Chapter::cmp_order);
            let new_chapter_count = ordered_chapters.len() as u32;
            if series.sync_chapter_count(new_chapter_count, now) {
                series_dirty = true;
            }
            if let Some(ref cov_bytes) = entry.series_cover_bytes
                && (series.cover_path.is_none() || (!series.metadata_locked && series_dirty))
                && !series.cover_is_custom
            {
                let thumb_key = format!("series_{}", series.id);
                if let Ok(saved_key) = self
                    .thumbnails
                    .generate_from_bytes(&thumb_key, cov_bytes)
                    .await
                    && series.apply_cover(CoverUpdate::Scanned(saved_key), now)
                {
                    series_dirty = true;
                }
            }
            if series.cover_path.is_none()
                && let Some(fallback_cover) =
                    ordered_chapters.iter().find_map(|c| c.cover_path.clone())
                && series.apply_cover(CoverUpdate::Fallback(fallback_cover), now)
            {
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
            if leftover.source.online_tracking().is_some() {
                continue;
            }
            if !seen_paths.contains(&path) && !leftover.is_missing {
                leftover.sync_scan(ChapterScanUpdate::Missing, now);
                affected_series.insert(leftover.series_id);
                self.chapters.save(&leftover).await?;
                missing_count += 1;
            }
        }

        for sid in affected_series {
            let chapters_in_series = self.chapters.query(ChapterQuery::Series(sid)).await?;
            if chapters_in_series.is_empty() {
                let _ = self
                    .bindings
                    .purge(BindingDeleteScope::Item {
                        media_kind: MediaKind::Comic,
                        item_id: sid.0,
                    })
                    .await;
                let _ = self
                    .reading
                    .delete_scope(ComicReadingDeleteScope::Series(sid))
                    .await;
                let _ = self.series.delete(sid).await;
            } else if let Some(mut s) = self
                .series
                .query(SeriesQuery::Id(sid))
                .await?
                .into_iter()
                .next()
            {
                let active_count = chapters_in_series
                    .into_iter()
                    .filter(|c| !c.is_missing)
                    .count() as u32;
                let _ = s.sync_chapter_count(active_count, now);
                s.updated_at = now;
                self.series.save(&s).await?;
            }
        }

        reporter.complete(added, updated, missing_count).await?;

        Ok(())
    }

    async fn count_library_items(&self, library_id: LibraryId) -> DomainResult<u64> {
        let series = self.series.query(SeriesQuery::Library(library_id)).await?;
        Ok(series
            .into_iter()
            .filter(|s| s.source.online_tracking().is_none())
            .count() as u64)
    }

    async fn on_library_deleted(&self, library_id: LibraryId) -> DomainResult<()> {
        let lib = self.libraries.find_by_id(library_id).await?;
        let is_managed = lib.is_some_and(|l| l.kind.is_managed());
        let lib_series = self.series.query(SeriesQuery::Library(library_id)).await?;
        for s in lib_series {
            if is_managed {
                for ch in self.chapters.query(ChapterQuery::Series(s.id)).await? {
                    if let Some((ChapterSourceKind::ArchiveFile, path)) = ch.source.resolved_local()
                    {
                        let _ = self.storage.delete_managed_file(path).await;
                    }
                }
            }
            let _ = self
                .bindings
                .purge(BindingDeleteScope::Item {
                    media_kind: MediaKind::Comic,
                    item_id: s.id.0,
                })
                .await?;
            self.reading
                .delete_scope(ComicReadingDeleteScope::Series(s.id))
                .await?;
            self.chapters
                .delete_scope(ChapterDeleteScope::Series(s.id))
                .await?;
            self.series.delete(s.id).await?;
        }
        Ok(())
    }

    async fn on_user_deleted(&self, user_id: UserId) -> DomainResult<()> {
        self.reading
            .delete_scope(ComicReadingDeleteScope::User(user_id))
            .await?;
        let unbound_ids = self
            .bindings
            .purge(BindingDeleteScope::UserMedia {
                user_id,
                media_kind: MediaKind::Comic,
            })
            .await?;
        for raw_id in unbound_ids {
            let sid = SeriesId(raw_id);
            self.bindings
                .gc_if_orphaned(MediaKind::Comic, raw_id, || async {
                    if let Some(series) = self
                        .series
                        .query(SeriesQuery::Id(sid))
                        .await?
                        .into_iter()
                        .next()
                        && series.source.online_tracking().is_some()
                    {
                        for ch in self.chapters.query(ChapterQuery::Series(sid)).await? {
                            if let Some((ChapterSourceKind::ArchiveFile, path)) =
                                ch.source.resolved_local()
                            {
                                let _ = self.storage.delete_managed_file(path).await;
                            }
                        }
                        self.reading
                            .delete_scope(ComicReadingDeleteScope::Series(sid))
                            .await?;
                        self.chapters
                            .delete_scope(ChapterDeleteScope::Series(sid))
                            .await?;
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

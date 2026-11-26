use std::collections::{HashMap, HashSet};
use std::path::Path;
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
use xmedia_formats::video::parse_video_path;

use crate::application::ingest::VideoIngestService;
use crate::application::ports::{VideoMediaEnginePort, VideoScannerPort, VideoStoragePort};
use crate::domain::id::{ShowId, VideoId};
use crate::domain::repository::{
    ShowScope, VideoCatalogDelete, VideoCatalogMutation, VideoCatalogRepository, VideoScope,
    VideoWatchPurge, VideoWatchRepository,
};
use crate::domain::video::{ShowQueryFilter, VideoItem, VideoSource};

#[derive(Clone)]
pub struct VideoDomainLifecycle {
    libraries: Arc<dyn LibraryRepository>,
    catalog_repo: Arc<dyn VideoCatalogRepository>,
    watch_repo: Arc<dyn VideoWatchRepository>,
    bindings: Arc<MediaBindingService>,
    ingest: Arc<VideoIngestService>,
    storage: Arc<dyn VideoStoragePort>,
    scanner: Arc<dyn VideoScannerPort>,
    engine: Arc<dyn VideoMediaEnginePort>,
    events: Arc<dyn RealtimeEventPort>,
}

impl VideoDomainLifecycle {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        libraries: Arc<dyn LibraryRepository>,
        catalog_repo: Arc<dyn VideoCatalogRepository>,
        watch_repo: Arc<dyn VideoWatchRepository>,
        bindings: Arc<MediaBindingService>,
        ingest: Arc<VideoIngestService>,
        storage: Arc<dyn VideoStoragePort>,
        scanner: Arc<dyn VideoScannerPort>,
        engine: Arc<dyn VideoMediaEnginePort>,
        events: Arc<dyn RealtimeEventPort>,
    ) -> Self {
        Self {
            libraries,
            catalog_repo,
            watch_repo,
            bindings,
            ingest,
            storage,
            scanner,
            engine,
            events,
        }
    }
}

#[async_trait]
impl MediaDomainLifecycle for VideoDomainLifecycle {
    fn media_kind(&self) -> MediaKind {
        MediaKind::Video
    }

    async fn scan_library(&self, library: Library) -> DomainResult<()> {
        let LibraryKind::LocalMount { ref root_path } = library.kind else {
            return Err(DomainError::Validation(
                "cannot scan managed library".into(),
            ));
        };
        let library_id = library.id;
        let root_path = root_path.clone();

        let entries = self.scanner.scan_video_entries(&root_path).await?;
        let entry_paths: HashSet<String> =
            entries.iter().map(|e| e.relative_path.clone()).collect();
        let existing_videos = self
            .catalog_repo
            .query_videos(VideoScope::Library(library_id))
            .await?;
        let mut existing_by_rel: HashMap<String, VideoItem> = HashMap::new();
        for v in existing_videos {
            if let VideoSource::LocalPath {
                ref relative_path, ..
            } = v.source
            {
                existing_by_rel.insert(relative_path.clone(), v);
            }
        }

        let total = entries.len() as u64;
        let mut reporter = ScanProgressReporter::new(
            library,
            self.libraries.clone(),
            self.events.clone(),
            Some(total),
        );
        reporter.start().await;

        let mut seen_rel: HashSet<String> = HashSet::new();
        let mut added_count = 0u64;
        let mut updated_count = 0u64;

        for entry in entries {
            seen_rel.insert(entry.relative_path.clone());

            let exact_prev = existing_by_rel.remove(&entry.relative_path);
            if let Some(mut existing) = exact_prev.clone()
                && let VideoSource::LocalPath {
                    file_mtime,
                    ref sidecar_subtitles,
                    ..
                } = existing.source
                && file_mtime == entry.file_mtime
                && existing.file_size == entry.file_size
                && sidecar_subtitles == &entry.sidecar_subtitles
            {
                if existing.is_missing {
                    existing.is_missing = false;
                    existing.updated_at = Timestamp::now();
                    self.catalog_repo
                        .save(VideoCatalogMutation::Video(&existing))
                        .await?;
                    updated_count += 1;
                }
                reporter.advance().await;
                continue;
            }

            let file_hash = self
                .scanner
                .compute_file_fingerprint(Path::new(&entry.absolute_path))
                .await
                .unwrap_or_else(|_| format!("{}:{}", entry.file_size, entry.file_mtime));

            let exact_was_found = exact_prev.is_some();
            let prev_item = match exact_prev {
                Some(prev) => Some(prev),
                None => {
                    let matched_key = existing_by_rel
                        .iter()
                        .find(|(old_rel, old_v)| {
                            !entry_paths.contains(*old_rel) && old_v.file_hash == file_hash
                        })
                        .map(|(k, _)| k.clone())
                        .or_else(|| {
                            let legacy: Vec<String> = existing_by_rel
                                .iter()
                                .filter(|(old_rel, old_v)| {
                                    !entry_paths.contains(*old_rel)
                                        && old_v.file_size == entry.file_size
                                        && old_v.file_hash.contains(':')
                                })
                                .map(|(k, _)| k.clone())
                                .collect();
                            (legacy.len() == 1).then(|| legacy[0].clone())
                        });
                    matched_key.and_then(|k| existing_by_rel.remove(&k))
                }
            };

            let prev_ref = prev_item.as_ref();
            let preserve_moved_edits = !exact_was_found
                && entry.nfo.is_none()
                && prev_ref.is_some_and(|m| {
                    matches!(
                        &m.source,
                        VideoSource::LocalPath {
                            file_mtime,
                            sidecar_subtitles,
                            ..
                        } if *file_mtime == entry.file_mtime
                            && sidecar_subtitles == &entry.sidecar_subtitles
                    )
                });
            let item = if preserve_moved_edits && let Some(mut migrated) = prev_item.clone() {
                migrated.source = VideoSource::LocalPath {
                    relative_path: entry.relative_path,
                    file_mtime: entry.file_mtime,
                    sidecar_subtitles: entry.sidecar_subtitles,
                };
                migrated.file_hash = file_hash;
                migrated.is_missing = false;
                migrated.updated_at = Timestamp::now();
                migrated
            } else {
                let video_id = prev_ref.map(|v| v.id).unwrap_or_default();
                let probed = self
                    .engine
                    .probe_video(Path::new(&entry.absolute_path))
                    .await;
                let parsed_name = parse_video_path(&entry.relative_path);

                let poster_key = if let Some(ref dir_poster) = entry.directory_poster_path
                    && let Ok(Some(bytes)) = self.scanner.read_external_image(dir_poster).await
                {
                    self.storage.save_poster_bytes(&bytes).await.ok()
                } else if let Some(existing_pk) = prev_ref.and_then(|v| v.poster_key.clone()) {
                    Some(existing_pk)
                } else if let Ok(frame_bytes) = self
                    .engine
                    .generate_poster_bytes(
                        Path::new(&entry.absolute_path),
                        probed.duration_secs,
                        &parsed_name.title,
                    )
                    .await
                {
                    self.storage
                        .save_generated_poster_bytes(&frame_bytes)
                        .await
                        .ok()
                } else {
                    None
                };

                let rel_path_clone = entry.relative_path.clone();
                self.ingest
                    .build_and_upsert_video(
                        library_id,
                        video_id,
                        prev_ref.map(|v| v.created_at),
                        &rel_path_clone,
                        entry.container,
                        entry.file_size,
                        file_hash,
                        probed,
                        entry.nfo,
                        poster_key,
                        VideoSource::LocalPath {
                            relative_path: entry.relative_path,
                            file_mtime: entry.file_mtime,
                            sidecar_subtitles: entry.sidecar_subtitles,
                        },
                    )
                    .await?
            };

            self.catalog_repo
                .save(VideoCatalogMutation::Video(&item))
                .await?;
            if prev_ref.is_some() {
                updated_count += 1;
            } else {
                added_count += 1;
            }

            reporter.advance().await;
        }

        let mut missing_count = 0u64;
        for (rel, mut old_item) in existing_by_rel {
            if !seen_rel.contains(&rel) && !old_item.is_missing {
                old_item.is_missing = true;
                old_item.updated_at = Timestamp::now();
                self.catalog_repo
                    .save(VideoCatalogMutation::Video(&old_item))
                    .await?;
                missing_count += 1;
            }
        }

        if added_count > 0 || updated_count > 0 || missing_count > 0 {
            self.catalog_repo
                .save(VideoCatalogMutation::RecomputeShowAggregates)
                .await?;
        }
        reporter
            .complete(added_count, updated_count, missing_count)
            .await?;

        Ok(())
    }

    async fn count_library_items(&self, library_id: LibraryId) -> DomainResult<u64> {
        let videos = self
            .catalog_repo
            .query_videos(VideoScope::Library(library_id))
            .await?;
        Ok(videos
            .into_iter()
            .filter(|v| !v.is_missing && v.source.online_tracking_info().is_none())
            .count() as u64)
    }

    async fn on_library_deleted(&self, library_id: LibraryId) -> DomainResult<()> {
        let videos = self
            .catalog_repo
            .query_videos(VideoScope::Library(library_id))
            .await?;
        let shows = self
            .catalog_repo
            .query_shows(ShowScope::Filter(&ShowQueryFilter {
                library_id: Some(library_id),
                ..Default::default()
            }))
            .await?;

        let mut video_ids = Vec::with_capacity(videos.len());
        let mut standalone_binding_ids = Vec::new();
        let mut show_id_set: HashSet<ShowId> = shows.into_iter().map(|s| s.id).collect();
        for v in &videos {
            for key in v.source.managed_file_keys() {
                let _ = self.storage.delete_managed_file(key).await;
            }
            video_ids.push(v.id);
            if let Some(sid) = v.show_id {
                show_id_set.insert(sid);
            } else {
                standalone_binding_ids.push(v.id.0);
            }
        }
        let show_ids: Vec<ShowId> = show_id_set.into_iter().collect();

        for raw_id in standalone_binding_ids {
            let _ = self
                .bindings
                .purge(BindingDeleteScope::Item {
                    media_kind: MediaKind::Video,
                    item_id: raw_id,
                })
                .await?;
        }
        for sid in &show_ids {
            let _ = self
                .bindings
                .purge(BindingDeleteScope::Item {
                    media_kind: MediaKind::Video,
                    item_id: sid.0,
                })
                .await?;
        }

        self.watch_repo
            .purge(VideoWatchPurge::LibraryReferences {
                video_ids: &video_ids,
                show_ids: &show_ids,
            })
            .await?;
        self.catalog_repo
            .delete(VideoCatalogDelete::LibraryVideos(library_id))
            .await?;
        self.catalog_repo
            .save(VideoCatalogMutation::RecomputeShowAggregates)
            .await
    }

    async fn on_user_deleted(&self, user_id: UserId) -> DomainResult<()> {
        self.watch_repo
            .purge(VideoWatchPurge::User(user_id))
            .await?;
        let unbound = self
            .bindings
            .purge(BindingDeleteScope::UserMedia {
                user_id,
                media_kind: MediaKind::Video,
            })
            .await?;
        for raw_id in unbound {
            self.bindings
                .gc_if_orphaned(MediaKind::Video, raw_id, || async {
                    if let Some(show) = self
                        .catalog_repo
                        .query_shows(ShowScope::Id(ShowId(raw_id)))
                        .await?
                        .into_iter()
                        .next()
                    {
                        if show.online_tracked().is_some() {
                            self.ingest.delete_show_without_bindings(show.id).await?;
                            return Ok(true);
                        }
                    } else if let Some(video) = self
                        .catalog_repo
                        .query_videos(VideoScope::Id(VideoId(raw_id)))
                        .await?
                        .into_iter()
                        .next()
                        && video.show_id.is_none()
                        && video.source.online_tracking_info().is_some()
                    {
                        self.ingest.delete_video_without_bindings(video).await?;
                        return Ok(true);
                    }
                    Ok(false)
                })
                .await?;
        }
        Ok(())
    }
}

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use jiff::Timestamp;
use xmedia_core::application::library::ScanProgressReporter;
use xmedia_core::application::ports::{MediaDomainLifecycle, RealtimeEventPort, SystemEvent};
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::{Library, LibraryId, LibraryKind, MediaKind};
use xmedia_core::domain::repository::LibraryRepository;
use xmedia_core::domain::user::{User, UserId, require_admin};

use crate::application::catalog::VideoCatalogService;
use crate::application::dto::{VideoUploadItem, VideoUploadPathItem, VideoUploadResultDto};
use crate::application::ports::{
    ParsedNfoMetadata, ProbedVideoInfo, VideoMediaEnginePort, VideoScannerPort, VideoStoragePort,
};
use crate::domain::filename_parser::{parse_sidecar_subtitle_lang_and_label, parse_video_path};
use crate::domain::id::{ShowId, VideoId};
use crate::domain::repository::{VideoCatalogRepository, VideoWatchRepository};
use crate::domain::video::{
    ShowQueryFilter, SidecarSubtitle, VideoContainer, VideoItem, VideoShow, VideoSource,
    is_subtitle_extension,
};

#[derive(Clone)]
pub struct VideoDomainLifecycle {
    libraries: Arc<dyn LibraryRepository>,
    catalog_repo: Arc<dyn VideoCatalogRepository>,
    watch_repo: Arc<dyn VideoWatchRepository>,
    catalog_service: VideoCatalogService,
    storage: Arc<dyn VideoStoragePort>,
    scanner: Arc<dyn VideoScannerPort>,
    engine: Arc<dyn VideoMediaEnginePort>,
    events: Arc<dyn RealtimeEventPort>,
}

impl VideoDomainLifecycle {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        libraries: Arc<dyn LibraryRepository>,
        catalog_repo: Arc<dyn VideoCatalogRepository>,
        watch_repo: Arc<dyn VideoWatchRepository>,
        catalog_service: VideoCatalogService,
        storage: Arc<dyn VideoStoragePort>,
        scanner: Arc<dyn VideoScannerPort>,
        engine: Arc<dyn VideoMediaEnginePort>,
        events: Arc<dyn RealtimeEventPort>,
    ) -> Self {
        Self {
            libraries,
            catalog_repo,
            watch_repo,
            catalog_service,
            storage,
            scanner,
            engine,
            events,
        }
    }

    pub async fn upload_files(
        &self,
        actor: &User,
        files: Vec<VideoUploadItem>,
    ) -> DomainResult<VideoUploadResultDto> {
        require_admin(actor)?;
        if files.is_empty() {
            return Err(DomainError::Validation("no files uploaded".into()));
        }

        let managed_lib = self.ensure_managed_library().await?;

        let mut video_items: Vec<(String, VideoContainer, Bytes)> = Vec::new();
        let mut sub_items: Vec<(String, String, Bytes)> = Vec::new();

        for f in files {
            let (stem, ext) = split_filename_stem_and_ext(&f.filename);
            if let Some(container) = VideoContainer::from_extension(&ext) {
                video_items.push((f.filename, container, f.bytes));
            } else if is_subtitle_extension(&ext) {
                sub_items.push((f.filename, ext.to_ascii_lowercase(), f.bytes));
            } else {
                return Err(DomainError::UnsupportedFormat(format!(
                    "unsupported file extension '.{ext}' (stem '{stem}')"
                )));
            }
        }

        let mut created_videos = Vec::new();
        let mut attached_subtitles_count = 0usize;
        let video_count = video_items.len();

        for (idx, (filename, container, bytes)) in video_items.into_iter().enumerate() {
            let video_id = VideoId::new();
            let stored = self
                .storage
                .save_managed_video(video_id, container, bytes)
                .await?;
            let (video_stem, _) = split_filename_stem_and_ext(&filename);

            let mut sidecar_subtitles = Vec::new();
            for (s_idx, (sub_name, sub_ext, sub_bytes)) in sub_items.iter().enumerate() {
                let (sub_stem, _) = split_filename_stem_and_ext(sub_name);
                let matches_this_video = video_count == 1
                    || sub_stem.eq_ignore_ascii_case(&video_stem)
                    || sub_stem
                        .to_lowercase()
                        .starts_with(&format!("{}.", video_stem.to_lowercase()))
                    || (idx == 0 && s_idx == 0 && video_count == 0);

                if matches_this_video {
                    let sub_id = format!("s{s_idx}");
                    let key = self
                        .storage
                        .save_managed_subtitle(video_id, &sub_id, sub_ext, sub_bytes.clone())
                        .await?;
                    let (lang, label) =
                        parse_sidecar_subtitle_lang_and_label(&video_stem, sub_name);
                    sidecar_subtitles.push(SidecarSubtitle {
                        id: sub_id,
                        format: sub_ext.clone(),
                        language: lang,
                        label,
                        path_or_key: key,
                    });
                    attached_subtitles_count += 1;
                }
            }

            let probed = self
                .engine
                .probe_video(Path::new(&stored.absolute_path))
                .await;
            let parsed = parse_video_path(&filename);
            let poster_bytes = self
                .engine
                .generate_poster_bytes(
                    Path::new(&stored.absolute_path),
                    probed.duration_secs,
                    &parsed.title,
                )
                .await
                .ok();
            let poster_key = match poster_bytes {
                Some(ref b) => self.storage.save_poster_bytes(b).await.ok(),
                None => None,
            };

            let item = self
                .build_and_upsert_video(
                    managed_lib.id,
                    video_id,
                    None,
                    &filename,
                    container,
                    stored.file_size,
                    stored.file_hash,
                    probed,
                    None,
                    poster_key,
                    VideoSource::Managed {
                        storage_key: stored.storage_key,
                        sidecar_subtitles,
                    },
                )
                .await?;

            self.catalog_repo.save_video(&item).await?;
            created_videos.push(item);
        }

        self.catalog_repo.recompute_show_aggregates().await?;
        self.events.publish(SystemEvent::MediaChanged {
            media_kind: MediaKind::Video,
        });

        let uploaded_videos = self
            .catalog_service
            .enrich_videos(actor.id, created_videos)
            .await?;

        Ok(VideoUploadResultDto {
            uploaded_videos,
            attached_subtitles_count,
        })
    }

    pub async fn upload_files_from_paths(
        &self,
        actor: &User,
        files: Vec<VideoUploadPathItem>,
    ) -> DomainResult<VideoUploadResultDto> {
        require_admin(actor)?;
        if files.is_empty() {
            return Err(DomainError::Validation("no files uploaded".into()));
        }

        let managed_lib = self.ensure_managed_library().await?;

        let mut video_items: Vec<(String, VideoContainer, PathBuf)> = Vec::new();
        let mut sub_items: Vec<(String, String, PathBuf)> = Vec::new();

        for f in files {
            let (stem, ext) = split_filename_stem_and_ext(&f.filename);
            if let Some(container) = VideoContainer::from_extension(&ext) {
                video_items.push((f.filename, container, f.temp_path));
            } else if is_subtitle_extension(&ext) {
                sub_items.push((f.filename, ext.to_ascii_lowercase(), f.temp_path));
            } else {
                return Err(DomainError::UnsupportedFormat(format!(
                    "unsupported file extension '.{ext}' (stem '{stem}')"
                )));
            }
        }

        let mut created_videos = Vec::new();
        let mut attached_subtitles_count = 0usize;
        let video_count = video_items.len();

        for (filename, container, temp_path) in video_items {
            let video_id = VideoId::new();
            let stored = self
                .storage
                .save_managed_video_from_path(video_id, container, &temp_path)
                .await?;
            let (video_stem, _) = split_filename_stem_and_ext(&filename);

            let mut sidecar_subtitles = Vec::new();
            for (s_idx, (sub_name, sub_ext, sub_path)) in sub_items.iter().enumerate() {
                let (sub_stem, _) = split_filename_stem_and_ext(sub_name);
                let matches_this_video = video_count == 1
                    || sub_stem.eq_ignore_ascii_case(&video_stem)
                    || sub_stem
                        .to_lowercase()
                        .starts_with(&format!("{}.", video_stem.to_lowercase()));

                if matches_this_video {
                    let sub_id = format!("s{s_idx}");
                    let key = self
                        .storage
                        .save_managed_subtitle_from_path(video_id, &sub_id, sub_ext, sub_path)
                        .await?;
                    let (lang, label) =
                        parse_sidecar_subtitle_lang_and_label(&video_stem, sub_name);
                    sidecar_subtitles.push(SidecarSubtitle {
                        id: sub_id,
                        format: sub_ext.clone(),
                        language: lang,
                        label,
                        path_or_key: key,
                    });
                    attached_subtitles_count += 1;
                }
            }

            let probed = self
                .engine
                .probe_video(Path::new(&stored.absolute_path))
                .await;
            let parsed = parse_video_path(&filename);
            let poster_bytes = self
                .engine
                .generate_poster_bytes(
                    Path::new(&stored.absolute_path),
                    probed.duration_secs,
                    &parsed.title,
                )
                .await
                .ok();
            let poster_key = match poster_bytes {
                Some(ref b) => self.storage.save_poster_bytes(b).await.ok(),
                None => None,
            };

            let item = self
                .build_and_upsert_video(
                    managed_lib.id,
                    video_id,
                    None,
                    &filename,
                    container,
                    stored.file_size,
                    stored.file_hash,
                    probed,
                    None,
                    poster_key,
                    VideoSource::Managed {
                        storage_key: stored.storage_key,
                        sidecar_subtitles,
                    },
                )
                .await?;

            self.catalog_repo.save_video(&item).await?;
            created_videos.push(item);
        }

        self.catalog_repo.recompute_show_aggregates().await?;
        self.events.publish(SystemEvent::MediaChanged {
            media_kind: MediaKind::Video,
        });

        let uploaded_videos = self
            .catalog_service
            .enrich_videos(actor.id, created_videos)
            .await?;

        Ok(VideoUploadResultDto {
            uploaded_videos,
            attached_subtitles_count,
        })
    }

    async fn ensure_managed_library(&self) -> DomainResult<Library> {
        if let Some(existing) = self
            .libraries
            .find_managed_by_media(MediaKind::Video)
            .await?
        {
            return Ok(existing);
        }
        let lib = Library::new_managed(MediaKind::Video, MediaKind::Video.default_managed_name())?;
        self.libraries.save(&lib).await?;
        Ok(lib)
    }

    #[allow(clippy::too_many_arguments)]
    async fn build_and_upsert_video(
        &self,
        library_id: LibraryId,
        video_id: VideoId,
        existing_created_at: Option<Timestamp>,
        path_or_filename: &str,
        container: VideoContainer,
        file_size: u64,
        file_hash: String,
        probed: ProbedVideoInfo,
        nfo: Option<ParsedNfoMetadata>,
        poster_key: Option<String>,
        source: VideoSource,
    ) -> DomainResult<VideoItem> {
        let now = Timestamp::now();
        let parsed = parse_video_path(path_or_filename);

        let title = nfo
            .as_ref()
            .and_then(|n| n.title.clone())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(parsed.title);
        let show_title = nfo
            .as_ref()
            .and_then(|n| n.show_title.clone())
            .filter(|s| !s.trim().is_empty())
            .or(parsed.show_title);
        let season_number = nfo.as_ref().and_then(|n| n.season).or(parsed.season_number);
        let episode_number = nfo
            .as_ref()
            .and_then(|n| n.episode)
            .or(parsed.episode_number);
        let year = nfo.as_ref().and_then(|n| n.year).or(parsed.year);
        let summary = nfo
            .as_ref()
            .and_then(|n| n.summary.clone())
            .unwrap_or_default();
        let genres = nfo.as_ref().map(|n| n.genres.clone()).unwrap_or_default();
        let tags = nfo.as_ref().map(|n| n.tags.clone()).unwrap_or_default();

        let show_id = if let Some(ref s_title) = show_title {
            let show = self
                .upsert_show(library_id, s_title, year, &genres, poster_key.as_deref())
                .await?;
            Some(show.id)
        } else {
            None
        };

        Ok(VideoItem {
            id: video_id,
            library_id,
            show_id,
            show_title,
            season_number,
            episode_number,
            title,
            summary,
            year,
            genres,
            tags,
            container,
            file_size,
            file_hash,
            duration_secs: probed.duration_secs,
            bit_rate_kbps: probed.bit_rate_kbps,
            width: probed.width,
            height: probed.height,
            video_codec: probed.video_codec,
            audio_streams: probed.audio_streams,
            subtitle_streams: probed.subtitle_streams,
            poster_key,
            source,
            is_missing: false,
            created_at: existing_created_at.unwrap_or(now),
            updated_at: now,
        })
    }

    async fn upsert_show(
        &self,
        library_id: LibraryId,
        title: &str,
        year: Option<i32>,
        genres: &[String],
        poster_key: Option<&str>,
    ) -> DomainResult<VideoShow> {
        let now = Timestamp::now();
        let show_key = VideoShow::compute_show_key(library_id, title);
        if let Some(mut existing) = self.catalog_repo.find_show_by_key(&show_key).await? {
            let mut dirty = false;
            if existing.year.is_none() && year.is_some() {
                existing.year = year;
                dirty = true;
            }
            if existing.genres.is_empty() && !genres.is_empty() {
                existing.genres = genres.to_vec();
                dirty = true;
            }
            if existing.poster_key.is_none()
                && let Some(pk) = poster_key
            {
                existing.poster_key = Some(pk.to_string());
                dirty = true;
            }
            if dirty {
                existing.updated_at = now;
                self.catalog_repo.save_show(&existing).await?;
            }
            return Ok(existing);
        }

        let created = VideoShow {
            id: ShowId::new(),
            library_id,
            show_key,
            title: title.trim().to_string(),
            summary: String::new(),
            year,
            genres: genres.to_vec(),
            tags: Vec::new(),
            poster_key: poster_key.map(str::to_string),
            season_count: 1,
            episode_count: 0,
            created_at: now,
            updated_at: now,
        };
        self.catalog_repo.save_show(&created).await?;
        Ok(created)
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
        let existing_videos = self.catalog_repo.list_videos_by_library(library_id).await?;
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
                    self.catalog_repo.save_video(&existing).await?;
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
                    self.storage.save_poster_bytes(&frame_bytes).await.ok()
                } else {
                    None
                };

                let rel_path_clone = entry.relative_path.clone();
                self.build_and_upsert_video(
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

            self.catalog_repo.save_video(&item).await?;
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
                self.catalog_repo.save_video(&old_item).await?;
                missing_count += 1;
            }
        }

        if added_count > 0 || updated_count > 0 || missing_count > 0 {
            self.catalog_repo.recompute_show_aggregates().await?;
        }
        reporter
            .complete(added_count, updated_count, missing_count)
            .await?;

        Ok(())
    }

    async fn count_library_items(&self, library_id: LibraryId) -> DomainResult<u64> {
        self.catalog_repo.count_active_by_library(library_id).await
    }

    async fn on_library_deleted(&self, library_id: LibraryId) -> DomainResult<()> {
        let videos = self.catalog_repo.list_videos_by_library(library_id).await?;
        let shows = self
            .catalog_repo
            .list_shows(&ShowQueryFilter {
                library_id: Some(library_id),
                ..Default::default()
            })
            .await?;

        let mut video_ids = Vec::with_capacity(videos.len());
        let mut show_id_set: HashSet<ShowId> = shows.into_iter().map(|s| s.id).collect();
        for v in &videos {
            if let VideoSource::Managed {
                ref storage_key,
                ref sidecar_subtitles,
            } = v.source
            {
                let _ = self.storage.delete_managed_file(storage_key).await;
                for sc in sidecar_subtitles {
                    let _ = self.storage.delete_managed_file(&sc.path_or_key).await;
                }
            }
            video_ids.push(v.id);
            if let Some(sid) = v.show_id {
                show_id_set.insert(sid);
            }
        }
        let show_ids: Vec<ShowId> = show_id_set.into_iter().collect();

        self.watch_repo
            .delete_library_references(&video_ids, &show_ids)
            .await?;
        self.catalog_repo
            .delete_videos_by_library(library_id)
            .await?;
        self.catalog_repo.recompute_show_aggregates().await
    }

    async fn on_user_deleted(&self, user_id: UserId) -> DomainResult<()> {
        self.watch_repo.delete_by_user(user_id).await
    }
}

fn split_filename_stem_and_ext(filename: &str) -> (String, String) {
    let p = Path::new(filename);
    let stem = p
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("untitled")
        .trim()
        .to_string();
    let ext = p
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .trim()
        .to_string();
    (stem, ext)
}

use std::path::{Path, PathBuf};
use std::sync::Arc;

use jiff::Timestamp;
use uuid::Uuid;
use xmedia_core::application::bindings::{
    BindMediaItemsInput, BindMediaItemsResultDto, MediaBindingService, UnbindMediaItemResultDto,
};
use xmedia_core::application::ports::{RealtimeEventPort, SystemEvent};
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::{Library, LibraryId, MediaKind};
use xmedia_core::domain::repository::{BindingDeleteScope, LibraryRepository};
use xmedia_core::domain::user::User;
use xmedia_formats::video::{
    SubtitleFormatExt, parse_sidecar_subtitle_lang_and_label, parse_video_path,
};

use crate::application::dto::{
    PoolShowSummaryDto, PoolVideoSummaryDto, VideoPoolDto, VideoSummaryDto, VideoUploadPathItem,
    VideoUploadResultDto,
};
use crate::application::ports::{
    ParsedNfoMetadata, ProbedVideoInfo, VideoMediaEnginePort, VideoStoragePort,
};
use crate::application::projector::VideoProjector;
use crate::domain::id::{ShowId, VideoId};
use crate::domain::repository::{
    ShowScope, VideoCatalogDelete, VideoCatalogMutation, VideoCatalogRepository, VideoScope,
    VideoWatchPurge, VideoWatchRepository,
};
use crate::domain::video::{
    MissingShowMetadata, ShowQueryFilter, SidecarSubtitle, VideoContainer, VideoItem,
    VideoKindFilter, VideoMetadataPatch, VideoQueryFilter, VideoShow, VideoSource,
};

#[derive(Clone)]
pub struct VideoIngestService {
    libraries: Arc<dyn LibraryRepository>,
    catalog: Arc<dyn VideoCatalogRepository>,
    watch: Arc<dyn VideoWatchRepository>,
    bindings: Arc<MediaBindingService>,
    storage: Arc<dyn VideoStoragePort>,
    engine: Arc<dyn VideoMediaEnginePort>,
    events: Arc<dyn RealtimeEventPort>,
    projector: VideoProjector,
    managed_lib_lock: Arc<tokio::sync::Mutex<()>>,
}

impl VideoIngestService {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        libraries: Arc<dyn LibraryRepository>,
        catalog: Arc<dyn VideoCatalogRepository>,
        watch: Arc<dyn VideoWatchRepository>,
        bindings: Arc<MediaBindingService>,
        storage: Arc<dyn VideoStoragePort>,
        engine: Arc<dyn VideoMediaEnginePort>,
        events: Arc<dyn RealtimeEventPort>,
        projector: VideoProjector,
    ) -> Self {
        Self {
            libraries,
            catalog,
            watch,
            bindings,
            storage,
            engine,
            events,
            projector,
            managed_lib_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    pub async fn list_pool(
        &self,
        user: &User,
        library_id: Option<LibraryId>,
        search: Option<String>,
    ) -> DomainResult<VideoPoolDto> {
        let bound_ids = self
            .bindings
            .list_bound_ids(user.id, MediaKind::Video)
            .await?;
        let raw_movies: Vec<VideoItem> = self
            .catalog
            .query_videos(VideoScope::Filter(&VideoQueryFilter {
                library_id,
                kind: VideoKindFilter::Standalone,
                search: search.clone(),
                sort_desc: Some(true),
                ..Default::default()
            }))
            .await?
            .into_iter()
            .filter(|v| v.source.online_tracking_info().is_none())
            .collect();
        let raw_shows: Vec<VideoShow> = self
            .catalog
            .query_shows(ShowScope::Filter(&ShowQueryFilter {
                library_id,
                search,
                ..Default::default()
            }))
            .await?
            .into_iter()
            .filter(|s| s.online_tracked().is_none())
            .collect();

        let movie_dtos = self.projector.enrich_videos(user.id, raw_movies).await?;
        let show_dtos = self.projector.enrich_shows(user.id, raw_shows).await?;

        let movies = movie_dtos
            .into_iter()
            .map(|video| {
                let bound = bound_ids.contains(&video.id.0);
                PoolVideoSummaryDto { video, bound }
            })
            .collect();
        let shows = show_dtos
            .into_iter()
            .map(|show| {
                let bound = bound_ids.contains(&show.id.0);
                PoolShowSummaryDto { show, bound }
            })
            .collect();

        Ok(VideoPoolDto { movies, shows })
    }

    pub async fn bind_items(
        &self,
        actor: &User,
        input: BindMediaItemsInput,
    ) -> DomainResult<BindMediaItemsResultDto> {
        let target_user_id = MediaBindingService::resolve_target_user(actor, input.user_id)?;
        let mut to_bind: Vec<Uuid> = Vec::new();

        if let Some(lib_uuid) = input.library_id {
            let lid = LibraryId(lib_uuid);
            for v in self.catalog.query_videos(VideoScope::Library(lid)).await? {
                if !v.is_missing && v.show_id.is_none() && v.source.online_tracking_info().is_none()
                {
                    to_bind.push(v.id.0);
                }
            }
            for s in self
                .catalog
                .query_shows(ShowScope::Filter(&ShowQueryFilter {
                    library_id: Some(lid),
                    ..Default::default()
                }))
                .await?
            {
                if s.online_tracked().is_none() {
                    to_bind.push(s.id.0);
                }
            }
        }

        for raw_id in input.item_ids {
            if let Some(show) = self
                .catalog
                .query_shows(ShowScope::Id(ShowId(raw_id)))
                .await?
                .into_iter()
                .next()
            {
                if show.online_tracked().is_some()
                    && !actor.role.is_admin()
                    && !self
                        .bindings
                        .is_bound(actor.id, MediaKind::Video, show.id.0)
                        .await?
                {
                    continue;
                }
                to_bind.push(show.id.0);
            } else if let Some(video) = self
                .catalog
                .query_videos(VideoScope::Id(VideoId(raw_id)))
                .await?
                .into_iter()
                .next()
                && !video.is_missing
            {
                let bid = video.binding_id();
                if video.source.online_tracking_info().is_some()
                    && !actor.role.is_admin()
                    && !self
                        .bindings
                        .is_bound(actor.id, MediaKind::Video, bid)
                        .await?
                {
                    continue;
                }
                to_bind.push(bid);
            }
        }

        let total_requested = to_bind.len();
        let bound_count = self
            .bindings
            .bind_batch(target_user_id, MediaKind::Video, &to_bind)
            .await?;
        Ok(BindMediaItemsResultDto {
            bound_count,
            total_requested,
        })
    }

    pub async fn unbind_item(
        &self,
        actor: &User,
        raw_id: Uuid,
        requested_user_id: Option<Uuid>,
    ) -> DomainResult<UnbindMediaItemResultDto> {
        let target_user_id = MediaBindingService::resolve_target_user(actor, requested_user_id)?;

        if let Some(show) = self
            .catalog
            .query_shows(ShowScope::Id(ShowId(raw_id)))
            .await?
            .into_iter()
            .next()
        {
            let (unbound, remaining_bindings, deleted_from_pool) = self
                .bindings
                .unbind_with_gc(target_user_id, MediaKind::Video, show.id.0, || async {
                    if let Some(fresh_show) = self
                        .catalog
                        .query_shows(ShowScope::Id(show.id))
                        .await?
                        .into_iter()
                        .next()
                        && fresh_show.online_tracked().is_some()
                    {
                        self.delete_show_without_bindings(fresh_show.id).await?;
                        return Ok(true);
                    }
                    Ok(false)
                })
                .await?;
            if unbound && !deleted_from_pool {
                let eps = self
                    .catalog
                    .query_videos(VideoScope::ShowEpisodes(show.id))
                    .await?;
                let ep_ids: Vec<VideoId> = eps.into_iter().map(|e| e.id).collect();
                self.watch
                    .purge(VideoWatchPurge::UserTargets {
                        user_id: target_user_id,
                        video_ids: &ep_ids,
                        show_id: Some(show.id),
                    })
                    .await?;
            }
            return Ok(UnbindMediaItemResultDto {
                unbound,
                remaining_bindings,
                deleted_from_pool,
            });
        }

        let video_opt = self
            .catalog
            .query_videos(VideoScope::Id(VideoId(raw_id)))
            .await?
            .into_iter()
            .next();
        let canonical_id = video_opt
            .as_ref()
            .map(VideoItem::binding_id)
            .unwrap_or(raw_id);
        let (unbound, remaining_bindings, deleted_from_pool) = self
            .bindings
            .unbind_with_gc(target_user_id, MediaKind::Video, canonical_id, || async {
                if let Some(ref v) = video_opt {
                    if let Some(sid) = v.show_id {
                        if let Some(show) = self
                            .catalog
                            .query_shows(ShowScope::Id(sid))
                            .await?
                            .into_iter()
                            .next()
                            && show.online_tracked().is_some()
                        {
                            self.delete_show_without_bindings(sid).await?;
                            return Ok(true);
                        }
                    } else if let Some(fresh_v) = self
                        .catalog
                        .query_videos(VideoScope::Id(v.id))
                        .await?
                        .into_iter()
                        .next()
                        && fresh_v.source.online_tracking_info().is_some()
                    {
                        self.delete_video_without_bindings(fresh_v).await?;
                        return Ok(true);
                    }
                }
                Ok(false)
            })
            .await?;
        if unbound && !deleted_from_pool {
            if let Some(ref v) = video_opt
                && let Some(sid) = v.show_id
            {
                let eps = self
                    .catalog
                    .query_videos(VideoScope::ShowEpisodes(sid))
                    .await?;
                let ep_ids: Vec<VideoId> = eps.into_iter().map(|e| e.id).collect();
                self.watch
                    .purge(VideoWatchPurge::UserTargets {
                        user_id: target_user_id,
                        video_ids: &ep_ids,
                        show_id: Some(sid),
                    })
                    .await?;
            } else {
                self.watch
                    .purge(VideoWatchPurge::UserTargets {
                        user_id: target_user_id,
                        video_ids: &[VideoId(canonical_id)],
                        show_id: None,
                    })
                    .await?;
            }
        }
        Ok(UnbindMediaItemResultDto {
            unbound,
            remaining_bindings,
            deleted_from_pool,
        })
    }

    pub async fn upload_files_from_paths(
        &self,
        actor: &User,
        files: Vec<VideoUploadPathItem>,
    ) -> DomainResult<VideoUploadResultDto> {
        actor.ensure_admin()?;
        if files.is_empty() {
            return Err(DomainError::Validation("no files uploaded".into()));
        }

        let managed_lib = self.ensure_managed_library().await?;

        let mut video_items: Vec<(String, VideoContainer, PathBuf)> = Vec::new();
        let mut sub_items: Vec<(String, String, PathBuf)> = Vec::new();

        for f in files {
            let (stem, ext) = split_filename_stem_and_ext(&f.filename);
            if let Ok(container) = ext.parse::<VideoContainer>() {
                video_items.push((f.filename, container, f.temp_path));
            } else if SubtitleFormatExt::from_extension(&ext).is_some() {
                sub_items.push((f.filename, ext.to_ascii_lowercase(), f.temp_path));
            } else {
                return Err(DomainError::UnsupportedFormat(format!(
                    "unsupported file extension '.{ext}' (stem '{stem}')"
                )));
            }
        }

        if video_items.is_empty() {
            return Err(DomainError::Validation(
                "at least one video file is required (subtitle-only uploads are not supported)"
                    .into(),
            ));
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
                Some(ref b) => self.storage.save_generated_poster_bytes(b).await.ok(),
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

            self.catalog
                .save(VideoCatalogMutation::Video(&item))
                .await?;
            created_videos.push(item);
        }

        self.catalog
            .save(VideoCatalogMutation::RecomputeShowAggregates)
            .await?;
        let binding_ids: Vec<Uuid> = created_videos.iter().map(VideoItem::binding_id).collect();
        self.bindings
            .bind_batch(actor.id, MediaKind::Video, &binding_ids)
            .await?;
        self.events.publish(SystemEvent::MediaChanged {
            media_kind: MediaKind::Video,
        });

        let uploaded_videos = self
            .projector
            .enrich_videos(actor.id, created_videos)
            .await?;

        Ok(VideoUploadResultDto {
            uploaded_videos,
            attached_subtitles_count,
        })
    }

    pub async fn update_video_metadata(
        &self,
        actor: &User,
        video_id: VideoId,
        patch: VideoMetadataPatch,
    ) -> DomainResult<VideoSummaryDto> {
        actor.ensure_admin()?;
        let mut item = self
            .catalog
            .query_videos(VideoScope::Id(video_id))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound("video not found".into()))?;

        item.apply_patch(patch)?;
        self.catalog
            .save(VideoCatalogMutation::Video(&item))
            .await?;
        if item.show_id.is_some() {
            self.catalog
                .save(VideoCatalogMutation::RecomputeShowAggregates)
                .await?;
        }
        self.projector
            .enrich_videos(actor.id, vec![item])
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound("video not found".into()))
    }

    pub async fn upload_video_poster(
        &self,
        actor: &User,
        video_id: VideoId,
        bytes: &[u8],
    ) -> DomainResult<VideoSummaryDto> {
        actor.ensure_admin()?;
        if bytes.is_empty() {
            return Err(DomainError::Validation(
                "poster image cannot be empty".into(),
            ));
        }
        let mut item = self
            .catalog
            .query_videos(VideoScope::Id(video_id))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound("video not found".into()))?;
        let key = self.storage.save_poster_bytes(bytes).await?;
        item.poster_key = Some(key);
        item.updated_at = Timestamp::now();
        self.catalog
            .save(VideoCatalogMutation::Video(&item))
            .await?;
        self.projector
            .enrich_videos(actor.id, vec![item])
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound("video not found".into()))
    }

    pub async fn delete_video(&self, actor: &User, video_id: VideoId) -> DomainResult<()> {
        actor.ensure_admin()?;
        let item = self
            .catalog
            .query_videos(VideoScope::Id(video_id))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound("video not found".into()))?;
        let _ = self.delete_video_inner(item, true).await?;
        Ok(())
    }

    pub(in crate::application) async fn ensure_managed_library(&self) -> DomainResult<Library> {
        let _guard = self.managed_lib_lock.lock().await;
        if let Some(existing) = self
            .libraries
            .find_managed_by_media(MediaKind::Video)
            .await?
        {
            return Ok(existing);
        }
        let lib = Library::new_managed(MediaKind::Video)?;
        self.libraries.save(&lib).await?;
        Ok(lib)
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::application) async fn build_and_upsert_video(
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
        if let Some(mut existing) = self
            .catalog
            .query_shows(ShowScope::Key(&show_key))
            .await?
            .into_iter()
            .next()
        {
            if existing.fill_missing_metadata(
                MissingShowMetadata {
                    year,
                    genres: Some(genres),
                    poster_key,
                    ..Default::default()
                },
                now,
            ) {
                self.catalog
                    .save(VideoCatalogMutation::Show(&existing))
                    .await?;
            }
            return Ok(existing);
        }

        let created = VideoShow::new_local(
            library_id,
            title,
            year,
            genres.to_vec(),
            poster_key.map(str::to_string),
            now,
        );
        self.catalog
            .save(VideoCatalogMutation::Show(&created))
            .await?;
        Ok(created)
    }

    pub(in crate::application) async fn delete_video_without_bindings(
        &self,
        item: VideoItem,
    ) -> DomainResult<()> {
        let _ = self.delete_video_inner(item, false).await?;
        Ok(())
    }

    async fn delete_video_inner(
        &self,
        item: VideoItem,
        delete_bindings: bool,
    ) -> DomainResult<Option<ShowId>> {
        let video_id = item.id;
        let old_show_id = item.show_id;
        for key in item.source.managed_file_keys() {
            let _ = self.storage.delete_managed_file(key).await;
        }
        self.watch
            .purge(VideoWatchPurge::VideoProgress(video_id))
            .await?;
        self.catalog
            .delete(VideoCatalogDelete::Video(video_id))
            .await?;
        self.catalog
            .save(VideoCatalogMutation::RecomputeShowAggregates)
            .await?;
        let deleted_show_id = if let Some(sid) = old_show_id {
            if self
                .catalog
                .query_shows(ShowScope::Id(sid))
                .await?
                .is_empty()
            {
                if delete_bindings {
                    let _ = self
                        .bindings
                        .purge(BindingDeleteScope::Item {
                            media_kind: MediaKind::Video,
                            item_id: sid.0,
                        })
                        .await?;
                }
                Some(sid)
            } else {
                None
            }
        } else {
            if delete_bindings {
                let _ = self
                    .bindings
                    .purge(BindingDeleteScope::Item {
                        media_kind: MediaKind::Video,
                        item_id: video_id.0,
                    })
                    .await?;
            }
            None
        };
        self.watch
            .purge(VideoWatchPurge::VideoReferences {
                video_id,
                deleted_show_id,
            })
            .await?;
        self.events.publish(SystemEvent::MediaChanged {
            media_kind: MediaKind::Video,
        });
        Ok(deleted_show_id)
    }

    pub(in crate::application) async fn delete_show_without_bindings(
        &self,
        show_id: ShowId,
    ) -> DomainResult<()> {
        let episodes = self
            .catalog
            .query_videos(VideoScope::ShowEpisodes(show_id))
            .await?;
        let mut ep_ids = Vec::with_capacity(episodes.len());
        for ep in episodes {
            for key in ep.source.managed_file_keys() {
                let _ = self.storage.delete_managed_file(key).await;
            }
            ep_ids.push(ep.id);
            self.catalog
                .delete(VideoCatalogDelete::Video(ep.id))
                .await?;
        }
        self.watch
            .purge(VideoWatchPurge::LibraryReferences {
                video_ids: &ep_ids,
                show_ids: &[show_id],
            })
            .await?;
        self.catalog
            .delete(VideoCatalogDelete::Show(show_id))
            .await?;
        self.catalog
            .save(VideoCatalogMutation::RecomputeShowAggregates)
            .await?;
        self.events.publish(SystemEvent::MediaChanged {
            media_kind: MediaKind::Video,
        });
        Ok(())
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

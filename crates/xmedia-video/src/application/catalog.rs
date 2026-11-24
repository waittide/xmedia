use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use jiff::Timestamp;
use uuid::Uuid;
use xmedia_core::application::bindings::{
    BindMediaItemsInput, BindMediaItemsResultDto, MediaBindingService, UnbindMediaItemResultDto,
};
use xmedia_core::application::ports::{BinaryPayload, RealtimeEventPort, SystemEvent};
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::{LibraryId, MediaKind};
use xmedia_core::domain::repository::LibraryRepository;
use xmedia_core::domain::user::{User, UserId, require_admin};

use crate::application::dto::{
    OnlineTrackedShowDto, OnlineTrackedVideoDto, PoolShowSummaryDto, PoolVideoSummaryDto,
    SeasonGroupDto, ShowDetailDto, ShowSummaryDto, SubtitleTrackDto, VideoDetailDto,
    VideoHomeOverviewDto, VideoPoolDto, VideoSummaryDto, VideoSystemStatusDto, VideoTaxonomyDto,
    VideoTaxonomyItemDto,
};
use crate::application::ports::{VideoMediaEnginePort, VideoStoragePort};
use crate::domain::id::{ShowId, VideoId};
use crate::domain::repository::{VideoCatalogRepository, VideoWatchRepository};
use crate::domain::video::{
    ShowMetadataPatch, ShowQueryFilter, VideoItem, VideoKindFilter, VideoMetadataPatch,
    VideoQueryFilter, VideoShow,
};
use crate::domain::watch::{VideoFavoriteKind, WatchProgress};

pub fn video_binding_id(video: &VideoItem) -> Uuid {
    video.show_id.map(|s| s.0).unwrap_or(video.id.0)
}

#[derive(Clone)]
pub struct VideoCatalogService {
    libraries: Arc<dyn LibraryRepository>,
    catalog: Arc<dyn VideoCatalogRepository>,
    watch: Arc<dyn VideoWatchRepository>,
    bindings: Arc<MediaBindingService>,
    storage: Arc<dyn VideoStoragePort>,
    engine: Arc<dyn VideoMediaEnginePort>,
    events: Arc<dyn RealtimeEventPort>,
}

impl VideoCatalogService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        libraries: Arc<dyn LibraryRepository>,
        catalog: Arc<dyn VideoCatalogRepository>,
        watch: Arc<dyn VideoWatchRepository>,
        bindings: Arc<MediaBindingService>,
        storage: Arc<dyn VideoStoragePort>,
        engine: Arc<dyn VideoMediaEnginePort>,
        events: Arc<dyn RealtimeEventPort>,
    ) -> Self {
        Self {
            libraries,
            catalog,
            watch,
            bindings,
            storage,
            engine,
            events,
        }
    }

    pub fn bindings(&self) -> &Arc<MediaBindingService> {
        &self.bindings
    }

    pub async fn list_all_binding_ids_for_backfill(&self) -> DomainResult<Vec<Uuid>> {
        let mut ids = HashSet::new();
        for v in self
            .catalog
            .list_videos(&VideoQueryFilter::default())
            .await?
        {
            ids.insert(video_binding_id(&v));
        }
        for s in self.catalog.list_shows(&ShowQueryFilter::default()).await? {
            ids.insert(s.id.0);
        }
        Ok(ids.into_iter().collect())
    }

    pub async fn bind_entity_for_user(
        &self,
        user_id: UserId,
        entity_id: Uuid,
    ) -> DomainResult<bool> {
        self.bindings
            .bind(user_id, MediaKind::Video, entity_id)
            .await
    }

    pub async fn bind_entities_for_user(
        &self,
        user_id: UserId,
        entity_ids: &[Uuid],
    ) -> DomainResult<usize> {
        self.bindings
            .bind_batch(user_id, MediaKind::Video, entity_ids)
            .await
    }

    pub async fn require_video_bound(
        &self,
        user_id: UserId,
        video_id: VideoId,
    ) -> DomainResult<VideoItem> {
        let video = self
            .catalog
            .find_video_by_id(video_id)
            .await?
            .filter(|v| !v.is_missing)
            .ok_or_else(|| DomainError::NotFound("video not found".into()))?;
        let bid = video_binding_id(&video);
        self.bindings
            .require_bound(user_id, MediaKind::Video, bid, "video not found")
            .await?;
        Ok(video)
    }

    pub async fn require_show_bound(&self, user_id: UserId, show_id: ShowId) -> DomainResult<()> {
        self.bindings
            .require_bound(user_id, MediaKind::Video, show_id.0, "show not found")
            .await
    }

    pub async fn system_status(&self, user_id: UserId) -> DomainResult<VideoSystemStatusDto> {
        let ffmpeg = self.engine.check_status().await;
        let libs = self.libraries.list_by_media(MediaKind::Video).await?;
        let scanning = libs.iter().any(|l| l.scan_status.is_scanning());
        let bound_ids = self
            .bindings
            .list_bound_ids(user_id, MediaKind::Video)
            .await?;
        let video_count = if bound_ids.is_empty() {
            0
        } else {
            self.catalog
                .list_videos(&VideoQueryFilter::default())
                .await?
                .into_iter()
                .filter(|v| bound_ids.contains(&video_binding_id(v)))
                .count() as u64
        };
        Ok(VideoSystemStatusDto {
            ffmpeg,
            scanning,
            video_count,
        })
    }

    fn enrich_videos_with_maps(
        videos: Vec<VideoItem>,
        prog_map: &HashMap<VideoId, WatchProgress>,
        fav_set: &HashSet<Uuid>,
    ) -> Vec<VideoSummaryDto> {
        videos
            .into_iter()
            .map(|v| {
                let resolution = v.resolution_label();
                let can_direct_play = v.can_direct_play_in_browser();
                let online_tracking =
                    v.source
                        .online_tracking_info()
                        .map(|info| OnlineTrackedVideoDto {
                            source_id: info.source_id,
                            video_id: info.video_id.clone(),
                            detail_url: info.detail_url.clone(),
                            play_url: info.play_url.clone(),
                            group_name: info.group_name.clone(),
                            episode_name: info.episode_name.clone(),
                            cached: info.cached && info.storage_key.is_some(),
                        });
                VideoSummaryDto {
                    id: v.id,
                    library_id: v.library_id,
                    show_id: v.show_id,
                    show_title: v.show_title,
                    season_number: v.season_number,
                    episode_number: v.episode_number,
                    title: v.title,
                    summary: v.summary,
                    year: v.year,
                    genres: v.genres,
                    tags: v.tags,
                    container: v.container,
                    mime_type: v.container.mime_type().to_string(),
                    can_direct_play,
                    file_size: v.file_size,
                    duration_secs: v.duration_secs,
                    width: v.width,
                    height: v.height,
                    resolution,
                    video_codec: v.video_codec,
                    poster_key: v.poster_key,
                    is_favorite: fav_set.contains(&v.id.0),
                    progress: prog_map.get(&v.id).cloned(),
                    created_at: v.created_at,
                    online_tracking,
                }
            })
            .collect()
    }

    fn enrich_shows_with_maps(
        shows: Vec<VideoShow>,
        fav_set: &HashSet<Uuid>,
        watched_by_show: &HashMap<ShowId, u32>,
    ) -> Vec<ShowSummaryDto> {
        shows
            .into_iter()
            .map(|s| {
                let online_tracking =
                    s.online_tracked_ref()
                        .map(|(source_id, item_key)| OnlineTrackedShowDto {
                            source_id,
                            item_key: item_key.to_string(),
                        });
                ShowSummaryDto {
                    id: s.id,
                    library_id: s.library_id,
                    title: s.title,
                    summary: s.summary,
                    year: s.year,
                    genres: s.genres,
                    tags: s.tags,
                    poster_key: s.poster_key,
                    season_count: s.season_count,
                    episode_count: s.episode_count,
                    watched_episode_count: watched_by_show.get(&s.id).copied().unwrap_or(0),
                    is_favorite: fav_set.contains(&s.id.0),
                    created_at: s.created_at,
                    online_tracking,
                }
            })
            .collect()
    }

    pub async fn enrich_videos(
        &self,
        user_id: UserId,
        videos: Vec<VideoItem>,
    ) -> DomainResult<Vec<VideoSummaryDto>> {
        if videos.is_empty() {
            return Ok(Vec::new());
        }
        let progress_list = self.watch.list_user_progress(user_id).await?;
        let favs = self
            .watch
            .list_favorites(user_id, Some(VideoFavoriteKind::Video))
            .await?;

        let prog_map: HashMap<VideoId, WatchProgress> =
            progress_list.into_iter().map(|p| (p.video_id, p)).collect();
        let fav_set: HashSet<Uuid> = favs.into_iter().map(|f| f.target_id).collect();

        Ok(Self::enrich_videos_with_maps(videos, &prog_map, &fav_set))
    }

    pub async fn enrich_shows(
        &self,
        user_id: UserId,
        shows: Vec<VideoShow>,
    ) -> DomainResult<Vec<ShowSummaryDto>> {
        if shows.is_empty() {
            return Ok(Vec::new());
        }
        let favs = self
            .watch
            .list_favorites(user_id, Some(VideoFavoriteKind::Show))
            .await?;
        let fav_set: HashSet<Uuid> = favs.into_iter().map(|f| f.target_id).collect();

        let progress_list = self.watch.list_user_progress(user_id).await?;
        let completed_video_ids: HashSet<VideoId> = progress_list
            .into_iter()
            .filter(|p| p.completed)
            .map(|p| p.video_id)
            .collect();

        let mut watched_by_show: HashMap<ShowId, u32> = HashMap::new();
        if !completed_video_ids.is_empty() {
            if shows.len() == 1 {
                let show_id = shows[0].id;
                let episodes = self.catalog.list_episodes_by_show(show_id).await?;
                let watched = episodes
                    .iter()
                    .filter(|ep| completed_video_ids.contains(&ep.id))
                    .count() as u32;
                watched_by_show.insert(show_id, watched);
            } else {
                let all_episodes = self
                    .catalog
                    .list_videos(&VideoQueryFilter {
                        kind: crate::domain::video::VideoKindFilter::Episodes,
                        ..Default::default()
                    })
                    .await?;
                for ep in all_episodes {
                    if let Some(sid) = ep.show_id
                        && completed_video_ids.contains(&ep.id)
                    {
                        *watched_by_show.entry(sid).or_default() += 1;
                    }
                }
            }
        }

        Ok(Self::enrich_shows_with_maps(
            shows,
            &fav_set,
            &watched_by_show,
        ))
    }

    pub async fn home_overview(&self, user: &User) -> DomainResult<VideoHomeOverviewDto> {
        let bound_ids = self
            .bindings
            .list_bound_ids(user.id, MediaKind::Video)
            .await?;

        let mut all_videos: Vec<VideoItem> = if bound_ids.is_empty() {
            Vec::new()
        } else {
            self.catalog
                .list_videos(&VideoQueryFilter {
                    sort_desc: Some(true),
                    ..Default::default()
                })
                .await?
                .into_iter()
                .filter(|v| bound_ids.contains(&video_binding_id(v)))
                .collect()
        };
        let mut all_shows: Vec<VideoShow> = if bound_ids.is_empty() {
            Vec::new()
        } else {
            self.catalog
                .list_shows(&ShowQueryFilter::default())
                .await?
                .into_iter()
                .filter(|s| bound_ids.contains(&s.id.0))
                .collect()
        };
        let bound_video_id_set: HashSet<VideoId> = all_videos.iter().map(|v| v.id).collect();
        let bound_show_id_set: HashSet<ShowId> = all_shows.iter().map(|s| s.id).collect();

        let all_progress_list: Vec<_> = self
            .watch
            .list_user_progress(user.id)
            .await?
            .into_iter()
            .filter(|p| bound_video_id_set.contains(&p.video_id))
            .collect();
        let all_favorites: Vec<_> = self
            .watch
            .list_favorites(user.id, None)
            .await?
            .into_iter()
            .filter(|fav| match fav.kind {
                VideoFavoriteKind::Video => bound_video_id_set.contains(&VideoId(fav.target_id)),
                VideoFavoriteKind::Show => bound_show_id_set.contains(&ShowId(fav.target_id)),
            })
            .collect();
        let favorites_count = all_favorites.len() as u64;

        let mut video_fav_set: HashSet<Uuid> = HashSet::new();
        let mut show_fav_set: HashSet<Uuid> = HashSet::new();
        for fav in all_favorites {
            match fav.kind {
                VideoFavoriteKind::Video => {
                    video_fav_set.insert(fav.target_id);
                }
                VideoFavoriteKind::Show => {
                    show_fav_set.insert(fav.target_id);
                }
            }
        }

        let mut prog_map: HashMap<VideoId, WatchProgress> =
            HashMap::with_capacity(all_progress_list.len());
        let mut completed_video_ids: HashSet<VideoId> = HashSet::new();
        let mut active_prog: Vec<(VideoId, Timestamp)> = Vec::new();
        for p in all_progress_list {
            if p.completed {
                completed_video_ids.insert(p.video_id);
            } else if p.position_secs > 1.0 {
                active_prog.push((p.video_id, p.updated_at));
            }
            prog_map.insert(p.video_id, p);
        }
        active_prog.sort_by_key(|(_, ts)| std::cmp::Reverse(*ts));

        let total_videos = all_videos.len() as u64;
        let mut movies_count = 0u64;
        let mut genre_set = HashSet::new();
        let mut tag_set = HashSet::new();
        let mut lib_set: HashSet<LibraryId> = HashSet::new();
        let mut watched_by_show: HashMap<ShowId, u32> = HashMap::new();
        let mut video_index: HashMap<VideoId, usize> = HashMap::with_capacity(all_videos.len());

        for (idx, v) in all_videos.iter().enumerate() {
            video_index.insert(v.id, idx);
            lib_set.insert(v.library_id);
            if let Some(sid) = v.show_id {
                if completed_video_ids.contains(&v.id) {
                    *watched_by_show.entry(sid).or_default() += 1;
                }
            } else {
                movies_count += 1;
            }
            for g in &v.genres {
                let clean = g.trim().to_lowercase();
                if !clean.is_empty() {
                    genre_set.insert(clean);
                }
            }
            for t in &v.tags {
                let clean = t.trim().to_lowercase();
                if !clean.is_empty() {
                    tag_set.insert(clean);
                }
            }
        }

        let mut active_cw: Vec<VideoItem> = active_prog
            .iter()
            .filter_map(|(id, _)| video_index.get(id).map(|&idx| all_videos[idx].clone()))
            .collect();
        let watching_count = active_cw.len() as u64;
        active_cw.truncate(12);
        let continue_watching = Self::enrich_videos_with_maps(active_cw, &prog_map, &video_fav_set);

        all_videos.truncate(16);
        let recent_videos = Self::enrich_videos_with_maps(all_videos, &prog_map, &video_fav_set);

        let total_shows = all_shows.len() as u64;
        for s in &all_shows {
            lib_set.insert(s.library_id);
            for g in &s.genres {
                let clean = g.trim().to_lowercase();
                if !clean.is_empty() {
                    genre_set.insert(clean);
                }
            }
            for t in &s.tags {
                let clean = t.trim().to_lowercase();
                if !clean.is_empty() {
                    tag_set.insert(clean);
                }
            }
        }
        all_shows.sort_by_key(|s| std::cmp::Reverse(s.created_at));
        all_shows.truncate(12);
        let recent_shows = Self::enrich_shows_with_maps(all_shows, &show_fav_set, &watched_by_show);

        let shelves_count = self.watch.list_shelves(user.id).await?.len() as u64;
        let libraries_count = lib_set.len() as u64;

        Ok(VideoHomeOverviewDto {
            continue_watching,
            recent_videos,
            recent_shows,
            total_videos,
            movies_count,
            total_shows,
            watching_count,
            favorites_count,
            shelves_count,
            genres_count: genre_set.len() as u64,
            tags_count: tag_set.len() as u64,
            libraries_count,
        })
    }

    pub async fn list_videos(
        &self,
        user: &User,
        filter: VideoQueryFilter,
    ) -> DomainResult<Vec<VideoSummaryDto>> {
        let bound_ids = self
            .bindings
            .list_bound_ids(user.id, MediaKind::Video)
            .await?;
        if bound_ids.is_empty() {
            return Ok(Vec::new());
        }
        let items: Vec<VideoItem> = self
            .catalog
            .list_videos(&filter)
            .await?
            .into_iter()
            .filter(|v| bound_ids.contains(&video_binding_id(v)))
            .collect();
        self.enrich_videos(user.id, items).await
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
            .list_videos(&VideoQueryFilter {
                library_id,
                kind: VideoKindFilter::Standalone,
                search: search.clone(),
                sort_desc: Some(true),
                ..Default::default()
            })
            .await?
            .into_iter()
            .filter(|v| !v.source.is_online_tracked())
            .collect();
        let raw_shows: Vec<VideoShow> = self
            .catalog
            .list_shows(&ShowQueryFilter {
                library_id,
                search,
                ..Default::default()
            })
            .await?
            .into_iter()
            .filter(|s| s.online_tracked_ref().is_none())
            .collect();

        let movie_dtos = self.enrich_videos(user.id, raw_movies).await?;
        let show_dtos = self.enrich_shows(user.id, raw_shows).await?;

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
        let target_user_id = MediaBindingService::resolve_target_user_id(actor, input.user_id)?;
        let mut to_bind: Vec<Uuid> = Vec::new();

        if let Some(lib_uuid) = input.library_id {
            let lid = LibraryId(lib_uuid);
            for v in self.catalog.list_videos_by_library(lid).await? {
                if !v.is_missing && v.show_id.is_none() && !v.source.is_online_tracked() {
                    to_bind.push(v.id.0);
                }
            }
            for s in self
                .catalog
                .list_shows(&ShowQueryFilter {
                    library_id: Some(lid),
                    ..Default::default()
                })
                .await?
            {
                if s.online_tracked_ref().is_none() {
                    to_bind.push(s.id.0);
                }
            }
        }

        for raw_id in input.item_ids {
            if let Some(show) = self.catalog.find_show_by_id(ShowId(raw_id)).await? {
                if show.online_tracked_ref().is_some()
                    && !actor.role.is_admin()
                    && !self
                        .bindings
                        .is_bound(actor.id, MediaKind::Video, show.id.0)
                        .await?
                {
                    continue;
                }
                to_bind.push(show.id.0);
            } else if let Some(video) = self.catalog.find_video_by_id(VideoId(raw_id)).await?
                && !video.is_missing
            {
                let bid = video_binding_id(&video);
                if video.source.is_online_tracked()
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
        let target_user_id = MediaBindingService::resolve_target_user_id(actor, requested_user_id)?;

        if let Some(show) = self.catalog.find_show_by_id(ShowId(raw_id)).await? {
            let (unbound, remaining_bindings, deleted_from_pool) = self
                .bindings
                .unbind_with_gc(target_user_id, MediaKind::Video, show.id.0, || async {
                    if let Some(fresh_show) = self.catalog.find_show_by_id(show.id).await?
                        && fresh_show.online_tracked_ref().is_some()
                    {
                        self.delete_show_without_bindings(fresh_show.id).await?;
                        return Ok(true);
                    }
                    Ok(false)
                })
                .await?;
            if unbound && !deleted_from_pool {
                let eps = self.catalog.list_episodes_by_show(show.id).await?;
                let ep_ids: Vec<VideoId> = eps.into_iter().map(|e| e.id).collect();
                self.watch
                    .delete_by_user_and_targets(target_user_id, &ep_ids, Some(show.id))
                    .await?;
            }
            return Ok(UnbindMediaItemResultDto {
                unbound,
                remaining_bindings,
                deleted_from_pool,
            });
        }

        let video_opt = self.catalog.find_video_by_id(VideoId(raw_id)).await?;
        let canonical_id = video_opt.as_ref().map(video_binding_id).unwrap_or(raw_id);
        let (unbound, remaining_bindings, deleted_from_pool) = self
            .bindings
            .unbind_with_gc(target_user_id, MediaKind::Video, canonical_id, || async {
                if let Some(ref v) = video_opt {
                    if let Some(sid) = v.show_id {
                        if let Some(show) = self.catalog.find_show_by_id(sid).await?
                            && show.online_tracked_ref().is_some()
                        {
                            self.delete_show_without_bindings(sid).await?;
                            return Ok(true);
                        }
                    } else if let Some(fresh_v) = self.catalog.find_video_by_id(v.id).await?
                        && fresh_v.source.is_online_tracked()
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
                let eps = self.catalog.list_episodes_by_show(sid).await?;
                let ep_ids: Vec<VideoId> = eps.into_iter().map(|e| e.id).collect();
                self.watch
                    .delete_by_user_and_targets(target_user_id, &ep_ids, Some(sid))
                    .await?;
            } else {
                self.watch
                    .delete_by_user_and_targets(target_user_id, &[VideoId(canonical_id)], None)
                    .await?;
            }
        }
        Ok(UnbindMediaItemResultDto {
            unbound,
            remaining_bindings,
            deleted_from_pool,
        })
    }

    pub async fn get_video_detail(
        &self,
        user: &User,
        video_id: VideoId,
    ) -> DomainResult<VideoDetailDto> {
        let item = self.require_video_bound(user.id, video_id).await?;

        let bit_rate_kbps = item.bit_rate_kbps;
        let path = item.source.display_path().to_string();
        let audio_streams = item.audio_streams.clone();
        let subtitles = Self::collect_subtitle_dtos(&item);

        let (prev_episode_id, next_episode_id) = if let Some(show_id) = item.show_id {
            let episodes = self.catalog.list_episodes_by_show(show_id).await?;
            let pos = episodes.iter().position(|e| e.id == item.id);
            let prev = pos
                .and_then(|idx| idx.checked_sub(1))
                .and_then(|i| episodes.get(i))
                .map(|e| e.id);
            let next = pos.and_then(|idx| episodes.get(idx + 1)).map(|e| e.id);
            (prev, next)
        } else {
            (None, None)
        };

        let summary = self
            .enrich_videos(user.id, vec![item])
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound("video not found".into()))?;

        Ok(VideoDetailDto {
            video: summary,
            bit_rate_kbps,
            path,
            audio_streams,
            subtitles,
            prev_episode_id,
            next_episode_id,
        })
    }

    pub async fn list_video_subtitles(
        &self,
        user_id: UserId,
        video_id: VideoId,
    ) -> DomainResult<Vec<SubtitleTrackDto>> {
        let item = self.require_video_bound(user_id, video_id).await?;
        Ok(Self::collect_subtitle_dtos(&item))
    }

    pub fn collect_subtitle_dtos(video: &VideoItem) -> Vec<SubtitleTrackDto> {
        let mut out = Vec::new();
        for sc in video.source.sidecar_subtitles() {
            out.push(SubtitleTrackDto {
                id: format!("ext-{}", sc.id),
                source_kind: "external".into(),
                codec_or_format: sc.format.clone(),
                language: sc.language.clone(),
                label: sc.label.clone(),
                is_default: out.is_empty(),
            });
        }
        for sub in &video.subtitle_streams {
            if sub.is_text_based {
                let label = sub
                    .title
                    .clone()
                    .or_else(|| sub.language.clone())
                    .unwrap_or_else(|| format!("Track #{}", sub.stream_index));
                out.push(SubtitleTrackDto {
                    id: format!("emb-{}", sub.stream_index),
                    source_kind: "embedded".into(),
                    codec_or_format: sub.codec.clone(),
                    language: sub.language.clone(),
                    label,
                    is_default: out.is_empty() && sub.is_default,
                });
            }
        }
        out
    }

    pub async fn get_subtitle_webvtt(
        &self,
        user_id: UserId,
        video_id: VideoId,
        sub_id: &str,
    ) -> DomainResult<String> {
        self.get_subtitle_webvtt_with_offset(user_id, video_id, sub_id, None)
            .await
    }

    pub async fn get_subtitle_webvtt_with_offset(
        &self,
        user_id: UserId,
        video_id: VideoId,
        sub_id: &str,
        offset_secs: Option<f64>,
    ) -> DomainResult<String> {
        let video = self.require_video_bound(user_id, video_id).await?;
        let library = self
            .libraries
            .find_by_id(video.library_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("library not found".into()))?;

        let trimmed = sub_id.trim().trim_end_matches(".vtt");
        let vtt = if let Some(raw_idx) = trimmed.strip_prefix("emb-") {
            let stream_index = raw_idx
                .parse::<u32>()
                .map_err(|_| DomainError::Validation("invalid embedded subtitle id".into()))?;
            let sub_stream = video
                .subtitle_streams
                .iter()
                .find(|s| s.stream_index == stream_index)
                .ok_or_else(|| {
                    DomainError::NotFound(format!(
                        "embedded subtitle stream {stream_index} not found"
                    ))
                })?;
            if !sub_stream.is_text_based {
                return Err(DomainError::Validation(format!(
                    "embedded subtitle stream {stream_index} ({}) is not text-based",
                    sub_stream.codec
                )));
            }
            let fs_path = self.storage.resolve_video_fs_path(&library, &video).await?;
            self.engine
                .extract_embedded_subtitle_vtt(Path::new(&fs_path), stream_index)
                .await?
        } else {
            let ext_id = trimmed.strip_prefix("ext-").unwrap_or(trimmed);
            let (fmt, bytes) = self
                .storage
                .read_sidecar_subtitle_bytes(&library, &video, ext_id)
                .await?;
            self.engine.convert_sidecar_subtitle_to_vtt(&fmt, &bytes)
        };

        if let Some(offset) = offset_secs.filter(|&v| v.is_finite() && v > 0.0) {
            Ok(self.engine.shift_webvtt_timestamps(&vtt, offset))
        } else {
            Ok(vtt)
        }
    }

    pub async fn get_taxonomy(&self, user_id: UserId) -> DomainResult<VideoTaxonomyDto> {
        let bound_ids = self
            .bindings
            .list_bound_ids(user_id, MediaKind::Video)
            .await?;
        if bound_ids.is_empty() {
            return Ok(VideoTaxonomyDto {
                genres: Vec::new(),
                tags: Vec::new(),
            });
        }
        let videos: Vec<_> = self
            .catalog
            .list_videos(&VideoQueryFilter::default())
            .await?
            .into_iter()
            .filter(|v| bound_ids.contains(&video_binding_id(v)))
            .collect();
        let shows: Vec<_> = self
            .catalog
            .list_shows(&ShowQueryFilter::default())
            .await?
            .into_iter()
            .filter(|s| bound_ids.contains(&s.id.0))
            .collect();

        let mut genre_counts: HashMap<String, usize> = HashMap::new();
        let mut tag_counts: HashMap<String, usize> = HashMap::new();

        for v in videos {
            for g in v.genres {
                let clean = g.trim();
                if !clean.is_empty() {
                    *genre_counts.entry(clean.to_string()).or_insert(0) += 1;
                }
            }
            for t in v.tags {
                let clean = t.trim();
                if !clean.is_empty() {
                    *tag_counts.entry(clean.to_string()).or_insert(0) += 1;
                }
            }
        }
        for s in shows {
            for g in s.genres {
                let clean = g.trim();
                if !clean.is_empty() {
                    genre_counts.entry(clean.to_string()).or_insert(1);
                }
            }
            for t in s.tags {
                let clean = t.trim();
                if !clean.is_empty() {
                    tag_counts.entry(clean.to_string()).or_insert(1);
                }
            }
        }

        let mut genres: Vec<VideoTaxonomyItemDto> = genre_counts
            .into_iter()
            .map(|(name, count)| VideoTaxonomyItemDto { name, count })
            .collect();
        genres.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));

        let mut tags: Vec<VideoTaxonomyItemDto> = tag_counts
            .into_iter()
            .map(|(name, count)| VideoTaxonomyItemDto { name, count })
            .collect();
        tags.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));

        Ok(VideoTaxonomyDto { genres, tags })
    }

    pub async fn update_video_metadata(
        &self,
        actor: &User,
        video_id: VideoId,
        patch: VideoMetadataPatch,
    ) -> DomainResult<VideoSummaryDto> {
        require_admin(actor)?;
        let mut item = self
            .catalog
            .find_video_by_id(video_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("video not found".into()))?;

        item.apply_patch(patch)?;
        self.catalog.save_video(&item).await?;
        if item.show_id.is_some() {
            self.catalog.recompute_show_aggregates().await?;
        }
        self.enrich_videos(actor.id, vec![item])
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
        require_admin(actor)?;
        if bytes.is_empty() {
            return Err(DomainError::Validation(
                "poster image cannot be empty".into(),
            ));
        }
        let mut item = self
            .catalog
            .find_video_by_id(video_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("video not found".into()))?;
        let key = self.storage.save_poster_bytes(bytes).await?;
        item.poster_key = Some(key);
        item.updated_at = Timestamp::now();
        self.catalog.save_video(&item).await?;
        self.enrich_videos(actor.id, vec![item])
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound("video not found".into()))
    }

    pub(crate) async fn delete_video_internal(&self, item: VideoItem) -> DomainResult<()> {
        let video_id = item.id;
        let old_show_id = item.show_id;
        let deleted_show_id = self.delete_video_inner(item, true).await?;
        let _ = (video_id, old_show_id, deleted_show_id);
        Ok(())
    }

    pub(crate) async fn delete_video_without_bindings(&self, item: VideoItem) -> DomainResult<()> {
        self.delete_video_inner(item, false).await?;
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
        self.watch.delete_progress_by_video(video_id).await?;
        self.catalog.delete_video(video_id).await?;
        self.catalog.recompute_show_aggregates().await?;
        let deleted_show_id = if let Some(sid) = old_show_id {
            if self.catalog.find_show_by_id(sid).await?.is_none() {
                if delete_bindings {
                    self.bindings
                        .delete_item_bindings(MediaKind::Video, sid.0)
                        .await?;
                }
                Some(sid)
            } else {
                None
            }
        } else {
            if delete_bindings {
                self.bindings
                    .delete_item_bindings(MediaKind::Video, video_id.0)
                    .await?;
            }
            None
        };
        self.watch
            .delete_video_references(video_id, deleted_show_id)
            .await?;
        self.events.publish(SystemEvent::MediaChanged {
            media_kind: MediaKind::Video,
        });
        Ok(deleted_show_id)
    }

    pub(crate) async fn delete_show_without_bindings(&self, show_id: ShowId) -> DomainResult<()> {
        let episodes = self.catalog.list_episodes_by_show(show_id).await?;
        let mut ep_ids = Vec::with_capacity(episodes.len());
        for ep in episodes {
            for key in ep.source.managed_file_keys() {
                let _ = self.storage.delete_managed_file(key).await;
            }
            ep_ids.push(ep.id);
            self.catalog.delete_video(ep.id).await?;
        }
        self.watch
            .delete_library_references(&ep_ids, &[show_id])
            .await?;
        self.catalog.delete_show(show_id).await?;
        self.catalog.recompute_show_aggregates().await?;
        self.events.publish(SystemEvent::MediaChanged {
            media_kind: MediaKind::Video,
        });
        Ok(())
    }

    pub async fn delete_video(&self, actor: &User, video_id: VideoId) -> DomainResult<()> {
        require_admin(actor)?;
        let item = self
            .catalog
            .find_video_by_id(video_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("video not found".into()))?;
        self.delete_video_internal(item).await
    }

    pub async fn list_shows(
        &self,
        user: &User,
        filter: ShowQueryFilter,
    ) -> DomainResult<Vec<ShowSummaryDto>> {
        let bound_ids = self
            .bindings
            .list_bound_ids(user.id, MediaKind::Video)
            .await?;
        if bound_ids.is_empty() {
            return Ok(Vec::new());
        }
        let shows: Vec<_> = self
            .catalog
            .list_shows(&filter)
            .await?
            .into_iter()
            .filter(|s| bound_ids.contains(&s.id.0))
            .collect();
        self.enrich_shows(user.id, shows).await
    }

    pub async fn get_show_detail(
        &self,
        user: &User,
        show_id: ShowId,
    ) -> DomainResult<ShowDetailDto> {
        self.require_show_bound(user.id, show_id).await?;
        let show = self
            .catalog
            .find_show_by_id(show_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("show not found".into()))?;
        let episodes = self.catalog.list_episodes_by_show(show_id).await?;
        let progress_list = self.watch.list_user_progress(user.id).await?;
        let all_favs = self.watch.list_favorites(user.id, None).await?;

        let prog_map: HashMap<VideoId, WatchProgress> =
            progress_list.into_iter().map(|p| (p.video_id, p)).collect();
        let mut video_fav_set: HashSet<Uuid> = HashSet::new();
        let mut show_fav_set: HashSet<Uuid> = HashSet::new();
        for fav in all_favs {
            match fav.kind {
                VideoFavoriteKind::Video => {
                    video_fav_set.insert(fav.target_id);
                }
                VideoFavoriteKind::Show => {
                    show_fav_set.insert(fav.target_id);
                }
            }
        }

        let watched_count = episodes
            .iter()
            .filter(|ep| prog_map.get(&ep.id).is_some_and(|p| p.completed))
            .count() as u32;
        let mut watched_by_show = HashMap::with_capacity(1);
        watched_by_show.insert(show.id, watched_count);

        let show_dto = Self::enrich_shows_with_maps(vec![show], &show_fav_set, &watched_by_show)
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound("show not found".into()))?;
        let ep_dtos = Self::enrich_videos_with_maps(episodes, &prog_map, &video_fav_set);

        let continue_episode = ep_dtos
            .iter()
            .filter(|e| {
                e.progress
                    .as_ref()
                    .is_some_and(|p| !p.completed && p.position_secs > 1.0)
            })
            .max_by_key(|e| e.progress.as_ref().map(|p| p.updated_at))
            .cloned()
            .or_else(|| {
                ep_dtos
                    .iter()
                    .find(|e| e.progress.as_ref().is_none_or(|p| !p.completed))
                    .cloned()
            })
            .or_else(|| ep_dtos.first().cloned());

        let mut season_buckets: BTreeMap<u32, Vec<VideoSummaryDto>> = BTreeMap::new();
        for ep in ep_dtos {
            let s_num = ep.season_number.unwrap_or(1);
            season_buckets.entry(s_num).or_default().push(ep);
        }
        let seasons = season_buckets
            .into_iter()
            .map(|(season_number, episodes)| SeasonGroupDto {
                season_number,
                episodes,
            })
            .collect();

        Ok(ShowDetailDto {
            show: show_dto,
            seasons,
            continue_episode,
        })
    }

    pub async fn update_show_metadata(
        &self,
        actor: &User,
        show_id: ShowId,
        patch: ShowMetadataPatch,
    ) -> DomainResult<ShowSummaryDto> {
        require_admin(actor)?;
        let mut show = self
            .catalog
            .find_show_by_id(show_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("show not found".into()))?;
        let old_show_key = show.show_key.clone();
        let title_changed = show.apply_patch(patch)?;
        if show.show_key != old_show_key
            && let Some(conflict) = self.catalog.find_show_by_key(&show.show_key).await?
            && conflict.id != show.id
        {
            return Err(DomainError::Conflict(format!(
                "a show named '{}' already exists in this library",
                show.title
            )));
        }
        self.catalog.save_show(&show).await?;
        if title_changed {
            let episodes = self.catalog.list_episodes_by_show(show_id).await?;
            for mut ep in episodes {
                ep.show_title = Some(show.title.clone());
                ep.updated_at = Timestamp::now();
                self.catalog.save_video(&ep).await?;
            }
        }
        self.enrich_shows(actor.id, vec![show])
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound("show not found".into()))
    }

    pub async fn upload_show_poster(
        &self,
        actor: &User,
        show_id: ShowId,
        bytes: &[u8],
    ) -> DomainResult<ShowSummaryDto> {
        require_admin(actor)?;
        if bytes.is_empty() {
            return Err(DomainError::Validation(
                "poster image cannot be empty".into(),
            ));
        }
        let mut show = self
            .catalog
            .find_show_by_id(show_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("show not found".into()))?;
        let key = self.storage.save_poster_bytes(bytes).await?;
        show.poster_key = Some(key);
        show.updated_at = Timestamp::now();
        self.catalog.save_show(&show).await?;
        self.enrich_shows(actor.id, vec![show])
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound("show not found".into()))
    }

    pub async fn get_poster(
        &self,
        actor: Option<&User>,
        key: &str,
        max_size: Option<u32>,
    ) -> DomainResult<BinaryPayload> {
        if let Some(user) = actor {
            let videos = self
                .catalog
                .list_videos(&VideoQueryFilter::default())
                .await?;
            let shows = self.catalog.list_shows(&ShowQueryFilter::default()).await?;

            let mut matched_pool = false;
            let mut matched_tracked_binding_ids: Vec<Uuid> = Vec::new();
            for v in &videos {
                if v.poster_key.as_deref() == Some(key) {
                    if v.source.is_online_tracked() {
                        matched_tracked_binding_ids.push(video_binding_id(v));
                    } else {
                        matched_pool = true;
                        break;
                    }
                }
            }
            if !matched_pool {
                for s in &shows {
                    if s.poster_key.as_deref() == Some(key) {
                        if s.online_tracked_ref().is_some() {
                            matched_tracked_binding_ids.push(s.id.0);
                        } else {
                            matched_pool = true;
                            break;
                        }
                    }
                }
            }
            if !matched_pool && !matched_tracked_binding_ids.is_empty() {
                let bound = self
                    .bindings
                    .list_bound_ids(user.id, MediaKind::Video)
                    .await?;
                if !matched_tracked_binding_ids
                    .iter()
                    .any(|id| bound.contains(id))
                {
                    return Err(DomainError::NotFound("poster not found".into()));
                }
            }
        }
        self.storage.read_poster(key, max_size).await
    }

    pub(crate) async fn find_raw_video(
        &self,
        video_id: VideoId,
    ) -> DomainResult<Option<VideoItem>> {
        Ok(self
            .catalog
            .find_video_by_id(video_id)
            .await?
            .filter(|v| !v.is_missing))
    }

    pub(crate) async fn find_raw_show(&self, show_id: ShowId) -> DomainResult<Option<VideoShow>> {
        self.catalog.find_show_by_id(show_id).await
    }

    pub(crate) async fn list_raw_shows(
        &self,
        filter: &ShowQueryFilter,
    ) -> DomainResult<Vec<VideoShow>> {
        self.catalog.list_shows(filter).await
    }
}

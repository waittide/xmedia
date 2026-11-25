mod pool_bindings;
mod shows_subtitles;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use jiff::Timestamp;
use uuid::Uuid;
use xmedia_core::application::bindings::MediaBindingService;
use xmedia_core::application::ports::RealtimeEventPort;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::{LibraryId, MediaKind};
use xmedia_core::domain::repository::LibraryRepository;
use xmedia_core::domain::user::{User, UserId, require_admin};

use crate::application::dto::{
    OnlineTrackedShowDto, OnlineTrackedVideoDto, ShowSummaryDto, VideoDetailDto,
    VideoHomeOverviewDto, VideoSummaryDto, VideoSystemStatusDto,
};
use crate::application::ports::{VideoMediaEnginePort, VideoStoragePort};
use crate::domain::id::{ShowId, VideoId};
use crate::domain::repository::{VideoCatalogRepository, VideoWatchRepository};
use crate::domain::video::{
    ShowQueryFilter, VideoItem, VideoKindFilter, VideoMetadataPatch, VideoQueryFilter, VideoShow,
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
                    s.online_tracked()
                        .map(|(source_id, item_id, _)| OnlineTrackedShowDto {
                            source_id,
                            item_key: item_id.to_string(),
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
                        kind: VideoKindFilter::Episodes,
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
        item.set_poster_key(key, Timestamp::now());
        self.catalog.save_video(&item).await?;
        self.enrich_videos(actor.id, vec![item])
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound("video not found".into()))
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

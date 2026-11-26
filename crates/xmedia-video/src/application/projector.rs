use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use uuid::Uuid;
use xmedia_core::application::bindings::MediaBindingService;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;
use xmedia_core::domain::user::{UserId, VideoQualityProfile};

use crate::application::dto::{
    OnlineTrackedShowDto, OnlineTrackedVideoDto, ShowSummaryDto, VideoSummaryDto,
};
use crate::domain::id::{ShowId, VideoId};
use crate::domain::repository::{
    ProgressScope, VideoCatalogRepository, VideoScope, VideoWatchRepository,
};
use crate::domain::video::{VideoItem, VideoKindFilter, VideoQueryFilter, VideoShow};
use crate::domain::watch::{VideoFavoriteKind, WatchProgress};

#[derive(Clone)]
pub(in crate::application) struct VideoProjector {
    catalog: Arc<dyn VideoCatalogRepository>,
    watch: Arc<dyn VideoWatchRepository>,
    bindings: Arc<MediaBindingService>,
}

impl VideoProjector {
    pub(in crate::application) fn new(
        catalog: Arc<dyn VideoCatalogRepository>,
        watch: Arc<dyn VideoWatchRepository>,
        bindings: Arc<MediaBindingService>,
    ) -> Self {
        Self {
            catalog,
            watch,
            bindings,
        }
    }

    pub(in crate::application) async fn require_video_bound(
        &self,
        user_id: UserId,
        video_id: VideoId,
    ) -> DomainResult<VideoItem> {
        let video = self
            .catalog
            .query_videos(VideoScope::Id(video_id))
            .await?
            .into_iter()
            .next()
            .filter(|v| !v.is_missing)
            .ok_or_else(|| DomainError::NotFound("video not found".into()))?;
        let bid = video.binding_id();
        self.bindings
            .require_bound(user_id, MediaKind::Video, bid, "video not found")
            .await?;
        Ok(video)
    }

    pub(in crate::application) async fn require_show_bound(
        &self,
        user_id: UserId,
        show_id: ShowId,
    ) -> DomainResult<()> {
        self.bindings
            .require_bound(user_id, MediaKind::Video, show_id.0, "show not found")
            .await
    }

    pub(in crate::application) async fn enrich_videos(
        &self,
        user_id: UserId,
        videos: Vec<VideoItem>,
    ) -> DomainResult<Vec<VideoSummaryDto>> {
        if videos.is_empty() {
            return Ok(Vec::new());
        }
        let progress_list = self
            .watch
            .query_progress(ProgressScope::User(user_id))
            .await?;
        let favs = self
            .watch
            .list_favorites(user_id, Some(VideoFavoriteKind::Video))
            .await?;

        let prog_map: HashMap<VideoId, WatchProgress> =
            progress_list.into_iter().map(|p| (p.video_id, p)).collect();
        let fav_set: HashSet<Uuid> = favs.into_iter().map(|f| f.target_id).collect();

        Ok(enrich_videos_with_maps(videos, &prog_map, &fav_set))
    }

    pub(in crate::application) async fn enrich_shows(
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

        let progress_list = self
            .watch
            .query_progress(ProgressScope::User(user_id))
            .await?;
        let completed_video_ids: HashSet<VideoId> = progress_list
            .into_iter()
            .filter(|p| p.completed)
            .map(|p| p.video_id)
            .collect();

        let mut watched_by_show: HashMap<ShowId, u32> = HashMap::new();
        if !completed_video_ids.is_empty() {
            if shows.len() == 1 {
                let show_id = shows[0].id;
                let episodes = self
                    .catalog
                    .query_videos(VideoScope::ShowEpisodes(show_id))
                    .await?;
                let watched = episodes
                    .iter()
                    .filter(|ep| completed_video_ids.contains(&ep.id))
                    .count() as u32;
                watched_by_show.insert(show_id, watched);
            } else {
                let all_episodes = self
                    .catalog
                    .query_videos(VideoScope::Filter(&VideoQueryFilter {
                        kind: VideoKindFilter::Episodes,
                        ..Default::default()
                    }))
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

        Ok(enrich_shows_with_maps(shows, &fav_set, &watched_by_show))
    }
}

pub(in crate::application) fn enrich_videos_with_maps(
    videos: Vec<VideoItem>,
    prog_map: &HashMap<VideoId, WatchProgress>,
    fav_set: &HashSet<Uuid>,
) -> Vec<VideoSummaryDto> {
    videos
        .into_iter()
        .map(|v| {
            let resolution = v.resolution_label();
            let can_direct_play = v
                .resolve_playback_plan(VideoQualityProfile::Auto, None, None)
                .is_ok_and(|plan| plan.is_none());
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

pub(in crate::application) fn enrich_shows_with_maps(
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

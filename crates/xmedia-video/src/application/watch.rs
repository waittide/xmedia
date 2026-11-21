use std::collections::HashMap;
use std::sync::Arc;

use jiff::Timestamp;
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::user::User;

use crate::application::catalog::VideoCatalogService;
use crate::application::dto::{
    SaveWatchProgressInput, UpsertVideoShelfInput, VideoFavoritesDto, VideoShelfDetailDto,
    VideoShelfDto,
};
use crate::domain::id::{ShowId, VideoId, VideoShelfId};
use crate::domain::repository::{VideoCatalogRepository, VideoWatchRepository};
use crate::domain::video::{ShowQueryFilter, VideoItem};
use crate::domain::watch::{VideoFavoriteKind, VideoShelf, WatchProgress};

#[derive(Clone)]
pub struct VideoWatchService {
    catalog_repo: Arc<dyn VideoCatalogRepository>,
    watch_repo: Arc<dyn VideoWatchRepository>,
    catalog_service: VideoCatalogService,
}

impl VideoWatchService {
    pub fn new(
        catalog_repo: Arc<dyn VideoCatalogRepository>,
        watch_repo: Arc<dyn VideoWatchRepository>,
        catalog_service: VideoCatalogService,
    ) -> Self {
        Self {
            catalog_repo,
            watch_repo,
            catalog_service,
        }
    }

    pub async fn save_progress(
        &self,
        user: &User,
        video_id: VideoId,
        input: SaveWatchProgressInput,
    ) -> DomainResult<WatchProgress> {
        let video = self
            .catalog_repo
            .find_video_by_id(video_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("video not found".into()))?;

        let existing = self.watch_repo.get_progress(user.id, video_id).await?;
        let duration_secs = input
            .duration_secs
            .filter(|&d| d > 0)
            .unwrap_or(video.duration_secs);
        let position_secs = input.position_secs.max(0.0);
        let completed =
            WatchProgress::compute_completed(position_secs, duration_secs, input.completed);

        let progress = WatchProgress {
            user_id: user.id,
            video_id,
            position_secs,
            duration_secs,
            completed,
            selected_audio_stream: input
                .selected_audio_stream
                .or_else(|| existing.as_ref().and_then(|e| e.selected_audio_stream)),
            selected_subtitle: input
                .selected_subtitle
                .or_else(|| existing.and_then(|e| e.selected_subtitle)),
            updated_at: Timestamp::now(),
        };
        self.watch_repo.save_progress(&progress).await?;
        Ok(progress)
    }

    pub async fn set_watched(
        &self,
        user: &User,
        video_id: VideoId,
        watched: bool,
    ) -> DomainResult<WatchProgress> {
        let video = self
            .catalog_repo
            .find_video_by_id(video_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("video not found".into()))?;
        let existing = self.watch_repo.get_progress(user.id, video_id).await?;
        let pos = if watched {
            f64::from(video.duration_secs)
        } else {
            0.0
        };
        let progress = WatchProgress {
            user_id: user.id,
            video_id,
            position_secs: pos,
            duration_secs: video.duration_secs,
            completed: watched,
            selected_audio_stream: existing.as_ref().and_then(|e| e.selected_audio_stream),
            selected_subtitle: existing.and_then(|e| e.selected_subtitle),
            updated_at: Timestamp::now(),
        };
        self.watch_repo.save_progress(&progress).await?;
        Ok(progress)
    }

    pub async fn set_favorite(
        &self,
        user: &User,
        kind: VideoFavoriteKind,
        target_id: Uuid,
        favorite: bool,
    ) -> DomainResult<()> {
        if favorite {
            match kind {
                VideoFavoriteKind::Video => {
                    if self
                        .catalog_repo
                        .find_video_by_id(VideoId(target_id))
                        .await?
                        .is_none()
                    {
                        return Err(DomainError::NotFound("video not found".into()));
                    }
                }
                VideoFavoriteKind::Show => {
                    if self
                        .catalog_repo
                        .find_show_by_id(ShowId(target_id))
                        .await?
                        .is_none()
                    {
                        return Err(DomainError::NotFound("show not found".into()));
                    }
                }
            }
            self.watch_repo.set_favorite(user.id, kind, target_id).await
        } else {
            self.watch_repo
                .unset_favorite(user.id, kind, target_id)
                .await
        }
    }

    pub async fn list_favorites(&self, user: &User) -> DomainResult<VideoFavoritesDto> {
        let favs = self.watch_repo.list_favorites(user.id, None).await?;
        let mut video_ids = Vec::new();
        let mut show_ids = Vec::new();
        for f in favs {
            match f.kind {
                VideoFavoriteKind::Video => video_ids.push(VideoId(f.target_id)),
                VideoFavoriteKind::Show => show_ids.push(ShowId(f.target_id)),
            }
        }

        let fetched_videos = self.catalog_repo.find_videos_by_ids(&video_ids).await?;
        let v_map: HashMap<VideoId, VideoItem> =
            fetched_videos.into_iter().map(|v| (v.id, v)).collect();
        let ordered_videos: Vec<VideoItem> = video_ids
            .iter()
            .filter_map(|id| v_map.get(id).cloned())
            .filter(|v| !v.is_missing)
            .collect();
        let videos = self
            .catalog_service
            .enrich_videos(user.id, ordered_videos)
            .await?;

        let all_shows = self
            .catalog_repo
            .list_shows(&ShowQueryFilter::default())
            .await?;
        let s_map: HashMap<ShowId, _> = all_shows.into_iter().map(|s| (s.id, s)).collect();
        let ordered_shows: Vec<_> = show_ids
            .iter()
            .filter_map(|id| s_map.get(id).cloned())
            .collect();
        let shows = self
            .catalog_service
            .enrich_shows(user.id, ordered_shows)
            .await?;

        Ok(VideoFavoritesDto { videos, shows })
    }

    pub async fn list_shelves(&self, user: &User) -> DomainResult<Vec<VideoShelfDto>> {
        let shelves = self.watch_repo.list_shelves(user.id).await?;
        let mut first_ids = Vec::new();
        for s in &shelves {
            if let Some(&fid) = s.video_ids.first() {
                first_ids.push(fid);
            }
        }
        let fetched = self.catalog_repo.find_videos_by_ids(&first_ids).await?;
        let poster_lookup: HashMap<VideoId, Option<String>> =
            fetched.into_iter().map(|v| (v.id, v.poster_key)).collect();

        Ok(shelves
            .into_iter()
            .map(|s| {
                let poster_key = s
                    .video_ids
                    .first()
                    .and_then(|fid| poster_lookup.get(fid).cloned())
                    .flatten();
                VideoShelfDto {
                    id: s.id,
                    name: s.name,
                    description: s.description,
                    video_count: s.video_ids.len(),
                    poster_key,
                    updated_at: s.updated_at,
                }
            })
            .collect())
    }

    pub async fn get_shelf(
        &self,
        user: &User,
        shelf_id: VideoShelfId,
    ) -> DomainResult<VideoShelfDetailDto> {
        let shelf = self
            .watch_repo
            .find_shelf_by_id(shelf_id)
            .await?
            .filter(|s| s.user_id == user.id)
            .ok_or_else(|| DomainError::NotFound("video shelf not found".into()))?;
        let fetched = self
            .catalog_repo
            .find_videos_by_ids(&shelf.video_ids)
            .await?;
        let v_map: HashMap<VideoId, VideoItem> = fetched.into_iter().map(|v| (v.id, v)).collect();
        let ordered: Vec<VideoItem> = shelf
            .video_ids
            .iter()
            .filter_map(|id| v_map.get(id).cloned())
            .filter(|v| !v.is_missing)
            .collect();
        let poster_key = ordered.first().and_then(|v| v.poster_key.clone());
        let videos = self.catalog_service.enrich_videos(user.id, ordered).await?;

        Ok(VideoShelfDetailDto {
            shelf: VideoShelfDto {
                id: shelf.id,
                name: shelf.name,
                description: shelf.description,
                video_count: videos.len(),
                poster_key,
                updated_at: shelf.updated_at,
            },
            videos,
        })
    }

    pub async fn create_shelf(
        &self,
        user: &User,
        input: UpsertVideoShelfInput,
    ) -> DomainResult<VideoShelfDetailDto> {
        let shelf = VideoShelf::new(user.id, input.name, input.description, input.video_ids)?;
        self.watch_repo.save_shelf(&shelf).await?;
        self.get_shelf(user, shelf.id).await
    }

    pub async fn update_shelf(
        &self,
        user: &User,
        shelf_id: VideoShelfId,
        input: UpsertVideoShelfInput,
    ) -> DomainResult<VideoShelfDetailDto> {
        let mut shelf = self
            .watch_repo
            .find_shelf_by_id(shelf_id)
            .await?
            .filter(|s| s.user_id == user.id)
            .ok_or_else(|| DomainError::NotFound("video shelf not found".into()))?;
        let trimmed = input.name.trim();
        if trimmed.is_empty() || trimmed.len() > 120 {
            return Err(DomainError::Validation(
                "shelf name must be between 1 and 120 characters".into(),
            ));
        }
        shelf.name = trimmed.to_string();
        shelf.description = input.description.trim().to_string();
        shelf.video_ids = input.video_ids;
        shelf.updated_at = Timestamp::now();
        self.watch_repo.save_shelf(&shelf).await?;
        self.get_shelf(user, shelf.id).await
    }

    pub async fn delete_shelf(&self, user: &User, shelf_id: VideoShelfId) -> DomainResult<()> {
        let shelf = self
            .watch_repo
            .find_shelf_by_id(shelf_id)
            .await?
            .filter(|s| s.user_id == user.id)
            .ok_or_else(|| DomainError::NotFound("video shelf not found".into()))?;
        self.watch_repo.delete_shelf(shelf.id).await
    }
}

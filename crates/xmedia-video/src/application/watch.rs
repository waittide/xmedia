use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use uuid::Uuid;
use xmedia_core::application::bindings::MediaBindingService;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;
use xmedia_core::domain::user::User;

use crate::application::dto::{
    SaveWatchProgressInput, UpsertVideoShelfInput, VideoFavoritesDto, VideoShelfDetailDto,
    VideoShelfDto,
};
use crate::application::projector::VideoProjector;
use crate::domain::id::{ShowId, VideoId, VideoShelfId};
use crate::domain::repository::{
    ProgressScope, ShowScope, VideoCatalogRepository, VideoScope, VideoShelfScope,
    VideoWatchMutation, VideoWatchPurge, VideoWatchRepository,
};
use crate::domain::video::{ShowQueryFilter, VideoItem};
use crate::domain::watch::{VideoFavoriteKind, VideoShelf, WatchProgress};

#[derive(Clone)]
pub struct VideoWatchService {
    catalog_repo: Arc<dyn VideoCatalogRepository>,
    watch_repo: Arc<dyn VideoWatchRepository>,
    bindings: Arc<MediaBindingService>,
    projector: VideoProjector,
}

impl VideoWatchService {
    pub(super) fn new(
        catalog_repo: Arc<dyn VideoCatalogRepository>,
        watch_repo: Arc<dyn VideoWatchRepository>,
        bindings: Arc<MediaBindingService>,
        projector: VideoProjector,
    ) -> Self {
        Self {
            catalog_repo,
            watch_repo,
            bindings,
            projector,
        }
    }

    pub async fn save_progress(
        &self,
        user: &User,
        video_id: VideoId,
        input: SaveWatchProgressInput,
    ) -> DomainResult<WatchProgress> {
        let video = self
            .projector
            .require_video_bound(user.id, video_id)
            .await?;
        let existing = self
            .watch_repo
            .query_progress(ProgressScope::One {
                user_id: user.id,
                video_id,
            })
            .await?
            .into_iter()
            .next();

        let progress = WatchProgress::reconcile(
            user.id,
            video_id,
            video.duration_secs,
            input.position_secs,
            input.duration_secs,
            input.completed,
            input.selected_audio_stream,
            input.selected_subtitle,
            existing,
        )?;
        self.watch_repo
            .save(VideoWatchMutation::Progress(&progress))
            .await?;
        Ok(progress)
    }

    pub async fn set_watched(
        &self,
        user: &User,
        video_id: VideoId,
        watched: bool,
    ) -> DomainResult<WatchProgress> {
        let video = self
            .projector
            .require_video_bound(user.id, video_id)
            .await?;
        let existing = self
            .watch_repo
            .query_progress(ProgressScope::One {
                user_id: user.id,
                video_id,
            })
            .await?
            .into_iter()
            .next();

        let progress =
            WatchProgress::mark_watched(user.id, video_id, video.duration_secs, watched, existing);
        self.watch_repo
            .save(VideoWatchMutation::Progress(&progress))
            .await?;
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
                    self.projector
                        .require_video_bound(user.id, VideoId(target_id))
                        .await?;
                }
                VideoFavoriteKind::Show => {
                    self.projector
                        .require_show_bound(user.id, ShowId(target_id))
                        .await?;
                    if self
                        .catalog_repo
                        .query_shows(ShowScope::Id(ShowId(target_id)))
                        .await?
                        .is_empty()
                    {
                        return Err(DomainError::NotFound("show not found".into()));
                    }
                }
            }
            self.watch_repo
                .save(VideoWatchMutation::SetFavorite {
                    user_id: user.id,
                    kind,
                    target_id,
                })
                .await
        } else {
            self.watch_repo
                .save(VideoWatchMutation::UnsetFavorite {
                    user_id: user.id,
                    kind,
                    target_id,
                })
                .await
        }
    }

    pub async fn list_favorites(&self, user: &User) -> DomainResult<VideoFavoritesDto> {
        let bound_ids = self
            .bindings
            .list_bound_ids(user.id, MediaKind::Video)
            .await?;
        if bound_ids.is_empty() {
            return Ok(VideoFavoritesDto {
                videos: Vec::new(),
                shows: Vec::new(),
            });
        }
        let favs = self.watch_repo.list_favorites(user.id, None).await?;
        let mut video_ids = Vec::new();
        let mut show_ids = Vec::new();
        for f in favs {
            match f.kind {
                VideoFavoriteKind::Video => video_ids.push(VideoId(f.target_id)),
                VideoFavoriteKind::Show => show_ids.push(ShowId(f.target_id)),
            }
        }

        let fetched_videos = self
            .catalog_repo
            .query_videos(VideoScope::Ids(&video_ids))
            .await?;
        let v_map: HashMap<VideoId, VideoItem> =
            fetched_videos.into_iter().map(|v| (v.id, v)).collect();
        let ordered_videos: Vec<VideoItem> = video_ids
            .iter()
            .filter_map(|id| v_map.get(id).cloned())
            .filter(|v| !v.is_missing && bound_ids.contains(&v.binding_id()))
            .collect();
        let videos = self
            .projector
            .enrich_videos(user.id, ordered_videos)
            .await?;

        let all_shows = self
            .catalog_repo
            .query_shows(ShowScope::Filter(&ShowQueryFilter::default()))
            .await?;
        let s_map: HashMap<ShowId, _> = all_shows.into_iter().map(|s| (s.id, s)).collect();
        let ordered_shows: Vec<_> = show_ids
            .iter()
            .filter_map(|id| s_map.get(id).cloned())
            .filter(|s| bound_ids.contains(&s.id.0))
            .collect();
        let shows = self.projector.enrich_shows(user.id, ordered_shows).await?;

        Ok(VideoFavoritesDto { videos, shows })
    }

    pub async fn list_shelves(&self, user: &User) -> DomainResult<Vec<VideoShelfDto>> {
        let bound_ids = self
            .bindings
            .list_bound_ids(user.id, MediaKind::Video)
            .await?;
        let shelves = self
            .watch_repo
            .query_shelves(VideoShelfScope::User(user.id))
            .await?;
        let all_ids: Vec<VideoId> = shelves
            .iter()
            .flat_map(|s| s.video_ids.iter().copied())
            .collect();
        let fetched = self
            .catalog_repo
            .query_videos(VideoScope::Ids(&all_ids))
            .await?;
        let video_lookup: HashMap<VideoId, VideoItem> = fetched
            .into_iter()
            .filter(|v| !v.is_missing && bound_ids.contains(&v.binding_id()))
            .map(|v| (v.id, v))
            .collect();

        Ok(shelves
            .into_iter()
            .map(|s| {
                let bound_videos: Vec<&VideoItem> = s
                    .video_ids
                    .iter()
                    .filter_map(|id| video_lookup.get(id))
                    .collect();
                let poster_key = bound_videos.first().and_then(|v| v.poster_key.clone());
                VideoShelfDto {
                    id: s.id,
                    name: s.name,
                    description: s.description,
                    video_count: bound_videos.len(),
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
        let shelf = self.load_owned_shelf(user, shelf_id).await?;
        let bound_ids = self
            .bindings
            .list_bound_ids(user.id, MediaKind::Video)
            .await?;
        let fetched = self
            .catalog_repo
            .query_videos(VideoScope::Ids(&shelf.video_ids))
            .await?;
        let v_map: HashMap<VideoId, VideoItem> = fetched.into_iter().map(|v| (v.id, v)).collect();
        let ordered: Vec<VideoItem> = shelf
            .video_ids
            .iter()
            .filter_map(|id| v_map.get(id).cloned())
            .filter(|v| !v.is_missing && bound_ids.contains(&v.binding_id()))
            .collect();
        let poster_key = ordered.first().and_then(|v| v.poster_key.clone());
        let videos = self.projector.enrich_videos(user.id, ordered).await?;

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
        let filtered_ids = self.filter_bound_video_ids(user, input.video_ids).await?;
        let shelf = VideoShelf::new(user.id, input.name, input.description, filtered_ids)?;
        self.watch_repo
            .save(VideoWatchMutation::Shelf(&shelf))
            .await?;
        self.get_shelf(user, shelf.id).await
    }

    pub async fn update_shelf(
        &self,
        user: &User,
        shelf_id: VideoShelfId,
        input: UpsertVideoShelfInput,
    ) -> DomainResult<VideoShelfDetailDto> {
        let mut shelf = self.load_owned_shelf(user, shelf_id).await?;
        let filtered_ids = self.filter_bound_video_ids(user, input.video_ids).await?;
        shelf.update(input.name, input.description, filtered_ids)?;
        self.watch_repo
            .save(VideoWatchMutation::Shelf(&shelf))
            .await?;
        self.get_shelf(user, shelf.id).await
    }

    pub async fn delete_shelf(&self, user: &User, shelf_id: VideoShelfId) -> DomainResult<()> {
        let shelf = self.load_owned_shelf(user, shelf_id).await?;
        self.watch_repo
            .purge(VideoWatchPurge::Shelf(shelf.id))
            .await
    }

    async fn load_owned_shelf(
        &self,
        user: &User,
        shelf_id: VideoShelfId,
    ) -> DomainResult<VideoShelf> {
        self.watch_repo
            .query_shelves(VideoShelfScope::Id(shelf_id))
            .await?
            .into_iter()
            .next()
            .filter(|s| s.user_id == user.id)
            .ok_or_else(|| DomainError::NotFound("video shelf not found".into()))
    }

    async fn filter_bound_video_ids(
        &self,
        user: &User,
        video_ids: Vec<VideoId>,
    ) -> DomainResult<Vec<VideoId>> {
        if video_ids.is_empty() {
            return Ok(Vec::new());
        }
        let bound_ids = self
            .bindings
            .list_bound_ids(user.id, MediaKind::Video)
            .await?;
        if bound_ids.is_empty() {
            return Ok(Vec::new());
        }
        let fetched = self
            .catalog_repo
            .query_videos(VideoScope::Ids(&video_ids))
            .await?;
        let allowed: HashSet<VideoId> = fetched
            .into_iter()
            .filter(|v| !v.is_missing && bound_ids.contains(&v.binding_id()))
            .map(|v| v.id)
            .collect();
        Ok(video_ids
            .into_iter()
            .filter(|id| allowed.contains(id))
            .collect())
    }
}

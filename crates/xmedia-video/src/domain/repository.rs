use async_trait::async_trait;
use uuid::Uuid;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::domain::library::LibraryId;
use xmedia_core::domain::user::UserId;

use crate::domain::id::{ShowId, VideoId, VideoShelfId};
use crate::domain::video::{ShowQueryFilter, VideoItem, VideoQueryFilter, VideoShow};
use crate::domain::watch::{VideoFavoriteEntry, VideoFavoriteKind, VideoShelf, WatchProgress};

#[async_trait]
pub trait VideoCatalogRepository: Send + Sync {
    async fn find_video_by_id(&self, id: VideoId) -> DomainResult<Option<VideoItem>>;
    async fn find_videos_by_ids(&self, ids: &[VideoId]) -> DomainResult<Vec<VideoItem>>;
    async fn list_videos(&self, filter: &VideoQueryFilter) -> DomainResult<Vec<VideoItem>>;
    async fn list_videos_by_library(&self, library_id: LibraryId) -> DomainResult<Vec<VideoItem>>;
    async fn list_episodes_by_show(&self, show_id: ShowId) -> DomainResult<Vec<VideoItem>>;
    async fn count_active_by_library(&self, library_id: LibraryId) -> DomainResult<u64>;
    async fn count_all_active_videos(&self) -> DomainResult<u64>;
    async fn save_video(&self, video: &VideoItem) -> DomainResult<()>;
    async fn delete_video(&self, id: VideoId) -> DomainResult<()>;
    async fn delete_videos_by_library(&self, library_id: LibraryId) -> DomainResult<()>;

    async fn find_show_by_id(&self, id: ShowId) -> DomainResult<Option<VideoShow>>;
    async fn find_show_by_key(&self, show_key: &str) -> DomainResult<Option<VideoShow>>;
    async fn list_shows(&self, filter: &ShowQueryFilter) -> DomainResult<Vec<VideoShow>>;
    async fn save_show(&self, show: &VideoShow) -> DomainResult<()>;
    async fn delete_show(&self, id: ShowId) -> DomainResult<()>;
    async fn recompute_show_aggregates(&self) -> DomainResult<()>;
}

#[async_trait]
pub trait VideoWatchRepository: Send + Sync {
    async fn get_progress(
        &self,
        user_id: UserId,
        video_id: VideoId,
    ) -> DomainResult<Option<WatchProgress>>;
    async fn list_user_progress(&self, user_id: UserId) -> DomainResult<Vec<WatchProgress>>;
    async fn save_progress(&self, progress: &WatchProgress) -> DomainResult<()>;
    async fn delete_progress_by_video(&self, video_id: VideoId) -> DomainResult<()>;

    async fn set_favorite(
        &self,
        user_id: UserId,
        kind: VideoFavoriteKind,
        target_id: Uuid,
    ) -> DomainResult<()>;
    async fn unset_favorite(
        &self,
        user_id: UserId,
        kind: VideoFavoriteKind,
        target_id: Uuid,
    ) -> DomainResult<()>;
    async fn list_favorites(
        &self,
        user_id: UserId,
        kind: Option<VideoFavoriteKind>,
    ) -> DomainResult<Vec<VideoFavoriteEntry>>;

    async fn list_shelves(&self, user_id: UserId) -> DomainResult<Vec<VideoShelf>>;
    async fn find_shelf_by_id(&self, id: VideoShelfId) -> DomainResult<Option<VideoShelf>>;
    async fn save_shelf(&self, shelf: &VideoShelf) -> DomainResult<()>;
    async fn delete_shelf(&self, id: VideoShelfId) -> DomainResult<()>;

    async fn delete_by_user(&self, user_id: UserId) -> DomainResult<()>;
}

use async_trait::async_trait;
use uuid::Uuid;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::domain::library::LibraryId;
use xmedia_core::domain::user::UserId;

use crate::domain::id::{ShowId, VideoId, VideoShelfId};
use crate::domain::video::{ShowQueryFilter, VideoItem, VideoQueryFilter, VideoShow};
use crate::domain::watch::{VideoFavoriteEntry, VideoFavoriteKind, VideoShelf, WatchProgress};

pub enum VideoScope<'a> {
    Id(VideoId),
    Ids(&'a [VideoId]),
    PosterKey(&'a str),
    Library(LibraryId),
    ShowEpisodes(ShowId),
    Filter(&'a VideoQueryFilter),
}

pub enum ShowScope<'a> {
    Id(ShowId),
    Key(&'a str),
    OnlineTracked { source_id: Uuid, item_id: &'a str },
    PosterKey(&'a str),
    Filter(&'a ShowQueryFilter),
}

pub enum VideoCatalogMutation<'a> {
    Video(&'a VideoItem),
    Show(&'a VideoShow),
    RecomputeShowAggregates,
}

pub enum VideoCatalogDelete {
    Video(VideoId),
    LibraryVideos(LibraryId),
    Show(ShowId),
}

#[async_trait]
pub trait VideoCatalogRepository: Send + Sync {
    async fn query_videos(&self, scope: VideoScope<'_>) -> DomainResult<Vec<VideoItem>>;
    async fn query_shows(&self, scope: ShowScope<'_>) -> DomainResult<Vec<VideoShow>>;
    async fn save(&self, mutation: VideoCatalogMutation<'_>) -> DomainResult<()>;
    async fn delete(&self, scope: VideoCatalogDelete) -> DomainResult<()>;
}

pub enum ProgressScope {
    One { user_id: UserId, video_id: VideoId },
    User(UserId),
}

pub enum VideoShelfScope {
    Id(VideoShelfId),
    User(UserId),
}

pub enum VideoWatchMutation<'a> {
    Progress(&'a WatchProgress),
    SetFavorite {
        user_id: UserId,
        kind: VideoFavoriteKind,
        target_id: Uuid,
    },
    UnsetFavorite {
        user_id: UserId,
        kind: VideoFavoriteKind,
        target_id: Uuid,
    },
    Shelf(&'a VideoShelf),
}

pub enum VideoWatchPurge<'a> {
    VideoProgress(VideoId),
    VideoReferences {
        video_id: VideoId,
        deleted_show_id: Option<ShowId>,
    },
    LibraryReferences {
        video_ids: &'a [VideoId],
        show_ids: &'a [ShowId],
    },
    Shelf(VideoShelfId),
    User(UserId),
    UserTargets {
        user_id: UserId,
        video_ids: &'a [VideoId],
        show_id: Option<ShowId>,
    },
}

#[async_trait]
pub trait VideoWatchRepository: Send + Sync {
    async fn query_progress(&self, scope: ProgressScope) -> DomainResult<Vec<WatchProgress>>;
    async fn list_favorites(
        &self,
        user_id: UserId,
        kind: Option<VideoFavoriteKind>,
    ) -> DomainResult<Vec<VideoFavoriteEntry>>;
    async fn query_shelves(&self, scope: VideoShelfScope) -> DomainResult<Vec<VideoShelf>>;
    async fn save(&self, mutation: VideoWatchMutation<'_>) -> DomainResult<()>;
    async fn purge(&self, scope: VideoWatchPurge<'_>) -> DomainResult<()>;
}

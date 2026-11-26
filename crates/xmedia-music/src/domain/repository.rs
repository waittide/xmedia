use std::collections::{HashMap, HashSet};

use async_trait::async_trait;
use jiff::Timestamp;
use uuid::Uuid;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::domain::library::LibraryId;
use xmedia_core::domain::user::UserId;

use crate::domain::catalog::{
    Album, AlbumId, AlbumListQuery, Artist, ArtistId, ArtistQuery, BoundCatalogCounts, GenreStat,
    MusicTagStat, PagedResult, Track, TrackId, TrackQuery,
};
use crate::domain::collection::{
    AudioBookmark, PlayQueue, Playlist, PlaylistId, ScrobbleEntry, StarTargetKind, StarredEntry,
    TrackPlayStat, UserEngagementSnapshot,
};

#[derive(Debug, Clone)]
pub enum TrackScope<'a> {
    Id(TrackId),
    Ids(&'a [TrackId]),
    CoverKey(&'a str),
    Library(LibraryId),
    Album {
        album_id: AlbumId,
        allowed: Option<&'a HashSet<TrackId>>,
    },
    Artist {
        artist_id: ArtistId,
        allowed: Option<&'a HashSet<TrackId>>,
    },
    Offset {
        allowed: &'a HashSet<TrackId>,
        query: &'a TrackQuery,
        offset: u32,
        limit: u32,
    },
    Random {
        allowed: &'a HashSet<TrackId>,
        size: u32,
        library_id: Option<LibraryId>,
        genre: Option<&'a str>,
        from_year: Option<i32>,
        to_year: Option<i32>,
    },
}

#[derive(Debug, Clone, Copy)]
pub enum TrackDeleteScope {
    Id(TrackId),
    Library(LibraryId),
}

#[derive(Debug, Clone)]
pub enum AlbumScope<'a> {
    Id(AlbumId),
    Key(&'a str),
    Ids(&'a [AlbumId]),
    CoverKey(&'a str),
    BoundByIds {
        allowed: &'a HashSet<TrackId>,
        album_ids: &'a [AlbumId],
        library_id: Option<LibraryId>,
    },
    BoundByArtist {
        allowed: &'a HashSet<TrackId>,
        artist_id: ArtistId,
    },
    BoundPaged {
        allowed: &'a HashSet<TrackId>,
        query: &'a AlbumListQuery,
        starred: &'a [StarredEntry],
        play_stats: &'a [TrackPlayStat],
    },
}

#[derive(Debug, Clone)]
pub enum ArtistScope<'a> {
    Id(ArtistId),
    Name(&'a str),
    Ids(&'a [ArtistId]),
    BoundByIds {
        allowed: &'a HashSet<TrackId>,
        artist_ids: &'a [ArtistId],
        library_id: Option<LibraryId>,
    },
    BoundPaged {
        allowed: &'a HashSet<TrackId>,
        query: &'a ArtistQuery,
    },
}

#[derive(Debug, Clone)]
pub enum CatalogFacetQuery<'a> {
    ActiveTrackIds,
    OverviewStats(&'a HashSet<TrackId>),
    LibraryTrackCounts(&'a HashSet<TrackId>),
    Genres(&'a HashSet<TrackId>),
    Tags(Option<&'a HashSet<TrackId>>),
    TrackTags(&'a [TrackId]),
    HasBoundTarget {
        allowed: &'a HashSet<TrackId>,
        kind: StarTargetKind,
        id: Uuid,
    },
}

#[derive(Debug, Clone)]
pub enum CatalogFacetResult {
    TrackIds(Vec<TrackId>),
    OverviewStats(BoundCatalogCounts),
    LibraryTrackCounts(HashMap<LibraryId, u64>),
    Genres(Vec<GenreStat>),
    Tags(Vec<MusicTagStat>),
    TrackTags(HashMap<TrackId, Vec<String>>),
    Exists(bool),
}

#[derive(Debug, Clone)]
pub enum CatalogMutation<'a> {
    Track(&'a Track),
    Album(&'a Album),
    Artist(&'a Artist),
    TrackTags {
        track_id: TrackId,
        tags: Vec<String>,
    },
    RecomputeAggregates,
}

#[async_trait]
pub trait MusicCatalogRepository: Send + Sync {
    async fn query_tracks(&self, scope: TrackScope<'_>) -> DomainResult<Vec<Track>>;
    async fn search_tracks(
        &self,
        allowed: Option<&HashSet<TrackId>>,
        query: &TrackQuery,
    ) -> DomainResult<PagedResult<Track>>;
    async fn query_albums(&self, scope: AlbumScope<'_>) -> DomainResult<PagedResult<Album>>;
    async fn query_artists(&self, scope: ArtistScope<'_>) -> DomainResult<PagedResult<Artist>>;
    async fn query_facets(&self, query: CatalogFacetQuery<'_>) -> DomainResult<CatalogFacetResult>;
    async fn save(&self, mutation: CatalogMutation<'_>) -> DomainResult<()>;
    async fn delete_tracks(&self, scope: TrackDeleteScope) -> DomainResult<()>;
}

#[derive(Debug, Clone, Copy)]
pub enum PlaylistQuery {
    Id(PlaylistId),
    VisibleTo(UserId),
}

#[derive(Debug, Clone)]
pub enum CollectionMutation<'a> {
    Playlist(&'a Playlist),
    PlayQueue(&'a PlayQueue),
    Bookmark(&'a AudioBookmark),
    Star {
        user_id: UserId,
        kind: StarTargetKind,
        target_id: Uuid,
        starred_at: Option<Timestamp>,
    },
    Rating {
        user_id: UserId,
        kind: StarTargetKind,
        target_id: Uuid,
        rating: Option<u8>,
    },
    Scrobble {
        user_id: UserId,
        track_id: TrackId,
        album_id: AlbumId,
        played_at: Timestamp,
    },
}

#[derive(Debug, Clone)]
pub enum CollectionPurgeScope<'a> {
    Playlist(PlaylistId),
    Bookmark {
        user_id: UserId,
        track_id: TrackId,
    },
    User(UserId),
    UserTrack {
        user_id: UserId,
        track_id: TrackId,
    },
    DeletedCatalog {
        track_ids: &'a [TrackId],
        album_ids: &'a [AlbumId],
        artist_ids: &'a [ArtistId],
    },
}

#[async_trait]
pub trait MusicCollectionRepository: Send + Sync {
    async fn query_playlists(&self, scope: PlaylistQuery) -> DomainResult<Vec<Playlist>>;
    async fn get_play_queue(&self, user_id: UserId) -> DomainResult<Option<PlayQueue>>;
    async fn query_bookmarks(
        &self,
        user_id: UserId,
        track_id: Option<TrackId>,
    ) -> DomainResult<Vec<AudioBookmark>>;
    async fn load_engagement(
        &self,
        user_id: UserId,
        kind: Option<StarTargetKind>,
    ) -> DomainResult<UserEngagementSnapshot>;
    async fn list_recent_scrobbles(
        &self,
        user_id: UserId,
        limit: u32,
    ) -> DomainResult<Vec<ScrobbleEntry>>;
    async fn save(&self, mutation: CollectionMutation<'_>) -> DomainResult<()>;
    async fn purge(&self, scope: CollectionPurgeScope<'_>) -> DomainResult<()>;
}

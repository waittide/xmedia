use async_trait::async_trait;
use jiff::Timestamp;
use uuid::Uuid;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::domain::library::LibraryId;
use xmedia_core::domain::user::UserId;

use crate::domain::catalog::{
    Album, AlbumId, AlbumListQuery, Artist, ArtistId, ArtistQuery, GenreStat, MusicTagStat,
    PagedResult, SearchQuery, SearchResult, Track, TrackId, TrackQuery,
};
use crate::domain::collection::{
    AudioBookmark, PlayQueue, Playlist, PlaylistId, RatingEntry, ScrobbleEntry, StarTargetKind,
    StarredEntry, TrackPlayStat,
};
use crate::domain::online::MusicSourceRecord;
use xmedia_source::model::SourceRuntimeState;

#[async_trait]
pub trait MusicCatalogRepository: Send + Sync {
    async fn find_track_by_id(&self, id: TrackId) -> DomainResult<Option<Track>>;
    async fn find_tracks_by_ids(&self, ids: &[TrackId]) -> DomainResult<Vec<Track>>;
    async fn list_tracks_by_library(&self, library_id: LibraryId) -> DomainResult<Vec<Track>>;
    async fn count_tracks_by_library(&self, library_id: LibraryId) -> DomainResult<u64>;
    async fn count_all_active_tracks(&self) -> DomainResult<u64>;
    async fn query_tracks(&self, query: &TrackQuery) -> DomainResult<PagedResult<Track>>;
    async fn list_tracks_by_album(&self, album_id: AlbumId) -> DomainResult<Vec<Track>>;
    async fn list_tracks_by_artist(&self, artist_id: ArtistId) -> DomainResult<Vec<Track>>;
    async fn list_random_tracks(
        &self,
        size: u32,
        library_id: Option<LibraryId>,
        genre: Option<&str>,
        from_year: Option<i32>,
        to_year: Option<i32>,
    ) -> DomainResult<Vec<Track>>;
    async fn list_tracks_by_genre(
        &self,
        genre: &str,
        offset: u32,
        size: u32,
        library_id: Option<LibraryId>,
    ) -> DomainResult<Vec<Track>>;
    async fn save_track(&self, track: &Track) -> DomainResult<()>;
    async fn delete_track(&self, id: TrackId) -> DomainResult<()>;
    async fn delete_tracks_by_library(&self, library_id: LibraryId) -> DomainResult<()>;

    async fn find_album_by_id(&self, id: AlbumId) -> DomainResult<Option<Album>>;
    async fn find_album_by_key(&self, album_key: &str) -> DomainResult<Option<Album>>;
    async fn find_albums_by_ids(&self, ids: &[AlbumId]) -> DomainResult<Vec<Album>>;
    async fn list_albums(
        &self,
        query: &AlbumListQuery,
        starred_album_ids: Option<&[AlbumId]>,
        ordered_album_ids: Option<&[AlbumId]>,
    ) -> DomainResult<PagedResult<Album>>;
    async fn list_albums_by_artist(&self, artist_id: ArtistId) -> DomainResult<Vec<Album>>;
    async fn save_album(&self, album: &Album) -> DomainResult<()>;

    async fn find_artist_by_id(&self, id: ArtistId) -> DomainResult<Option<Artist>>;
    async fn find_artist_by_name(&self, name: &str) -> DomainResult<Option<Artist>>;
    async fn find_artists_by_ids(&self, ids: &[ArtistId]) -> DomainResult<Vec<Artist>>;
    async fn list_artists(&self, query: &ArtistQuery) -> DomainResult<PagedResult<Artist>>;
    async fn save_artist(&self, artist: &Artist) -> DomainResult<()>;

    async fn list_genres(&self) -> DomainResult<Vec<GenreStat>>;
    async fn list_tags(&self) -> DomainResult<Vec<MusicTagStat>>;
    async fn count_active_tags(
        &self,
        _active_track_ids: &std::collections::HashSet<TrackId>,
    ) -> DomainResult<u64> {
        Ok(self.list_tags().await?.len() as u64)
    }
    async fn get_tags_for_tracks(
        &self,
        track_ids: &[TrackId],
    ) -> DomainResult<std::collections::HashMap<TrackId, Vec<String>>>;
    async fn set_track_tags(
        &self,
        track_id: TrackId,
        tags: Vec<String>,
    ) -> DomainResult<Vec<String>>;
    async fn search_catalog(&self, query: &SearchQuery) -> DomainResult<SearchResult>;
    async fn recompute_catalog_aggregates(&self) -> DomainResult<()>;
}

#[async_trait]
pub trait MusicCollectionRepository: Send + Sync {
    async fn find_playlist_by_id(&self, id: PlaylistId) -> DomainResult<Option<Playlist>>;
    async fn list_visible_playlists(&self, user_id: UserId) -> DomainResult<Vec<Playlist>>;
    async fn save_playlist(&self, playlist: &Playlist) -> DomainResult<()>;
    async fn delete_playlist(&self, id: PlaylistId) -> DomainResult<()>;
    async fn delete_tracks_references(
        &self,
        track_ids: &[TrackId],
        deleted_album_ids: &[AlbumId],
        deleted_artist_ids: &[ArtistId],
    ) -> DomainResult<()>;

    async fn set_starred(
        &self,
        user_id: UserId,
        kind: StarTargetKind,
        target_id: Uuid,
        starred_at: Timestamp,
    ) -> DomainResult<()>;
    async fn unset_starred(
        &self,
        user_id: UserId,
        kind: StarTargetKind,
        target_id: Uuid,
    ) -> DomainResult<()>;
    async fn list_starred(
        &self,
        user_id: UserId,
        kind: Option<StarTargetKind>,
    ) -> DomainResult<Vec<StarredEntry>>;

    async fn set_rating(
        &self,
        user_id: UserId,
        kind: StarTargetKind,
        target_id: Uuid,
        rating: u8,
    ) -> DomainResult<()>;
    async fn remove_rating(
        &self,
        user_id: UserId,
        kind: StarTargetKind,
        target_id: Uuid,
    ) -> DomainResult<()>;
    async fn list_user_ratings(&self, user_id: UserId) -> DomainResult<Vec<RatingEntry>>;

    async fn record_play(
        &self,
        user_id: UserId,
        track_id: TrackId,
        album_id: AlbumId,
        played_at: Timestamp,
    ) -> DomainResult<TrackPlayStat>;
    async fn list_user_play_stats(&self, user_id: UserId) -> DomainResult<Vec<TrackPlayStat>>;
    async fn list_recent_scrobbles(
        &self,
        user_id: UserId,
        limit: u32,
    ) -> DomainResult<Vec<ScrobbleEntry>>;

    async fn get_play_queue(&self, user_id: UserId) -> DomainResult<Option<PlayQueue>>;
    async fn save_play_queue(&self, queue: &PlayQueue) -> DomainResult<()>;

    async fn list_bookmarks(&self, user_id: UserId) -> DomainResult<Vec<AudioBookmark>>;
    async fn get_bookmark(
        &self,
        user_id: UserId,
        track_id: TrackId,
    ) -> DomainResult<Option<AudioBookmark>>;
    async fn save_bookmark(&self, bookmark: &AudioBookmark) -> DomainResult<()>;
    async fn delete_bookmark(&self, user_id: UserId, track_id: TrackId) -> DomainResult<()>;
    async fn delete_by_user(&self, user_id: UserId) -> DomainResult<()>;
}

#[async_trait]
pub trait MusicSourceRepository: Send + Sync {
    async fn list_all(&self) -> DomainResult<Vec<MusicSourceRecord>>;
    async fn find_by_id(&self, id: Uuid) -> DomainResult<Option<MusicSourceRecord>>;
    async fn save(&self, source: &MusicSourceRecord) -> DomainResult<()>;
    async fn update_existing(&self, source: &MusicSourceRecord) -> DomainResult<bool>;
    async fn update_runtime_state(
        &self,
        id: Uuid,
        runtime_state: SourceRuntimeState,
        updated_at: Timestamp,
    ) -> DomainResult<()>;
    async fn update_test_result(
        &self,
        id: Uuid,
        runtime_state: SourceRuntimeState,
        last_test_ok: bool,
        last_test_msg: Option<String>,
        updated_at: Timestamp,
    ) -> DomainResult<bool>;
    async fn update_weight_if_exists(
        &self,
        id: Uuid,
        weight: i64,
        updated_at: Timestamp,
    ) -> DomainResult<Option<MusicSourceRecord>>;
    async fn update_batch_fields_if_exists(
        &self,
        id: Uuid,
        enabled: Option<bool>,
        group_name: Option<String>,
        updated_at: Timestamp,
    ) -> DomainResult<bool>;
    async fn delete(&self, id: Uuid) -> DomainResult<()>;
}

mod albums_artists;
mod tags_search;
mod tracks;

use std::collections::{HashMap, HashSet};

use async_trait::async_trait;
use toasty::Db;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::domain::library::LibraryId;

use crate::domain::catalog::{
    Album, AlbumId, AlbumListQuery, Artist, ArtistId, ArtistQuery, AudioFormat, GenreStat,
    MusicTagStat, PagedResult, SearchQuery, SearchResult, Track, TrackId, TrackQuery,
};
use crate::domain::repository::MusicCatalogRepository;
use crate::infra::persistence::models::{MusicAlbumModel, MusicArtistModel, MusicTrackModel};

#[derive(Clone)]
pub struct ToastyMusicCatalogRepository {
    pub(super) db: Db,
}

impl ToastyMusicCatalogRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    pub(super) fn track_to_domain(m: MusicTrackModel) -> Track {
        let format = AudioFormat::parse(&m.format).unwrap_or(AudioFormat::Mp3);
        Track {
            id: TrackId(m.id),
            library_id: LibraryId(m.library_id),
            album_id: AlbumId(m.album_id),
            artist_id: ArtistId(m.artist_id),
            album_artist_id: ArtistId(m.album_artist_id),
            title: m.title,
            artist_name: m.artist_name,
            album_artist_name: m.album_artist_name,
            album_title: m.album_title,
            track_number: m.track_number.and_then(|n| u32::try_from(n).ok()),
            disc_number: m.disc_number.and_then(|n| u32::try_from(n).ok()),
            year: m.year.and_then(|y| i32::try_from(y).ok()),
            genre: m.genre,
            duration_secs: u32::try_from(m.duration_secs.max(0)).unwrap_or(0),
            bit_rate_kbps: m.bit_rate_kbps.and_then(|b| u32::try_from(b).ok()),
            sample_rate_hz: m.sample_rate_hz.and_then(|s| u32::try_from(s).ok()),
            bit_depth: m.bit_depth.and_then(|d| u16::try_from(d).ok()),
            channels: m.channels.and_then(|c| u8::try_from(c).ok()),
            format,
            file_size: u64::try_from(m.file_size.max(0)).unwrap_or(0),
            file_hash: m.file_hash,
            replay_gain: m.replay_gain.0,
            has_embedded_lyrics: m.has_embedded_lyrics,
            has_sidecar_lyrics: m.has_sidecar_lyrics,
            is_compilation: m.is_compilation,
            cover_key: m.cover_key,
            source: m.source.0,
            is_missing: m.is_missing,
            created_at: m.created_at,
            updated_at: m.updated_at,
        }
    }

    pub(super) fn album_to_domain(m: MusicAlbumModel) -> Album {
        Album {
            id: AlbumId(m.id),
            library_id: LibraryId(m.library_id),
            album_key: m.album_key,
            title: m.title,
            album_artist_id: ArtistId(m.album_artist_id),
            album_artist_name: m.album_artist_name,
            year: m.year.and_then(|y| i32::try_from(y).ok()),
            genre: m.genre,
            disc_count: u32::try_from(m.disc_count.max(1)).unwrap_or(1),
            track_count: u32::try_from(m.track_count.max(0)).unwrap_or(0),
            duration_secs: u32::try_from(m.duration_secs.max(0)).unwrap_or(0),
            is_compilation: m.is_compilation,
            cover_key: m.cover_key,
            created_at: m.created_at,
            updated_at: m.updated_at,
        }
    }

    pub(super) fn artist_to_domain(m: MusicArtistModel) -> Artist {
        Artist {
            id: ArtistId(m.id),
            name: m.name,
            is_album_artist: m.is_album_artist,
            is_track_artist: m.is_track_artist,
            album_count: u32::try_from(m.album_count.max(0)).unwrap_or(0),
            track_count: u32::try_from(m.track_count.max(0)).unwrap_or(0),
            cover_key: m.cover_key,
            created_at: m.created_at,
            updated_at: m.updated_at,
        }
    }
}

#[async_trait]
impl MusicCatalogRepository for ToastyMusicCatalogRepository {
    async fn find_track_by_id(&self, id: TrackId) -> DomainResult<Option<Track>> {
        self.find_track_by_id_impl(id).await
    }

    async fn find_tracks_by_ids(&self, ids: &[TrackId]) -> DomainResult<Vec<Track>> {
        self.find_tracks_by_ids_impl(ids).await
    }

    async fn list_tracks_by_library(&self, library_id: LibraryId) -> DomainResult<Vec<Track>> {
        self.list_tracks_by_library_impl(library_id).await
    }

    async fn count_tracks_by_library(&self, library_id: LibraryId) -> DomainResult<u64> {
        self.count_tracks_by_library_impl(library_id).await
    }

    async fn count_all_active_tracks(&self) -> DomainResult<u64> {
        self.count_all_active_tracks_impl().await
    }

    async fn list_all_active_track_ids(&self) -> DomainResult<Vec<TrackId>> {
        self.list_all_active_track_ids_impl().await
    }

    async fn query_tracks(&self, query: &TrackQuery) -> DomainResult<PagedResult<Track>> {
        self.query_tracks_impl(query).await
    }

    async fn list_tracks_by_album(&self, album_id: AlbumId) -> DomainResult<Vec<Track>> {
        self.list_tracks_by_album_impl(album_id).await
    }

    async fn list_tracks_by_artist(&self, artist_id: ArtistId) -> DomainResult<Vec<Track>> {
        self.list_tracks_by_artist_impl(artist_id).await
    }

    async fn list_random_tracks(
        &self,
        size: u32,
        library_id: Option<LibraryId>,
        genre: Option<&str>,
        from_year: Option<i32>,
        to_year: Option<i32>,
    ) -> DomainResult<Vec<Track>> {
        self.list_random_tracks_impl(size, library_id, genre, from_year, to_year)
            .await
    }

    async fn list_tracks_by_genre(
        &self,
        genre: &str,
        offset: u32,
        size: u32,
        library_id: Option<LibraryId>,
    ) -> DomainResult<Vec<Track>> {
        self.list_tracks_by_genre_impl(genre, offset, size, library_id)
            .await
    }

    async fn save_track(&self, track: &Track) -> DomainResult<()> {
        self.save_track_impl(track).await
    }

    async fn delete_track(&self, id: TrackId) -> DomainResult<()> {
        self.delete_track_impl(id).await
    }

    async fn delete_tracks_by_library(&self, library_id: LibraryId) -> DomainResult<()> {
        self.delete_tracks_by_library_impl(library_id).await
    }

    async fn find_album_by_id(&self, id: AlbumId) -> DomainResult<Option<Album>> {
        self.find_album_by_id_impl(id).await
    }

    async fn find_album_by_key(&self, album_key: &str) -> DomainResult<Option<Album>> {
        self.find_album_by_key_impl(album_key).await
    }

    async fn find_albums_by_ids(&self, ids: &[AlbumId]) -> DomainResult<Vec<Album>> {
        self.find_albums_by_ids_impl(ids).await
    }

    async fn list_albums(
        &self,
        query: &AlbumListQuery,
        starred_album_ids: Option<&[AlbumId]>,
        ordered_album_ids: Option<&[AlbumId]>,
    ) -> DomainResult<PagedResult<Album>> {
        self.list_albums_impl(query, starred_album_ids, ordered_album_ids)
            .await
    }

    async fn list_albums_by_artist(&self, artist_id: ArtistId) -> DomainResult<Vec<Album>> {
        self.list_albums_by_artist_impl(artist_id).await
    }

    async fn save_album(&self, album: &Album) -> DomainResult<()> {
        self.save_album_impl(album).await
    }

    async fn find_artist_by_id(&self, id: ArtistId) -> DomainResult<Option<Artist>> {
        self.find_artist_by_id_impl(id).await
    }

    async fn find_artist_by_name(&self, name: &str) -> DomainResult<Option<Artist>> {
        self.find_artist_by_name_impl(name).await
    }

    async fn find_artists_by_ids(&self, ids: &[ArtistId]) -> DomainResult<Vec<Artist>> {
        self.find_artists_by_ids_impl(ids).await
    }

    async fn list_artists(&self, query: &ArtistQuery) -> DomainResult<PagedResult<Artist>> {
        self.list_artists_impl(query).await
    }

    async fn save_artist(&self, artist: &Artist) -> DomainResult<()> {
        self.save_artist_impl(artist).await
    }

    async fn list_genres(&self) -> DomainResult<Vec<GenreStat>> {
        self.list_genres_impl().await
    }

    async fn list_tags(&self) -> DomainResult<Vec<MusicTagStat>> {
        self.list_tags_impl().await
    }

    async fn count_active_tags(&self, active_track_ids: &HashSet<TrackId>) -> DomainResult<u64> {
        self.count_active_tags_impl(active_track_ids).await
    }

    async fn get_tags_for_tracks(
        &self,
        track_ids: &[TrackId],
    ) -> DomainResult<HashMap<TrackId, Vec<String>>> {
        self.get_tags_for_tracks_impl(track_ids).await
    }

    async fn set_track_tags(
        &self,
        track_id: TrackId,
        tags: Vec<String>,
    ) -> DomainResult<Vec<String>> {
        self.set_track_tags_impl(track_id, tags).await
    }

    async fn search_catalog(&self, query: &SearchQuery) -> DomainResult<SearchResult> {
        self.search_catalog_impl(query).await
    }

    async fn recompute_catalog_aggregates(&self) -> DomainResult<()> {
        self.recompute_catalog_aggregates_impl().await
    }
}

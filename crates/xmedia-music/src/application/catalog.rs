mod browse;
mod media;
mod overview_tracks;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use jiff::Timestamp;
use uuid::Uuid;
use xmedia_core::application::bindings::MediaBindingService;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::domain::library::{LibraryId, MediaKind};
use xmedia_core::domain::repository::LibraryRepository;
use xmedia_core::domain::user::UserId;

use crate::application::dto::{AlbumDto, ArtistDto, OnlineTrackedTrackDto, TrackDto};
use crate::application::ports::{LyricsResolverPort, MusicStoragePort};
use crate::domain::catalog::{Album, AlbumId, Artist, Track, TrackId};
use crate::domain::collection::StarTargetKind;
use crate::domain::repository::{MusicCatalogRepository, MusicCollectionRepository};

#[derive(Debug, Clone)]
pub enum MusicDirectoryNode {
    Folder {
        id: String,
        parent: Option<String>,
        name: String,
        sub_albums: Vec<AlbumDto>,
        tracks: Vec<TrackDto>,
    },
}

#[derive(Clone)]
pub struct MusicCatalogService {
    libraries: Arc<dyn LibraryRepository>,
    catalog: Arc<dyn MusicCatalogRepository>,
    collections: Arc<dyn MusicCollectionRepository>,
    bindings: Arc<MediaBindingService>,
    storage: Arc<dyn MusicStoragePort>,
    lyrics: Arc<dyn LyricsResolverPort>,
}

impl MusicCatalogService {
    pub fn new(
        libraries: Arc<dyn LibraryRepository>,
        catalog: Arc<dyn MusicCatalogRepository>,
        collections: Arc<dyn MusicCollectionRepository>,
        bindings: Arc<MediaBindingService>,
        storage: Arc<dyn MusicStoragePort>,
        lyrics: Arc<dyn LyricsResolverPort>,
    ) -> Self {
        Self {
            libraries,
            catalog,
            collections,
            bindings,
            storage,
            lyrics,
        }
    }

    pub fn bindings(&self) -> &Arc<MediaBindingService> {
        &self.bindings
    }

    pub(crate) async fn bound_track_ids(&self, user_id: UserId) -> DomainResult<HashSet<TrackId>> {
        Ok(self
            .bindings
            .list_bound_ids(user_id, MediaKind::Music)
            .await?
            .into_iter()
            .map(TrackId)
            .collect())
    }

    pub async fn list_all_track_ids_for_backfill(&self) -> DomainResult<Vec<Uuid>> {
        Ok(self
            .catalog
            .list_all_active_track_ids()
            .await?
            .into_iter()
            .map(|id| id.0)
            .collect())
    }

    pub async fn bind_track_for_user(
        &self,
        user_id: UserId,
        track_id: TrackId,
    ) -> DomainResult<bool> {
        self.bindings
            .bind(user_id, MediaKind::Music, track_id.0)
            .await
    }

    pub async fn bind_tracks_for_user(
        &self,
        user_id: UserId,
        track_ids: &[TrackId],
    ) -> DomainResult<usize> {
        let raw_ids: Vec<Uuid> = track_ids.iter().map(|id| id.0).collect();
        self.bindings
            .bind_batch(user_id, MediaKind::Music, &raw_ids)
            .await
    }

    pub async fn require_track_bound(
        &self,
        user_id: UserId,
        track_id: TrackId,
    ) -> DomainResult<()> {
        self.bindings
            .require_bound(user_id, MediaKind::Music, track_id.0, "track not found")
            .await
    }

    pub async fn count_bound_active_tracks(&self, user_id: UserId) -> DomainResult<u64> {
        let allowed = self.bound_track_ids(user_id).await?;
        self.catalog.count_bound_active_tracks(&allowed).await
    }

    pub async fn count_bound_tracks_by_library(
        &self,
        user_id: UserId,
    ) -> DomainResult<HashMap<LibraryId, u64>> {
        let allowed = self.bound_track_ids(user_id).await?;
        self.catalog.count_bound_tracks_by_library(&allowed).await
    }

    fn enrich_tracks_with_maps(
        tracks: Vec<Track>,
        tag_map: &mut HashMap<TrackId, Vec<String>>,
        star_map: &HashMap<Uuid, Timestamp>,
        rating_map: &HashMap<Uuid, u8>,
        stat_map: &HashMap<TrackId, (u64, Timestamp)>,
    ) -> Vec<TrackDto> {
        tracks
            .into_iter()
            .map(|t| {
                let (play_count, last_played_at) = stat_map
                    .get(&t.id)
                    .map(|(c, ts)| (*c, Some(*ts)))
                    .unwrap_or((0, None));
                let online_tracking =
                    t.source
                        .online_tracking_info()
                        .map(|(source_id, item_id, cached)| OnlineTrackedTrackDto {
                            source_id,
                            item_id: item_id.to_string(),
                            cached,
                        });
                TrackDto {
                    id: t.id,
                    library_id: t.library_id,
                    album_id: t.album_id,
                    artist_id: t.artist_id,
                    album_artist_id: t.album_artist_id,
                    title: t.title.clone(),
                    artist_name: t.artist_name.clone(),
                    album_artist_name: t.album_artist_name.clone(),
                    album_title: t.album_title.clone(),
                    track_number: t.track_number,
                    disc_number: t.disc_number,
                    year: t.year,
                    genre: t.genre.clone(),
                    duration_secs: t.duration_secs,
                    bit_rate_kbps: t.bit_rate_kbps,
                    sample_rate_hz: t.sample_rate_hz,
                    bit_depth: t.bit_depth,
                    channels: t.channels,
                    format: t.format,
                    mime_type: t.format.mime_type().to_string(),
                    is_lossless: t.format.is_lossless(),
                    file_size: t.file_size,
                    replay_gain: t.replay_gain.clone(),
                    has_lyrics: t.has_lyrics(),
                    has_embedded_lyrics: t.has_embedded_lyrics,
                    has_sidecar_lyrics: t.has_sidecar_lyrics,
                    is_compilation: t.is_compilation,
                    cover_key: t.cover_key.clone(),
                    path: t.source.display_path().to_string(),
                    tags: tag_map.remove(&t.id).unwrap_or_default(),
                    starred_at: star_map.get(&t.id.0).copied(),
                    user_rating: rating_map.get(&t.id.0).copied(),
                    play_count,
                    last_played_at,
                    created_at: t.created_at,
                    online_tracking,
                }
            })
            .collect()
    }

    fn enrich_albums_with_maps(
        albums: Vec<Album>,
        star_map: &HashMap<Uuid, Timestamp>,
        rating_map: &HashMap<Uuid, u8>,
        album_play_counts: &HashMap<AlbumId, u64>,
    ) -> Vec<AlbumDto> {
        albums
            .into_iter()
            .map(|a| AlbumDto {
                id: a.id,
                library_id: a.library_id,
                title: a.title,
                album_artist_id: a.album_artist_id,
                album_artist_name: a.album_artist_name,
                year: a.year,
                genre: a.genre,
                disc_count: a.disc_count,
                track_count: a.track_count,
                duration_secs: a.duration_secs,
                is_compilation: a.is_compilation,
                cover_key: a.cover_key,
                starred_at: star_map.get(&a.id.0).copied(),
                user_rating: rating_map.get(&a.id.0).copied(),
                play_count: album_play_counts.get(&a.id).copied().unwrap_or(0),
                created_at: a.created_at,
            })
            .collect()
    }

    pub async fn enrich_tracks(
        &self,
        user_id: UserId,
        tracks: Vec<Track>,
    ) -> DomainResult<Vec<TrackDto>> {
        if tracks.is_empty() {
            return Ok(Vec::new());
        }
        let track_ids: Vec<TrackId> = tracks.iter().map(|t| t.id).collect();
        let mut tag_map = self.catalog.get_tags_for_tracks(&track_ids).await?;
        let stars = self
            .collections
            .list_starred(user_id, Some(StarTargetKind::Track))
            .await?;
        let ratings = self.collections.list_user_ratings(user_id).await?;
        let stats = self.collections.list_user_play_stats(user_id).await?;

        let star_map: HashMap<Uuid, Timestamp> = stars
            .into_iter()
            .map(|s| (s.target_id, s.starred_at))
            .collect();
        let rating_map: HashMap<Uuid, u8> = ratings
            .into_iter()
            .filter(|r| r.kind == StarTargetKind::Track)
            .map(|r| (r.target_id, r.rating))
            .collect();
        let stat_map: HashMap<TrackId, (u64, Timestamp)> = stats
            .into_iter()
            .map(|st| (st.track_id, (st.play_count, st.last_played_at)))
            .collect();

        Ok(Self::enrich_tracks_with_maps(
            tracks,
            &mut tag_map,
            &star_map,
            &rating_map,
            &stat_map,
        ))
    }

    pub async fn enrich_albums(
        &self,
        user_id: UserId,
        albums: Vec<Album>,
    ) -> DomainResult<Vec<AlbumDto>> {
        if albums.is_empty() {
            return Ok(Vec::new());
        }
        let stars = self
            .collections
            .list_starred(user_id, Some(StarTargetKind::Album))
            .await?;
        let ratings = self.collections.list_user_ratings(user_id).await?;
        let stats = self.collections.list_user_play_stats(user_id).await?;

        let star_map: HashMap<Uuid, Timestamp> = stars
            .into_iter()
            .map(|s| (s.target_id, s.starred_at))
            .collect();
        let rating_map: HashMap<Uuid, u8> = ratings
            .into_iter()
            .filter(|r| r.kind == StarTargetKind::Album)
            .map(|r| (r.target_id, r.rating))
            .collect();
        let mut album_play_counts: HashMap<AlbumId, u64> = HashMap::new();
        for st in stats {
            *album_play_counts.entry(st.album_id).or_default() += st.play_count;
        }

        Ok(Self::enrich_albums_with_maps(
            albums,
            &star_map,
            &rating_map,
            &album_play_counts,
        ))
    }

    pub async fn enrich_artists(
        &self,
        user_id: UserId,
        artists: Vec<Artist>,
    ) -> DomainResult<Vec<ArtistDto>> {
        if artists.is_empty() {
            return Ok(Vec::new());
        }
        let stars = self
            .collections
            .list_starred(user_id, Some(StarTargetKind::Artist))
            .await?;
        let ratings = self.collections.list_user_ratings(user_id).await?;

        let star_map: HashMap<Uuid, Timestamp> = stars
            .into_iter()
            .map(|s| (s.target_id, s.starred_at))
            .collect();
        let rating_map: HashMap<Uuid, u8> = ratings
            .into_iter()
            .filter(|r| r.kind == StarTargetKind::Artist)
            .map(|r| (r.target_id, r.rating))
            .collect();

        Ok(artists
            .into_iter()
            .map(|ar| ArtistDto {
                id: ar.id,
                name: ar.name,
                is_album_artist: ar.is_album_artist,
                is_track_artist: ar.is_track_artist,
                album_count: ar.album_count,
                track_count: ar.track_count,
                cover_key: ar.cover_key,
                starred_at: star_map.get(&ar.id.0).copied(),
                user_rating: rating_map.get(&ar.id.0).copied(),
                created_at: ar.created_at,
            })
            .collect())
    }
}

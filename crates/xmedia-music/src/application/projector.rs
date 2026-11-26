use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use jiff::Timestamp;
use uuid::Uuid;
use xmedia_core::application::bindings::MediaBindingService;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::domain::library::MediaKind;
use xmedia_core::domain::user::UserId;

use crate::application::dto::{AlbumDto, ArtistDto, OnlineTrackedTrackDto, TrackDto};
use crate::domain::catalog::{Album, AlbumId, Artist, Track, TrackId, TrackSource};
use crate::domain::collection::StarTargetKind;
use crate::domain::repository::{
    CatalogFacetQuery, CatalogFacetResult, MusicCatalogRepository, MusicCollectionRepository,
};

#[derive(Clone)]
pub(crate) struct MusicProjector {
    catalog: Arc<dyn MusicCatalogRepository>,
    collections: Arc<dyn MusicCollectionRepository>,
    bindings: Arc<MediaBindingService>,
}

impl MusicProjector {
    pub(crate) fn new(
        catalog: Arc<dyn MusicCatalogRepository>,
        collections: Arc<dyn MusicCollectionRepository>,
        bindings: Arc<MediaBindingService>,
    ) -> Self {
        Self {
            catalog,
            collections,
            bindings,
        }
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

    pub(crate) async fn load_track_tags(
        &self,
        track_ids: &[TrackId],
    ) -> DomainResult<HashMap<TrackId, Vec<String>>> {
        match self
            .catalog
            .query_facets(CatalogFacetQuery::TrackTags(track_ids))
            .await?
        {
            CatalogFacetResult::TrackTags(map) => Ok(map),
            _ => Ok(HashMap::new()),
        }
    }

    pub(crate) fn enrich_tracks_with_maps(
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
                let online_tracking = match &t.source {
                    TrackSource::OnlineTracked(info) => Some(OnlineTrackedTrackDto {
                        source_id: info.source_id,
                        item_id: info.item_id.clone(),
                        cached: info.cached && info.storage_key.is_some(),
                    }),
                    TrackSource::Managed { .. } | TrackSource::LocalPath { .. } => None,
                };
                let path = t.source.display_path().to_string();
                let has_lyrics = t.has_lyrics();
                TrackDto {
                    id: t.id,
                    library_id: t.library_id,
                    album_id: t.album_id,
                    artist_id: t.artist_id,
                    album_artist_id: t.album_artist_id,
                    title: t.title,
                    artist_name: t.artist_name,
                    album_artist_name: t.album_artist_name,
                    album_title: t.album_title,
                    track_number: t.track_number,
                    disc_number: t.disc_number,
                    year: t.year,
                    genre: t.genre,
                    duration_secs: t.duration_secs,
                    bit_rate_kbps: t.bit_rate_kbps,
                    sample_rate_hz: t.sample_rate_hz,
                    bit_depth: t.bit_depth,
                    channels: t.channels,
                    format: t.format,
                    mime_type: t.format.mime_type().to_string(),
                    is_lossless: t.format.is_lossless(),
                    file_size: t.file_size,
                    replay_gain: t.replay_gain,
                    has_lyrics,
                    has_embedded_lyrics: t.has_embedded_lyrics,
                    has_sidecar_lyrics: t.has_sidecar_lyrics,
                    is_compilation: t.is_compilation,
                    cover_key: t.cover_key,
                    path,
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

    pub(crate) fn enrich_albums_with_maps(
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

    pub(crate) async fn enrich_tracks(
        &self,
        user_id: UserId,
        tracks: Vec<Track>,
    ) -> DomainResult<Vec<TrackDto>> {
        if tracks.is_empty() {
            return Ok(Vec::new());
        }
        let track_ids: Vec<TrackId> = tracks.iter().map(|t| t.id).collect();
        let mut tag_map = self.load_track_tags(&track_ids).await?;
        let engagement = self
            .collections
            .load_engagement(user_id, Some(StarTargetKind::Track))
            .await?;
        let star_map = engagement.star_map(StarTargetKind::Track);
        let rating_map = engagement.rating_map(StarTargetKind::Track);
        let (stat_map, _) = engagement.play_maps(None);

        Ok(Self::enrich_tracks_with_maps(
            tracks,
            &mut tag_map,
            &star_map,
            &rating_map,
            &stat_map,
        ))
    }

    pub(crate) async fn enrich_albums(
        &self,
        user_id: UserId,
        albums: Vec<Album>,
    ) -> DomainResult<Vec<AlbumDto>> {
        if albums.is_empty() {
            return Ok(Vec::new());
        }
        let engagement = self
            .collections
            .load_engagement(user_id, Some(StarTargetKind::Album))
            .await?;
        let star_map = engagement.star_map(StarTargetKind::Album);
        let rating_map = engagement.rating_map(StarTargetKind::Album);
        let (_, album_play_counts) = engagement.play_maps(None);

        Ok(Self::enrich_albums_with_maps(
            albums,
            &star_map,
            &rating_map,
            &album_play_counts,
        ))
    }

    pub(crate) async fn enrich_artists(
        &self,
        user_id: UserId,
        artists: Vec<Artist>,
    ) -> DomainResult<Vec<ArtistDto>> {
        if artists.is_empty() {
            return Ok(Vec::new());
        }
        let engagement = self
            .collections
            .load_engagement(user_id, Some(StarTargetKind::Artist))
            .await?;
        let star_map = engagement.star_map(StarTargetKind::Artist);
        let rating_map = engagement.rating_map(StarTargetKind::Artist);

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

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
use crate::domain::catalog::{
    Album, AlbumId, Artist, ArtistId, SortOrder, Track, TrackId, TrackSortField,
};
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

    pub async fn load_bound_tracks(&self, user_id: UserId) -> DomainResult<Vec<Track>> {
        let bound_uuids = self
            .bindings
            .list_bound_ids(user_id, MediaKind::Music)
            .await?;
        if bound_uuids.is_empty() {
            return Ok(Vec::new());
        }
        let ids: Vec<TrackId> = bound_uuids.into_iter().map(TrackId).collect();
        let tracks = self.catalog.find_tracks_by_ids(&ids).await?;
        Ok(tracks.into_iter().filter(|t| !t.is_missing).collect())
    }

    pub async fn count_bound_tracks_by_library(
        &self,
        user_id: UserId,
    ) -> DomainResult<HashMap<LibraryId, u64>> {
        let bound_tracks = self.load_bound_tracks(user_id).await?;
        let mut counts: HashMap<LibraryId, u64> = HashMap::new();
        for tr in bound_tracks {
            *counts.entry(tr.library_id).or_insert(0) += 1;
        }
        Ok(counts)
    }

    pub(crate) async fn project_user_albums(
        &self,
        bound_tracks: &[Track],
    ) -> DomainResult<Vec<Album>> {
        if bound_tracks.is_empty() {
            return Ok(Vec::new());
        }
        let mut stats: HashMap<AlbumId, (u32, u32, u32)> = HashMap::new();
        for tr in bound_tracks {
            let entry = stats.entry(tr.album_id).or_insert((0, 0, 1));
            entry.0 = entry.0.saturating_add(1);
            entry.1 = entry.1.saturating_add(tr.duration_secs);
            if let Some(disc) = tr.disc_number
                && disc > entry.2
            {
                entry.2 = disc;
            }
        }
        let album_ids: Vec<AlbumId> = stats.keys().copied().collect();
        let albums = self.catalog.find_albums_by_ids(&album_ids).await?;
        let mut out = Vec::with_capacity(albums.len());
        for mut alb in albums {
            if let Some(&(track_count, duration_secs, disc_count)) = stats.get(&alb.id) {
                alb.track_count = track_count;
                alb.duration_secs = duration_secs;
                alb.disc_count = disc_count;
                out.push(alb);
            }
        }
        Ok(out)
    }

    pub(crate) async fn project_user_artists(
        &self,
        bound_tracks: &[Track],
    ) -> DomainResult<Vec<Artist>> {
        if bound_tracks.is_empty() {
            return Ok(Vec::new());
        }
        let mut track_counts: HashMap<ArtistId, HashSet<TrackId>> = HashMap::new();
        let mut album_sets: HashMap<ArtistId, HashSet<AlbumId>> = HashMap::new();
        let mut as_track_artist: HashSet<ArtistId> = HashSet::new();
        let mut as_album_artist: HashSet<ArtistId> = HashSet::new();

        for tr in bound_tracks {
            as_track_artist.insert(tr.artist_id);
            as_album_artist.insert(tr.album_artist_id);
            track_counts.entry(tr.artist_id).or_default().insert(tr.id);
            track_counts
                .entry(tr.album_artist_id)
                .or_default()
                .insert(tr.id);
            album_sets
                .entry(tr.album_artist_id)
                .or_default()
                .insert(tr.album_id);
        }

        let artist_ids: Vec<ArtistId> = track_counts.keys().copied().collect();
        let artists = self.catalog.find_artists_by_ids(&artist_ids).await?;
        let mut out = Vec::with_capacity(artists.len());
        for mut ar in artists {
            let t_cnt = track_counts.get(&ar.id).map_or(0, HashSet::len) as u32;
            let a_cnt = album_sets.get(&ar.id).map_or(0, HashSet::len) as u32;
            if t_cnt == 0 && a_cnt == 0 {
                continue;
            }
            ar.track_count = t_cnt;
            ar.album_count = a_cnt;
            ar.is_track_artist = as_track_artist.contains(&ar.id);
            ar.is_album_artist = as_album_artist.contains(&ar.id);
            out.push(ar);
        }
        Ok(out)
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

fn sort_album_tracks(tracks: &mut [Track]) {
    tracks.sort_by(|a, b| {
        a.disc_number
            .unwrap_or(1)
            .cmp(&b.disc_number.unwrap_or(1))
            .then_with(|| {
                a.track_number
                    .unwrap_or(u32::MAX)
                    .cmp(&b.track_number.unwrap_or(u32::MAX))
            })
            .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
    });
}

fn sort_tracks(tracks: &mut [Track], sort_by: TrackSortField, sort_order: SortOrder) {
    tracks.sort_by(|a, b| {
        let ord = match sort_by {
            TrackSortField::Title => a.title.to_lowercase().cmp(&b.title.to_lowercase()),
            TrackSortField::Artist => a
                .artist_name
                .to_lowercase()
                .cmp(&b.artist_name.to_lowercase()),
            TrackSortField::Album => a
                .album_title
                .to_lowercase()
                .cmp(&b.album_title.to_lowercase()),
            TrackSortField::CreatedAt => a.created_at.cmp(&b.created_at),
            TrackSortField::Year => a.year.cmp(&b.year),
            TrackSortField::Duration => a.duration_secs.cmp(&b.duration_secs),
            TrackSortField::TrackOrder => a
                .disc_number
                .unwrap_or(1)
                .cmp(&b.disc_number.unwrap_or(1))
                .then_with(|| {
                    a.track_number
                        .unwrap_or(u32::MAX)
                        .cmp(&b.track_number.unwrap_or(u32::MAX))
                })
                .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase())),
        };
        match sort_order {
            SortOrder::Asc => ord,
            SortOrder::Desc => ord.reverse(),
        }
    });
}

fn shuffle_prefix<T>(slice: &mut [T], count: usize) {
    let n = slice.len();
    let k = count.min(n);
    if k <= 1 {
        return;
    }
    let mut rng_state = (Uuid::now_v7().as_u128() as u64)
        ^ (Timestamp::now().as_nanosecond() as u64)
        ^ 0x9E37_79B9_7F4A_7C15;
    for i in 0..k {
        rng_state = rng_state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = rng_state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z = z ^ (z >> 31);
        let j = i + ((z as usize) % (n - i));
        slice.swap(i, j);
    }
}

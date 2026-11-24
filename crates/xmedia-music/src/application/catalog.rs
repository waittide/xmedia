use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use jiff::Timestamp;
use uuid::Uuid;
use xmedia_core::application::bindings::MediaBindingService;
use xmedia_core::application::ports::BinaryPayload;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::{LibraryId, MediaKind};
use xmedia_core::domain::repository::LibraryRepository;
use xmedia_core::domain::user::{User, UserId, require_admin};

use crate::application::dto::{
    AlbumDetailDto, AlbumDto, ArtistDetailDto, ArtistDto, BookmarkDto, CatalogStatsDto,
    HomeOverviewDto, OnlineTrackedTrackDto, PlayQueueDto, PoolTrackDto, SearchResultDto, TrackDto,
};
use crate::application::ports::{LyricsResolverPort, MusicStoragePort};
use crate::domain::catalog::{
    Album, AlbumId, AlbumListQuery, AlbumListType, Artist, ArtistId, ArtistQuery, ArtistRoleFilter,
    GenreStat, MusicTagStat, PagedResult, SearchQuery, SortOrder, StructuredLyrics, Track, TrackId,
    TrackQuery, TrackSortField,
};
use crate::domain::collection::{PlaylistId, StarTargetKind};
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

    pub async fn list_recent_played_tracks(
        &self,
        user_id: UserId,
        limit: u32,
    ) -> DomainResult<Vec<TrackDto>> {
        let bound_ids = self
            .bindings
            .list_bound_ids(user_id, MediaKind::Music)
            .await?;
        if bound_ids.is_empty() {
            return Ok(Vec::new());
        }
        let clamped = limit.clamp(1, 100) as usize;
        let scrobbles = self
            .collections
            .list_recent_scrobbles(user_id, (clamped * 3).min(300) as u32)
            .await?;
        let mut seen_track_ids = Vec::new();
        for sc in scrobbles {
            if !bound_ids.contains(&sc.track_id.0) {
                continue;
            }
            if !seen_track_ids.contains(&sc.track_id) {
                seen_track_ids.push(sc.track_id);
            }
            if seen_track_ids.len() >= clamped {
                break;
            }
        }
        if seen_track_ids.is_empty() {
            return Ok(Vec::new());
        }
        let fetched_recent = self.catalog.find_tracks_by_ids(&seen_track_ids).await?;
        let track_lookup: HashMap<TrackId, Track> =
            fetched_recent.into_iter().map(|t| (t.id, t)).collect();
        let ordered_recent: Vec<Track> = seen_track_ids
            .iter()
            .filter_map(|id| track_lookup.get(id).cloned())
            .filter(|t| !t.is_missing)
            .collect();
        self.enrich_tracks(user_id, ordered_recent).await
    }

    pub async fn get_home_overview(&self, user_id: UserId) -> DomainResult<HomeOverviewDto> {
        let mut all_active_tracks = self.load_bound_tracks(user_id).await?;
        let total_tracks = all_active_tracks.len() as u64;

        let mut active_track_ids: HashSet<TrackId> =
            HashSet::with_capacity(all_active_tracks.len());
        let mut active_library_ids: HashSet<LibraryId> = HashSet::new();
        let mut genre_set: HashSet<String> = HashSet::new();
        let mut track_idx: HashMap<TrackId, usize> =
            HashMap::with_capacity(all_active_tracks.len());
        for (idx, tr) in all_active_tracks.iter().enumerate() {
            active_track_ids.insert(tr.id);
            active_library_ids.insert(tr.library_id);
            track_idx.insert(tr.id, idx);
            if let Some(ref g) = tr.genre {
                let clean = g.trim().to_lowercase();
                if !clean.is_empty() {
                    genre_set.insert(clean);
                }
            }
        }
        let genres_count = genre_set.len() as u64;

        let mut user_albums = self.project_user_albums(&all_active_tracks).await?;
        user_albums.sort_by_key(|a| std::cmp::Reverse(a.created_at));
        let total_albums = user_albums.len() as u64;
        let active_album_ids: HashSet<AlbumId> = user_albums.iter().map(|a| a.id).collect();
        let user_artists = self.project_user_artists(&all_active_tracks).await?;
        let total_artists = user_artists.len() as u64;

        let all_stars = self.collections.list_starred(user_id, None).await?;
        let mut track_star_map: HashMap<Uuid, Timestamp> = HashMap::new();
        let mut album_star_map: HashMap<Uuid, Timestamp> = HashMap::new();
        for s in all_stars {
            match s.kind {
                StarTargetKind::Track if active_track_ids.contains(&TrackId(s.target_id)) => {
                    track_star_map.insert(s.target_id, s.starred_at);
                }
                StarTargetKind::Album if active_album_ids.contains(&AlbumId(s.target_id)) => {
                    album_star_map.insert(s.target_id, s.starred_at);
                }
                _ => {}
            }
        }
        let starred_tracks = track_star_map.len() as u64;

        let all_ratings = self.collections.list_user_ratings(user_id).await?;
        let mut track_rating_map: HashMap<Uuid, u8> = HashMap::new();
        let mut album_rating_map: HashMap<Uuid, u8> = HashMap::new();
        for r in all_ratings {
            match r.kind {
                StarTargetKind::Track if active_track_ids.contains(&TrackId(r.target_id)) => {
                    track_rating_map.insert(r.target_id, r.rating);
                }
                StarTargetKind::Album if active_album_ids.contains(&AlbumId(r.target_id)) => {
                    album_rating_map.insert(r.target_id, r.rating);
                }
                _ => {}
            }
        }

        let all_stats = self.collections.list_user_play_stats(user_id).await?;
        let mut track_stat_map: HashMap<TrackId, (u64, Timestamp)> =
            HashMap::with_capacity(all_stats.len());
        let mut album_play_counts: HashMap<AlbumId, u64> = HashMap::new();
        for st in all_stats {
            if !active_track_ids.contains(&st.track_id) {
                continue;
            }
            *album_play_counts.entry(st.album_id).or_default() += st.play_count;
            track_stat_map.insert(st.track_id, (st.play_count, st.last_played_at));
        }

        let recent_added_raw: Vec<Album> = user_albums.iter().take(12).cloned().collect();
        let album_by_id: HashMap<AlbumId, Album> =
            user_albums.into_iter().map(|a| (a.id, a)).collect();

        let mut freq_pairs: Vec<(AlbumId, u64)> = album_play_counts
            .iter()
            .map(|(&id, &cnt)| (id, cnt))
            .collect();
        freq_pairs.sort_by_key(|a| std::cmp::Reverse(a.1));
        let frequent_raw: Vec<Album> = freq_pairs
            .into_iter()
            .filter_map(|(id, _)| album_by_id.get(&id).cloned())
            .take(12)
            .collect();

        let recent_added = Self::enrich_albums_with_maps(
            recent_added_raw,
            &album_star_map,
            &album_rating_map,
            &album_play_counts,
        );
        let frequent = Self::enrich_albums_with_maps(
            frequent_raw,
            &album_star_map,
            &album_rating_map,
            &album_play_counts,
        );

        let scrobbles = self.collections.list_recent_scrobbles(user_id, 30).await?;
        let mut recent_ids: Vec<TrackId> = Vec::new();
        for sc in scrobbles {
            if !active_track_ids.contains(&sc.track_id) {
                continue;
            }
            if !recent_ids.contains(&sc.track_id) {
                recent_ids.push(sc.track_id);
            }
            if recent_ids.len() >= 10 {
                break;
            }
        }
        let raw_bookmarks: Vec<_> = self
            .collections
            .list_bookmarks(user_id)
            .await?
            .into_iter()
            .filter(|bm| active_track_ids.contains(&bm.track_id))
            .collect();
        let bookmarks_count = raw_bookmarks.len() as u64;
        let raw_queue = self.collections.get_play_queue(user_id).await?;

        let mut track_pool: HashMap<TrackId, Track> = HashMap::new();
        for id in &recent_ids {
            if let Some(&idx) = track_idx.get(id) {
                track_pool
                    .entry(*id)
                    .or_insert_with(|| all_active_tracks[idx].clone());
            }
        }
        for bm in &raw_bookmarks {
            if let Some(&idx) = track_idx.get(&bm.track_id) {
                track_pool
                    .entry(bm.track_id)
                    .or_insert_with(|| all_active_tracks[idx].clone());
            }
        }
        if let Some(ref q) = raw_queue
            && let Some(cid) = q.current_track_id
            && let Some(&idx) = track_idx.get(&cid)
        {
            track_pool
                .entry(cid)
                .or_insert_with(|| all_active_tracks[idx].clone());
        }

        let sample_count = all_active_tracks.len().min(12);
        shuffle_prefix(&mut all_active_tracks, sample_count);
        let random_ids: Vec<TrackId> = all_active_tracks
            .iter()
            .take(sample_count)
            .map(|t| t.id)
            .collect();
        for tr in all_active_tracks.into_iter().take(sample_count) {
            track_pool.entry(tr.id).or_insert(tr);
        }

        let needed_ids: Vec<TrackId> = track_pool.keys().copied().collect();
        let mut tag_map = self.catalog.get_tags_for_tracks(&needed_ids).await?;
        let all_dtos = Self::enrich_tracks_with_maps(
            track_pool.into_values().collect(),
            &mut tag_map,
            &track_star_map,
            &track_rating_map,
            &track_stat_map,
        );
        let dto_lookup: HashMap<TrackId, TrackDto> =
            all_dtos.into_iter().map(|t| (t.id, t)).collect();

        let recent_played_tracks: Vec<TrackDto> = recent_ids
            .iter()
            .filter_map(|id| dto_lookup.get(id).cloned())
            .collect();
        let random_tracks: Vec<TrackDto> = random_ids
            .iter()
            .filter_map(|id| dto_lookup.get(id).cloned())
            .collect();

        let mut bookmarks = Vec::new();
        for bm in raw_bookmarks {
            if let Some(track) = dto_lookup.get(&bm.track_id).cloned() {
                bookmarks.push(BookmarkDto {
                    track,
                    position_ms: bm.position_ms,
                    comment: bm.comment,
                    created_at: bm.created_at,
                    updated_at: bm.updated_at,
                });
            }
        }
        bookmarks.sort_by_key(|b| std::cmp::Reverse(b.updated_at));

        let play_queue = raw_queue.and_then(|q| {
            let current_track_id = q
                .current_track_id
                .filter(|cid| active_track_ids.contains(cid));
            let current_tracks: Vec<TrackDto> = current_track_id
                .and_then(|cid| dto_lookup.get(&cid).cloned())
                .into_iter()
                .collect();
            if current_track_id.is_none()
                && !q.track_ids.iter().any(|tid| active_track_ids.contains(tid))
            {
                return None;
            }
            Some(PlayQueueDto {
                tracks: current_tracks,
                current_track_id,
                position_ms: q.position_ms,
                changed_by: q.changed_by,
                updated_at: q.updated_at,
            })
        });

        let playlists_count = self
            .collections
            .list_visible_playlists(user_id)
            .await?
            .len() as u64;
        let tags_count = self.catalog.count_active_tags(&active_track_ids).await?;
        let libraries_count = active_library_ids.len() as u64;

        let mut listening_ids: HashSet<TrackId> = bookmarks.iter().map(|b| b.track.id).collect();
        if let Some(ref pq) = play_queue
            && let Some(cid) = pq.current_track_id
        {
            listening_ids.insert(cid);
        }
        for rt in &recent_played_tracks {
            listening_ids.insert(rt.id);
        }
        let listening_count = listening_ids.len() as u64;

        Ok(HomeOverviewDto {
            stats: CatalogStatsDto {
                total_tracks,
                total_albums,
                total_artists,
                starred_tracks,
                playlists_count,
                listening_count,
                bookmarks_count,
                genres_count,
                tags_count,
                libraries_count,
            },
            recent_added_albums: recent_added,
            frequent_albums: frequent,
            recent_played_tracks,
            random_tracks,
            bookmarks,
            play_queue,
        })
    }

    pub async fn query_tracks(
        &self,
        user_id: UserId,
        query: TrackQuery,
    ) -> DomainResult<PagedResult<TrackDto>> {
        let tracks = self.load_bound_tracks(user_id).await?;
        let tag_filter = query
            .tag
            .as_ref()
            .map(|t| t.trim().to_lowercase())
            .filter(|t| !t.is_empty());
        let tag_map_for_filter = if tag_filter.is_some() {
            let ids: Vec<TrackId> = tracks.iter().map(|t| t.id).collect();
            Some(self.catalog.get_tags_for_tracks(&ids).await?)
        } else {
            None
        };
        let genre_lower = query
            .genre
            .as_ref()
            .map(|g| g.trim().to_lowercase())
            .filter(|g| !g.is_empty());
        let kw_lower = query
            .search
            .as_ref()
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty());

        let mut matched: Vec<Track> = tracks
            .into_iter()
            .filter(|t| {
                if let Some(lib_id) = query.library_id
                    && t.library_id != lib_id
                {
                    return false;
                }
                if let Some(alb_id) = query.album_id
                    && t.album_id != alb_id
                {
                    return false;
                }
                if let Some(ar_id) = query.artist_id
                    && t.artist_id != ar_id
                    && t.album_artist_id != ar_id
                {
                    return false;
                }
                if let (Some(tf), Some(tmap)) = (&tag_filter, &tag_map_for_filter)
                    && !tmap
                        .get(&t.id)
                        .is_some_and(|list| list.iter().any(|x| x.to_lowercase() == *tf))
                {
                    return false;
                }
                if let Some(ref g) = genre_lower
                    && !t
                        .genre
                        .as_ref()
                        .is_some_and(|tg| tg.trim().to_lowercase() == *g)
                {
                    return false;
                }
                if let Some(fy) = query.year_from
                    && t.year.is_none_or(|y| y < fy)
                {
                    return false;
                }
                if let Some(ty) = query.year_to
                    && t.year.is_none_or(|y| y > ty)
                {
                    return false;
                }
                if let Some(ref kw) = kw_lower {
                    let hit = t.title.to_lowercase().contains(kw)
                        || t.artist_name.to_lowercase().contains(kw)
                        || t.album_title.to_lowercase().contains(kw);
                    if !hit {
                        return false;
                    }
                }
                true
            })
            .collect();

        sort_tracks(&mut matched, query.sort_by, query.sort_order);

        let total = matched.len() as u64;
        let page = query.page.max(1);
        let page_size = query.page_size.max(1);
        let start = ((page - 1) as usize).saturating_mul(page_size as usize);
        let page_items: Vec<Track> = matched
            .into_iter()
            .skip(start)
            .take(page_size as usize)
            .collect();
        let items = self.enrich_tracks(user_id, page_items).await?;
        Ok(PagedResult {
            items,
            total,
            page,
            page_size,
        })
    }

    pub async fn list_pool_tracks(
        &self,
        user_id: UserId,
        mut query: TrackQuery,
    ) -> DomainResult<PagedResult<PoolTrackDto>> {
        query.exclude_online_tracked = true;
        let bound_ids = self
            .bindings
            .list_bound_ids(user_id, MediaKind::Music)
            .await?;
        let paged = self.catalog.query_tracks(&query).await?;
        let track_dtos = self.enrich_tracks(user_id, paged.items).await?;
        let items = track_dtos
            .into_iter()
            .map(|track| {
                let bound = bound_ids.contains(&track.id.0);
                PoolTrackDto { track, bound }
            })
            .collect();
        Ok(PagedResult {
            items,
            total: paged.total,
            page: paged.page,
            page_size: paged.page_size,
        })
    }

    pub async fn get_track(&self, user_id: UserId, track_id: TrackId) -> DomainResult<TrackDto> {
        self.require_track_bound(user_id, track_id).await?;
        let track = self
            .catalog
            .find_track_by_id(track_id)
            .await?
            .filter(|t| !t.is_missing)
            .ok_or_else(|| DomainError::NotFound("track not found".into()))?;
        let mut list = self.enrich_tracks(user_id, vec![track]).await?;
        list.pop()
            .ok_or_else(|| DomainError::NotFound("track not found".into()))
    }

    pub async fn list_albums(
        &self,
        user_id: UserId,
        query: AlbumListQuery,
    ) -> DomainResult<PagedResult<AlbumDto>> {
        let bound_tracks = self.load_bound_tracks(user_id).await?;
        let active_track_ids: HashSet<TrackId> = bound_tracks.iter().map(|t| t.id).collect();
        let mut pool = self.project_user_albums(&bound_tracks).await?;

        let kw_lower = query
            .search
            .as_ref()
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty());
        let genre_lower = query
            .genre
            .as_ref()
            .map(|g| g.trim().to_lowercase())
            .filter(|g| !g.is_empty());
        let (year_min, year_max) = match (query.from_year, query.to_year) {
            (Some(f), Some(t)) => (Some(f.min(t)), Some(f.max(t))),
            (f, t) => (f, t),
        };

        pool.retain(|a| {
            if let Some(lib_id) = query.library_id
                && a.library_id != lib_id
            {
                return false;
            }
            if let Some(ref g) = genre_lower
                && !a
                    .genre
                    .as_ref()
                    .is_some_and(|ag| ag.trim().to_lowercase() == *g)
            {
                return false;
            }
            if let Some(min_y) = year_min
                && a.year.is_none_or(|y| y < min_y)
            {
                return false;
            }
            if let Some(max_y) = year_max
                && a.year.is_none_or(|y| y > max_y)
            {
                return false;
            }
            if let Some(ref kw) = kw_lower {
                let hit = a.title.to_lowercase().contains(kw)
                    || a.album_artist_name.to_lowercase().contains(kw);
                if !hit {
                    return false;
                }
            }
            true
        });

        match query.list_type {
            AlbumListType::Starred => {
                let mut stars = self
                    .collections
                    .list_starred(user_id, Some(StarTargetKind::Album))
                    .await?;
                stars.sort_by_key(|a| std::cmp::Reverse(a.starred_at));
                let lookup: HashMap<AlbumId, Album> = pool.into_iter().map(|a| (a.id, a)).collect();
                pool = stars
                    .into_iter()
                    .filter_map(|s| lookup.get(&AlbumId(s.target_id)).cloned())
                    .collect();
            }
            AlbumListType::Frequent => {
                let stats = self.collections.list_user_play_stats(user_id).await?;
                let mut agg: HashMap<AlbumId, u64> = HashMap::new();
                for st in stats {
                    if active_track_ids.contains(&st.track_id) {
                        *agg.entry(st.album_id).or_default() += st.play_count;
                    }
                }
                let mut pairs: Vec<(AlbumId, u64)> = agg.into_iter().collect();
                pairs.sort_by_key(|a| std::cmp::Reverse(a.1));
                let lookup: HashMap<AlbumId, Album> = pool.into_iter().map(|a| (a.id, a)).collect();
                pool = pairs
                    .into_iter()
                    .filter_map(|(id, _)| lookup.get(&id).cloned())
                    .collect();
            }
            AlbumListType::Recent => {
                let stats = self.collections.list_user_play_stats(user_id).await?;
                let mut agg: HashMap<AlbumId, Timestamp> = HashMap::new();
                for st in stats {
                    if !active_track_ids.contains(&st.track_id) {
                        continue;
                    }
                    agg.entry(st.album_id)
                        .and_modify(|ts| {
                            if st.last_played_at > *ts {
                                *ts = st.last_played_at;
                            }
                        })
                        .or_insert(st.last_played_at);
                }
                let mut pairs: Vec<(AlbumId, Timestamp)> = agg.into_iter().collect();
                pairs.sort_by_key(|a| std::cmp::Reverse(a.1));
                let lookup: HashMap<AlbumId, Album> = pool.into_iter().map(|a| (a.id, a)).collect();
                pool = pairs
                    .into_iter()
                    .filter_map(|(id, _)| lookup.get(&id).cloned())
                    .collect();
            }
            AlbumListType::Newest => {
                pool.sort_by_key(|a| std::cmp::Reverse(a.created_at));
            }
            AlbumListType::AlphabeticalByName | AlbumListType::ByGenre => {
                pool.sort_by_key(|a| a.title.to_lowercase());
            }
            AlbumListType::AlphabeticalByArtist => {
                pool.sort_by(|a, b| {
                    a.album_artist_name
                        .to_lowercase()
                        .cmp(&b.album_artist_name.to_lowercase())
                        .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
                });
            }
            AlbumListType::ByYear => {
                let rev = query
                    .from_year
                    .zip(query.to_year)
                    .is_some_and(|(f, t)| f > t);
                pool.sort_by(|a, b| {
                    let ord = a
                        .year
                        .cmp(&b.year)
                        .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
                    if rev { ord.reverse() } else { ord }
                });
            }
            AlbumListType::Random => {
                let len = pool.len();
                shuffle_prefix(&mut pool, len);
            }
        }

        let total = pool.len() as u64;
        let size = query.size.max(1);
        let page_items: Vec<Album> = pool
            .into_iter()
            .skip(query.offset as usize)
            .take(size as usize)
            .collect();
        let page = (query.offset / size) + 1;
        let items = self.enrich_albums(user_id, page_items).await?;
        Ok(PagedResult {
            items,
            total,
            page,
            page_size: size,
        })
    }

    pub async fn get_album_detail(
        &self,
        user_id: UserId,
        album_id: AlbumId,
    ) -> DomainResult<AlbumDetailDto> {
        let bound_tracks = self.load_bound_tracks(user_id).await?;
        let mut tracks: Vec<Track> = bound_tracks
            .into_iter()
            .filter(|t| t.album_id == album_id)
            .collect();
        if tracks.is_empty() {
            return Err(DomainError::NotFound("album not found".into()));
        }
        sort_album_tracks(&mut tracks);
        let projected = self.project_user_albums(&tracks).await?;
        let album = projected
            .into_iter()
            .find(|a| a.id == album_id)
            .ok_or_else(|| DomainError::NotFound("album not found".into()))?;
        let album_dto = self
            .enrich_albums(user_id, vec![album])
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound("album not found".into()))?;
        let track_dtos = self.enrich_tracks(user_id, tracks).await?;
        Ok(AlbumDetailDto {
            album: album_dto,
            tracks: track_dtos,
        })
    }

    pub async fn list_artists(
        &self,
        user_id: UserId,
        query: ArtistQuery,
    ) -> DomainResult<PagedResult<ArtistDto>> {
        let mut bound_tracks = self.load_bound_tracks(user_id).await?;
        if let Some(lib_id) = query.library_id {
            bound_tracks.retain(|t| t.library_id == lib_id);
        }
        let mut artists = self.project_user_artists(&bound_tracks).await?;
        let kw_lower = query
            .search
            .as_ref()
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty());

        artists.retain(|a| {
            match query.role_filter {
                ArtistRoleFilter::All => {}
                ArtistRoleFilter::AlbumArtist => {
                    if !a.is_album_artist {
                        return false;
                    }
                }
                ArtistRoleFilter::TrackArtist => {
                    if !a.is_track_artist {
                        return false;
                    }
                }
            }
            if let Some(ref kw) = kw_lower
                && !a.name.to_lowercase().contains(kw)
            {
                return false;
            }
            true
        });

        artists.sort_by_key(|a| a.name.to_lowercase());
        let total = artists.len() as u64;
        let size = query.size.max(1);
        let page_items: Vec<Artist> = artists
            .into_iter()
            .skip(query.offset as usize)
            .take(size as usize)
            .collect();
        let page = (query.offset / size) + 1;
        let items = self.enrich_artists(user_id, page_items).await?;
        Ok(PagedResult {
            items,
            total,
            page,
            page_size: size,
        })
    }

    pub async fn get_artist_detail(
        &self,
        user_id: UserId,
        artist_id: ArtistId,
    ) -> DomainResult<ArtistDetailDto> {
        let bound_tracks = self.load_bound_tracks(user_id).await?;
        let all_tracks: Vec<Track> = bound_tracks
            .iter()
            .filter(|t| t.artist_id == artist_id || t.album_artist_id == artist_id)
            .cloned()
            .collect();
        if all_tracks.is_empty() {
            return Err(DomainError::NotFound("artist not found".into()));
        }

        let artist = self
            .project_user_artists(&all_tracks)
            .await?
            .into_iter()
            .find(|a| a.id == artist_id)
            .ok_or_else(|| DomainError::NotFound("artist not found".into()))?;
        let mut albums: Vec<Album> = self
            .project_user_albums(&bound_tracks)
            .await?
            .into_iter()
            .filter(|a| a.album_artist_id == artist_id)
            .collect();
        albums.sort_by(|a, b| {
            b.year
                .cmp(&a.year)
                .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
        });

        let appears_raw: Vec<Track> = all_tracks
            .iter()
            .filter(|t| t.artist_id == artist_id && t.album_artist_id != artist_id)
            .cloned()
            .collect();

        let mut enriched_all = self.enrich_tracks(user_id, all_tracks).await?;
        enriched_all.sort_by(|a, b| {
            b.play_count
                .cmp(&a.play_count)
                .then_with(|| a.title.cmp(&b.title))
        });
        enriched_all.truncate(20);

        let artist_dto = self
            .enrich_artists(user_id, vec![artist])
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound("artist not found".into()))?;
        let album_dtos = self.enrich_albums(user_id, albums).await?;
        let appears_dtos = self.enrich_tracks(user_id, appears_raw).await?;

        Ok(ArtistDetailDto {
            artist: artist_dto,
            albums: album_dtos,
            appears_on_tracks: appears_dtos,
            top_tracks: enriched_all,
        })
    }

    pub async fn list_genres(&self, user_id: UserId) -> DomainResult<Vec<GenreStat>> {
        let tracks = self.load_bound_tracks(user_id).await?;
        let mut map: BTreeMap<String, (String, u64, HashSet<AlbumId>)> = BTreeMap::new();
        for tr in tracks {
            if let Some(ref g) = tr.genre {
                let trimmed = g.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let entry = map
                    .entry(trimmed.to_lowercase())
                    .or_insert_with(|| (trimmed.to_string(), 0, HashSet::new()));
                entry.1 += 1;
                entry.2.insert(tr.album_id);
            }
        }
        let mut out: Vec<GenreStat> = map
            .into_values()
            .map(|(display, song_count, albums)| GenreStat {
                name: display,
                song_count,
                album_count: albums.len() as u64,
            })
            .collect();
        out.sort_by(|a, b| {
            b.song_count
                .cmp(&a.song_count)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        Ok(out)
    }

    pub async fn list_tags(&self, user_id: UserId) -> DomainResult<Vec<MusicTagStat>> {
        let tracks = self.load_bound_tracks(user_id).await?;
        if tracks.is_empty() {
            return Ok(Vec::new());
        }
        let ids: Vec<TrackId> = tracks.iter().map(|t| t.id).collect();
        let tag_map = self.catalog.get_tags_for_tracks(&ids).await?;
        let mut counts: BTreeMap<String, (String, u64)> = BTreeMap::new();
        for tags in tag_map.into_values() {
            for tag in tags {
                let entry = counts.entry(tag.to_lowercase()).or_insert_with(|| (tag, 0));
                entry.1 += 1;
            }
        }
        let mut out: Vec<MusicTagStat> = counts
            .into_values()
            .map(|(name, song_count)| MusicTagStat { name, song_count })
            .collect();
        out.sort_by(|a, b| {
            b.song_count
                .cmp(&a.song_count)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        Ok(out)
    }

    pub async fn set_track_tags(
        &self,
        actor: &User,
        track_id: TrackId,
        tags: Vec<String>,
    ) -> DomainResult<TrackDto> {
        require_admin(actor)?;
        let track = self
            .catalog
            .find_track_by_id(track_id)
            .await?
            .filter(|t| !t.is_missing)
            .ok_or_else(|| DomainError::NotFound("track not found".into()))?;
        self.catalog.set_track_tags(track.id, tags).await?;
        let mut list = self.enrich_tracks(actor.id, vec![track]).await?;
        list.pop()
            .ok_or_else(|| DomainError::NotFound("track not found".into()))
    }

    pub async fn get_random_tracks(
        &self,
        user_id: UserId,
        size: u32,
        library_id: Option<LibraryId>,
        genre: Option<&str>,
        from_year: Option<i32>,
        to_year: Option<i32>,
    ) -> DomainResult<Vec<TrackDto>> {
        let genre_lower = genre
            .map(|g| g.trim().to_lowercase())
            .filter(|g| !g.is_empty());
        let mut tracks: Vec<Track> = self
            .load_bound_tracks(user_id)
            .await?
            .into_iter()
            .filter(|t| {
                if let Some(lid) = library_id
                    && t.library_id != lid
                {
                    return false;
                }
                if let Some(ref g) = genre_lower
                    && !t
                        .genre
                        .as_ref()
                        .is_some_and(|tg| tg.trim().to_lowercase() == *g)
                {
                    return false;
                }
                if let Some(fy) = from_year
                    && t.year.is_none_or(|y| y < fy)
                {
                    return false;
                }
                if let Some(ty) = to_year
                    && t.year.is_none_or(|y| y > ty)
                {
                    return false;
                }
                true
            })
            .collect();
        let take_n = tracks.len().min(size.clamp(1, 500) as usize);
        shuffle_prefix(&mut tracks, take_n);
        tracks.truncate(take_n);
        self.enrich_tracks(user_id, tracks).await
    }

    pub async fn get_tracks_by_genre(
        &self,
        user_id: UserId,
        genre: &str,
        offset: u32,
        size: u32,
        library_id: Option<LibraryId>,
    ) -> DomainResult<Vec<TrackDto>> {
        let g_lower = genre.trim().to_lowercase();
        let mut tracks: Vec<Track> = self
            .load_bound_tracks(user_id)
            .await?
            .into_iter()
            .filter(|t| {
                library_id.is_none_or(|lid| t.library_id == lid)
                    && t.genre
                        .as_ref()
                        .is_some_and(|tg| tg.trim().to_lowercase() == g_lower)
            })
            .collect();
        tracks.sort_by_key(|t| t.title.to_lowercase());
        let sliced: Vec<Track> = tracks
            .into_iter()
            .skip(offset as usize)
            .take(size.clamp(1, 500) as usize)
            .collect();
        self.enrich_tracks(user_id, sliced).await
    }

    pub async fn get_top_tracks_for_artist_name(
        &self,
        user_id: UserId,
        artist_name: &str,
        count: u32,
    ) -> DomainResult<Vec<TrackDto>> {
        let Some(artist) = self.catalog.find_artist_by_name(artist_name.trim()).await? else {
            return Ok(Vec::new());
        };
        let tracks: Vec<Track> = self
            .load_bound_tracks(user_id)
            .await?
            .into_iter()
            .filter(|t| t.artist_id == artist.id || t.album_artist_id == artist.id)
            .collect();
        let mut dtos = self.enrich_tracks(user_id, tracks).await?;
        dtos.sort_by(|a, b| {
            b.play_count
                .cmp(&a.play_count)
                .then_with(|| a.title.cmp(&b.title))
        });
        dtos.truncate(count.clamp(1, 200) as usize);
        Ok(dtos)
    }

    pub async fn search(
        &self,
        user_id: UserId,
        query: SearchQuery,
    ) -> DomainResult<SearchResultDto> {
        let mut bound_tracks = self.load_bound_tracks(user_id).await?;
        if let Some(lib_id) = query.library_id {
            bound_tracks.retain(|t| t.library_id == lib_id);
        }
        let q_lower = query.query.trim().trim_matches('"').to_lowercase();

        let mut artists = self.project_user_artists(&bound_tracks).await?;
        artists.retain(|a| q_lower.is_empty() || a.name.to_lowercase().contains(&q_lower));
        artists.sort_by_key(|a| a.name.to_lowercase());
        let artists: Vec<Artist> = artists
            .into_iter()
            .skip(query.artist_offset as usize)
            .take(query.artist_count as usize)
            .collect();

        let mut albums = self.project_user_albums(&bound_tracks).await?;
        albums.retain(|a| {
            q_lower.is_empty()
                || a.title.to_lowercase().contains(&q_lower)
                || a.album_artist_name.to_lowercase().contains(&q_lower)
        });
        albums.sort_by_key(|a| a.title.to_lowercase());
        let albums: Vec<Album> = albums
            .into_iter()
            .skip(query.album_offset as usize)
            .take(query.album_count as usize)
            .collect();

        let mut tracks: Vec<Track> = bound_tracks
            .into_iter()
            .filter(|t| {
                q_lower.is_empty()
                    || t.title.to_lowercase().contains(&q_lower)
                    || t.artist_name.to_lowercase().contains(&q_lower)
                    || t.album_title.to_lowercase().contains(&q_lower)
            })
            .collect();
        tracks.sort_by_key(|t| t.title.to_lowercase());
        let tracks: Vec<Track> = tracks
            .into_iter()
            .skip(query.song_offset as usize)
            .take(query.song_count as usize)
            .collect();

        Ok(SearchResultDto {
            artists: self.enrich_artists(user_id, artists).await?,
            albums: self.enrich_albums(user_id, albums).await?,
            tracks: self.enrich_tracks(user_id, tracks).await?,
        })
    }

    pub async fn get_music_directory(
        &self,
        user_id: UserId,
        raw_id: &str,
    ) -> DomainResult<MusicDirectoryNode> {
        let uuid = Uuid::parse_str(raw_id.trim())
            .map_err(|_| DomainError::NotFound("invalid directory id".into()))?;
        let bound_tracks = self.load_bound_tracks(user_id).await?;

        if let Some(album) = self.catalog.find_album_by_id(AlbumId(uuid)).await? {
            let mut tracks: Vec<Track> = bound_tracks
                .into_iter()
                .filter(|t| t.album_id == album.id)
                .collect();
            if tracks.is_empty() {
                return Err(DomainError::NotFound("music directory not found".into()));
            }
            sort_album_tracks(&mut tracks);
            return Ok(MusicDirectoryNode::Folder {
                id: album.id.to_string(),
                parent: Some(album.album_artist_id.to_string()),
                name: album.title,
                sub_albums: Vec::new(),
                tracks: self.enrich_tracks(user_id, tracks).await?,
            });
        }

        if let Some(artist) = self.catalog.find_artist_by_id(ArtistId(uuid)).await? {
            let mut albums: Vec<Album> = self
                .project_user_albums(&bound_tracks)
                .await?
                .into_iter()
                .filter(|a| a.album_artist_id == artist.id)
                .collect();
            if albums.is_empty() {
                return Err(DomainError::NotFound("music directory not found".into()));
            }
            albums.sort_by_key(|a| a.title.to_lowercase());
            return Ok(MusicDirectoryNode::Folder {
                id: artist.id.to_string(),
                parent: None,
                name: artist.name,
                sub_albums: self.enrich_albums(user_id, albums).await?,
                tracks: Vec::new(),
            });
        }

        if let Some(lib) = self.libraries.find_by_id(LibraryId(uuid)).await? {
            let lib_tracks: Vec<Track> = bound_tracks
                .into_iter()
                .filter(|t| t.library_id == lib.id)
                .collect();
            let mut albums = self.project_user_albums(&lib_tracks).await?;
            albums.sort_by_key(|a| a.title.to_lowercase());
            albums.truncate(500);
            return Ok(MusicDirectoryNode::Folder {
                id: lib.id.to_string(),
                parent: None,
                name: lib.name,
                sub_albums: self.enrich_albums(user_id, albums).await?,
                tracks: Vec::new(),
            });
        }

        Err(DomainError::NotFound("music directory not found".into()))
    }

    pub async fn get_lyrics_by_track(
        &self,
        user_id: UserId,
        track_id: TrackId,
    ) -> DomainResult<StructuredLyrics> {
        self.require_track_bound(user_id, track_id).await?;
        let track = self
            .catalog
            .find_track_by_id(track_id)
            .await?
            .filter(|t| !t.is_missing)
            .ok_or_else(|| DomainError::NotFound("track not found".into()))?;
        let library = self
            .libraries
            .find_by_id(track.library_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("library not found".into()))?;

        let sidecar = self
            .storage
            .read_sidecar_lrc_for_track(&library, &track)
            .await?;
        let audio_path = self
            .storage
            .resolve_audio_fs_path(&library, &track)
            .await
            .ok();

        let lyrics_resolver = self.lyrics.clone();
        let mut resolved = tokio::task::spawn_blocking(move || {
            lyrics_resolver.resolve_lyrics(audio_path.as_deref(), sidecar.as_deref())
        })
        .await
        .map_err(|e| DomainError::Storage(format!("lyrics resolve task failed: {e}")))??;
        if resolved.display_artist.is_none() {
            resolved.display_artist = Some(track.artist_name);
        }
        if resolved.display_title.is_none() {
            resolved.display_title = Some(track.title);
        }
        Ok(resolved)
    }

    pub async fn get_lyrics_by_artist_title(
        &self,
        user_id: UserId,
        artist: Option<&str>,
        title: Option<&str>,
    ) -> DomainResult<StructuredLyrics> {
        let title_q = title.unwrap_or_default().trim();
        let artist_q = artist.unwrap_or_default().trim();
        let search_term = if !title_q.is_empty() {
            title_q
        } else if !artist_q.is_empty() {
            artist_q
        } else {
            return Ok(StructuredLyrics::empty());
        };
        let search_lower = search_term.to_lowercase();

        let mut candidates: Vec<Track> = self
            .load_bound_tracks(user_id)
            .await?
            .into_iter()
            .filter(|t| {
                t.title.to_lowercase().contains(&search_lower)
                    || t.artist_name.to_lowercase().contains(&search_lower)
                    || t.album_title.to_lowercase().contains(&search_lower)
            })
            .collect();
        candidates.sort_by_key(|t| t.title.to_lowercase());
        candidates.truncate(25);

        let artist_lower = artist_q.to_lowercase();
        let title_lower = title_q.to_lowercase();

        let mut prioritized: Vec<&Track> = candidates
            .iter()
            .filter(|t| {
                t.has_lyrics()
                    && (title_lower.is_empty() || t.title.to_lowercase() == title_lower)
                    && (artist_lower.is_empty()
                        || t.artist_name.to_lowercase().contains(&artist_lower))
            })
            .collect();
        for tr in &candidates {
            if tr.has_lyrics() && !prioritized.iter().any(|p| p.id == tr.id) {
                prioritized.push(tr);
            }
        }

        for track in prioritized {
            let lyr = self.get_lyrics_by_track(user_id, track.id).await?;
            if !lyr.is_empty() {
                return Ok(lyr);
            }
        }
        Ok(StructuredLyrics::empty())
    }

    pub async fn get_cover_art(
        &self,
        actor: &User,
        raw_id: &str,
        max_size: Option<u32>,
    ) -> DomainResult<BinaryPayload> {
        let clean = raw_id
            .trim()
            .strip_prefix("al-")
            .or_else(|| raw_id.trim().strip_prefix("ar-"))
            .or_else(|| raw_id.trim().strip_prefix("tr-"))
            .or_else(|| raw_id.trim().strip_prefix("pl-"))
            .unwrap_or_else(|| raw_id.trim());

        if let Ok(uuid) = Uuid::parse_str(clean) {
            if let Some(album) = self.catalog.find_album_by_id(AlbumId(uuid)).await? {
                let alb_tracks = self.catalog.list_tracks_by_album(album.id).await?;
                let has_local_pool_track = alb_tracks.iter().any(|t| !t.source.is_online_tracked());
                if !has_local_pool_track {
                    let bound = self
                        .bindings
                        .list_bound_ids(actor.id, MediaKind::Music)
                        .await?;
                    if !alb_tracks.iter().any(|t| bound.contains(&t.id.0)) {
                        return Err(DomainError::NotFound("cover art not found".into()));
                    }
                }
                if let Some(ref key) = album.cover_key {
                    return self.storage.read_cover(key, max_size).await;
                }
            }
            if let Some(track) = self.catalog.find_track_by_id(TrackId(uuid)).await?
                && !track.is_missing
            {
                if track.source.is_online_tracked() {
                    self.require_track_bound(actor.id, track.id).await?;
                }
                if let Some(ref key) = track.cover_key {
                    return self.storage.read_cover(key, max_size).await;
                }
            }
            if let Some(artist) = self.catalog.find_artist_by_id(ArtistId(uuid)).await? {
                let ar_tracks = self.catalog.list_tracks_by_artist(artist.id).await?;
                let has_local_pool_track = ar_tracks.iter().any(|t| !t.source.is_online_tracked());
                if !has_local_pool_track {
                    let bound = self
                        .bindings
                        .list_bound_ids(actor.id, MediaKind::Music)
                        .await?;
                    if !ar_tracks.iter().any(|t| bound.contains(&t.id.0)) {
                        return Err(DomainError::NotFound("cover art not found".into()));
                    }
                }
                if let Some(ref key) = artist.cover_key {
                    return self.storage.read_cover(key, max_size).await;
                }
            }
            if let Some(pl) = self
                .collections
                .find_playlist_by_id(PlaylistId(uuid))
                .await?
            {
                if !pl.is_public && pl.owner_id != actor.id && !actor.role.is_admin() {
                    return Err(DomainError::Forbidden(
                        "not authorized to access private playlist cover".into(),
                    ));
                }
                let bound = self
                    .bindings
                    .list_bound_ids(actor.id, MediaKind::Music)
                    .await?;
                for &track_id in &pl.track_ids {
                    if !bound.contains(&track_id.0) {
                        continue;
                    }
                    if let Some(track) = self.catalog.find_track_by_id(track_id).await?
                        && !track.is_missing
                        && let Some(ref key) = track.cover_key
                    {
                        return self.storage.read_cover(key, max_size).await;
                    }
                }
                return Err(DomainError::NotFound("playlist has no cover art".into()));
            }
        }

        // Raw cover_key lookup (used by Web UI `buildCoverUrl(track.cover_key)`):
        // enforce OnlineTracked isolation so private tracked covers are never leaked.
        let libs = self.libraries.list_by_media(MediaKind::Music).await?;
        let mut matched_pool = false;
        let mut matched_tracked_tracks: Vec<TrackId> = Vec::new();
        let mut tracked_album_ids: HashSet<AlbumId> = HashSet::new();
        let mut album_to_tracked_tracks: HashMap<AlbumId, Vec<TrackId>> = HashMap::new();
        for lib in libs {
            let tracks = self.catalog.list_tracks_by_library(lib.id).await?;
            for tr in tracks {
                if tr.is_missing {
                    continue;
                }
                if tr.cover_key.as_deref() == Some(clean) {
                    if tr.source.is_online_tracked() {
                        matched_tracked_tracks.push(tr.id);
                    } else {
                        matched_pool = true;
                        break;
                    }
                }
                if tr.source.is_online_tracked() {
                    tracked_album_ids.insert(tr.album_id);
                    album_to_tracked_tracks
                        .entry(tr.album_id)
                        .or_default()
                        .push(tr.id);
                }
            }
            if matched_pool {
                break;
            }
        }
        if !matched_pool && !tracked_album_ids.is_empty() {
            let alb_ids: Vec<AlbumId> = tracked_album_ids.into_iter().collect();
            for alb in self.catalog.find_albums_by_ids(&alb_ids).await? {
                if alb.cover_key.as_deref() == Some(clean)
                    && let Some(tids) = album_to_tracked_tracks.get(&alb.id)
                {
                    matched_tracked_tracks.extend(tids.iter().copied());
                }
            }
        }
        if !matched_pool && !matched_tracked_tracks.is_empty() {
            let bound = self
                .bindings
                .list_bound_ids(actor.id, MediaKind::Music)
                .await?;
            if !matched_tracked_tracks
                .iter()
                .any(|tid| bound.contains(&tid.0))
            {
                return Err(DomainError::NotFound("cover art not found".into()));
            }
        }

        self.storage.read_cover(clean, max_size).await
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

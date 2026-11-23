use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use jiff::Timestamp;
use uuid::Uuid;
use xmedia_core::application::ports::BinaryPayload;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::{LibraryId, MediaKind};
use xmedia_core::domain::repository::LibraryRepository;
use xmedia_core::domain::user::{User, UserId, require_admin};

use crate::application::dto::{
    AlbumDetailDto, AlbumDto, ArtistDetailDto, ArtistDto, BookmarkDto, CatalogStatsDto,
    HomeOverviewDto, OnlineTrackedTrackDto, PlayQueueDto, SearchResultDto, TrackDto,
};
use crate::application::ports::{LyricsResolverPort, MusicStoragePort};
use crate::domain::catalog::{
    Album, AlbumId, AlbumListQuery, AlbumListType, Artist, ArtistId, ArtistQuery, GenreStat,
    MusicTagStat, PagedResult, SearchQuery, SortOrder, StructuredLyrics, Track, TrackId,
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
    storage: Arc<dyn MusicStoragePort>,
    lyrics: Arc<dyn LyricsResolverPort>,
}

impl MusicCatalogService {
    pub fn new(
        libraries: Arc<dyn LibraryRepository>,
        catalog: Arc<dyn MusicCatalogRepository>,
        collections: Arc<dyn MusicCollectionRepository>,
        storage: Arc<dyn MusicStoragePort>,
        lyrics: Arc<dyn LyricsResolverPort>,
    ) -> Self {
        Self {
            libraries,
            catalog,
            collections,
            storage,
            lyrics,
        }
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
        let clamped = limit.clamp(1, 100) as usize;
        let scrobbles = self
            .collections
            .list_recent_scrobbles(user_id, (clamped * 3).min(300) as u32)
            .await?;
        let mut seen_track_ids = Vec::new();
        for sc in scrobbles {
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
        // Fetch shared user engagement datasets ONCE and reuse across albums, tracks, and stats.
        let all_stars = self.collections.list_starred(user_id, None).await?;
        let mut track_star_map: HashMap<Uuid, Timestamp> = HashMap::new();
        let mut album_star_map: HashMap<Uuid, Timestamp> = HashMap::new();
        for s in all_stars {
            match s.kind {
                StarTargetKind::Track => {
                    track_star_map.insert(s.target_id, s.starred_at);
                }
                StarTargetKind::Album => {
                    album_star_map.insert(s.target_id, s.starred_at);
                }
                StarTargetKind::Artist => {}
            }
        }
        let starred_tracks = track_star_map.len() as u64;

        let all_ratings = self.collections.list_user_ratings(user_id).await?;
        let mut track_rating_map: HashMap<Uuid, u8> = HashMap::new();
        let mut album_rating_map: HashMap<Uuid, u8> = HashMap::new();
        for r in all_ratings {
            match r.kind {
                StarTargetKind::Track => {
                    track_rating_map.insert(r.target_id, r.rating);
                }
                StarTargetKind::Album => {
                    album_rating_map.insert(r.target_id, r.rating);
                }
                StarTargetKind::Artist => {}
            }
        }

        let all_stats = self.collections.list_user_play_stats(user_id).await?;
        let mut track_stat_map: HashMap<TrackId, (u64, Timestamp)> =
            HashMap::with_capacity(all_stats.len());
        let mut album_play_counts: HashMap<AlbumId, u64> = HashMap::new();
        for st in all_stats {
            *album_play_counts.entry(st.album_id).or_default() += st.play_count;
            track_stat_map.insert(st.track_id, (st.play_count, st.last_played_at));
        }

        // Fetch all active albums ONCE (already sorted newest-first).
        let all_albums_page = self
            .catalog
            .list_albums(
                &AlbumListQuery {
                    list_type: AlbumListType::Newest,
                    size: u32::MAX,
                    ..Default::default()
                },
                None,
                None,
            )
            .await?;
        let total_albums = all_albums_page.total;
        let recent_added_raw: Vec<Album> = all_albums_page.items.iter().take(12).cloned().collect();
        let album_by_id: HashMap<AlbumId, Album> = all_albums_page
            .items
            .into_iter()
            .map(|a| (a.id, a))
            .collect();

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
            if !recent_ids.contains(&sc.track_id) {
                recent_ids.push(sc.track_id);
            }
            if recent_ids.len() >= 10 {
                break;
            }
        }
        let raw_bookmarks = self.collections.list_bookmarks(user_id).await?;
        let bookmarks_count = raw_bookmarks.len() as u64;
        let raw_queue = self.collections.get_play_queue(user_id).await?;

        // Fetch all active tracks ONCE for total_tracks, genres_count, random selection,
        // and O(1) lookup for scrobbles / bookmarks / play queue.
        let mut all_active_tracks = self
            .catalog
            .query_tracks(&TrackQuery {
                page: 1,
                page_size: u32::MAX,
                ..Default::default()
            })
            .await?
            .items;
        let total_tracks = all_active_tracks.len() as u64;

        let mut active_track_ids: HashSet<TrackId> =
            HashSet::with_capacity(all_active_tracks.len());
        let mut genre_set: HashSet<String> = HashSet::new();
        let mut track_idx: HashMap<TrackId, usize> =
            HashMap::with_capacity(all_active_tracks.len());
        for (idx, tr) in all_active_tracks.iter().enumerate() {
            active_track_ids.insert(tr.id);
            track_idx.insert(tr.id, idx);
            if let Some(ref g) = tr.genre {
                let clean = g.trim().to_lowercase();
                if !clean.is_empty() {
                    genre_set.insert(clean);
                }
            }
        }
        let genres_count = genre_set.len() as u64;

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

        // Sample up to 12 random tracks directly in memory without another DB scan.
        let sample_count = all_active_tracks.len().min(12);
        let mut rng_state = (Uuid::now_v7().as_u128() as u64)
            ^ (Timestamp::now().as_nanosecond() as u64)
            ^ 0x9E37_79B9_7F4A_7C15;
        let n = all_active_tracks.len();
        for i in 0..sample_count {
            rng_state = rng_state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = rng_state;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z = z ^ (z >> 31);
            let j = i + ((z as usize) % (n - i));
            all_active_tracks.swap(i, j);
        }
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

        let play_queue = raw_queue.map(|q| {
            let current_tracks: Vec<TrackDto> = q
                .current_track_id
                .and_then(|cid| dto_lookup.get(&cid).cloned())
                .into_iter()
                .collect();
            PlayQueueDto {
                tracks: current_tracks,
                current_track_id: q.current_track_id,
                position_ms: q.position_ms,
                changed_by: q.changed_by,
                updated_at: q.updated_at,
            }
        });

        let total_artists = self
            .catalog
            .list_artists(&ArtistQuery {
                size: 1,
                ..Default::default()
            })
            .await?
            .total;
        let playlists_count = self
            .collections
            .list_visible_playlists(user_id)
            .await?
            .len() as u64;
        let tags_count = self.catalog.count_active_tags(&active_track_ids).await?;
        let libraries_count = self.libraries.list_by_media(MediaKind::Music).await?.len() as u64;

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
        let paged = self.catalog.query_tracks(&query).await?;
        let items = self.enrich_tracks(user_id, paged.items).await?;
        Ok(PagedResult {
            items,
            total: paged.total,
            page: paged.page,
            page_size: paged.page_size,
        })
    }

    pub async fn get_track(&self, user_id: UserId, track_id: TrackId) -> DomainResult<TrackDto> {
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
        let paged = match query.list_type {
            AlbumListType::Starred => {
                let mut stars = self
                    .collections
                    .list_starred(user_id, Some(StarTargetKind::Album))
                    .await?;
                stars.sort_by_key(|a| std::cmp::Reverse(a.starred_at));
                let ids: Vec<AlbumId> = stars.into_iter().map(|s| AlbumId(s.target_id)).collect();
                self.catalog.list_albums(&query, Some(&ids), None).await?
            }
            AlbumListType::Frequent => {
                let stats = self.collections.list_user_play_stats(user_id).await?;
                let mut agg: HashMap<AlbumId, u64> = HashMap::new();
                for st in stats {
                    *agg.entry(st.album_id).or_default() += st.play_count;
                }
                let mut pairs: Vec<(AlbumId, u64)> = agg.into_iter().collect();
                pairs.sort_by_key(|a| std::cmp::Reverse(a.1));
                let ordered: Vec<AlbumId> = pairs.into_iter().map(|(id, _)| id).collect();
                self.catalog
                    .list_albums(&query, None, Some(&ordered))
                    .await?
            }
            AlbumListType::Recent => {
                let stats = self.collections.list_user_play_stats(user_id).await?;
                let mut agg: HashMap<AlbumId, Timestamp> = HashMap::new();
                for st in stats {
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
                let ordered: Vec<AlbumId> = pairs.into_iter().map(|(id, _)| id).collect();
                self.catalog
                    .list_albums(&query, None, Some(&ordered))
                    .await?
            }
            _ => self.catalog.list_albums(&query, None, None).await?,
        };

        let items = self.enrich_albums(user_id, paged.items).await?;
        Ok(PagedResult {
            items,
            total: paged.total,
            page: paged.page,
            page_size: paged.page_size,
        })
    }

    pub async fn get_album_detail(
        &self,
        user_id: UserId,
        album_id: AlbumId,
    ) -> DomainResult<AlbumDetailDto> {
        let album = self
            .catalog
            .find_album_by_id(album_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("album not found".into()))?;
        let tracks = self.catalog.list_tracks_by_album(album_id).await?;
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
        let paged = self.catalog.list_artists(&query).await?;
        let items = self.enrich_artists(user_id, paged.items).await?;
        Ok(PagedResult {
            items,
            total: paged.total,
            page: paged.page,
            page_size: paged.page_size,
        })
    }

    pub async fn get_artist_detail(
        &self,
        user_id: UserId,
        artist_id: ArtistId,
    ) -> DomainResult<ArtistDetailDto> {
        let artist = self
            .catalog
            .find_artist_by_id(artist_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("artist not found".into()))?;
        let albums = self.catalog.list_albums_by_artist(artist_id).await?;
        let all_tracks = self.catalog.list_tracks_by_artist(artist_id).await?;

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

    pub async fn list_genres(&self) -> DomainResult<Vec<GenreStat>> {
        self.catalog.list_genres().await
    }

    pub async fn list_tags(&self) -> DomainResult<Vec<MusicTagStat>> {
        self.catalog.list_tags().await
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
        self.get_track(actor.id, track.id).await
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
        let tracks = self
            .catalog
            .list_random_tracks(size.clamp(1, 500), library_id, genre, from_year, to_year)
            .await?;
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
        let tracks = self
            .catalog
            .list_tracks_by_genre(genre, offset, size.clamp(1, 500), library_id)
            .await?;
        self.enrich_tracks(user_id, tracks).await
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
        let tracks = self.catalog.list_tracks_by_artist(artist.id).await?;
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
        let res = self.catalog.search_catalog(&query).await?;
        Ok(SearchResultDto {
            artists: self.enrich_artists(user_id, res.artists).await?,
            albums: self.enrich_albums(user_id, res.albums).await?,
            tracks: self.enrich_tracks(user_id, res.tracks).await?,
        })
    }

    pub async fn get_music_directory(
        &self,
        user_id: UserId,
        raw_id: &str,
    ) -> DomainResult<MusicDirectoryNode> {
        let uuid = Uuid::parse_str(raw_id.trim())
            .map_err(|_| DomainError::NotFound("invalid directory id".into()))?;

        if let Some(album) = self.catalog.find_album_by_id(AlbumId(uuid)).await? {
            let tracks = self.catalog.list_tracks_by_album(album.id).await?;
            return Ok(MusicDirectoryNode::Folder {
                id: album.id.to_string(),
                parent: Some(album.album_artist_id.to_string()),
                name: album.title,
                sub_albums: Vec::new(),
                tracks: self.enrich_tracks(user_id, tracks).await?,
            });
        }

        if let Some(artist) = self.catalog.find_artist_by_id(ArtistId(uuid)).await? {
            let albums = self.catalog.list_albums_by_artist(artist.id).await?;
            return Ok(MusicDirectoryNode::Folder {
                id: artist.id.to_string(),
                parent: None,
                name: artist.name,
                sub_albums: self.enrich_albums(user_id, albums).await?,
                tracks: Vec::new(),
            });
        }

        if let Some(lib) = self.libraries.find_by_id(LibraryId(uuid)).await? {
            let albums = self
                .catalog
                .list_albums(
                    &AlbumListQuery {
                        list_type: AlbumListType::AlphabeticalByName,
                        library_id: Some(lib.id),
                        size: 500,
                        ..Default::default()
                    },
                    None,
                    None,
                )
                .await?
                .items;
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

    pub async fn get_lyrics_by_track(&self, track_id: TrackId) -> DomainResult<StructuredLyrics> {
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

        let candidates = self
            .catalog
            .query_tracks(&TrackQuery {
                search: Some(search_term.to_string()),
                sort_by: TrackSortField::Title,
                sort_order: SortOrder::Asc,
                page: 1,
                page_size: 25,
                ..Default::default()
            })
            .await?
            .items;

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
            let lyr = self.get_lyrics_by_track(track.id).await?;
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
            if let Some(album) = self.catalog.find_album_by_id(AlbumId(uuid)).await?
                && let Some(ref key) = album.cover_key
            {
                return self.storage.read_cover(key, max_size).await;
            }
            if let Some(track) = self.catalog.find_track_by_id(TrackId(uuid)).await?
                && !track.is_missing
                && let Some(ref key) = track.cover_key
            {
                return self.storage.read_cover(key, max_size).await;
            }
            if let Some(artist) = self.catalog.find_artist_by_id(ArtistId(uuid)).await?
                && let Some(ref key) = artist.cover_key
            {
                return self.storage.read_cover(key, max_size).await;
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
                for &track_id in &pl.track_ids {
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

        self.storage.read_cover(clean, max_size).await
    }
}

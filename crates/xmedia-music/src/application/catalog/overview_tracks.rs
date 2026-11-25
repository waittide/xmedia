use std::collections::{HashMap, HashSet};

use jiff::Timestamp;
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::{LibraryId, MediaKind};
use xmedia_core::domain::user::UserId;

use super::{MusicCatalogService, shuffle_prefix, sort_tracks};
use crate::application::dto::{
    BookmarkDto, CatalogStatsDto, HomeOverviewDto, PlayQueueDto, PoolTrackDto, TrackDto,
};
use crate::domain::catalog::{Album, AlbumId, PagedResult, Track, TrackId, TrackQuery};
use crate::domain::collection::StarTargetKind;

impl MusicCatalogService {
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
}

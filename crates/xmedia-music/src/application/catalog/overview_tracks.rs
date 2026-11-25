use std::collections::{HashMap, HashSet};

use jiff::Timestamp;
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::{LibraryId, MediaKind};
use xmedia_core::domain::user::UserId;

use super::MusicCatalogService;
use crate::application::dto::{
    BookmarkDto, CatalogStatsDto, HomeOverviewDto, PlayQueueDto, PoolTrackDto, TrackDto,
};
use crate::domain::catalog::{
    AlbumId, AlbumListQuery, AlbumListType, PagedResult, SortOrder, Track, TrackId, TrackQuery,
    TrackSortField,
};
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
        let allowed = self.bound_track_ids(user_id).await?;
        let counts = self.catalog.get_bound_overview_stats(&allowed).await?;

        let all_stars = self.collections.list_starred(user_id, None).await?;
        let mut track_star_map: HashMap<Uuid, Timestamp> = HashMap::new();
        let mut album_star_map: HashMap<Uuid, Timestamp> = HashMap::new();
        for s in all_stars {
            match s.kind {
                StarTargetKind::Track
                    if counts.active_track_ids.contains(&TrackId(s.target_id)) =>
                {
                    track_star_map.insert(s.target_id, s.starred_at);
                }
                StarTargetKind::Album
                    if counts.active_album_ids.contains(&AlbumId(s.target_id)) =>
                {
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
                StarTargetKind::Track
                    if counts.active_track_ids.contains(&TrackId(r.target_id)) =>
                {
                    track_rating_map.insert(r.target_id, r.rating);
                }
                StarTargetKind::Album
                    if counts.active_album_ids.contains(&AlbumId(r.target_id)) =>
                {
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
            if !counts.active_track_ids.contains(&st.track_id) {
                continue;
            }
            *album_play_counts.entry(st.album_id).or_default() += st.play_count;
            track_stat_map.insert(st.track_id, (st.play_count, st.last_played_at));
        }

        let recent_added_raw = self
            .catalog
            .query_bound_albums(
                &counts.active_track_ids,
                &AlbumListQuery {
                    list_type: AlbumListType::Newest,
                    size: 12,
                    ..Default::default()
                },
                &[],
                &[],
                &[],
            )
            .await?
            .items;

        let mut freq_pairs: Vec<(AlbumId, u64)> = album_play_counts
            .iter()
            .filter(|(id, _)| counts.active_album_ids.contains(id))
            .map(|(&id, &cnt)| (id, cnt))
            .collect();
        freq_pairs.sort_by_key(|a| std::cmp::Reverse(a.1));
        let freq_ids: Vec<AlbumId> = freq_pairs.into_iter().take(12).map(|(id, _)| id).collect();
        let frequent_raw = self
            .catalog
            .project_bound_albums_by_ids(&counts.active_track_ids, &freq_ids, None)
            .await?;

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
            if !counts.active_track_ids.contains(&sc.track_id) {
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
            .filter(|bm| counts.active_track_ids.contains(&bm.track_id))
            .collect();
        let bookmarks_count = raw_bookmarks.len() as u64;
        let raw_queue = self.collections.get_play_queue(user_id).await?;

        let random_raw = self
            .catalog
            .list_random_bound_tracks(&counts.active_track_ids, 12, None, None, None, None)
            .await?;
        let random_ids: Vec<TrackId> = random_raw.iter().map(|t| t.id).collect();
        let mut track_pool: HashMap<TrackId, Track> =
            random_raw.into_iter().map(|t| (t.id, t)).collect();

        let mut extra_fetch_ids: Vec<TrackId> = Vec::new();
        for &id in &recent_ids {
            if !track_pool.contains_key(&id) && !extra_fetch_ids.contains(&id) {
                extra_fetch_ids.push(id);
            }
        }
        for bm in &raw_bookmarks {
            if !track_pool.contains_key(&bm.track_id) && !extra_fetch_ids.contains(&bm.track_id) {
                extra_fetch_ids.push(bm.track_id);
            }
        }
        if let Some(ref q) = raw_queue
            && let Some(cid) = q.current_track_id
            && counts.active_track_ids.contains(&cid)
            && !track_pool.contains_key(&cid)
            && !extra_fetch_ids.contains(&cid)
        {
            extra_fetch_ids.push(cid);
        }
        if !extra_fetch_ids.is_empty() {
            for tr in self.catalog.find_tracks_by_ids(&extra_fetch_ids).await? {
                if !tr.is_missing {
                    track_pool.insert(tr.id, tr);
                }
            }
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
                .filter(|cid| counts.active_track_ids.contains(cid));
            let current_tracks: Vec<TrackDto> = current_track_id
                .and_then(|cid| dto_lookup.get(&cid).cloned())
                .into_iter()
                .collect();
            if current_track_id.is_none()
                && !q
                    .track_ids
                    .iter()
                    .any(|tid| counts.active_track_ids.contains(tid))
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
                total_tracks: counts.total_tracks,
                total_albums: counts.total_albums,
                total_artists: counts.total_artists,
                starred_tracks,
                playlists_count,
                listening_count,
                bookmarks_count,
                genres_count: counts.genres_count,
                tags_count: counts.tags_count,
                libraries_count: counts.libraries_count,
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
        let allowed = self.bound_track_ids(user_id).await?;
        let paged = self.catalog.query_bound_tracks(&allowed, &query).await?;
        let items = self.enrich_tracks(user_id, paged.items).await?;
        Ok(PagedResult {
            items,
            total: paged.total,
            page: paged.page,
            page_size: paged.page_size,
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
        let allowed = self.bound_track_ids(user_id).await?;
        let tracks = self
            .catalog
            .list_random_bound_tracks(&allowed, size, library_id, genre, from_year, to_year)
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
        let allowed = self.bound_track_ids(user_id).await?;
        let tracks = self
            .catalog
            .list_bound_tracks_offset(
                &allowed,
                &TrackQuery {
                    library_id,
                    genre: Some(genre.to_string()),
                    sort_by: TrackSortField::Title,
                    sort_order: SortOrder::Asc,
                    ..Default::default()
                },
                offset,
                size.clamp(1, 500),
            )
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
        let allowed = self.bound_track_ids(user_id).await?;
        let tracks = self
            .catalog
            .list_bound_tracks_by_artist(&allowed, artist.id)
            .await?;
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

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::user::{User, UserId};

use crate::application::dto::{
    BookmarkDto, CatalogStatsDto, HomeOverviewDto, PlayQueueDto, SearchResultDto, TrackDto,
};
use crate::application::projector::MusicProjector;
use crate::domain::catalog::{
    AlbumId, AlbumListQuery, AlbumListType, ArtistQuery, ArtistRoleFilter, BoundCatalogCounts,
    GenreStat, MusicTagStat, SearchQuery, SortOrder, Track, TrackId, TrackQuery, TrackSortField,
};
use crate::domain::collection::StarTargetKind;
use crate::domain::repository::{
    AlbumScope, ArtistScope, CatalogFacetQuery, CatalogFacetResult, CatalogMutation,
    MusicCatalogRepository, MusicCollectionRepository, PlaylistQuery, TrackScope,
};

#[derive(Clone)]
pub struct MusicCatalogService {
    catalog: Arc<dyn MusicCatalogRepository>,
    collections: Arc<dyn MusicCollectionRepository>,
    projector: MusicProjector,
}

impl MusicCatalogService {
    pub(crate) fn new(
        catalog: Arc<dyn MusicCatalogRepository>,
        collections: Arc<dyn MusicCollectionRepository>,
        projector: MusicProjector,
    ) -> Self {
        Self {
            catalog,
            collections,
            projector,
        }
    }

    pub async fn list_all_track_ids_for_backfill(&self) -> DomainResult<Vec<Uuid>> {
        match self
            .catalog
            .query_facets(CatalogFacetQuery::ActiveTrackIds)
            .await?
        {
            CatalogFacetResult::TrackIds(ids) => Ok(ids.into_iter().map(|id| id.0).collect()),
            _ => Ok(Vec::new()),
        }
    }

    pub async fn get_home_overview(&self, user_id: UserId) -> DomainResult<HomeOverviewDto> {
        let allowed = self.projector.bound_track_ids(user_id).await?;
        let counts = match self
            .catalog
            .query_facets(CatalogFacetQuery::OverviewStats(&allowed))
            .await?
        {
            CatalogFacetResult::OverviewStats(c) => c,
            _ => BoundCatalogCounts::default(),
        };

        let engagement = self.collections.load_engagement(user_id, None).await?;
        let mut track_star_map = engagement.star_map(StarTargetKind::Track);
        track_star_map.retain(|id, _| counts.active_track_ids.contains(&TrackId(*id)));
        let mut album_star_map = engagement.star_map(StarTargetKind::Album);
        album_star_map.retain(|id, _| counts.active_album_ids.contains(&AlbumId(*id)));
        let starred_tracks = track_star_map.len() as u64;

        let mut track_rating_map = engagement.rating_map(StarTargetKind::Track);
        track_rating_map.retain(|id, _| counts.active_track_ids.contains(&TrackId(*id)));
        let mut album_rating_map = engagement.rating_map(StarTargetKind::Album);
        album_rating_map.retain(|id, _| counts.active_album_ids.contains(&AlbumId(*id)));

        let (track_stat_map, album_play_counts) =
            engagement.play_maps(Some(&counts.active_track_ids));

        let recent_added_raw = self
            .catalog
            .query_albums(AlbumScope::BoundPaged {
                allowed: &counts.active_track_ids,
                query: &AlbumListQuery {
                    list_type: AlbumListType::Newest,
                    size: 12,
                    ..Default::default()
                },
                starred: &[],
                play_stats: &[],
            })
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
            .query_albums(AlbumScope::BoundByIds {
                allowed: &counts.active_track_ids,
                album_ids: &freq_ids,
                library_id: None,
            })
            .await?
            .items;

        let recent_added = MusicProjector::enrich_albums_with_maps(
            recent_added_raw,
            &album_star_map,
            &album_rating_map,
            &album_play_counts,
        );
        let frequent = MusicProjector::enrich_albums_with_maps(
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
            .query_bookmarks(user_id, None)
            .await?
            .into_iter()
            .filter(|bm| counts.active_track_ids.contains(&bm.track_id))
            .collect();
        let bookmarks_count = raw_bookmarks.len() as u64;
        let raw_queue = self.collections.get_play_queue(user_id).await?;

        let random_raw = self
            .catalog
            .query_tracks(TrackScope::Random {
                allowed: &counts.active_track_ids,
                size: 12,
                library_id: None,
                genre: None,
                from_year: None,
                to_year: None,
            })
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
            for tr in self
                .catalog
                .query_tracks(TrackScope::Ids(&extra_fetch_ids))
                .await?
            {
                if !tr.is_missing {
                    track_pool.insert(tr.id, tr);
                }
            }
        }

        let needed_ids: Vec<TrackId> = track_pool.keys().copied().collect();
        let mut tag_map = self.projector.load_track_tags(&needed_ids).await?;
        let all_dtos = MusicProjector::enrich_tracks_with_maps(
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
            .query_playlists(PlaylistQuery::VisibleTo(user_id))
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

    pub async fn list_genres(&self, user_id: UserId) -> DomainResult<Vec<GenreStat>> {
        let allowed = self.projector.bound_track_ids(user_id).await?;
        match self
            .catalog
            .query_facets(CatalogFacetQuery::Genres(&allowed))
            .await?
        {
            CatalogFacetResult::Genres(list) => Ok(list),
            _ => Ok(Vec::new()),
        }
    }

    pub async fn list_tags(&self, user_id: UserId) -> DomainResult<Vec<MusicTagStat>> {
        let allowed = self.projector.bound_track_ids(user_id).await?;
        match self
            .catalog
            .query_facets(CatalogFacetQuery::Tags(Some(&allowed)))
            .await?
        {
            CatalogFacetResult::Tags(list) => Ok(list),
            _ => Ok(Vec::new()),
        }
    }

    pub async fn set_track_tags(
        &self,
        actor: &User,
        track_id: TrackId,
        tags: Vec<String>,
    ) -> DomainResult<TrackDto> {
        actor.ensure_admin()?;
        let track = self
            .catalog
            .query_tracks(TrackScope::Id(track_id))
            .await?
            .into_iter()
            .find(|t| !t.is_missing)
            .ok_or_else(|| DomainError::NotFound("track not found".into()))?;
        self.catalog
            .save(CatalogMutation::TrackTags {
                track_id: track.id,
                tags,
            })
            .await?;
        let mut list = self.projector.enrich_tracks(actor.id, vec![track]).await?;
        list.pop()
            .ok_or_else(|| DomainError::NotFound("track not found".into()))
    }

    pub async fn search(
        &self,
        user_id: UserId,
        query: SearchQuery,
    ) -> DomainResult<SearchResultDto> {
        let allowed = self.projector.bound_track_ids(user_id).await?;
        if allowed.is_empty() {
            return Ok(SearchResultDto {
                artists: Vec::new(),
                albums: Vec::new(),
                tracks: Vec::new(),
            });
        }
        let clean_q = query.query.trim().trim_matches('"');
        let search_opt = (!clean_q.is_empty()).then(|| clean_q.to_string());

        let artists = if query.artist_count > 0 {
            self.catalog
                .query_artists(ArtistScope::BoundPaged {
                    allowed: &allowed,
                    query: &ArtistQuery {
                        library_id: query.library_id,
                        search: search_opt.clone(),
                        role_filter: ArtistRoleFilter::All,
                        offset: query.artist_offset,
                        size: query.artist_count,
                    },
                })
                .await?
                .items
        } else {
            Vec::new()
        };

        let albums = if query.album_count > 0 {
            self.catalog
                .query_albums(AlbumScope::BoundPaged {
                    allowed: &allowed,
                    query: &AlbumListQuery {
                        list_type: AlbumListType::AlphabeticalByName,
                        library_id: query.library_id,
                        search: search_opt.clone(),
                        offset: query.album_offset,
                        size: query.album_count,
                        ..Default::default()
                    },
                    starred: &[],
                    play_stats: &[],
                })
                .await?
                .items
        } else {
            Vec::new()
        };

        let tracks = if query.song_count > 0 {
            self.catalog
                .query_tracks(TrackScope::Offset {
                    allowed: &allowed,
                    query: &TrackQuery {
                        library_id: query.library_id,
                        search: search_opt,
                        sort_by: TrackSortField::Title,
                        sort_order: SortOrder::Asc,
                        ..Default::default()
                    },
                    offset: query.song_offset,
                    limit: query.song_count,
                })
                .await?
        } else {
            Vec::new()
        };

        Ok(SearchResultDto {
            artists: self.projector.enrich_artists(user_id, artists).await?,
            albums: self.projector.enrich_albums(user_id, albums).await?,
            tracks: self.projector.enrich_tracks(user_id, tracks).await?,
        })
    }
}

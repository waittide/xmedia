use std::collections::{HashMap, HashSet};

use jiff::Timestamp;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::domain::library::LibraryId;

use super::ToastyMusicCatalogRepository;
use super::sql::{
    PROJECTED_ALBUM_SELECT_COLUMNS, SqlBuilder, row_fields, row_to_projected_album,
    row_to_projected_artist, val_i64, val_string, val_uuid,
};
use crate::domain::catalog::{
    Album, AlbumId, AlbumListQuery, AlbumListType, Artist, ArtistId, ArtistQuery, ArtistRoleFilter,
    BoundCatalogCounts, GenreStat, MusicTagStat, PagedResult, TrackId,
};
use crate::domain::collection::{RatingEntry, StarTargetKind, StarredEntry, TrackPlayStat};

impl ToastyMusicCatalogRepository {
    pub(super) async fn count_bound_active_tracks_impl(
        &self,
        allowed_track_ids: &HashSet<TrackId>,
    ) -> DomainResult<u64> {
        if allowed_track_ids.is_empty() {
            return Ok(0);
        }
        let mut db = self.db.clone();
        let mut b = SqlBuilder::default();
        let scope = b.bind_track_scope("t.id", allowed_track_ids);
        let sql = format!("SELECT COUNT(*) FROM music_tracks t WHERE t.is_missing = 0 AND {scope}");
        let rows = b.exec_rows(sql, &mut db).await?;
        let count = rows
            .first()
            .and_then(|r| row_fields(r).ok())
            .and_then(|f| f.first())
            .and_then(|v| val_i64(v).ok())
            .unwrap_or(0);
        Ok(u64::try_from(count.max(0)).unwrap_or(0))
    }

    pub(super) async fn count_bound_tracks_by_library_impl(
        &self,
        allowed_track_ids: &HashSet<TrackId>,
    ) -> DomainResult<HashMap<LibraryId, u64>> {
        if allowed_track_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let mut db = self.db.clone();
        let mut b = SqlBuilder::default();
        let scope = b.bind_track_scope("t.id", allowed_track_ids);
        let sql = format!(
            "SELECT t.library_id, COUNT(*) FROM music_tracks t \
             WHERE t.is_missing = 0 AND {scope} GROUP BY t.library_id"
        );
        let rows = b.exec_rows(sql, &mut db).await?;
        let mut out = HashMap::with_capacity(rows.len());
        for row in &rows {
            let f = row_fields(row)?;
            if f.len() >= 2 {
                let lid = LibraryId(val_uuid(&f[0])?);
                let cnt = u64::try_from(val_i64(&f[1])?.max(0)).unwrap_or(0);
                out.insert(lid, cnt);
            }
        }
        Ok(out)
    }

    pub(super) async fn get_bound_overview_stats_impl(
        &self,
        allowed_track_ids: &HashSet<TrackId>,
    ) -> DomainResult<BoundCatalogCounts> {
        if allowed_track_ids.is_empty() {
            return Ok(BoundCatalogCounts::default());
        }
        let mut db = self.db.clone();

        let id_rows = {
            let mut b = SqlBuilder::default();
            let scope = b.bind_track_scope("t.id", allowed_track_ids);
            let sql = format!(
                "SELECT t.id, t.album_id FROM music_tracks t WHERE t.is_missing = 0 AND {scope}"
            );
            b.exec_rows(sql, &mut db).await?
        };
        if id_rows.is_empty() {
            return Ok(BoundCatalogCounts::default());
        }

        let mut active_track_ids = HashSet::with_capacity(id_rows.len());
        let mut active_album_ids = HashSet::new();
        for row in &id_rows {
            let f = row_fields(row)?;
            if f.len() >= 2 {
                active_track_ids.insert(TrackId(val_uuid(&f[0])?));
                active_album_ids.insert(AlbumId(val_uuid(&f[1])?));
            }
        }

        let agg_rows = {
            let mut b = SqlBuilder::default();
            let scope = b.bind_track_scope("t.id", &active_track_ids);
            let sql = format!(
                "WITH bt AS ( \
                    SELECT id, library_id, album_id, artist_id, album_artist_id, genre \
                    FROM music_tracks t WHERE t.is_missing = 0 AND {scope} \
                 ), \
                 ar AS ( \
                    SELECT artist_id AS aid FROM bt \
                    UNION \
                    SELECT album_artist_id AS aid FROM bt \
                 ) \
                 SELECT \
                    (SELECT COUNT(DISTINCT aid) FROM ar WHERE aid IN (SELECT id FROM music_artists)), \
                    (SELECT COUNT(DISTINCT LOWER(TRIM(genre))) FROM bt WHERE genre IS NOT NULL AND TRIM(genre) != ''), \
                    (SELECT COUNT(DISTINCT library_id) FROM bt), \
                    (SELECT COUNT(DISTINCT tt.tag_lower) FROM music_track_tags tt INNER JOIN bt ON bt.id = tt.track_id)"
            );
            b.exec_rows(sql, &mut db).await?
        };

        let (total_artists, genres_count, libraries_count, tags_count) =
            if let Some(first) = agg_rows.first() {
                let f = row_fields(first)?;
                (
                    u64::try_from(val_i64(&f[0])?.max(0)).unwrap_or(0),
                    u64::try_from(val_i64(&f[1])?.max(0)).unwrap_or(0),
                    u64::try_from(val_i64(&f[2])?.max(0)).unwrap_or(0),
                    u64::try_from(val_i64(&f[3])?.max(0)).unwrap_or(0),
                )
            } else {
                (0, 0, 0, 0)
            };

        let total_tracks = active_track_ids.len() as u64;
        let total_albums = active_album_ids.len() as u64;

        Ok(BoundCatalogCounts {
            active_track_ids,
            active_album_ids,
            total_tracks,
            total_albums,
            total_artists,
            genres_count,
            libraries_count,
            tags_count,
        })
    }

    pub(super) async fn query_bound_albums_impl(
        &self,
        allowed_track_ids: &HashSet<TrackId>,
        query: &AlbumListQuery,
        starred_ids: &[StarredEntry],
        play_stats: &[TrackPlayStat],
        _ratings: &[RatingEntry],
    ) -> DomainResult<PagedResult<Album>> {
        let size = query.size.max(1);
        let page = (query.offset / size) + 1;
        if allowed_track_ids.is_empty() {
            return Ok(PagedResult {
                items: Vec::new(),
                total: 0,
                page,
                page_size: size,
            });
        }

        match query.list_type {
            AlbumListType::Starred => {
                let mut stars: Vec<&StarredEntry> = starred_ids
                    .iter()
                    .filter(|s| s.kind == StarTargetKind::Album)
                    .collect();
                stars.sort_by_key(|s| std::cmp::Reverse(s.starred_at));
                let ordered_ids: Vec<AlbumId> =
                    stars.into_iter().map(|s| AlbumId(s.target_id)).collect();
                self.query_ordered_candidate_albums(allowed_track_ids, query, &ordered_ids)
                    .await
            }
            AlbumListType::Frequent => {
                let active_played_tracks = self
                    .filter_active_bound_played_tracks(allowed_track_ids, play_stats)
                    .await?;
                let mut agg: HashMap<AlbumId, u64> = HashMap::new();
                for st in play_stats {
                    if active_played_tracks.contains(&st.track_id) {
                        *agg.entry(st.album_id).or_default() += st.play_count;
                    }
                }
                let mut pairs: Vec<(AlbumId, u64)> = agg.into_iter().collect();
                pairs.sort_by_key(|a| std::cmp::Reverse(a.1));
                let ordered_ids: Vec<AlbumId> = pairs.into_iter().map(|(id, _)| id).collect();
                self.query_ordered_candidate_albums(allowed_track_ids, query, &ordered_ids)
                    .await
            }
            AlbumListType::Recent => {
                let active_played_tracks = self
                    .filter_active_bound_played_tracks(allowed_track_ids, play_stats)
                    .await?;
                let mut agg: HashMap<AlbumId, Timestamp> = HashMap::new();
                for st in play_stats {
                    if !active_played_tracks.contains(&st.track_id) {
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
                let ordered_ids: Vec<AlbumId> = pairs.into_iter().map(|(id, _)| id).collect();
                self.query_ordered_candidate_albums(allowed_track_ids, query, &ordered_ids)
                    .await
            }
            _ => {
                let mut db = self.db.clone();
                let total = {
                    let mut b = SqlBuilder::default();
                    let mut conds = Vec::new();
                    append_album_query_conditions(&mut b, &mut conds, allowed_track_ids, query);
                    let where_sql = conds.join(" AND ");
                    let sql = format!(
                        "SELECT COUNT(DISTINCT a.id) FROM music_albums a \
                         INNER JOIN music_tracks t ON t.album_id = a.id WHERE {where_sql}"
                    );
                    let rows = b.exec_rows(sql, &mut db).await?;
                    rows.first()
                        .and_then(|r| row_fields(r).ok())
                        .and_then(|f| f.first())
                        .and_then(|v| val_i64(v).ok())
                        .and_then(|n| u64::try_from(n.max(0)).ok())
                        .unwrap_or(0)
                };
                if total == 0 {
                    return Ok(PagedResult {
                        items: Vec::new(),
                        total: 0,
                        page,
                        page_size: size,
                    });
                }

                let order_sql = album_order_by_sql(query);
                let mut b = SqlBuilder::default();
                let mut conds = Vec::new();
                append_album_query_conditions(&mut b, &mut conds, allowed_track_ids, query);
                let where_sql = conds.join(" AND ");
                let p_limit = b.bind_val(i64::from(size));
                let p_offset = b.bind_val(i64::from(query.offset));
                let sql = format!(
                    "SELECT {PROJECTED_ALBUM_SELECT_COLUMNS} FROM music_albums a \
                     INNER JOIN music_tracks t ON t.album_id = a.id \
                     WHERE {where_sql} GROUP BY a.id \
                     ORDER BY {order_sql} LIMIT {p_limit} OFFSET {p_offset}"
                );
                let rows = b.exec_rows(sql, &mut db).await?;
                let items = rows
                    .iter()
                    .map(row_to_projected_album)
                    .collect::<DomainResult<Vec<_>>>()?;
                Ok(PagedResult {
                    items,
                    total,
                    page,
                    page_size: size,
                })
            }
        }
    }

    async fn filter_active_bound_played_tracks(
        &self,
        allowed_track_ids: &HashSet<TrackId>,
        play_stats: &[TrackPlayStat],
    ) -> DomainResult<HashSet<TrackId>> {
        let candidate_ids: HashSet<TrackId> = play_stats
            .iter()
            .map(|s| s.track_id)
            .filter(|tid| allowed_track_ids.contains(tid))
            .collect();
        if candidate_ids.is_empty() {
            return Ok(HashSet::new());
        }
        let mut db = self.db.clone();
        let mut b = SqlBuilder::default();
        let scope = b.bind_track_scope("t.id", &candidate_ids);
        let sql = format!("SELECT t.id FROM music_tracks t WHERE t.is_missing = 0 AND {scope}");
        let rows = b.exec_rows(sql, &mut db).await?;
        let mut out = HashSet::with_capacity(rows.len());
        for row in &rows {
            let f = row_fields(row)?;
            if let Some(first) = f.first() {
                out.insert(TrackId(val_uuid(first)?));
            }
        }
        Ok(out)
    }

    async fn query_ordered_candidate_albums(
        &self,
        allowed_track_ids: &HashSet<TrackId>,
        query: &AlbumListQuery,
        ordered_ids: &[AlbumId],
    ) -> DomainResult<PagedResult<Album>> {
        let size = query.size.max(1);
        let page = (query.offset / size) + 1;
        if ordered_ids.is_empty() {
            return Ok(PagedResult {
                items: Vec::new(),
                total: 0,
                page,
                page_size: size,
            });
        }
        let unique_album_ids: HashSet<AlbumId> = ordered_ids.iter().copied().collect();
        let mut db = self.db.clone();
        let mut b = SqlBuilder::default();
        let mut conds = Vec::new();
        append_album_query_conditions(&mut b, &mut conds, allowed_track_ids, query);
        conds.push(b.bind_uuid_set(
            "a.id",
            unique_album_ids.iter().map(|id| id.0),
            unique_album_ids.len(),
        ));
        let where_sql = conds.join(" AND ");
        let sql = format!(
            "SELECT {PROJECTED_ALBUM_SELECT_COLUMNS} FROM music_albums a \
             INNER JOIN music_tracks t ON t.album_id = a.id \
             WHERE {where_sql} GROUP BY a.id"
        );
        let rows = b.exec_rows(sql, &mut db).await?;
        let mut lookup: HashMap<AlbumId, Album> = HashMap::with_capacity(rows.len());
        for row in &rows {
            let alb = row_to_projected_album(row)?;
            lookup.insert(alb.id, alb);
        }
        let mut ordered_matched = Vec::new();
        let mut seen = HashSet::new();
        for id in ordered_ids {
            if seen.insert(*id)
                && let Some(alb) = lookup.get(id)
            {
                ordered_matched.push(alb.clone());
            }
        }
        let total = ordered_matched.len() as u64;
        let items = ordered_matched
            .into_iter()
            .skip(query.offset as usize)
            .take(size as usize)
            .collect();
        Ok(PagedResult {
            items,
            total,
            page,
            page_size: size,
        })
    }

    pub(super) async fn project_bound_albums_by_ids_impl(
        &self,
        allowed_track_ids: &HashSet<TrackId>,
        album_ids: &[AlbumId],
        library_id: Option<LibraryId>,
    ) -> DomainResult<Vec<Album>> {
        if allowed_track_ids.is_empty() || album_ids.is_empty() {
            return Ok(Vec::new());
        }
        let unique_ids: HashSet<AlbumId> = album_ids.iter().copied().collect();
        let mut db = self.db.clone();
        let mut b = SqlBuilder::default();
        let mut conds = vec![
            "t.is_missing = 0".to_string(),
            b.bind_track_scope("t.id", allowed_track_ids),
            b.bind_uuid_set("a.id", unique_ids.iter().map(|id| id.0), unique_ids.len()),
        ];
        if let Some(lid) = library_id {
            let p = b.bind_uuid(lid.0);
            conds.push(format!("a.library_id = {p}"));
        }
        let where_sql = conds.join(" AND ");
        let sql = format!(
            "SELECT {PROJECTED_ALBUM_SELECT_COLUMNS} FROM music_albums a \
             INNER JOIN music_tracks t ON t.album_id = a.id \
             WHERE {where_sql} GROUP BY a.id"
        );
        let rows = b.exec_rows(sql, &mut db).await?;
        let mut lookup: HashMap<AlbumId, Album> = HashMap::with_capacity(rows.len());
        for row in &rows {
            let alb = row_to_projected_album(row)?;
            lookup.insert(alb.id, alb);
        }
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        for id in album_ids {
            if seen.insert(*id)
                && let Some(alb) = lookup.get(id)
            {
                out.push(alb.clone());
            }
        }
        Ok(out)
    }

    pub(super) async fn list_bound_albums_by_artist_impl(
        &self,
        allowed_track_ids: &HashSet<TrackId>,
        artist_id: ArtistId,
    ) -> DomainResult<Vec<Album>> {
        if allowed_track_ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut db = self.db.clone();
        let mut b = SqlBuilder::default();
        let p_artist = b.bind_uuid(artist_id.0);
        let scope = b.bind_track_scope("t.id", allowed_track_ids);
        let sql = format!(
            "SELECT {PROJECTED_ALBUM_SELECT_COLUMNS} FROM music_albums a \
             INNER JOIN music_tracks t ON t.album_id = a.id \
             WHERE a.album_artist_id = {p_artist} AND t.is_missing = 0 AND {scope} \
             GROUP BY a.id \
             ORDER BY a.year DESC, LOWER(a.title) ASC"
        );
        let rows = b.exec_rows(sql, &mut db).await?;
        rows.iter().map(row_to_projected_album).collect()
    }

    pub(super) async fn query_bound_artists_impl(
        &self,
        allowed_track_ids: &HashSet<TrackId>,
        query: &ArtistQuery,
    ) -> DomainResult<PagedResult<Artist>> {
        let size = query.size.max(1);
        let page = (query.offset / size) + 1;
        if allowed_track_ids.is_empty() {
            return Ok(PagedResult {
                items: Vec::new(),
                total: 0,
                page,
                page_size: size,
            });
        }
        let mut db = self.db.clone();

        let total = {
            let mut b = SqlBuilder::default();
            let cte = build_artist_agg_cte(&mut b, allowed_track_ids, query.library_id);
            let where_sql = build_artist_filter_where(&mut b, query);
            let sql = format!(
                "{cte} SELECT COUNT(*) FROM music_artists ar \
                 INNER JOIN artist_agg agg ON agg.aid = ar.id {where_sql}"
            );
            let rows = b.exec_rows(sql, &mut db).await?;
            rows.first()
                .and_then(|r| row_fields(r).ok())
                .and_then(|f| f.first())
                .and_then(|v| val_i64(v).ok())
                .and_then(|n| u64::try_from(n.max(0)).ok())
                .unwrap_or(0)
        };
        if total == 0 {
            return Ok(PagedResult {
                items: Vec::new(),
                total: 0,
                page,
                page_size: size,
            });
        }

        let mut b = SqlBuilder::default();
        let cte = build_artist_agg_cte(&mut b, allowed_track_ids, query.library_id);
        let where_sql = build_artist_filter_where(&mut b, query);
        let p_limit = b.bind_val(i64::from(size));
        let p_offset = b.bind_val(i64::from(query.offset));
        let sql = format!(
            "{cte} SELECT ar.id, ar.name, agg.user_is_album_artist, agg.user_is_track_artist, \
             agg.user_album_count, agg.user_track_count, ar.cover_key, ar.created_at, ar.updated_at \
             FROM music_artists ar INNER JOIN artist_agg agg ON agg.aid = ar.id \
             {where_sql} ORDER BY LOWER(ar.name) ASC LIMIT {p_limit} OFFSET {p_offset}"
        );
        let rows = b.exec_rows(sql, &mut db).await?;
        let items = rows
            .iter()
            .map(row_to_projected_artist)
            .collect::<DomainResult<Vec<_>>>()?;
        Ok(PagedResult {
            items,
            total,
            page,
            page_size: size,
        })
    }

    pub(super) async fn project_bound_artists_by_ids_impl(
        &self,
        allowed_track_ids: &HashSet<TrackId>,
        artist_ids: &[ArtistId],
        library_id: Option<LibraryId>,
    ) -> DomainResult<Vec<Artist>> {
        if allowed_track_ids.is_empty() || artist_ids.is_empty() {
            return Ok(Vec::new());
        }
        let unique_ids: HashSet<ArtistId> = artist_ids.iter().copied().collect();
        let mut db = self.db.clone();
        let mut b = SqlBuilder::default();
        let mut track_conds = vec![
            "t.is_missing = 0".to_string(),
            b.bind_track_scope("t.id", allowed_track_ids),
        ];
        if let Some(lid) = library_id {
            let p = b.bind_uuid(lid.0);
            track_conds.push(format!("t.library_id = {p}"));
        }
        let ar_tr_scope = b.bind_uuid_set(
            "t.artist_id",
            unique_ids.iter().map(|id| id.0),
            unique_ids.len(),
        );
        let ar_aa_scope = b.bind_uuid_set(
            "t.album_artist_id",
            unique_ids.iter().map(|id| id.0),
            unique_ids.len(),
        );
        track_conds.push(format!("({ar_tr_scope} OR {ar_aa_scope})"));
        let ar_id_scope =
            b.bind_uuid_set("ar.id", unique_ids.iter().map(|id| id.0), unique_ids.len());
        let track_where = track_conds.join(" AND ");
        let sql = format!(
            "WITH bound_tracks AS ( \
                SELECT t.id, t.album_id, t.artist_id, t.album_artist_id \
                FROM music_tracks t WHERE {track_where} \
             ), \
             artist_roles AS ( \
                SELECT artist_id AS aid, id AS tid, NULL AS albid, 1 AS is_tr, 0 AS is_aa FROM bound_tracks \
                UNION ALL \
                SELECT album_artist_id AS aid, id AS tid, album_id AS albid, 0 AS is_tr, 1 AS is_aa FROM bound_tracks \
             ), \
             artist_agg AS ( \
                SELECT aid, COUNT(DISTINCT tid) AS user_track_count, COUNT(DISTINCT albid) AS user_album_count, \
                       MAX(is_tr) AS user_is_track_artist, MAX(is_aa) AS user_is_album_artist \
                FROM artist_roles GROUP BY aid \
             ) \
             SELECT ar.id, ar.name, agg.user_is_album_artist, agg.user_is_track_artist, \
                    agg.user_album_count, agg.user_track_count, ar.cover_key, ar.created_at, ar.updated_at \
             FROM music_artists ar INNER JOIN artist_agg agg ON agg.aid = ar.id \
             WHERE {ar_id_scope}"
        );
        let rows = b.exec_rows(sql, &mut db).await?;
        let mut lookup: HashMap<ArtistId, Artist> = HashMap::with_capacity(rows.len());
        for row in &rows {
            let ar = row_to_projected_artist(row)?;
            lookup.insert(ar.id, ar);
        }
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        for id in artist_ids {
            if seen.insert(*id)
                && let Some(ar) = lookup.get(id)
            {
                out.push(ar.clone());
            }
        }
        Ok(out)
    }

    pub(super) async fn list_bound_genres_impl(
        &self,
        allowed_track_ids: &HashSet<TrackId>,
    ) -> DomainResult<Vec<GenreStat>> {
        if allowed_track_ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut db = self.db.clone();
        let mut b = SqlBuilder::default();
        let scope = b.bind_track_scope("t.id", allowed_track_ids);
        let sql = format!(
            "SELECT MIN(TRIM(t.genre)), COUNT(*), COUNT(DISTINCT t.album_id) \
             FROM music_tracks t \
             WHERE t.is_missing = 0 AND t.genre IS NOT NULL AND TRIM(t.genre) != '' AND {scope} \
             GROUP BY LOWER(TRIM(t.genre)) \
             ORDER BY COUNT(*) DESC, LOWER(MIN(TRIM(t.genre))) ASC"
        );
        let rows = b.exec_rows(sql, &mut db).await?;
        let mut out = Vec::with_capacity(rows.len());
        for row in &rows {
            let f = row_fields(row)?;
            if f.len() >= 3 {
                out.push(GenreStat {
                    name: val_string(&f[0])?,
                    song_count: u64::try_from(val_i64(&f[1])?.max(0)).unwrap_or(0),
                    album_count: u64::try_from(val_i64(&f[2])?.max(0)).unwrap_or(0),
                });
            }
        }
        Ok(out)
    }

    pub(super) async fn list_bound_tags_impl(
        &self,
        allowed_track_ids: &HashSet<TrackId>,
    ) -> DomainResult<Vec<MusicTagStat>> {
        if allowed_track_ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut db = self.db.clone();
        let mut b = SqlBuilder::default();
        let scope = b.bind_track_scope("t.id", allowed_track_ids);
        let sql = format!(
            "SELECT MIN(tt.tag), COUNT(*) \
             FROM music_track_tags tt INNER JOIN music_tracks t ON t.id = tt.track_id \
             WHERE t.is_missing = 0 AND {scope} \
             GROUP BY tt.tag_lower \
             ORDER BY COUNT(*) DESC, tt.tag_lower ASC"
        );
        let rows = b.exec_rows(sql, &mut db).await?;
        let mut out = Vec::with_capacity(rows.len());
        for row in &rows {
            let f = row_fields(row)?;
            if f.len() >= 2 {
                out.push(MusicTagStat {
                    name: val_string(&f[0])?,
                    song_count: u64::try_from(val_i64(&f[1])?.max(0)).unwrap_or(0),
                });
            }
        }
        Ok(out)
    }

    pub(super) async fn has_bound_active_track_impl(
        &self,
        allowed_track_ids: &HashSet<TrackId>,
        track_id: TrackId,
    ) -> DomainResult<bool> {
        if !allowed_track_ids.contains(&track_id) {
            return Ok(false);
        }
        let mut db = self.db.clone();
        let mut b = SqlBuilder::default();
        let p_id = b.bind_uuid(track_id.0);
        let sql = format!(
            "SELECT 1 FROM music_tracks t WHERE t.id = {p_id} AND t.is_missing = 0 LIMIT 1"
        );
        Ok(!b.exec_rows(sql, &mut db).await?.is_empty())
    }

    pub(super) async fn has_bound_active_album_impl(
        &self,
        allowed_track_ids: &HashSet<TrackId>,
        album_id: AlbumId,
    ) -> DomainResult<bool> {
        if allowed_track_ids.is_empty() {
            return Ok(false);
        }
        let mut db = self.db.clone();
        let mut b = SqlBuilder::default();
        let p_alb = b.bind_uuid(album_id.0);
        let scope = b.bind_track_scope("t.id", allowed_track_ids);
        let sql = format!(
            "SELECT 1 FROM music_tracks t \
             WHERE t.album_id = {p_alb} AND t.is_missing = 0 AND {scope} LIMIT 1"
        );
        Ok(!b.exec_rows(sql, &mut db).await?.is_empty())
    }

    pub(super) async fn has_bound_active_artist_impl(
        &self,
        allowed_track_ids: &HashSet<TrackId>,
        artist_id: ArtistId,
    ) -> DomainResult<bool> {
        if allowed_track_ids.is_empty() {
            return Ok(false);
        }
        let mut db = self.db.clone();
        let mut b = SqlBuilder::default();
        let p_ar = b.bind_uuid(artist_id.0);
        let scope = b.bind_track_scope("t.id", allowed_track_ids);
        let sql = format!(
            "SELECT 1 FROM music_tracks t \
             WHERE t.is_missing = 0 AND (t.artist_id = {p_ar} OR t.album_artist_id = {p_ar}) \
             AND {scope} LIMIT 1"
        );
        Ok(!b.exec_rows(sql, &mut db).await?.is_empty())
    }
}

fn append_album_query_conditions(
    b: &mut SqlBuilder,
    conds: &mut Vec<String>,
    allowed_track_ids: &HashSet<TrackId>,
    query: &AlbumListQuery,
) {
    conds.push("t.is_missing = 0".to_string());
    conds.push(b.bind_track_scope("t.id", allowed_track_ids));
    if let Some(lib_id) = query.library_id {
        let p = b.bind_uuid(lib_id.0);
        conds.push(format!("a.library_id = {p}"));
    }
    if let Some(genre_lower) = query
        .genre
        .as_ref()
        .map(|g| g.trim().to_lowercase())
        .filter(|g| !g.is_empty())
    {
        let p = b.bind_val(genre_lower);
        conds.push(format!("LOWER(TRIM(COALESCE(a.genre, ''))) = {p}"));
    }
    let (year_min, year_max) = match (query.from_year, query.to_year) {
        (Some(f), Some(t)) => (Some(f.min(t)), Some(f.max(t))),
        (f, t) => (f, t),
    };
    if let Some(min_y) = year_min {
        let p = b.bind_val(i64::from(min_y));
        conds.push(format!("(a.year IS NOT NULL AND a.year >= {p})"));
    }
    if let Some(max_y) = year_max {
        let p = b.bind_val(i64::from(max_y));
        conds.push(format!("(a.year IS NOT NULL AND a.year <= {p})"));
    }
    if let Some(kw_lower) = query
        .search
        .as_ref()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
    {
        let p = b.bind_val(kw_lower);
        conds.push(format!(
            "(INSTR(LOWER(a.title), {p}) > 0 OR INSTR(LOWER(a.album_artist_name), {p}) > 0)"
        ));
    }
}

fn album_order_by_sql(query: &AlbumListQuery) -> &'static str {
    match query.list_type {
        AlbumListType::Newest => "a.created_at DESC, LOWER(a.title) ASC",
        AlbumListType::AlphabeticalByName | AlbumListType::ByGenre => {
            "LOWER(a.title) ASC, a.id ASC"
        }
        AlbumListType::AlphabeticalByArtist => {
            "LOWER(a.album_artist_name) ASC, LOWER(a.title) ASC, a.id ASC"
        }
        AlbumListType::ByYear => {
            let rev = query
                .from_year
                .zip(query.to_year)
                .is_some_and(|(f, t)| f > t);
            if rev {
                "a.year DESC, LOWER(a.title) DESC"
            } else {
                "a.year ASC, LOWER(a.title) ASC"
            }
        }
        AlbumListType::Random => "RANDOM()",
        AlbumListType::Starred | AlbumListType::Frequent | AlbumListType::Recent => {
            "a.created_at DESC"
        }
    }
}

fn build_artist_agg_cte(
    b: &mut SqlBuilder,
    allowed_track_ids: &HashSet<TrackId>,
    library_id: Option<LibraryId>,
) -> String {
    let mut track_conds = vec![
        "t.is_missing = 0".to_string(),
        b.bind_track_scope("t.id", allowed_track_ids),
    ];
    if let Some(lid) = library_id {
        let p = b.bind_uuid(lid.0);
        track_conds.push(format!("t.library_id = {p}"));
    }
    let track_where = track_conds.join(" AND ");
    format!(
        "WITH bound_tracks AS ( \
            SELECT t.id, t.album_id, t.artist_id, t.album_artist_id \
            FROM music_tracks t WHERE {track_where} \
         ), \
         artist_roles AS ( \
            SELECT artist_id AS aid, id AS tid, NULL AS albid, 1 AS is_tr, 0 AS is_aa FROM bound_tracks \
            UNION ALL \
            SELECT album_artist_id AS aid, id AS tid, album_id AS albid, 0 AS is_tr, 1 AS is_aa FROM bound_tracks \
         ), \
         artist_agg AS ( \
            SELECT aid, COUNT(DISTINCT tid) AS user_track_count, COUNT(DISTINCT albid) AS user_album_count, \
                   MAX(is_tr) AS user_is_track_artist, MAX(is_aa) AS user_is_album_artist \
            FROM artist_roles GROUP BY aid \
         )"
    )
}

fn build_artist_filter_where(b: &mut SqlBuilder, query: &ArtistQuery) -> String {
    let mut conds = Vec::new();
    match query.role_filter {
        ArtistRoleFilter::All => {}
        ArtistRoleFilter::AlbumArtist => conds.push("agg.user_is_album_artist = 1".to_string()),
        ArtistRoleFilter::TrackArtist => conds.push("agg.user_is_track_artist = 1".to_string()),
    }
    if let Some(kw_lower) = query
        .search
        .as_ref()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
    {
        let p = b.bind_val(kw_lower);
        conds.push(format!("INSTR(LOWER(ar.name), {p}) > 0"));
    }
    if conds.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conds.join(" AND "))
    }
}

use std::collections::{HashMap, HashSet};

use jiff::Timestamp;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::domain::library::LibraryId;

use super::ToastyMusicCatalogRepository;
use super::sql::{
    PROJECTED_ALBUM_SELECT_COLUMNS, SqlBuilder, row_fields, row_to_projected_album,
    row_to_projected_artist, val_i64, val_uuid,
};
use crate::domain::catalog::{
    Album, AlbumId, AlbumListQuery, AlbumListType, Artist, ArtistId, ArtistQuery, ArtistRoleFilter,
    PagedResult, TrackId,
};
use crate::domain::collection::{StarTargetKind, StarredEntry, TrackPlayStat};

impl ToastyMusicCatalogRepository {
    pub(super) async fn query_bound_albums_impl(
        &self,
        allowed_track_ids: &HashSet<TrackId>,
        query: &AlbumListQuery,
        starred_ids: &[StarredEntry],
        play_stats: &[TrackPlayStat],
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
                    let mut b = SqlBuilder::new(&db);
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

                let mut b = SqlBuilder::new(&db);
                let order_sql = album_order_by_sql(&b, query);
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
        let mut b = SqlBuilder::new(&db);
        let not_missing = b.not_missing("t.is_missing");
        let scope = b.bind_track_scope("t.id", &candidate_ids);
        let sql = format!("SELECT t.id FROM music_tracks t WHERE {not_missing} AND {scope}");
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
        let mut b = SqlBuilder::new(&db);
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
        let mut b = SqlBuilder::new(&db);
        let mut conds = vec![
            b.not_missing("t.is_missing"),
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
        let mut b = SqlBuilder::new(&db);
        let p_artist = b.bind_uuid(artist_id.0);
        let not_missing = b.not_missing("t.is_missing");
        let scope = b.bind_track_scope("t.id", allowed_track_ids);
        let sql = format!(
            "SELECT {PROJECTED_ALBUM_SELECT_COLUMNS} FROM music_albums a \
             INNER JOIN music_tracks t ON t.album_id = a.id \
             WHERE a.album_artist_id = {p_artist} AND {not_missing} AND {scope} \
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
            let mut b = SqlBuilder::new(&db);
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

        let mut b = SqlBuilder::new(&db);
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
        let mut b = SqlBuilder::new(&db);
        let mut track_conds = vec![
            b.not_missing("t.is_missing"),
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
}

fn append_album_query_conditions(
    b: &mut SqlBuilder,
    conds: &mut Vec<String>,
    allowed_track_ids: &HashSet<TrackId>,
    query: &AlbumListQuery,
) {
    conds.push(b.not_missing("t.is_missing"));
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
        conds.push(b.contains_any_lower(&["a.title", "a.album_artist_name"], &kw_lower));
    }
}

fn album_order_by_sql(b: &SqlBuilder, query: &AlbumListQuery) -> &'static str {
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
        AlbumListType::Random => b.random_order_sql(),
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
        b.not_missing("t.is_missing"),
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
        conds.push(b.contains_any_lower(&["ar.name"], &kw_lower));
    }
    if conds.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conds.join(" AND "))
    }
}

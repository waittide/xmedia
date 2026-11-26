use std::collections::{HashMap, HashSet};

use xmedia_core::domain::error::DomainResult;
use xmedia_core::domain::library::LibraryId;

use super::ToastyMusicCatalogRepository;
use super::sql::{SqlBuilder, row_fields, val_i64, val_string, val_uuid};
use crate::domain::catalog::{
    AlbumId, ArtistId, BoundCatalogCounts, GenreStat, MusicTagStat, TrackId,
};

impl ToastyMusicCatalogRepository {
    pub(super) async fn count_bound_tracks_by_library_impl(
        &self,
        allowed_track_ids: &HashSet<TrackId>,
    ) -> DomainResult<HashMap<LibraryId, u64>> {
        if allowed_track_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let mut db = self.db.clone();
        let mut b = SqlBuilder::new(&db);
        let not_missing = b.not_missing("t.is_missing");
        let scope = b.bind_track_scope("t.id", allowed_track_ids);
        let sql = format!(
            "SELECT t.library_id, COUNT(*) FROM music_tracks t \
             WHERE {not_missing} AND {scope} GROUP BY t.library_id"
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
            let mut b = SqlBuilder::new(&db);
            let not_missing = b.not_missing("t.is_missing");
            let scope = b.bind_track_scope("t.id", allowed_track_ids);
            let sql = format!(
                "SELECT t.id, t.album_id FROM music_tracks t WHERE {not_missing} AND {scope}"
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
            let mut b = SqlBuilder::new(&db);
            let not_missing = b.not_missing("t.is_missing");
            let scope = b.bind_track_scope("t.id", &active_track_ids);
            let sql = format!(
                "WITH bt AS ( \
                    SELECT id, library_id, album_id, artist_id, album_artist_id, genre \
                    FROM music_tracks t WHERE {not_missing} AND {scope} \
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

    pub(super) async fn list_bound_genres_impl(
        &self,
        allowed_track_ids: &HashSet<TrackId>,
    ) -> DomainResult<Vec<GenreStat>> {
        if allowed_track_ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut db = self.db.clone();
        let mut b = SqlBuilder::new(&db);
        let not_missing = b.not_missing("t.is_missing");
        let scope = b.bind_track_scope("t.id", allowed_track_ids);
        let sql = format!(
            "SELECT MIN(TRIM(t.genre)), COUNT(*), COUNT(DISTINCT t.album_id) \
             FROM music_tracks t \
             WHERE {not_missing} AND t.genre IS NOT NULL AND TRIM(t.genre) != '' AND {scope} \
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
        let mut b = SqlBuilder::new(&db);
        let not_missing = b.not_missing("t.is_missing");
        let scope = b.bind_track_scope("t.id", allowed_track_ids);
        let sql = format!(
            "SELECT MIN(tt.tag), COUNT(*) \
             FROM music_track_tags tt INNER JOIN music_tracks t ON t.id = tt.track_id \
             WHERE {not_missing} AND {scope} \
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
        let mut b = SqlBuilder::new(&db);
        let p_id = b.bind_uuid(track_id.0);
        let not_missing = b.not_missing("t.is_missing");
        let sql =
            format!("SELECT 1 FROM music_tracks t WHERE t.id = {p_id} AND {not_missing} LIMIT 1");
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
        let mut b = SqlBuilder::new(&db);
        let p_alb = b.bind_uuid(album_id.0);
        let not_missing = b.not_missing("t.is_missing");
        let scope = b.bind_track_scope("t.id", allowed_track_ids);
        let sql = format!(
            "SELECT 1 FROM music_tracks t \
             WHERE t.album_id = {p_alb} AND {not_missing} AND {scope} LIMIT 1"
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
        let mut b = SqlBuilder::new(&db);
        let not_missing = b.not_missing("t.is_missing");
        let p_ar1 = b.bind_uuid(artist_id.0);
        let p_ar2 = b.bind_uuid(artist_id.0);
        let scope = b.bind_track_scope("t.id", allowed_track_ids);
        let sql = format!(
            "SELECT 1 FROM music_tracks t \
             WHERE {not_missing} AND (t.artist_id = {p_ar1} OR t.album_artist_id = {p_ar2}) \
             AND {scope} LIMIT 1"
        );
        Ok(!b.exec_rows(sql, &mut db).await?.is_empty())
    }
}

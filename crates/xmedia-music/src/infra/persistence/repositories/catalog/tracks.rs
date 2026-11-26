use std::collections::{HashMap, HashSet};

use uuid::Uuid;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::domain::library::LibraryId;

use super::ToastyMusicCatalogRepository;
use super::sql::{
    SqlBuilder, TRACK_SELECT_COLUMNS, append_track_query_conditions, row_fields, row_to_track,
    track_order_by_sql, val_i64,
};
use crate::domain::catalog::{AlbumId, ArtistId, PagedResult, Track, TrackId, TrackQuery};
use crate::infra::persistence::models::{MusicTrackModel, MusicTrackTagModel};
use crate::infra::persistence::repositories::db_err;

impl ToastyMusicCatalogRepository {
    pub(super) async fn find_track_by_id_impl(&self, id: TrackId) -> DomainResult<Option<Track>> {
        let mut db = self.db.clone();
        let row = MusicTrackModel::filter_by_id(id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(row.map(Self::track_to_domain))
    }

    pub(super) async fn find_tracks_by_ids_impl(
        &self,
        ids: &[TrackId],
    ) -> DomainResult<Vec<Track>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut db = self.db.clone();
        let mut out = Vec::with_capacity(ids.len());
        for chunk in ids.chunks(500) {
            let raw_ids: Vec<Uuid> = chunk.iter().map(|i| i.0).collect();
            let rows = MusicTrackModel::filter(MusicTrackModel::fields().id().in_list(raw_ids))
                .exec(&mut db)
                .await
                .map_err(db_err)?;
            out.extend(rows.into_iter().map(Self::track_to_domain));
        }
        Ok(out)
    }

    pub(super) async fn find_tracks_by_cover_key_impl(
        &self,
        cover_key: &str,
    ) -> DomainResult<Vec<Track>> {
        let clean = cover_key.trim();
        if clean.is_empty() {
            return Ok(Vec::new());
        }
        let mut db = self.db.clone();
        let rows = MusicTrackModel::filter_by_cover_key(clean.to_string())
            .filter(MusicTrackModel::fields().is_missing().eq(false))
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(rows.into_iter().map(Self::track_to_domain).collect())
    }

    pub(super) async fn list_tracks_by_library_impl(
        &self,
        library_id: LibraryId,
    ) -> DomainResult<Vec<Track>> {
        let mut db = self.db.clone();
        let rows = MusicTrackModel::filter_by_library_id(library_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(rows.into_iter().map(Self::track_to_domain).collect())
    }

    pub(super) async fn list_all_active_track_ids_impl(&self) -> DomainResult<Vec<TrackId>> {
        let mut db = self.db.clone();
        let rows = MusicTrackModel::filter_by_is_missing(false)
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(rows.into_iter().map(|r| TrackId(r.id)).collect())
    }

    pub(super) async fn query_tracks_impl(
        &self,
        query: &TrackQuery,
    ) -> DomainResult<PagedResult<Track>> {
        self.query_tracks_scoped_sql(None, query).await
    }

    pub(super) async fn query_bound_tracks_impl(
        &self,
        allowed_track_ids: &HashSet<TrackId>,
        query: &TrackQuery,
    ) -> DomainResult<PagedResult<Track>> {
        let page = query.page.max(1);
        let page_size = query.page_size.max(1);
        if allowed_track_ids.is_empty() {
            return Ok(PagedResult {
                items: Vec::new(),
                total: 0,
                page,
                page_size,
            });
        }
        self.query_tracks_scoped_sql(Some(allowed_track_ids), query)
            .await
    }

    async fn query_tracks_scoped_sql(
        &self,
        allowed: Option<&HashSet<TrackId>>,
        query: &TrackQuery,
    ) -> DomainResult<PagedResult<Track>> {
        let page = query.page.max(1);
        let page_size = query.page_size.max(1);
        let offset = u64::from(page - 1).saturating_mul(u64::from(page_size));
        let mut db = self.db.clone();

        let total = {
            let mut b = SqlBuilder::new(&db);
            let mut conds = Vec::new();
            append_track_query_conditions(&mut b, &mut conds, allowed, query);
            let where_sql = conds.join(" AND ");
            let sql = format!("SELECT COUNT(*) FROM music_tracks t WHERE {where_sql}");
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
                page_size,
            });
        }

        let order_sql = track_order_by_sql(query.sort_by, query.sort_order);
        let mut b = SqlBuilder::new(&db);
        let mut conds = Vec::new();
        append_track_query_conditions(&mut b, &mut conds, allowed, query);
        let where_sql = conds.join(" AND ");
        let p_limit = b.bind_val(i64::from(page_size));
        let p_offset = b.bind_val(i64::try_from(offset).unwrap_or(i64::MAX));
        let sql = format!(
            "SELECT {TRACK_SELECT_COLUMNS} FROM music_tracks t \
             WHERE {where_sql} ORDER BY {order_sql} LIMIT {p_limit} OFFSET {p_offset}"
        );
        let rows = b.exec_rows(sql, &mut db).await?;
        let items = rows
            .iter()
            .map(row_to_track)
            .collect::<DomainResult<Vec<_>>>()?;
        Ok(PagedResult {
            items,
            total,
            page,
            page_size,
        })
    }

    pub(super) async fn list_bound_tracks_offset_impl(
        &self,
        allowed_track_ids: &HashSet<TrackId>,
        query: &TrackQuery,
        offset: u32,
        limit: u32,
    ) -> DomainResult<Vec<Track>> {
        if allowed_track_ids.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let mut db = self.db.clone();
        let order_sql = track_order_by_sql(query.sort_by, query.sort_order);
        let mut b = SqlBuilder::new(&db);
        let mut conds = Vec::new();
        append_track_query_conditions(&mut b, &mut conds, Some(allowed_track_ids), query);
        let where_sql = conds.join(" AND ");
        let p_limit = b.bind_val(i64::from(limit));
        let p_offset = b.bind_val(i64::from(offset));
        let sql = format!(
            "SELECT {TRACK_SELECT_COLUMNS} FROM music_tracks t \
             WHERE {where_sql} ORDER BY {order_sql} LIMIT {p_limit} OFFSET {p_offset}"
        );
        let rows = b.exec_rows(sql, &mut db).await?;
        rows.iter().map(row_to_track).collect()
    }

    pub(super) async fn list_random_bound_tracks_impl(
        &self,
        allowed_track_ids: &HashSet<TrackId>,
        size: u32,
        library_id: Option<LibraryId>,
        genre: Option<&str>,
        from_year: Option<i32>,
        to_year: Option<i32>,
    ) -> DomainResult<Vec<Track>> {
        if allowed_track_ids.is_empty() || size == 0 {
            return Ok(Vec::new());
        }
        let query = TrackQuery {
            library_id,
            genre: genre.map(str::to_string),
            year_from: from_year,
            year_to: to_year,
            ..Default::default()
        };
        let mut db = self.db.clone();
        let mut b = SqlBuilder::new(&db);
        let mut conds = Vec::new();
        append_track_query_conditions(&mut b, &mut conds, Some(allowed_track_ids), &query);
        let where_sql = conds.join(" AND ");
        let rand_sql = b.random_order_sql();
        let p_limit = b.bind_val(i64::from(size.clamp(1, 500)));
        let sql = format!(
            "SELECT {TRACK_SELECT_COLUMNS} FROM music_tracks t \
             WHERE {where_sql} ORDER BY {rand_sql} LIMIT {p_limit}"
        );
        let rows = b.exec_rows(sql, &mut db).await?;
        rows.iter().map(row_to_track).collect()
    }

    pub(super) async fn list_bound_tracks_by_album_impl(
        &self,
        allowed_track_ids: &HashSet<TrackId>,
        album_id: AlbumId,
    ) -> DomainResult<Vec<Track>> {
        if allowed_track_ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut db = self.db.clone();
        let mut b = SqlBuilder::new(&db);
        let p_alb = b.bind_uuid(album_id.0);
        let not_missing = b.not_missing("t.is_missing");
        let sql = format!(
            "SELECT {TRACK_SELECT_COLUMNS} FROM music_tracks t \
             WHERE t.album_id = {p_alb} AND {not_missing} \
             ORDER BY COALESCE(t.disc_number, 1) ASC, COALESCE(t.track_number, 4294967295) ASC, LOWER(t.title) ASC"
        );
        let rows = b.exec_rows(sql, &mut db).await?;
        let mut out = Vec::new();
        for row in &rows {
            let tr = row_to_track(row)?;
            if allowed_track_ids.contains(&tr.id) {
                out.push(tr);
            }
        }
        Ok(out)
    }

    pub(super) async fn list_bound_tracks_by_artist_impl(
        &self,
        allowed_track_ids: &HashSet<TrackId>,
        artist_id: ArtistId,
    ) -> DomainResult<Vec<Track>> {
        if allowed_track_ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut db = self.db.clone();
        let mut b = SqlBuilder::new(&db);
        let not_missing = b.not_missing("t.is_missing");
        let p_ar1 = b.bind_uuid(artist_id.0);
        let p_ar2 = b.bind_uuid(artist_id.0);
        let sql = format!(
            "SELECT {TRACK_SELECT_COLUMNS} FROM music_tracks t \
             WHERE {not_missing} AND (t.artist_id = {p_ar1} OR t.album_artist_id = {p_ar2}) \
             ORDER BY LOWER(t.title) ASC"
        );
        let rows = b.exec_rows(sql, &mut db).await?;
        let mut out = Vec::new();
        for row in &rows {
            let tr = row_to_track(row)?;
            if allowed_track_ids.contains(&tr.id) {
                out.push(tr);
            }
        }
        Ok(out)
    }

    pub(super) async fn list_tracks_by_album_impl(
        &self,
        album_id: AlbumId,
    ) -> DomainResult<Vec<Track>> {
        let mut db = self.db.clone();
        let rows = MusicTrackModel::filter_by_album_id(album_id.0)
            .filter(MusicTrackModel::fields().is_missing().eq(false))
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        let mut tracks: Vec<Track> = rows.into_iter().map(Self::track_to_domain).collect();
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
        Ok(tracks)
    }

    pub(super) async fn list_tracks_by_artist_impl(
        &self,
        artist_id: ArtistId,
    ) -> DomainResult<Vec<Track>> {
        let mut db = self.db.clone();
        let by_artist = MusicTrackModel::filter_by_artist_id(artist_id.0)
            .filter(MusicTrackModel::fields().is_missing().eq(false))
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        let by_album_artist = MusicTrackModel::filter_by_album_artist_id(artist_id.0)
            .filter(MusicTrackModel::fields().is_missing().eq(false))
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        let mut dedup: HashMap<TrackId, Track> =
            HashMap::with_capacity(by_artist.len() + by_album_artist.len());
        for row in by_artist.into_iter().chain(by_album_artist) {
            let tr = Self::track_to_domain(row);
            dedup.entry(tr.id).or_insert(tr);
        }
        let mut tracks: Vec<Track> = dedup.into_values().collect();
        tracks.sort_by_key(|a| a.title.to_lowercase());
        Ok(tracks)
    }

    pub(super) async fn save_track_impl(&self, track: &Track) -> DomainResult<()> {
        let mut db = self.db.clone();
        let existing = MusicTrackModel::filter_by_id(track.id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;

        let track_num = track.track_number.map(i64::from);
        let disc_num = track.disc_number.map(i64::from);
        let year = track.year.map(i64::from);
        let duration_secs = i64::from(track.duration_secs);
        let bit_rate = track.bit_rate_kbps.map(i64::from);
        let sample_rate = track.sample_rate_hz.map(i64::from);
        let bit_depth = track.bit_depth.map(i64::from);
        let channels = track.channels.map(i64::from);
        let file_size = i64::try_from(track.file_size).unwrap_or(i64::MAX);

        if let Some(mut rec) = existing {
            rec.update()
                .library_id(track.library_id.0)
                .album_id(track.album_id.0)
                .artist_id(track.artist_id.0)
                .album_artist_id(track.album_artist_id.0)
                .title(track.title.clone())
                .artist_name(track.artist_name.clone())
                .album_artist_name(track.album_artist_name.clone())
                .album_title(track.album_title.clone())
                .track_number(track_num)
                .disc_number(disc_num)
                .year(year)
                .genre(track.genre.clone())
                .duration_secs(duration_secs)
                .bit_rate_kbps(bit_rate)
                .sample_rate_hz(sample_rate)
                .bit_depth(bit_depth)
                .channels(channels)
                .format(track.format.to_string())
                .file_size(file_size)
                .file_hash(track.file_hash.clone())
                .replay_gain(toasty::Json(track.replay_gain.clone()))
                .has_embedded_lyrics(track.has_embedded_lyrics)
                .has_sidecar_lyrics(track.has_sidecar_lyrics)
                .is_compilation(track.is_compilation)
                .cover_key(track.cover_key.clone())
                .source(toasty::Json(track.source.clone()))
                .is_missing(track.is_missing)
                .updated_at(track.updated_at)
                .exec(&mut db)
                .await
                .map_err(db_err)?;
        } else {
            toasty::create!(MusicTrackModel {
                id: track.id.0,
                library_id: track.library_id.0,
                album_id: track.album_id.0,
                artist_id: track.artist_id.0,
                album_artist_id: track.album_artist_id.0,
                title: track.title.clone(),
                artist_name: track.artist_name.clone(),
                album_artist_name: track.album_artist_name.clone(),
                album_title: track.album_title.clone(),
                track_number: track_num,
                disc_number: disc_num,
                year: year,
                genre: track.genre.clone(),
                duration_secs: duration_secs,
                bit_rate_kbps: bit_rate,
                sample_rate_hz: sample_rate,
                bit_depth: bit_depth,
                channels: channels,
                format: track.format.to_string(),
                file_size: file_size,
                file_hash: track.file_hash.clone(),
                replay_gain: toasty::Json(track.replay_gain.clone()),
                has_embedded_lyrics: track.has_embedded_lyrics,
                has_sidecar_lyrics: track.has_sidecar_lyrics,
                is_compilation: track.is_compilation,
                cover_key: track.cover_key.clone(),
                source: toasty::Json(track.source.clone()),
                is_missing: track.is_missing,
                created_at: track.created_at,
                updated_at: track.updated_at,
            })
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        }
        Ok(())
    }

    pub(super) async fn delete_track_impl(&self, id: TrackId) -> DomainResult<()> {
        let mut db = self.db.clone();
        MusicTrackTagModel::filter_by_track_id(id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        MusicTrackModel::filter_by_id(id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    pub(super) async fn delete_tracks_by_library_impl(
        &self,
        library_id: LibraryId,
    ) -> DomainResult<()> {
        let mut db = self.db.clone();
        let rows = MusicTrackModel::filter_by_library_id(library_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        for rec in rows {
            MusicTrackTagModel::filter_by_track_id(rec.id)
                .delete()
                .exec(&mut db)
                .await
                .map_err(db_err)?;
            rec.delete().exec(&mut db).await.map_err(db_err)?;
        }
        Ok(())
    }
}

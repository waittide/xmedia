use std::collections::{HashMap, HashSet};

use uuid::Uuid;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::domain::library::LibraryId;

use super::ToastyMusicCatalogRepository;
use crate::domain::catalog::{
    AlbumId, ArtistId, PagedResult, SortOrder, Track, TrackId, TrackQuery, TrackSortField,
};
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
        let mut db = self.db.clone();
        let mut builder =
            MusicTrackModel::all().filter(MusicTrackModel::fields().is_missing().eq(false));
        if let Some(lib_id) = query.library_id {
            builder = builder.filter(MusicTrackModel::fields().library_id().eq(lib_id.0));
        }
        if let Some(alb_id) = query.album_id {
            builder = builder.filter(MusicTrackModel::fields().album_id().eq(alb_id.0));
        }
        let rows = builder.exec(&mut db).await.map_err(db_err)?;

        let tag_track_set: Option<HashSet<TrackId>> = if let Some(tag_lower) = query
            .tag
            .as_ref()
            .map(|t| t.trim().to_lowercase())
            .filter(|t| !t.is_empty())
        {
            let tag_rows = MusicTrackTagModel::filter_by_tag_lower(tag_lower)
                .exec(&mut db)
                .await
                .map_err(db_err)?;
            Some(tag_rows.into_iter().map(|r| TrackId(r.track_id)).collect())
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

        let mut matched: Vec<Track> = rows
            .into_iter()
            .map(Self::track_to_domain)
            .filter(|t| {
                if query.exclude_online_tracked && t.source.is_online_tracked() {
                    return false;
                }
                if let Some(ref tset) = tag_track_set
                    && !tset.contains(&t.id)
                {
                    return false;
                }
                if let Some(ar_id) = query.artist_id
                    && t.artist_id != ar_id
                    && t.album_artist_id != ar_id
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

        matched.sort_by(|a, b| {
            let ord = match query.sort_by {
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
            match query.sort_order {
                SortOrder::Asc => ord,
                SortOrder::Desc => ord.reverse(),
            }
        });

        let total = matched.len() as u64;
        let page = query.page.max(1);
        let page_size = query.page_size.max(1);
        let start = ((page - 1) as usize).saturating_mul(page_size as usize);
        let items = matched
            .into_iter()
            .skip(start)
            .take(page_size as usize)
            .collect();

        Ok(PagedResult {
            items,
            total,
            page,
            page_size,
        })
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
                .format(track.format.as_str().to_string())
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
                format: track.format.as_str().to_string(),
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

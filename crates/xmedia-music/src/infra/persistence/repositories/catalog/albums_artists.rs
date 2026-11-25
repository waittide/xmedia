use std::collections::HashMap;

use uuid::Uuid;
use xmedia_core::domain::error::DomainResult;

use super::ToastyMusicCatalogRepository;
use crate::domain::catalog::{Album, AlbumId, Artist, ArtistId, Track};
use crate::infra::persistence::models::{MusicAlbumModel, MusicArtistModel, MusicTrackModel};
use crate::infra::persistence::repositories::db_err;

impl ToastyMusicCatalogRepository {
    pub(super) async fn find_album_by_id_impl(&self, id: AlbumId) -> DomainResult<Option<Album>> {
        let mut db = self.db.clone();
        let row = MusicAlbumModel::filter_by_id(id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(row.map(Self::album_to_domain))
    }

    pub(super) async fn find_album_by_key_impl(
        &self,
        album_key: &str,
    ) -> DomainResult<Option<Album>> {
        let mut db = self.db.clone();
        let row = MusicAlbumModel::filter_by_album_key(album_key)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(row.map(Self::album_to_domain))
    }

    pub(super) async fn find_albums_by_ids_impl(
        &self,
        ids: &[AlbumId],
    ) -> DomainResult<Vec<Album>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut db = self.db.clone();
        let mut out = Vec::with_capacity(ids.len());
        for chunk in ids.chunks(500) {
            let raw_ids: Vec<Uuid> = chunk.iter().map(|i| i.0).collect();
            let rows = MusicAlbumModel::filter(MusicAlbumModel::fields().id().in_list(raw_ids))
                .exec(&mut db)
                .await
                .map_err(db_err)?;
            out.extend(rows.into_iter().map(Self::album_to_domain));
        }
        Ok(out)
    }

    pub(super) async fn find_albums_by_cover_key_impl(
        &self,
        cover_key: &str,
    ) -> DomainResult<Vec<Album>> {
        let clean = cover_key.trim();
        if clean.is_empty() {
            return Ok(Vec::new());
        }
        let mut db = self.db.clone();
        let rows = MusicAlbumModel::filter_by_cover_key(clean.to_string())
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(rows.into_iter().map(Self::album_to_domain).collect())
    }

    pub(super) async fn save_album_impl(&self, album: &Album) -> DomainResult<()> {
        let mut db = self.db.clone();
        let existing = MusicAlbumModel::filter_by_id(album.id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        let year = album.year.map(i64::from);

        if let Some(mut rec) = existing {
            rec.update()
                .library_id(album.library_id.0)
                .album_key(album.album_key.clone())
                .title(album.title.clone())
                .album_artist_id(album.album_artist_id.0)
                .album_artist_name(album.album_artist_name.clone())
                .year(year)
                .genre(album.genre.clone())
                .disc_count(i64::from(album.disc_count))
                .track_count(i64::from(album.track_count))
                .duration_secs(i64::from(album.duration_secs))
                .is_compilation(album.is_compilation)
                .cover_key(album.cover_key.clone())
                .updated_at(album.updated_at)
                .exec(&mut db)
                .await
                .map_err(db_err)?;
        } else {
            toasty::create!(MusicAlbumModel {
                id: album.id.0,
                library_id: album.library_id.0,
                album_key: album.album_key.clone(),
                title: album.title.clone(),
                album_artist_id: album.album_artist_id.0,
                album_artist_name: album.album_artist_name.clone(),
                year: year,
                genre: album.genre.clone(),
                disc_count: i64::from(album.disc_count),
                track_count: i64::from(album.track_count),
                duration_secs: i64::from(album.duration_secs),
                is_compilation: album.is_compilation,
                cover_key: album.cover_key.clone(),
                created_at: album.created_at,
                updated_at: album.updated_at,
            })
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        }
        Ok(())
    }

    pub(super) async fn find_artist_by_id_impl(
        &self,
        id: ArtistId,
    ) -> DomainResult<Option<Artist>> {
        let mut db = self.db.clone();
        let row = MusicArtistModel::filter_by_id(id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(row.map(Self::artist_to_domain))
    }

    pub(super) async fn find_artist_by_name_impl(
        &self,
        name: &str,
    ) -> DomainResult<Option<Artist>> {
        let mut db = self.db.clone();
        let lower = name.trim().to_lowercase();
        let row = MusicArtistModel::filter_by_name_lower(&lower)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(row.map(Self::artist_to_domain))
    }

    pub(super) async fn find_artists_by_ids_impl(
        &self,
        ids: &[ArtistId],
    ) -> DomainResult<Vec<Artist>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut db = self.db.clone();
        let mut out = Vec::with_capacity(ids.len());
        for chunk in ids.chunks(500) {
            let raw_ids: Vec<Uuid> = chunk.iter().map(|i| i.0).collect();
            let rows = MusicArtistModel::filter(MusicArtistModel::fields().id().in_list(raw_ids))
                .exec(&mut db)
                .await
                .map_err(db_err)?;
            out.extend(rows.into_iter().map(Self::artist_to_domain));
        }
        Ok(out)
    }

    pub(super) async fn save_artist_impl(&self, artist: &Artist) -> DomainResult<()> {
        let mut db = self.db.clone();
        let existing = MusicArtistModel::filter_by_id(artist.id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        let lower = artist.name.trim().to_lowercase();

        if let Some(mut rec) = existing {
            rec.update()
                .name(artist.name.clone())
                .name_lower(lower)
                .is_album_artist(artist.is_album_artist)
                .is_track_artist(artist.is_track_artist)
                .album_count(i64::from(artist.album_count))
                .track_count(i64::from(artist.track_count))
                .cover_key(artist.cover_key.clone())
                .updated_at(artist.updated_at)
                .exec(&mut db)
                .await
                .map_err(db_err)?;
        } else {
            toasty::create!(MusicArtistModel {
                id: artist.id.0,
                name: artist.name.clone(),
                name_lower: lower,
                is_album_artist: artist.is_album_artist,
                is_track_artist: artist.is_track_artist,
                album_count: i64::from(artist.album_count),
                track_count: i64::from(artist.track_count),
                cover_key: artist.cover_key.clone(),
                created_at: artist.created_at,
                updated_at: artist.updated_at,
            })
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        }
        Ok(())
    }

    pub(super) async fn recompute_catalog_aggregates_impl(&self) -> DomainResult<()> {
        let mut db = self.db.clone();
        let active_tracks: Vec<Track> = MusicTrackModel::filter_by_is_missing(false)
            .exec(&mut db)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(Self::track_to_domain)
            .collect();

        #[derive(Default)]
        struct AlbumAcc {
            track_count: u32,
            duration_secs: u32,
            max_disc: u32,
            year: Option<i32>,
            genre: Option<String>,
            cover_key: Option<String>,
        }

        let mut album_acc: HashMap<AlbumId, AlbumAcc> = HashMap::new();
        for tr in &active_tracks {
            let acc = album_acc.entry(tr.album_id).or_default();
            acc.track_count += 1;
            acc.duration_secs = acc.duration_secs.saturating_add(tr.duration_secs);
            acc.max_disc = acc.max_disc.max(tr.disc_number.unwrap_or(1));
            if acc.year.is_none() {
                acc.year = tr.year;
            }
            if acc.genre.is_none() {
                acc.genre = tr.genre.clone();
            }
            if acc.cover_key.is_none() {
                acc.cover_key = tr.cover_key.clone();
            }
        }

        let all_albums = MusicAlbumModel::all().exec(&mut db).await.map_err(db_err)?;
        let mut active_albums_by_artist: HashMap<ArtistId, u32> = HashMap::new();

        for mut rec in all_albums {
            let album_id = AlbumId(rec.id);
            if let Some(acc) = album_acc.get(&album_id) {
                let new_track_count = i64::from(acc.track_count);
                let new_duration_secs = i64::from(acc.duration_secs);
                let new_disc_count = i64::from(acc.max_disc.max(1));
                let new_year = rec.year.or_else(|| acc.year.map(i64::from));
                let new_genre = rec.genre.clone().or_else(|| acc.genre.clone());
                let new_cover_key = rec.cover_key.clone().or_else(|| acc.cover_key.clone());
                *active_albums_by_artist
                    .entry(ArtistId(rec.album_artist_id))
                    .or_default() += 1;

                let changed = rec.track_count != new_track_count
                    || rec.duration_secs != new_duration_secs
                    || rec.disc_count != new_disc_count
                    || rec.year != new_year
                    || rec.genre != new_genre
                    || rec.cover_key != new_cover_key;
                if changed {
                    rec.update()
                        .track_count(new_track_count)
                        .duration_secs(new_duration_secs)
                        .disc_count(new_disc_count)
                        .year(new_year)
                        .genre(new_genre)
                        .cover_key(new_cover_key)
                        .exec(&mut db)
                        .await
                        .map_err(db_err)?;
                }
            } else {
                rec.delete().exec(&mut db).await.map_err(db_err)?;
            }
        }

        #[derive(Default)]
        struct ArtistAcc {
            track_count: u32,
            is_track_artist: bool,
            is_album_artist: bool,
            cover_key: Option<String>,
        }

        let mut artist_acc: HashMap<ArtistId, ArtistAcc> = HashMap::new();
        for tr in &active_tracks {
            {
                let a = artist_acc.entry(tr.artist_id).or_default();
                a.track_count += 1;
                a.is_track_artist = true;
                if a.cover_key.is_none() {
                    a.cover_key = tr.cover_key.clone();
                }
            }
            {
                let aa = artist_acc.entry(tr.album_artist_id).or_default();
                aa.is_album_artist = true;
                if tr.album_artist_id != tr.artist_id {
                    aa.track_count += 1;
                }
                if aa.cover_key.is_none() {
                    aa.cover_key = tr.cover_key.clone();
                }
            }
        }

        let all_artists = MusicArtistModel::all()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        for mut rec in all_artists {
            let ar_id = ArtistId(rec.id);
            let album_cnt = active_albums_by_artist.get(&ar_id).copied().unwrap_or(0);
            if let Some(acc) = artist_acc.get(&ar_id) {
                let new_album_count = i64::from(album_cnt);
                let new_track_count = i64::from(acc.track_count);
                let new_is_album_artist = acc.is_album_artist || album_cnt > 0;
                let new_is_track_artist = acc.is_track_artist;
                let new_cover_key = rec.cover_key.clone().or_else(|| acc.cover_key.clone());

                let changed = rec.album_count != new_album_count
                    || rec.track_count != new_track_count
                    || rec.is_album_artist != new_is_album_artist
                    || rec.is_track_artist != new_is_track_artist
                    || rec.cover_key != new_cover_key;
                if changed {
                    rec.update()
                        .album_count(new_album_count)
                        .track_count(new_track_count)
                        .is_album_artist(new_is_album_artist)
                        .is_track_artist(new_is_track_artist)
                        .cover_key(new_cover_key)
                        .exec(&mut db)
                        .await
                        .map_err(db_err)?;
                }
            } else {
                rec.delete().exec(&mut db).await.map_err(db_err)?;
            }
        }

        Ok(())
    }
}

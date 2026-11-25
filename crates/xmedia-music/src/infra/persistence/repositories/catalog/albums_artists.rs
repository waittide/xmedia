use std::collections::{HashMap, HashSet};

use uuid::Uuid;
use xmedia_core::domain::error::DomainResult;

use super::ToastyMusicCatalogRepository;
use crate::domain::catalog::{
    Album, AlbumId, AlbumListQuery, AlbumListType, Artist, ArtistId, ArtistQuery, ArtistRoleFilter,
    PagedResult, Track,
};
use crate::infra::persistence::models::{MusicAlbumModel, MusicArtistModel, MusicTrackModel};
use crate::infra::persistence::repositories::{db_err, shuffle_in_place};

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

    pub(super) async fn list_albums_impl(
        &self,
        query: &AlbumListQuery,
        starred_album_ids: Option<&[AlbumId]>,
        ordered_album_ids: Option<&[AlbumId]>,
    ) -> DomainResult<PagedResult<Album>> {
        let mut db = self.db.clone();
        let mut builder = MusicAlbumModel::all();
        if let Some(lib_id) = query.library_id {
            builder = builder.filter(MusicAlbumModel::fields().library_id().eq(lib_id.0));
        }
        let rows = builder.exec(&mut db).await.map_err(db_err)?;

        let kw_lower = query
            .search
            .as_ref()
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty());
        let genre_lower = query
            .genre
            .as_ref()
            .map(|g| g.trim().to_lowercase())
            .filter(|g| !g.is_empty());

        let (year_min, year_max) = match (query.from_year, query.to_year) {
            (Some(f), Some(t)) => (Some(f.min(t)), Some(f.max(t))),
            (f, t) => (f, t),
        };

        let mut pool: Vec<Album> = rows
            .into_iter()
            .map(Self::album_to_domain)
            .filter(|a| a.track_count > 0)
            .filter(|a| {
                if let Some(ref g) = genre_lower
                    && !a
                        .genre
                        .as_ref()
                        .is_some_and(|ag| ag.trim().to_lowercase() == *g)
                {
                    return false;
                }
                if let Some(min_y) = year_min
                    && a.year.is_none_or(|y| y < min_y)
                {
                    return false;
                }
                if let Some(max_y) = year_max
                    && a.year.is_none_or(|y| y > max_y)
                {
                    return false;
                }
                if let Some(ref kw) = kw_lower {
                    let hit = a.title.to_lowercase().contains(kw)
                        || a.album_artist_name.to_lowercase().contains(kw);
                    if !hit {
                        return false;
                    }
                }
                true
            })
            .collect();

        if let Some(star_ids) = starred_album_ids {
            let lookup: HashMap<AlbumId, Album> = pool.into_iter().map(|a| (a.id, a)).collect();
            pool = star_ids
                .iter()
                .filter_map(|id| lookup.get(id).cloned())
                .collect();
        } else if let Some(ord_ids) = ordered_album_ids {
            let lookup: HashMap<AlbumId, Album> = pool.into_iter().map(|a| (a.id, a)).collect();
            pool = ord_ids
                .iter()
                .filter_map(|id| lookup.get(id).cloned())
                .collect();
        } else {
            match query.list_type {
                AlbumListType::Newest => {
                    pool.sort_by_key(|a| std::cmp::Reverse(a.created_at));
                }
                AlbumListType::AlphabeticalByName | AlbumListType::ByGenre => {
                    pool.sort_by_key(|a| a.title.to_lowercase());
                }
                AlbumListType::AlphabeticalByArtist => {
                    pool.sort_by(|a, b| {
                        a.album_artist_name
                            .to_lowercase()
                            .cmp(&b.album_artist_name.to_lowercase())
                            .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
                    });
                }
                AlbumListType::ByYear => {
                    pool.sort_by(|a, b| {
                        let rev = query
                            .from_year
                            .zip(query.to_year)
                            .is_some_and(|(f, t)| f > t);
                        let ord = a
                            .year
                            .cmp(&b.year)
                            .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
                        if rev { ord.reverse() } else { ord }
                    });
                }
                AlbumListType::Random => {
                    shuffle_in_place(&mut pool);
                }
                _ => {
                    pool.sort_by_key(|a| std::cmp::Reverse(a.created_at));
                }
            }
        }

        let total = pool.len() as u64;
        let size = query.size.max(1);
        let items = pool
            .into_iter()
            .skip(query.offset as usize)
            .take(size as usize)
            .collect();
        let page = (query.offset / size) + 1;

        Ok(PagedResult {
            items,
            total,
            page,
            page_size: size,
        })
    }

    pub(super) async fn list_albums_by_artist_impl(
        &self,
        artist_id: ArtistId,
    ) -> DomainResult<Vec<Album>> {
        let mut db = self.db.clone();
        let rows = MusicAlbumModel::filter_by_album_artist_id(artist_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        let mut albums: Vec<Album> = rows
            .into_iter()
            .map(Self::album_to_domain)
            .filter(|a| a.track_count > 0)
            .collect();
        albums.sort_by(|a, b| {
            b.year
                .cmp(&a.year)
                .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
        });
        Ok(albums)
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

    pub(super) async fn list_artists_impl(
        &self,
        query: &ArtistQuery,
    ) -> DomainResult<PagedResult<Artist>> {
        let mut db = self.db.clone();
        let allowed_artist_ids: Option<HashSet<Uuid>> = if let Some(lib_id) = query.library_id {
            let lib_tracks = MusicTrackModel::filter_by_library_id(lib_id.0)
                .exec(&mut db)
                .await
                .map_err(db_err)?;
            let mut set = HashSet::new();
            for tr in lib_tracks {
                if !tr.is_missing {
                    set.insert(tr.artist_id);
                    set.insert(tr.album_artist_id);
                }
            }
            Some(set)
        } else {
            None
        };

        let rows = MusicArtistModel::all()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        let kw_lower = query
            .search
            .as_ref()
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty());

        let mut artists: Vec<Artist> = rows
            .into_iter()
            .map(Self::artist_to_domain)
            .filter(|ar| ar.track_count > 0 || ar.album_count > 0)
            .filter(|ar| {
                allowed_artist_ids
                    .as_ref()
                    .is_none_or(|ids| ids.contains(&ar.id.0))
            })
            .filter(|ar| match query.role_filter {
                ArtistRoleFilter::All => true,
                ArtistRoleFilter::AlbumArtist => ar.is_album_artist,
                ArtistRoleFilter::TrackArtist => ar.is_track_artist,
            })
            .filter(|ar| {
                if let Some(ref kw) = kw_lower {
                    ar.name.to_lowercase().contains(kw)
                } else {
                    true
                }
            })
            .collect();

        artists.sort_by_key(|a| a.name.to_lowercase());

        let total = artists.len() as u64;
        let size = query.size.max(1);
        let items = artists
            .into_iter()
            .skip(query.offset as usize)
            .take(size as usize)
            .collect();
        let page = (query.offset / size) + 1;

        Ok(PagedResult {
            items,
            total,
            page,
            page_size: size,
        })
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

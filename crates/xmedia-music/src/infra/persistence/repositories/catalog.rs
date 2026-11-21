use std::collections::{BTreeMap, HashMap, HashSet};

use async_trait::async_trait;
use jiff::Timestamp;
use toasty::Db;
use uuid::Uuid;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::domain::library::LibraryId;

use super::{db_err, shuffle_in_place};
use crate::domain::catalog::{
    Album, AlbumId, AlbumListQuery, AlbumListType, Artist, ArtistId, ArtistQuery, ArtistRoleFilter,
    AudioFormat, GenreStat, MusicTagStat, PagedResult, SearchQuery, SearchResult, SortOrder, Track,
    TrackId, TrackQuery, TrackSortField,
};
use crate::domain::repository::MusicCatalogRepository;
use crate::infra::persistence::models::{
    MusicAlbumModel, MusicArtistModel, MusicTrackModel, MusicTrackTagModel,
};

#[derive(Clone)]
pub struct ToastyMusicCatalogRepository {
    db: Db,
}

impl ToastyMusicCatalogRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    fn track_to_domain(m: MusicTrackModel) -> Track {
        let format = AudioFormat::parse(&m.format).unwrap_or(AudioFormat::Mp3);
        Track {
            id: TrackId(m.id),
            library_id: LibraryId(m.library_id),
            album_id: AlbumId(m.album_id),
            artist_id: ArtistId(m.artist_id),
            album_artist_id: ArtistId(m.album_artist_id),
            title: m.title,
            artist_name: m.artist_name,
            album_artist_name: m.album_artist_name,
            album_title: m.album_title,
            track_number: m.track_number.and_then(|n| u32::try_from(n).ok()),
            disc_number: m.disc_number.and_then(|n| u32::try_from(n).ok()),
            year: m.year.and_then(|y| i32::try_from(y).ok()),
            genre: m.genre,
            duration_secs: u32::try_from(m.duration_secs.max(0)).unwrap_or(0),
            bit_rate_kbps: m.bit_rate_kbps.and_then(|b| u32::try_from(b).ok()),
            sample_rate_hz: m.sample_rate_hz.and_then(|s| u32::try_from(s).ok()),
            bit_depth: m.bit_depth.and_then(|d| u16::try_from(d).ok()),
            channels: m.channels.and_then(|c| u8::try_from(c).ok()),
            format,
            file_size: u64::try_from(m.file_size.max(0)).unwrap_or(0),
            file_hash: m.file_hash,
            replay_gain: m.replay_gain.0,
            has_embedded_lyrics: m.has_embedded_lyrics,
            has_sidecar_lyrics: m.has_sidecar_lyrics,
            is_compilation: m.is_compilation,
            cover_key: m.cover_key,
            source: m.source.0,
            is_missing: m.is_missing,
            created_at: m.created_at,
            updated_at: m.updated_at,
        }
    }

    fn album_to_domain(m: MusicAlbumModel) -> Album {
        Album {
            id: AlbumId(m.id),
            library_id: LibraryId(m.library_id),
            album_key: m.album_key,
            title: m.title,
            album_artist_id: ArtistId(m.album_artist_id),
            album_artist_name: m.album_artist_name,
            year: m.year.and_then(|y| i32::try_from(y).ok()),
            genre: m.genre,
            disc_count: u32::try_from(m.disc_count.max(1)).unwrap_or(1),
            track_count: u32::try_from(m.track_count.max(0)).unwrap_or(0),
            duration_secs: u32::try_from(m.duration_secs.max(0)).unwrap_or(0),
            is_compilation: m.is_compilation,
            cover_key: m.cover_key,
            created_at: m.created_at,
            updated_at: m.updated_at,
        }
    }

    fn artist_to_domain(m: MusicArtistModel) -> Artist {
        Artist {
            id: ArtistId(m.id),
            name: m.name,
            is_album_artist: m.is_album_artist,
            is_track_artist: m.is_track_artist,
            album_count: u32::try_from(m.album_count.max(0)).unwrap_or(0),
            track_count: u32::try_from(m.track_count.max(0)).unwrap_or(0),
            cover_key: m.cover_key,
            created_at: m.created_at,
            updated_at: m.updated_at,
        }
    }
}

#[async_trait]
impl MusicCatalogRepository for ToastyMusicCatalogRepository {
    async fn find_track_by_id(&self, id: TrackId) -> DomainResult<Option<Track>> {
        let mut db = self.db.clone();
        let row = MusicTrackModel::filter_by_id(id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(row.map(Self::track_to_domain))
    }

    async fn find_tracks_by_ids(&self, ids: &[TrackId]) -> DomainResult<Vec<Track>> {
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

    async fn list_tracks_by_library(&self, library_id: LibraryId) -> DomainResult<Vec<Track>> {
        let mut db = self.db.clone();
        let rows = MusicTrackModel::filter_by_library_id(library_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(rows.into_iter().map(Self::track_to_domain).collect())
    }

    async fn count_tracks_by_library(&self, library_id: LibraryId) -> DomainResult<u64> {
        let mut db = self.db.clone();
        let rows = MusicTrackModel::filter_by_library_id(library_id.0)
            .filter(MusicTrackModel::fields().is_missing().eq(false))
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(rows.len() as u64)
    }

    async fn count_all_active_tracks(&self) -> DomainResult<u64> {
        let mut db = self.db.clone();
        let rows = MusicTrackModel::filter_by_is_missing(false)
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(rows.len() as u64)
    }

    async fn query_tracks(&self, query: &TrackQuery) -> DomainResult<PagedResult<Track>> {
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

    async fn list_tracks_by_album(&self, album_id: AlbumId) -> DomainResult<Vec<Track>> {
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

    async fn list_tracks_by_artist(&self, artist_id: ArtistId) -> DomainResult<Vec<Track>> {
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

    async fn list_random_tracks(
        &self,
        size: u32,
        library_id: Option<LibraryId>,
        genre: Option<&str>,
        from_year: Option<i32>,
        to_year: Option<i32>,
    ) -> DomainResult<Vec<Track>> {
        let mut db = self.db.clone();
        let mut builder =
            MusicTrackModel::all().filter(MusicTrackModel::fields().is_missing().eq(false));
        if let Some(lib_id) = library_id {
            builder = builder.filter(MusicTrackModel::fields().library_id().eq(lib_id.0));
        }
        let rows = builder.exec(&mut db).await.map_err(db_err)?;
        let genre_lower = genre
            .map(|g| g.trim().to_lowercase())
            .filter(|g| !g.is_empty());

        let mut pool: Vec<Track> = rows
            .into_iter()
            .map(Self::track_to_domain)
            .filter(|t| {
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

        shuffle_in_place(&mut pool);
        pool.truncate(size as usize);
        Ok(pool)
    }

    async fn list_tracks_by_genre(
        &self,
        genre: &str,
        offset: u32,
        size: u32,
        library_id: Option<LibraryId>,
    ) -> DomainResult<Vec<Track>> {
        let mut db = self.db.clone();
        let mut builder =
            MusicTrackModel::all().filter(MusicTrackModel::fields().is_missing().eq(false));
        if let Some(lib_id) = library_id {
            builder = builder.filter(MusicTrackModel::fields().library_id().eq(lib_id.0));
        }
        let rows = builder.exec(&mut db).await.map_err(db_err)?;
        let g_lower = genre.trim().to_lowercase();
        let mut tracks: Vec<Track> = rows
            .into_iter()
            .map(Self::track_to_domain)
            .filter(|t| {
                t.genre
                    .as_ref()
                    .is_some_and(|tg| tg.trim().to_lowercase() == g_lower)
            })
            .collect();
        tracks.sort_by_key(|a| a.title.to_lowercase());
        Ok(tracks
            .into_iter()
            .skip(offset as usize)
            .take(size as usize)
            .collect())
    }

    async fn save_track(&self, track: &Track) -> DomainResult<()> {
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

    async fn delete_track(&self, id: TrackId) -> DomainResult<()> {
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

    async fn delete_tracks_by_library(&self, library_id: LibraryId) -> DomainResult<()> {
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

    async fn find_album_by_id(&self, id: AlbumId) -> DomainResult<Option<Album>> {
        let mut db = self.db.clone();
        let row = MusicAlbumModel::filter_by_id(id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(row.map(Self::album_to_domain))
    }

    async fn find_album_by_key(&self, album_key: &str) -> DomainResult<Option<Album>> {
        let mut db = self.db.clone();
        let row = MusicAlbumModel::filter_by_album_key(album_key)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(row.map(Self::album_to_domain))
    }

    async fn find_albums_by_ids(&self, ids: &[AlbumId]) -> DomainResult<Vec<Album>> {
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

    async fn list_albums(
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

    async fn list_albums_by_artist(&self, artist_id: ArtistId) -> DomainResult<Vec<Album>> {
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

    async fn save_album(&self, album: &Album) -> DomainResult<()> {
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

    async fn find_artist_by_id(&self, id: ArtistId) -> DomainResult<Option<Artist>> {
        let mut db = self.db.clone();
        let row = MusicArtistModel::filter_by_id(id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(row.map(Self::artist_to_domain))
    }

    async fn find_artist_by_name(&self, name: &str) -> DomainResult<Option<Artist>> {
        let mut db = self.db.clone();
        let lower = name.trim().to_lowercase();
        let row = MusicArtistModel::filter_by_name_lower(&lower)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(row.map(Self::artist_to_domain))
    }

    async fn find_artists_by_ids(&self, ids: &[ArtistId]) -> DomainResult<Vec<Artist>> {
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

    async fn list_artists(&self, query: &ArtistQuery) -> DomainResult<PagedResult<Artist>> {
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

    async fn save_artist(&self, artist: &Artist) -> DomainResult<()> {
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

    async fn list_genres(&self) -> DomainResult<Vec<GenreStat>> {
        let mut db = self.db.clone();
        let rows = MusicTrackModel::filter_by_is_missing(false)
            .exec(&mut db)
            .await
            .map_err(db_err)?;

        let mut map: BTreeMap<String, (String, u64, HashSet<Uuid>)> = BTreeMap::new();
        for tr in rows {
            if let Some(ref g) = tr.genre {
                let trimmed = g.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let entry = map
                    .entry(trimmed.to_lowercase())
                    .or_insert_with(|| (trimmed.to_string(), 0, HashSet::new()));
                entry.1 += 1;
                entry.2.insert(tr.album_id);
            }
        }

        let mut out: Vec<GenreStat> = map
            .into_values()
            .map(|(display, song_count, albums)| GenreStat {
                name: display,
                song_count,
                album_count: albums.len() as u64,
            })
            .collect();
        out.sort_by(|a, b| {
            b.song_count
                .cmp(&a.song_count)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        Ok(out)
    }

    async fn list_tags(&self) -> DomainResult<Vec<MusicTagStat>> {
        let mut db = self.db.clone();
        let active_ids: HashSet<Uuid> = MusicTrackModel::filter_by_is_missing(false)
            .exec(&mut db)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(|r| r.id)
            .collect();
        let rows = MusicTrackTagModel::all()
            .exec(&mut db)
            .await
            .map_err(db_err)?;

        let mut map: BTreeMap<String, (String, u64)> = BTreeMap::new();
        for row in rows {
            if !active_ids.contains(&row.track_id) {
                continue;
            }
            let entry = map.entry(row.tag_lower).or_insert_with(|| (row.tag, 0));
            entry.1 += 1;
        }

        let mut out: Vec<MusicTagStat> = map
            .into_values()
            .map(|(name, song_count)| MusicTagStat { name, song_count })
            .collect();
        out.sort_by(|a, b| {
            b.song_count
                .cmp(&a.song_count)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        Ok(out)
    }

    async fn count_active_tags(&self, active_track_ids: &HashSet<TrackId>) -> DomainResult<u64> {
        if active_track_ids.is_empty() {
            return Ok(0);
        }
        let mut db = self.db.clone();
        let rows = MusicTrackTagModel::all()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        let mut distinct = HashSet::new();
        for row in rows {
            if active_track_ids.contains(&TrackId(row.track_id)) {
                distinct.insert(row.tag_lower);
            }
        }
        Ok(distinct.len() as u64)
    }

    async fn get_tags_for_tracks(
        &self,
        track_ids: &[TrackId],
    ) -> DomainResult<HashMap<TrackId, Vec<String>>> {
        if track_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let mut db = self.db.clone();
        let mut map: HashMap<TrackId, Vec<String>> = HashMap::new();
        for chunk in track_ids.chunks(500) {
            let raw_ids: Vec<Uuid> = chunk.iter().map(|i| i.0).collect();
            let rows = MusicTrackTagModel::filter(
                MusicTrackTagModel::fields().track_id().in_list(raw_ids),
            )
            .exec(&mut db)
            .await
            .map_err(db_err)?;
            for row in rows {
                map.entry(TrackId(row.track_id)).or_default().push(row.tag);
            }
        }
        for list in map.values_mut() {
            list.sort_by_key(|t| t.to_lowercase());
        }
        Ok(map)
    }

    async fn set_track_tags(
        &self,
        track_id: TrackId,
        tags: Vec<String>,
    ) -> DomainResult<Vec<String>> {
        let mut db = self.db.clone();
        MusicTrackTagModel::filter_by_track_id(track_id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;

        let now = Timestamp::now();
        let mut seen = HashSet::new();
        let mut saved = Vec::new();
        for raw in tags {
            let clean = raw.trim();
            if clean.is_empty() {
                continue;
            }
            let lower = clean.to_lowercase();
            if !seen.insert(lower.clone()) {
                continue;
            }
            let key = format!("{}:{}", track_id.0, lower);
            toasty::create!(MusicTrackTagModel {
                composite_key: key,
                track_id: track_id.0,
                tag_lower: lower,
                tag: clean.to_string(),
                created_at: now,
            })
            .exec(&mut db)
            .await
            .map_err(db_err)?;
            saved.push(clean.to_string());
        }
        saved.sort_by_key(|t| t.to_lowercase());
        Ok(saved)
    }

    async fn search_catalog(&self, query: &SearchQuery) -> DomainResult<SearchResult> {
        let mut db = self.db.clone();
        let q_lower = query.query.trim().trim_matches('"').to_lowercase();

        let artist_rows = MusicArtistModel::all()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        let mut artists: Vec<Artist> = artist_rows
            .into_iter()
            .map(Self::artist_to_domain)
            .filter(|a| a.track_count > 0 || a.album_count > 0)
            .filter(|a| q_lower.is_empty() || a.name.to_lowercase().contains(&q_lower))
            .collect();
        artists.sort_by_key(|a| a.name.to_lowercase());
        let artists = artists
            .into_iter()
            .skip(query.artist_offset as usize)
            .take(query.artist_count as usize)
            .collect();

        let mut album_builder = MusicAlbumModel::all();
        if let Some(lib_id) = query.library_id {
            album_builder =
                album_builder.filter(MusicAlbumModel::fields().library_id().eq(lib_id.0));
        }
        let album_rows = album_builder.exec(&mut db).await.map_err(db_err)?;
        let mut albums: Vec<Album> = album_rows
            .into_iter()
            .map(Self::album_to_domain)
            .filter(|a| a.track_count > 0)
            .filter(|a| {
                q_lower.is_empty()
                    || a.title.to_lowercase().contains(&q_lower)
                    || a.album_artist_name.to_lowercase().contains(&q_lower)
            })
            .collect();
        albums.sort_by_key(|a| a.title.to_lowercase());
        let albums = albums
            .into_iter()
            .skip(query.album_offset as usize)
            .take(query.album_count as usize)
            .collect();

        let mut track_builder =
            MusicTrackModel::all().filter(MusicTrackModel::fields().is_missing().eq(false));
        if let Some(lib_id) = query.library_id {
            track_builder =
                track_builder.filter(MusicTrackModel::fields().library_id().eq(lib_id.0));
        }
        let track_rows = track_builder.exec(&mut db).await.map_err(db_err)?;
        let mut tracks: Vec<Track> = track_rows
            .into_iter()
            .map(Self::track_to_domain)
            .filter(|t| {
                q_lower.is_empty()
                    || t.title.to_lowercase().contains(&q_lower)
                    || t.artist_name.to_lowercase().contains(&q_lower)
                    || t.album_title.to_lowercase().contains(&q_lower)
            })
            .collect();
        tracks.sort_by_key(|t| t.title.to_lowercase());
        let tracks = tracks
            .into_iter()
            .skip(query.song_offset as usize)
            .take(query.song_count as usize)
            .collect();

        Ok(SearchResult {
            artists,
            albums,
            tracks,
        })
    }

    async fn recompute_catalog_aggregates(&self) -> DomainResult<()> {
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

use std::collections::{BTreeMap, HashMap, HashSet};

use async_trait::async_trait;
use jiff::Timestamp;
use toasty::Db;
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::LibraryId;
use xmedia_core::domain::user::UserId;

use crate::domain::catalog::{
    Album, AlbumId, AlbumListQuery, AlbumListType, Artist, ArtistId, ArtistQuery, ArtistRoleFilter,
    AudioFormat, GenreStat, MusicTagStat, PagedResult, SearchQuery, SearchResult, SortOrder, Track,
    TrackId, TrackQuery, TrackSortField,
};
use crate::domain::collection::{
    AudioBookmark, PlayQueue, Playlist, PlaylistId, RatingEntry, ScrobbleEntry, StarTargetKind,
    StarredEntry, TrackPlayStat,
};
use crate::domain::repository::{MusicCatalogRepository, MusicCollectionRepository};

use super::models::{
    MusicAlbumModel, MusicArtistModel, MusicBookmarkModel, MusicPlayQueueModel, MusicPlayStatModel,
    MusicPlaylistModel, MusicRatingModel, MusicScrobbleModel, MusicStarredModel, MusicTrackModel,
    MusicTrackTagModel,
};

fn db_err(err: impl std::fmt::Display) -> DomainError {
    DomainError::Persistence(err.to_string())
}

fn shuffle_in_place<T>(slice: &mut [T]) {
    if slice.len() <= 1 {
        return;
    }
    let mut state = (Uuid::now_v7().as_u128() as u64)
        ^ (Timestamp::now().as_nanosecond() as u64)
        ^ 0x9E37_79B9_7F4A_7C15;
    for i in (1..slice.len()).rev() {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z = z ^ (z >> 31);
        let j = (z as usize) % (i + 1);
        slice.swap(i, j);
    }
}

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
        let raw_ids: Vec<Uuid> = ids.iter().map(|i| i.0).collect();
        let rows = MusicTrackModel::filter(MusicTrackModel::fields().id().in_list(raw_ids))
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(rows.into_iter().map(Self::track_to_domain).collect())
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

        let only_set: Option<HashSet<TrackId>> = query
            .only_ids
            .as_ref()
            .map(|ids| ids.iter().copied().collect());
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
                if let Some(ref set) = only_set
                    && !set.contains(&t.id)
                {
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
        let rows = MusicTrackModel::filter_by_is_missing(false)
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        let mut tracks: Vec<Track> = rows
            .into_iter()
            .map(Self::track_to_domain)
            .filter(|t| t.artist_id == artist_id || t.album_artist_id == artist_id)
            .collect();
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

        let title_lower = track.title.to_lowercase();
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
                .title_lower(title_lower)
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
                title_lower: title_lower,
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
        let raw_ids: Vec<Uuid> = ids.iter().map(|i| i.0).collect();
        let rows = MusicAlbumModel::filter(MusicAlbumModel::fields().id().in_list(raw_ids))
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(rows.into_iter().map(Self::album_to_domain).collect())
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
        let title_lower = album.title.to_lowercase();
        let year = album.year.map(i64::from);

        if let Some(mut rec) = existing {
            rec.update()
                .library_id(album.library_id.0)
                .album_key(album.album_key.clone())
                .title(album.title.clone())
                .title_lower(title_lower)
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
                title_lower: title_lower,
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

    async fn delete_album(&self, id: AlbumId) -> DomainResult<()> {
        let mut db = self.db.clone();
        MusicAlbumModel::filter_by_id(id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
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
        let raw_ids: Vec<Uuid> = ids.iter().map(|i| i.0).collect();
        let rows = MusicArtistModel::filter(MusicArtistModel::fields().id().in_list(raw_ids))
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(rows.into_iter().map(Self::artist_to_domain).collect())
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

    async fn delete_artist(&self, id: ArtistId) -> DomainResult<()> {
        let mut db = self.db.clone();
        MusicArtistModel::filter_by_id(id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
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

    async fn get_tags_for_tracks(
        &self,
        track_ids: &[TrackId],
    ) -> DomainResult<HashMap<TrackId, Vec<String>>> {
        if track_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let mut db = self.db.clone();
        let raw_ids: Vec<Uuid> = track_ids.iter().map(|i| i.0).collect();
        let rows =
            MusicTrackTagModel::filter(MusicTrackTagModel::fields().track_id().in_list(raw_ids))
                .exec(&mut db)
                .await
                .map_err(db_err)?;

        let mut map: HashMap<TrackId, Vec<String>> = HashMap::new();
        for row in rows {
            map.entry(TrackId(row.track_id)).or_default().push(row.tag);
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

        for rec in all_albums {
            let album_id = AlbumId(rec.id);
            if let Some(acc) = album_acc.get(&album_id) {
                let mut dom = Self::album_to_domain(rec);
                dom.track_count = acc.track_count;
                dom.duration_secs = acc.duration_secs;
                dom.disc_count = acc.max_disc.max(1);
                if dom.year.is_none() {
                    dom.year = acc.year;
                }
                if dom.genre.is_none() {
                    dom.genre = acc.genre.clone();
                }
                if dom.cover_key.is_none() {
                    dom.cover_key = acc.cover_key.clone();
                }
                *active_albums_by_artist
                    .entry(dom.album_artist_id)
                    .or_default() += 1;
                self.save_album(&dom).await?;
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
        for rec in all_artists {
            let ar_id = ArtistId(rec.id);
            let album_cnt = active_albums_by_artist.get(&ar_id).copied().unwrap_or(0);
            if let Some(acc) = artist_acc.get(&ar_id) {
                let mut dom = Self::artist_to_domain(rec);
                dom.album_count = album_cnt;
                dom.track_count = acc.track_count;
                dom.is_album_artist = acc.is_album_artist || album_cnt > 0;
                dom.is_track_artist = acc.is_track_artist;
                if dom.cover_key.is_none() {
                    dom.cover_key = acc.cover_key.clone();
                }
                self.save_artist(&dom).await?;
            } else {
                rec.delete().exec(&mut db).await.map_err(db_err)?;
            }
        }

        Ok(())
    }
}

#[derive(Clone)]
pub struct ToastyMusicCollectionRepository {
    db: Db,
}

impl ToastyMusicCollectionRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    fn playlist_to_domain(m: MusicPlaylistModel) -> Playlist {
        Playlist {
            id: PlaylistId(m.id),
            owner_id: UserId(m.owner_id),
            owner_username: m.owner_username,
            name: m.name,
            comment: m.comment,
            is_public: m.is_public,
            track_ids: m.track_ids.0,
            created_at: m.created_at,
            updated_at: m.updated_at,
        }
    }
}

#[async_trait]
impl MusicCollectionRepository for ToastyMusicCollectionRepository {
    async fn find_playlist_by_id(&self, id: PlaylistId) -> DomainResult<Option<Playlist>> {
        let mut db = self.db.clone();
        let row = MusicPlaylistModel::filter_by_id(id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(row.map(Self::playlist_to_domain))
    }

    async fn list_visible_playlists(&self, user_id: UserId) -> DomainResult<Vec<Playlist>> {
        let mut db = self.db.clone();
        let rows = MusicPlaylistModel::all()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        let mut out: Vec<Playlist> = rows
            .into_iter()
            .filter(|m| m.owner_id == user_id.0 || m.is_public)
            .map(Self::playlist_to_domain)
            .collect();
        out.sort_by_key(|p| std::cmp::Reverse(p.updated_at));
        Ok(out)
    }

    async fn list_owned_playlists(&self, owner_id: UserId) -> DomainResult<Vec<Playlist>> {
        let mut db = self.db.clone();
        let rows = MusicPlaylistModel::filter_by_owner_id(owner_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        let mut out: Vec<Playlist> = rows.into_iter().map(Self::playlist_to_domain).collect();
        out.sort_by_key(|p| std::cmp::Reverse(p.updated_at));
        Ok(out)
    }

    async fn save_playlist(&self, playlist: &Playlist) -> DomainResult<()> {
        let mut db = self.db.clone();
        let existing = MusicPlaylistModel::filter_by_id(playlist.id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;

        if let Some(mut rec) = existing {
            rec.update()
                .owner_username(playlist.owner_username.clone())
                .name(playlist.name.clone())
                .comment(playlist.comment.clone())
                .is_public(playlist.is_public)
                .track_ids(toasty::Json(playlist.track_ids.clone()))
                .updated_at(playlist.updated_at)
                .exec(&mut db)
                .await
                .map_err(db_err)?;
        } else {
            toasty::create!(MusicPlaylistModel {
                id: playlist.id.0,
                owner_id: playlist.owner_id.0,
                owner_username: playlist.owner_username.clone(),
                name: playlist.name.clone(),
                comment: playlist.comment.clone(),
                is_public: playlist.is_public,
                track_ids: toasty::Json(playlist.track_ids.clone()),
                created_at: playlist.created_at,
                updated_at: playlist.updated_at,
            })
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        }
        Ok(())
    }

    async fn delete_playlist(&self, id: PlaylistId) -> DomainResult<()> {
        let mut db = self.db.clone();
        MusicPlaylistModel::filter_by_id(id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    async fn delete_track_references(
        &self,
        track_id: TrackId,
        deleted_album_id: Option<AlbumId>,
        deleted_artist_ids: &[ArtistId],
    ) -> DomainResult<()> {
        let mut db = self.db.clone();
        for mut pl in MusicPlaylistModel::all()
            .exec(&mut db)
            .await
            .map_err(db_err)?
        {
            if pl.track_ids.0.contains(&track_id) {
                let filtered: Vec<TrackId> = pl
                    .track_ids
                    .0
                    .iter()
                    .copied()
                    .filter(|&id| id != track_id)
                    .collect();
                pl.update()
                    .track_ids(toasty::Json(filtered))
                    .updated_at(Timestamp::now())
                    .exec(&mut db)
                    .await
                    .map_err(db_err)?;
            }
        }
        for mut q in MusicPlayQueueModel::all()
            .exec(&mut db)
            .await
            .map_err(db_err)?
        {
            let has_in_list = q.track_ids.0.contains(&track_id);
            let is_current = q.current_track_id == Some(track_id.0);
            if has_in_list || is_current {
                let filtered: Vec<TrackId> = q
                    .track_ids
                    .0
                    .iter()
                    .copied()
                    .filter(|&id| id != track_id)
                    .collect();
                let next_current = if is_current {
                    filtered.first().map(|id| id.0)
                } else {
                    q.current_track_id
                };
                q.update()
                    .track_ids(toasty::Json(filtered))
                    .current_track_id(next_current)
                    .updated_at(Timestamp::now())
                    .exec(&mut db)
                    .await
                    .map_err(db_err)?;
            }
        }
        for row in MusicBookmarkModel::filter_by_track_id(track_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?
        {
            row.delete().exec(&mut db).await.map_err(db_err)?;
        }
        for row in MusicPlayStatModel::filter_by_track_id(track_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?
        {
            row.delete().exec(&mut db).await.map_err(db_err)?;
        }
        for row in MusicScrobbleModel::filter_by_track_id(track_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?
        {
            row.delete().exec(&mut db).await.map_err(db_err)?;
        }

        let mut remove_targets: Vec<(StarTargetKind, Uuid)> =
            vec![(StarTargetKind::Track, track_id.0)];
        if let Some(aid) = deleted_album_id {
            remove_targets.push((StarTargetKind::Album, aid.0));
        }
        for arid in deleted_artist_ids {
            remove_targets.push((StarTargetKind::Artist, arid.0));
        }
        for (kind, target_uuid) in remove_targets {
            for row in MusicStarredModel::filter_by_target_id(target_uuid)
                .exec(&mut db)
                .await
                .map_err(db_err)?
            {
                if row.kind == kind.as_str() {
                    row.delete().exec(&mut db).await.map_err(db_err)?;
                }
            }
            for row in MusicRatingModel::filter_by_target_id(target_uuid)
                .exec(&mut db)
                .await
                .map_err(db_err)?
            {
                if row.kind == kind.as_str() {
                    row.delete().exec(&mut db).await.map_err(db_err)?;
                }
            }
        }
        Ok(())
    }

    async fn set_starred(
        &self,
        user_id: UserId,
        kind: StarTargetKind,
        target_id: Uuid,
        starred_at: Timestamp,
    ) -> DomainResult<()> {
        let mut db = self.db.clone();
        let key = format!("{}:{}:{target_id}", user_id.0, kind.as_str());
        if MusicStarredModel::filter_by_star_key(&key)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?
            .is_none()
        {
            toasty::create!(MusicStarredModel {
                star_key: key,
                user_id: user_id.0,
                kind: kind.as_str().to_string(),
                target_id: target_id,
                starred_at: starred_at,
            })
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        }
        Ok(())
    }

    async fn unset_starred(
        &self,
        user_id: UserId,
        kind: StarTargetKind,
        target_id: Uuid,
    ) -> DomainResult<()> {
        let mut db = self.db.clone();
        let key = format!("{}:{}:{target_id}", user_id.0, kind.as_str());
        MusicStarredModel::filter_by_star_key(&key)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    async fn list_starred(
        &self,
        user_id: UserId,
        kind: Option<StarTargetKind>,
    ) -> DomainResult<Vec<StarredEntry>> {
        let mut db = self.db.clone();
        let rows = MusicStarredModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?;

        let mut out = Vec::new();
        for r in rows {
            let parsed = StarTargetKind::parse(&r.kind)?;
            if kind.is_none_or(|k| k == parsed) {
                out.push(StarredEntry {
                    user_id: UserId(r.user_id),
                    kind: parsed,
                    target_id: r.target_id,
                    starred_at: r.starred_at,
                });
            }
        }
        Ok(out)
    }

    async fn set_rating(
        &self,
        user_id: UserId,
        kind: StarTargetKind,
        target_id: Uuid,
        rating: u8,
    ) -> DomainResult<()> {
        let mut db = self.db.clone();
        let key = format!("{}:{}:{target_id}", user_id.0, kind.as_str());
        let now = Timestamp::now();
        let existing = MusicRatingModel::filter_by_rating_key(&key)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;

        if let Some(mut rec) = existing {
            rec.update()
                .rating(i64::from(rating))
                .updated_at(now)
                .exec(&mut db)
                .await
                .map_err(db_err)?;
        } else {
            toasty::create!(MusicRatingModel {
                rating_key: key,
                user_id: user_id.0,
                kind: kind.as_str().to_string(),
                target_id: target_id,
                rating: i64::from(rating),
                updated_at: now,
            })
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        }
        Ok(())
    }

    async fn remove_rating(
        &self,
        user_id: UserId,
        kind: StarTargetKind,
        target_id: Uuid,
    ) -> DomainResult<()> {
        let mut db = self.db.clone();
        let key = format!("{}:{}:{target_id}", user_id.0, kind.as_str());
        MusicRatingModel::filter_by_rating_key(&key)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    async fn list_user_ratings(&self, user_id: UserId) -> DomainResult<Vec<RatingEntry>> {
        let mut db = self.db.clone();
        let rows = MusicRatingModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(RatingEntry {
                user_id: UserId(r.user_id),
                kind: StarTargetKind::parse(&r.kind)?,
                target_id: r.target_id,
                rating: u8::try_from(r.rating.clamp(1, 5)).unwrap_or(5),
                updated_at: r.updated_at,
            });
        }
        Ok(out)
    }

    async fn record_play(
        &self,
        user_id: UserId,
        track_id: TrackId,
        album_id: AlbumId,
        played_at: Timestamp,
    ) -> DomainResult<TrackPlayStat> {
        let mut db = self.db.clone();
        let stat_key = format!("{}:{}", user_id.0, track_id.0);
        let existing = MusicPlayStatModel::filter_by_stat_key(&stat_key)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;

        let new_count = if let Some(mut rec) = existing {
            let next = rec.play_count.saturating_add(1);
            let latest = played_at.max(rec.last_played_at);
            rec.update()
                .album_id(album_id.0)
                .play_count(next)
                .last_played_at(latest)
                .exec(&mut db)
                .await
                .map_err(db_err)?;
            u64::try_from(next.max(1)).unwrap_or(1)
        } else {
            toasty::create!(MusicPlayStatModel {
                stat_key: stat_key,
                user_id: user_id.0,
                track_id: track_id.0,
                album_id: album_id.0,
                play_count: 1,
                last_played_at: played_at,
            })
            .exec(&mut db)
            .await
            .map_err(db_err)?;
            1
        };

        toasty::create!(MusicScrobbleModel {
            id: Uuid::now_v7(),
            user_id: user_id.0,
            track_id: track_id.0,
            played_at: played_at,
        })
        .exec(&mut db)
        .await
        .map_err(db_err)?;

        Ok(TrackPlayStat {
            user_id,
            track_id,
            album_id,
            play_count: new_count,
            last_played_at: played_at,
        })
    }

    async fn list_user_play_stats(&self, user_id: UserId) -> DomainResult<Vec<TrackPlayStat>> {
        let mut db = self.db.clone();
        let rows = MusicPlayStatModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|r| TrackPlayStat {
                user_id: UserId(r.user_id),
                track_id: TrackId(r.track_id),
                album_id: AlbumId(r.album_id),
                play_count: u64::try_from(r.play_count.max(0)).unwrap_or(0),
                last_played_at: r.last_played_at,
            })
            .collect())
    }

    async fn list_recent_scrobbles(
        &self,
        user_id: UserId,
        limit: u32,
    ) -> DomainResult<Vec<ScrobbleEntry>> {
        let mut db = self.db.clone();
        let mut rows = MusicScrobbleModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        rows.sort_by_key(|r| std::cmp::Reverse(r.played_at));
        rows.truncate(limit as usize);
        Ok(rows
            .into_iter()
            .map(|r| ScrobbleEntry {
                id: r.id,
                user_id: UserId(r.user_id),
                track_id: TrackId(r.track_id),
                played_at: r.played_at,
            })
            .collect())
    }

    async fn get_play_queue(&self, user_id: UserId) -> DomainResult<Option<PlayQueue>> {
        let mut db = self.db.clone();
        let row = MusicPlayQueueModel::filter_by_user_id(user_id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(row.map(|m| PlayQueue {
            user_id: UserId(m.user_id),
            track_ids: m.track_ids.0,
            current_track_id: m.current_track_id.map(TrackId),
            position_ms: u64::try_from(m.position_ms.max(0)).unwrap_or(0),
            changed_by: m.changed_by,
            updated_at: m.updated_at,
        }))
    }

    async fn save_play_queue(&self, queue: &PlayQueue) -> DomainResult<()> {
        let mut db = self.db.clone();
        let existing = MusicPlayQueueModel::filter_by_user_id(queue.user_id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        let pos = i64::try_from(queue.position_ms).unwrap_or(i64::MAX);
        let cur = queue.current_track_id.map(|t| t.0);

        if let Some(mut rec) = existing {
            rec.update()
                .track_ids(toasty::Json(queue.track_ids.clone()))
                .current_track_id(cur)
                .position_ms(pos)
                .changed_by(queue.changed_by.clone())
                .updated_at(queue.updated_at)
                .exec(&mut db)
                .await
                .map_err(db_err)?;
        } else {
            toasty::create!(MusicPlayQueueModel {
                user_id: queue.user_id.0,
                track_ids: toasty::Json(queue.track_ids.clone()),
                current_track_id: cur,
                position_ms: pos,
                changed_by: queue.changed_by.clone(),
                updated_at: queue.updated_at,
            })
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        }
        Ok(())
    }

    async fn list_bookmarks(&self, user_id: UserId) -> DomainResult<Vec<AudioBookmark>> {
        let mut db = self.db.clone();
        let mut rows = MusicBookmarkModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        rows.sort_by_key(|r| std::cmp::Reverse(r.updated_at));
        Ok(rows
            .into_iter()
            .map(|r| AudioBookmark {
                user_id: UserId(r.user_id),
                track_id: TrackId(r.track_id),
                position_ms: u64::try_from(r.position_ms.max(0)).unwrap_or(0),
                comment: r.comment,
                created_at: r.created_at,
                updated_at: r.updated_at,
            })
            .collect())
    }

    async fn get_bookmark(
        &self,
        user_id: UserId,
        track_id: TrackId,
    ) -> DomainResult<Option<AudioBookmark>> {
        let mut db = self.db.clone();
        let key = format!("{}:{}", user_id.0, track_id.0);
        let row = MusicBookmarkModel::filter_by_bookmark_key(&key)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(row.map(|r| AudioBookmark {
            user_id: UserId(r.user_id),
            track_id: TrackId(r.track_id),
            position_ms: u64::try_from(r.position_ms.max(0)).unwrap_or(0),
            comment: r.comment,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }))
    }

    async fn save_bookmark(&self, bookmark: &AudioBookmark) -> DomainResult<()> {
        let mut db = self.db.clone();
        let key = format!("{}:{}", bookmark.user_id.0, bookmark.track_id.0);
        let existing = MusicBookmarkModel::filter_by_bookmark_key(&key)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        let pos = i64::try_from(bookmark.position_ms).unwrap_or(i64::MAX);

        if let Some(mut rec) = existing {
            rec.update()
                .position_ms(pos)
                .comment(bookmark.comment.clone())
                .updated_at(bookmark.updated_at)
                .exec(&mut db)
                .await
                .map_err(db_err)?;
        } else {
            toasty::create!(MusicBookmarkModel {
                bookmark_key: key,
                user_id: bookmark.user_id.0,
                track_id: bookmark.track_id.0,
                position_ms: pos,
                comment: bookmark.comment.clone(),
                created_at: bookmark.created_at,
                updated_at: bookmark.updated_at,
            })
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        }
        Ok(())
    }

    async fn delete_bookmark(&self, user_id: UserId, track_id: TrackId) -> DomainResult<()> {
        let mut db = self.db.clone();
        let key = format!("{}:{}", user_id.0, track_id.0);
        MusicBookmarkModel::filter_by_bookmark_key(&key)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    async fn delete_by_user(&self, user_id: UserId) -> DomainResult<()> {
        let mut db = self.db.clone();
        for row in MusicPlaylistModel::filter_by_owner_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?
        {
            row.delete().exec(&mut db).await.map_err(db_err)?;
        }
        for row in MusicStarredModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?
        {
            row.delete().exec(&mut db).await.map_err(db_err)?;
        }
        for row in MusicRatingModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?
        {
            row.delete().exec(&mut db).await.map_err(db_err)?;
        }
        for row in MusicPlayStatModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?
        {
            row.delete().exec(&mut db).await.map_err(db_err)?;
        }
        for row in MusicScrobbleModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?
        {
            row.delete().exec(&mut db).await.map_err(db_err)?;
        }
        MusicPlayQueueModel::filter_by_user_id(user_id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        for row in MusicBookmarkModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?
        {
            row.delete().exec(&mut db).await.map_err(db_err)?;
        }
        Ok(())
    }
}

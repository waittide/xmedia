use std::collections::{BTreeMap, HashMap, HashSet};

use jiff::Timestamp;
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::LibraryId;
use xmedia_core::domain::user::{User, UserId, require_admin};

use super::{MusicCatalogService, MusicDirectoryNode, shuffle_prefix, sort_album_tracks};
use crate::application::dto::{
    AlbumDetailDto, AlbumDto, ArtistDetailDto, ArtistDto, SearchResultDto, TrackDto,
};
use crate::domain::catalog::{
    Album, AlbumId, AlbumListQuery, AlbumListType, Artist, ArtistId, ArtistQuery, ArtistRoleFilter,
    GenreStat, MusicTagStat, PagedResult, SearchQuery, Track, TrackId,
};
use crate::domain::collection::StarTargetKind;

impl MusicCatalogService {
    pub async fn list_albums(
        &self,
        user_id: UserId,
        query: AlbumListQuery,
    ) -> DomainResult<PagedResult<AlbumDto>> {
        let bound_tracks = self.load_bound_tracks(user_id).await?;
        let active_track_ids: HashSet<TrackId> = bound_tracks.iter().map(|t| t.id).collect();
        let mut pool = self.project_user_albums(&bound_tracks).await?;

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

        pool.retain(|a| {
            if let Some(lib_id) = query.library_id
                && a.library_id != lib_id
            {
                return false;
            }
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
        });

        match query.list_type {
            AlbumListType::Starred => {
                let mut stars = self
                    .collections
                    .list_starred(user_id, Some(StarTargetKind::Album))
                    .await?;
                stars.sort_by_key(|a| std::cmp::Reverse(a.starred_at));
                let lookup: HashMap<AlbumId, Album> = pool.into_iter().map(|a| (a.id, a)).collect();
                pool = stars
                    .into_iter()
                    .filter_map(|s| lookup.get(&AlbumId(s.target_id)).cloned())
                    .collect();
            }
            AlbumListType::Frequent => {
                let stats = self.collections.list_user_play_stats(user_id).await?;
                let mut agg: HashMap<AlbumId, u64> = HashMap::new();
                for st in stats {
                    if active_track_ids.contains(&st.track_id) {
                        *agg.entry(st.album_id).or_default() += st.play_count;
                    }
                }
                let mut pairs: Vec<(AlbumId, u64)> = agg.into_iter().collect();
                pairs.sort_by_key(|a| std::cmp::Reverse(a.1));
                let lookup: HashMap<AlbumId, Album> = pool.into_iter().map(|a| (a.id, a)).collect();
                pool = pairs
                    .into_iter()
                    .filter_map(|(id, _)| lookup.get(&id).cloned())
                    .collect();
            }
            AlbumListType::Recent => {
                let stats = self.collections.list_user_play_stats(user_id).await?;
                let mut agg: HashMap<AlbumId, Timestamp> = HashMap::new();
                for st in stats {
                    if !active_track_ids.contains(&st.track_id) {
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
                let lookup: HashMap<AlbumId, Album> = pool.into_iter().map(|a| (a.id, a)).collect();
                pool = pairs
                    .into_iter()
                    .filter_map(|(id, _)| lookup.get(&id).cloned())
                    .collect();
            }
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
                let rev = query
                    .from_year
                    .zip(query.to_year)
                    .is_some_and(|(f, t)| f > t);
                pool.sort_by(|a, b| {
                    let ord = a
                        .year
                        .cmp(&b.year)
                        .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
                    if rev { ord.reverse() } else { ord }
                });
            }
            AlbumListType::Random => {
                let len = pool.len();
                shuffle_prefix(&mut pool, len);
            }
        }

        let total = pool.len() as u64;
        let size = query.size.max(1);
        let page_items: Vec<Album> = pool
            .into_iter()
            .skip(query.offset as usize)
            .take(size as usize)
            .collect();
        let page = (query.offset / size) + 1;
        let items = self.enrich_albums(user_id, page_items).await?;
        Ok(PagedResult {
            items,
            total,
            page,
            page_size: size,
        })
    }

    pub async fn get_album_detail(
        &self,
        user_id: UserId,
        album_id: AlbumId,
    ) -> DomainResult<AlbumDetailDto> {
        let bound_tracks = self.load_bound_tracks(user_id).await?;
        let mut tracks: Vec<Track> = bound_tracks
            .into_iter()
            .filter(|t| t.album_id == album_id)
            .collect();
        if tracks.is_empty() {
            return Err(DomainError::NotFound("album not found".into()));
        }
        sort_album_tracks(&mut tracks);
        let projected = self.project_user_albums(&tracks).await?;
        let album = projected
            .into_iter()
            .find(|a| a.id == album_id)
            .ok_or_else(|| DomainError::NotFound("album not found".into()))?;
        let album_dto = self
            .enrich_albums(user_id, vec![album])
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound("album not found".into()))?;
        let track_dtos = self.enrich_tracks(user_id, tracks).await?;
        Ok(AlbumDetailDto {
            album: album_dto,
            tracks: track_dtos,
        })
    }

    pub async fn list_artists(
        &self,
        user_id: UserId,
        query: ArtistQuery,
    ) -> DomainResult<PagedResult<ArtistDto>> {
        let mut bound_tracks = self.load_bound_tracks(user_id).await?;
        if let Some(lib_id) = query.library_id {
            bound_tracks.retain(|t| t.library_id == lib_id);
        }
        let mut artists = self.project_user_artists(&bound_tracks).await?;
        let kw_lower = query
            .search
            .as_ref()
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty());

        artists.retain(|a| {
            match query.role_filter {
                ArtistRoleFilter::All => {}
                ArtistRoleFilter::AlbumArtist => {
                    if !a.is_album_artist {
                        return false;
                    }
                }
                ArtistRoleFilter::TrackArtist => {
                    if !a.is_track_artist {
                        return false;
                    }
                }
            }
            if let Some(ref kw) = kw_lower
                && !a.name.to_lowercase().contains(kw)
            {
                return false;
            }
            true
        });

        artists.sort_by_key(|a| a.name.to_lowercase());
        let total = artists.len() as u64;
        let size = query.size.max(1);
        let page_items: Vec<Artist> = artists
            .into_iter()
            .skip(query.offset as usize)
            .take(size as usize)
            .collect();
        let page = (query.offset / size) + 1;
        let items = self.enrich_artists(user_id, page_items).await?;
        Ok(PagedResult {
            items,
            total,
            page,
            page_size: size,
        })
    }

    pub async fn get_artist_detail(
        &self,
        user_id: UserId,
        artist_id: ArtistId,
    ) -> DomainResult<ArtistDetailDto> {
        let bound_tracks = self.load_bound_tracks(user_id).await?;
        let all_tracks: Vec<Track> = bound_tracks
            .iter()
            .filter(|t| t.artist_id == artist_id || t.album_artist_id == artist_id)
            .cloned()
            .collect();
        if all_tracks.is_empty() {
            return Err(DomainError::NotFound("artist not found".into()));
        }

        let artist = self
            .project_user_artists(&all_tracks)
            .await?
            .into_iter()
            .find(|a| a.id == artist_id)
            .ok_or_else(|| DomainError::NotFound("artist not found".into()))?;
        let mut albums: Vec<Album> = self
            .project_user_albums(&bound_tracks)
            .await?
            .into_iter()
            .filter(|a| a.album_artist_id == artist_id)
            .collect();
        albums.sort_by(|a, b| {
            b.year
                .cmp(&a.year)
                .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
        });

        let appears_raw: Vec<Track> = all_tracks
            .iter()
            .filter(|t| t.artist_id == artist_id && t.album_artist_id != artist_id)
            .cloned()
            .collect();

        let mut enriched_all = self.enrich_tracks(user_id, all_tracks).await?;
        enriched_all.sort_by(|a, b| {
            b.play_count
                .cmp(&a.play_count)
                .then_with(|| a.title.cmp(&b.title))
        });
        enriched_all.truncate(20);

        let artist_dto = self
            .enrich_artists(user_id, vec![artist])
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound("artist not found".into()))?;
        let album_dtos = self.enrich_albums(user_id, albums).await?;
        let appears_dtos = self.enrich_tracks(user_id, appears_raw).await?;

        Ok(ArtistDetailDto {
            artist: artist_dto,
            albums: album_dtos,
            appears_on_tracks: appears_dtos,
            top_tracks: enriched_all,
        })
    }

    pub async fn list_genres(&self, user_id: UserId) -> DomainResult<Vec<GenreStat>> {
        let tracks = self.load_bound_tracks(user_id).await?;
        let mut map: BTreeMap<String, (String, u64, HashSet<AlbumId>)> = BTreeMap::new();
        for tr in tracks {
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

    pub async fn list_tags(&self, user_id: UserId) -> DomainResult<Vec<MusicTagStat>> {
        let tracks = self.load_bound_tracks(user_id).await?;
        if tracks.is_empty() {
            return Ok(Vec::new());
        }
        let ids: Vec<TrackId> = tracks.iter().map(|t| t.id).collect();
        let tag_map = self.catalog.get_tags_for_tracks(&ids).await?;
        let mut counts: BTreeMap<String, (String, u64)> = BTreeMap::new();
        for tags in tag_map.into_values() {
            for tag in tags {
                let entry = counts.entry(tag.to_lowercase()).or_insert_with(|| (tag, 0));
                entry.1 += 1;
            }
        }
        let mut out: Vec<MusicTagStat> = counts
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

    pub async fn set_track_tags(
        &self,
        actor: &User,
        track_id: TrackId,
        tags: Vec<String>,
    ) -> DomainResult<TrackDto> {
        require_admin(actor)?;
        let track = self
            .catalog
            .find_track_by_id(track_id)
            .await?
            .filter(|t| !t.is_missing)
            .ok_or_else(|| DomainError::NotFound("track not found".into()))?;
        self.catalog.set_track_tags(track.id, tags).await?;
        let mut list = self.enrich_tracks(actor.id, vec![track]).await?;
        list.pop()
            .ok_or_else(|| DomainError::NotFound("track not found".into()))
    }

    pub async fn search(
        &self,
        user_id: UserId,
        query: SearchQuery,
    ) -> DomainResult<SearchResultDto> {
        let mut bound_tracks = self.load_bound_tracks(user_id).await?;
        if let Some(lib_id) = query.library_id {
            bound_tracks.retain(|t| t.library_id == lib_id);
        }
        let q_lower = query.query.trim().trim_matches('"').to_lowercase();

        let mut artists = self.project_user_artists(&bound_tracks).await?;
        artists.retain(|a| q_lower.is_empty() || a.name.to_lowercase().contains(&q_lower));
        artists.sort_by_key(|a| a.name.to_lowercase());
        let artists: Vec<Artist> = artists
            .into_iter()
            .skip(query.artist_offset as usize)
            .take(query.artist_count as usize)
            .collect();

        let mut albums = self.project_user_albums(&bound_tracks).await?;
        albums.retain(|a| {
            q_lower.is_empty()
                || a.title.to_lowercase().contains(&q_lower)
                || a.album_artist_name.to_lowercase().contains(&q_lower)
        });
        albums.sort_by_key(|a| a.title.to_lowercase());
        let albums: Vec<Album> = albums
            .into_iter()
            .skip(query.album_offset as usize)
            .take(query.album_count as usize)
            .collect();

        let mut tracks: Vec<Track> = bound_tracks
            .into_iter()
            .filter(|t| {
                q_lower.is_empty()
                    || t.title.to_lowercase().contains(&q_lower)
                    || t.artist_name.to_lowercase().contains(&q_lower)
                    || t.album_title.to_lowercase().contains(&q_lower)
            })
            .collect();
        tracks.sort_by_key(|t| t.title.to_lowercase());
        let tracks: Vec<Track> = tracks
            .into_iter()
            .skip(query.song_offset as usize)
            .take(query.song_count as usize)
            .collect();

        Ok(SearchResultDto {
            artists: self.enrich_artists(user_id, artists).await?,
            albums: self.enrich_albums(user_id, albums).await?,
            tracks: self.enrich_tracks(user_id, tracks).await?,
        })
    }

    pub async fn get_music_directory(
        &self,
        user_id: UserId,
        raw_id: &str,
    ) -> DomainResult<MusicDirectoryNode> {
        let uuid = Uuid::parse_str(raw_id.trim())
            .map_err(|_| DomainError::NotFound("invalid directory id".into()))?;
        let bound_tracks = self.load_bound_tracks(user_id).await?;

        if let Some(album) = self.catalog.find_album_by_id(AlbumId(uuid)).await? {
            let mut tracks: Vec<Track> = bound_tracks
                .into_iter()
                .filter(|t| t.album_id == album.id)
                .collect();
            if tracks.is_empty() {
                return Err(DomainError::NotFound("music directory not found".into()));
            }
            sort_album_tracks(&mut tracks);
            return Ok(MusicDirectoryNode::Folder {
                id: album.id.to_string(),
                parent: Some(album.album_artist_id.to_string()),
                name: album.title,
                sub_albums: Vec::new(),
                tracks: self.enrich_tracks(user_id, tracks).await?,
            });
        }

        if let Some(artist) = self.catalog.find_artist_by_id(ArtistId(uuid)).await? {
            let mut albums: Vec<Album> = self
                .project_user_albums(&bound_tracks)
                .await?
                .into_iter()
                .filter(|a| a.album_artist_id == artist.id)
                .collect();
            if albums.is_empty() {
                return Err(DomainError::NotFound("music directory not found".into()));
            }
            albums.sort_by_key(|a| a.title.to_lowercase());
            return Ok(MusicDirectoryNode::Folder {
                id: artist.id.to_string(),
                parent: None,
                name: artist.name,
                sub_albums: self.enrich_albums(user_id, albums).await?,
                tracks: Vec::new(),
            });
        }

        if let Some(lib) = self.libraries.find_by_id(LibraryId(uuid)).await? {
            let lib_tracks: Vec<Track> = bound_tracks
                .into_iter()
                .filter(|t| t.library_id == lib.id)
                .collect();
            let mut albums = self.project_user_albums(&lib_tracks).await?;
            albums.sort_by_key(|a| a.title.to_lowercase());
            albums.truncate(500);
            return Ok(MusicDirectoryNode::Folder {
                id: lib.id.to_string(),
                parent: None,
                name: lib.name,
                sub_albums: self.enrich_albums(user_id, albums).await?,
                tracks: Vec::new(),
            });
        }

        Err(DomainError::NotFound("music directory not found".into()))
    }
}

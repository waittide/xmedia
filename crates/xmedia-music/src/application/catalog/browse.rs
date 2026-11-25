use std::collections::HashMap;

use jiff::Timestamp;
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::LibraryId;
use xmedia_core::domain::user::{User, UserId, require_admin};

use super::{MusicCatalogService, MusicDirectoryNode};
use crate::application::dto::{
    AlbumDetailDto, AlbumDto, ArtistDetailDto, ArtistDto, SearchResultDto, TrackDto,
};
use crate::domain::catalog::{
    AlbumId, AlbumListQuery, AlbumListType, ArtistId, ArtistQuery, ArtistRoleFilter, GenreStat,
    MusicTagStat, PagedResult, SearchQuery, SortOrder, Track, TrackId, TrackQuery, TrackSortField,
};
use crate::domain::collection::StarTargetKind;

impl MusicCatalogService {
    pub async fn list_albums(
        &self,
        user_id: UserId,
        query: AlbumListQuery,
    ) -> DomainResult<PagedResult<AlbumDto>> {
        let allowed = self.bound_track_ids(user_id).await?;
        let stars = self
            .collections
            .list_starred(user_id, Some(StarTargetKind::Album))
            .await?;
        let ratings = self.collections.list_user_ratings(user_id).await?;
        let stats = self.collections.list_user_play_stats(user_id).await?;

        let paged = self
            .catalog
            .query_bound_albums(&allowed, &query, &stars, &stats, &ratings)
            .await?;
        if paged.items.is_empty() {
            return Ok(PagedResult {
                items: Vec::new(),
                total: paged.total,
                page: paged.page,
                page_size: paged.page_size,
            });
        }

        let star_map: HashMap<Uuid, Timestamp> = stars
            .into_iter()
            .map(|s| (s.target_id, s.starred_at))
            .collect();
        let rating_map: HashMap<Uuid, u8> = ratings
            .into_iter()
            .filter(|r| r.kind == StarTargetKind::Album)
            .map(|r| (r.target_id, r.rating))
            .collect();
        let mut album_play_counts: HashMap<AlbumId, u64> = HashMap::new();
        for st in stats {
            *album_play_counts.entry(st.album_id).or_default() += st.play_count;
        }

        let items =
            Self::enrich_albums_with_maps(paged.items, &star_map, &rating_map, &album_play_counts);
        Ok(PagedResult {
            items,
            total: paged.total,
            page: paged.page,
            page_size: paged.page_size,
        })
    }

    pub async fn get_album_detail(
        &self,
        user_id: UserId,
        album_id: AlbumId,
    ) -> DomainResult<AlbumDetailDto> {
        let allowed = self.bound_track_ids(user_id).await?;
        let tracks = self
            .catalog
            .list_bound_tracks_by_album(&allowed, album_id)
            .await?;
        if tracks.is_empty() {
            return Err(DomainError::NotFound("album not found".into()));
        }
        let album = self
            .catalog
            .project_bound_albums_by_ids(&allowed, &[album_id], None)
            .await?
            .into_iter()
            .next()
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
        let allowed = self.bound_track_ids(user_id).await?;
        let paged = self.catalog.query_bound_artists(&allowed, &query).await?;
        let items = self.enrich_artists(user_id, paged.items).await?;
        Ok(PagedResult {
            items,
            total: paged.total,
            page: paged.page,
            page_size: paged.page_size,
        })
    }

    pub async fn get_artist_detail(
        &self,
        user_id: UserId,
        artist_id: ArtistId,
    ) -> DomainResult<ArtistDetailDto> {
        let allowed = self.bound_track_ids(user_id).await?;
        let all_tracks = self
            .catalog
            .list_bound_tracks_by_artist(&allowed, artist_id)
            .await?;
        if all_tracks.is_empty() {
            return Err(DomainError::NotFound("artist not found".into()));
        }

        let artist = self
            .catalog
            .project_bound_artists_by_ids(&allowed, &[artist_id], None)
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound("artist not found".into()))?;
        let albums = self
            .catalog
            .list_bound_albums_by_artist(&allowed, artist_id)
            .await?;

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
        let allowed = self.bound_track_ids(user_id).await?;
        self.catalog.list_bound_genres(&allowed).await
    }

    pub async fn list_tags(&self, user_id: UserId) -> DomainResult<Vec<MusicTagStat>> {
        let allowed = self.bound_track_ids(user_id).await?;
        self.catalog.list_bound_tags(&allowed).await
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
        let allowed = self.bound_track_ids(user_id).await?;
        if allowed.is_empty() {
            return Ok(SearchResultDto {
                artists: Vec::new(),
                albums: Vec::new(),
                tracks: Vec::new(),
            });
        }
        let clean_q = query.query.trim().trim_matches('"');
        let search_opt = (!clean_q.is_empty()).then(|| clean_q.to_string());

        let artists = if query.artist_count > 0 {
            self.catalog
                .query_bound_artists(
                    &allowed,
                    &ArtistQuery {
                        library_id: query.library_id,
                        search: search_opt.clone(),
                        role_filter: ArtistRoleFilter::All,
                        offset: query.artist_offset,
                        size: query.artist_count,
                    },
                )
                .await?
                .items
        } else {
            Vec::new()
        };

        let albums = if query.album_count > 0 {
            self.catalog
                .query_bound_albums(
                    &allowed,
                    &AlbumListQuery {
                        list_type: AlbumListType::AlphabeticalByName,
                        library_id: query.library_id,
                        search: search_opt.clone(),
                        offset: query.album_offset,
                        size: query.album_count,
                        ..Default::default()
                    },
                    &[],
                    &[],
                    &[],
                )
                .await?
                .items
        } else {
            Vec::new()
        };

        let tracks = if query.song_count > 0 {
            self.catalog
                .list_bound_tracks_offset(
                    &allowed,
                    &TrackQuery {
                        library_id: query.library_id,
                        search: search_opt,
                        sort_by: TrackSortField::Title,
                        sort_order: SortOrder::Asc,
                        ..Default::default()
                    },
                    query.song_offset,
                    query.song_count,
                )
                .await?
        } else {
            Vec::new()
        };

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
        let allowed = self.bound_track_ids(user_id).await?;

        if let Some(album) = self.catalog.find_album_by_id(AlbumId(uuid)).await? {
            let tracks = self
                .catalog
                .list_bound_tracks_by_album(&allowed, album.id)
                .await?;
            if tracks.is_empty() {
                return Err(DomainError::NotFound("music directory not found".into()));
            }
            return Ok(MusicDirectoryNode::Folder {
                id: album.id.to_string(),
                parent: Some(album.album_artist_id.to_string()),
                name: album.title,
                sub_albums: Vec::new(),
                tracks: self.enrich_tracks(user_id, tracks).await?,
            });
        }

        if let Some(artist) = self.catalog.find_artist_by_id(ArtistId(uuid)).await? {
            let mut albums = self
                .catalog
                .list_bound_albums_by_artist(&allowed, artist.id)
                .await?;
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
            let albums = self
                .catalog
                .query_bound_albums(
                    &allowed,
                    &AlbumListQuery {
                        list_type: AlbumListType::AlphabeticalByName,
                        library_id: Some(lib.id),
                        offset: 0,
                        size: 500,
                        ..Default::default()
                    },
                    &[],
                    &[],
                    &[],
                )
                .await?
                .items;
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

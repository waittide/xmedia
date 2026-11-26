use std::collections::HashMap;
use std::sync::Arc;

use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::LibraryId;
use xmedia_core::domain::repository::LibraryRepository;
use xmedia_core::domain::user::UserId;

use crate::application::dto::{AlbumDetailDto, AlbumDto, ArtistDetailDto, ArtistDto, TrackDto};
use crate::application::projector::MusicProjector;
use crate::domain::catalog::{
    AlbumId, AlbumListQuery, AlbumListType, ArtistId, ArtistQuery, PagedResult, Track,
};
use crate::domain::collection::StarTargetKind;
use crate::domain::repository::{
    AlbumScope, ArtistScope, CatalogFacetQuery, CatalogFacetResult, MusicCatalogRepository,
    MusicCollectionRepository, TrackScope,
};

#[derive(Debug, Clone)]
pub enum MusicDirectoryNode {
    Folder {
        id: String,
        parent: Option<String>,
        name: String,
        sub_albums: Vec<AlbumDto>,
        tracks: Vec<TrackDto>,
    },
}

#[derive(Clone)]
pub struct MusicBrowseService {
    libraries: Arc<dyn LibraryRepository>,
    catalog: Arc<dyn MusicCatalogRepository>,
    collections: Arc<dyn MusicCollectionRepository>,
    projector: MusicProjector,
}

impl MusicBrowseService {
    pub(crate) fn new(
        libraries: Arc<dyn LibraryRepository>,
        catalog: Arc<dyn MusicCatalogRepository>,
        collections: Arc<dyn MusicCollectionRepository>,
        projector: MusicProjector,
    ) -> Self {
        Self {
            libraries,
            catalog,
            collections,
            projector,
        }
    }

    pub async fn count_bound_tracks_by_library(
        &self,
        user_id: UserId,
    ) -> DomainResult<HashMap<LibraryId, u64>> {
        let allowed = self.projector.bound_track_ids(user_id).await?;
        match self
            .catalog
            .query_facets(CatalogFacetQuery::LibraryTrackCounts(&allowed))
            .await?
        {
            CatalogFacetResult::LibraryTrackCounts(counts) => Ok(counts),
            _ => Ok(HashMap::new()),
        }
    }

    pub async fn list_albums(
        &self,
        user_id: UserId,
        query: AlbumListQuery,
    ) -> DomainResult<PagedResult<AlbumDto>> {
        let allowed = self.projector.bound_track_ids(user_id).await?;
        let engagement = self
            .collections
            .load_engagement(user_id, Some(StarTargetKind::Album))
            .await?;

        let paged = self
            .catalog
            .query_albums(AlbumScope::BoundPaged {
                allowed: &allowed,
                query: &query,
                starred: &engagement.starred,
                play_stats: &engagement.play_stats,
            })
            .await?;
        if paged.items.is_empty() {
            return Ok(PagedResult {
                items: Vec::new(),
                total: paged.total,
                page: paged.page,
                page_size: paged.page_size,
            });
        }

        let star_map = engagement.star_map(StarTargetKind::Album);
        let rating_map = engagement.rating_map(StarTargetKind::Album);
        let (_, album_play_counts) = engagement.play_maps(None);
        let items = MusicProjector::enrich_albums_with_maps(
            paged.items,
            &star_map,
            &rating_map,
            &album_play_counts,
        );
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
        let allowed = self.projector.bound_track_ids(user_id).await?;
        let tracks = self
            .catalog
            .query_tracks(TrackScope::Album {
                album_id,
                allowed: Some(&allowed),
            })
            .await?;
        if tracks.is_empty() {
            return Err(DomainError::NotFound("album not found".into()));
        }
        let album = self
            .catalog
            .query_albums(AlbumScope::BoundByIds {
                allowed: &allowed,
                album_ids: &[album_id],
                library_id: None,
            })
            .await?
            .items
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound("album not found".into()))?;
        let album_dto = self
            .projector
            .enrich_albums(user_id, vec![album])
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound("album not found".into()))?;
        let track_dtos = self.projector.enrich_tracks(user_id, tracks).await?;
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
        let allowed = self.projector.bound_track_ids(user_id).await?;
        let paged = self
            .catalog
            .query_artists(ArtistScope::BoundPaged {
                allowed: &allowed,
                query: &query,
            })
            .await?;
        let items = self.projector.enrich_artists(user_id, paged.items).await?;
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
        let allowed = self.projector.bound_track_ids(user_id).await?;
        let all_tracks = self
            .catalog
            .query_tracks(TrackScope::Artist {
                artist_id,
                allowed: Some(&allowed),
            })
            .await?;
        if all_tracks.is_empty() {
            return Err(DomainError::NotFound("artist not found".into()));
        }

        let artist = self
            .catalog
            .query_artists(ArtistScope::BoundByIds {
                allowed: &allowed,
                artist_ids: &[artist_id],
                library_id: None,
            })
            .await?
            .items
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound("artist not found".into()))?;
        let albums = self
            .catalog
            .query_albums(AlbumScope::BoundByArtist {
                allowed: &allowed,
                artist_id,
            })
            .await?
            .items;

        let appears_raw: Vec<Track> = all_tracks
            .iter()
            .filter(|t| t.artist_id == artist_id && t.album_artist_id != artist_id)
            .cloned()
            .collect();

        let mut enriched_all = self.projector.enrich_tracks(user_id, all_tracks).await?;
        enriched_all.sort_by(|a, b| {
            b.play_count
                .cmp(&a.play_count)
                .then_with(|| a.title.cmp(&b.title))
        });
        enriched_all.truncate(20);

        let artist_dto = self
            .projector
            .enrich_artists(user_id, vec![artist])
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound("artist not found".into()))?;
        let album_dtos = self.projector.enrich_albums(user_id, albums).await?;
        let appears_dtos = self.projector.enrich_tracks(user_id, appears_raw).await?;

        Ok(ArtistDetailDto {
            artist: artist_dto,
            albums: album_dtos,
            appears_on_tracks: appears_dtos,
            top_tracks: enriched_all,
        })
    }

    pub async fn get_music_directory(
        &self,
        user_id: UserId,
        raw_id: &str,
    ) -> DomainResult<MusicDirectoryNode> {
        let uuid = Uuid::parse_str(raw_id.trim())
            .map_err(|_| DomainError::NotFound("invalid directory id".into()))?;
        let allowed = self.projector.bound_track_ids(user_id).await?;

        if let Some(album) = self
            .catalog
            .query_albums(AlbumScope::Id(AlbumId(uuid)))
            .await?
            .items
            .into_iter()
            .next()
        {
            let tracks = self
                .catalog
                .query_tracks(TrackScope::Album {
                    album_id: album.id,
                    allowed: Some(&allowed),
                })
                .await?;
            if tracks.is_empty() {
                return Err(DomainError::NotFound("music directory not found".into()));
            }
            return Ok(MusicDirectoryNode::Folder {
                id: album.id.to_string(),
                parent: Some(album.album_artist_id.to_string()),
                name: album.title,
                sub_albums: Vec::new(),
                tracks: self.projector.enrich_tracks(user_id, tracks).await?,
            });
        }

        if let Some(artist) = self
            .catalog
            .query_artists(ArtistScope::Id(ArtistId(uuid)))
            .await?
            .items
            .into_iter()
            .next()
        {
            let mut albums = self
                .catalog
                .query_albums(AlbumScope::BoundByArtist {
                    allowed: &allowed,
                    artist_id: artist.id,
                })
                .await?
                .items;
            if albums.is_empty() {
                return Err(DomainError::NotFound("music directory not found".into()));
            }
            albums.sort_by_key(|a| a.title.to_lowercase());
            return Ok(MusicDirectoryNode::Folder {
                id: artist.id.to_string(),
                parent: None,
                name: artist.name,
                sub_albums: self.projector.enrich_albums(user_id, albums).await?,
                tracks: Vec::new(),
            });
        }

        if let Some(lib) = self.libraries.find_by_id(LibraryId(uuid)).await? {
            let albums = self
                .catalog
                .query_albums(AlbumScope::BoundPaged {
                    allowed: &allowed,
                    query: &AlbumListQuery {
                        list_type: AlbumListType::AlphabeticalByName,
                        library_id: Some(lib.id),
                        offset: 0,
                        size: 500,
                        ..Default::default()
                    },
                    starred: &[],
                    play_stats: &[],
                })
                .await?
                .items;
            return Ok(MusicDirectoryNode::Folder {
                id: lib.id.to_string(),
                parent: None,
                name: lib.name,
                sub_albums: self.projector.enrich_albums(user_id, albums).await?,
                tracks: Vec::new(),
            });
        }

        Err(DomainError::NotFound("music directory not found".into()))
    }
}

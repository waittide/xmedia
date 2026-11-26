use std::collections::HashMap;
use std::sync::Arc;

use xmedia_core::application::bindings::MediaBindingService;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::{LibraryId, MediaKind};
use xmedia_core::domain::user::UserId;

use crate::application::dto::TrackDto;
use crate::application::projector::MusicProjector;
use crate::domain::catalog::{PagedResult, SortOrder, Track, TrackId, TrackQuery, TrackSortField};
use crate::domain::repository::{
    ArtistScope, MusicCatalogRepository, MusicCollectionRepository, TrackScope,
};

#[derive(Clone)]
pub struct MusicTrackService {
    catalog: Arc<dyn MusicCatalogRepository>,
    collections: Arc<dyn MusicCollectionRepository>,
    bindings: Arc<MediaBindingService>,
    projector: MusicProjector,
}

impl MusicTrackService {
    pub(crate) fn new(
        catalog: Arc<dyn MusicCatalogRepository>,
        collections: Arc<dyn MusicCollectionRepository>,
        bindings: Arc<MediaBindingService>,
        projector: MusicProjector,
    ) -> Self {
        Self {
            catalog,
            collections,
            bindings,
            projector,
        }
    }

    pub async fn query_tracks(
        &self,
        user_id: UserId,
        query: TrackQuery,
    ) -> DomainResult<PagedResult<TrackDto>> {
        let allowed = self.projector.bound_track_ids(user_id).await?;
        let paged = self.catalog.search_tracks(Some(&allowed), &query).await?;
        let items = self.projector.enrich_tracks(user_id, paged.items).await?;
        Ok(PagedResult {
            items,
            total: paged.total,
            page: paged.page,
            page_size: paged.page_size,
        })
    }

    pub async fn get_track(&self, user_id: UserId, track_id: TrackId) -> DomainResult<TrackDto> {
        self.bindings
            .require_bound(user_id, MediaKind::Music, track_id.0, "track not found")
            .await?;
        let track = self
            .catalog
            .query_tracks(TrackScope::Id(track_id))
            .await?
            .into_iter()
            .find(|t| !t.is_missing)
            .ok_or_else(|| DomainError::NotFound("track not found".into()))?;
        let mut list = self.projector.enrich_tracks(user_id, vec![track]).await?;
        list.pop()
            .ok_or_else(|| DomainError::NotFound("track not found".into()))
    }

    pub async fn list_recent_played_tracks(
        &self,
        user_id: UserId,
        limit: u32,
    ) -> DomainResult<Vec<TrackDto>> {
        let bound_ids = self
            .bindings
            .list_bound_ids(user_id, MediaKind::Music)
            .await?;
        if bound_ids.is_empty() {
            return Ok(Vec::new());
        }
        let clamped = limit.clamp(1, 100) as usize;
        let scrobbles = self
            .collections
            .list_recent_scrobbles(user_id, (clamped * 3).min(300) as u32)
            .await?;
        let mut seen_track_ids = Vec::new();
        for sc in scrobbles {
            if !bound_ids.contains(&sc.track_id.0) {
                continue;
            }
            if !seen_track_ids.contains(&sc.track_id) {
                seen_track_ids.push(sc.track_id);
            }
            if seen_track_ids.len() >= clamped {
                break;
            }
        }
        if seen_track_ids.is_empty() {
            return Ok(Vec::new());
        }
        let fetched_recent = self
            .catalog
            .query_tracks(TrackScope::Ids(&seen_track_ids))
            .await?;
        let track_lookup: HashMap<TrackId, Track> =
            fetched_recent.into_iter().map(|t| (t.id, t)).collect();
        let ordered_recent: Vec<Track> = seen_track_ids
            .iter()
            .filter_map(|id| track_lookup.get(id).cloned())
            .filter(|t| !t.is_missing)
            .collect();
        self.projector.enrich_tracks(user_id, ordered_recent).await
    }

    pub async fn get_random_tracks(
        &self,
        user_id: UserId,
        size: u32,
        library_id: Option<LibraryId>,
        genre: Option<&str>,
        from_year: Option<i32>,
        to_year: Option<i32>,
    ) -> DomainResult<Vec<TrackDto>> {
        let allowed = self.projector.bound_track_ids(user_id).await?;
        let tracks = self
            .catalog
            .query_tracks(TrackScope::Random {
                allowed: &allowed,
                size,
                library_id,
                genre,
                from_year,
                to_year,
            })
            .await?;
        self.projector.enrich_tracks(user_id, tracks).await
    }

    pub async fn get_tracks_by_genre(
        &self,
        user_id: UserId,
        genre: &str,
        offset: u32,
        size: u32,
        library_id: Option<LibraryId>,
    ) -> DomainResult<Vec<TrackDto>> {
        let allowed = self.projector.bound_track_ids(user_id).await?;
        let tracks = self
            .catalog
            .query_tracks(TrackScope::Offset {
                allowed: &allowed,
                query: &TrackQuery {
                    library_id,
                    genre: Some(genre.to_string()),
                    sort_by: TrackSortField::Title,
                    sort_order: SortOrder::Asc,
                    ..Default::default()
                },
                offset,
                limit: size.clamp(1, 500),
            })
            .await?;
        self.projector.enrich_tracks(user_id, tracks).await
    }

    pub async fn get_top_tracks_for_artist_name(
        &self,
        user_id: UserId,
        artist_name: &str,
        count: u32,
    ) -> DomainResult<Vec<TrackDto>> {
        let Some(artist) = self
            .catalog
            .query_artists(ArtistScope::Name(artist_name.trim()))
            .await?
            .items
            .into_iter()
            .next()
        else {
            return Ok(Vec::new());
        };
        let allowed = self.projector.bound_track_ids(user_id).await?;
        let tracks = self
            .catalog
            .query_tracks(TrackScope::Artist {
                artist_id: artist.id,
                allowed: Some(&allowed),
            })
            .await?;
        let mut dtos = self.projector.enrich_tracks(user_id, tracks).await?;
        dtos.sort_by(|a, b| {
            b.play_count
                .cmp(&a.play_count)
                .then_with(|| a.title.cmp(&b.title))
        });
        dtos.truncate(count.clamp(1, 200) as usize);
        Ok(dtos)
    }
}

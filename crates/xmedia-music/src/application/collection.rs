use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

use jiff::Timestamp;
use uuid::Uuid;
use xmedia_core::application::bindings::MediaBindingService;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::{LibraryId, MediaKind};
use xmedia_core::domain::user::{User, UserId};

use crate::application::dto::{
    BookmarkDto, NowPlayingDto, PlayQueueDto, PlaylistDetailDto, PlaylistSummaryDto,
    StarredBundleDto, UpdatePlaylistInput,
};
use crate::application::projector::MusicProjector;
use crate::domain::catalog::{AlbumId, ArtistId, Track, TrackId};
use crate::domain::collection::{
    AudioBookmark, NowPlayingEntry, PlayQueue, Playlist, PlaylistId, PlaylistTracksOp,
    StarTargetKind,
};
use crate::domain::repository::{
    AlbumScope, ArtistScope, CatalogFacetQuery, CatalogFacetResult, CollectionMutation,
    CollectionPurgeScope, MusicCatalogRepository, MusicCollectionRepository, PlaylistQuery,
    TrackScope,
};

#[derive(Clone)]
pub struct MusicCollectionService {
    catalog_repo: Arc<dyn MusicCatalogRepository>,
    collections: Arc<dyn MusicCollectionRepository>,
    bindings: Arc<MediaBindingService>,
    projector: MusicProjector,
    now_playing: Arc<RwLock<HashMap<(UserId, String), NowPlayingEntry>>>,
}

impl MusicCollectionService {
    pub(crate) fn new(
        catalog_repo: Arc<dyn MusicCatalogRepository>,
        collections: Arc<dyn MusicCollectionRepository>,
        bindings: Arc<MediaBindingService>,
        projector: MusicProjector,
    ) -> Self {
        Self {
            catalog_repo,
            collections,
            bindings,
            projector,
            now_playing: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn list_playlists(&self, user_id: UserId) -> DomainResult<Vec<PlaylistSummaryDto>> {
        let bound_ids = self
            .bindings
            .list_bound_ids(user_id, MediaKind::Music)
            .await?;
        let playlists = self
            .collections
            .query_playlists(PlaylistQuery::VisibleTo(user_id))
            .await?;
        let mut all_track_ids = Vec::new();
        for pl in &playlists {
            for &tid in &pl.track_ids {
                if bound_ids.contains(&tid.0) && !all_track_ids.contains(&tid) {
                    all_track_ids.push(tid);
                }
            }
        }
        let tracks = self
            .catalog_repo
            .query_tracks(TrackScope::Ids(&all_track_ids))
            .await?;
        let track_map: HashMap<TrackId, Track> = tracks.into_iter().map(|t| (t.id, t)).collect();

        Ok(playlists
            .iter()
            .map(|pl| Self::summarize_playlist(pl, &track_map))
            .collect())
    }

    pub async fn get_playlist(
        &self,
        actor: &User,
        playlist_id: PlaylistId,
    ) -> DomainResult<PlaylistDetailDto> {
        let pl = self
            .collections
            .query_playlists(PlaylistQuery::Id(playlist_id))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound("playlist not found".into()))?;
        if !pl.is_public && pl.owner_id != actor.id && !actor.role.is_admin() {
            return Err(DomainError::Forbidden(
                "access denied to private playlist".into(),
            ));
        }

        let bound_ids = self
            .bindings
            .list_bound_ids(actor.id, MediaKind::Music)
            .await?;
        let visible_ids: Vec<TrackId> = pl
            .track_ids
            .iter()
            .copied()
            .filter(|tid| bound_ids.contains(&tid.0))
            .collect();
        let fetched = self
            .catalog_repo
            .query_tracks(TrackScope::Ids(&visible_ids))
            .await?;
        let track_map: HashMap<TrackId, Track> = fetched.into_iter().map(|t| (t.id, t)).collect();
        let ordered: Vec<Track> = visible_ids
            .iter()
            .filter_map(|tid| track_map.get(tid).cloned())
            .filter(|t| !t.is_missing)
            .collect();

        let summary = Self::summarize_playlist(&pl, &track_map);
        let track_dtos = self.projector.enrich_tracks(actor.id, ordered).await?;
        Ok(PlaylistDetailDto {
            playlist: summary,
            tracks: track_dtos,
        })
    }

    pub async fn create_playlist(
        &self,
        owner: &User,
        name: &str,
        comment: Option<String>,
        is_public: bool,
        initial_track_ids: Vec<TrackId>,
    ) -> DomainResult<PlaylistDetailDto> {
        let allowed = if initial_track_ids.is_empty() {
            Vec::new()
        } else {
            let bound_ids = self
                .bindings
                .list_bound_ids(owner.id, MediaKind::Music)
                .await?;
            initial_track_ids
                .into_iter()
                .filter(|tid| bound_ids.contains(&tid.0))
                .collect()
        };
        let pl = Playlist::new(owner.id, &owner.username, name, comment, is_public, allowed)?;
        self.collections
            .save(CollectionMutation::Playlist(&pl))
            .await?;
        self.get_playlist(owner, pl.id).await
    }

    pub async fn update_playlist(
        &self,
        actor: &User,
        playlist_id: PlaylistId,
        input: UpdatePlaylistInput,
    ) -> DomainResult<PlaylistDetailDto> {
        let mut pl = self
            .collections
            .query_playlists(PlaylistQuery::Id(playlist_id))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound("playlist not found".into()))?;

        if pl.owner_id != actor.id && !actor.role.is_admin() {
            return Err(DomainError::Forbidden(
                "only the playlist owner may modify this playlist".into(),
            ));
        }

        let tracks_op = if let Some(replaced) = input.replace_track_ids {
            let bound_ids = self
                .bindings
                .list_bound_ids(actor.id, MediaKind::Music)
                .await?;
            let allowed = replaced
                .into_iter()
                .filter(|tid| bound_ids.contains(&tid.0))
                .collect();
            PlaylistTracksOp::Replace(allowed)
        } else if !input.remove_indices.is_empty() || !input.add_track_ids.is_empty() {
            let append = if input.add_track_ids.is_empty() {
                Vec::new()
            } else {
                let bound_ids = self
                    .bindings
                    .list_bound_ids(actor.id, MediaKind::Music)
                    .await?;
                input
                    .add_track_ids
                    .into_iter()
                    .filter(|tid| bound_ids.contains(&tid.0))
                    .collect()
            };
            PlaylistTracksOp::Patch {
                remove_indices: input.remove_indices,
                append,
            }
        } else {
            PlaylistTracksOp::Keep
        };

        pl.apply_mutation(input.name, input.comment, input.is_public, tracks_op)?;
        self.collections
            .save(CollectionMutation::Playlist(&pl))
            .await?;
        self.get_playlist(actor, pl.id).await
    }

    pub async fn delete_playlist(&self, actor: &User, playlist_id: PlaylistId) -> DomainResult<()> {
        let pl = self
            .collections
            .query_playlists(PlaylistQuery::Id(playlist_id))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound("playlist not found".into()))?;
        if pl.owner_id != actor.id && !actor.role.is_admin() {
            return Err(DomainError::Forbidden(
                "only the playlist owner may delete this playlist".into(),
            ));
        }
        self.collections
            .purge(CollectionPurgeScope::Playlist(playlist_id))
            .await
    }

    pub async fn set_starred(
        &self,
        user_id: UserId,
        starred: bool,
        raw_ids: &[String],
        album_ids: &[AlbumId],
        artist_ids: &[ArtistId],
    ) -> DomainResult<()> {
        if starred {
            let allowed_tracks = self.projector.bound_track_ids(user_id).await?;
            let now = Some(Timestamp::now());
            for &aid in album_ids {
                if !self
                    .has_bound_target(&allowed_tracks, StarTargetKind::Album, aid.0)
                    .await?
                {
                    return Err(DomainError::NotFound("album not found".into()));
                }
                self.collections
                    .save(CollectionMutation::Star {
                        user_id,
                        kind: StarTargetKind::Album,
                        target_id: aid.0,
                        starred_at: now,
                    })
                    .await?;
            }
            for &arid in artist_ids {
                if !self
                    .has_bound_target(&allowed_tracks, StarTargetKind::Artist, arid.0)
                    .await?
                {
                    return Err(DomainError::NotFound("artist not found".into()));
                }
                self.collections
                    .save(CollectionMutation::Star {
                        user_id,
                        kind: StarTargetKind::Artist,
                        target_id: arid.0,
                        starred_at: now,
                    })
                    .await?;
            }
            for raw in raw_ids {
                let (kind, uuid) = self.resolve_bound_star_target(raw, &allowed_tracks).await?;
                self.collections
                    .save(CollectionMutation::Star {
                        user_id,
                        kind,
                        target_id: uuid,
                        starred_at: now,
                    })
                    .await?;
            }
        } else {
            for &aid in album_ids {
                self.collections
                    .save(CollectionMutation::Star {
                        user_id,
                        kind: StarTargetKind::Album,
                        target_id: aid.0,
                        starred_at: None,
                    })
                    .await?;
            }
            for &arid in artist_ids {
                self.collections
                    .save(CollectionMutation::Star {
                        user_id,
                        kind: StarTargetKind::Artist,
                        target_id: arid.0,
                        starred_at: None,
                    })
                    .await?;
            }
            for raw in raw_ids {
                let (kind, uuid) = self.resolve_star_target(raw).await?;
                self.collections
                    .save(CollectionMutation::Star {
                        user_id,
                        kind,
                        target_id: uuid,
                        starred_at: None,
                    })
                    .await?;
            }
        }
        Ok(())
    }

    pub async fn get_starred(
        &self,
        user_id: UserId,
        library_id: Option<LibraryId>,
    ) -> DomainResult<StarredBundleDto> {
        let allowed_tracks = self.projector.bound_track_ids(user_id).await?;
        if allowed_tracks.is_empty() {
            return Ok(StarredBundleDto {
                artists: Vec::new(),
                albums: Vec::new(),
                tracks: Vec::new(),
            });
        }

        let mut all = self
            .collections
            .load_engagement(user_id, None)
            .await?
            .starred;
        all.sort_by_key(|a| std::cmp::Reverse(a.starred_at));

        let mut artist_ids = Vec::new();
        let mut album_ids = Vec::new();
        let mut track_ids = Vec::new();

        for s in all {
            match s.kind {
                StarTargetKind::Artist => artist_ids.push(ArtistId(s.target_id)),
                StarTargetKind::Album => album_ids.push(AlbumId(s.target_id)),
                StarTargetKind::Track => {
                    let tid = TrackId(s.target_id);
                    if allowed_tracks.contains(&tid) {
                        track_ids.push(tid);
                    }
                }
            }
        }

        let ordered_artists = self
            .catalog_repo
            .query_artists(ArtistScope::BoundByIds {
                allowed: &allowed_tracks,
                artist_ids: &artist_ids,
                library_id,
            })
            .await?
            .items;

        let ordered_albums = self
            .catalog_repo
            .query_albums(AlbumScope::BoundByIds {
                allowed: &allowed_tracks,
                album_ids: &album_ids,
                library_id,
            })
            .await?
            .items;

        let fetched_tracks = self
            .catalog_repo
            .query_tracks(TrackScope::Ids(&track_ids))
            .await?;
        let track_map: HashMap<TrackId, Track> = fetched_tracks
            .into_iter()
            .filter(|t| !t.is_missing && library_id.is_none_or(|lid| t.library_id == lid))
            .map(|t| (t.id, t))
            .collect();
        let ordered_tracks: Vec<Track> = track_ids
            .into_iter()
            .filter_map(|id| track_map.get(&id).cloned())
            .collect();

        Ok(StarredBundleDto {
            artists: self
                .projector
                .enrich_artists(user_id, ordered_artists)
                .await?,
            albums: self
                .projector
                .enrich_albums(user_id, ordered_albums)
                .await?,
            tracks: self
                .projector
                .enrich_tracks(user_id, ordered_tracks)
                .await?,
        })
    }

    pub async fn set_rating(&self, user_id: UserId, raw_id: &str, rating: u8) -> DomainResult<()> {
        if rating > 5 {
            return Err(DomainError::Validation(
                "rating must be between 0 and 5".into(),
            ));
        }
        let allowed_tracks = self.projector.bound_track_ids(user_id).await?;
        let (kind, uuid) = self
            .resolve_bound_star_target(raw_id, &allowed_tracks)
            .await?;
        self.collections
            .save(CollectionMutation::Rating {
                user_id,
                kind,
                target_id: uuid,
                rating: (rating > 0).then_some(rating),
            })
            .await
    }

    pub async fn scrobble(
        &self,
        user: &User,
        entries: Vec<(TrackId, Option<Timestamp>)>,
        submission: bool,
        client_name: &str,
    ) -> DomainResult<()> {
        let bound_ids = self
            .bindings
            .list_bound_ids(user.id, MediaKind::Music)
            .await?;
        let now = Timestamp::now();
        let client = if client_name.trim().is_empty() {
            "xmedia-web"
        } else {
            client_name.trim()
        };

        for (track_id, played_at_opt) in entries {
            if !bound_ids.contains(&track_id.0) {
                continue;
            }
            let Some(track) = self
                .catalog_repo
                .query_tracks(TrackScope::Id(track_id))
                .await?
                .into_iter()
                .find(|t| !t.is_missing)
            else {
                continue;
            };
            if let Ok(mut guard) = self.now_playing.write() {
                guard.retain(|_, entry| now.duration_since(entry.updated_at).as_secs() <= 15 * 60);
                guard.insert(
                    (user.id, client.to_string()),
                    NowPlayingEntry {
                        user_id: user.id,
                        username: user.username.clone(),
                        track_id: track.id,
                        client_name: client.to_string(),
                        updated_at: now,
                    },
                );
            }
            if submission {
                self.collections
                    .save(CollectionMutation::Scrobble {
                        user_id: user.id,
                        track_id: track.id,
                        album_id: track.album_id,
                        played_at: played_at_opt.unwrap_or(now),
                    })
                    .await?;
            }
        }
        Ok(())
    }

    pub async fn list_now_playing(&self, viewer_id: UserId) -> DomainResult<Vec<NowPlayingDto>> {
        let bound_ids = self
            .bindings
            .list_bound_ids(viewer_id, MediaKind::Music)
            .await?;
        let now = Timestamp::now();
        let active_entries: Vec<NowPlayingEntry> = {
            let Ok(mut guard) = self.now_playing.write() else {
                return Ok(Vec::new());
            };
            guard.retain(|_, entry| now.duration_since(entry.updated_at).as_secs() <= 15 * 60);
            guard
                .values()
                .filter(|e| bound_ids.contains(&e.track_id.0))
                .cloned()
                .collect()
        };

        if active_entries.is_empty() {
            return Ok(Vec::new());
        }

        let track_ids: Vec<TrackId> = active_entries.iter().map(|e| e.track_id).collect();
        let tracks = self
            .catalog_repo
            .query_tracks(TrackScope::Ids(&track_ids))
            .await?;
        let enriched = self.projector.enrich_tracks(viewer_id, tracks).await?;
        let lookup: HashMap<TrackId, _> = enriched.into_iter().map(|t| (t.id, t)).collect();

        let mut out = Vec::new();
        for entry in active_entries {
            if let Some(track) = lookup.get(&entry.track_id).cloned() {
                let mins = (now.duration_since(entry.updated_at).as_secs().max(0) / 60) as u32;
                out.push(NowPlayingDto {
                    username: entry.username,
                    client_name: entry.client_name,
                    minutes_ago: mins,
                    track,
                });
            }
        }
        Ok(out)
    }

    pub async fn get_play_queue(&self, user_id: UserId) -> DomainResult<Option<PlayQueueDto>> {
        let Some(queue) = self.collections.get_play_queue(user_id).await? else {
            return Ok(None);
        };
        let bound_ids = self
            .bindings
            .list_bound_ids(user_id, MediaKind::Music)
            .await?;
        let visible_ids: Vec<TrackId> = queue
            .track_ids
            .iter()
            .copied()
            .filter(|id| bound_ids.contains(&id.0))
            .collect();
        let fetched = self
            .catalog_repo
            .query_tracks(TrackScope::Ids(&visible_ids))
            .await?;
        let lookup: HashMap<TrackId, Track> = fetched.into_iter().map(|t| (t.id, t)).collect();
        let ordered: Vec<Track> = visible_ids
            .iter()
            .filter_map(|id| lookup.get(id).cloned())
            .filter(|t| !t.is_missing)
            .collect();
        let current_track_id = queue
            .current_track_id
            .filter(|cid| bound_ids.contains(&cid.0));
        let dtos = self.projector.enrich_tracks(user_id, ordered).await?;
        Ok(Some(PlayQueueDto {
            tracks: dtos,
            current_track_id,
            position_ms: queue.position_ms,
            changed_by: queue.changed_by,
            updated_at: queue.updated_at,
        }))
    }

    pub async fn save_play_queue(
        &self,
        user_id: UserId,
        track_ids: Vec<TrackId>,
        current_track_id: Option<TrackId>,
        position_ms: u64,
        changed_by: &str,
    ) -> DomainResult<()> {
        let bound_ids = self
            .bindings
            .list_bound_ids(user_id, MediaKind::Music)
            .await?;
        let allowed_ids: Vec<TrackId> = track_ids
            .into_iter()
            .filter(|id| bound_ids.contains(&id.0))
            .collect();
        let allowed_current = current_track_id.filter(|cid| bound_ids.contains(&cid.0));
        let existing = self.collections.get_play_queue(user_id).await?;
        let queue = PlayQueue::reconcile(
            existing,
            user_id,
            allowed_ids,
            allowed_current,
            position_ms,
            changed_by,
            Timestamp::now(),
        );
        self.collections
            .save(CollectionMutation::PlayQueue(&queue))
            .await
    }

    pub async fn list_bookmarks(&self, user_id: UserId) -> DomainResult<Vec<BookmarkDto>> {
        let bound_ids = self
            .bindings
            .list_bound_ids(user_id, MediaKind::Music)
            .await?;
        let bms: Vec<_> = self
            .collections
            .query_bookmarks(user_id, None)
            .await?
            .into_iter()
            .filter(|b| bound_ids.contains(&b.track_id.0))
            .collect();
        if bms.is_empty() {
            return Ok(Vec::new());
        }
        let ids: Vec<TrackId> = bms.iter().map(|b| b.track_id).collect();
        let tracks = self
            .catalog_repo
            .query_tracks(TrackScope::Ids(&ids))
            .await?;
        let dtos = self.projector.enrich_tracks(user_id, tracks).await?;
        let lookup: HashMap<TrackId, _> = dtos.into_iter().map(|t| (t.id, t)).collect();

        let mut out = Vec::new();
        for bm in bms {
            if let Some(track) = lookup.get(&bm.track_id).cloned() {
                out.push(BookmarkDto {
                    track,
                    position_ms: bm.position_ms,
                    comment: bm.comment,
                    created_at: bm.created_at,
                    updated_at: bm.updated_at,
                });
            }
        }
        Ok(out)
    }

    pub async fn upsert_bookmark(
        &self,
        user_id: UserId,
        track_id: TrackId,
        position_ms: u64,
        comment: Option<String>,
    ) -> DomainResult<BookmarkDto> {
        self.bindings
            .require_bound(user_id, MediaKind::Music, track_id.0, "track not found")
            .await?;
        let track = self
            .catalog_repo
            .query_tracks(TrackScope::Id(track_id))
            .await?
            .into_iter()
            .find(|t| !t.is_missing)
            .ok_or_else(|| DomainError::NotFound("track not found".into()))?;
        let track_dto = self
            .projector
            .enrich_tracks(user_id, vec![track])
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound("track not found".into()))?;
        let now = Timestamp::now();
        let existing = self
            .collections
            .query_bookmarks(user_id, Some(track_id))
            .await?
            .into_iter()
            .next();
        let bm = AudioBookmark::new_or_updated(
            existing.as_ref(),
            user_id,
            track_id,
            position_ms,
            comment,
            now,
        );
        self.collections
            .save(CollectionMutation::Bookmark(&bm))
            .await?;
        Ok(BookmarkDto {
            track: track_dto,
            position_ms: bm.position_ms,
            comment: bm.comment,
            created_at: bm.created_at,
            updated_at: bm.updated_at,
        })
    }

    pub async fn delete_bookmark(&self, user_id: UserId, track_id: TrackId) -> DomainResult<()> {
        self.collections
            .purge(CollectionPurgeScope::Bookmark { user_id, track_id })
            .await
    }

    async fn has_bound_target(
        &self,
        allowed_tracks: &HashSet<TrackId>,
        kind: StarTargetKind,
        id: Uuid,
    ) -> DomainResult<bool> {
        match self
            .catalog_repo
            .query_facets(CatalogFacetQuery::HasBoundTarget {
                allowed: allowed_tracks,
                kind,
                id,
            })
            .await?
        {
            CatalogFacetResult::Exists(exists) => Ok(exists),
            _ => Ok(false),
        }
    }

    async fn resolve_bound_star_target(
        &self,
        raw_id: &str,
        allowed_tracks: &HashSet<TrackId>,
    ) -> DomainResult<(StarTargetKind, Uuid)> {
        let trimmed = raw_id.trim();
        let uuid = Uuid::parse_str(trimmed)
            .map_err(|_| DomainError::Validation(format!("invalid entity id: {trimmed}")))?;

        for kind in [
            StarTargetKind::Track,
            StarTargetKind::Album,
            StarTargetKind::Artist,
        ] {
            if self.has_bound_target(allowed_tracks, kind, uuid).await? {
                return Ok((kind, uuid));
            }
        }
        Err(DomainError::NotFound(format!(
            "entity not found: {trimmed}"
        )))
    }

    async fn resolve_star_target(&self, raw_id: &str) -> DomainResult<(StarTargetKind, Uuid)> {
        let trimmed = raw_id.trim();
        let uuid = Uuid::parse_str(trimmed)
            .map_err(|_| DomainError::Validation(format!("invalid entity id: {trimmed}")))?;

        if !self
            .catalog_repo
            .query_tracks(TrackScope::Id(TrackId(uuid)))
            .await?
            .is_empty()
        {
            return Ok((StarTargetKind::Track, uuid));
        }
        if !self
            .catalog_repo
            .query_albums(AlbumScope::Id(AlbumId(uuid)))
            .await?
            .items
            .is_empty()
        {
            return Ok((StarTargetKind::Album, uuid));
        }
        if !self
            .catalog_repo
            .query_artists(ArtistScope::Id(ArtistId(uuid)))
            .await?
            .items
            .is_empty()
        {
            return Ok((StarTargetKind::Artist, uuid));
        }
        Err(DomainError::NotFound(format!(
            "entity not found: {trimmed}"
        )))
    }

    fn summarize_playlist(
        pl: &Playlist,
        track_map: &HashMap<TrackId, Track>,
    ) -> PlaylistSummaryDto {
        let (track_count, duration_secs, cover_key) = pl.summarize(track_map);
        PlaylistSummaryDto {
            id: pl.id,
            owner_id: pl.owner_id,
            owner_username: pl.owner_username.clone(),
            name: pl.name.clone(),
            comment: pl.comment.clone(),
            is_public: pl.is_public,
            track_count,
            duration_secs,
            cover_key,
            created_at: pl.created_at,
            updated_at: pl.updated_at,
        }
    }
}

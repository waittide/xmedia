use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use jiff::Timestamp;
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::LibraryId;
use xmedia_core::domain::user::{User, UserId};

use crate::application::catalog::MusicCatalogService;
use crate::application::dto::{
    BookmarkDto, NowPlayingDto, PlayQueueDto, PlaylistDetailDto, PlaylistSummaryDto,
    StarredBundleDto, UpdatePlaylistInput,
};
use crate::domain::catalog::{Album, AlbumId, Artist, ArtistId, Track, TrackId};
use crate::domain::collection::{
    AudioBookmark, NowPlayingEntry, PlayQueue, Playlist, PlaylistId, StarTargetKind,
};
use crate::domain::repository::{MusicCatalogRepository, MusicCollectionRepository};

#[derive(Clone)]
pub struct MusicCollectionService {
    catalog_repo: Arc<dyn MusicCatalogRepository>,
    collections: Arc<dyn MusicCollectionRepository>,
    catalog_service: MusicCatalogService,
    now_playing: Arc<RwLock<HashMap<(UserId, String), NowPlayingEntry>>>,
}

impl MusicCollectionService {
    pub fn new(
        catalog_repo: Arc<dyn MusicCatalogRepository>,
        collections: Arc<dyn MusicCollectionRepository>,
        catalog_service: MusicCatalogService,
    ) -> Self {
        Self {
            catalog_repo,
            collections,
            catalog_service,
            now_playing: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn list_playlists(&self, user_id: UserId) -> DomainResult<Vec<PlaylistSummaryDto>> {
        let playlists = self.collections.list_owned_playlists(user_id).await?;
        let mut all_track_ids = Vec::new();
        for pl in &playlists {
            for &tid in &pl.track_ids {
                if !all_track_ids.contains(&tid) {
                    all_track_ids.push(tid);
                }
            }
        }
        let tracks = self.catalog_repo.find_tracks_by_ids(&all_track_ids).await?;
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
            .find_playlist_by_id(playlist_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("playlist not found".into()))?;
        if !pl.is_public && pl.owner_id != actor.id && !actor.role.is_admin() {
            return Err(DomainError::Forbidden(
                "access denied to private playlist".into(),
            ));
        }

        let fetched = self.catalog_repo.find_tracks_by_ids(&pl.track_ids).await?;
        let track_map: HashMap<TrackId, Track> = fetched.into_iter().map(|t| (t.id, t)).collect();
        let ordered: Vec<Track> = pl
            .track_ids
            .iter()
            .filter_map(|tid| track_map.get(tid).cloned())
            .filter(|t| !t.is_missing)
            .collect();

        let summary = Self::summarize_playlist(&pl, &track_map);
        let track_dtos = self
            .catalog_service
            .enrich_tracks(actor.id, ordered)
            .await?;
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
        let mut pl = Playlist::new(owner.id, &owner.username, name, comment, is_public)?;
        if !initial_track_ids.is_empty() {
            pl.replace_tracks(initial_track_ids);
        }
        self.collections.save_playlist(&pl).await?;
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
            .find_playlist_by_id(playlist_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("playlist not found".into()))?;

        if pl.owner_id != actor.id && !actor.role.is_admin() {
            return Err(DomainError::Forbidden(
                "only the playlist owner may modify this playlist".into(),
            ));
        }

        pl.update_meta(input.name, input.comment, input.is_public)?;
        if let Some(replaced) = input.replace_track_ids {
            pl.replace_tracks(replaced);
        } else {
            if !input.remove_indices.is_empty() {
                pl.remove_indices(input.remove_indices);
            }
            if !input.add_track_ids.is_empty() {
                pl.append_tracks(&input.add_track_ids);
            }
        }
        self.collections.save_playlist(&pl).await?;
        self.get_playlist(actor, pl.id).await
    }

    pub async fn delete_playlist(&self, actor: &User, playlist_id: PlaylistId) -> DomainResult<()> {
        let pl = self
            .collections
            .find_playlist_by_id(playlist_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("playlist not found".into()))?;
        if pl.owner_id != actor.id && !actor.role.is_admin() {
            return Err(DomainError::Forbidden(
                "only the playlist owner may delete this playlist".into(),
            ));
        }
        self.collections.delete_playlist(playlist_id).await
    }

    pub async fn star_items(
        &self,
        user_id: UserId,
        raw_ids: &[String],
        album_ids: &[AlbumId],
        artist_ids: &[ArtistId],
    ) -> DomainResult<()> {
        let now = Timestamp::now();
        for &aid in album_ids {
            self.collections
                .set_starred(user_id, StarTargetKind::Album, aid.0, now)
                .await?;
        }
        for &arid in artist_ids {
            self.collections
                .set_starred(user_id, StarTargetKind::Artist, arid.0, now)
                .await?;
        }
        for raw in raw_ids {
            let (kind, uuid) = self.resolve_star_target(raw).await?;
            self.collections
                .set_starred(user_id, kind, uuid, now)
                .await?;
        }
        Ok(())
    }

    pub async fn unstar_items(
        &self,
        user_id: UserId,
        raw_ids: &[String],
        album_ids: &[AlbumId],
        artist_ids: &[ArtistId],
    ) -> DomainResult<()> {
        for &aid in album_ids {
            self.collections
                .unset_starred(user_id, StarTargetKind::Album, aid.0)
                .await?;
        }
        for &arid in artist_ids {
            self.collections
                .unset_starred(user_id, StarTargetKind::Artist, arid.0)
                .await?;
        }
        for raw in raw_ids {
            let (kind, uuid) = self.resolve_star_target(raw).await?;
            self.collections.unset_starred(user_id, kind, uuid).await?;
        }
        Ok(())
    }

    pub async fn get_starred(
        &self,
        user_id: UserId,
        library_id: Option<LibraryId>,
    ) -> DomainResult<StarredBundleDto> {
        let mut all = self.collections.list_starred(user_id, None).await?;
        all.sort_by_key(|a| std::cmp::Reverse(a.starred_at));

        let mut artist_ids = Vec::new();
        let mut album_ids = Vec::new();
        let mut track_ids = Vec::new();

        for s in all {
            match s.kind {
                StarTargetKind::Artist => artist_ids.push(ArtistId(s.target_id)),
                StarTargetKind::Album => album_ids.push(AlbumId(s.target_id)),
                StarTargetKind::Track => track_ids.push(TrackId(s.target_id)),
            }
        }

        let fetched_artists = self.catalog_repo.find_artists_by_ids(&artist_ids).await?;
        let artist_map: HashMap<ArtistId, Artist> =
            fetched_artists.into_iter().map(|a| (a.id, a)).collect();
        let ordered_artists: Vec<Artist> = artist_ids
            .into_iter()
            .filter_map(|id| artist_map.get(&id).cloned())
            .collect();

        let fetched_albums = self.catalog_repo.find_albums_by_ids(&album_ids).await?;
        let album_map: HashMap<AlbumId, Album> =
            fetched_albums.into_iter().map(|a| (a.id, a)).collect();
        let ordered_albums: Vec<Album> = album_ids
            .into_iter()
            .filter_map(|id| album_map.get(&id).cloned())
            .filter(|a| library_id.is_none_or(|lid| a.library_id == lid))
            .collect();

        let fetched_tracks = self.catalog_repo.find_tracks_by_ids(&track_ids).await?;
        let track_map: HashMap<TrackId, Track> =
            fetched_tracks.into_iter().map(|t| (t.id, t)).collect();
        let ordered_tracks: Vec<Track> = track_ids
            .into_iter()
            .filter_map(|id| track_map.get(&id).cloned())
            .filter(|t| !t.is_missing && library_id.is_none_or(|lid| t.library_id == lid))
            .collect();

        Ok(StarredBundleDto {
            artists: self
                .catalog_service
                .enrich_artists(user_id, ordered_artists)
                .await?,
            albums: self
                .catalog_service
                .enrich_albums(user_id, ordered_albums)
                .await?,
            tracks: self
                .catalog_service
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
        let (kind, uuid) = self.resolve_star_target(raw_id).await?;
        if rating == 0 {
            self.collections.remove_rating(user_id, kind, uuid).await
        } else {
            self.collections
                .set_rating(user_id, kind, uuid, rating)
                .await
        }
    }

    pub async fn scrobble(
        &self,
        user: &User,
        entries: Vec<(TrackId, Option<Timestamp>)>,
        submission: bool,
        client_name: &str,
    ) -> DomainResult<()> {
        let now = Timestamp::now();
        let client = if client_name.trim().is_empty() {
            "xmedia-web"
        } else {
            client_name.trim()
        };

        for (track_id, played_at_opt) in entries {
            let Some(track) = self.catalog_repo.find_track_by_id(track_id).await? else {
                continue;
            };
            if let Ok(mut guard) = self.now_playing.write() {
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
                let ts = played_at_opt.unwrap_or(now);
                self.collections
                    .record_play(user.id, track.id, track.album_id, ts)
                    .await?;
            }
        }
        Ok(())
    }

    pub async fn list_now_playing(&self, viewer_id: UserId) -> DomainResult<Vec<NowPlayingDto>> {
        let now = Timestamp::now();
        let active_entries: Vec<NowPlayingEntry> = {
            let Ok(mut guard) = self.now_playing.write() else {
                return Ok(Vec::new());
            };
            guard.retain(|_, entry| now.duration_since(entry.updated_at).as_secs() <= 15 * 60);
            guard.values().cloned().collect()
        };

        if active_entries.is_empty() {
            return Ok(Vec::new());
        }

        let track_ids: Vec<TrackId> = active_entries.iter().map(|e| e.track_id).collect();
        let tracks = self.catalog_repo.find_tracks_by_ids(&track_ids).await?;
        let enriched = self
            .catalog_service
            .enrich_tracks(viewer_id, tracks)
            .await?;
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
        let fetched = self
            .catalog_repo
            .find_tracks_by_ids(&queue.track_ids)
            .await?;
        let lookup: HashMap<TrackId, Track> = fetched.into_iter().map(|t| (t.id, t)).collect();
        let ordered: Vec<Track> = queue
            .track_ids
            .iter()
            .filter_map(|id| lookup.get(id).cloned())
            .filter(|t| !t.is_missing)
            .collect();
        let dtos = self.catalog_service.enrich_tracks(user_id, ordered).await?;
        Ok(Some(PlayQueueDto {
            tracks: dtos,
            current_track_id: queue.current_track_id,
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
        let queue = PlayQueue {
            user_id,
            track_ids,
            current_track_id,
            position_ms,
            changed_by: if changed_by.trim().is_empty() {
                "xmedia-web".to_string()
            } else {
                changed_by.trim().to_string()
            },
            updated_at: Timestamp::now(),
        };
        self.collections.save_play_queue(&queue).await
    }

    pub async fn list_bookmarks(&self, user_id: UserId) -> DomainResult<Vec<BookmarkDto>> {
        let bms = self.collections.list_bookmarks(user_id).await?;
        if bms.is_empty() {
            return Ok(Vec::new());
        }
        let ids: Vec<TrackId> = bms.iter().map(|b| b.track_id).collect();
        let tracks = self.catalog_repo.find_tracks_by_ids(&ids).await?;
        let dtos = self.catalog_service.enrich_tracks(user_id, tracks).await?;
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
        let track_dto = self.catalog_service.get_track(user_id, track_id).await?;
        let now = Timestamp::now();
        let existing = self.collections.get_bookmark(user_id, track_id).await?;
        let bm = AudioBookmark {
            user_id,
            track_id,
            position_ms,
            comment: comment
                .map(|c| c.trim().to_string())
                .filter(|c| !c.is_empty()),
            created_at: existing.map(|e| e.created_at).unwrap_or(now),
            updated_at: now,
        };
        self.collections.save_bookmark(&bm).await?;
        Ok(BookmarkDto {
            track: track_dto,
            position_ms: bm.position_ms,
            comment: bm.comment,
            created_at: bm.created_at,
            updated_at: bm.updated_at,
        })
    }

    pub async fn delete_bookmark(&self, user_id: UserId, track_id: TrackId) -> DomainResult<()> {
        self.collections.delete_bookmark(user_id, track_id).await
    }

    async fn resolve_star_target(&self, raw_id: &str) -> DomainResult<(StarTargetKind, Uuid)> {
        let trimmed = raw_id.trim();
        let uuid = Uuid::parse_str(trimmed)
            .map_err(|_| DomainError::Validation(format!("invalid entity id: {trimmed}")))?;

        if self
            .catalog_repo
            .find_track_by_id(TrackId(uuid))
            .await?
            .is_some()
        {
            return Ok((StarTargetKind::Track, uuid));
        }
        if self
            .catalog_repo
            .find_album_by_id(AlbumId(uuid))
            .await?
            .is_some()
        {
            return Ok((StarTargetKind::Album, uuid));
        }
        if self
            .catalog_repo
            .find_artist_by_id(ArtistId(uuid))
            .await?
            .is_some()
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
        let mut count = 0u32;
        let mut duration_secs = 0u32;
        let mut cover_key = None;
        for tid in &pl.track_ids {
            if let Some(tr) = track_map.get(tid)
                && !tr.is_missing
            {
                count += 1;
                duration_secs = duration_secs.saturating_add(tr.duration_secs);
                if cover_key.is_none() {
                    cover_key = tr.cover_key.clone();
                }
            }
        }
        PlaylistSummaryDto {
            id: pl.id,
            owner_id: pl.owner_id,
            owner_username: pl.owner_username.clone(),
            name: pl.name.clone(),
            comment: pl.comment.clone(),
            is_public: pl.is_public,
            track_count: count,
            duration_secs,
            cover_key,
            created_at: pl.created_at,
            updated_at: pl.updated_at,
        }
    }
}

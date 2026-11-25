use uuid::Uuid;
use xmedia_core::application::ports::BinaryPayload;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;
use xmedia_core::domain::user::{User, UserId};

use super::MusicCatalogService;
use crate::domain::catalog::{
    AlbumId, ArtistId, SortOrder, StructuredLyrics, Track, TrackId, TrackQuery, TrackSortField,
};
use crate::domain::collection::PlaylistId;

impl MusicCatalogService {
    pub async fn get_lyrics_by_track(
        &self,
        user_id: UserId,
        track_id: TrackId,
    ) -> DomainResult<StructuredLyrics> {
        self.require_track_bound(user_id, track_id).await?;
        let track = self
            .catalog
            .find_track_by_id(track_id)
            .await?
            .filter(|t| !t.is_missing)
            .ok_or_else(|| DomainError::NotFound("track not found".into()))?;
        let library = self
            .libraries
            .find_by_id(track.library_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("library not found".into()))?;

        let sidecar = self
            .storage
            .read_sidecar_lrc_for_track(&library, &track)
            .await?;
        let audio_path = self
            .storage
            .resolve_audio_fs_path(&library, &track)
            .await
            .ok();

        let lyrics_resolver = self.lyrics.clone();
        let mut resolved = tokio::task::spawn_blocking(move || {
            lyrics_resolver.resolve_lyrics(audio_path.as_deref(), sidecar.as_deref())
        })
        .await
        .map_err(|e| DomainError::Storage(format!("lyrics resolve task failed: {e}")))??;
        if resolved.display_artist.is_none() {
            resolved.display_artist = Some(track.artist_name);
        }
        if resolved.display_title.is_none() {
            resolved.display_title = Some(track.title);
        }
        Ok(resolved)
    }

    pub async fn get_lyrics_by_artist_title(
        &self,
        user_id: UserId,
        artist: Option<&str>,
        title: Option<&str>,
    ) -> DomainResult<StructuredLyrics> {
        let title_q = title.unwrap_or_default().trim();
        let artist_q = artist.unwrap_or_default().trim();
        let search_term = if !title_q.is_empty() {
            title_q
        } else if !artist_q.is_empty() {
            artist_q
        } else {
            return Ok(StructuredLyrics::empty());
        };
        let allowed = self.bound_track_ids(user_id).await?;
        let candidates = self
            .catalog
            .list_bound_tracks_offset(
                &allowed,
                &TrackQuery {
                    search: Some(search_term.to_string()),
                    sort_by: TrackSortField::Title,
                    sort_order: SortOrder::Asc,
                    ..Default::default()
                },
                0,
                25,
            )
            .await?;

        let artist_lower = artist_q.to_lowercase();
        let title_lower = title_q.to_lowercase();

        let mut prioritized: Vec<&Track> = candidates
            .iter()
            .filter(|t| {
                t.has_lyrics()
                    && (title_lower.is_empty() || t.title.to_lowercase() == title_lower)
                    && (artist_lower.is_empty()
                        || t.artist_name.to_lowercase().contains(&artist_lower))
            })
            .collect();
        for tr in &candidates {
            if tr.has_lyrics() && !prioritized.iter().any(|p| p.id == tr.id) {
                prioritized.push(tr);
            }
        }

        for track in prioritized {
            let lyr = self.get_lyrics_by_track(user_id, track.id).await?;
            if !lyr.is_empty() {
                return Ok(lyr);
            }
        }
        Ok(StructuredLyrics::empty())
    }

    pub async fn get_cover_art(
        &self,
        actor: &User,
        raw_id: &str,
        max_size: Option<u32>,
    ) -> DomainResult<BinaryPayload> {
        let clean = raw_id.trim();

        if let Ok(uuid) = Uuid::parse_str(clean) {
            if let Some(album) = self.catalog.find_album_by_id(AlbumId(uuid)).await? {
                let alb_tracks = self.catalog.list_tracks_by_album(album.id).await?;
                let has_local_pool_track = alb_tracks.iter().any(|t| !t.source.is_online_tracked());
                if !has_local_pool_track {
                    let bound = self
                        .bindings
                        .list_bound_ids(actor.id, MediaKind::Music)
                        .await?;
                    if !alb_tracks.iter().any(|t| bound.contains(&t.id.0)) {
                        return Err(DomainError::NotFound("cover art not found".into()));
                    }
                }
                if let Some(ref key) = album.cover_key {
                    return self.storage.read_cover(key, max_size).await;
                }
            }
            if let Some(track) = self.catalog.find_track_by_id(TrackId(uuid)).await?
                && !track.is_missing
            {
                if track.source.is_online_tracked() {
                    self.require_track_bound(actor.id, track.id).await?;
                }
                if let Some(ref key) = track.cover_key {
                    return self.storage.read_cover(key, max_size).await;
                }
            }
            if let Some(artist) = self.catalog.find_artist_by_id(ArtistId(uuid)).await? {
                let ar_tracks = self.catalog.list_tracks_by_artist(artist.id).await?;
                let has_local_pool_track = ar_tracks.iter().any(|t| !t.source.is_online_tracked());
                if !has_local_pool_track {
                    let bound = self
                        .bindings
                        .list_bound_ids(actor.id, MediaKind::Music)
                        .await?;
                    if !ar_tracks.iter().any(|t| bound.contains(&t.id.0)) {
                        return Err(DomainError::NotFound("cover art not found".into()));
                    }
                }
                if let Some(ref key) = artist.cover_key {
                    return self.storage.read_cover(key, max_size).await;
                }
            }
            if let Some(pl) = self
                .collections
                .find_playlist_by_id(PlaylistId(uuid))
                .await?
            {
                if !pl.is_public && pl.owner_id != actor.id && !actor.role.is_admin() {
                    return Err(DomainError::Forbidden(
                        "not authorized to access private playlist cover".into(),
                    ));
                }
                let bound = self
                    .bindings
                    .list_bound_ids(actor.id, MediaKind::Music)
                    .await?;
                for &track_id in &pl.track_ids {
                    if !bound.contains(&track_id.0) {
                        continue;
                    }
                    if let Some(track) = self.catalog.find_track_by_id(track_id).await?
                        && !track.is_missing
                        && let Some(ref key) = track.cover_key
                    {
                        return self.storage.read_cover(key, max_size).await;
                    }
                }
                return Err(DomainError::NotFound("playlist has no cover art".into()));
            }
        }

        // Raw cover_key lookup (used by Web UI `buildCoverUrl(track.cover_key)`):
        // enforce OnlineTracked isolation via indexed lookups so private tracked covers never leak.
        let matched_tracks = self.catalog.find_tracks_by_cover_key(clean).await?;
        let mut matched_pool = false;
        let mut matched_tracked_tracks: Vec<TrackId> = Vec::new();
        for tr in matched_tracks {
            if tr.source.is_online_tracked() {
                matched_tracked_tracks.push(tr.id);
            } else {
                matched_pool = true;
                break;
            }
        }
        if !matched_pool {
            let matched_albums = self.catalog.find_albums_by_cover_key(clean).await?;
            for alb in matched_albums {
                let alb_tracks = self.catalog.list_tracks_by_album(alb.id).await?;
                for tr in alb_tracks {
                    if tr.source.is_online_tracked() {
                        matched_tracked_tracks.push(tr.id);
                    } else {
                        matched_pool = true;
                        break;
                    }
                }
                if matched_pool {
                    break;
                }
            }
        }
        if !matched_pool && !matched_tracked_tracks.is_empty() {
            let bound = self
                .bindings
                .list_bound_ids(actor.id, MediaKind::Music)
                .await?;
            if !matched_tracked_tracks
                .iter()
                .any(|tid| bound.contains(&tid.0))
            {
                return Err(DomainError::NotFound("cover art not found".into()));
            }
        }

        self.storage.read_cover(clean, max_size).await
    }
}

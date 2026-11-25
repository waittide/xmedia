mod cache;

use jiff::Timestamp;
use xmedia_core::application::ports::SystemEvent;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::{LibraryId, MediaKind};
use xmedia_core::domain::user::User;

use crate::application::dto::TrackDto;
use crate::domain::catalog::{
    Album, Artist, AudioFormat, OnlineTrackedTrackSource, ReplayGain, Track, TrackId, TrackSource,
};
use crate::domain::online::{MusicTrackContext, infer_audio_format};

use super::dto::TrackOnlineMusicInput;
use super::{MusicOnlineService, MusicTrackingDeps, get_or_insert_weak_lock};

pub(super) const MAX_TRACKED_AUDIO_BYTES: u64 = 256 * 1024 * 1024;
const MAX_BATCH_TRACK_COUNT: usize = 200;

fn clean_opt(v: Option<String>) -> Option<String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn canonical_item_id(input: &TrackOnlineMusicInput) -> String {
    let tid = input.track_id.trim();
    if !tid.is_empty() {
        return tid.to_string();
    }
    if let Some(u) = input
        .detail_url
        .as_deref()
        .or(input.play_url.as_deref())
        .or(input.url_id.as_deref())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return u.to_string();
    }
    format!(
        "{}::{}",
        input.artist.trim().to_lowercase(),
        input.title.trim().to_lowercase()
    )
}

impl MusicOnlineService {
    pub(super) fn require_tracking(&self) -> DomainResult<&MusicTrackingDeps> {
        self.tracking
            .as_ref()
            .ok_or_else(|| DomainError::Storage("音乐追更存储依赖未初始化".into()))
    }

    async fn track_single_inner(
        &self,
        actor: &User,
        deps: &MusicTrackingDeps,
        managed_lib_id: LibraryId,
        existing_tracks: &mut Vec<Track>,
        input: TrackOnlineMusicInput,
    ) -> DomainResult<(Track, bool)> {
        let title = input.title.trim().to_string();
        if title.is_empty() {
            return Err(DomainError::Validation("曲目标题不能为空".into()));
        }
        let _src_rec = self
            .sources
            .load_source_for_actor(actor, input.source_id)
            .await?;
        let item_id = canonical_item_id(&input);

        let track_lock =
            get_or_insert_weak_lock(&self.track_locks, (input.source_id, item_id.clone()));
        let _track_guard = track_lock.lock().await;

        let fresh_existing = deps
            .catalog_repo
            .list_tracks_by_library(managed_lib_id)
            .await?;
        *existing_tracks = fresh_existing;

        if let Some(existing) = existing_tracks.iter().find(|tr| {
            matches!(
                &tr.source,
                TrackSource::OnlineTracked(info)
                    if info.source_id == input.source_id && info.item_id == item_id
            )
        }) {
            return Ok((existing.clone(), false));
        }

        let artist_name = Artist::normalize_name(&input.artist);
        let album_title = Album::normalize_title(input.album.as_deref().unwrap_or(""));

        let track_id = TrackId::new();
        let mut cover_url = clean_opt(input.cover_url.clone());
        if cover_url.is_none()
            && input
                .cover_id
                .as_deref()
                .is_some_and(|s| !s.trim().is_empty())
        {
            let ctx = MusicTrackContext {
                track_id: item_id.clone(),
                title: title.clone(),
                artist: artist_name.clone(),
                album: Some(album_title.clone()),
                duration_secs: input.duration_secs,
                cover_id: clean_opt(input.cover_id.clone()),
                detail_url: clean_opt(input.detail_url.clone()),
                ..Default::default()
            };
            cover_url = self.resolve_cover_url_internal(input.source_id, ctx).await;
        }

        let cover_key = self
            .fetch_and_save_cover_best_effort(
                deps,
                input.source_id,
                &track_id.0.to_string(),
                cover_url.as_deref(),
            )
            .await;

        let artist = deps
            .ingest_service
            .ensure_artist(&artist_name, true, true, cover_key.as_deref())
            .await?;
        let album = deps
            .ingest_service
            .ensure_album(
                managed_lib_id,
                &album_title,
                artist.id,
                &artist.name,
                None,
                None,
                false,
                cover_key.as_deref(),
            )
            .await?;

        let final_cover = cover_key.or_else(|| album.cover_key.clone());
        let format = input
            .format
            .as_deref()
            .and_then(|f| AudioFormat::parse(f).ok())
            .or_else(|| {
                input
                    .play_url
                    .as_deref()
                    .and_then(infer_audio_format)
                    .and_then(|ext| AudioFormat::parse(&ext).ok())
            })
            .unwrap_or(AudioFormat::Mp3);

        let now = Timestamp::now();
        let track = Track {
            id: track_id,
            library_id: managed_lib_id,
            album_id: album.id,
            artist_id: artist.id,
            album_artist_id: artist.id,
            title,
            artist_name: artist.name.clone(),
            album_artist_name: artist.name,
            album_title: album.title,
            track_number: None,
            disc_number: None,
            year: None,
            genre: None,
            duration_secs: input.duration_secs.unwrap_or(0),
            bit_rate_kbps: None,
            sample_rate_hz: None,
            bit_depth: None,
            channels: None,
            format,
            file_size: 0,
            file_hash: format!("online:{}:{item_id}", input.source_id),
            replay_gain: ReplayGain::default(),
            has_embedded_lyrics: false,
            has_sidecar_lyrics: false,
            is_compilation: false,
            cover_key: final_cover,
            source: TrackSource::OnlineTracked(Box::new(OnlineTrackedTrackSource {
                storage_key: None,
                source_id: input.source_id,
                item_id,
                detail_url: clean_opt(input.detail_url),
                play_url: clean_opt(input.play_url),
                url_id: clean_opt(input.url_id),
                cover_url,
                cover_id: clean_opt(input.cover_id),
                lyric_id: clean_opt(input.lyric_id),
                quality: clean_opt(input.quality),
                cached: false,
                sidecar_lrc_key: None,
            })),
            is_missing: false,
            created_at: now,
            updated_at: now,
        };

        deps.catalog_repo.save_track(&track).await?;
        existing_tracks.push(track.clone());
        Ok((track, true))
    }

    pub async fn track_online_track(
        &self,
        actor: &User,
        input: TrackOnlineMusicInput,
    ) -> DomainResult<TrackDto> {
        let deps = self.require_tracking()?;
        let managed_lib = deps.ingest_service.ensure_default_managed_library().await?;
        let mut existing = deps
            .catalog_repo
            .list_tracks_by_library(managed_lib.id)
            .await?;

        let (track, created) = self
            .track_single_inner(actor, deps, managed_lib.id, &mut existing, input)
            .await?;

        if created {
            deps.catalog_repo.recompute_catalog_aggregates().await?;
            deps.events.publish(SystemEvent::MediaChanged {
                media_kind: MediaKind::Music,
            });
        }
        deps.catalog_service
            .bind_track_for_user(actor.id, track.id)
            .await?;

        let mut list = deps
            .catalog_service
            .enrich_tracks(actor.id, vec![track])
            .await?;
        list.pop()
            .ok_or_else(|| DomainError::NotFound("track not found".into()))
    }

    pub async fn track_online_tracks_batch(
        &self,
        actor: &User,
        inputs: Vec<TrackOnlineMusicInput>,
    ) -> DomainResult<Vec<TrackDto>> {
        if inputs.is_empty() {
            return Err(DomainError::Validation("请至少选择一首曲目".into()));
        }
        if inputs.len() > MAX_BATCH_TRACK_COUNT {
            return Err(DomainError::Validation(format!(
                "单次最多批量加入 {MAX_BATCH_TRACK_COUNT} 首曲目"
            )));
        }

        let deps = self.require_tracking()?;
        let managed_lib = deps.ingest_service.ensure_default_managed_library().await?;
        let mut existing = deps
            .catalog_repo
            .list_tracks_by_library(managed_lib.id)
            .await?;

        let mut tracks = Vec::with_capacity(inputs.len());
        let mut any_created = false;
        for item in inputs {
            let (track, created) = self
                .track_single_inner(actor, deps, managed_lib.id, &mut existing, item)
                .await?;
            any_created |= created;
            tracks.push(track);
        }

        if any_created {
            deps.catalog_repo.recompute_catalog_aggregates().await?;
            deps.events.publish(SystemEvent::MediaChanged {
                media_kind: MediaKind::Music,
            });
        }
        let track_ids: Vec<TrackId> = tracks.iter().map(|t| t.id).collect();
        deps.catalog_service
            .bind_tracks_for_user(actor.id, &track_ids)
            .await?;

        deps.catalog_service.enrich_tracks(actor.id, tracks).await
    }

    pub async fn cache_tracked_track(
        &self,
        actor: &User,
        track_id: TrackId,
    ) -> DomainResult<TrackDto> {
        let deps = self.require_tracking()?;
        deps.catalog_service
            .require_track_bound(actor.id, track_id)
            .await?;
        self.ensure_tracked_track_cached(track_id).await?;
        deps.catalog_service.get_track(actor.id, track_id).await
    }
}

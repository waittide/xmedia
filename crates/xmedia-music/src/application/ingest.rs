use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use jiff::Timestamp;
use uuid::Uuid;
use xmedia_core::application::bindings::{
    BindMediaItemsResultDto, MediaBindingService, UnbindMediaItemResultDto,
};
use xmedia_core::application::ports::{RealtimeEventPort, SystemEvent};
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::{Library, LibraryId, MediaKind};
use xmedia_core::domain::repository::LibraryRepository;
use xmedia_core::domain::user::{User, UserId};

use crate::application::dto::{
    BindMusicItemsInput, GlobalScanStatusDto, PoolTrackDto, UploadBatchResultDto,
    UploadFilePathItem,
};
use crate::application::ports::{AudioMetadataProbePort, MusicStoragePort, ProbedAudioMetadata};
use crate::application::projector::MusicProjector;
use crate::domain::catalog::{
    Album, AlbumId, AlbumSpec, Artist, ArtistId, AudioFormat, PagedResult, ProbedTrackInit, Track,
    TrackId, TrackQuery, TrackSource,
};
use crate::domain::repository::{
    AlbumScope, ArtistScope, CatalogFacetQuery, CatalogFacetResult, CatalogMutation,
    CollectionPurgeScope, MusicCatalogRepository, MusicCollectionRepository, TrackDeleteScope,
    TrackScope,
};

#[derive(Clone)]
pub struct MusicIngestService {
    libraries: Arc<dyn LibraryRepository>,
    catalog_repo: Arc<dyn MusicCatalogRepository>,
    collections: Arc<dyn MusicCollectionRepository>,
    bindings: Arc<MediaBindingService>,
    projector: MusicProjector,
    storage: Arc<dyn MusicStoragePort>,
    probe: Arc<dyn AudioMetadataProbePort>,
    events: Arc<dyn RealtimeEventPort>,
    managed_lib_lock: Arc<tokio::sync::Mutex<()>>,
}

impl MusicIngestService {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        libraries: Arc<dyn LibraryRepository>,
        catalog_repo: Arc<dyn MusicCatalogRepository>,
        collections: Arc<dyn MusicCollectionRepository>,
        bindings: Arc<MediaBindingService>,
        projector: MusicProjector,
        storage: Arc<dyn MusicStoragePort>,
        probe: Arc<dyn AudioMetadataProbePort>,
        events: Arc<dyn RealtimeEventPort>,
    ) -> Self {
        Self {
            libraries,
            catalog_repo,
            collections,
            bindings,
            projector,
            storage,
            probe,
            events,
            managed_lib_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    pub async fn get_scan_status(&self, user_id: UserId) -> DomainResult<GlobalScanStatusDto> {
        let libs = self.libraries.list(Some(MediaKind::Music)).await?;
        let scanning = libs.iter().any(|l| l.scan_status.is_scanning());
        let allowed = self.projector.bound_track_ids(user_id).await?;
        let count = match self
            .catalog_repo
            .query_facets(CatalogFacetQuery::LibraryTrackCounts(&allowed))
            .await?
        {
            CatalogFacetResult::LibraryTrackCounts(by_lib) => by_lib.values().sum(),
            _ => 0,
        };
        Ok(GlobalScanStatusDto { scanning, count })
    }

    pub async fn list_pool_tracks(
        &self,
        user_id: UserId,
        mut query: TrackQuery,
    ) -> DomainResult<PagedResult<PoolTrackDto>> {
        query.exclude_online_tracked = true;
        let bound_ids = self
            .bindings
            .list_bound_ids(user_id, MediaKind::Music)
            .await?;
        let paged = self.catalog_repo.search_tracks(None, &query).await?;
        let track_dtos = self.projector.enrich_tracks(user_id, paged.items).await?;
        let items = track_dtos
            .into_iter()
            .map(|track| {
                let bound = bound_ids.contains(&track.id.0);
                PoolTrackDto { track, bound }
            })
            .collect();
        Ok(PagedResult {
            items,
            total: paged.total,
            page: paged.page,
            page_size: paged.page_size,
        })
    }

    pub async fn upload_files_from_paths(
        &self,
        actor: &User,
        files: Vec<UploadFilePathItem>,
    ) -> DomainResult<UploadBatchResultDto> {
        actor.ensure_admin()?;
        if files.is_empty() {
            return Err(DomainError::Validation("no files uploaded".into()));
        }

        let managed_lib = self.ensure_default_managed_library().await?;

        let mut audio_items: Vec<(String, String, PathBuf)> = Vec::new();
        let mut lrc_by_stem: HashMap<String, PathBuf> = HashMap::new();

        for item in files {
            let (stem, ext) = split_filename_stem_and_ext(&item.filename);
            if ext.eq_ignore_ascii_case("lrc") {
                lrc_by_stem.insert(stem.to_lowercase(), item.temp_path);
            } else if ext.parse::<AudioFormat>().is_ok() {
                audio_items.push((stem, ext, item.temp_path));
            } else {
                return Err(DomainError::UnsupportedFormat(format!(
                    "unsupported file extension '.{ext}' for '{}'",
                    item.filename
                )));
            }
        }

        let mut created_tracks: Vec<Track> = Vec::new();
        let mut attached_lyrics_count = 0usize;
        let mut consumed_lrc_stems: HashSet<String> = HashSet::new();

        for (stem, ext, audio_path) in audio_items {
            let probe = self.probe.clone();
            let path_str = audio_path.to_string_lossy().into_owned();
            let probe_ext = ext.clone();
            let probe_stem = stem.clone();
            let probed = tokio::task::spawn_blocking(move || {
                probe.probe_audio_file(&path_str, &probe_ext, &probe_stem)
            })
            .await
            .map_err(|e| DomainError::Storage(format!("audio probe task failed: {e}")))??;

            let track_id = TrackId::new();
            let stored = self
                .storage
                .save_managed_audio_from_path(track_id, probed.format, &audio_path)
                .await?;

            let stem_lower = stem.to_lowercase();
            let sidecar_lrc_key = if let Some(lrc_path) = lrc_by_stem.get(&stem_lower) {
                consumed_lrc_stems.insert(stem_lower);
                attached_lyrics_count += 1;
                Some(
                    self.storage
                        .save_managed_sidecar_lrc_from_path(track_id, lrc_path)
                        .await?,
                )
            } else {
                None
            };

            let cover_key = if let Some(ref embedded_cover) = probed.cover {
                Some(
                    self.storage
                        .save_cover(&track_id.0.to_string(), embedded_cover)
                        .await?,
                )
            } else {
                None
            };

            let has_sidecar = sidecar_lrc_key.is_some();
            let source = TrackSource::Managed {
                storage_key: stored.storage_key,
                sidecar_lrc_key,
            };

            let track = self
                .upsert_probed_track(
                    managed_lib.id,
                    track_id,
                    None,
                    probed,
                    stored.file_size,
                    stored.file_hash,
                    has_sidecar,
                    cover_key,
                    source,
                )
                .await?;

            self.catalog_repo
                .save(CatalogMutation::Track(&track))
                .await?;
            created_tracks.push(track);
        }

        if lrc_by_stem.len() > consumed_lrc_stems.len() {
            let existing_managed = self
                .catalog_repo
                .query_tracks(TrackScope::Library(managed_lib.id))
                .await?;
            for (stem_lower, lrc_path) in lrc_by_stem {
                if consumed_lrc_stems.contains(&stem_lower) {
                    continue;
                }
                if let Some(mut matched) = existing_managed
                    .iter()
                    .find(|t| t.title.trim().to_lowercase() == stem_lower)
                    .cloned()
                    && matches!(matched.source, TrackSource::Managed { .. })
                {
                    let key = self
                        .storage
                        .save_managed_sidecar_lrc_from_path(matched.id, &lrc_path)
                        .await?;
                    if matched.attach_sidecar_lrc(key, Timestamp::now()) {
                        self.catalog_repo
                            .save(CatalogMutation::Track(&matched))
                            .await?;
                        attached_lyrics_count += 1;
                    }
                }
            }
        }

        if created_tracks.is_empty() && attached_lyrics_count == 0 {
            return Err(DomainError::Validation(
                "no audio tracks or matching lyrics were imported".into(),
            ));
        }

        self.catalog_repo
            .save(CatalogMutation::RecomputeAggregates)
            .await?;
        let created_ids: Vec<Uuid> = created_tracks.iter().map(|t| t.id.0).collect();
        if !created_ids.is_empty() {
            self.bindings
                .bind_batch(actor.id, MediaKind::Music, &created_ids)
                .await?;
        }
        self.events.publish(SystemEvent::MediaChanged {
            media_kind: MediaKind::Music,
        });
        let uploaded_dtos = self
            .projector
            .enrich_tracks(actor.id, created_tracks)
            .await?;

        Ok(UploadBatchResultDto {
            uploaded_tracks: uploaded_dtos,
            attached_lyrics_count,
        })
    }

    pub async fn bind_tracks(
        &self,
        actor: &User,
        input: BindMusicItemsInput,
    ) -> DomainResult<BindMediaItemsResultDto> {
        let target_user_id = MediaBindingService::resolve_target_user(actor, input.user_id)?;
        let mut to_bind: Vec<Uuid> = Vec::new();
        if let Some(lib_uuid) = input.library_id {
            for tr in self
                .catalog_repo
                .query_tracks(TrackScope::Library(LibraryId(lib_uuid)))
                .await?
            {
                if !tr.is_missing && !matches!(tr.source, TrackSource::OnlineTracked(_)) {
                    to_bind.push(tr.id.0);
                }
            }
        }
        if let Some(alb_uuid) = input.album_id {
            for tr in self
                .catalog_repo
                .query_tracks(TrackScope::Album {
                    album_id: AlbumId(alb_uuid),
                    allowed: None,
                })
                .await?
            {
                if !tr.is_missing && !matches!(tr.source, TrackSource::OnlineTracked(_)) {
                    to_bind.push(tr.id.0);
                }
            }
        }
        if let Some(ar_uuid) = input.artist_id {
            for tr in self
                .catalog_repo
                .query_tracks(TrackScope::Artist {
                    artist_id: ArtistId(ar_uuid),
                    allowed: None,
                })
                .await?
            {
                if !tr.is_missing && !matches!(tr.source, TrackSource::OnlineTracked(_)) {
                    to_bind.push(tr.id.0);
                }
            }
        }
        for raw_id in input.item_ids {
            let Some(tr) = self
                .catalog_repo
                .query_tracks(TrackScope::Id(TrackId(raw_id)))
                .await?
                .into_iter()
                .next()
            else {
                continue;
            };
            if tr.is_missing {
                continue;
            }
            if matches!(tr.source, TrackSource::OnlineTracked(_))
                && !actor.role.is_admin()
                && !self
                    .bindings
                    .is_bound(actor.id, MediaKind::Music, tr.id.0)
                    .await?
            {
                continue;
            }
            to_bind.push(tr.id.0);
        }
        let total_requested = to_bind.len();
        let bound_count = self
            .bindings
            .bind_batch(target_user_id, MediaKind::Music, &to_bind)
            .await?;
        Ok(BindMediaItemsResultDto {
            bound_count,
            total_requested,
        })
    }

    pub async fn unbind_track(
        &self,
        actor: &User,
        track_id: TrackId,
        requested_user_id: Option<Uuid>,
    ) -> DomainResult<UnbindMediaItemResultDto> {
        let target_user_id = MediaBindingService::resolve_target_user(actor, requested_user_id)?;
        let (unbound, remaining_bindings, deleted_from_pool) = self
            .bindings
            .unbind_with_gc(target_user_id, MediaKind::Music, track_id.0, || async {
                if let Some(tr) = self
                    .catalog_repo
                    .query_tracks(TrackScope::Id(track_id))
                    .await?
                    .into_iter()
                    .next()
                    && matches!(tr.source, TrackSource::OnlineTracked(_))
                {
                    self.delete_track_without_bindings(tr).await?;
                    return Ok(true);
                }
                Ok(false)
            })
            .await?;
        if unbound && !deleted_from_pool {
            self.collections
                .purge(CollectionPurgeScope::UserTrack {
                    user_id: target_user_id,
                    track_id,
                })
                .await?;
        }
        Ok(UnbindMediaItemResultDto {
            unbound,
            remaining_bindings,
            deleted_from_pool,
        })
    }

    pub async fn delete_track(&self, actor: &User, track_id: TrackId) -> DomainResult<()> {
        actor.ensure_admin()?;
        let track = self
            .catalog_repo
            .query_tracks(TrackScope::Id(track_id))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound("track not found".into()))?;
        let _ = self
            .bindings
            .purge(xmedia_core::domain::repository::BindingDeleteScope::Item {
                media_kind: MediaKind::Music,
                item_id: track.id.0,
            })
            .await?;
        self.delete_track_without_bindings(track).await
    }

    pub(crate) async fn delete_track_without_bindings(&self, track: Track) -> DomainResult<()> {
        let track_id = track.id;
        for key in track.source.managed_file_keys() {
            let _ = self.storage.delete_managed_file(key).await;
        }

        self.catalog_repo
            .delete_tracks(TrackDeleteScope::Id(track_id))
            .await?;
        self.catalog_repo
            .save(CatalogMutation::RecomputeAggregates)
            .await?;

        let deleted_album_ids: Vec<AlbumId> = if self
            .catalog_repo
            .query_albums(AlbumScope::Id(track.album_id))
            .await?
            .items
            .is_empty()
        {
            vec![track.album_id]
        } else {
            Vec::new()
        };
        let mut deleted_artist_ids = Vec::new();
        for cand_artist in [track.artist_id, track.album_artist_id] {
            if !deleted_artist_ids.contains(&cand_artist)
                && self
                    .catalog_repo
                    .query_artists(ArtistScope::Id(cand_artist))
                    .await?
                    .items
                    .is_empty()
            {
                deleted_artist_ids.push(cand_artist);
            }
        }
        self.collections
            .purge(CollectionPurgeScope::DeletedCatalog {
                track_ids: &[track_id],
                album_ids: &deleted_album_ids,
                artist_ids: &deleted_artist_ids,
            })
            .await?;

        self.events.publish(SystemEvent::MediaChanged {
            media_kind: MediaKind::Music,
        });
        Ok(())
    }

    pub(crate) async fn ensure_default_managed_library(&self) -> DomainResult<Library> {
        let _guard = self.managed_lib_lock.lock().await;
        if let Some(existing) = self
            .libraries
            .find_managed_by_media(MediaKind::Music)
            .await?
        {
            return Ok(existing);
        }
        let lib = Library::new_managed(MediaKind::Music)?;
        self.libraries.save(&lib).await?;
        Ok(lib)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn upsert_probed_track(
        &self,
        library_id: LibraryId,
        track_id: TrackId,
        existing_created_at: Option<Timestamp>,
        probed: ProbedAudioMetadata,
        file_size: u64,
        file_hash: String,
        has_sidecar_lyrics: bool,
        cover_key: Option<String>,
        source: TrackSource,
    ) -> DomainResult<Track> {
        let now = Timestamp::now();
        let same_artist = probed
            .album_artist
            .trim()
            .eq_ignore_ascii_case(probed.track_artist.trim());

        let track_artist = self
            .ensure_artist(
                &probed.track_artist,
                same_artist,
                true,
                cover_key.as_deref(),
            )
            .await?;
        let album_artist = if same_artist {
            track_artist.clone()
        } else {
            self.ensure_artist(&probed.album_artist, true, false, cover_key.as_deref())
                .await?
        };

        let album = self
            .ensure_album(AlbumSpec {
                library_id,
                raw_title: &probed.album_title,
                album_artist_id: album_artist.id,
                album_artist_name: &album_artist.name,
                year: probed.year,
                genre: probed.genre.as_deref(),
                is_compilation: probed.is_compilation,
                cover_hint: cover_key.as_deref(),
            })
            .await?;

        let final_cover = cover_key.or_else(|| album.cover_key.clone());

        Ok(Track::from_probed(
            ProbedTrackInit {
                id: track_id,
                library_id,
                album_id: album.id,
                album_title: album.title,
                artist_id: track_artist.id,
                artist_name: track_artist.name,
                album_artist_id: album_artist.id,
                album_artist_name: album_artist.name,
                title: probed.title,
                track_number: probed.track_number,
                disc_number: probed.disc_number,
                year: probed.year,
                genre: probed.genre,
                duration_secs: probed.duration_secs,
                bit_rate_kbps: probed.bit_rate_kbps,
                sample_rate_hz: probed.sample_rate_hz,
                bit_depth: probed.bit_depth,
                channels: probed.channels,
                format: probed.format,
                file_size,
                file_hash,
                replay_gain: probed.replay_gain,
                has_embedded_lyrics: probed.has_embedded_lyrics,
                has_sidecar_lyrics,
                is_compilation: probed.is_compilation,
                cover_key: final_cover,
                source,
                existing_created_at,
            },
            now,
        ))
    }

    pub(crate) async fn ensure_artist(
        &self,
        raw_name: &str,
        as_album_artist: bool,
        as_track_artist: bool,
        cover_hint: Option<&str>,
    ) -> DomainResult<Artist> {
        let name = Artist::normalize_name(raw_name);
        let existing = self
            .catalog_repo
            .query_artists(ArtistScope::Name(&name))
            .await?
            .items
            .into_iter()
            .next();
        let (artist, dirty) = Artist::reconcile(
            existing,
            &name,
            as_album_artist,
            as_track_artist,
            cover_hint,
            Timestamp::now(),
        );
        if dirty {
            self.catalog_repo
                .save(CatalogMutation::Artist(&artist))
                .await?;
        }
        Ok(artist)
    }

    pub(crate) async fn ensure_album(&self, spec: AlbumSpec<'_>) -> DomainResult<Album> {
        let (_, album_key) = Album::lookup_key(
            spec.library_id,
            spec.raw_title,
            spec.album_artist_name,
            spec.is_compilation,
        );
        let existing = self
            .catalog_repo
            .query_albums(AlbumScope::Key(&album_key))
            .await?
            .items
            .into_iter()
            .next();
        let (album, dirty) = Album::reconcile(existing, spec, Timestamp::now());
        if dirty {
            self.catalog_repo
                .save(CatalogMutation::Album(&album))
                .await?;
        }
        Ok(album)
    }
}

fn split_filename_stem_and_ext(filename: &str) -> (String, String) {
    let p = Path::new(filename);
    let stem = p
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("untitled")
        .trim()
        .to_string();
    let ext = p
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .trim()
        .to_string();
    (stem, ext)
}

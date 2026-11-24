use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use jiff::Timestamp;
use uuid::Uuid;
use xmedia_core::application::bindings::{
    BindMediaItemsResultDto, MediaBindingService, UnbindMediaItemResultDto,
};
use xmedia_core::application::library::ScanProgressReporter;
use xmedia_core::application::ports::{MediaDomainLifecycle, RealtimeEventPort, SystemEvent};
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::{Library, LibraryId, LibraryKind, MediaKind};
use xmedia_core::domain::repository::LibraryRepository;
use xmedia_core::domain::user::{User, UserId, require_admin};
use xmedia_core::http::TempUploadFile;

use crate::application::catalog::MusicCatalogService;
use crate::application::dto::{
    BindMusicItemsInput, GlobalScanStatusDto, UploadBatchResultDto, UploadFileItem,
    UploadFilePathItem,
};
use crate::application::ports::{
    AudioMetadataProbePort, MusicScannerPort, MusicStoragePort, ProbedAudioMetadata,
};
use crate::domain::catalog::{
    Album, AlbumId, Artist, ArtistId, AudioFormat, Track, TrackId, TrackSource,
};
use crate::domain::repository::{MusicCatalogRepository, MusicCollectionRepository};

#[derive(Clone)]
pub struct MusicIngestService {
    libraries: Arc<dyn LibraryRepository>,
    catalog_repo: Arc<dyn MusicCatalogRepository>,
    collections: Arc<dyn MusicCollectionRepository>,
    bindings: Arc<MediaBindingService>,
    catalog_service: MusicCatalogService,
    storage: Arc<dyn MusicStoragePort>,
    scanner: Arc<dyn MusicScannerPort>,
    probe: Arc<dyn AudioMetadataProbePort>,
    events: Arc<dyn RealtimeEventPort>,
}

impl MusicIngestService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        libraries: Arc<dyn LibraryRepository>,
        catalog_repo: Arc<dyn MusicCatalogRepository>,
        collections: Arc<dyn MusicCollectionRepository>,
        bindings: Arc<MediaBindingService>,
        catalog_service: MusicCatalogService,
        storage: Arc<dyn MusicStoragePort>,
        scanner: Arc<dyn MusicScannerPort>,
        probe: Arc<dyn AudioMetadataProbePort>,
        events: Arc<dyn RealtimeEventPort>,
    ) -> Self {
        Self {
            libraries,
            catalog_repo,
            collections,
            bindings,
            catalog_service,
            storage,
            scanner,
            probe,
            events,
        }
    }

    pub async fn get_scan_status(&self, user_id: UserId) -> DomainResult<GlobalScanStatusDto> {
        let libs = self.libraries.list_by_media(MediaKind::Music).await?;
        let scanning = libs.iter().any(|l| l.scan_status.is_scanning());
        let count = self.catalog_service.load_bound_tracks(user_id).await?.len() as u64;
        Ok(GlobalScanStatusDto { scanning, count })
    }

    pub async fn upload_files(
        &self,
        actor: &User,
        files: Vec<UploadFileItem>,
    ) -> DomainResult<UploadBatchResultDto> {
        require_admin(actor)?;
        if files.is_empty() {
            return Err(DomainError::Validation("no files uploaded".into()));
        }

        let mut temp_guards = Vec::with_capacity(files.len());
        let mut path_items = Vec::with_capacity(files.len());
        for item in files {
            let tmp = TempUploadFile::from_bytes(&item.bytes).await?;
            path_items.push(UploadFilePathItem {
                filename: item.filename,
                temp_path: tmp.path().to_path_buf(),
            });
            temp_guards.push(tmp);
        }

        let result = self.upload_files_from_paths(actor, path_items).await;
        drop(temp_guards);
        result
    }

    pub async fn upload_files_from_paths(
        &self,
        actor: &User,
        files: Vec<UploadFilePathItem>,
    ) -> DomainResult<UploadBatchResultDto> {
        require_admin(actor)?;
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
            } else if AudioFormat::from_extension(&ext).is_some() {
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

            self.catalog_repo.save_track(&track).await?;
            created_tracks.push(track);
        }

        if lrc_by_stem.len() > consumed_lrc_stems.len() {
            let existing_managed = self
                .catalog_repo
                .list_tracks_by_library(managed_lib.id)
                .await?;
            for (stem_lower, lrc_path) in lrc_by_stem {
                if consumed_lrc_stems.contains(&stem_lower) {
                    continue;
                }
                if let Some(mut matched) = existing_managed
                    .iter()
                    .find(|t| t.title.trim().to_lowercase() == stem_lower)
                    .cloned()
                    && let TrackSource::Managed {
                        ref storage_key, ..
                    } = matched.source
                {
                    let key = self
                        .storage
                        .save_managed_sidecar_lrc_from_path(matched.id, &lrc_path)
                        .await?;
                    matched.source = TrackSource::Managed {
                        storage_key: storage_key.clone(),
                        sidecar_lrc_key: Some(key),
                    };
                    matched.has_sidecar_lyrics = true;
                    matched.updated_at = Timestamp::now();
                    self.catalog_repo.save_track(&matched).await?;
                    attached_lyrics_count += 1;
                }
            }
        }

        if created_tracks.is_empty() && attached_lyrics_count == 0 {
            return Err(DomainError::Validation(
                "no audio tracks or matching lyrics were imported".into(),
            ));
        }

        self.catalog_repo.recompute_catalog_aggregates().await?;
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
            .catalog_service
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
        let target_user_id = MediaBindingService::resolve_target_user_id(actor, input.user_id)?;
        let mut to_bind: Vec<Uuid> = Vec::new();
        if let Some(lib_uuid) = input.library_id {
            for tr in self
                .catalog_repo
                .list_tracks_by_library(LibraryId(lib_uuid))
                .await?
            {
                if !tr.is_missing && !tr.source.is_online_tracked() {
                    to_bind.push(tr.id.0);
                }
            }
        }
        if let Some(alb_uuid) = input.album_id {
            for tr in self
                .catalog_repo
                .list_tracks_by_album(AlbumId(alb_uuid))
                .await?
            {
                if !tr.is_missing && !tr.source.is_online_tracked() {
                    to_bind.push(tr.id.0);
                }
            }
        }
        if let Some(ar_uuid) = input.artist_id {
            for tr in self
                .catalog_repo
                .list_tracks_by_artist(ArtistId(ar_uuid))
                .await?
            {
                if !tr.is_missing && !tr.source.is_online_tracked() {
                    to_bind.push(tr.id.0);
                }
            }
        }
        for raw_id in input.item_ids {
            let Some(tr) = self.catalog_repo.find_track_by_id(TrackId(raw_id)).await? else {
                continue;
            };
            if tr.is_missing {
                continue;
            }
            if tr.source.is_online_tracked()
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
        let target_user_id = MediaBindingService::resolve_target_user_id(actor, requested_user_id)?;
        let (unbound, remaining_bindings, deleted_from_pool) = self
            .bindings
            .unbind_with_gc(target_user_id, MediaKind::Music, track_id.0, || async {
                if let Some(tr) = self.catalog_repo.find_track_by_id(track_id).await?
                    && tr.source.is_online_tracked()
                {
                    self.delete_track_without_bindings(tr).await?;
                    return Ok(true);
                }
                Ok(false)
            })
            .await?;
        if unbound && !deleted_from_pool {
            self.collections
                .delete_by_user_and_track(target_user_id, track_id)
                .await?;
        }
        Ok(UnbindMediaItemResultDto {
            unbound,
            remaining_bindings,
            deleted_from_pool,
        })
    }

    async fn delete_track_internal(&self, track: Track) -> DomainResult<()> {
        self.bindings
            .delete_item_bindings(MediaKind::Music, track.id.0)
            .await?;
        self.delete_track_without_bindings(track).await
    }

    async fn delete_track_without_bindings(&self, track: Track) -> DomainResult<()> {
        let track_id = track.id;
        for key in track.source.managed_file_keys() {
            let _ = self.storage.delete_managed_file(key).await;
        }

        self.catalog_repo.delete_track(track_id).await?;
        self.catalog_repo.recompute_catalog_aggregates().await?;

        let deleted_album_ids: Vec<AlbumId> = if self
            .catalog_repo
            .find_album_by_id(track.album_id)
            .await?
            .is_none()
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
                    .find_artist_by_id(cand_artist)
                    .await?
                    .is_none()
            {
                deleted_artist_ids.push(cand_artist);
            }
        }
        self.collections
            .delete_tracks_references(&[track_id], &deleted_album_ids, &deleted_artist_ids)
            .await?;

        self.events.publish(SystemEvent::MediaChanged {
            media_kind: MediaKind::Music,
        });
        Ok(())
    }

    pub async fn delete_track(&self, actor: &User, track_id: TrackId) -> DomainResult<()> {
        require_admin(actor)?;
        let track = self
            .catalog_repo
            .find_track_by_id(track_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("track not found".into()))?;
        self.delete_track_internal(track).await
    }

    pub(crate) async fn ensure_default_managed_library(&self) -> DomainResult<Library> {
        if let Some(existing) = self
            .libraries
            .find_managed_by_media(MediaKind::Music)
            .await?
        {
            return Ok(existing);
        }
        let lib = Library::new_managed(MediaKind::Music, MediaKind::Music.default_managed_name())?;
        self.libraries.save(&lib).await?;
        Ok(lib)
    }

    #[allow(clippy::too_many_arguments)]
    async fn upsert_probed_track(
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

        let track_artist = self
            .ensure_artist(&probed.track_artist, false, true, cover_key.as_deref())
            .await?;
        let album_artist = if probed
            .album_artist
            .trim()
            .eq_ignore_ascii_case(probed.track_artist.trim())
        {
            let mut same = track_artist.clone();
            if !same.is_album_artist {
                same.is_album_artist = true;
                same.updated_at = now;
                self.catalog_repo.save_artist(&same).await?;
            }
            same
        } else {
            self.ensure_artist(&probed.album_artist, true, false, cover_key.as_deref())
                .await?
        };

        let album = self
            .ensure_album(
                library_id,
                &probed.album_title,
                album_artist.id,
                &album_artist.name,
                probed.year,
                probed.genre.as_deref(),
                probed.is_compilation,
                cover_key.as_deref(),
            )
            .await?;

        let final_cover = cover_key.or_else(|| album.cover_key.clone());

        Ok(Track {
            id: track_id,
            library_id,
            album_id: album.id,
            artist_id: track_artist.id,
            album_artist_id: album_artist.id,
            title: probed.title,
            artist_name: track_artist.name,
            album_artist_name: album_artist.name,
            album_title: album.title,
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
            is_missing: false,
            created_at: existing_created_at.unwrap_or(now),
            updated_at: now,
        })
    }

    pub(crate) async fn ensure_artist(
        &self,
        raw_name: &str,
        as_album_artist: bool,
        as_track_artist: bool,
        cover_hint: Option<&str>,
    ) -> DomainResult<Artist> {
        let name = {
            let t = raw_name.trim();
            if t.is_empty() {
                "Unknown Artist".to_string()
            } else {
                t.to_string()
            }
        };
        let now = Timestamp::now();
        if let Some(mut existing) = self.catalog_repo.find_artist_by_name(&name).await? {
            let mut dirty = false;
            if as_album_artist && !existing.is_album_artist {
                existing.is_album_artist = true;
                dirty = true;
            }
            if as_track_artist && !existing.is_track_artist {
                existing.is_track_artist = true;
                dirty = true;
            }
            if existing.cover_key.is_none()
                && let Some(cov) = cover_hint
            {
                existing.cover_key = Some(cov.to_string());
                dirty = true;
            }
            if dirty {
                existing.updated_at = now;
                self.catalog_repo.save_artist(&existing).await?;
            }
            Ok(existing)
        } else {
            let artist = Artist {
                id: ArtistId::new(),
                name,
                is_album_artist: as_album_artist,
                is_track_artist: as_track_artist,
                album_count: 0,
                track_count: 0,
                cover_key: cover_hint.map(str::to_string),
                created_at: now,
                updated_at: now,
            };
            self.catalog_repo.save_artist(&artist).await?;
            Ok(artist)
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn ensure_album(
        &self,
        library_id: LibraryId,
        raw_title: &str,
        album_artist_id: ArtistId,
        album_artist_name: &str,
        year: Option<i32>,
        genre: Option<&str>,
        is_compilation: bool,
        cover_hint: Option<&str>,
    ) -> DomainResult<Album> {
        let title = {
            let t = raw_title.trim();
            if t.is_empty() {
                "Unknown Album".to_string()
            } else {
                t.to_string()
            }
        };
        let album_key = Album::compute_key(library_id, &title, album_artist_name, is_compilation);
        let now = Timestamp::now();

        if let Some(mut existing) = self.catalog_repo.find_album_by_key(&album_key).await? {
            let mut dirty = false;
            if year.is_some() && existing.year != year {
                existing.year = year;
                dirty = true;
            }
            if genre.is_some() && existing.genre.as_deref() != genre {
                existing.genre = genre.map(str::to_string);
                dirty = true;
            }
            if existing.cover_key.is_none()
                && let Some(cov) = cover_hint
            {
                existing.cover_key = Some(cov.to_string());
                dirty = true;
            }
            if dirty {
                existing.updated_at = now;
                self.catalog_repo.save_album(&existing).await?;
            }
            Ok(existing)
        } else {
            let album = Album {
                id: AlbumId::new(),
                library_id,
                album_key,
                title,
                album_artist_id,
                album_artist_name: album_artist_name.to_string(),
                year,
                genre: genre.map(str::to_string),
                disc_count: 1,
                track_count: 0,
                duration_secs: 0,
                is_compilation,
                cover_key: cover_hint.map(str::to_string),
                created_at: now,
                updated_at: now,
            };
            self.catalog_repo.save_album(&album).await?;
            Ok(album)
        }
    }
}

#[async_trait]
impl MediaDomainLifecycle for MusicIngestService {
    fn media_kind(&self) -> MediaKind {
        MediaKind::Music
    }

    async fn scan_library(&self, library: Library) -> DomainResult<()> {
        let LibraryKind::LocalMount { ref root_path } = library.kind else {
            return Err(DomainError::Validation(
                "managed upload library cannot be scanned".into(),
            ));
        };
        let root_path = root_path.clone();
        let library_id = library.id;

        let entries = self.scanner.scan_audio_entries(&root_path).await?;
        let entry_paths: HashSet<String> =
            entries.iter().map(|e| e.relative_path.clone()).collect();
        let existing_tracks = self.catalog_repo.list_tracks_by_library(library_id).await?;
        let mut existing_by_rel: HashMap<String, Track> = HashMap::new();
        for tr in existing_tracks {
            if let TrackSource::LocalPath {
                ref relative_path, ..
            } = tr.source
            {
                existing_by_rel.insert(relative_path.clone(), tr);
            }
        }

        let total = entries.len() as u64;
        let mut reporter = ScanProgressReporter::new(
            library,
            self.libraries.clone(),
            self.events.clone(),
            Some(total),
        );
        reporter.start().await;

        let mut seen_rel: HashSet<String> = HashSet::new();
        let mut dir_cover_cache: HashMap<String, Option<String>> = HashMap::new();
        let mut added_count = 0u64;
        let mut updated_count = 0u64;

        for entry in entries {
            seen_rel.insert(entry.relative_path.clone());
            let has_sidecar = entry.sidecar_lrc_relative_path.is_some();

            let exact_prev = existing_by_rel.remove(&entry.relative_path);
            if let Some(mut prev) = exact_prev.clone()
                && let TrackSource::LocalPath {
                    file_mtime,
                    ref sidecar_lrc_path,
                    ..
                } = prev.source
                && file_mtime == entry.file_mtime
                && prev.file_size == entry.file_size
                && sidecar_lrc_path == &entry.sidecar_lrc_relative_path
            {
                if prev.is_missing {
                    prev.is_missing = false;
                    prev.updated_at = Timestamp::now();
                    self.catalog_repo.save_track(&prev).await?;
                    updated_count += 1;
                }
                reporter.advance().await;
                continue;
            }

            let file_hash = self
                .scanner
                .compute_file_fingerprint(&entry.absolute_path)
                .await?;

            let prev_track = match exact_prev {
                Some(prev) => Some(prev),
                None => {
                    let moved_key = existing_by_rel
                        .iter()
                        .find(|(old_rel, old_tr)| {
                            !entry_paths.contains(*old_rel) && old_tr.file_hash == file_hash
                        })
                        .map(|(k, _)| k.clone());
                    moved_key.and_then(|k| existing_by_rel.remove(&k))
                }
            };

            let probe = self.probe.clone();
            let probe_path = entry.absolute_path.clone();
            let probe_ext = entry.ext_hint.clone();
            let probe_fallback = entry.fallback_title.clone();
            let probed = match tokio::task::spawn_blocking(move || {
                probe.probe_audio_file(&probe_path, &probe_ext, &probe_fallback)
            })
            .await
            {
                Ok(Ok(p)) => p,
                Ok(Err(err)) => {
                    tracing::warn!(
                        library_id = %library_id,
                        path = %entry.relative_path,
                        error = %err,
                        "failed to probe audio file during scan"
                    );
                    if let Some(mut stale) = prev_track
                        && !stale.is_missing
                    {
                        stale.is_missing = true;
                        stale.updated_at = Timestamp::now();
                        let _ = self.catalog_repo.save_track(&stale).await;
                    }
                    reporter.advance().await;
                    continue;
                }
                Err(err) => {
                    tracing::warn!(
                        library_id = %library_id,
                        path = %entry.relative_path,
                        error = %err,
                        "audio probe task panicked during scan"
                    );
                    reporter.advance().await;
                    continue;
                }
            };

            let track_id = prev_track.as_ref().map(|t| t.id).unwrap_or_default();

            let mut cover_key: Option<String> = None;
            if let Some(ref embedded_cover) = probed.cover {
                cover_key = Some(
                    self.storage
                        .save_cover(&track_id.0.to_string(), embedded_cover)
                        .await?,
                );
            } else if let Some(ref dir_cover_abs) = entry.directory_cover_path {
                if let Some(cached) = dir_cover_cache.get(dir_cover_abs) {
                    cover_key = cached.clone();
                } else {
                    let loaded = self.scanner.read_directory_cover(dir_cover_abs).await?;
                    let saved = if let Some(ref payload) = loaded {
                        Some(
                            self.storage
                                .save_cover(&format!("dir_{}", track_id.0), payload)
                                .await?,
                        )
                    } else {
                        None
                    };
                    dir_cover_cache.insert(dir_cover_abs.clone(), saved.clone());
                    cover_key = saved;
                }
            }

            let source = TrackSource::LocalPath {
                relative_path: entry.relative_path.clone(),
                file_mtime: entry.file_mtime,
                sidecar_lrc_path: entry.sidecar_lrc_relative_path.clone(),
            };

            let track = self
                .upsert_probed_track(
                    library_id,
                    track_id,
                    prev_track.as_ref().map(|t| t.created_at),
                    probed,
                    entry.file_size,
                    file_hash,
                    has_sidecar,
                    cover_key,
                    source,
                )
                .await?;

            self.catalog_repo.save_track(&track).await?;
            if prev_track.is_some() {
                updated_count += 1;
            } else {
                added_count += 1;
            }

            reporter.advance().await;
        }

        let mut missing_count = 0u64;
        for (rel, mut old_track) in existing_by_rel {
            if !seen_rel.contains(&rel) && !old_track.is_missing {
                old_track.is_missing = true;
                old_track.updated_at = Timestamp::now();
                self.catalog_repo.save_track(&old_track).await?;
                missing_count += 1;
            }
        }

        if added_count > 0 || updated_count > 0 || missing_count > 0 {
            self.catalog_repo.recompute_catalog_aggregates().await?;
        }
        reporter
            .complete(added_count, updated_count, missing_count)
            .await?;

        Ok(())
    }

    async fn count_library_items(&self, library_id: LibraryId) -> DomainResult<u64> {
        let tracks = self.catalog_repo.list_tracks_by_library(library_id).await?;
        Ok(tracks
            .into_iter()
            .filter(|t| !t.is_missing && !t.source.is_online_tracked())
            .count() as u64)
    }

    async fn on_library_deleted(&self, library_id: LibraryId) -> DomainResult<()> {
        let tracks = self.catalog_repo.list_tracks_by_library(library_id).await?;
        let mut track_ids = Vec::with_capacity(tracks.len());
        let mut candidate_albums = HashSet::new();
        let mut candidate_artists = HashSet::new();
        let mut files_to_delete: Vec<String> = Vec::new();

        for tr in &tracks {
            track_ids.push(tr.id);
            candidate_albums.insert(tr.album_id);
            candidate_artists.insert(tr.artist_id);
            candidate_artists.insert(tr.album_artist_id);

            for key in tr.source.managed_file_keys() {
                files_to_delete.push(key.to_string());
            }
            self.bindings
                .delete_item_bindings(MediaKind::Music, tr.id.0)
                .await?;
        }

        self.catalog_repo
            .delete_tracks_by_library(library_id)
            .await?;
        self.catalog_repo.recompute_catalog_aggregates().await?;

        for key in files_to_delete {
            let _ = self.storage.delete_managed_file(&key).await;
        }

        let candidate_album_vec: Vec<AlbumId> = candidate_albums.into_iter().collect();
        let surviving_albums: HashSet<AlbumId> = self
            .catalog_repo
            .find_albums_by_ids(&candidate_album_vec)
            .await?
            .into_iter()
            .map(|a| a.id)
            .collect();
        let deleted_album_ids: Vec<AlbumId> = candidate_album_vec
            .into_iter()
            .filter(|id| !surviving_albums.contains(id))
            .collect();

        let candidate_artist_vec: Vec<ArtistId> = candidate_artists.into_iter().collect();
        let surviving_artists: HashSet<ArtistId> = self
            .catalog_repo
            .find_artists_by_ids(&candidate_artist_vec)
            .await?
            .into_iter()
            .map(|a| a.id)
            .collect();
        let deleted_artist_ids: Vec<ArtistId> = candidate_artist_vec
            .into_iter()
            .filter(|id| !surviving_artists.contains(id))
            .collect();

        self.collections
            .delete_tracks_references(&track_ids, &deleted_album_ids, &deleted_artist_ids)
            .await?;

        if !track_ids.is_empty() {
            self.events.publish(SystemEvent::MediaChanged {
                media_kind: MediaKind::Music,
            });
        }
        Ok(())
    }

    async fn on_user_deleted(&self, user_id: UserId) -> DomainResult<()> {
        self.collections.delete_by_user(user_id).await?;
        let unbound_ids = self
            .bindings
            .delete_user_bindings_for_media(user_id, MediaKind::Music)
            .await?;
        for raw_id in unbound_ids {
            let tid = TrackId(raw_id);
            self.bindings
                .gc_if_orphaned(MediaKind::Music, raw_id, || async {
                    if let Some(track) = self.catalog_repo.find_track_by_id(tid).await?
                        && track.source.is_online_tracked()
                    {
                        self.delete_track_without_bindings(track).await?;
                        return Ok(true);
                    }
                    Ok(false)
                })
                .await?;
        }
        Ok(())
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

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use jiff::Timestamp;
use xmedia_core::application::ports::{MediaDomainLifecycle, RealtimeEventPort, SystemEvent};
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::{Library, LibraryId, LibraryKind, MediaKind};
use xmedia_core::domain::repository::LibraryRepository;
use xmedia_core::domain::user::{User, UserId};

use crate::application::catalog::MusicCatalogService;
use crate::application::dto::{GlobalScanStatusDto, UploadBatchResultDto, UploadFileItem};
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
            catalog_service,
            storage,
            scanner,
            probe,
            events,
        }
    }

    pub async fn get_scan_status(&self) -> DomainResult<GlobalScanStatusDto> {
        let libs = self.libraries.list_by_media(MediaKind::Music).await?;
        let scanning = libs.iter().any(|l| l.scan_status.is_scanning());
        let count = self.catalog_repo.count_all_active_tracks().await?;
        Ok(GlobalScanStatusDto { scanning, count })
    }

    pub async fn upload_files(
        &self,
        actor: &User,
        files: Vec<UploadFileItem>,
    ) -> DomainResult<UploadBatchResultDto> {
        if !actor.role.is_admin() {
            return Err(DomainError::Forbidden(
                "administrator privileges required".into(),
            ));
        }
        if files.is_empty() {
            return Err(DomainError::Validation("no files uploaded".into()));
        }

        let managed_lib = self.ensure_default_managed_library().await?;

        let mut audio_items: Vec<(String, String, Bytes)> = Vec::new();
        let mut lrc_by_stem: HashMap<String, Bytes> = HashMap::new();

        for item in files {
            let (stem, ext) = split_filename_stem_and_ext(&item.filename);
            if ext.eq_ignore_ascii_case("lrc") {
                lrc_by_stem.insert(stem.to_lowercase(), item.bytes);
            } else if AudioFormat::from_extension(&ext).is_some() {
                audio_items.push((stem, ext, item.bytes));
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

        for (stem, ext, audio_bytes) in audio_items {
            let probed = self.probe.probe_audio_bytes(&audio_bytes, &ext, &stem)?;
            let track_id = TrackId::new();
            let stored = self
                .storage
                .save_managed_audio(track_id, probed.format, audio_bytes)
                .await?;

            let stem_lower = stem.to_lowercase();
            let sidecar_lrc_key = if let Some(lrc_bytes) = lrc_by_stem.get(&stem_lower) {
                consumed_lrc_stems.insert(stem_lower);
                attached_lyrics_count += 1;
                Some(
                    self.storage
                        .save_managed_sidecar_lrc(track_id, lrc_bytes.clone())
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
            for (stem_lower, lrc_bytes) in lrc_by_stem {
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
                        .save_managed_sidecar_lrc(matched.id, lrc_bytes)
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

        self.catalog_repo.recompute_catalog_aggregates().await?;
        let uploaded_dtos = self
            .catalog_service
            .enrich_tracks(actor.id, created_tracks)
            .await?;

        Ok(UploadBatchResultDto {
            uploaded_tracks: uploaded_dtos,
            attached_lyrics_count,
        })
    }

    async fn ensure_default_managed_library(&self) -> DomainResult<Library> {
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

    async fn ensure_artist(
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
    async fn ensure_album(
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
            if existing.year.is_none() && year.is_some() {
                existing.year = year;
                dirty = true;
            }
            if existing.genre.is_none() && genre.is_some() {
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

    async fn scan_library(&self, mut library: Library) -> DomainResult<()> {
        let LibraryKind::LocalMount { ref root_path } = library.kind else {
            return Err(DomainError::Validation(
                "managed upload library cannot be scanned".into(),
            ));
        };
        let root_path = root_path.clone();

        let entries = self.scanner.scan_audio_entries(&root_path).await?;
        let existing_tracks = self.catalog_repo.list_tracks_by_library(library.id).await?;
        let mut existing_by_rel: HashMap<String, Track> = HashMap::new();
        for tr in existing_tracks {
            if let TrackSource::LocalPath {
                ref relative_path, ..
            } = tr.source
            {
                existing_by_rel.insert(relative_path.clone(), tr);
            }
        }

        let mut seen_rel: HashSet<String> = HashSet::new();
        let mut dir_cover_cache: HashMap<String, Option<String>> = HashMap::new();
        let mut scanned_count = 0u64;
        let mut added_count = 0u64;
        let mut updated_count = 0u64;

        for entry in entries {
            scanned_count += 1;
            seen_rel.insert(entry.relative_path.clone());
            let has_sidecar = entry.sidecar_lrc_relative_path.is_some();

            if let Some(mut prev) = existing_by_rel.get(&entry.relative_path).cloned()
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
                continue;
            }

            let probed = match self.probe.probe_audio_file(
                &entry.absolute_path,
                &entry.ext_hint,
                &entry.fallback_title,
            ) {
                Ok(p) => p,
                Err(_) => continue,
            };

            let file_hash = self
                .scanner
                .compute_file_fingerprint(&entry.relative_path, entry.file_size, entry.file_mtime)
                .await?;

            let prev_track = existing_by_rel.get(&entry.relative_path);
            let track_id = prev_track.map(|t| t.id).unwrap_or_default();

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
                    library.id,
                    track_id,
                    prev_track.map(|t| t.created_at),
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

            if scanned_count.is_multiple_of(20) {
                library.mark_scanning(scanned_count);
                let _ = self.libraries.save(&library).await;
                self.events.publish(SystemEvent::LibraryScanProgress {
                    library_id: library.id,
                    media_kind: MediaKind::Music,
                    scanned: scanned_count,
                });
            }
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

        self.catalog_repo.recompute_catalog_aggregates().await?;
        library.mark_scan_completed(scanned_count, added_count, updated_count, missing_count);
        self.libraries.save(&library).await?;

        self.events.publish(SystemEvent::LibraryScanCompleted {
            library_id: library.id,
            media_kind: MediaKind::Music,
            scanned: scanned_count,
            added: added_count,
            updated: updated_count,
            missing: missing_count,
        });
        if added_count > 0 || updated_count > 0 || missing_count > 0 {
            self.events.publish(SystemEvent::MediaChanged {
                media_kind: MediaKind::Music,
            });
        }

        Ok(())
    }

    async fn count_library_items(&self, library_id: LibraryId) -> DomainResult<u64> {
        self.catalog_repo.count_tracks_by_library(library_id).await
    }

    async fn on_library_deleted(&self, library_id: LibraryId) -> DomainResult<()> {
        let tracks = self.catalog_repo.list_tracks_by_library(library_id).await?;
        for tr in &tracks {
            if let TrackSource::Managed {
                ref storage_key,
                ref sidecar_lrc_key,
            } = tr.source
            {
                let _ = self.storage.delete_managed_file(storage_key).await;
                if let Some(lrc) = sidecar_lrc_key {
                    let _ = self.storage.delete_managed_file(lrc).await;
                }
            }
        }
        self.catalog_repo
            .delete_tracks_by_library(library_id)
            .await?;
        self.catalog_repo.recompute_catalog_aggregates().await
    }

    async fn on_user_deleted(&self, user_id: UserId) -> DomainResult<()> {
        self.collections.delete_by_user(user_id).await
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

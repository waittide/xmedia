use std::collections::{HashMap, HashSet};

use async_trait::async_trait;
use jiff::Timestamp;
use xmedia_core::application::library::ScanProgressReporter;
use xmedia_core::application::ports::{MediaDomainLifecycle, SystemEvent};
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::{Library, LibraryId, LibraryKind, MediaKind};
use xmedia_core::domain::user::UserId;

use super::MusicIngestService;
use crate::domain::catalog::{AlbumId, ArtistId, Track, TrackId, TrackSource};

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
                if prev.mark_present(Timestamp::now()) {
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
                        && stale.mark_missing(Timestamp::now())
                    {
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
            if !seen_rel.contains(&rel) && old_track.mark_missing(Timestamp::now()) {
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

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use bytes::Bytes;
use walkdir::WalkDir;
use xmedia_core::application::ports::{BinaryPayload, ByteRangeSpec, ByteRangeStream};
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::{Library, LibraryKind};
use xmedia_core::infra::fingerprint::{fast_content_fingerprint, max_mtime_secs};
use xmedia_core::infra::stream::{
    move_or_copy_file, open_file_range_stream_spec, sanitize_join, sha256_file_hex, sha256_hex,
};
use xmedia_core::infra::thumbnail::{ThumbnailStore, sniff_image_mime};

use crate::application::ports::{
    MusicScannerPort, MusicStoragePort, ScannedAudioEntry, StoredManagedAudio,
};
use crate::domain::catalog::{AudioFormat, Track, TrackId, TrackSource};

#[derive(Clone)]
pub struct LocalMusicStorage {
    uploads_dir: PathBuf,
    lyrics_dir: PathBuf,
    thumbnails: ThumbnailStore,
}

impl LocalMusicStorage {
    pub fn new(data_dir: impl AsRef<Path>) -> DomainResult<Self> {
        let base = data_dir.as_ref().join("music");
        let uploads_dir = base.join("uploads");
        let lyrics_dir = base.join("lyrics");
        for dir in [&uploads_dir, &lyrics_dir] {
            std::fs::create_dir_all(dir).map_err(|e| {
                DomainError::Storage(format!("failed to create music dir {}: {e}", dir.display()))
            })?;
        }
        let thumbnails = ThumbnailStore::new(&base)?;
        Ok(Self {
            uploads_dir,
            lyrics_dir,
            thumbnails,
        })
    }

    fn resolve_track_path(&self, library: &Library, track: &Track) -> DomainResult<PathBuf> {
        match (&library.kind, &track.source) {
            (LibraryKind::Managed, TrackSource::Managed { storage_key, .. }) => {
                sanitize_join(&self.uploads_dir, storage_key)
            }
            (
                LibraryKind::LocalMount { root_path },
                TrackSource::LocalPath { relative_path, .. },
            ) => sanitize_join(Path::new(root_path), relative_path),
            _ => Err(DomainError::Validation(
                "mismatched library kind and track source".into(),
            )),
        }
    }
}

#[async_trait]
impl MusicStoragePort for LocalMusicStorage {
    async fn save_managed_audio(
        &self,
        track_id: TrackId,
        format: AudioFormat,
        data: Bytes,
    ) -> DomainResult<StoredManagedAudio> {
        let file_name = format!("{}.{}", track_id.0, format.extension());
        let dest = self.uploads_dir.join(&file_name);
        let file_size = data.len() as u64;
        let data_for_hash = data.clone();
        let file_hash = tokio::task::spawn_blocking(move || sha256_hex(&data_for_hash))
            .await
            .map_err(|e| DomainError::Storage(format!("audio hash task failed: {e}")))?;

        tokio::fs::write(&dest, &data)
            .await
            .map_err(|e| DomainError::Storage(format!("failed saving managed audio: {e}")))?;

        Ok(StoredManagedAudio {
            storage_key: file_name,
            file_hash,
            file_size,
        })
    }

    async fn save_managed_audio_from_path(
        &self,
        track_id: TrackId,
        format: AudioFormat,
        src_path: &Path,
    ) -> DomainResult<StoredManagedAudio> {
        let file_name = format!("{}.{}", track_id.0, format.extension());
        let dest = self.uploads_dir.join(&file_name);
        let _ = move_or_copy_file(src_path, &dest).await?;
        let (file_size, file_hash) = sha256_file_hex(dest).await?;

        Ok(StoredManagedAudio {
            storage_key: file_name,
            file_hash,
            file_size,
        })
    }

    async fn save_managed_sidecar_lrc(
        &self,
        track_id: TrackId,
        data: Bytes,
    ) -> DomainResult<String> {
        let file_name = format!("{}.lrc", track_id.0);
        let dest = self.lyrics_dir.join(&file_name);
        tokio::fs::write(&dest, &data)
            .await
            .map_err(|e| DomainError::Storage(format!("failed saving sidecar lrc: {e}")))?;
        Ok(file_name)
    }

    async fn save_managed_sidecar_lrc_from_path(
        &self,
        track_id: TrackId,
        src_path: &Path,
    ) -> DomainResult<String> {
        let file_name = format!("{}.lrc", track_id.0);
        let dest = self.lyrics_dir.join(&file_name);
        tokio::fs::copy(src_path, &dest)
            .await
            .map_err(|e| DomainError::Storage(format!("failed saving sidecar lrc: {e}")))?;
        Ok(file_name)
    }

    async fn delete_managed_file(&self, storage_key: &str) -> DomainResult<()> {
        for base in [&self.uploads_dir, &self.lyrics_dir] {
            if let Ok(p) = sanitize_join(base, storage_key)
                && p.exists()
            {
                let _ = tokio::fs::remove_file(&p).await;
            }
        }
        Ok(())
    }

    async fn save_cover(&self, _cover_id: &str, cover: &BinaryPayload) -> DomainResult<String> {
        self.thumbnails
            .store_cover_bytes_async(cover.bytes.clone())
            .await
    }

    async fn read_cover(
        &self,
        cover_key: &str,
        max_size: Option<u32>,
    ) -> DomainResult<BinaryPayload> {
        self.thumbnails
            .load_cover_async(cover_key.to_string(), max_size)
            .await
    }

    async fn resolve_audio_fs_path(
        &self,
        library: &Library,
        track: &Track,
    ) -> DomainResult<String> {
        let p = self.resolve_track_path(library, track)?;
        if !p.is_file() {
            return Err(DomainError::NotFound(format!(
                "audio file missing on disk: {}",
                p.display()
            )));
        }
        Ok(p.to_string_lossy().into_owned())
    }

    async fn read_sidecar_lrc_for_track(
        &self,
        library: &Library,
        track: &Track,
    ) -> DomainResult<Option<Bytes>> {
        match (&library.kind, &track.source) {
            (
                LibraryKind::Managed,
                TrackSource::Managed {
                    sidecar_lrc_key: Some(key),
                    ..
                },
            ) => {
                let p = sanitize_join(&self.lyrics_dir, key)?;
                if p.is_file() {
                    let b = tokio::fs::read(&p)
                        .await
                        .map_err(|e| DomainError::Storage(format!("failed reading lrc: {e}")))?;
                    return Ok(Some(Bytes::from(b)));
                }
                Ok(None)
            }
            (
                LibraryKind::LocalMount { root_path },
                TrackSource::LocalPath {
                    relative_path,
                    sidecar_lrc_path,
                    ..
                },
            ) => {
                if let Some(rel_lrc) = sidecar_lrc_path {
                    let p = sanitize_join(Path::new(root_path), rel_lrc)?;
                    if p.is_file()
                        && let Ok(b) = tokio::fs::read(&p).await
                    {
                        return Ok(Some(Bytes::from(b)));
                    }
                }
                let audio_p = sanitize_join(Path::new(root_path), relative_path)?;
                let lrc_p = audio_p.with_extension("lrc");
                if lrc_p.is_file()
                    && let Ok(b) = tokio::fs::read(&lrc_p).await
                {
                    return Ok(Some(Bytes::from(b)));
                }
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    async fn open_audio_stream(
        &self,
        library: &Library,
        track: &Track,
        spec: ByteRangeSpec,
    ) -> DomainResult<ByteRangeStream> {
        let path = self.resolve_track_path(library, track)?;
        open_file_range_stream_spec(&path, spec).await
    }
}

#[derive(Default)]
pub struct LocalMusicScanner;

#[async_trait]
impl MusicScannerPort for LocalMusicScanner {
    async fn scan_audio_entries(&self, root_path: &str) -> DomainResult<Vec<ScannedAudioEntry>> {
        let root = PathBuf::from(root_path);
        tokio::task::spawn_blocking(move || -> DomainResult<Vec<ScannedAudioEntry>> {
            if !root.is_dir() {
                return Err(DomainError::Storage(format!(
                    "music root missing: {}",
                    root.display()
                )));
            }
            let mut dir_cover_lookup: HashMap<PathBuf, Option<String>> = HashMap::new();
            let mut out = Vec::new();

            for entry in WalkDir::new(&root)
                .follow_links(false)
                .into_iter()
                .filter_entry(|e| {
                    e.depth() == 0
                        || !e
                            .file_name()
                            .to_str()
                            .is_some_and(|name| name.starts_with('.'))
                })
            {
                let Ok(entry) = entry else { continue };
                if !entry.file_type().is_file() {
                    continue;
                }
                let path = entry.path();
                let Some(ext) = path.extension().and_then(|s| s.to_str()) else {
                    continue;
                };
                if AudioFormat::from_extension(ext).is_none() {
                    continue;
                }
                let Ok(meta) = entry.metadata() else {
                    continue;
                };
                let Ok(rel) = path.strip_prefix(&root) else {
                    continue;
                };
                let relative_path = rel
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/");

                let fallback_title = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or("Untitled")
                    .to_string();

                let sidecar_lrc = path.with_extension("lrc");
                let sidecar_lrc_relative_path = if sidecar_lrc.is_file() {
                    sidecar_lrc.strip_prefix(&root).ok().map(|r| {
                        r.components()
                            .map(|c| c.as_os_str().to_string_lossy().into_owned())
                            .collect::<Vec<_>>()
                            .join("/")
                    })
                } else {
                    None
                };

                let parent_dir = path.parent().unwrap_or(&root).to_path_buf();
                let directory_cover_path = dir_cover_lookup
                    .entry(parent_dir.clone())
                    .or_insert_with(|| find_directory_cover(&parent_dir))
                    .clone();

                let mut sidecars: Vec<PathBuf> = Vec::new();
                if sidecar_lrc.is_file() {
                    sidecars.push(sidecar_lrc);
                }
                if let Some(ref cov) = directory_cover_path {
                    sidecars.push(PathBuf::from(cov));
                }
                let file_mtime = max_mtime_secs(path, &sidecars);

                out.push(ScannedAudioEntry {
                    absolute_path: path.to_string_lossy().into_owned(),
                    relative_path,
                    ext_hint: ext.to_ascii_lowercase(),
                    file_size: meta.len(),
                    file_mtime,
                    fallback_title,
                    sidecar_lrc_relative_path,
                    directory_cover_path,
                });
            }

            out.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
            Ok(out)
        })
        .await
        .map_err(|e| DomainError::Storage(format!("music scan panicked: {e}")))?
    }

    async fn read_directory_cover(
        &self,
        absolute_cover_path: &str,
    ) -> DomainResult<Option<BinaryPayload>> {
        let p = Path::new(absolute_cover_path);
        if !p.is_file() {
            return Ok(None);
        }
        let bytes = tokio::fs::read(p).await.map_err(|e| {
            DomainError::Storage(format!(
                "failed to read directory cover {absolute_cover_path}: {e}"
            ))
        })?;
        if bytes.is_empty() {
            return Ok(None);
        }
        let mime_type = sniff_image_mime(&bytes).to_string();
        Ok(Some(BinaryPayload { bytes, mime_type }))
    }

    async fn compute_file_fingerprint(&self, absolute_path: &str) -> DomainResult<String> {
        let path = PathBuf::from(absolute_path);
        let (_, hash) = tokio::task::spawn_blocking(move || fast_content_fingerprint(&path))
            .await
            .map_err(|e| DomainError::Storage(format!("fingerprint task failed: {e}")))??;
        Ok(hash)
    }
}

fn find_directory_cover(dir: &Path) -> Option<String> {
    const CANDIDATES: &[&str] = &[
        "cover.jpg",
        "cover.jpeg",
        "cover.png",
        "cover.webp",
        "folder.jpg",
        "folder.png",
        "front.jpg",
        "front.png",
    ];
    for name in CANDIDATES {
        let p = dir.join(name);
        if p.is_file() {
            return Some(p.to_string_lossy().into_owned());
        }
    }
    None
}

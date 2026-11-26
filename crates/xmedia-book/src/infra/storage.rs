use std::collections::HashMap;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use bytes::Bytes;
use walkdir::WalkDir;
use xmedia_core::application::ports::{ByteRangeSpec, ByteRangeStream};
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::{Library, LibraryKind};
use xmedia_core::infra::fingerprint::{fast_bytes_fingerprint, max_mtime_secs};
use xmedia_core::infra::stream::{open_file_range_stream_spec, sanitize_join, sha256_hex};
use xmedia_core::infra::thumbnail::ThumbnailStore;
use xmedia_formats::book::sidecar::{
    dir_has_single_book, find_existing_sidecar_opf, find_sidecar_cover, tracked_book_sidecar_paths,
};
use xmedia_source::proxy::validate_safe_raster_image;

use crate::application::ports::{
    BookScannerPort, BookStoragePort, CoverPayload, LoadedMountedBook, ScannedBookEntry,
    StoredManagedBook,
};
use crate::domain::book::{Book, BookFormat, BookId, BookSource};

#[derive(Clone)]
pub struct LocalBookStorage {
    uploads_dir: PathBuf,
    covers_dir: PathBuf,
    online_tracked_dir: PathBuf,
    thumbnails: ThumbnailStore,
}

impl LocalBookStorage {
    pub fn new(data_dir: impl AsRef<Path>) -> DomainResult<Self> {
        let base = data_dir.as_ref().join("book");
        let uploads_dir = base.join("uploads");
        let covers_dir = base.join("covers");
        let online_tracked_dir = base.join("online_tracked");
        std::fs::create_dir_all(&uploads_dir).map_err(|e| {
            DomainError::Storage(format!(
                "failed to create book uploads directory {}: {e}",
                uploads_dir.display()
            ))
        })?;
        let thumbnails = ThumbnailStore::new(&base)?;
        Ok(Self {
            uploads_dir,
            covers_dir,
            online_tracked_dir,
            thumbnails,
        })
    }

    fn resolve_book_path(&self, library: &Library, book: &Book) -> DomainResult<PathBuf> {
        match (&library.kind, &book.source) {
            (
                _,
                BookSource::ManagedUpload { storage_key }
                | BookSource::OnlineTracked { storage_key, .. },
            ) => sanitize_join(&self.uploads_dir, storage_key),
            (
                LibraryKind::LocalMount { root_path },
                BookSource::MountedFile { relative_path, .. },
            ) => sanitize_join(Path::new(root_path), relative_path),
            _ => Err(DomainError::Validation(
                "mismatched library kind and book source".into(),
            )),
        }
    }

    fn cover_mime_for_key(key: &str) -> &'static str {
        let lower = key.to_ascii_lowercase();
        if lower.ends_with(".png") {
            "image/png"
        } else if lower.ends_with(".webp") {
            "image/webp"
        } else if lower.ends_with(".gif") {
            "image/gif"
        } else if lower.ends_with(".svg") {
            "image/svg+xml"
        } else {
            "image/jpeg"
        }
    }
}

#[async_trait]
impl BookStoragePort for LocalBookStorage {
    async fn save_managed_book(
        &self,
        book_id: BookId,
        format: BookFormat,
        data: Bytes,
    ) -> DomainResult<StoredManagedBook> {
        let file_name = format!("{}.{format}", book_id.0);
        let full_path = self.uploads_dir.join(&file_name);
        let tmp_name = format!(".{}.{}.tmp", file_name, uuid::Uuid::new_v4().simple());
        let tmp_path = self.uploads_dir.join(tmp_name);
        let file_hash = fast_bytes_fingerprint(&data);
        let file_size = data.len() as u64;

        if let Err(e) = tokio::fs::write(&tmp_path, &data).await {
            let _ = tokio::fs::remove_file(&tmp_path).await;
            return Err(DomainError::Storage(format!(
                "failed to write temp managed book: {e}"
            )));
        }
        if let Err(e) = tokio::fs::rename(&tmp_path, &full_path).await {
            let _ = tokio::fs::remove_file(&tmp_path).await;
            return Err(DomainError::Storage(format!(
                "failed to commit managed book: {e}"
            )));
        }

        Ok(StoredManagedBook {
            storage_key: file_name,
            file_hash,
            file_size,
        })
    }

    async fn delete_managed_book(&self, storage_key: &str) -> DomainResult<()> {
        let path = sanitize_join(&self.uploads_dir, storage_key)?;
        if path.exists() {
            tokio::fs::remove_file(&path)
                .await
                .map_err(|e| DomainError::Storage(format!("failed to remove managed book: {e}")))?;
        }
        // If this managed book also had an online tracked chapter workspace, clean it up too.
        if let Some(stem) = Path::new(storage_key).file_stem().and_then(|s| s.to_str())
            && uuid::Uuid::parse_str(stem).is_ok()
        {
            let tracked_dir = self.online_tracked_dir.join(stem);
            if tracked_dir.is_dir() {
                let _ = tokio::fs::remove_dir_all(&tracked_dir).await;
            }
        }
        Ok(())
    }

    async fn save_cover(&self, _book_id: BookId, cover: &CoverPayload) -> DomainResult<String> {
        if cover.trusted_generated_svg && cover.mime_type.eq_ignore_ascii_case("image/svg+xml") {
            return self
                .thumbnails
                .store_trusted_svg_bytes_async(cover.bytes.to_vec())
                .await;
        }
        self.thumbnails
            .store_cover_bytes_async(cover.bytes.to_vec())
            .await
    }

    async fn read_cover(&self, cover_key: &str) -> DomainResult<CoverPayload> {
        match self
            .thumbnails
            .load_cover_async(cover_key.to_string(), None)
            .await
        {
            Ok(payload) => Ok(CoverPayload::new(
                Bytes::from(payload.bytes),
                payload.mime_type,
            )),
            Err(DomainError::NotFound(_)) => {
                // Backwards-compatible fallback for pre-existing per-book covers directly under `book/covers/`
                let path = sanitize_join(&self.covers_dir, cover_key)?;
                if !path.is_file() {
                    return Err(DomainError::NotFound("book cover not found".into()));
                }
                let data = tokio::fs::read(&path)
                    .await
                    .map_err(|e| DomainError::Storage(format!("failed to read book cover: {e}")))?;
                Ok(CoverPayload::new(
                    Bytes::from(data),
                    Self::cover_mime_for_key(cover_key),
                ))
            }
            Err(err) => Err(err),
        }
    }

    async fn delete_cover(&self, cover_key: &str) -> DomainResult<()> {
        // Content-addressed covers in ThumbnailStore may be shared across books; only remove
        // legacy per-book files stored directly under `book/covers/`.
        let legacy_path = sanitize_join(&self.covers_dir, cover_key)?;
        if legacy_path.is_file() {
            tokio::fs::remove_file(&legacy_path)
                .await
                .map_err(|e| DomainError::Storage(format!("failed to remove book cover: {e}")))?;
        }
        Ok(())
    }

    async fn read_book_bytes(&self, library: &Library, book: &Book) -> DomainResult<Bytes> {
        let path = self.resolve_book_path(library, book)?;
        let data = tokio::fs::read(&path).await.map_err(|e| {
            DomainError::Storage(format!("failed to read book {}: {e}", path.display()))
        })?;
        Ok(Bytes::from(data))
    }

    fn resolve_book_fs_path(
        &self,
        library: &Library,
        book: &Book,
    ) -> DomainResult<Option<PathBuf>> {
        self.resolve_book_path(library, book).map(Some)
    }

    async fn open_book_stream(
        &self,
        library: &Library,
        book: &Book,
        spec: ByteRangeSpec,
    ) -> DomainResult<ByteRangeStream> {
        let path = self.resolve_book_path(library, book)?;
        open_file_range_stream_spec(&path, spec).await
    }

    async fn open_path_stream(
        &self,
        path: &Path,
        spec: ByteRangeSpec,
    ) -> DomainResult<ByteRangeStream> {
        open_file_range_stream_spec(path, spec).await
    }
}

#[derive(Default, Clone)]
pub struct LocalBookScanner;

#[async_trait]
impl BookScannerPort for LocalBookScanner {
    async fn scan_entries(&self, root_path: &str) -> DomainResult<Vec<ScannedBookEntry>> {
        let root = PathBuf::from(root_path);
        tokio::task::spawn_blocking(move || {
            if !root.is_dir() {
                return Err(DomainError::Storage(format!(
                    "mounted directory missing: {}",
                    root.display()
                )));
            }

            let mut entries = Vec::new();
            let mut single_book_dirs: HashMap<PathBuf, bool> = HashMap::new();
            let walker = WalkDir::new(&root).follow_links(false).into_iter();
            for item in walker.filter_entry(|e| {
                e.depth() == 0
                    || !e
                        .file_name()
                        .to_str()
                        .is_some_and(|name| name.starts_with('.'))
            }) {
                let Ok(entry) = item else { continue };
                if !entry.file_type().is_file() {
                    continue;
                }
                let path = entry.path();
                let Some(ext) = path.extension().and_then(|s| s.to_str()) else {
                    continue;
                };
                let Ok(format) = ext.parse::<BookFormat>() else {
                    continue;
                };
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

                let parent_dir = path.parent().unwrap_or(&root).to_path_buf();
                let is_single_book_dir = parent_dir != root
                    && *single_book_dirs
                        .entry(parent_dir.clone())
                        .or_insert_with(|| dir_has_single_book(&parent_dir));

                let tracked_sidecars = tracked_book_sidecar_paths(path, is_single_book_dir);
                let file_mtime = max_mtime_secs(path, &tracked_sidecars);

                let fallback_title = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or("Untitled")
                    .to_string();

                entries.push(ScannedBookEntry {
                    relative_path,
                    format,
                    file_size: meta.len(),
                    file_mtime,
                    fallback_title,
                });
            }

            entries.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
            Ok(entries)
        })
        .await
        .map_err(|e| DomainError::Storage(format!("scan task panicked: {e}")))?
    }

    async fn read_entry(
        &self,
        root_path: &str,
        relative_path: &str,
    ) -> DomainResult<LoadedMountedBook> {
        let full = sanitize_join(Path::new(root_path), relative_path)?;
        let root_buf = PathBuf::from(root_path);
        tokio::task::spawn_blocking(move || -> DomainResult<LoadedMountedBook> {
            let data = std::fs::read(&full).map_err(|e| {
                DomainError::Storage(format!("failed to read file {}: {e}", full.display()))
            })?;
            let file_hash = fast_bytes_fingerprint(&data);
            let legacy_sha256 = sha256_hex(&data);

            let sidecar_opf = find_existing_sidecar_opf(&full, Some(&root_buf))
                .and_then(|opf_path| std::fs::read(opf_path).ok());

            let sidecar_cover = find_sidecar_cover(&full, Some(&root_buf)).and_then(|cp| {
                let img_bytes = std::fs::read(cp).ok()?;
                let safe_mime = validate_safe_raster_image(None, &img_bytes).ok()?;
                Some(CoverPayload::new(Bytes::from(img_bytes), safe_mime))
            });

            Ok(LoadedMountedBook {
                bytes: Bytes::from(data),
                file_hash,
                legacy_sha256,
                sidecar_opf,
                sidecar_cover,
            })
        })
        .await
        .map_err(|e| DomainError::Storage(format!("read_entry task panicked: {e}")))?
    }
}

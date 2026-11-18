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
use xmedia_core::infra::thumbnail::sniff_image_mime;

use crate::application::ports::{
    BookScannerPort, BookStoragePort, CoverPayload, LoadedMountedBook, ScannedBookEntry,
    StoredManagedBook,
};
use crate::domain::book::{Book, BookFormat, BookId, BookSource};

#[derive(Clone)]
pub struct LocalBookStorage {
    uploads_dir: PathBuf,
    covers_dir: PathBuf,
}

impl LocalBookStorage {
    pub fn new(data_dir: impl AsRef<Path>) -> DomainResult<Self> {
        let base = data_dir.as_ref().join("book");
        let uploads_dir = base.join("uploads");
        let covers_dir = base.join("covers");
        std::fs::create_dir_all(&uploads_dir).map_err(|e| {
            DomainError::Storage(format!(
                "failed to create book uploads directory {}: {e}",
                uploads_dir.display()
            ))
        })?;
        std::fs::create_dir_all(&covers_dir).map_err(|e| {
            DomainError::Storage(format!(
                "failed to create book covers directory {}: {e}",
                covers_dir.display()
            ))
        })?;
        Ok(Self {
            uploads_dir,
            covers_dir,
        })
    }

    fn resolve_book_path(&self, library: &Library, book: &Book) -> DomainResult<PathBuf> {
        match (&library.kind, &book.source) {
            (_, BookSource::ManagedUpload { storage_key }) => {
                sanitize_join(&self.uploads_dir, storage_key)
            }
            (
                LibraryKind::LocalMount { root_path },
                BookSource::MountedFile { relative_path, .. },
            ) => sanitize_join(Path::new(root_path), relative_path),
            _ => Err(DomainError::Validation(
                "mismatched library kind and book source".into(),
            )),
        }
    }

    fn cover_extension_for_mime(mime: &str) -> &'static str {
        let lower = mime.to_ascii_lowercase();
        if lower.contains("png") {
            "png"
        } else if lower.contains("webp") {
            "webp"
        } else if lower.contains("gif") {
            "gif"
        } else if lower.contains("svg") {
            "svg"
        } else {
            "jpg"
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
        let file_name = format!("{}.{}", book_id.0, format.as_str());
        let full_path = self.uploads_dir.join(&file_name);
        let file_hash = fast_bytes_fingerprint(&data);
        let file_size = data.len() as u64;

        tokio::fs::write(&full_path, &data)
            .await
            .map_err(|e| DomainError::Storage(format!("failed to write managed book: {e}")))?;

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
        Ok(())
    }

    async fn save_cover(&self, book_id: BookId, cover: &CoverPayload) -> DomainResult<String> {
        let ext = Self::cover_extension_for_mime(&cover.mime_type);
        let key = format!("{}.{ext}", book_id.0);
        let path = self.covers_dir.join(&key);
        tokio::fs::write(&path, &cover.bytes)
            .await
            .map_err(|e| DomainError::Storage(format!("failed to write book cover: {e}")))?;
        Ok(key)
    }

    async fn read_cover(&self, cover_key: &str) -> DomainResult<CoverPayload> {
        let path = sanitize_join(&self.covers_dir, cover_key)?;
        if !path.exists() {
            return Err(DomainError::NotFound("book cover not found".into()));
        }
        let data = tokio::fs::read(&path)
            .await
            .map_err(|e| DomainError::Storage(format!("failed to read book cover: {e}")))?;
        Ok(CoverPayload {
            bytes: Bytes::from(data),
            mime_type: Self::cover_mime_for_key(cover_key).to_string(),
        })
    }

    async fn delete_cover(&self, cover_key: &str) -> DomainResult<()> {
        let path = sanitize_join(&self.covers_dir, cover_key)?;
        if path.exists() {
            tokio::fs::remove_file(&path)
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

    async fn open_book_stream(
        &self,
        library: &Library,
        book: &Book,
        spec: ByteRangeSpec,
    ) -> DomainResult<ByteRangeStream> {
        let path = self.resolve_book_path(library, book)?;
        open_file_range_stream_spec(&path, spec).await
    }
}

fn dir_has_single_book(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    let mut count = 0usize;
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_file()
            && p.extension()
                .and_then(|s| s.to_str())
                .is_some_and(|e| BookFormat::from_extension(e).is_some())
        {
            count += 1;
            if count > 1 {
                return false;
            }
        }
    }
    count == 1
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
                let Some(format) = BookFormat::from_extension(ext) else {
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

                let mut tracked_sidecars = vec![
                    path.with_extension("opf"),
                    path.with_extension("jpg"),
                    path.with_extension("jpeg"),
                    path.with_extension("png"),
                    path.with_extension("webp"),
                ];
                if is_single_book_dir {
                    tracked_sidecars.extend([
                        parent_dir.join("metadata.opf"),
                        parent_dir.join("cover.jpg"),
                        parent_dir.join("cover.jpeg"),
                        parent_dir.join("cover.png"),
                        parent_dir.join("cover.webp"),
                    ]);
                }
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
        let data = tokio::fs::read(&full).await.map_err(|e| {
            DomainError::Storage(format!("failed to read file {}: {e}", full.display()))
        })?;
        let file_hash = fast_bytes_fingerprint(&data);
        let legacy_sha256 = sha256_hex(&data);

        let root_buf = PathBuf::from(root_path);
        let parent_dir = full.parent().map(Path::to_path_buf);
        let single_book_dir = parent_dir
            .as_deref()
            .is_some_and(|p| p != root_buf && dir_has_single_book(p));

        let opf_path = full.with_extension("opf");
        let sidecar_opf = if opf_path.is_file() {
            tokio::fs::read(&opf_path).await.ok()
        } else if single_book_dir
            && let Some(ref p) = parent_dir
            && p.join("metadata.opf").is_file()
        {
            tokio::fs::read(p.join("metadata.opf")).await.ok()
        } else {
            None
        };

        let mut sidecar_cover = None;
        for ext in ["jpg", "jpeg", "png", "webp"] {
            let cp = full.with_extension(ext);
            if cp.is_file()
                && let Ok(img_bytes) = tokio::fs::read(&cp).await
                && !img_bytes.is_empty()
            {
                let mime_type = sniff_image_mime(&img_bytes).to_string();
                sidecar_cover = Some(CoverPayload {
                    bytes: Bytes::from(img_bytes),
                    mime_type,
                });
                break;
            }
        }
        if sidecar_cover.is_none()
            && single_book_dir
            && let Some(ref p) = parent_dir
        {
            for ext in ["jpg", "jpeg", "png", "webp"] {
                let cp = p.join(format!("cover.{ext}"));
                if cp.is_file()
                    && let Ok(img_bytes) = tokio::fs::read(&cp).await
                    && !img_bytes.is_empty()
                {
                    let mime_type = sniff_image_mime(&img_bytes).to_string();
                    sidecar_cover = Some(CoverPayload {
                        bytes: Bytes::from(img_bytes),
                        mime_type,
                    });
                    break;
                }
            }
        }

        Ok(LoadedMountedBook {
            bytes: Bytes::from(data),
            file_hash,
            legacy_sha256,
            sidecar_opf,
            sidecar_cover,
        })
    }
}

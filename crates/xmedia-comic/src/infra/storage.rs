use std::path::{Path, PathBuf};

use async_trait::async_trait;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::infra::stream::move_or_copy_file;

use crate::application::ports::{ComicFileStoragePort, StoredUploadFile};
use crate::infra::archive::fast_file_fingerprint;

pub struct LocalComicFileStorage {
    uploads_root: PathBuf,
}

impl LocalComicFileStorage {
    pub fn new(data_dir: impl AsRef<Path>) -> DomainResult<Self> {
        let uploads_root = data_dir.as_ref().join("comic").join("uploads");
        std::fs::create_dir_all(&uploads_root).map_err(|e| {
            DomainError::Storage(format!(
                "failed to create comic uploads directory '{}': {e}",
                uploads_root.display()
            ))
        })?;
        Ok(Self { uploads_root })
    }
}

#[async_trait]
impl ComicFileStoragePort for LocalComicFileStorage {
    async fn ensure_managed_root(&self) -> DomainResult<String> {
        tokio::fs::create_dir_all(&self.uploads_root)
            .await
            .map_err(|e| {
                DomainError::Storage(format!(
                    "failed to ensure comic uploads root '{}': {e}",
                    self.uploads_root.display()
                ))
            })?;
        Ok(self.uploads_root.to_string_lossy().into_owned())
    }

    async fn store_uploaded_archive(
        &self,
        series_folder: &str,
        filename: &str,
        bytes: &[u8],
    ) -> DomainResult<StoredUploadFile> {
        let safe_series = sanitize_component(series_folder);
        let safe_file = sanitize_component(filename);
        let series_dir = self.uploads_root.join(safe_series);
        tokio::fs::create_dir_all(&series_dir).await.map_err(|e| {
            DomainError::Storage(format!(
                "failed to create series upload dir '{}': {e}",
                series_dir.display()
            ))
        })?;

        let dest = unique_destination(&series_dir, &safe_file);
        tokio::fs::write(&dest, bytes).await.map_err(|e| {
            DomainError::Storage(format!(
                "failed to write uploaded comic '{}': {e}",
                dest.display()
            ))
        })?;

        let dest_clone = dest.clone();
        let (file_size, file_hash) =
            tokio::task::spawn_blocking(move || fast_file_fingerprint(&dest_clone))
                .await
                .map_err(|e| DomainError::Storage(format!("fingerprint join failed: {e}")))??;

        Ok(StoredUploadFile {
            source_dir: series_dir.to_string_lossy().into_owned(),
            source_path: dest.to_string_lossy().into_owned(),
            file_size,
            file_hash,
        })
    }

    async fn store_uploaded_archive_from_path(
        &self,
        series_folder: &str,
        filename: &str,
        src_path: &Path,
    ) -> DomainResult<StoredUploadFile> {
        let safe_series = sanitize_component(series_folder);
        let safe_file = sanitize_component(filename);
        let series_dir = self.uploads_root.join(safe_series);
        tokio::fs::create_dir_all(&series_dir).await.map_err(|e| {
            DomainError::Storage(format!(
                "failed to create series upload dir '{}': {e}",
                series_dir.display()
            ))
        })?;

        let dest = unique_destination(&series_dir, &safe_file);
        let _ = move_or_copy_file(src_path, &dest).await?;

        let dest_clone = dest.clone();
        let (file_size, file_hash) =
            tokio::task::spawn_blocking(move || fast_file_fingerprint(&dest_clone))
                .await
                .map_err(|e| DomainError::Storage(format!("fingerprint join failed: {e}")))??;

        Ok(StoredUploadFile {
            source_dir: series_dir.to_string_lossy().into_owned(),
            source_path: dest.to_string_lossy().into_owned(),
            file_size,
            file_hash,
        })
    }

    async fn delete_managed_file(&self, path: &str) -> DomainResult<()> {
        let target = PathBuf::from(path);
        if target.starts_with(&self.uploads_root) && target.exists() {
            let _ = tokio::fs::remove_file(&target).await;
        }
        Ok(())
    }
}

fn sanitize_component(name: &str) -> String {
    let out: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            other => other,
        })
        .collect();
    let trimmed = out.trim().trim_matches('.');
    if trimmed.is_empty() {
        "comic_upload".into()
    } else {
        trimmed.to_owned()
    }
}

fn unique_destination(dir: &Path, filename: &str) -> PathBuf {
    let candidate = dir.join(filename);
    if !candidate.exists() {
        return candidate;
    }
    let stem = Path::new(filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("chapter");
    let ext = Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{e}"))
        .unwrap_or_default();

    for idx in 1..10000 {
        let next = dir.join(format!("{stem}_{idx}{ext}"));
        if !next.exists() {
            return next;
        }
    }
    dir.join(format!("{stem}_{}{ext}", uuid::Uuid::now_v7()))
}

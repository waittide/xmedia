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

        let dir_for_reserve = series_dir.clone();
        let file_for_reserve = safe_file.clone();
        let dest = tokio::task::spawn_blocking(move || {
            reserve_unique_destination(&dir_for_reserve, &file_for_reserve)
        })
        .await
        .map_err(|e| DomainError::Storage(format!("reserve upload path join failed: {e}")))??;
        if let Err(e) = tokio::fs::write(&dest, bytes).await {
            let _ = tokio::fs::remove_file(&dest).await;
            return Err(DomainError::Storage(format!(
                "failed to write uploaded comic '{}': {e}",
                dest.display()
            )));
        }

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

        let dir_for_reserve = series_dir.clone();
        let file_for_reserve = safe_file.clone();
        let dest = tokio::task::spawn_blocking(move || {
            reserve_unique_destination(&dir_for_reserve, &file_for_reserve)
        })
        .await
        .map_err(|e| DomainError::Storage(format!("reserve upload path join failed: {e}")))??;
        if let Err(err) = move_or_copy_file(src_path, &dest).await {
            let _ = tokio::fs::remove_file(&dest).await;
            return Err(err);
        }

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
        let root = self.uploads_root.clone();
        let target = PathBuf::from(path);
        tokio::task::spawn_blocking(move || {
            let Ok(canon_root) = root.canonicalize() else {
                return;
            };
            let Ok(canon_target) = target.canonicalize() else {
                return;
            };
            if canon_target.starts_with(&canon_root) && canon_target.is_file() {
                let _ = std::fs::remove_file(&canon_target);
            }
        })
        .await
        .map_err(|e| DomainError::Storage(format!("delete managed comic file failed: {e}")))?;
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

fn reserve_unique_destination(dir: &Path, filename: &str) -> DomainResult<PathBuf> {
    let try_create = |p: &Path| -> std::io::Result<bool> {
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(p)
        {
            Ok(_) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
            Err(e) => Err(e),
        }
    };

    let candidate = dir.join(filename);
    if try_create(&candidate).map_err(|e| {
        DomainError::Storage(format!(
            "failed to reserve destination '{}': {e}",
            candidate.display()
        ))
    })? {
        return Ok(candidate);
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
        if try_create(&next).map_err(|e| {
            DomainError::Storage(format!(
                "failed to reserve destination '{}': {e}",
                next.display()
            ))
        })? {
            return Ok(next);
        }
    }
    let fallback = dir.join(format!("{stem}_{}{ext}", uuid::Uuid::now_v7()));
    let _ = try_create(&fallback).map_err(|e| {
        DomainError::Storage(format!(
            "failed to reserve destination '{}': {e}",
            fallback.display()
        ))
    })?;
    Ok(fallback)
}

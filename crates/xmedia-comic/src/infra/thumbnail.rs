use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use image::ImageFormat;
use image::imageops::FilterType;
use xmedia_core::domain::error::{DomainError, DomainResult};

use crate::application::ports::{PageContent, ThumbnailGeneratorPort};

pub struct WebpThumbnailGenerator {
    cache_dir: PathBuf,
}

impl WebpThumbnailGenerator {
    pub fn new(data_dir: impl AsRef<Path>) -> DomainResult<Self> {
        let dir = data_dir.as_ref().join("comic").join("thumbnails");
        fs::create_dir_all(&dir).map_err(|e| {
            DomainError::Storage(format!(
                "failed to create comic thumbnail directory '{}': {e}",
                dir.display()
            ))
        })?;
        Ok(Self { cache_dir: dir })
    }
}

#[async_trait]
impl ThumbnailGeneratorPort for WebpThumbnailGenerator {
    async fn generate_from_bytes(&self, key: &str, image_bytes: &[u8]) -> DomainResult<String> {
        let safe_key = sanitize_key(key);
        let filename = format!("{safe_key}.webp");
        let target_path = self.cache_dir.join(&filename);
        let owned = image_bytes.to_vec();

        tokio::task::spawn_blocking(move || {
            let img = image::load_from_memory(&owned).map_err(|e| {
                DomainError::Validation(format!("failed to decode cover image: {e}"))
            })?;
            let resized = img.resize(450, 640, FilterType::CatmullRom);
            let mut out = Cursor::new(Vec::new());
            resized
                .write_to(&mut out, ImageFormat::WebP)
                .map_err(|e| DomainError::Storage(format!("failed to encode WebP cover: {e}")))?;
            fs::write(&target_path, out.into_inner()).map_err(|e| {
                DomainError::Storage(format!(
                    "failed to write thumbnail '{}': {e}",
                    target_path.display()
                ))
            })?;
            Ok(filename)
        })
        .await
        .map_err(|e| DomainError::Storage(format!("thumbnail join error: {e}")))?
    }

    async fn read_thumbnail(&self, cover_key: &str) -> DomainResult<PageContent> {
        if cover_key.contains("..") || cover_key.contains('/') || cover_key.contains('\\') {
            return Err(DomainError::Validation("invalid cover key".into()));
        }
        let path = self.cache_dir.join(cover_key);
        tokio::task::spawn_blocking(move || {
            let bytes = fs::read(&path)
                .map_err(|_| DomainError::NotFound("cover thumbnail not found".into()))?;
            let hash = blake3::hash(&bytes).to_hex();
            let etag = format!("\"cov-{}\"", &hash.as_str()[..20]);
            let mime_type = if path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("webp"))
            {
                "image/webp".into()
            } else {
                "image/jpeg".into()
            };
            Ok(PageContent {
                bytes,
                mime_type,
                etag,
            })
        })
        .await
        .map_err(|e| DomainError::Storage(format!("read_thumbnail join error: {e}")))?
    }
}

fn sanitize_key(raw: &str) -> String {
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

use std::fs;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::infra::thumbnail::{ThumbnailStore, sniff_image_mime};

use crate::application::ports::{PageContent, ThumbnailGeneratorPort};

/// Default thumbnail bucket requested for comic covers (maps to the 480px bucket in `ThumbnailStore`).
pub const DEFAULT_COMIC_THUMB_SIZE: u32 = 480;

pub struct WebpThumbnailGenerator {
    store: ThumbnailStore,
    legacy_dir: PathBuf,
}

impl WebpThumbnailGenerator {
    pub fn new(data_dir: impl AsRef<Path>) -> DomainResult<Self> {
        let comic_root = data_dir.as_ref().join("comic");
        let store = ThumbnailStore::new(&comic_root)?;
        let legacy_dir = comic_root.join("thumbnails");
        Ok(Self { store, legacy_dir })
    }

    fn build_page_content(bytes: Vec<u8>, mime_type: String) -> PageContent {
        let hash = blake3::hash(&bytes).to_hex();
        let etag = format!("\"cov-{}\"", &hash.as_str()[..20]);
        PageContent {
            bytes,
            mime_type,
            etag,
        }
    }
}

#[async_trait]
impl ThumbnailGeneratorPort for WebpThumbnailGenerator {
    async fn generate_from_bytes(&self, _key: &str, image_bytes: &[u8]) -> DomainResult<String> {
        if image_bytes.is_empty() {
            return Err(DomainError::Validation(
                "cover image cannot be empty".into(),
            ));
        }
        self.store
            .store_cover_bytes_async(image_bytes.to_vec())
            .await
    }

    async fn read_thumbnail(&self, cover_key: &str) -> DomainResult<PageContent> {
        self.read_thumbnail_sized(cover_key, Some(DEFAULT_COMIC_THUMB_SIZE))
            .await
    }

    async fn read_thumbnail_sized(
        &self,
        cover_key: &str,
        max_size: Option<u32>,
    ) -> DomainResult<PageContent> {
        match self
            .store
            .load_cover_async(cover_key.to_owned(), max_size)
            .await
        {
            Ok(payload) => Ok(Self::build_page_content(payload.bytes, payload.mime_type)),
            Err(DomainError::NotFound(_)) => {
                // Backward-compatible fallback for pre-migration legacy `.webp` thumbnails.
                let legacy_path = self.legacy_dir.join(cover_key);
                tokio::task::spawn_blocking(move || {
                    let bytes = fs::read(&legacy_path)
                        .map_err(|_| DomainError::NotFound("cover thumbnail not found".into()))?;
                    let mime_type = sniff_image_mime(&bytes).to_owned();
                    Ok(Self::build_page_content(bytes, mime_type))
                })
                .await
                .map_err(|e| DomainError::Storage(format!("read_thumbnail join error: {e}")))?
            }
            Err(err) => Err(err),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use image::{ImageBuffer, ImageFormat, Rgba};
    use tempfile::TempDir;

    use super::*;

    fn make_png(width: u32, height: u32, alpha: u8) -> Vec<u8> {
        let img: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::from_fn(width, height, |x, y| {
            Rgba([
                ((x * 7) % 255) as u8,
                ((y * 11) % 255) as u8,
                ((x + y) % 255) as u8,
                alpha,
            ])
        });
        let mut cursor = Cursor::new(Vec::new());
        img.write_to(&mut cursor, ImageFormat::Png).unwrap();
        cursor.into_inner()
    }

    #[tokio::test]
    async fn thumbnail_store_deduplicates_and_serves_bucketed_jpeg() {
        let tmp = TempDir::new().unwrap();
        let generator = WebpThumbnailGenerator::new(tmp.path()).unwrap();
        let png = make_png(700, 900, 255);

        // Two distinct key hints with identical bytes must deduplicate to the same content key.
        let key1 = generator.generate_from_bytes("ch_one", &png).await.unwrap();
        let key2 = generator.generate_from_bytes("ch_two", &png).await.unwrap();
        assert_eq!(key1, key2);
        assert!(key1.ends_with(".png"));

        // Default read_thumbnail uses bucket 480 and encodes opaque image as JPEG 82.
        let thumb_480 = generator.read_thumbnail(&key1).await.unwrap();
        assert_eq!(thumb_480.mime_type, "image/jpeg");
        assert!(thumb_480.bytes.starts_with(&[0xFF, 0xD8, 0xFF]));
        assert!(thumb_480.etag.starts_with("\"cov-"));

        // Requesting 200px maps to bucket 240 and caches separately from 480.
        let thumb_240 = generator
            .read_thumbnail_sized(&key1, Some(200))
            .await
            .unwrap();
        assert_eq!(thumb_240.mime_type, "image/jpeg");
        assert!(thumb_240.bytes.starts_with(&[0xFF, 0xD8, 0xFF]));
        assert_ne!(thumb_240.bytes, thumb_480.bytes);

        let thumbs_dir = tmp.path().join("comic").join("covers").join("thumbs");
        let stem = key1.split('.').next().unwrap();
        assert!(thumbs_dir.join(format!("{stem}_480.jpg")).is_file());
        assert!(thumbs_dir.join(format!("{stem}_240.jpg")).is_file());
    }

    #[tokio::test]
    async fn thumbnail_store_preserves_alpha_and_rejects_traversal_and_reads_legacy() {
        let tmp = TempDir::new().unwrap();
        let generator = WebpThumbnailGenerator::new(tmp.path()).unwrap();

        let alpha_png = make_png(300, 300, 128);
        let key = generator
            .generate_from_bytes("alpha_cover", &alpha_png)
            .await
            .unwrap();
        let alpha_thumb = generator.read_thumbnail(&key).await.unwrap();
        assert_eq!(alpha_thumb.mime_type, "image/png");
        assert!(alpha_thumb.bytes.starts_with(&[0x89, b'P', b'N', b'G']));

        // Reject path traversal
        assert!(matches!(
            generator.read_thumbnail("../evil.png").await,
            Err(DomainError::Validation(_))
        ));

        // Reject empty image input
        assert!(matches!(
            generator.generate_from_bytes("empty", &[]).await,
            Err(DomainError::Validation(_))
        ));

        // Fallback to legacy comic/thumbnails directory when present
        let legacy_dir = tmp.path().join("comic").join("thumbnails");
        fs::create_dir_all(&legacy_dir).unwrap();
        let legacy_bytes = make_png(64, 64, 255);
        fs::write(legacy_dir.join("legacy_key.png"), &legacy_bytes).unwrap();
        let legacy_read = generator.read_thumbnail("legacy_key.png").await.unwrap();
        assert_eq!(legacy_read.bytes, legacy_bytes);
        assert_eq!(legacy_read.mime_type, "image/png");
    }
}

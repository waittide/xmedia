use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use image::ImageFormat;
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;

use crate::application::ports::BinaryPayload;
use crate::domain::error::{DomainError, DomainResult};
use crate::infra::stream::sha256_hex;

pub const THUMB_SIZE_BUCKETS: [u32; 3] = [240, 480, 960];
const THUMB_JPEG_QUALITY: u8 = 82;

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Normalizes any requested thumbnail max dimension into fixed discrete buckets `[240, 480, 960]`
/// to prevent arbitrary `?size=` parameters from flooding the disk cache.
pub fn normalize_thumb_size(max_size: u32) -> u32 {
    if max_size <= THUMB_SIZE_BUCKETS[0] {
        THUMB_SIZE_BUCKETS[0]
    } else if max_size <= THUMB_SIZE_BUCKETS[1] {
        THUMB_SIZE_BUCKETS[1]
    } else {
        THUMB_SIZE_BUCKETS[2]
    }
}

fn atomic_write_file(dest: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = dest.parent().unwrap_or_else(|| Path::new("."));
    let file_name = dest.file_name().and_then(|s| s.to_str()).unwrap_or("thumb");
    let seq = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = parent.join(format!(".{file_name}.{}.{seq}.tmp", std::process::id()));
    std::fs::write(&tmp, bytes)?;
    if let Err(err) = std::fs::rename(&tmp, dest) {
        let _ = std::fs::remove_file(&tmp);
        return Err(err);
    }
    Ok(())
}

fn image_has_transparency(img: &image::DynamicImage) -> bool {
    if !img.color().has_alpha() {
        return false;
    }
    img.to_rgba8().pixels().any(|p| p.0[3] < 250)
}

pub fn sniff_image_mime(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "image/png"
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "image/jpeg"
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else if bytes.starts_with(b"GIF8") {
        "image/gif"
    } else if bytes.starts_with(b"<svg") || bytes.starts_with(b"<?xml") {
        "image/svg+xml"
    } else {
        "image/jpeg"
    }
}

#[derive(Clone)]
pub struct ThumbnailStore {
    orig_dir: PathBuf,
    thumb_dir: PathBuf,
}

impl ThumbnailStore {
    pub fn new(data_dir: impl AsRef<Path>) -> DomainResult<Self> {
        let root = data_dir.as_ref();
        let orig_dir = root.join("covers").join("orig");
        let thumb_dir = root.join("covers").join("thumbs");
        for dir in [&orig_dir, &thumb_dir] {
            std::fs::create_dir_all(dir).map_err(|e| {
                DomainError::Storage(format!(
                    "failed to create cover cache dir {}: {e}",
                    dir.display()
                ))
            })?;
        }
        Ok(Self {
            orig_dir,
            thumb_dir,
        })
    }

    pub fn store_cover_bytes(&self, bytes: &[u8]) -> DomainResult<String> {
        let hash = sha256_hex(bytes);
        let mime = sniff_image_mime(bytes);
        let ext = match mime {
            "image/png" => "png",
            "image/webp" => "webp",
            "image/gif" => "gif",
            "image/svg+xml" => "svg",
            _ => "jpg",
        };
        let key = format!("{hash}.{ext}");
        let dest = self.orig_dir.join(&key);
        if !dest.exists() {
            atomic_write_file(&dest, bytes).map_err(|e| {
                DomainError::Storage(format!("failed to write cover {}: {e}", dest.display()))
            })?;
        }
        Ok(key)
    }

    pub async fn store_cover_bytes_async(&self, bytes: Vec<u8>) -> DomainResult<String> {
        let store = self.clone();
        tokio::task::spawn_blocking(move || store.store_cover_bytes(&bytes))
            .await
            .map_err(|e| DomainError::Storage(format!("store_cover_bytes task failed: {e}")))?
    }

    pub async fn load_cover_async(
        &self,
        cover_key: String,
        max_size: Option<u32>,
    ) -> DomainResult<BinaryPayload> {
        let store = self.clone();
        tokio::task::spawn_blocking(move || store.load_cover(&cover_key, max_size))
            .await
            .map_err(|e| DomainError::Storage(format!("load_cover task failed: {e}")))?
    }

    pub fn load_cover(
        &self,
        cover_key: &str,
        max_size: Option<u32>,
    ) -> DomainResult<BinaryPayload> {
        if cover_key.contains('/') || cover_key.contains('\\') || cover_key.contains("..") {
            return Err(DomainError::Validation("invalid cover key".into()));
        }
        let orig_path = self.orig_dir.join(cover_key);
        if !orig_path.is_file() {
            return Err(DomainError::NotFound("cover art not found".into()));
        }

        let is_svg_key = orig_path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("svg"));

        let Some(target) = max_size
            .filter(|&s| (64..=1600).contains(&s))
            .filter(|_| !is_svg_key)
        else {
            let orig_bytes = std::fs::read(&orig_path)
                .map_err(|e| DomainError::Storage(format!("failed to read cover: {e}")))?;
            let orig_mime = sniff_image_mime(&orig_bytes);
            return Ok(BinaryPayload {
                bytes: orig_bytes,
                mime_type: orig_mime.to_string(),
            });
        };

        let bucket = normalize_thumb_size(target);
        let stem = cover_key.split('.').next().unwrap_or(cover_key);
        let jpg_path = self.thumb_dir.join(format!("{stem}_{bucket}.jpg"));
        if let Ok(cached) = std::fs::read(&jpg_path) {
            return Ok(BinaryPayload {
                bytes: cached,
                mime_type: "image/jpeg".to_string(),
            });
        }
        let png_path = self.thumb_dir.join(format!("{stem}_{bucket}.png"));
        if let Ok(cached) = std::fs::read(&png_path) {
            return Ok(BinaryPayload {
                bytes: cached,
                mime_type: "image/png".to_string(),
            });
        }

        let orig_bytes = std::fs::read(&orig_path)
            .map_err(|e| DomainError::Storage(format!("failed to read cover: {e}")))?;
        let orig_mime = sniff_image_mime(&orig_bytes);

        if orig_mime == "image/svg+xml" {
            return Ok(BinaryPayload {
                bytes: orig_bytes,
                mime_type: orig_mime.to_string(),
            });
        }

        match image::load_from_memory(&orig_bytes) {
            Ok(img) => {
                let resized = img.resize(bucket, bucket, FilterType::Lanczos3);
                if image_has_transparency(&resized) {
                    let mut out = Cursor::new(Vec::new());
                    if resized.write_to(&mut out, ImageFormat::Png).is_ok() {
                        let png_bytes = out.into_inner();
                        let _ = atomic_write_file(&png_path, &png_bytes);
                        return Ok(BinaryPayload {
                            bytes: png_bytes,
                            mime_type: "image/png".to_string(),
                        });
                    }
                } else {
                    let mut out = Vec::new();
                    let mut encoder = JpegEncoder::new_with_quality(&mut out, THUMB_JPEG_QUALITY);
                    if encoder.encode_image(&resized.to_rgb8()).is_ok() {
                        let _ = atomic_write_file(&jpg_path, &out);
                        return Ok(BinaryPayload {
                            bytes: out,
                            mime_type: "image/jpeg".to_string(),
                        });
                    }
                }
                Ok(BinaryPayload {
                    bytes: orig_bytes,
                    mime_type: orig_mime.to_string(),
                })
            }
            Err(_) => Ok(BinaryPayload {
                bytes: orig_bytes,
                mime_type: orig_mime.to_string(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgba};
    use tempfile::TempDir;

    fn make_png_bytes(width: u32, height: u32, alpha: u8) -> Vec<u8> {
        let img: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::from_fn(width, height, |x, y| {
            Rgba([
                ((x * 3) % 255) as u8,
                ((y * 5) % 255) as u8,
                ((x + y) % 255) as u8,
                alpha,
            ])
        });
        let mut cursor = Cursor::new(Vec::new());
        img.write_to(&mut cursor, ImageFormat::Png).unwrap();
        cursor.into_inner()
    }

    #[test]
    fn normalize_thumb_size_buckets_correctly() {
        assert_eq!(normalize_thumb_size(64), 240);
        assert_eq!(normalize_thumb_size(240), 240);
        assert_eq!(normalize_thumb_size(241), 480);
        assert_eq!(normalize_thumb_size(320), 480);
        assert_eq!(normalize_thumb_size(480), 480);
        assert_eq!(normalize_thumb_size(481), 960);
        assert_eq!(normalize_thumb_size(1200), 960);
    }

    #[tokio::test]
    async fn thumbnail_store_uses_jpeg_buckets_and_hits_cache_without_reading_original() {
        let tmp = TempDir::new().unwrap();
        let store = ThumbnailStore::new(tmp.path()).unwrap();

        let opaque_png = make_png_bytes(600, 600, 255);
        let key = store.store_cover_bytes_async(opaque_png).await.unwrap();

        // Requesting 300 maps to bucket 480 and encodes JPEG.
        let thumb = store
            .load_cover_async(key.clone(), Some(300))
            .await
            .unwrap();
        assert_eq!(thumb.mime_type, "image/jpeg");
        assert!(thumb.bytes.starts_with(&[0xFF, 0xD8, 0xFF]));

        // Overwrite original file with invalid sentinel bytes while keeping file present.
        // Cache hit for another size in the same bucket (400 -> 480) must NOT read original.
        let orig_path = tmp.path().join("covers").join("orig").join(&key);
        std::fs::write(&orig_path, b"corrupted-sentinel").unwrap();

        let cached = store
            .load_cover_async(key.clone(), Some(400))
            .await
            .unwrap();
        assert_eq!(cached.mime_type, "image/jpeg");
        assert_eq!(cached.bytes, thumb.bytes);
    }

    #[test]
    fn thumbnail_store_preserves_png_when_image_has_transparency() {
        let tmp = TempDir::new().unwrap();
        let store = ThumbnailStore::new(tmp.path()).unwrap();

        let transparent_png = make_png_bytes(400, 400, 128);
        let key = store.store_cover_bytes(&transparent_png).unwrap();

        let thumb = store.load_cover(&key, Some(200)).unwrap();
        assert_eq!(thumb.mime_type, "image/png");
        assert!(thumb.bytes.starts_with(&[0x89, b'P', b'N', b'G']));
    }
}

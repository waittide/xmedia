#[cfg(feature = "server")]
use std::io::Cursor;
#[cfg(feature = "server")]
use std::path::{Path, PathBuf};
#[cfg(feature = "server")]
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(feature = "server")]
use image::codecs::jpeg::JpegEncoder;
#[cfg(feature = "server")]
use image::imageops::FilterType;
#[cfg(feature = "server")]
use image::{ImageFormat, ImageReader, Limits};

#[cfg(feature = "server")]
use crate::application::ports::BinaryPayload;
#[cfg(feature = "server")]
use crate::domain::error::{DomainError, DomainResult};
#[cfg(feature = "server")]
use crate::infra::stream::sha256_hex;

#[cfg(feature = "server")]
pub const THUMB_SIZE_BUCKETS: [u32; 3] = [240, 480, 960];
#[cfg(feature = "server")]
const THUMB_JPEG_QUALITY: u8 = 82;
#[cfg(feature = "server")]
pub const MAX_COVER_INPUT_BYTES: usize = 32 * 1024 * 1024;
#[cfg(feature = "server")]
const MAX_COVER_IMAGE_DIM: u32 = 16_384;
#[cfg(feature = "server")]
const MAX_COVER_DECODE_ALLOC_BYTES: u64 = 64 * 1024 * 1024;

#[cfg(feature = "server")]
static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Normalizes any requested thumbnail max dimension into fixed discrete buckets `[240, 480, 960]`
/// to prevent arbitrary `?size=` parameters from flooding the disk cache.
#[cfg(feature = "server")]
pub fn normalize_thumb_size(max_size: u32) -> u32 {
    if max_size <= THUMB_SIZE_BUCKETS[0] {
        THUMB_SIZE_BUCKETS[0]
    } else if max_size <= THUMB_SIZE_BUCKETS[1] {
        THUMB_SIZE_BUCKETS[1]
    } else {
        THUMB_SIZE_BUCKETS[2]
    }
}

#[cfg(feature = "server")]
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

#[cfg(feature = "server")]
fn image_has_transparency(img: &image::DynamicImage) -> bool {
    if !img.color().has_alpha() {
        return false;
    }
    img.to_rgba8().pixels().any(|p| p.0[3] < 250)
}

/// Strict magic-byte sniffer that returns `Some(mime)` **only** for safe raster image formats
/// (PNG, JPEG, WebP, GIF, BMP, ICO, AVIF). Returns `None` for SVG, XML, HTML, or unknown payloads.
pub fn sniff_raster_image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some("image/png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.starts_with(b"BM") && bytes.len() >= 14 {
        Some("image/bmp")
    } else if bytes.starts_with(&[0x00, 0x00, 0x01, 0x00]) && bytes.len() >= 6 {
        Some("image/x-icon")
    } else if bytes.len() >= 12
        && &bytes[4..8] == b"ftyp"
        && matches!(&bytes[8..12], b"avif" | b"avis")
    {
        Some("image/avif")
    } else {
        None
    }
}

fn looks_like_svg_or_xml(bytes: &[u8]) -> bool {
    let mut slice = bytes;
    if slice.starts_with(&[0xEF, 0xBB, 0xBF]) {
        slice = &slice[3..];
    }
    let trimmed = slice
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .map(|i| &slice[i..])
        .unwrap_or(&[]);
    let head_len = trimmed.len().min(512);
    let head = String::from_utf8_lossy(&trimmed[..head_len]).to_ascii_lowercase();
    head.starts_with("<svg")
        || head.starts_with("<?xml")
        || head.starts_with("<!doctype svg")
        || head.contains("<svg")
}

pub fn sniff_image_mime(bytes: &[u8]) -> &'static str {
    if let Some(raster) = sniff_raster_image_mime(bytes) {
        raster
    } else if looks_like_svg_or_xml(bytes) {
        "image/svg+xml"
    } else {
        "image/jpeg"
    }
}

#[cfg(feature = "server")]
fn cover_decode_limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_COVER_IMAGE_DIM);
    limits.max_image_height = Some(MAX_COVER_IMAGE_DIM);
    limits.max_alloc = Some(MAX_COVER_DECODE_ALLOC_BYTES);
    limits
}

#[cfg(feature = "server")]
fn decode_bounded_raster_image(bytes: &[u8]) -> DomainResult<image::DynamicImage> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| DomainError::Validation(format!("unreadable cover image: {e}")))?;
    reader.limits(cover_decode_limits());
    let (w, h) = reader
        .into_dimensions()
        .map_err(|e| DomainError::Validation(format!("invalid cover image dimensions: {e}")))?;
    if w == 0 || h == 0 || w > MAX_COVER_IMAGE_DIM || h > MAX_COVER_IMAGE_DIM {
        return Err(DomainError::Validation(format!(
            "cover image dimensions {w}x{h} out of allowed bounds"
        )));
    }
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| DomainError::Validation(format!("unreadable cover image: {e}")))?;
    reader.limits(cover_decode_limits());
    reader
        .decode()
        .map_err(|e| DomainError::Validation(format!("invalid cover image payload: {e}")))
}

#[cfg(feature = "server")]
fn validate_trusted_svg_bytes(bytes: &[u8]) -> DomainResult<()> {
    if bytes.is_empty() || bytes.len() > 256 * 1024 {
        return Err(DomainError::Validation(
            "trusted SVG cover exceeds allowed size".into(),
        ));
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| DomainError::Validation("trusted SVG cover must be UTF-8".into()))?;
    let lower = text.to_ascii_lowercase();
    if !lower.contains("<svg")
        || lower.contains("<script")
        || lower.contains("<foreignobject")
        || lower.contains("<iframe")
        || lower.contains("javascript:")
        || lower.contains("onload=")
        || lower.contains("onerror=")
        || lower.contains("onclick=")
    {
        return Err(DomainError::Validation(
            "unsafe SVG cover payload rejected".into(),
        ));
    }
    Ok(())
}

#[cfg(feature = "server")]
#[derive(Clone)]
pub struct ThumbnailStore {
    orig_dir: PathBuf,
    thumb_dir: PathBuf,
}

#[cfg(feature = "server")]
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
        if bytes.is_empty() {
            return Err(DomainError::Validation(
                "cover image cannot be empty".into(),
            ));
        }
        if bytes.len() > MAX_COVER_INPUT_BYTES {
            return Err(DomainError::Validation(format!(
                "cover image exceeds {} MiB limit",
                MAX_COVER_INPUT_BYTES / (1024 * 1024)
            )));
        }
        let mime = sniff_raster_image_mime(bytes).ok_or_else(|| {
            DomainError::Validation(
                "unsupported cover image format; only raster images (PNG, JPEG, WebP, GIF, BMP, ICO, AVIF) are allowed".into(),
            )
        })?;
        // Verify dimensions via bounded header probe for formats supported by the `image` crate
        // so corrupt/non-image or bomb payloads cannot be persisted.
        if mime != "image/avif" {
            let mut reader = ImageReader::new(Cursor::new(bytes))
                .with_guessed_format()
                .map_err(|e| DomainError::Validation(format!("unreadable cover image: {e}")))?;
            reader.limits(cover_decode_limits());
            let (w, h) = reader.into_dimensions().map_err(|e| {
                DomainError::Validation(format!("invalid cover image dimensions: {e}"))
            })?;
            if w == 0 || h == 0 || w > MAX_COVER_IMAGE_DIM || h > MAX_COVER_IMAGE_DIM {
                return Err(DomainError::Validation(format!(
                    "cover image dimensions {w}x{h} out of allowed bounds"
                )));
            }
        }
        let hash = sha256_hex(bytes);
        let ext = match mime {
            "image/png" => "png",
            "image/webp" => "webp",
            "image/gif" => "gif",
            "image/bmp" => "bmp",
            "image/x-icon" => "ico",
            "image/avif" => "avif",
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

    /// Stores a server-generated fallback SVG cover after validating that it contains no active
    /// scripting constructs. Must never be called with untrusted user uploads or external files.
    pub fn store_trusted_svg_bytes(&self, bytes: &[u8]) -> DomainResult<String> {
        validate_trusted_svg_bytes(bytes)?;
        let hash = sha256_hex(bytes);
        let key = format!("{hash}.svg");
        let dest = self.orig_dir.join(&key);
        if !dest.exists() {
            atomic_write_file(&dest, bytes).map_err(|e| {
                DomainError::Storage(format!("failed to write cover {}: {e}", dest.display()))
            })?;
        }
        Ok(key)
    }

    pub async fn store_trusted_svg_bytes_async(&self, bytes: Vec<u8>) -> DomainResult<String> {
        let store = self.clone();
        tokio::task::spawn_blocking(move || store.store_trusted_svg_bytes(&bytes))
            .await
            .map_err(|e| {
                DomainError::Storage(format!("store_trusted_svg_bytes task failed: {e}"))
            })?
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
            let orig_mime = if is_svg_key {
                validate_trusted_svg_bytes(&orig_bytes)?;
                "image/svg+xml"
            } else {
                sniff_raster_image_mime(&orig_bytes).ok_or_else(|| {
                    DomainError::Validation("stored cover is not a valid raster image".into())
                })?
            };
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
        let orig_mime = sniff_raster_image_mime(&orig_bytes).ok_or_else(|| {
            DomainError::Validation("stored cover is not a valid raster image".into())
        })?;

        match decode_bounded_raster_image(&orig_bytes) {
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
            Err(_) if orig_mime == "image/avif" => Ok(BinaryPayload {
                bytes: orig_bytes,
                mime_type: orig_mime.to_string(),
            }),
            Err(err) => Err(err),
        }
    }
}

#[cfg(all(test, feature = "server"))]
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
    fn thumbnail_store_rejects_svg_and_non_image_payloads() {
        let tmp = TempDir::new().unwrap();
        let store = ThumbnailStore::new(tmp.path()).unwrap();

        let svg = b"  <svg xmlns='http://www.w3.org/2000/svg'><script>alert(1)</script></svg>";
        assert!(store.store_cover_bytes(svg).is_err());
        assert!(
            store
                .store_cover_bytes(b"<html><body>xss</body></html>")
                .is_err()
        );
        assert!(store.store_cover_bytes(b"plain text").is_err());
        assert!(store.store_trusted_svg_bytes(svg).is_err());

        let safe_svg =
            b"<svg xmlns='http://www.w3.org/2000/svg'><rect width='10' height='10'/></svg>";
        let key = store.store_trusted_svg_bytes(safe_svg).unwrap();
        assert!(key.ends_with(".svg"));
        let loaded = store.load_cover(&key, None).unwrap();
        assert_eq!(loaded.mime_type, "image/svg+xml");
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

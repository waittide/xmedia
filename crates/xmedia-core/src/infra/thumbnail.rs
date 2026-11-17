use std::io::Cursor;
use std::path::{Path, PathBuf};

use image::ImageFormat;
use image::imageops::FilterType;

use crate::application::ports::BinaryPayload;
use crate::domain::error::{DomainError, DomainResult};
use crate::infra::stream::sha256_hex;

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
            std::fs::write(&dest, bytes).map_err(|e| {
                DomainError::Storage(format!("failed to write cover {}: {e}", dest.display()))
            })?;
        }
        Ok(key)
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
        if !orig_path.exists() {
            return Err(DomainError::NotFound("cover art not found".into()));
        }
        let orig_bytes = std::fs::read(&orig_path)
            .map_err(|e| DomainError::Storage(format!("failed to read cover: {e}")))?;
        let orig_mime = sniff_image_mime(&orig_bytes);

        let Some(target) = max_size.filter(|&s| (64..=1600).contains(&s)) else {
            return Ok(BinaryPayload {
                bytes: orig_bytes,
                mime_type: orig_mime.to_string(),
            });
        };

        if orig_mime == "image/svg+xml" {
            return Ok(BinaryPayload {
                bytes: orig_bytes,
                mime_type: orig_mime.to_string(),
            });
        }

        let stem = cover_key.split('.').next().unwrap_or(cover_key);
        let thumb_name = format!("{stem}_{target}.webp");
        let thumb_path = self.thumb_dir.join(&thumb_name);
        if thumb_path.exists()
            && let Ok(cached) = std::fs::read(&thumb_path)
        {
            return Ok(BinaryPayload {
                bytes: cached,
                mime_type: "image/webp".to_string(),
            });
        }

        match image::load_from_memory(&orig_bytes) {
            Ok(img) => {
                let resized = img.resize(target, target, FilterType::Lanczos3);
                let mut out = Cursor::new(Vec::new());
                if resized.write_to(&mut out, ImageFormat::WebP).is_ok() {
                    let webp_bytes = out.into_inner();
                    let _ = std::fs::write(&thumb_path, &webp_bytes);
                    return Ok(BinaryPayload {
                        bytes: webp_bytes,
                        mime_type: "image/webp".to_string(),
                    });
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

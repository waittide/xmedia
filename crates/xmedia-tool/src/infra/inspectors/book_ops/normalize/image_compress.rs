use std::io::Cursor;
use std::path::Path;

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{ColorType, DynamicImage, GenericImageView, ImageFormat, ImageReader};
use xmedia_core::infra::thumbnail::sniff_image_mime;

use super::{BookImageCompressFormat, NormalizeBookToEpubOptions};
use crate::infra::inspectors::book::find_sidecar_cover;
use crate::infra::inspectors::book_ops::epub_io::ext_for_image_mime;

pub(super) type EncodedCoverAsset = (Vec<u8>, &'static str, &'static str);

fn mime_for_ext(ext: &str) -> &'static str {
    match ext {
        "webp" => "image/webp",
        "png" => "image/png",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        _ => "image/jpeg",
    }
}

pub(super) fn is_raster_compressible_mime(mime: &str, path: &str) -> bool {
    let lower_mt = mime.to_ascii_lowercase();
    if lower_mt == "image/svg+xml" || path.to_ascii_lowercase().ends_with(".svg") {
        return false;
    }
    lower_mt.starts_with("image/")
        && matches!(
            lower_mt.as_str(),
            "image/jpeg" | "image/jpg" | "image/png" | "image/webp" | "image/bmp" | "image/gif"
        )
}

fn is_animated_gif(bytes: &[u8]) -> bool {
    if bytes.len() < 14 || (!bytes.starts_with(b"GIF87a") && !bytes.starts_with(b"GIF89a")) {
        return false;
    }
    // Count Graphic Control Extension blocks (`0x21 0xF9 0x04`); >= 2 indicates animation
    bytes
        .windows(3)
        .filter(|w| w[0] == 0x21 && w[1] == 0xF9 && w[2] == 0x04)
        .count()
        >= 2
}

pub(super) struct CompressedImage {
    pub(super) bytes: Vec<u8>,
    pub(super) ext: &'static str,
    pub(super) mime: &'static str,
    pub(super) changed: bool,
}

pub(super) fn compress_image_bytes(
    raw_bytes: Vec<u8>,
    orig_mime: &str,
    opts: &NormalizeBookToEpubOptions,
) -> CompressedImage {
    let orig_ext = ext_for_image_mime(orig_mime);
    let orig_static_mime = mime_for_ext(orig_ext);

    if !opts.compress_images || raw_bytes.is_empty() || is_animated_gif(&raw_bytes) {
        return CompressedImage {
            bytes: raw_bytes,
            ext: orig_ext,
            mime: orig_static_mime,
            changed: false,
        };
    }

    let decoded = ImageReader::new(Cursor::new(&raw_bytes))
        .with_guessed_format()
        .ok()
        .and_then(|r| r.decode().ok());
    let Some(mut img) = decoded else {
        return CompressedImage {
            bytes: raw_bytes,
            ext: orig_ext,
            mime: orig_static_mime,
            changed: false,
        };
    };

    let (w, h) = img.dimensions();
    let max_dim = opts.max_image_dimension.filter(|&d| d >= 320);
    let mut resized = false;
    if let Some(limit) = max_dim
        && (w > limit || h > limit)
    {
        img = img.resize(limit, limit, FilterType::Lanczos3);
        resized = true;
    }

    let quality = opts.image_quality.clamp(50, 95);
    let has_alpha = matches!(
        img.color(),
        ColorType::Rgba8
            | ColorType::Rgba16
            | ColorType::La8
            | ColorType::La16
            | ColorType::Rgba32F
    );

    let encode_webp = |im: &DynamicImage| -> Option<Vec<u8>> {
        let mut cursor = Cursor::new(Vec::new());
        im.write_to(&mut cursor, ImageFormat::WebP).ok()?;
        Some(cursor.into_inner())
    };
    let encode_jpeg = |im: &DynamicImage| -> Option<Vec<u8>> {
        let mut out = Vec::new();
        let mut enc = JpegEncoder::new_with_quality(&mut out, quality);
        enc.encode_image(&im.to_rgb8()).ok()?;
        Some(out)
    };

    match opts.image_format {
        BookImageCompressFormat::Webp => {
            let webp = encode_webp(&img);
            // Pure-Rust `image` WebP encoder is lossless: great for PNG/BMP/illustrations,
            // while noisy photos may compress much smaller with lossy JPEG.
            // Pick WebP whenever it saves space (or matches JPEG); otherwise avoid bloating.
            if let Some(w_bytes) = webp {
                if !has_alpha && orig_ext == "jpg" {
                    let jpeg_candidate = encode_jpeg(&img);
                    if let Some(j_bytes) = jpeg_candidate {
                        if w_bytes.len() <= j_bytes.len()
                            && (resized || w_bytes.len() < raw_bytes.len())
                        {
                            return CompressedImage {
                                bytes: w_bytes,
                                ext: "webp",
                                mime: "image/webp",
                                changed: true,
                            };
                        }
                        if resized || j_bytes.len() < raw_bytes.len() {
                            return CompressedImage {
                                bytes: j_bytes,
                                ext: "jpg",
                                mime: "image/jpeg",
                                changed: true,
                            };
                        }
                    }
                } else if resized || w_bytes.len() < raw_bytes.len() {
                    return CompressedImage {
                        bytes: w_bytes,
                        ext: "webp",
                        mime: "image/webp",
                        changed: true,
                    };
                }
            }
        }
        BookImageCompressFormat::Jpeg => {
            if let Some(j_bytes) = encode_jpeg(&img)
                && (resized || j_bytes.len() < raw_bytes.len())
            {
                return CompressedImage {
                    bytes: j_bytes,
                    ext: "jpg",
                    mime: "image/jpeg",
                    changed: true,
                };
            }
        }
        BookImageCompressFormat::Keep => {
            if orig_ext == "jpg" {
                if let Some(j_bytes) = encode_jpeg(&img)
                    && (resized || j_bytes.len() < raw_bytes.len())
                {
                    return CompressedImage {
                        bytes: j_bytes,
                        ext: "jpg",
                        mime: "image/jpeg",
                        changed: true,
                    };
                }
            } else if orig_ext == "webp" {
                if let Some(w_bytes) = encode_webp(&img)
                    && (resized || w_bytes.len() < raw_bytes.len())
                {
                    return CompressedImage {
                        bytes: w_bytes,
                        ext: "webp",
                        mime: "image/webp",
                        changed: true,
                    };
                }
            } else if resized {
                let mut cursor = Cursor::new(Vec::new());
                let fmt = match orig_ext {
                    "png" => ImageFormat::Png,
                    "gif" => ImageFormat::Gif,
                    _ => ImageFormat::Jpeg,
                };
                if img.write_to(&mut cursor, fmt).is_ok() {
                    return CompressedImage {
                        bytes: cursor.into_inner(),
                        ext: orig_ext,
                        mime: orig_static_mime,
                        changed: true,
                    };
                }
            }
        }
    }

    CompressedImage {
        bytes: raw_bytes,
        ext: orig_ext,
        mime: orig_static_mime,
        changed: false,
    }
}

pub(super) fn load_and_compress_sidecar_cover(
    source_path: &Path,
    opts: &NormalizeBookToEpubOptions,
) -> (Option<EncodedCoverAsset>, usize) {
    let Some(cov_path) = find_sidecar_cover(source_path) else {
        return (None, 0);
    };
    let Ok(bytes) = std::fs::read(&cov_path) else {
        return (None, 0);
    };
    if bytes.is_empty() {
        return (None, 0);
    }
    let mime = sniff_image_mime(&bytes);
    let comp = compress_image_bytes(bytes, mime, opts);
    let count = usize::from(comp.changed);
    (Some((comp.bytes, comp.mime, comp.ext)), count)
}

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufReader, BufWriter, Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{ColorType, DynamicImage, GenericImageView, ImageBuffer, ImageFormat, ImageReader};
use lopdf::{Document, Object, ObjectId, decode_text_string};
use quick_xml::escape::escape;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::infra::stream::decode_text_smart;
use xmedia_core::infra::thumbnail::sniff_image_mime;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::domain::media::BookFileMeta;
use crate::infra::atomic_fs::atomic_write_via_temp;
use crate::infra::inspectors::book::{
    find_sidecar_cover, normalize_zip_rel, read_book_meta_any, replace_opf_metadata_block,
};
use crate::infra::inspectors::book_content::{
    DEFAULT_EPUB_CSS, ManifestNode, extract_heading_from_xhtml, is_nav_manifest_node,
    parse_epub_structure, parse_nav_toc_map, parse_ncx_toc_map, percent_decode_path,
    plain_to_xhtml_body, read_zip_bytes, rebuild_opf_manifest_and_spine, relative_href_from_dir,
    render_book_jacket_xhtml, render_nav_xhtml, render_toc_ncx, segment_txt_chapters,
    wrap_full_xhtml_document,
};

fn default_true() -> bool {
    true
}

fn default_max_image_dim() -> Option<u32> {
    Some(1600)
}

fn default_image_quality() -> u8 {
    82
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BookImageCompressFormat {
    #[default]
    Webp,
    Jpeg,
    Keep,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NormalizeBookToEpubOptions {
    /// Keep original source file (default: true).
    /// - For TXT/PDF: keeps source `.txt`/`.pdf` alongside newly created `.epub`.
    /// - For EPUB: backs up original to `<stem>.orig.epub` and writes normalized EPUB to `<stem>.epub`.
    #[serde(default = "default_true")]
    pub keep_source: bool,
    /// Compress cover & embedded illustrations.
    #[serde(default = "default_true")]
    pub compress_images: bool,
    /// Target format for compressed images: "webp" (default) | "jpeg" | "keep".
    #[serde(default)]
    pub image_format: BookImageCompressFormat,
    /// Maximum longest dimension in pixels (e.g. 1600; None or < 320 disables resize).
    #[serde(default = "default_max_image_dim")]
    pub max_image_dimension: Option<u32>,
    /// Quality for lossy JPEG compression/fallback (50..=95, default 82).
    #[serde(default = "default_image_quality")]
    pub image_quality: u8,
    /// Insert or refresh standardized book jacket page (`xmedia_jacket.xhtml`).
    #[serde(default = "default_true")]
    pub generate_jacket: bool,
    /// Remove unreferenced orphan files & OS junk (`__MACOSX`, `.DS_Store`, `Thumbs.db`).
    #[serde(default = "default_true")]
    pub remove_orphans: bool,
    /// For scanned/image-only PDFs without extractable text, convert page images into image-page EPUB.
    #[serde(default = "default_true")]
    pub pdf_scanned_fallback: bool,
}

impl Default for NormalizeBookToEpubOptions {
    fn default() -> Self {
        Self {
            keep_source: true,
            compress_images: true,
            image_format: BookImageCompressFormat::Webp,
            max_image_dimension: default_max_image_dim(),
            image_quality: default_image_quality(),
            generate_jacket: true,
            remove_orphans: true,
            pdf_scanned_fallback: true,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct NormalizeBookOutcome {
    pub output_path: PathBuf,
    pub backup_path: Option<PathBuf>,
    pub source_format: String,
    pub before_bytes: u64,
    pub after_bytes: u64,
    pub chapter_count: usize,
    pub compressed_images: usize,
    pub removed_orphans: usize,
    pub deleted_source: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct NormalizeBookItemReportDto {
    pub source_rel_path: String,
    pub output_rel_path: Option<String>,
    pub backup_rel_path: Option<String>,
    pub source_format: String,
    pub before_bytes: u64,
    pub after_bytes: u64,
    pub chapter_count: usize,
    pub compressed_images: usize,
    pub removed_orphans: usize,
    pub deleted_source: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NormalizeBooksBatchReportDto {
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub total_before_bytes: u64,
    pub total_after_bytes: u64,
    pub items: Vec<NormalizeBookItemReportDto>,
}

fn deflate_best_options() -> SimpleFileOptions {
    SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .compression_level(Some(9))
}

fn stored_options() -> SimpleFileOptions {
    SimpleFileOptions::default().compression_method(CompressionMethod::Stored)
}

fn entry_zip_options(path: &str, media_type: &str) -> SimpleFileOptions {
    let lower_mt = media_type.to_ascii_lowercase();
    let lower_path = path.to_ascii_lowercase();
    let precompressed = lower_mt == "image/jpeg"
        || lower_mt == "image/webp"
        || lower_mt == "font/woff2"
        || lower_mt == "application/font-woff2"
        || lower_path.ends_with(".jpg")
        || lower_path.ends_with(".jpeg")
        || lower_path.ends_with(".webp")
        || lower_path.ends_with(".woff2");
    if precompressed {
        stored_options()
    } else {
        deflate_best_options()
    }
}

fn mime_for_ext(ext: &str) -> &'static str {
    match ext {
        "webp" => "image/webp",
        "png" => "image/png",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        _ => "image/jpeg",
    }
}

fn ext_for_mime(mime: &str) -> &'static str {
    match mime.to_ascii_lowercase().as_str() {
        "image/webp" => "webp",
        "image/png" => "png",
        "image/gif" => "gif",
        "image/svg+xml" => "svg",
        _ => "jpg",
    }
}

fn is_raster_compressible_mime(mime: &str, path: &str) -> bool {
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

struct CompressedImage {
    bytes: Vec<u8>,
    ext: &'static str,
    mime: &'static str,
    changed: bool,
}

fn compress_image_bytes(
    raw_bytes: Vec<u8>,
    orig_mime: &str,
    opts: &NormalizeBookToEpubOptions,
) -> CompressedImage {
    let orig_ext = ext_for_mime(orig_mime);
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

fn is_os_junk_entry(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.starts_with("__macosx/")
        || lower.ends_with("/.ds_store")
        || lower == ".ds_store"
        || lower.ends_with("/thumbs.db")
        || lower == "thumbs.db"
        || lower.ends_with("/desktop.ini")
        || lower == "desktop.ini"
}

fn unique_orig_epub_backup_path(epub_path: &Path) -> PathBuf {
    let parent = epub_path.parent().unwrap_or_else(|| Path::new(""));
    let stem = epub_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("book");
    let first = parent.join(format!("{stem}.orig.epub"));
    if !first.exists() {
        return first;
    }
    for idx in 2..1000 {
        let candidate = parent.join(format!("{stem}.orig.{idx}.epub"));
        if !candidate.exists() {
            return candidate;
        }
    }
    parent.join(format!("{stem}.orig.{}.epub", Uuid::now_v7()))
}

fn remove_source_and_stem_sidecars(source_path: &Path) {
    let _ = std::fs::remove_file(source_path);
    // Clean exact-stem sidecar .opf and cover only (never touch shared directory metadata.opf/cover.jpg if other books exist)
    let stem_opf = source_path.with_extension("opf");
    if stem_opf.is_file() {
        let _ = std::fs::remove_file(stem_opf);
    }
    for ext in ["jpg", "jpeg", "png", "webp"] {
        let stem_cov = source_path.with_extension(ext);
        if stem_cov.is_file() {
            let _ = std::fs::remove_file(stem_cov);
        }
    }
}

/// Entry point: converts or normalizes a `.txt`, `.epub`, or `.pdf` book into a
/// clean, compressed EPUB 3.0 file.
pub fn normalize_book_to_epub(
    source_path: &Path,
    opts: &NormalizeBookToEpubOptions,
) -> DomainResult<NormalizeBookOutcome> {
    if !source_path.is_file() {
        return Err(DomainError::NotFound(format!(
            "源文件不存在: {}",
            source_path.display()
        )));
    }
    let ext = source_path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    match ext.as_str() {
        "txt" => normalize_txt_to_epub(source_path, opts),
        "epub" => normalize_existing_epub(source_path, opts),
        "pdf" => normalize_pdf_to_epub(source_path, opts),
        other => Err(DomainError::Validation(format!(
            "暂不支持将 '.{other}' 转为规范化 EPUB（支持 txt / epub / pdf）"
        ))),
    }
}

struct GeneratedChapter {
    title: String,
    level: u8,
    body_xhtml: String,
}

struct GeneratedImageAsset {
    /// Relative path under `OEBPS/`, e.g. `Images/page_0001.webp`
    rel_href: String,
    media_type: &'static str,
    bytes: Vec<u8>,
}

#[allow(clippy::too_many_arguments)]
fn write_fresh_normalized_epub(
    epub_path: &Path,
    meta: &BookFileMeta,
    chapters: &[GeneratedChapter],
    extra_images: &[GeneratedImageAsset],
    cover: Option<(Vec<u8>, &'static str, &'static str)>,
    css: &str,
    generate_jacket: bool,
) -> DomainResult<()> {
    let lang = meta.language.as_deref().unwrap_or("zh-CN");
    let book_uuid = format!("urn:uuid:{}", Uuid::now_v7());

    atomic_write_via_temp(epub_path, |staging| {
        let out_file = File::create(staging)
            .map_err(|e| DomainError::Storage(format!("创建 EPUB 临时文件失败: {e}")))?;
        let mut writer = ZipWriter::new(BufWriter::new(out_file));

        let stored = stored_options();
        let deflated = deflate_best_options();

        // 1. OCF mimetype (entry #0, Stored)
        writer
            .start_file("mimetype", stored)
            .map_err(|e| DomainError::Storage(format!("epub mimetype start error: {e}")))?;
        writer
            .write_all(b"application/epub+zip")
            .map_err(|e| DomainError::Storage(format!("epub mimetype write error: {e}")))?;

        // 2. META-INF/container.xml
        let container_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>
"#;
        writer
            .start_file("META-INF/container.xml", deflated)
            .map_err(|e| DomainError::Storage(format!("epub container error: {e}")))?;
        writer
            .write_all(container_xml.as_bytes())
            .map_err(|e| DomainError::Storage(format!("epub container write error: {e}")))?;

        // 3. Stylesheet
        writer
            .start_file("OEBPS/Styles/style.css", deflated)
            .map_err(|e| DomainError::Storage(format!("epub css error: {e}")))?;
        writer
            .write_all(css.as_bytes())
            .map_err(|e| DomainError::Storage(format!("epub css write error: {e}")))?;

        let mut manifest_nodes = vec![
            ManifestNode {
                id: "style-css".to_string(),
                href: "Styles/style.css".to_string(),
                full_path: "OEBPS/Styles/style.css".to_string(),
                media_type: "text/css".to_string(),
                properties: None,
            },
            ManifestNode {
                id: "nav".to_string(),
                href: "nav.xhtml".to_string(),
                full_path: "OEBPS/nav.xhtml".to_string(),
                media_type: "application/xhtml+xml".to_string(),
                properties: Some("nav".to_string()),
            },
            ManifestNode {
                id: "ncx".to_string(),
                href: "toc.ncx".to_string(),
                full_path: "OEBPS/toc.ncx".to_string(),
                media_type: "application/x-dtbncx+xml".to_string(),
                properties: None,
            },
        ];
        let mut spine_ids = Vec::new();
        let mut toc_triples: Vec<(String, String, u8)> = Vec::new();

        // 4. Optional Cover Image & Cover Wrapper Page
        let mut cover_item_id: Option<&str> = None;
        let mut cover_rel_for_jacket: Option<String> = None;
        if let Some((cov_bytes, cov_mime, cov_ext)) = &cover {
            let img_rel = format!("Images/cover.{cov_ext}");
            let img_full = format!("OEBPS/{img_rel}");
            writer
                .start_file(&img_full, entry_zip_options(&img_full, cov_mime))
                .map_err(|e| DomainError::Storage(format!("epub cover image error: {e}")))?;
            writer
                .write_all(cov_bytes)
                .map_err(|e| DomainError::Storage(format!("epub cover write error: {e}")))?;

            manifest_nodes.push(ManifestNode {
                id: "cover-image".to_string(),
                href: img_rel.clone(),
                full_path: img_full,
                media_type: (*cov_mime).to_string(),
                properties: Some("cover-image".to_string()),
            });
            cover_item_id = Some("cover-image");
            cover_rel_for_jacket = Some(format!("../{img_rel}"));

            let cover_body = format!(
                "  <div class=\"cover-page\"><img src=\"../{img_rel}\" alt=\"{}\"/></div>",
                escape(&meta.title)
            );
            let cover_xhtml = wrap_full_xhtml_document(
                "封面",
                lang,
                None,
                Some("../Styles/style.css"),
                &cover_body,
            );
            writer
                .start_file("OEBPS/Text/cover.xhtml", deflated)
                .map_err(|e| DomainError::Storage(format!("epub cover page error: {e}")))?;
            writer
                .write_all(cover_xhtml.as_bytes())
                .map_err(|e| DomainError::Storage(format!("epub cover page write error: {e}")))?;

            manifest_nodes.push(ManifestNode {
                id: "cover-page".to_string(),
                href: "Text/cover.xhtml".to_string(),
                full_path: "OEBPS/Text/cover.xhtml".to_string(),
                media_type: "application/xhtml+xml".to_string(),
                properties: None,
            });
            spine_ids.push("cover-page".to_string());
        }

        // 5. Optional Jacket Metadata Page
        if generate_jacket {
            let jacket_xhtml = render_book_jacket_xhtml(
                meta,
                cover_rel_for_jacket.as_deref(),
                Some("../Styles/style.css"),
            );
            writer
                .start_file("OEBPS/Text/xmedia_jacket.xhtml", deflated)
                .map_err(|e| DomainError::Storage(format!("epub jacket error: {e}")))?;
            writer
                .write_all(jacket_xhtml.as_bytes())
                .map_err(|e| DomainError::Storage(format!("epub jacket write error: {e}")))?;

            manifest_nodes.push(ManifestNode {
                id: "xmedia-jacket".to_string(),
                href: "Text/xmedia_jacket.xhtml".to_string(),
                full_path: "OEBPS/Text/xmedia_jacket.xhtml".to_string(),
                media_type: "application/xhtml+xml".to_string(),
                properties: None,
            });
            spine_ids.push("xmedia-jacket".to_string());
            toc_triples.push((
                "书籍简介".to_string(),
                "OEBPS/Text/xmedia_jacket.xhtml".to_string(),
                0,
            ));
        }

        // 6. Extra Images (e.g. PDF extracted page images)
        for (idx, asset) in extra_images.iter().enumerate() {
            let full_path = format!("OEBPS/{}", asset.rel_href);
            writer
                .start_file(&full_path, entry_zip_options(&full_path, asset.media_type))
                .map_err(|e| DomainError::Storage(format!("epub image start error: {e}")))?;
            writer
                .write_all(&asset.bytes)
                .map_err(|e| DomainError::Storage(format!("epub image write error: {e}")))?;
            manifest_nodes.push(ManifestNode {
                id: format!("img-{:04}", idx + 1),
                href: asset.rel_href.clone(),
                full_path,
                media_type: asset.media_type.to_string(),
                properties: None,
            });
        }

        // 7. Chapters
        for (idx, ch) in chapters.iter().enumerate() {
            let title = if ch.title.trim().is_empty() {
                format!("第 {} 章", idx + 1)
            } else {
                ch.title.trim().to_string()
            };
            let level = ch.level.min(1);
            let item_id = format!("ch-{:04}", idx + 1);
            let rel_href = format!("Text/chapter_{:04}.xhtml", idx + 1);
            let full_path = format!("OEBPS/{rel_href}");

            let xhtml = wrap_full_xhtml_document(
                &title,
                lang,
                None,
                Some("../Styles/style.css"),
                &ch.body_xhtml,
            );
            writer
                .start_file(&full_path, deflated)
                .map_err(|e| DomainError::Storage(format!("epub chapter start error: {e}")))?;
            writer
                .write_all(xhtml.as_bytes())
                .map_err(|e| DomainError::Storage(format!("epub chapter write error: {e}")))?;

            manifest_nodes.push(ManifestNode {
                id: item_id.clone(),
                href: rel_href,
                full_path: full_path.clone(),
                media_type: "application/xhtml+xml".to_string(),
                properties: None,
            });
            spine_ids.push(item_id);
            toc_triples.push((title, full_path, level));
        }

        // 8. nav.xhtml + toc.ncx
        let oebps_dir = Path::new("OEBPS");
        let nav_xml = render_nav_xhtml(&meta.title, lang, &toc_triples, oebps_dir);
        writer
            .start_file("OEBPS/nav.xhtml", deflated)
            .map_err(|e| DomainError::Storage(format!("epub nav error: {e}")))?;
        writer
            .write_all(nav_xml.as_bytes())
            .map_err(|e| DomainError::Storage(format!("epub nav write error: {e}")))?;

        let ncx_xml = render_toc_ncx(&book_uuid, &meta.title, &toc_triples, oebps_dir);
        writer
            .start_file("OEBPS/toc.ncx", deflated)
            .map_err(|e| DomainError::Storage(format!("epub ncx error: {e}")))?;
        writer
            .write_all(ncx_xml.as_bytes())
            .map_err(|e| DomainError::Storage(format!("epub ncx write error: {e}")))?;

        // 9. OEBPS/content.opf
        let base_opf = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"bookid\">\n  <metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n    <dc:identifier id=\"bookid\">{}</dc:identifier>\n  </metadata>\n  <manifest></manifest>\n  <spine toc=\"ncx\"></spine>\n</package>\n",
            escape(&book_uuid)
        );
        let with_meta = replace_opf_metadata_block(&base_opf, meta, cover_item_id, "bookid", "3.0");
        let final_opf = rebuild_opf_manifest_and_spine(
            &with_meta,
            &manifest_nodes,
            &spine_ids,
            "ncx",
            None,
            meta,
            "bookid",
            "3.0",
        );

        writer
            .start_file("OEBPS/content.opf", deflated)
            .map_err(|e| DomainError::Storage(format!("epub opf error: {e}")))?;
        writer
            .write_all(final_opf.as_bytes())
            .map_err(|e| DomainError::Storage(format!("epub opf write error: {e}")))?;

        writer
            .finish()
            .map_err(|e| DomainError::Storage(format!("epub finalize error: {e}")))?;
        Ok(())
    })
}

fn load_and_compress_sidecar_cover(
    source_path: &Path,
    opts: &NormalizeBookToEpubOptions,
) -> (Option<(Vec<u8>, &'static str, &'static str)>, usize) {
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

fn normalize_txt_to_epub(
    txt_path: &Path,
    opts: &NormalizeBookToEpubOptions,
) -> DomainResult<NormalizeBookOutcome> {
    let before_bytes = std::fs::metadata(txt_path).map(|m| m.len()).unwrap_or(0);
    let epub_path = txt_path.with_extension("epub");
    if epub_path.exists() {
        return Err(DomainError::Conflict(format!(
            "目标文件 '{}' 已存在，请先重命名或移除已有同名 EPUB",
            epub_path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        )));
    }

    let meta = read_book_meta_any(txt_path)?;
    let bytes =
        std::fs::read(txt_path).map_err(|e| DomainError::Storage(format!("读取 TXT 失败: {e}")))?;
    let (decoded, _) = decode_text_smart(&bytes);
    let segmented = segment_txt_chapters(&decoded, None, None)?;
    let chapters: Vec<GeneratedChapter> = segmented
        .into_iter()
        .map(|c| GeneratedChapter {
            title: c.title,
            level: c.level,
            body_xhtml: c.xhtml_body,
        })
        .collect();

    let (cover, compressed_images) = load_and_compress_sidecar_cover(txt_path, opts);

    write_fresh_normalized_epub(
        &epub_path,
        &meta,
        &chapters,
        &[],
        cover,
        DEFAULT_EPUB_CSS,
        opts.generate_jacket,
    )?;

    let after_bytes = std::fs::metadata(&epub_path).map(|m| m.len()).unwrap_or(0);
    let deleted_source = if !opts.keep_source {
        remove_source_and_stem_sidecars(txt_path);
        true
    } else {
        false
    };

    Ok(NormalizeBookOutcome {
        output_path: epub_path,
        backup_path: None,
        source_format: "txt".to_string(),
        before_bytes,
        after_bytes,
        chapter_count: chapters.len(),
        compressed_images,
        removed_orphans: 0,
        deleted_source,
    })
}

fn rewrite_image_refs_in_text(
    doc_full_path: &str,
    content: &str,
    renames: &[(String, String)],
) -> String {
    if renames.is_empty() {
        return content.to_string();
    }
    let doc_dir = Path::new(doc_full_path)
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let mut updated = content.to_string();
    for (old_full, new_full) in renames {
        let old_rel = relative_href_from_dir(doc_dir, old_full);
        let new_rel = relative_href_from_dir(doc_dir, new_full);
        if old_rel != new_rel && updated.contains(&old_rel) {
            updated = updated.replace(&old_rel, &new_rel);
        }
        if old_full != new_full && updated.contains(old_full.as_str()) {
            updated = updated.replace(old_full.as_str(), new_full.as_str());
        }
        // Handle percent-decoded relative paths if source used raw Unicode or encoded paths
        let old_rel_dec = percent_decode_path(&old_rel);
        let new_rel_dec = percent_decode_path(&new_rel);
        if old_rel_dec != old_rel && old_rel_dec != new_rel_dec && updated.contains(&old_rel_dec) {
            updated = updated.replace(&old_rel_dec, &new_rel_dec);
        }
    }
    updated
}

fn replace_path_extension(path: &str, new_ext: &str) -> String {
    if let Some(dot) = path.rfind('.') {
        let slash = path.rfind('/').unwrap_or(0);
        if dot > slash {
            return format!("{}.{new_ext}", &path[..dot]);
        }
    }
    format!("{path}.{new_ext}")
}

fn normalize_existing_epub(
    epub_path: &Path,
    opts: &NormalizeBookToEpubOptions,
) -> DomainResult<NormalizeBookOutcome> {
    let before_bytes = std::fs::metadata(epub_path).map(|m| m.len()).unwrap_or(0);
    let meta = read_book_meta_any(epub_path)?;
    let lang = meta.language.as_deref().unwrap_or("zh-CN");

    let f =
        File::open(epub_path).map_err(|e| DomainError::Storage(format!("打开 EPUB 失败: {e}")))?;
    let mut archive = ZipArchive::new(BufReader::new(f))
        .map_err(|e| DomainError::Validation(format!("无效的 EPUB 压缩包: {e}")))?;
    let info = parse_epub_structure(&mut archive)?;

    // Existing TOC map
    let mut toc_map: HashMap<String, (String, u8)> = HashMap::new();
    if let Some(nav_node) = info.manifest.iter().find(|m| is_nav_manifest_node(m))
        && let Some(nav_bytes) = read_zip_bytes(&mut archive, &nav_node.full_path)
    {
        let nav_dir = Path::new(&nav_node.full_path)
            .parent()
            .unwrap_or_else(|| Path::new(""));
        toc_map = parse_nav_toc_map(&nav_bytes, nav_dir);
    }
    if toc_map.is_empty()
        && let Some(ncx_node) = info.manifest.iter().find(|m| {
            info.toc_ncx_attr.as_deref() == Some(m.id.as_str())
                || m.media_type
                    .eq_ignore_ascii_case("application/x-dtbncx+xml")
        })
        && let Some(ncx_bytes) = read_zip_bytes(&mut archive, &ncx_node.full_path)
    {
        let ncx_dir = Path::new(&ncx_node.full_path)
            .parent()
            .unwrap_or_else(|| Path::new(""));
        toc_map = parse_ncx_toc_map(&ncx_bytes, ncx_dir);
    }

    // Locate cover image node in manifest
    let cover_img_id = info
        .manifest
        .iter()
        .find(|m| {
            m.media_type.starts_with("image/")
                && (m
                    .properties
                    .as_deref()
                    .is_some_and(|p| p.split_whitespace().any(|t| t == "cover-image"))
                    || info.cover_meta_id.as_deref() == Some(m.id.as_str()))
        })
        .or_else(|| {
            info.manifest.iter().find(|m| {
                m.media_type.starts_with("image/")
                    && (m.id.to_ascii_lowercase().contains("cover")
                        || m.href.to_ascii_lowercase().contains("cover"))
            })
        })
        .map(|m| m.id.clone());

    let primary_css_full = info
        .manifest
        .iter()
        .find(|m| m.media_type.eq_ignore_ascii_case("text/css"))
        .map(|m| m.full_path.clone());

    // Read all existing archive files into memory (skipping OS junk)
    let mut raw_entries: HashMap<String, Vec<u8>> = HashMap::new();
    let mut entry_order: Vec<String> = Vec::new();
    let mut junk_removed = 0usize;
    for i in 0..archive.len() {
        let Ok(mut zf) = archive.by_index(i) else {
            continue;
        };
        let Ok(name_ref) = zf.name() else {
            continue;
        };
        let name = name_ref.into_owned();
        if name.ends_with('/') {
            continue;
        }
        if opts.remove_orphans && is_os_junk_entry(&name) {
            junk_removed += 1;
            continue;
        }
        let mut buf = Vec::with_capacity(zf.size() as usize);
        if zf.read_to_end(&mut buf).is_ok() {
            entry_order.push(name.clone());
            raw_entries.insert(name, buf);
        }
    }
    drop(archive);

    // Helper to look up bytes by manifest full_path (with percent-decode fallback)
    let get_entry_bytes =
        |entries: &HashMap<String, Vec<u8>>, full_path: &str| -> Option<(String, Vec<u8>)> {
            if let Some(b) = entries.get(full_path) {
                return Some((full_path.to_string(), b.clone()));
            }
            if full_path.contains('%') {
                let dec = percent_decode_path(full_path);
                if let Some(b) = entries.get(&dec) {
                    return Some((dec, b.clone()));
                }
            }
            None
        };

    // Prune unreferenced manifest images BEFORE compressing so we don't waste CPU on orphans
    let mut removed_orphans = junk_removed;
    let initial_manifest: Vec<ManifestNode> = if opts.remove_orphans {
        let mut all_doc_sources = String::new();
        for m in &info.manifest {
            let mt = m.media_type.to_ascii_lowercase();
            if (mt.contains("html")
                || mt.contains("xml")
                || mt.contains("css")
                || mt.contains("svg"))
                && let Some((_, bytes)) = get_entry_bytes(&raw_entries, &m.full_path)
            {
                all_doc_sources.push_str(&String::from_utf8_lossy(&bytes));
                all_doc_sources.push('\n');
            }
        }
        let all_doc_decoded = percent_decode_path(&all_doc_sources);

        let mut kept = Vec::with_capacity(info.manifest.len());
        for m in info.manifest.clone() {
            if m.media_type.starts_with("image/") && cover_img_id.as_deref() != Some(m.id.as_str())
            {
                let fname = Path::new(&m.full_path)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default();
                let fname_dec = percent_decode_path(fname);
                let referenced = !fname.is_empty()
                    && (all_doc_sources.contains(fname)
                        || all_doc_decoded.contains(&fname_dec)
                        || all_doc_sources.contains(&m.href));
                if !referenced {
                    raw_entries.remove(&m.full_path);
                    raw_entries.remove(&percent_decode_path(&m.full_path));
                    removed_orphans += 1;
                    continue;
                }
            }
            kept.push(m);
        }
        kept
    } else {
        info.manifest.clone()
    };

    // Compress raster images in manifest & track renames
    let mut updated_manifest: Vec<ManifestNode> = Vec::with_capacity(initial_manifest.len() + 3);
    let mut image_renames: Vec<(String, String)> = Vec::new();
    let mut renamed_source_keys: HashSet<String> = HashSet::new();
    let mut compressed_images = 0usize;
    let mut used_full_paths: HashSet<String> = initial_manifest
        .iter()
        .map(|m| m.full_path.clone())
        .collect();

    for mut m in initial_manifest {
        if opts.compress_images
            && is_raster_compressible_mime(&m.media_type, &m.full_path)
            && let Some((actual_key, bytes)) = get_entry_bytes(&raw_entries, &m.full_path)
        {
            let comp = compress_image_bytes(bytes, &m.media_type, opts);
            if comp.changed {
                compressed_images += 1;
                let mut new_full = if comp.mime != m.media_type {
                    replace_path_extension(&m.full_path, comp.ext)
                } else {
                    m.full_path.clone()
                };
                if new_full != m.full_path && used_full_paths.contains(&new_full) {
                    // Avoid collision with another existing file
                    new_full = m.full_path.clone();
                }
                if new_full != m.full_path {
                    used_full_paths.insert(new_full.clone());
                    image_renames.push((m.full_path.clone(), new_full.clone()));
                    if actual_key != m.full_path {
                        image_renames.push((actual_key.clone(), new_full.clone()));
                    }
                    renamed_source_keys.insert(actual_key);
                    m.full_path = new_full.clone();
                    m.href = relative_href_from_dir(&info.opf_dir, &new_full);
                    m.media_type = comp.mime.to_string();
                } else {
                    m.media_type = comp.mime.to_string();
                }
                raw_entries.insert(m.full_path.clone(), comp.bytes);
            }
        }
        updated_manifest.push(m);
    }

    for old_key in &renamed_source_keys {
        raw_entries.remove(old_key);
    }

    // Rewrite references inside XHTML / HTML / CSS / SVG files when image extensions changed
    if !image_renames.is_empty() {
        for m in &updated_manifest {
            let mt = m.media_type.to_ascii_lowercase();
            if (mt.contains("html")
                || mt.contains("xml")
                || mt.contains("css")
                || mt.contains("svg"))
                && let Some((actual_key, bytes)) = get_entry_bytes(&raw_entries, &m.full_path)
            {
                let text = String::from_utf8_lossy(&bytes);
                let rewritten = rewrite_image_refs_in_text(&m.full_path, &text, &image_renames);
                if rewritten != text {
                    raw_entries.insert(actual_key, rewritten.into_bytes());
                }
            }
        }
    }

    // Optional jacket page generation/refresh
    let mut spine_ids = info.spine_idrefs.clone();
    let mut jacket_id_opt: Option<String> = None;
    let mut jacket_full_opt: Option<String> = None;

    if opts.generate_jacket {
        let cover_img_full = cover_img_id.as_deref().and_then(|cid| {
            updated_manifest
                .iter()
                .find(|m| m.id == cid)
                .map(|m| m.full_path.clone())
        });

        let (jacket_id, jacket_full_path) = if let Some(existing_jacket) = updated_manifest
            .iter()
            .find(|m| m.id == "xmedia-jacket" || m.href.ends_with("xmedia_jacket.xhtml"))
        {
            (
                existing_jacket.id.clone(),
                existing_jacket.full_path.clone(),
            )
        } else {
            let jf = normalize_zip_rel(&info.opf_dir, "Text/xmedia_jacket.xhtml");
            let jh = relative_href_from_dir(&info.opf_dir, &jf);
            let jid = "xmedia-jacket".to_string();
            updated_manifest.push(ManifestNode {
                id: jid.clone(),
                href: jh,
                full_path: jf.clone(),
                media_type: "application/xhtml+xml".to_string(),
                properties: None,
            });
            (jid, jf)
        };

        let jacket_dir = Path::new(&jacket_full_path)
            .parent()
            .unwrap_or_else(|| Path::new(""));
        let jacket_cover_rel = cover_img_full
            .as_deref()
            .map(|cf| relative_href_from_dir(jacket_dir, cf));
        let jacket_css_rel = primary_css_full
            .as_deref()
            .map(|sf| relative_href_from_dir(jacket_dir, sf));
        let jacket_xhtml = render_book_jacket_xhtml(
            &meta,
            jacket_cover_rel.as_deref(),
            jacket_css_rel.as_deref(),
        );
        raw_entries.insert(jacket_full_path.clone(), jacket_xhtml.into_bytes());

        if !spine_ids.iter().any(|id| id == &jacket_id) {
            let insert_pos = if spine_ids
                .first()
                .and_then(|fid| updated_manifest.iter().find(|m| &m.id == fid))
                .is_some_and(|n| {
                    let hl = n.href.to_ascii_lowercase();
                    hl.contains("cover") || hl.ends_with("titlepage.xhtml")
                }) {
                1
            } else {
                0
            };
            spine_ids.insert(insert_pos.min(spine_ids.len()), jacket_id.clone());
        }
        jacket_id_opt = Some(jacket_id);
        jacket_full_opt = Some(jacket_full_path);
    }

    // Build TOC triples from spine
    let manifest_by_id: HashMap<String, ManifestNode> = updated_manifest
        .iter()
        .map(|m| (m.id.clone(), m.clone()))
        .collect();

    let mut toc_triples: Vec<(String, String, u8)> = Vec::new();
    if let Some(ref jf) = jacket_full_opt {
        toc_triples.push(("书籍简介".to_string(), jf.clone(), 0));
    }
    let mut chapter_count = 0usize;
    for (idx, idref) in spine_ids.iter().enumerate() {
        if jacket_id_opt.as_deref() == Some(idref.as_str()) {
            continue;
        }
        let Some(node) = manifest_by_id.get(idref) else {
            continue;
        };
        if is_nav_manifest_node(node) {
            continue;
        }
        let hl = node.href.to_ascii_lowercase();
        if idx == 0 && (hl.contains("cover") || hl.ends_with("titlepage.xhtml")) {
            continue;
        }
        chapter_count += 1;
        if let Some((t, lvl)) = toc_map.get(&node.full_path) {
            toc_triples.push((t.clone(), node.full_path.clone(), *lvl));
        } else if let Some((_, bytes)) = get_entry_bytes(&raw_entries, &node.full_path) {
            let s = String::from_utf8_lossy(&bytes);
            let title =
                extract_heading_from_xhtml(&s).unwrap_or_else(|| format!("第 {chapter_count} 章"));
            toc_triples.push((title, node.full_path.clone(), 0));
        }
    }

    // Ensure nav.xhtml & toc.ncx exist and are synced
    let (nav_full_path, _) =
        if let Some(n) = updated_manifest.iter().find(|m| is_nav_manifest_node(m)) {
            (n.full_path.clone(), n.id.clone())
        } else {
            let nf = normalize_zip_rel(&info.opf_dir, "nav.xhtml");
            let nh = relative_href_from_dir(&info.opf_dir, &nf);
            updated_manifest.push(ManifestNode {
                id: "nav".to_string(),
                href: nh,
                full_path: nf.clone(),
                media_type: "application/xhtml+xml".to_string(),
                properties: Some("nav".to_string()),
            });
            (nf, "nav".to_string())
        };
    let nav_dir = Path::new(&nav_full_path)
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let nav_xml = render_nav_xhtml(&meta.title, lang, &toc_triples, nav_dir);
    raw_entries.insert(nav_full_path, nav_xml.into_bytes());

    let (ncx_full_path, ncx_id) = if let Some(n) = updated_manifest.iter().find(|m| {
        info.toc_ncx_attr.as_deref() == Some(m.id.as_str())
            || m.media_type
                .eq_ignore_ascii_case("application/x-dtbncx+xml")
    }) {
        (n.full_path.clone(), n.id.clone())
    } else {
        let nf = normalize_zip_rel(&info.opf_dir, "toc.ncx");
        let nh = relative_href_from_dir(&info.opf_dir, &nf);
        updated_manifest.push(ManifestNode {
            id: "ncx".to_string(),
            href: nh,
            full_path: nf.clone(),
            media_type: "application/x-dtbncx+xml".to_string(),
            properties: None,
        });
        (nf, "ncx".to_string())
    };
    let ncx_dir = Path::new(&ncx_full_path)
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let ncx_xml = render_toc_ncx(&info.unique_id, &meta.title, &toc_triples, ncx_dir);
    raw_entries.insert(ncx_full_path, ncx_xml.into_bytes());

    // Build final normalized EPUB 3.0 OPF
    let updated_opf = rebuild_opf_manifest_and_spine(
        &info.raw_opf,
        &updated_manifest,
        &spine_ids,
        &ncx_id,
        info.spine_page_progression.as_deref(),
        &meta,
        &info.unique_id,
        "3.0",
    );
    raw_entries.insert(info.opf_path.clone(), updated_opf.into_bytes());

    // Determine valid zip paths
    let mut manifest_mime_by_path: HashMap<String, String> = HashMap::new();
    let mut valid_zip_paths: HashSet<String> = HashSet::new();
    valid_zip_paths.insert("mimetype".to_string());
    valid_zip_paths.insert(info.opf_path.clone());
    for m in &updated_manifest {
        valid_zip_paths.insert(m.full_path.clone());
        manifest_mime_by_path.insert(m.full_path.clone(), m.media_type.clone());
        if m.full_path.contains('%') {
            let dec = percent_decode_path(&m.full_path);
            valid_zip_paths.insert(dec.clone());
            manifest_mime_by_path.insert(dec, m.media_type.clone());
        }
    }

    if opts.remove_orphans {
        let extra_unmanifested: Vec<String> = raw_entries
            .keys()
            .filter(|k| {
                !k.starts_with("META-INF/")
                    && !k.eq_ignore_ascii_case("mimetype")
                    && !valid_zip_paths.contains(*k)
            })
            .cloned()
            .collect();
        for k in extra_unmanifested {
            raw_entries.remove(&k);
            removed_orphans += 1;
        }
    }

    // Write re-packed compressed EPUB into staging path, then apply Option B backup if keep_source
    let write_epub_to = |target: &Path| -> DomainResult<()> {
        let out_file = File::create(target)
            .map_err(|e| DomainError::Storage(format!("写入规范化 EPUB 失败: {e}")))?;
        let mut writer = ZipWriter::new(BufWriter::new(out_file));

        // Entry #0: uncompressed mimetype
        writer
            .start_file("mimetype", stored_options())
            .map_err(|e| DomainError::Storage(format!("epub mimetype start error: {e}")))?;
        writer
            .write_all(b"application/epub+zip")
            .map_err(|e| DomainError::Storage(format!("epub mimetype write error: {e}")))?;

        let mut written: HashSet<String> = HashSet::new();
        written.insert("mimetype".to_string());

        // Write existing entries in deterministic order first, then newly added entries
        let mut ordered_keys: Vec<String> = entry_order
            .iter()
            .filter(|k| !written.contains(*k) && raw_entries.contains_key(*k))
            .cloned()
            .collect();
        let mut extra_keys: Vec<String> = raw_entries
            .keys()
            .filter(|k| !written.contains(*k) && !ordered_keys.contains(k))
            .cloned()
            .collect();
        extra_keys.sort();
        ordered_keys.extend(extra_keys);

        for key in ordered_keys {
            if written.contains(&key) {
                continue;
            }
            let Some(bytes) = raw_entries.get(&key) else {
                continue;
            };
            let mt = manifest_mime_by_path
                .get(&key)
                .map(String::as_str)
                .unwrap_or("application/octet-stream");
            writer
                .start_file(&key, entry_zip_options(&key, mt))
                .map_err(|e| DomainError::Storage(format!("epub start '{key}' error: {e}")))?;
            writer
                .write_all(bytes)
                .map_err(|e| DomainError::Storage(format!("epub write '{key}' error: {e}")))?;
            written.insert(key);
        }

        writer
            .finish()
            .map_err(|e| DomainError::Storage(format!("epub finalize error: {e}")))?;
        Ok(())
    };

    let backup_path = if opts.keep_source {
        let backup = unique_orig_epub_backup_path(epub_path);
        let parent = epub_path.parent().unwrap_or_else(|| Path::new("."));
        let staging = parent.join(format!(".xmedia_norm_{}.tmp", Uuid::now_v7()));
        if let Err(err) = write_epub_to(&staging) {
            let _ = std::fs::remove_file(&staging);
            return Err(err);
        }
        std::fs::rename(epub_path, &backup).map_err(|e| {
            let _ = std::fs::remove_file(&staging);
            DomainError::Storage(format!("备份原 EPUB 失败: {e}"))
        })?;
        if let Err(e) = std::fs::rename(&staging, epub_path) {
            // Rollback original file name
            let _ = std::fs::rename(&backup, epub_path);
            let _ = std::fs::remove_file(&staging);
            return Err(DomainError::Storage(format!("替换规范化 EPUB 失败: {e}")));
        }
        Some(backup)
    } else {
        atomic_write_via_temp(epub_path, write_epub_to)?;
        None
    };

    let after_bytes = std::fs::metadata(epub_path).map(|m| m.len()).unwrap_or(0);

    Ok(NormalizeBookOutcome {
        output_path: epub_path.to_path_buf(),
        backup_path,
        source_format: "epub".to_string(),
        before_bytes,
        after_bytes,
        chapter_count: chapter_count.max(1),
        compressed_images,
        removed_orphans,
        deleted_source: false,
    })
}

fn try_extract_pdf_pages_with_pdftotext(pdf_path: &Path) -> Option<Vec<String>> {
    let output = Command::new("pdftotext")
        .arg("-enc")
        .arg("UTF-8")
        .arg(pdf_path)
        .arg("-")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    let mut pages: Vec<String> = text.split('\u{000c}').map(|s| s.to_string()).collect();
    if pages.last().is_some_and(|s| s.trim().is_empty()) && pages.len() > 1 {
        pages.pop();
    }
    Some(pages)
}

fn extract_pdf_pages_with_lopdf(doc: &Document) -> Vec<String> {
    let page_map = doc.get_pages();
    let mut page_nums: Vec<u32> = page_map.keys().copied().collect();
    page_nums.sort_unstable();
    let mut out = Vec::with_capacity(page_nums.len());
    for num in page_nums {
        let text = doc.extract_text(&[num]).unwrap_or_default();
        out.push(text);
    }
    out
}

fn resolve_pdf_dest_page(
    doc: &Document,
    dest_obj: &Object,
    page_id_to_num: &HashMap<ObjectId, u32>,
) -> Option<u32> {
    let resolved = match dest_obj {
        Object::Reference(id) => doc.get_object(*id).ok()?,
        other => other,
    };
    match resolved {
        Object::Array(arr) => {
            if let Some(Object::Reference(pid)) = arr.first() {
                return page_id_to_num.get(pid).copied();
            }
            None
        }
        Object::Dictionary(d) => {
            if let Ok(inner) = d.get(b"D") {
                return resolve_pdf_dest_page(doc, inner, page_id_to_num);
            }
            None
        }
        _ => None,
    }
}

fn collect_pdf_outline_items(
    doc: &Document,
    first_id: ObjectId,
    level: u8,
    page_id_to_num: &HashMap<ObjectId, u32>,
    out: &mut Vec<(String, u8, u32)>,
    visited: &mut HashSet<ObjectId>,
) {
    let mut current = Some(first_id);
    while let Some(oid) = current {
        if !visited.insert(oid) || out.len() >= 1000 {
            break;
        }
        let Ok(dict) = doc.get_dictionary(oid) else {
            break;
        };

        let title = dict
            .get(b"Title")
            .ok()
            .and_then(|o| match o {
                Object::Reference(id) => doc.get_object(*id).ok(),
                other => Some(other),
            })
            .and_then(|o| decode_text_string(o).ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        let page_num = dict
            .get(b"Dest")
            .ok()
            .and_then(|d| resolve_pdf_dest_page(doc, d, page_id_to_num))
            .or_else(|| {
                dict.get(b"A")
                    .ok()
                    .and_then(|a| resolve_pdf_dest_page(doc, a, page_id_to_num))
            });

        if let (Some(t), Some(pn)) = (title, page_num) {
            out.push((t, level.min(1), pn));
        }

        if let Ok(Object::Reference(child_id)) = dict.get(b"First") {
            collect_pdf_outline_items(
                doc,
                *child_id,
                level.saturating_add(1),
                page_id_to_num,
                out,
                visited,
            );
        }

        current = match dict.get(b"Next") {
            Ok(Object::Reference(next_id)) => Some(*next_id),
            _ => None,
        };
    }
}

fn extract_pdf_outlines(doc: &Document) -> Vec<(String, u8, u32)> {
    let page_id_to_num: HashMap<ObjectId, u32> = doc
        .get_pages()
        .into_iter()
        .map(|(num, oid)| (oid, num))
        .collect();

    let Ok(catalog) = doc.catalog() else {
        return Vec::new();
    };
    let Ok(Object::Reference(outlines_id)) = catalog.get(b"Outlines") else {
        return Vec::new();
    };
    let Ok(outlines_dict) = doc.get_dictionary(*outlines_id) else {
        return Vec::new();
    };
    let Ok(Object::Reference(first_id)) = outlines_dict.get(b"First") else {
        return Vec::new();
    };

    let mut out = Vec::new();
    let mut visited = HashSet::new();
    collect_pdf_outline_items(doc, *first_id, 0, &page_id_to_num, &mut out, &mut visited);
    out
}

fn extract_pdf_stream_image(
    doc: &Document,
    stream_id: ObjectId,
) -> Option<(Vec<u8>, &'static str)> {
    let Ok(Object::Stream(stream)) = doc.get_object(stream_id) else {
        return None;
    };
    let dict = &stream.dict;
    let subtype = dict
        .get(b"Subtype")
        .ok()
        .and_then(|o| o.as_name().ok())
        .unwrap_or_default();
    if subtype != b"Image" {
        return None;
    }

    let width = dict
        .get(b"Width")
        .ok()
        .and_then(|o| o.as_i64().ok())
        .unwrap_or(0) as u32;
    let height = dict
        .get(b"Height")
        .ok()
        .and_then(|o| o.as_i64().ok())
        .unwrap_or(0) as u32;
    if width < 80 || height < 80 {
        return None;
    }

    let filter_name: Option<Vec<u8>> = match dict.get(b"Filter") {
        Ok(Object::Name(n)) => Some(n.clone()),
        Ok(Object::Array(arr)) => arr
            .last()
            .and_then(|o| o.as_name().ok())
            .map(|n| n.to_vec()),
        _ => None,
    };

    if filter_name.as_deref() == Some(b"DCTDecode") {
        return Some((stream.content.clone(), "image/jpeg"));
    }

    if filter_name.as_deref() == Some(b"FlateDecode") || filter_name.is_none() {
        let mut cloned = stream.clone();
        if cloned.decompress().is_ok() {
            let bpc = dict
                .get(b"BitsPerComponent")
                .ok()
                .and_then(|o| o.as_i64().ok())
                .unwrap_or(8);
            if bpc != 8 {
                return None;
            }
            let cs_name = dict.get(b"ColorSpace").ok().and_then(|o| match o {
                Object::Name(n) => Some(n.as_slice()),
                Object::Array(arr) => arr.first().and_then(|x| x.as_name().ok()),
                _ => None,
            });
            let pixel_count = (width as usize).checked_mul(height as usize)?;
            let dynamic = if cs_name == Some(b"DeviceGray") && cloned.content.len() >= pixel_count {
                let buf = ImageBuffer::<image::Luma<u8>, Vec<u8>>::from_raw(
                    width,
                    height,
                    cloned.content[..pixel_count].to_vec(),
                )?;
                DynamicImage::ImageLuma8(buf)
            } else if cloned.content.len() >= pixel_count.saturating_mul(3) {
                let rgb_len = pixel_count * 3;
                let buf = ImageBuffer::<image::Rgb<u8>, Vec<u8>>::from_raw(
                    width,
                    height,
                    cloned.content[..rgb_len].to_vec(),
                )?;
                DynamicImage::ImageRgb8(buf)
            } else {
                return None;
            };
            let mut cursor = Cursor::new(Vec::new());
            if dynamic.write_to(&mut cursor, ImageFormat::Png).is_ok() {
                return Some((cursor.into_inner(), "image/png"));
            }
        }
    }

    None
}

fn extract_largest_image_from_pdf_page(
    doc: &Document,
    page_id: ObjectId,
) -> Option<(Vec<u8>, &'static str)> {
    let Ok((res_opt, _)) = doc.get_page_resources(page_id) else {
        return None;
    };
    let res_dict = res_opt?;
    let xobj_obj = res_dict.get(b"XObject").ok()?;
    let xobj_dict = match xobj_obj {
        Object::Reference(id) => doc.get_dictionary(*id).ok()?,
        Object::Dictionary(d) => d,
        _ => return None,
    };

    let mut best: Option<(usize, Vec<u8>, &'static str)> = None;
    for (_, val) in xobj_dict.iter() {
        if let Object::Reference(sid) = val
            && let Some((bytes, mime)) = extract_pdf_stream_image(doc, *sid)
        {
            let score = bytes.len();
            if best.as_ref().is_none_or(|(bs, _, _)| score > *bs) {
                best = Some((score, bytes, mime));
            }
        }
    }
    best.map(|(_, bytes, mime)| (bytes, mime))
}

fn clean_pdf_page_text(raw: &str) -> String {
    let mut paragraphs: Vec<String> = Vec::new();
    let mut current = String::new();
    for line in raw.replace("\r\n", "\n").lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !current.is_empty() {
                paragraphs.push(std::mem::take(&mut current));
            }
            continue;
        }
        if current.is_empty() {
            current.push_str(trimmed);
        } else if current.ends_with('-')
            && trimmed
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_lowercase())
        {
            current.pop();
            current.push_str(trimmed);
        } else {
            let last_cjk = current
                .chars()
                .last()
                .is_some_and(|c| ('\u{4e00}'..='\u{9fff}').contains(&c));
            let next_cjk = trimmed
                .chars()
                .next()
                .is_some_and(|c| ('\u{4e00}'..='\u{9fff}').contains(&c));
            if !(last_cjk && next_cjk) {
                current.push(' ');
            }
            current.push_str(trimmed);
        }
    }
    if !current.is_empty() {
        paragraphs.push(current);
    }
    paragraphs.join("\n\n")
}

fn normalize_pdf_to_epub(
    pdf_path: &Path,
    opts: &NormalizeBookToEpubOptions,
) -> DomainResult<NormalizeBookOutcome> {
    let before_bytes = std::fs::metadata(pdf_path).map(|m| m.len()).unwrap_or(0);
    let epub_path = pdf_path.with_extension("epub");
    if epub_path.exists() {
        return Err(DomainError::Conflict(format!(
            "目标文件 '{}' 已存在，请先重命名或移除已有同名 EPUB",
            epub_path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        )));
    }

    let meta = read_book_meta_any(pdf_path)?;
    let doc = Document::load(pdf_path)
        .map_err(|e| DomainError::Validation(format!("解析 PDF 失败（可能已加密或损坏）: {e}")))?;

    let pages_map = doc.get_pages();
    let mut sorted_pages: Vec<(u32, ObjectId)> = pages_map.into_iter().collect();
    sorted_pages.sort_by_key(|(num, _)| *num);

    let (mut cover, mut compressed_images) = load_and_compress_sidecar_cover(pdf_path, opts);
    if cover.is_none()
        && let Some((_, first_page_id)) = sorted_pages.first()
        && let Some((raw_cov, raw_mime)) = extract_largest_image_from_pdf_page(&doc, *first_page_id)
    {
        let comp = compress_image_bytes(raw_cov, raw_mime, opts);
        if comp.changed {
            compressed_images += 1;
        }
        cover = Some((comp.bytes, comp.mime, comp.ext));
    }

    // Extract page texts (prefer `pdftotext` when installed on host, fallback to pure-Rust `lopdf`)
    let page_texts = try_extract_pdf_pages_with_pdftotext(pdf_path)
        .unwrap_or_else(|| extract_pdf_pages_with_lopdf(&doc));

    let total_non_ws: usize = page_texts
        .iter()
        .map(|p| p.chars().filter(|c| !c.is_whitespace()).count())
        .sum();
    let page_count = sorted_pages.len().max(1);
    let has_usable_text = total_non_ws >= 40 && (page_count == 1 || total_non_ws / page_count >= 8);

    let mut chapters: Vec<GeneratedChapter> = Vec::new();
    let mut extra_images: Vec<GeneratedImageAsset> = Vec::new();

    if has_usable_text {
        let outlines = extract_pdf_outlines(&doc);
        let distinct_outline_pages: HashSet<u32> = outlines.iter().map(|(_, _, p)| *p).collect();

        if outlines.len() >= 2 && distinct_outline_pages.len() >= 2 {
            // Group pages using PDF bookmarks/outlines
            let mut sorted_outlines = outlines;
            sorted_outlines.sort_by_key(|(_, _, p)| *p);
            for (idx, (title, level, start_page)) in sorted_outlines.iter().enumerate() {
                let start_idx = start_page.saturating_sub(1) as usize;
                let end_idx = sorted_outlines
                    .get(idx + 1)
                    .map(|(_, _, next_p)| next_p.saturating_sub(1) as usize)
                    .unwrap_or(page_texts.len())
                    .max(start_idx.saturating_add(1))
                    .min(page_texts.len());
                if start_idx >= page_texts.len() {
                    continue;
                }
                let joined = page_texts[start_idx..end_idx]
                    .iter()
                    .map(|p| clean_pdf_page_text(p))
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n\n");
                let body = plain_to_xhtml_body(title, &joined);
                chapters.push(GeneratedChapter {
                    title: title.clone(),
                    level: *level,
                    body_xhtml: body,
                });
            }
        }

        if chapters.is_empty() {
            let full_text = page_texts
                .iter()
                .map(|p| clean_pdf_page_text(p))
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("\n\n");
            let segmented = segment_txt_chapters(&full_text, None, None)?;
            chapters = segmented
                .into_iter()
                .map(|c| GeneratedChapter {
                    title: c.title,
                    level: c.level,
                    body_xhtml: c.xhtml_body,
                })
                .collect();
        }
    } else {
        if !opts.pdf_scanned_fallback {
            return Err(DomainError::Validation(
                "该 PDF 未提取到有效文字层（疑似扫描版），请开启「扫描版 PDF 自动转图页」后重试"
                    .into(),
            ));
        }

        for (idx, (_, page_id)) in sorted_pages.iter().enumerate() {
            if let Some((raw_img, raw_mime)) = extract_largest_image_from_pdf_page(&doc, *page_id) {
                let comp = compress_image_bytes(raw_img, raw_mime, opts);
                if comp.changed {
                    compressed_images += 1;
                }
                let rel_href = format!("Images/page_{:04}.{}", idx + 1, comp.ext);
                let page_title = format!("第 {} 页", idx + 1);
                let body = format!(
                    "  <div class=\"pdf-page\" style=\"text-align:center;\"><img src=\"../{rel_href}\" alt=\"{}\" style=\"max-width:100%;height:auto;\"/></div>",
                    escape(&page_title)
                );
                extra_images.push(GeneratedImageAsset {
                    rel_href,
                    media_type: comp.mime,
                    bytes: comp.bytes,
                });
                chapters.push(GeneratedChapter {
                    title: page_title,
                    level: 0,
                    body_xhtml: body,
                });
            }
        }

        if chapters.is_empty() {
            return Err(DomainError::Validation(
                "该 PDF 既无有效文字层也未提取到可转换的位图页面".into(),
            ));
        }
    }

    write_fresh_normalized_epub(
        &epub_path,
        &meta,
        &chapters,
        &extra_images,
        cover,
        DEFAULT_EPUB_CSS,
        opts.generate_jacket,
    )?;

    let after_bytes = std::fs::metadata(&epub_path).map(|m| m.len()).unwrap_or(0);
    let deleted_source = if !opts.keep_source {
        remove_source_and_stem_sidecars(pdf_path);
        true
    } else {
        false
    };

    Ok(NormalizeBookOutcome {
        output_path: epub_path,
        backup_path: None,
        source_format: "pdf".to_string(),
        before_bytes,
        after_bytes,
        chapter_count: chapters.len(),
        compressed_images,
        removed_orphans: 0,
        deleted_source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_png_bytes(w: u32, h: u32) -> Vec<u8> {
        let mut img = ImageBuffer::<image::Rgb<u8>, Vec<u8>>::new(w, h);
        for (x, y, px) in img.enumerate_pixels_mut() {
            *px = image::Rgb([
                ((x * 7) % 256) as u8,
                ((y * 11) % 256) as u8,
                (((x + y) * 5) % 256) as u8,
            ]);
        }
        let mut cursor = Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(img)
            .write_to(&mut cursor, ImageFormat::Png)
            .unwrap();
        cursor.into_inner()
    }

    fn write_sample_epub_with_png_and_junk(path: &Path) {
        let f = File::create(path).unwrap();
        let mut zw = ZipWriter::new(f);
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        zw.start_file("mimetype", stored).unwrap();
        zw.write_all(b"application/epub+zip").unwrap();
        zw.start_file("META-INF/container.xml", stored).unwrap();
        zw.write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#,
        )
        .unwrap();
        zw.start_file("OEBPS/content.opf", stored).unwrap();
        zw.write_all(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0" unique-identifier="bookid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>测试压缩书籍</dc:title>
    <dc:creator>测试作者</dc:creator>
    <dc:identifier id="bookid">urn:uuid:1234</dc:identifier>
  </metadata>
  <manifest>
    <item id="ch1" href="Text/ch1.xhtml" media-type="application/xhtml+xml"/>
    <item id="pic1" href="Images/pic1.png" media-type="image/png"/>
    <item id="orphan-img" href="Images/unused.png" media-type="image/png"/>
  </manifest>
  <spine>
    <itemref idref="ch1"/>
  </spine>
</package>"#
                .as_bytes(),
        )
        .unwrap();
        zw.start_file("OEBPS/Text/ch1.xhtml", stored).unwrap();
        zw.write_all(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml"><head><title>第一章</title></head><body><h2>第一章</h2><p>正文内容测试重复文本重复文本重复文本重复文本。</p><img src="../Images/pic1.png" alt="p"/></body></html>"#
                .as_bytes(),
        )
        .unwrap();
        let big_png = make_png_bytes(600, 800);
        zw.start_file("OEBPS/Images/pic1.png", stored).unwrap();
        zw.write_all(&big_png).unwrap();
        zw.start_file("OEBPS/Images/unused.png", stored).unwrap();
        zw.write_all(&big_png).unwrap();
        zw.start_file("__MACOSX/._junk", stored).unwrap();
        zw.write_all(b"junk").unwrap();
        zw.finish().unwrap();
    }

    #[test]
    fn normalize_txt_to_epub_keeps_or_deletes_source() {
        let tmp = tempfile::tempdir().unwrap();
        let txt_path = tmp.path().join("demo.txt");
        std::fs::write(
            &txt_path,
            "第一章 初遇\n这是一段测试正文内容。\n\n第二章 再见\n另一段正文内容。\n",
        )
        .unwrap();

        let opts = NormalizeBookToEpubOptions::default();
        assert!(opts.keep_source);
        assert_eq!(opts.image_format, BookImageCompressFormat::Webp);

        let res = normalize_book_to_epub(&txt_path, &opts).unwrap();
        assert!(txt_path.exists());
        assert!(res.output_path.exists());
        assert_eq!(res.chapter_count, 2);
        assert!(!res.deleted_source);

        // Verify valid EPUB 3.0 with Stored mimetype as entry #0
        let mut za = ZipArchive::new(File::open(&res.output_path).unwrap()).unwrap();
        let first = za.by_index(0).unwrap();
        assert_eq!(first.name().unwrap(), "mimetype");
        assert_eq!(first.compression(), CompressionMethod::Stored);
    }

    #[test]
    fn normalize_epub_backs_up_orig_cleans_orphans_and_compresses_to_webp() {
        let tmp = tempfile::tempdir().unwrap();
        let epub_path = tmp.path().join("novel.epub");
        write_sample_epub_with_png_and_junk(&epub_path);

        let opts = NormalizeBookToEpubOptions {
            keep_source: true,
            compress_images: true,
            image_format: BookImageCompressFormat::Webp,
            max_image_dimension: Some(400),
            image_quality: 82,
            generate_jacket: true,
            remove_orphans: true,
            pdf_scanned_fallback: true,
        };

        let res = normalize_book_to_epub(&epub_path, &opts).unwrap();
        assert!(
            res.backup_path
                .as_ref()
                .is_some_and(|p| p.ends_with("novel.orig.epub") && p.is_file())
        );
        assert!(res.after_bytes < res.before_bytes);
        assert!(res.removed_orphans >= 2);
        assert_eq!(res.compressed_images, 1);

        let mut za = ZipArchive::new(File::open(&epub_path).unwrap()).unwrap();
        let first = za.by_index(0).unwrap();
        assert_eq!(first.name().unwrap(), "mimetype");
        assert_eq!(first.compression(), CompressionMethod::Stored);
        drop(first);

        // Verify pic1.png was converted to pic1.webp and ch1.xhtml reference was rewritten
        assert!(za.by_name("OEBPS/Images/pic1.webp").is_ok());
        assert!(za.by_name("OEBPS/Images/unused.png").is_err());
        let mut ch1 = String::new();
        za.by_name("OEBPS/Text/ch1.xhtml")
            .unwrap()
            .read_to_string(&mut ch1)
            .unwrap();
        assert!(ch1.contains("pic1.webp"));
    }

    #[test]
    fn normalize_pdf_to_epub_extracts_text_and_respects_keep_source() {
        use lopdf::content::{Content, Operation};
        use lopdf::{Dictionary, Stream, dictionary};

        let tmp = tempfile::tempdir().unwrap();
        let pdf_path = tmp.path().join("sample.pdf");

        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let font_id = doc.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
        });
        let resources_id = doc.add_object(dictionary! {
            "Font" => dictionary! {
                "F1" => font_id,
            },
        });
        let content = Content {
            operations: vec![
                Operation::new("BT", vec![]),
                Operation::new("Tf", vec!["F1".into(), 14.into()]),
                Operation::new("Td", vec![50.into(), 720.into()]),
                Operation::new(
                    "Tj",
                    vec![Object::string_literal(
                        "Chapter 1 Getting Started with Rust and Normalized EPUB Generation.",
                    )],
                ),
                Operation::new("ET", vec![]),
            ],
        };
        let content_id = doc.add_object(Stream::new(Dictionary::new(), content.encode().unwrap()));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
            "Resources" => resources_id,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        });
        let pages = dictionary! {
            "Type" => "Pages",
            "Kids" => vec![page_id.into()],
            "Count" => 1,
        };
        doc.objects.insert(pages_id, Object::Dictionary(pages));
        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog_id);
        doc.save(&pdf_path).unwrap();

        let opts = NormalizeBookToEpubOptions::default();
        let res = normalize_book_to_epub(&pdf_path, &opts).unwrap();
        assert!(pdf_path.exists());
        assert!(res.output_path.exists());
        assert_eq!(res.source_format, "pdf");
        assert!(res.chapter_count >= 1);

        let mut za = ZipArchive::new(File::open(&res.output_path).unwrap()).unwrap();
        let mut ch1 = String::new();
        za.by_name("OEBPS/Text/chapter_0001.xhtml")
            .unwrap()
            .read_to_string(&mut ch1)
            .unwrap();
        assert!(ch1.contains("Normalized EPUB Generation"));
    }
}

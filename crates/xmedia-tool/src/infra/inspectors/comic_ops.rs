use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, BufWriter, Cursor, Read, Write};
use std::path::{Path, PathBuf};

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{GenericImageView, ImageFormat, ImageReader};
use quick_xml::Reader;
use quick_xml::events::Event;
use serde::{Deserialize, Serialize};
use xmedia_core::application::ports::BinaryPayload;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::infra::thumbnail::sniff_image_mime;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::domain::media::ComicFileMeta;
use crate::infra::atomic_fs::atomic_write_via_temp;
use crate::infra::inspectors::book::attr_value;
use crate::infra::inspectors::comic::{
    extract_existing_comic_info_bytes, is_image_entry, natural_sort_key, read_comic_meta,
    render_comic_info_xml_preserving, render_comic_info_xml_with_pages,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComicPageInfoDto {
    pub index: usize,
    pub entry_name: String,
    pub file_size: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// ComicInfo.xml PageType: "Story" | "FrontCover" | "InnerCover" | "BackCover" | "Advertisement" | "Deleted"
    pub page_type: String,
    pub double_page: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComicPagesSnapshotDto {
    pub format: String,
    pub total_pages: usize,
    pub total_size_bytes: u64,
    pub pages: Vec<ComicPageInfoDto>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SaveComicPageInput {
    pub entry_name: String,
    #[serde(default = "default_page_type")]
    pub page_type: String,
    #[serde(default)]
    pub double_page: bool,
}

fn default_page_type() -> String {
    "Story".to_string()
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ComicImageReencodeFormat {
    #[default]
    Keep,
    Jpeg,
    Webp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComicOptimizeReportDto {
    pub before_bytes: u64,
    pub after_bytes: u64,
    pub page_count: usize,
    pub reencoded_pages: usize,
    pub removed_junk_entries: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeComicsReportDto {
    pub output_rel_path: String,
    pub source_count: usize,
    pub total_pages: usize,
    pub output_bytes: u64,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct BatchComicSeriesSyncInput {
    pub series: Option<String>,
    pub series_group: Option<String>,
    pub writers: Option<Vec<String>>,
    pub pencillers: Option<Vec<String>>,
    pub publisher: Option<String>,
    pub genres: Option<Vec<String>>,
    pub tags: Option<Vec<String>>,
    pub reading_direction: Option<String>,
    /// Optional sequential numbering mode: "none" | "chapter" | "volume"
    #[serde(default)]
    pub numbering_mode: String,
    #[serde(default = "default_start_index")]
    pub start_index: i32,
    #[serde(default = "default_pad_width")]
    pub pad_width: usize,
}

fn default_start_index() -> i32 {
    1
}

fn default_pad_width() -> usize {
    2
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchComicSeriesSyncReportDto {
    pub updated_count: usize,
    pub failed_paths: Vec<String>,
}

#[derive(Debug, Clone, Default)]
struct ParsedPageAttr {
    page_type: Option<String>,
    double_page: Option<bool>,
    width: Option<u32>,
    height: Option<u32>,
}

fn parse_existing_pages_map(xml_bytes: &[u8]) -> HashMap<usize, ParsedPageAttr> {
    let mut map = HashMap::new();
    let xml_str = String::from_utf8_lossy(xml_bytes);
    let mut reader = Reader::from_str(&xml_str);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) => {
                let name = e.local_name().as_ref().to_ascii_lowercase();
                if name == "page"
                    && let Some(idx_str) = attr_value(&e, "Image")
                    && let Ok(idx) = idx_str.trim().parse::<usize>()
                {
                    let page_type = attr_value(&e, "Type").filter(|s| !s.trim().is_empty());
                    let double_page =
                        attr_value(&e, "DoublePage").map(|v| v.trim().eq_ignore_ascii_case("true"));
                    let width = attr_value(&e, "ImageWidth").and_then(|v| v.trim().parse().ok());
                    let height = attr_value(&e, "ImageHeight").and_then(|v| v.trim().parse().ok());
                    map.insert(
                        idx,
                        ParsedPageAttr {
                            page_type,
                            double_page,
                            width,
                            height,
                        },
                    );
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    map
}

fn probe_image_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    reader.into_dimensions().ok()
}

fn is_cbz_or_zip(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .map(|s| matches!(s.to_ascii_lowercase().as_str(), "cbz" | "zip"))
        .unwrap_or(false)
}

fn is_junk_zip_entry(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with('/') || lower.contains("__macosx") {
        return true;
    }
    let base = lower.rsplit('/').next().unwrap_or(&lower);
    base == ".ds_store" || base == "thumbs.db" || base == "desktop.ini" || base.starts_with("._")
}

pub fn list_comic_pages(path: &Path) -> DomainResult<ComicPagesSnapshotDto> {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("cbz")
        .to_ascii_lowercase();
    let total_size_bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);

    let file = File::open(path)
        .map_err(|e| DomainError::Storage(format!("open comic archive failed: {e}")))?;
    let mut archive = ZipArchive::new(BufReader::new(file))
        .map_err(|e| DomainError::Validation(format!("invalid comic zip/cbz archive: {e}")))?;

    let mut pages_attr_map: HashMap<usize, ParsedPageAttr> = HashMap::new();
    let mut image_entries: Vec<(String, u64)> = Vec::new();

    for idx in 0..archive.len() {
        let Ok(mut entry) = archive.by_index(idx) else {
            continue;
        };
        let Ok(name_raw) = entry.name() else {
            continue;
        };
        let name = name_raw.to_string();
        if name.eq_ignore_ascii_case("comicinfo.xml")
            || name.to_ascii_lowercase().ends_with("/comicinfo.xml")
        {
            let mut xml_buf = Vec::new();
            if entry.read_to_end(&mut xml_buf).is_ok() {
                pages_attr_map = parse_existing_pages_map(&xml_buf);
            }
        } else if is_image_entry(&name) {
            image_entries.push((name, entry.size()));
        }
    }

    image_entries.sort_by_key(|a| natural_sort_key(&a.0));

    let mut pages = Vec::with_capacity(image_entries.len());
    for (idx, (entry_name, file_size)) in image_entries.into_iter().enumerate() {
        let existing = pages_attr_map.get(&idx);
        let (width, height) = if let Some(ex) = existing
            && ex.width.is_some()
            && ex.height.is_some()
        {
            (ex.width, ex.height)
        } else if let Ok(mut entry) = archive.by_name(&entry_name) {
            // Read up to 64KB header first; fall back to full buffer only if needed
            let mut buf = Vec::with_capacity(file_size.min(128 * 1024) as usize);
            let _ = entry.read_to_end(&mut buf);
            match probe_image_dimensions(&buf) {
                Some((w, h)) => (Some(w), Some(h)),
                None => (None, None),
            }
        } else {
            (None, None)
        };

        let inferred_double = match (width, height) {
            (Some(w), Some(h)) if h > 0 => (w as f32) / (h as f32) >= 1.2,
            _ => false,
        };
        let double_page = existing
            .and_then(|e| e.double_page)
            .unwrap_or(inferred_double);

        let page_type = existing
            .and_then(|e| e.page_type.clone())
            .unwrap_or_else(|| {
                if idx == 0 {
                    "FrontCover".to_string()
                } else {
                    "Story".to_string()
                }
            });

        pages.push(ComicPageInfoDto {
            index: idx,
            entry_name,
            file_size,
            width,
            height,
            page_type,
            double_page,
        });
    }

    Ok(ComicPagesSnapshotDto {
        format: ext,
        total_pages: pages.len(),
        total_size_bytes,
        pages,
    })
}

pub fn read_comic_page_image(
    path: &Path,
    entry_name: &str,
    max_width: Option<u32>,
) -> DomainResult<BinaryPayload> {
    let file = File::open(path)
        .map_err(|e| DomainError::Storage(format!("open comic archive failed: {e}")))?;
    let mut archive = ZipArchive::new(BufReader::new(file))
        .map_err(|e| DomainError::Validation(format!("invalid comic zip/cbz archive: {e}")))?;

    const MAX_COMIC_PAGE_BYTES: u64 = 64 * 1024 * 1024;
    let entry = archive
        .by_name(entry_name)
        .map_err(|_| DomainError::NotFound(format!("comic page '{entry_name}' not found")))?;
    let mut bytes = Vec::with_capacity(entry.size().min(1024 * 1024) as usize);
    entry
        .take(MAX_COMIC_PAGE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| DomainError::Storage(format!("read comic page failed: {e}")))?;
    if (bytes.len() as u64) > MAX_COMIC_PAGE_BYTES {
        return Err(DomainError::Validation(format!(
            "comic page '{entry_name}' exceeds 64 MiB safety limit"
        )));
    }

    if let Some(target_w) = max_width
        && target_w >= 64
        && let Ok(img) = image::load_from_memory(&bytes)
    {
        let (w, h) = img.dimensions();
        if w > target_w {
            let target_h = ((h as u64 * target_w as u64) / (w as u64).max(1)).max(1) as u32;
            let resized = img.resize(target_w, target_h, FilterType::Triangle);
            let mut out = Vec::new();
            let mut encoder = JpegEncoder::new_with_quality(&mut out, 80);
            if encoder.encode_image(&resized.to_rgb8()).is_ok() {
                return Ok(BinaryPayload {
                    bytes: out,
                    mime_type: "image/jpeg".to_string(),
                });
            }
        }
    }

    let mime_type = sniff_image_mime(&bytes).to_string();
    Ok(BinaryPayload { bytes, mime_type })
}

fn maybe_reencode_page_image(
    raw_bytes: Vec<u8>,
    orig_ext: &str,
    reencode: ComicImageReencodeFormat,
    jpeg_quality: u8,
    max_width: Option<u32>,
) -> (Vec<u8>, String, bool, Option<(u32, u32)>) {
    let orig_dims = probe_image_dimensions(&raw_bytes);
    let needs_resize = max_width
        .zip(orig_dims)
        .is_some_and(|(mw, (w, _))| mw >= 320 && w > mw);

    if reencode == ComicImageReencodeFormat::Keep && !needs_resize {
        return (raw_bytes, orig_ext.to_string(), false, orig_dims);
    }

    let Ok(mut img) = image::load_from_memory(&raw_bytes) else {
        return (raw_bytes, orig_ext.to_string(), false, orig_dims);
    };

    if let Some(mw) = max_width
        && mw >= 320
        && img.width() > mw
    {
        let new_h = ((img.height() as u64 * mw as u64) / (img.width() as u64).max(1)).max(1) as u32;
        img = img.resize(mw, new_h, FilterType::Lanczos3);
    }
    let final_dims = Some(img.dimensions());

    match reencode {
        ComicImageReencodeFormat::Jpeg => {
            let q = jpeg_quality.clamp(50, 96);
            let mut out = Vec::new();
            let mut enc = JpegEncoder::new_with_quality(&mut out, q);
            if enc.encode_image(&img.to_rgb8()).is_ok()
                && (needs_resize || out.len() < raw_bytes.len())
            {
                return (out, "jpg".to_string(), true, final_dims);
            }
        }
        ComicImageReencodeFormat::Webp => {
            let mut cursor = Cursor::new(Vec::new());
            if img.write_to(&mut cursor, ImageFormat::WebP).is_ok() {
                let out = cursor.into_inner();
                if needs_resize || out.len() < raw_bytes.len() {
                    return (out, "webp".to_string(), true, final_dims);
                }
            }
        }
        ComicImageReencodeFormat::Keep => {
            if needs_resize {
                let fmt = match orig_ext {
                    "png" => ImageFormat::Png,
                    "webp" => ImageFormat::WebP,
                    _ => ImageFormat::Jpeg,
                };
                let mut cursor = Cursor::new(Vec::new());
                let ok = if fmt == ImageFormat::Jpeg {
                    let mut out = Vec::new();
                    let mut enc =
                        JpegEncoder::new_with_quality(&mut out, jpeg_quality.clamp(60, 95));
                    if enc.encode_image(&img.to_rgb8()).is_ok() {
                        return (out, "jpg".to_string(), true, final_dims);
                    }
                    false
                } else {
                    img.write_to(&mut cursor, fmt).is_ok()
                };
                if ok {
                    return (cursor.into_inner(), orig_ext.to_string(), true, final_dims);
                }
            }
        }
    }

    (raw_bytes, orig_ext.to_string(), false, orig_dims)
}

struct RenderedPageMeta<'a> {
    index: usize,
    page_type: &'a str,
    double_page: bool,
    size: u64,
    dims: Option<(u32, u32)>,
}

struct PreparedPagePayload {
    target_name: String,
    bytes: Vec<u8>,
    page_type: String,
    double_page: bool,
    dims: Option<(u32, u32)>,
}

fn render_pages_xml_block(entries: &[RenderedPageMeta<'_>]) -> String {
    let mut xml = String::from("  <Pages>\n");
    for e in entries {
        xml.push_str(&format!("    <Page Image=\"{}\"", e.index));
        if !e.page_type.is_empty() && e.page_type != "Story" {
            xml.push_str(&format!(" Type=\"{}\"", e.page_type));
        }
        if e.double_page {
            xml.push_str(" DoublePage=\"true\"");
        }
        xml.push_str(&format!(" ImageSize=\"{}\"", e.size));
        if let Some((w, h)) = e.dims {
            xml.push_str(&format!(" ImageWidth=\"{w}\" ImageHeight=\"{h}\""));
        }
        xml.push_str(" />\n");
    }
    xml.push_str("  </Pages>\n");
    xml
}

pub fn save_comic_pages(
    path: &Path,
    pages: &[SaveComicPageInput],
    reencode: ComicImageReencodeFormat,
    jpeg_quality: u8,
    max_width: Option<u32>,
) -> DomainResult<ComicOptimizeReportDto> {
    if !is_cbz_or_zip(path) {
        return Err(DomainError::Validation(
            "page management currently supports .cbz / .zip comic archives".into(),
        ));
    }
    if pages.is_empty() {
        return Err(DomainError::Validation(
            "at least one comic page must be kept".into(),
        ));
    }

    let before_bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let mut meta = read_comic_meta(path)?;
    let existing_comic_info = extract_existing_comic_info_bytes(path);

    let file = File::open(path)
        .map_err(|e| DomainError::Storage(format!("open comic archive failed: {e}")))?;
    let mut archive = ZipArchive::new(BufReader::new(file))
        .map_err(|e| DomainError::Validation(format!("invalid comic archive: {e}")))?;

    let mut page_payloads: Vec<PreparedPagePayload> = Vec::with_capacity(pages.len());
    let mut reencoded_pages = 0usize;

    const MAX_COMIC_PAGE_BYTES: u64 = 64 * 1024 * 1024;
    for (new_idx, spec) in pages.iter().enumerate() {
        let entry = archive.by_name(&spec.entry_name).map_err(|_| {
            DomainError::Validation(format!("page '{}' not found in archive", spec.entry_name))
        })?;
        let mut raw = Vec::with_capacity(entry.size().min(1024 * 1024) as usize);
        entry
            .take(MAX_COMIC_PAGE_BYTES + 1)
            .read_to_end(&mut raw)
            .map_err(|e| {
                DomainError::Storage(format!("read page '{}' failed: {e}", spec.entry_name))
            })?;
        if (raw.len() as u64) > MAX_COMIC_PAGE_BYTES {
            return Err(DomainError::Validation(format!(
                "comic page '{}' exceeds 64 MiB safety limit",
                spec.entry_name
            )));
        }

        let orig_ext = Path::new(&spec.entry_name)
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_else(|| "jpg".to_string());

        let (final_bytes, final_ext, did_reencode, dims) =
            maybe_reencode_page_image(raw, &orig_ext, reencode, jpeg_quality, max_width);
        if did_reencode {
            reencoded_pages += 1;
        }

        let target_name = if spec.page_type.eq_ignore_ascii_case("FrontCover") && new_idx == 0 {
            format!("000_cover.{final_ext}")
        } else {
            format!("{:04}.{final_ext}", new_idx + 1)
        };
        let sanitized_type = match spec.page_type.as_str() {
            "FrontCover" | "InnerCover" | "BackCover" | "Advertisement" | "Story" => {
                spec.page_type.clone()
            }
            _ => "Story".to_string(),
        };
        page_payloads.push(PreparedPagePayload {
            target_name,
            bytes: final_bytes,
            page_type: sanitized_type,
            double_page: spec.double_page,
            dims,
        });
    }

    let mut removed_junk_entries = 0usize;
    for idx in 0..archive.len() {
        if let Ok(entry) = archive.by_index(idx)
            && !entry.is_dir()
            && let Ok(name_raw) = entry.name()
            && is_junk_zip_entry(&name_raw)
        {
            removed_junk_entries += 1;
        }
    }
    drop(archive);

    meta.page_count = page_payloads.len() as u32;
    let xml_page_meta: Vec<RenderedPageMeta<'_>> = page_payloads
        .iter()
        .enumerate()
        .map(|(i, p)| RenderedPageMeta {
            index: i,
            page_type: p.page_type.as_str(),
            double_page: p.double_page,
            size: p.bytes.len() as u64,
            dims: p.dims,
        })
        .collect();
    let pages_xml = render_pages_xml_block(&xml_page_meta);
    let comic_info_xml =
        render_comic_info_xml_preserving(&meta, existing_comic_info.as_deref(), Some(&pages_xml));

    let reserved_names: std::collections::HashSet<String> = page_payloads
        .iter()
        .map(|p| p.target_name.to_ascii_lowercase())
        .chain(std::iter::once("comicinfo.xml".to_string()))
        .collect();

    atomic_write_via_temp(path, |tmp| {
        let src_file = File::open(path)
            .map_err(|e| DomainError::Storage(format!("reopen source cbz failed: {e}")))?;
        let mut src_archive = ZipArchive::new(BufReader::new(src_file))
            .map_err(|e| DomainError::Validation(format!("invalid comic archive: {e}")))?;
        let out = File::create(tmp)
            .map_err(|e| DomainError::Storage(format!("create temp cbz failed: {e}")))?;
        let mut writer = ZipWriter::new(BufWriter::new(out));
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

        let mut preserved_mimetype = false;
        if let Ok(mut mt_entry) = src_archive.by_name("mimetype") {
            let mut mt_bytes = Vec::new();
            if mt_entry.read_to_end(&mut mt_bytes).is_ok() {
                writer
                    .start_file("mimetype", stored)
                    .map_err(|e| DomainError::Storage(format!("write mimetype failed: {e}")))?;
                writer
                    .write_all(&mt_bytes)
                    .map_err(|e| DomainError::Storage(format!("write mimetype failed: {e}")))?;
                preserved_mimetype = true;
            }
        }

        writer
            .start_file("ComicInfo.xml", deflated)
            .map_err(|e| DomainError::Storage(format!("write ComicInfo.xml header failed: {e}")))?;
        writer
            .write_all(comic_info_xml.as_bytes())
            .map_err(|e| DomainError::Storage(format!("write ComicInfo.xml failed: {e}")))?;

        for p in &page_payloads {
            writer
                .start_file(&p.target_name, stored)
                .map_err(|e| DomainError::Storage(format!("write page header failed: {e}")))?;
            writer
                .write_all(&p.bytes)
                .map_err(|e| DomainError::Storage(format!("write page bytes failed: {e}")))?;
        }

        // Preserve non-image, non-ComicInfo, non-junk companion files inside the archive
        for idx in 0..src_archive.len() {
            let entry = src_archive
                .by_index(idx)
                .map_err(|e| DomainError::Storage(format!("read cbz entry failed: {e}")))?;
            let Ok(name_raw) = entry.name() else {
                continue;
            };
            let name = name_raw.to_string();
            let lower = name.to_ascii_lowercase();
            if entry.is_dir()
                || (preserved_mimetype && name == "mimetype")
                || is_image_entry(&name)
                || is_junk_zip_entry(&name)
                || lower == "comicinfo.xml"
                || lower.ends_with("/comicinfo.xml")
                || reserved_names.contains(&lower)
            {
                continue;
            }
            writer.raw_copy_file(entry).map_err(|e| {
                DomainError::Storage(format!("copy non-image cbz entry failed: {e}"))
            })?;
        }

        writer
            .finish()
            .map_err(|e| DomainError::Storage(format!("finish cbz failed: {e}")))?;
        Ok(())
    })?;

    let after_bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);

    Ok(ComicOptimizeReportDto {
        before_bytes,
        after_bytes,
        page_count: page_payloads.len(),
        reencoded_pages,
        removed_junk_entries,
    })
}

pub fn optimize_comic_cbz(
    path: &Path,
    reencode: ComicImageReencodeFormat,
    jpeg_quality: u8,
    max_width: Option<u32>,
) -> DomainResult<ComicOptimizeReportDto> {
    let snap = list_comic_pages(path)?;
    let inputs: Vec<SaveComicPageInput> = snap
        .pages
        .into_iter()
        .map(|p| SaveComicPageInput {
            entry_name: p.entry_name,
            page_type: p.page_type,
            double_page: p.double_page,
        })
        .collect();
    save_comic_pages(path, &inputs, reencode, jpeg_quality, max_width)
}

pub fn merge_comics_to_cbz(
    sources: &[PathBuf],
    output_path: &Path,
    output_rel_path: String,
    series: Option<String>,
    title: Option<String>,
    volume: Option<f64>,
) -> DomainResult<MergeComicsReportDto> {
    if sources.len() < 2 {
        return Err(DomainError::Validation(
            "merging requires at least two comic archives".into(),
        ));
    }
    if output_path.exists() {
        return Err(DomainError::Conflict(format!(
            "target archive '{}' already exists",
            output_rel_path
        )));
    }

    let first_meta = read_comic_meta(&sources[0]).unwrap_or_default();
    let mut merged_meta = ComicFileMeta {
        format: "cbz".to_string(),
        series: series
            .filter(|s| !s.trim().is_empty())
            .or(first_meta.series),
        series_group: first_meta.series_group,
        title: title.filter(|s| !s.trim().is_empty()).or(first_meta.title),
        number: None,
        volume: volume.or(first_meta.volume),
        summary: first_meta.summary,
        writers: first_meta.writers,
        pencillers: first_meta.pencillers,
        publisher: first_meta.publisher,
        genres: first_meta.genres,
        tags: first_meta.tags,
        year: first_meta.year,
        language_iso: first_meta.language_iso,
        reading_direction: if first_meta.reading_direction.is_empty() {
            "rtl".to_string()
        } else {
            first_meta.reading_direction
        },
        ..Default::default()
    };

    let mut all_pages: Vec<PreparedPagePayload> = Vec::new();
    for src in sources {
        let file = File::open(src)
            .map_err(|e| DomainError::Storage(format!("open source comic failed: {e}")))?;
        let mut archive = ZipArchive::new(BufReader::new(file))
            .map_err(|e| DomainError::Validation(format!("invalid source comic zip: {e}")))?;

        let mut names: Vec<String> = Vec::new();
        for idx in 0..archive.len() {
            if let Ok(entry) = archive.by_index(idx)
                && let Ok(name_raw) = entry.name()
                && is_image_entry(&name_raw)
            {
                names.push(name_raw.to_string());
            }
        }
        names.sort_by_key(|a| natural_sort_key(a));

        const MAX_COMIC_PAGE_BYTES: u64 = 64 * 1024 * 1024;
        for name in names {
            if let Ok(entry) = archive.by_name(&name) {
                let mut bytes = Vec::with_capacity(entry.size().min(1024 * 1024) as usize);
                if entry
                    .take(MAX_COMIC_PAGE_BYTES + 1)
                    .read_to_end(&mut bytes)
                    .is_ok()
                    && (bytes.len() as u64) <= MAX_COMIC_PAGE_BYTES
                {
                    let ext = Path::new(&name)
                        .extension()
                        .and_then(|s| s.to_str())
                        .map(|s| s.to_ascii_lowercase())
                        .unwrap_or_else(|| "jpg".to_string());
                    let dims = probe_image_dimensions(&bytes);
                    let dp = dims.is_some_and(|(w, h)| h > 0 && (w as f32) / (h as f32) >= 1.2);
                    let page_idx = all_pages.len() + 1;
                    let target_name = if page_idx == 1 {
                        format!("000_cover.{ext}")
                    } else {
                        format!("{page_idx:04}.{ext}")
                    };
                    all_pages.push(PreparedPagePayload {
                        target_name,
                        bytes,
                        page_type: if page_idx == 1 {
                            "FrontCover".to_string()
                        } else {
                            "Story".to_string()
                        },
                        double_page: dp,
                        dims,
                    });
                }
            }
        }
    }

    if all_pages.is_empty() {
        return Err(DomainError::Validation(
            "no image pages found in selected comic archives".into(),
        ));
    }

    merged_meta.page_count = all_pages.len() as u32;
    let xml_page_meta: Vec<RenderedPageMeta<'_>> = all_pages
        .iter()
        .enumerate()
        .map(|(i, p)| RenderedPageMeta {
            index: i,
            page_type: p.page_type.as_str(),
            double_page: p.double_page,
            size: p.bytes.len() as u64,
            dims: p.dims,
        })
        .collect();
    let pages_xml = render_pages_xml_block(&xml_page_meta);
    let comic_info_xml = render_comic_info_xml_with_pages(&merged_meta, Some(&pages_xml));

    atomic_write_via_temp(output_path, |tmp| {
        let out = File::create(tmp)
            .map_err(|e| DomainError::Storage(format!("create merged cbz failed: {e}")))?;
        let mut writer = ZipWriter::new(BufWriter::new(out));
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

        writer
            .start_file("ComicInfo.xml", deflated)
            .map_err(|e| DomainError::Storage(format!("write ComicInfo.xml header failed: {e}")))?;
        writer
            .write_all(comic_info_xml.as_bytes())
            .map_err(|e| DomainError::Storage(format!("write ComicInfo.xml failed: {e}")))?;

        for p in &all_pages {
            writer.start_file(&p.target_name, stored).map_err(|e| {
                DomainError::Storage(format!("write merged page header failed: {e}"))
            })?;
            writer.write_all(&p.bytes).map_err(|e| {
                DomainError::Storage(format!("write merged page bytes failed: {e}"))
            })?;
        }

        writer
            .finish()
            .map_err(|e| DomainError::Storage(format!("finish merged cbz failed: {e}")))?;
        Ok(())
    })?;

    let output_bytes = std::fs::metadata(output_path).map(|m| m.len()).unwrap_or(0);
    Ok(MergeComicsReportDto {
        output_rel_path,
        source_count: sources.len(),
        total_pages: all_pages.len(),
        output_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgb};

    fn make_png_bytes(w: u32, h: u32, color: [u8; 3]) -> Vec<u8> {
        let img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::from_fn(w, h, |_, _| Rgb(color));
        let mut cursor = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut cursor, ImageFormat::Png)
            .unwrap();
        cursor.into_inner()
    }

    fn create_sample_cbz(path: &Path, pages: &[(&str, Vec<u8>)]) {
        let f = File::create(path).unwrap();
        let mut zw = ZipWriter::new(f);
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        zw.start_file("__MACOSX/._junk", stored).unwrap();
        zw.write_all(b"junk").unwrap();
        zw.start_file("credits.txt", stored).unwrap();
        zw.write_all(b"Translation notes").unwrap();
        for (name, bytes) in pages {
            zw.start_file(*name, stored).unwrap();
            zw.write_all(bytes).unwrap();
        }
        zw.finish().unwrap();
    }

    #[test]
    fn comic_pages_list_reorder_delete_double_page_and_merge_work() {
        let tmp = tempfile::tempdir().unwrap();
        let cbz1 = tmp.path().join("vol1.cbz");
        let cbz2 = tmp.path().join("vol2.cbz");

        create_sample_cbz(
            &cbz1,
            &[
                ("p1_ad.png", make_png_bytes(400, 600, [255, 0, 0])),
                ("p2_cover.png", make_png_bytes(400, 600, [0, 255, 0])),
                ("p3_spread.png", make_png_bytes(900, 600, [0, 0, 255])),
            ],
        );
        create_sample_cbz(
            &cbz2,
            &[("p1.png", make_png_bytes(400, 600, [120, 120, 120]))],
        );

        let snap = list_comic_pages(&cbz1).unwrap();
        assert_eq!(snap.total_pages, 3);
        assert!(!snap.pages[0].double_page);
        assert!(snap.pages[2].double_page); // 900x600 detected as double page

        // Delete ad page (p1_ad.png), set p2_cover.png as FrontCover, keep spread
        let report = save_comic_pages(
            &cbz1,
            &[
                SaveComicPageInput {
                    entry_name: "p2_cover.png".into(),
                    page_type: "FrontCover".into(),
                    double_page: false,
                },
                SaveComicPageInput {
                    entry_name: "p3_spread.png".into(),
                    page_type: "Story".into(),
                    double_page: true,
                },
            ],
            ComicImageReencodeFormat::Jpeg,
            82,
            None,
        )
        .unwrap();
        assert_eq!(report.page_count, 2);
        assert_eq!(report.removed_junk_entries, 1);
        {
            let rf = File::open(&cbz1).unwrap();
            let mut za = ZipArchive::new(rf).unwrap();
            assert!(za.by_name("__MACOSX/._junk").is_err());
            let mut notes = String::new();
            za.by_name("credits.txt")
                .unwrap()
                .read_to_string(&mut notes)
                .unwrap();
            assert_eq!(notes, "Translation notes");
        }

        let snap2 = list_comic_pages(&cbz1).unwrap();
        assert_eq!(snap2.total_pages, 2);
        assert_eq!(snap2.pages[0].page_type, "FrontCover");
        assert!(snap2.pages[1].double_page);

        // Merge vol1 + vol2 into omnibus
        let merged = tmp.path().join("omnibus.cbz");
        let merge_rep = merge_comics_to_cbz(
            &[cbz1.clone(), cbz2.clone()],
            &merged,
            "omnibus.cbz".into(),
            Some("测试漫画".into()),
            Some("合订本".into()),
            Some(1.5),
        )
        .unwrap();
        assert_eq!(merge_rep.total_pages, 3);
        let merged_meta = read_comic_meta(&merged).unwrap();
        assert_eq!(merged_meta.series.as_deref(), Some("测试漫画"));
        assert_eq!(merged_meta.volume, Some(1.5));
        assert_eq!(merged_meta.page_count, 3);
    }
}

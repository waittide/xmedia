mod archive;
#[cfg(test)]
mod tests;

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use image::GenericImageView;
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use serde::{Deserialize, Serialize};
use xmedia_core::application::ports::BinaryPayload;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_formats::comic::probe_comic_pages;
use xmedia_formats::sniff_image_mime;
use zip::ZipArchive;

use crate::infra::inspectors::map_format_err;

pub use archive::{merge_comics_to_cbz, optimize_comic_cbz, save_comic_pages};

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

pub fn list_comic_pages(path: &Path) -> DomainResult<ComicPagesSnapshotDto> {
    let format = if path.is_dir() {
        "folder".to_string()
    } else {
        path.extension()
            .and_then(|s| s.to_str())
            .unwrap_or("cbz")
            .to_ascii_lowercase()
    };

    let probed = probe_comic_pages(path).map_err(map_format_err)?;
    let total_size_bytes = if path.is_dir() {
        probed.iter().map(|p| p.file_size).sum()
    } else {
        std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
    };

    let mut pages = Vec::with_capacity(probed.len());
    for p in probed {
        let inferred_double = match (p.width, p.height) {
            (Some(w), Some(h)) if h > 0 => (w as f32) / (h as f32) >= 1.2,
            _ => false,
        };
        let double_page = p.double_page.unwrap_or(inferred_double);
        let page_type = p.page_type.unwrap_or_else(|| {
            if p.index == 0 {
                "FrontCover".to_string()
            } else {
                "Story".to_string()
            }
        });
        pages.push(ComicPageInfoDto {
            index: p.index,
            entry_name: p.entry_name,
            file_size: p.file_size,
            width: p.width,
            height: p.height,
            page_type,
            double_page,
        });
    }

    Ok(ComicPagesSnapshotDto {
        format,
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
    const MAX_COMIC_PAGE_BYTES: u64 = 64 * 1024 * 1024;

    let bytes = if path.is_dir() {
        if entry_name.is_empty()
            || entry_name.contains('/')
            || entry_name.contains('\\')
            || entry_name.contains("..")
        {
            return Err(DomainError::Validation("invalid comic page name".into()));
        }
        let img_path = path.join(entry_name);
        if img_path.parent() != Some(path) || !img_path.is_file() {
            return Err(DomainError::NotFound(format!(
                "comic page '{entry_name}' not found"
            )));
        }
        let file = File::open(&img_path)
            .map_err(|e| DomainError::Storage(format!("open comic page failed: {e}")))?;
        let size = file.metadata().map(|m| m.len()).unwrap_or(0);
        let mut buf = Vec::with_capacity(size.min(1024 * 1024) as usize);
        file.take(MAX_COMIC_PAGE_BYTES + 1)
            .read_to_end(&mut buf)
            .map_err(|e| DomainError::Storage(format!("read comic page failed: {e}")))?;
        if (buf.len() as u64) > MAX_COMIC_PAGE_BYTES {
            return Err(DomainError::Validation(format!(
                "comic page '{entry_name}' exceeds 64 MiB safety limit"
            )));
        }
        buf
    } else {
        let file = File::open(path)
            .map_err(|e| DomainError::Storage(format!("open comic archive failed: {e}")))?;
        let mut archive = ZipArchive::new(BufReader::new(file))
            .map_err(|e| DomainError::Validation(format!("invalid comic zip/cbz archive: {e}")))?;
        let entry = archive
            .by_name(entry_name)
            .map_err(|_| DomainError::NotFound(format!("comic page '{entry_name}' not found")))?;
        let mut buf = Vec::with_capacity(entry.size().min(1024 * 1024) as usize);
        entry
            .take(MAX_COMIC_PAGE_BYTES + 1)
            .read_to_end(&mut buf)
            .map_err(|e| DomainError::Storage(format!("read comic page failed: {e}")))?;
        if (buf.len() as u64) > MAX_COMIC_PAGE_BYTES {
            return Err(DomainError::Validation(format!(
                "comic page '{entry_name}' exceeds 64 MiB safety limit"
            )));
        }
        buf
    };

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

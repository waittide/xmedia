mod archive;
#[cfg(test)]
mod tests;

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use image::GenericImageView;
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use xmedia_core::application::ports::BinaryPayload;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_formats::comic::inspect_comic_path;
use xmedia_formats::sniff_image_mime;
use zip::ZipArchive;

use crate::application::ops::{
    ComicImageReencodeFormat, ComicOptimizeReportDto, ComicPageInfoDto, ComicPagesSnapshotDto,
    ComicToolCommand, ComicToolOutput, MergeComicsReportDto, SaveComicPageInput,
};
use crate::infra::inspectors::map_format_err;

use archive::{merge_comics_to_cbz, optimize_comic_cbz, save_comic_pages};

pub fn run_comic_tool(cmd: ComicToolCommand) -> DomainResult<ComicToolOutput> {
    match cmd {
        ComicToolCommand::ListPages { path } => list_comic_pages(&path).map(ComicToolOutput::Pages),
        ComicToolCommand::ReadPageImage {
            path,
            entry_name,
            max_width,
        } => read_comic_page_image(&path, &entry_name, max_width).map(ComicToolOutput::Binary),
        ComicToolCommand::SavePages {
            path,
            pages,
            reencode,
            jpeg_quality,
            max_width,
        } => save_comic_pages(&path, &pages, reencode, jpeg_quality, max_width)
            .map(ComicToolOutput::Optimized),
        ComicToolCommand::OptimizeCbz {
            path,
            reencode,
            jpeg_quality,
            max_width,
        } => optimize_comic_cbz(&path, reencode, jpeg_quality, max_width)
            .map(ComicToolOutput::Optimized),
        ComicToolCommand::MergeToCbz {
            sources,
            target_abs,
            output_rel_path,
            series,
            title,
            volume,
        } => merge_comics_to_cbz(
            &sources,
            &target_abs,
            output_rel_path,
            series,
            title,
            volume,
        )
        .map(ComicToolOutput::Merged),
    }
}

fn list_comic_pages(path: &Path) -> DomainResult<ComicPagesSnapshotDto> {
    let format = if path.is_dir() {
        "folder".to_string()
    } else {
        path.extension()
            .and_then(|s| s.to_str())
            .unwrap_or("cbz")
            .to_ascii_lowercase()
    };

    let probed = inspect_comic_path(path).map_err(map_format_err)?.pages;
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

fn read_comic_page_image(
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

use std::fs::File;
use std::io::{BufReader, BufWriter, Cursor, Read, Write};
use std::path::{Path, PathBuf};

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{GenericImageView, ImageFormat};
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_formats::comic::{is_junk_archive_entry, scan_comic_image_dir};
use xmedia_formats::probe_image_dimensions_bytes;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use super::{
    ComicImageReencodeFormat, ComicOptimizeReportDto, MergeComicsReportDto, SaveComicPageInput,
    list_comic_pages,
};
use crate::domain::media::ComicFileMeta;
use crate::infra::atomic_fs::atomic_write_via_temp;
use crate::infra::inspectors::comic::{
    extract_existing_comic_info_bytes, is_image_entry, natural_page_entry_cmp, read_comic_meta,
    render_comic_info_xml_preserving, render_comic_info_xml_with_pages,
};

fn is_cbz_or_zip(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .map(|s| matches!(s.to_ascii_lowercase().as_str(), "cbz" | "zip"))
        .unwrap_or(false)
}

fn maybe_reencode_page_image(
    raw_bytes: Vec<u8>,
    orig_ext: &str,
    reencode: ComicImageReencodeFormat,
    jpeg_quality: u8,
    max_width: Option<u32>,
) -> (Vec<u8>, String, bool, Option<(u32, u32)>) {
    let orig_dims = probe_image_dimensions_bytes(&raw_bytes);
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
                if fmt == ImageFormat::Jpeg {
                    let mut out = Vec::new();
                    let mut enc =
                        JpegEncoder::new_with_quality(&mut out, jpeg_quality.clamp(60, 95));
                    if enc.encode_image(&img.to_rgb8()).is_ok() {
                        return (out, "jpg".to_string(), true, final_dims);
                    }
                } else {
                    let mut cursor = Cursor::new(Vec::new());
                    if img.write_to(&mut cursor, fmt).is_ok() {
                        return (cursor.into_inner(), orig_ext.to_string(), true, final_dims);
                    }
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

struct StreamedPageRecord {
    target_name: String,
    page_type: String,
    double_page: bool,
    size: u64,
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

    let mut page_count = 0usize;
    let mut reencoded_pages = 0usize;
    let mut removed_junk_entries = 0usize;

    atomic_write_via_temp(path, |tmp| {
        let src_file = File::open(path)
            .map_err(|e| DomainError::Storage(format!("open comic archive failed: {e}")))?;
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
            if mt_entry
                .by_ref()
                .take(1024)
                .read_to_end(&mut mt_bytes)
                .is_ok()
            {
                writer
                    .start_file("mimetype", stored)
                    .map_err(|e| DomainError::Storage(format!("write mimetype failed: {e}")))?;
                writer
                    .write_all(&mt_bytes)
                    .map_err(|e| DomainError::Storage(format!("write mimetype failed: {e}")))?;
                preserved_mimetype = true;
            }
        }

        let mut recorded_pages: Vec<StreamedPageRecord> = Vec::with_capacity(pages.len());
        let mut local_reencoded = 0usize;
        const MAX_COMIC_PAGE_BYTES: u64 = 64 * 1024 * 1024;
        const HEADER_PROBE_BYTES: u64 = 256 * 1024;

        for (new_idx, spec) in pages.iter().enumerate() {
            let orig_ext = Path::new(&spec.entry_name)
                .extension()
                .and_then(|s| s.to_str())
                .map(|s| s.to_ascii_lowercase())
                .unwrap_or_else(|| "jpg".to_string());
            let sanitized_type = match spec.page_type.as_str() {
                "FrontCover" | "InnerCover" | "BackCover" | "Advertisement" | "Story" => {
                    spec.page_type.clone()
                }
                _ => "Story".to_string(),
            };

            if reencode == ComicImageReencodeFormat::Keep && max_width.is_none() {
                let target_name =
                    if spec.page_type.eq_ignore_ascii_case("FrontCover") && new_idx == 0 {
                        format!("000_cover.{orig_ext}")
                    } else {
                        format!("{:04}.{orig_ext}", new_idx + 1)
                    };
                let mut entry = src_archive.by_name(&spec.entry_name).map_err(|_| {
                    DomainError::Validation(format!(
                        "page '{}' not found in archive",
                        spec.entry_name
                    ))
                })?;
                if entry.size() > MAX_COMIC_PAGE_BYTES {
                    return Err(DomainError::Validation(format!(
                        "comic page '{}' exceeds 64 MiB safety limit",
                        spec.entry_name
                    )));
                }
                let mut head = Vec::with_capacity(entry.size().min(64 * 1024) as usize);
                entry
                    .by_ref()
                    .take(HEADER_PROBE_BYTES)
                    .read_to_end(&mut head)
                    .map_err(|e| {
                        DomainError::Storage(format!("read page '{}' failed: {e}", spec.entry_name))
                    })?;
                let dims = probe_image_dimensions_bytes(&head);

                writer
                    .start_file(&target_name, stored)
                    .map_err(|e| DomainError::Storage(format!("write page header failed: {e}")))?;
                writer
                    .write_all(&head)
                    .map_err(|e| DomainError::Storage(format!("write page bytes failed: {e}")))?;
                let remaining_cap = MAX_COMIC_PAGE_BYTES + 1 - (head.len() as u64);
                let rest = std::io::copy(&mut entry.take(remaining_cap), &mut writer)
                    .map_err(|e| DomainError::Storage(format!("stream page bytes failed: {e}")))?;
                let written = (head.len() as u64) + rest;
                if written > MAX_COMIC_PAGE_BYTES {
                    return Err(DomainError::Validation(format!(
                        "comic page '{}' exceeds 64 MiB safety limit",
                        spec.entry_name
                    )));
                }
                recorded_pages.push(StreamedPageRecord {
                    target_name,
                    page_type: sanitized_type,
                    double_page: spec.double_page,
                    size: written,
                    dims,
                });
                continue;
            }

            let entry = src_archive.by_name(&spec.entry_name).map_err(|_| {
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

            let (final_bytes, final_ext, did_reencode, dims) =
                maybe_reencode_page_image(raw, &orig_ext, reencode, jpeg_quality, max_width);
            if did_reencode {
                local_reencoded += 1;
            }
            let target_name = if spec.page_type.eq_ignore_ascii_case("FrontCover") && new_idx == 0 {
                format!("000_cover.{final_ext}")
            } else {
                format!("{:04}.{final_ext}", new_idx + 1)
            };
            let byte_len = final_bytes.len() as u64;
            writer
                .start_file(&target_name, stored)
                .map_err(|e| DomainError::Storage(format!("write page header failed: {e}")))?;
            writer
                .write_all(&final_bytes)
                .map_err(|e| DomainError::Storage(format!("write page bytes failed: {e}")))?;
            drop(final_bytes);

            recorded_pages.push(StreamedPageRecord {
                target_name,
                page_type: sanitized_type,
                double_page: spec.double_page,
                size: byte_len,
                dims,
            });
        }

        let reserved_names: std::collections::HashSet<String> = recorded_pages
            .iter()
            .map(|p| p.target_name.to_ascii_lowercase())
            .chain(std::iter::once("comicinfo.xml".to_string()))
            .collect();

        let mut local_junk = 0usize;
        for idx in 0..src_archive.len() {
            let entry = src_archive
                .by_index(idx)
                .map_err(|e| DomainError::Storage(format!("read cbz entry failed: {e}")))?;
            let Ok(name_raw) = entry.name() else {
                continue;
            };
            let name = name_raw.to_string();
            if !entry.is_dir() && is_junk_archive_entry(&name) {
                local_junk += 1;
                continue;
            }
            let lower = name.to_ascii_lowercase();
            if entry.is_dir()
                || (preserved_mimetype && name == "mimetype")
                || is_image_entry(&name)
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

        meta.page_count = recorded_pages.len() as u32;
        let xml_page_meta: Vec<RenderedPageMeta<'_>> = recorded_pages
            .iter()
            .enumerate()
            .map(|(i, p)| RenderedPageMeta {
                index: i,
                page_type: p.page_type.as_str(),
                double_page: p.double_page,
                size: p.size,
                dims: p.dims,
            })
            .collect();
        let pages_xml = render_pages_xml_block(&xml_page_meta);
        let comic_info_xml = render_comic_info_xml_preserving(
            &meta,
            existing_comic_info.as_deref(),
            Some(&pages_xml),
        );

        writer
            .start_file("ComicInfo.xml", deflated)
            .map_err(|e| DomainError::Storage(format!("write ComicInfo.xml header failed: {e}")))?;
        writer
            .write_all(comic_info_xml.as_bytes())
            .map_err(|e| DomainError::Storage(format!("write ComicInfo.xml failed: {e}")))?;

        writer
            .finish()
            .map_err(|e| DomainError::Storage(format!("finish cbz failed: {e}")))?;

        page_count = recorded_pages.len();
        reencoded_pages = local_reencoded;
        removed_junk_entries = local_junk;
        Ok(())
    })?;

    let after_bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);

    Ok(ComicOptimizeReportDto {
        before_bytes,
        after_bytes,
        page_count,
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

fn stream_reader_into_merged_zip<R: Read, W: Write + std::io::Seek>(
    mut reader: R,
    size_hint: u64,
    ext: &str,
    label: &str,
    writer: &mut ZipWriter<W>,
    recorded_pages: &mut Vec<StreamedPageRecord>,
) -> DomainResult<()> {
    const MAX_COMIC_PAGE_BYTES: u64 = 64 * 1024 * 1024;
    const HEADER_PROBE_BYTES: u64 = 256 * 1024;
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);

    let mut head = Vec::with_capacity(size_hint.min(64 * 1024) as usize);
    reader
        .by_ref()
        .take(HEADER_PROBE_BYTES)
        .read_to_end(&mut head)
        .map_err(|e| DomainError::Storage(format!("read merged page '{label}' failed: {e}")))?;
    let dims = probe_image_dimensions_bytes(&head);
    let dp = dims.is_some_and(|(w, h)| h > 0 && (w as f32) / (h as f32) >= 1.2);
    let page_idx = recorded_pages.len() + 1;
    let target_name = if page_idx == 1 {
        format!("000_cover.{ext}")
    } else {
        format!("{page_idx:04}.{ext}")
    };

    writer
        .start_file(&target_name, stored)
        .map_err(|e| DomainError::Storage(format!("write merged page header failed: {e}")))?;
    writer
        .write_all(&head)
        .map_err(|e| DomainError::Storage(format!("write merged page bytes failed: {e}")))?;
    let remaining_cap = MAX_COMIC_PAGE_BYTES + 1 - (head.len() as u64);
    let rest = std::io::copy(&mut reader.take(remaining_cap), writer)
        .map_err(|e| DomainError::Storage(format!("stream merged page bytes failed: {e}")))?;
    let written = (head.len() as u64) + rest;
    if written > MAX_COMIC_PAGE_BYTES {
        return Err(DomainError::Validation(format!(
            "comic page '{label}' exceeds 64 MiB safety limit"
        )));
    }
    recorded_pages.push(StreamedPageRecord {
        target_name,
        page_type: if page_idx == 1 {
            "FrontCover".to_string()
        } else {
            "Story".to_string()
        },
        double_page: dp,
        size: written,
        dims,
    });
    Ok(())
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
            "merging requires at least two comic items".into(),
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

    let mut total_pages = 0usize;

    atomic_write_via_temp(output_path, |tmp| {
        let out = File::create(tmp)
            .map_err(|e| DomainError::Storage(format!("create merged cbz failed: {e}")))?;
        let mut writer = ZipWriter::new(BufWriter::new(out));
        let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        let mut recorded_pages: Vec<StreamedPageRecord> = Vec::new();
        const MAX_COMIC_PAGE_BYTES: u64 = 64 * 1024 * 1024;

        for src in sources {
            if src.is_dir() {
                if let Some(scan) = scan_comic_image_dir(src) {
                    for img_path in scan.image_files {
                        let Ok(f) = File::open(&img_path) else {
                            continue;
                        };
                        let sz = f.metadata().map(|m| m.len()).unwrap_or(0);
                        if sz > MAX_COMIC_PAGE_BYTES {
                            continue;
                        }
                        let ext = img_path
                            .extension()
                            .and_then(|s| s.to_str())
                            .map(|s| s.to_ascii_lowercase())
                            .unwrap_or_else(|| "jpg".to_string());
                        let label = img_path.to_string_lossy();
                        stream_reader_into_merged_zip(
                            BufReader::new(f),
                            sz,
                            &ext,
                            &label,
                            &mut writer,
                            &mut recorded_pages,
                        )?;
                    }
                }
                continue;
            }

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
            names.sort_by(|a, b| natural_page_entry_cmp(a, b));

            for name in names {
                let Ok(entry) = archive.by_name(&name) else {
                    continue;
                };
                if entry.size() > MAX_COMIC_PAGE_BYTES {
                    continue;
                }
                let sz = entry.size();
                let ext = Path::new(&name)
                    .extension()
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_ascii_lowercase())
                    .unwrap_or_else(|| "jpg".to_string());
                stream_reader_into_merged_zip(
                    entry,
                    sz,
                    &ext,
                    &name,
                    &mut writer,
                    &mut recorded_pages,
                )?;
            }
        }

        if recorded_pages.is_empty() {
            return Err(DomainError::Validation(
                "no image pages found in selected comic items".into(),
            ));
        }

        merged_meta.page_count = recorded_pages.len() as u32;
        let xml_page_meta: Vec<RenderedPageMeta<'_>> = recorded_pages
            .iter()
            .enumerate()
            .map(|(i, p)| RenderedPageMeta {
                index: i,
                page_type: p.page_type.as_str(),
                double_page: p.double_page,
                size: p.size,
                dims: p.dims,
            })
            .collect();
        let pages_xml = render_pages_xml_block(&xml_page_meta);
        let comic_info_xml = render_comic_info_xml_with_pages(&merged_meta, Some(&pages_xml));

        writer
            .start_file("ComicInfo.xml", deflated)
            .map_err(|e| DomainError::Storage(format!("write ComicInfo.xml header failed: {e}")))?;
        writer
            .write_all(comic_info_xml.as_bytes())
            .map_err(|e| DomainError::Storage(format!("write ComicInfo.xml failed: {e}")))?;

        writer
            .finish()
            .map_err(|e| DomainError::Storage(format!("finish merged cbz failed: {e}")))?;

        total_pages = recorded_pages.len();
        Ok(())
    })?;

    let output_bytes = std::fs::metadata(output_path).map(|m| m.len()).unwrap_or(0);
    Ok(MergeComicsReportDto {
        output_rel_path,
        source_count: sources.len(),
        total_pages,
        output_bytes,
    })
}

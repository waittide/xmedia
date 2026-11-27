use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;

use async_trait::async_trait;
use xmedia_core::application::ports::BinaryPayload;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;
use xmedia_formats::comic::{ComicFormatExt, scan_comic_image_dir};
use xmedia_formats::sniff_image_mime;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::application::ports::{ArtworkIo, MediaInspectorPort, QuickFileSummary};
use crate::domain::media::{
    ComicFileMeta, ComicMetaPatch, CompletenessFlags, MediaMetadataPatch, MediaMetadataPayload,
};
use crate::infra::atomic_fs::{atomic_write_bytes, atomic_write_via_temp};
use crate::infra::inspectors::video::write_image_replacing_opposite_ext;

use super::filename::{
    find_series_cover_path, guess_comic_details, is_image_entry, natural_page_entry_cmp,
};
use super::xml::{
    apply_epub_opf_fallback_into_comic, extract_existing_comic_info_bytes, parse_comic_info_into,
    render_comic_info_xml_preserving,
};

pub struct CbzComicInspector;

pub(crate) fn read_comic_meta(path: &Path) -> DomainResult<ComicFileMeta> {
    if path.is_dir() {
        let scan = scan_comic_image_dir(path).ok_or_else(|| {
            DomainError::Validation(format!(
                "'{}' is not a comic image directory",
                path.display()
            ))
        })?;
        let fallback_stem = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("Untitled")
            .to_string();
        let (guessed_series, guessed_number, guessed_volume, guessed_title) =
            guess_comic_details(path);
        let initial_title = guessed_title.or_else(|| {
            (guessed_series.is_none() && guessed_number.is_none() && guessed_volume.is_none())
                .then_some(fallback_stem)
        });
        let page_count = scan.image_files.len() as u32;
        let comic_info_bytes = scan
            .comic_info_path
            .as_ref()
            .and_then(|p| std::fs::read(p).ok());

        let mut meta = ComicFileMeta {
            format: "folder".to_string(),
            series: guessed_series,
            title: initial_title,
            number: guessed_number,
            volume: guessed_volume,
            reading_direction: "rtl".to_string(),
            page_count,
            has_comic_info_xml: comic_info_bytes.is_some(),
            has_cover: page_count > 0,
            has_series_cover: find_series_cover_path(path).is_some(),
            ..Default::default()
        };
        if let Some(ref xml) = comic_info_bytes {
            parse_comic_info_into(xml, &mut meta);
        }
        return Ok(meta);
    }

    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("cbz")
        .to_ascii_lowercase();
    let fallback_stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Untitled")
        .to_string();
    let (guessed_series, guessed_number, guessed_volume, guessed_title) = guess_comic_details(path);
    let initial_title = guessed_title.or_else(|| {
        (guessed_series.is_none() && guessed_number.is_none() && guessed_volume.is_none())
            .then_some(fallback_stem)
    });

    if let Some(fmt) = ComicFormatExt::from_path(path)
        && !fmt.is_zip_based()
    {
        return Ok(ComicFileMeta {
            format: fmt.as_str().to_string(),
            series: guessed_series,
            title: initial_title,
            number: guessed_number,
            volume: guessed_volume,
            reading_direction: "rtl".to_string(),
            page_count: 0,
            has_comic_info_xml: false,
            has_cover: false,
            has_series_cover: find_series_cover_path(path).is_some(),
            ..Default::default()
        });
    }

    let file = File::open(path)
        .map_err(|e| DomainError::Storage(format!("failed opening '{}': {e}", path.display())))?;
    let mut archive = ZipArchive::new(BufReader::new(file))
        .map_err(|e| DomainError::Validation(format!("invalid comic archive: {e}")))?;

    let mut page_count = 0u32;
    let mut comic_info_bytes: Option<Vec<u8>> = None;
    let mut opf_bytes: Option<Vec<u8>> = None;

    for idx in 0..archive.len() {
        let Ok(mut entry) = archive.by_index(idx) else {
            continue;
        };
        let Ok(name_raw) = entry.name() else {
            continue;
        };
        let name = name_raw.to_string();
        let lower = name.to_ascii_lowercase();
        if is_image_entry(&name) {
            page_count += 1;
        } else if lower == "comicinfo.xml" || lower.ends_with("/comicinfo.xml") {
            let mut buf = Vec::new();
            if entry.read_to_end(&mut buf).is_ok() {
                comic_info_bytes = Some(buf);
            }
        } else if ext == "epub" && opf_bytes.is_none() && lower.ends_with(".opf") {
            let mut buf = Vec::new();
            if entry.read_to_end(&mut buf).is_ok() {
                opf_bytes = Some(buf);
            }
        }
    }

    let mut meta = ComicFileMeta {
        format: ext,
        series: guessed_series,
        title: initial_title,
        number: guessed_number,
        volume: guessed_volume,
        reading_direction: "rtl".to_string(),
        page_count,
        has_comic_info_xml: comic_info_bytes.is_some(),
        has_cover: page_count > 0,
        has_series_cover: find_series_cover_path(path).is_some(),
        ..Default::default()
    };

    if let Some(ref opf) = opf_bytes {
        apply_epub_opf_fallback_into_comic(opf, &mut meta);
    }
    if let Some(ref xml) = comic_info_bytes {
        parse_comic_info_into(xml, &mut meta);
    }

    Ok(meta)
}

fn apply_comic_patch_fields(meta: &mut ComicFileMeta, cp: ComicMetaPatch) {
    meta.apply_patch(cp);
}

fn rewrite_zip_with_replacements(
    source_path: &Path,
    replacements: Vec<(String, Vec<u8>)>,
    skip_predicate: impl Fn(&str) -> bool,
) -> DomainResult<()> {
    atomic_write_via_temp(source_path, |staging| {
        let src_file = File::open(source_path)
            .map_err(|e| DomainError::Storage(format!("failed reading archive: {e}")))?;
        let mut src_archive = ZipArchive::new(BufReader::new(src_file))
            .map_err(|e| DomainError::Validation(format!("invalid zip archive: {e}")))?;
        let out_file = File::create(staging)
            .map_err(|e| DomainError::Storage(format!("failed creating staging zip: {e}")))?;
        let mut writer = ZipWriter::new(BufWriter::new(out_file));

        let mut preserved_mimetype = false;
        if let Ok(mut mt_entry) = src_archive.by_name("mimetype") {
            let mut mt_bytes = Vec::new();
            if mt_entry.read_to_end(&mut mt_bytes).is_ok() {
                let stored =
                    SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
                writer
                    .start_file("mimetype", stored)
                    .map_err(|e| DomainError::Storage(format!("zip mimetype start error: {e}")))?;
                writer
                    .write_all(&mt_bytes)
                    .map_err(|e| DomainError::Storage(format!("zip mimetype write error: {e}")))?;
                preserved_mimetype = true;
            }
        }

        for (rep_name, rep_data) in &replacements {
            let method = if is_image_entry(rep_name) {
                CompressionMethod::Stored
            } else {
                CompressionMethod::Deflated
            };
            let opts = SimpleFileOptions::default().compression_method(method);
            writer
                .start_file(rep_name.as_str(), opts)
                .map_err(|e| DomainError::Storage(format!("zip start_file error: {e}")))?;
            writer
                .write_all(rep_data)
                .map_err(|e| DomainError::Storage(format!("zip write error: {e}")))?;
        }

        for idx in 0..src_archive.len() {
            let entry = src_archive
                .by_index(idx)
                .map_err(|e| DomainError::Storage(format!("zip read entry error: {e}")))?;
            let Ok(name_ref) = entry.name() else {
                continue;
            };
            let name = name_ref.to_string();
            if entry.is_dir() || (preserved_mimetype && name == "mimetype") || skip_predicate(&name)
            {
                continue;
            }
            writer
                .raw_copy_file(entry)
                .map_err(|e| DomainError::Storage(format!("zip raw copy error: {e}")))?;
        }

        let buf_writer = writer
            .finish()
            .map_err(|e| DomainError::Storage(format!("zip finish error: {e}")))?;
        buf_writer
            .into_inner()
            .map_err(|e| DomainError::Storage(format!("zip flush error: {e}")))?;
        Ok(())
    })
}

#[async_trait]
impl MediaInspectorPort for CbzComicInspector {
    fn media_kind(&self) -> MediaKind {
        MediaKind::Comic
    }

    fn supports_path(&self, path: &Path) -> bool {
        if path.is_dir() {
            scan_comic_image_dir(path).is_some()
        } else {
            ComicFormatExt::from_path(path).is_some()
        }
    }

    async fn summarize_file(&self, path: &Path, fallback_stem: &str) -> QuickFileSummary {
        let p = path.to_path_buf();
        let stem = fallback_stem.to_string();
        tokio::task::spawn_blocking(move || match read_comic_meta(&p) {
            Ok(m) => {
                let title = m.title.clone().unwrap_or_else(|| stem.clone());
                let sub = match (&m.series, &m.number) {
                    (Some(s), Some(n)) => format!("{s} · #{n}"),
                    (Some(s), None) => s.clone(),
                    _ => format!("{}P", m.page_count),
                };
                QuickFileSummary {
                    display_title: title,
                    display_subtitle: sub,
                    year: m.year,
                    completeness: CompletenessFlags {
                        has_title: m.title.as_ref().is_some_and(|t| !t.trim().is_empty())
                            || m.series.as_ref().is_some_and(|s| !s.trim().is_empty()),
                        has_creator: !m.writers.is_empty() || !m.pencillers.is_empty(),
                        has_cover: m.has_cover,
                        has_summary_or_lyrics: m
                            .summary
                            .as_ref()
                            .is_some_and(|s| !s.trim().is_empty()),
                        has_nfo_or_tag_block: m.has_comic_info_xml,
                        is_normalized: false,
                        is_organized: false,
                    },
                    metadata: Some(MediaMetadataPayload::Comic(m)),
                }
            }
            Err(_) => QuickFileSummary {
                display_title: stem,
                display_subtitle: String::new(),
                year: None,
                completeness: CompletenessFlags::default(),
                metadata: None,
            },
        })
        .await
        .unwrap_or_else(|_| QuickFileSummary {
            display_title: fallback_stem.to_string(),
            display_subtitle: String::new(),
            year: None,
            completeness: CompletenessFlags::default(),
            metadata: None,
        })
    }

    async fn inspect_file(&self, path: &Path) -> DomainResult<MediaMetadataPayload> {
        let p = path.to_path_buf();
        let meta = tokio::task::spawn_blocking(move || read_comic_meta(&p))
            .await
            .map_err(|e| DomainError::Storage(format!("comic inspect task failed: {e}")))??;
        Ok(MediaMetadataPayload::Comic(meta))
    }

    async fn write_metadata(
        &self,
        path: &Path,
        patch: &MediaMetadataPatch,
    ) -> DomainResult<MediaMetadataPayload> {
        let MediaMetadataPatch::Comic(cp) = patch else {
            return Err(DomainError::Validation(
                "expected comic metadata patch".into(),
            ));
        };
        let p = path.to_path_buf();
        let cp_cloned: ComicMetaPatch = cp.clone();

        let updated = tokio::task::spawn_blocking(move || -> DomainResult<ComicFileMeta> {
            let mut meta = read_comic_meta(&p)?;
            apply_comic_patch_fields(&mut meta, cp_cloned);

            let existing_xml = extract_existing_comic_info_bytes(&p);
            let xml = render_comic_info_xml_preserving(&meta, existing_xml.as_deref(), None);

            if p.is_dir() {
                let target_ci = scan_comic_image_dir(&p)
                    .and_then(|s| s.comic_info_path)
                    .unwrap_or_else(|| p.join("ComicInfo.xml"));
                atomic_write_bytes(&target_ci, xml.as_bytes())?;
                return read_comic_meta(&p);
            }

            if ComicFormatExt::from_path(&p).is_some_and(|f| !f.is_zip_based()) {
                return Err(DomainError::UnsupportedFormat(
                    "writing ComicInfo.xml directly into CBR/CB7 archives is not supported".into(),
                ));
            }

            rewrite_zip_with_replacements(
                &p,
                vec![("ComicInfo.xml".to_string(), xml.into_bytes())],
                |entry_name| {
                    entry_name.eq_ignore_ascii_case("comicinfo.xml")
                        || entry_name.to_ascii_lowercase().ends_with("/comicinfo.xml")
                },
            )?;
            read_comic_meta(&p)
        })
        .await
        .map_err(|e| DomainError::Storage(format!("comic write task failed: {e}")))??;

        Ok(MediaMetadataPayload::Comic(updated))
    }

    async fn artwork(&self, path: &Path, op: ArtworkIo<'_>) -> DomainResult<Option<BinaryPayload>> {
        match op {
            ArtworkIo::Read { slot } => read_comic_artwork(path, slot).await,
            ArtworkIo::Write {
                slot,
                bytes,
                mime_type,
            } => {
                write_comic_artwork(path, bytes, mime_type, slot).await?;
                Ok(None)
            }
        }
    }
}

async fn read_comic_artwork(
    path: &Path,
    slot: Option<&str>,
) -> DomainResult<Option<BinaryPayload>> {
    let p = path.to_path_buf();
    let slot_owned = slot.map(str::to_ascii_lowercase);
    tokio::task::spawn_blocking(move || {
        if slot_owned.as_deref() == Some("series_cover")
            && let Some(cov_path) = find_series_cover_path(&p)
            && let Ok(bytes) = std::fs::read(&cov_path)
            && !bytes.is_empty()
        {
            let mime = sniff_image_mime(&bytes).to_string();
            return Ok(Some(BinaryPayload {
                bytes,
                mime_type: mime,
            }));
        }

        if p.is_dir() {
            let Some(scan) = scan_comic_image_dir(&p) else {
                return Ok(None);
            };
            let Some(cov_path) = scan.cover_path else {
                return Ok(None);
            };
            let bytes = std::fs::read(&cov_path)
                .map_err(|e| DomainError::Storage(format!("failed reading cover page: {e}")))?;
            let mime = sniff_image_mime(&bytes).to_string();
            return Ok(Some(BinaryPayload {
                bytes,
                mime_type: mime,
            }));
        }

        if ComicFormatExt::from_path(&p).is_some_and(|f| !f.is_zip_based()) {
            if let Some(cov_path) = find_series_cover_path(&p)
                && let Ok(bytes) = std::fs::read(&cov_path)
                && !bytes.is_empty()
            {
                let mime = sniff_image_mime(&bytes).to_string();
                return Ok(Some(BinaryPayload {
                    bytes,
                    mime_type: mime,
                }));
            }
            return Ok(None);
        }

        let file = File::open(&p)
            .map_err(|e| DomainError::Storage(format!("failed opening '{}': {e}", p.display())))?;
        let mut archive = ZipArchive::new(BufReader::new(file))
            .map_err(|e| DomainError::Validation(format!("invalid comic archive: {e}")))?;

        let mut image_names: Vec<String> = Vec::new();
        for idx in 0..archive.len() {
            if let Ok(entry) = archive.by_index(idx)
                && let Ok(name) = entry.name()
                && is_image_entry(&name)
            {
                image_names.push(name.into_owned());
            }
        }
        image_names.sort_by(|a, b| natural_page_entry_cmp(a, b));
        let Some(first_img) = image_names.first() else {
            return Ok(None);
        };
        let mut entry = archive
            .by_name(first_img)
            .map_err(|e| DomainError::Storage(format!("failed reading cover page: {e}")))?;
        let mut buf = Vec::new();
        entry
            .read_to_end(&mut buf)
            .map_err(|e| DomainError::Storage(format!("failed reading cover bytes: {e}")))?;
        let mime = sniff_image_mime(&buf).to_string();
        Ok(Some(BinaryPayload {
            bytes: buf,
            mime_type: mime,
        }))
    })
    .await
    .map_err(|e| DomainError::Storage(format!("comic cover read failed: {e}")))?
}

async fn write_comic_artwork(
    path: &Path,
    bytes: &[u8],
    mime_type: &str,
    slot: &str,
) -> DomainResult<()> {
    let p = path.to_path_buf();
    let raw = bytes.to_vec();
    let detected = if mime_type.trim().is_empty() {
        sniff_image_mime(&raw)
    } else {
        mime_type.trim()
    };
    let ext = if detected.contains("png") {
        "png"
    } else if detected.contains("webp") {
        "webp"
    } else {
        "jpg"
    };

    if slot.eq_ignore_ascii_case("series_cover") {
        let parent = p
            .parent()
            .ok_or_else(|| DomainError::Validation("comic entry has no parent directory".into()))?
            .to_path_buf();
        return tokio::task::spawn_blocking(move || {
            write_image_replacing_opposite_ext(&parent, "cover", ext, &raw)
        })
        .await
        .map_err(|e| DomainError::Storage(format!("comic series cover write failed: {e}")))?;
    }

    if p.is_dir() {
        return tokio::task::spawn_blocking(move || {
            write_image_replacing_opposite_ext(&p, "000_cover", ext, &raw)
        })
        .await
        .map_err(|e| DomainError::Storage(format!("comic folder cover write failed: {e}")))?;
    }

    let cover_entry = format!("000_cover.{ext}");
    tokio::task::spawn_blocking(move || {
        rewrite_zip_with_replacements(&p, vec![(cover_entry, raw)], |entry_name| {
            let lower = entry_name.to_ascii_lowercase();
            lower.starts_with("000_cover.")
        })
    })
    .await
    .map_err(|e| DomainError::Storage(format!("comic cover write failed: {e}")))?
}

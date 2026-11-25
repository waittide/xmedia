pub mod comicinfo;
pub mod comicinfo_write;
pub mod filename;

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use zip::ZipArchive;

use crate::common::probe_reader_dimensions;
use crate::error::{FormatError, FormatResult};

/// Canonical supported comic archive extensions across main server and `xmedia-tool`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ComicFormatExt {
    Cbz,
    Zip,
    Epub,
    Cbr,
    Cb7,
}

impl ComicFormatExt {
    pub const ALL_EXTENSIONS: &'static [&'static str] = &["cbz", "zip", "epub", "cbr", "cb7"];

    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext
            .trim()
            .trim_start_matches('.')
            .to_ascii_lowercase()
            .as_str()
        {
            "cbz" => Some(Self::Cbz),
            "zip" => Some(Self::Zip),
            "epub" => Some(Self::Epub),
            "cbr" | "rar" => Some(Self::Cbr),
            "cb7" | "7z" => Some(Self::Cb7),
            _ => None,
        }
    }

    pub fn from_path(path: &Path) -> Option<Self> {
        path.extension()
            .and_then(|s| s.to_str())
            .and_then(Self::from_extension)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cbz => "cbz",
            Self::Zip => "zip",
            Self::Epub => "epub",
            Self::Cbr => "cbr",
            Self::Cb7 => "cb7",
        }
    }

    pub fn mime_type(self) -> &'static str {
        match self {
            Self::Cbz => "application/vnd.comicbook+zip",
            Self::Zip => "application/zip",
            Self::Epub => "application/epub+zip",
            Self::Cbr => "application/vnd.comicbook-rar",
            Self::Cb7 => "application/x-cb7",
        }
    }

    pub fn is_zip_based(self) -> bool {
        matches!(self, Self::Cbz | Self::Zip | Self::Epub)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ComicReadingDirection {
    #[default]
    Rtl,
    Ltr,
    Webtoon,
}

impl ComicReadingDirection {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rtl => "rtl",
            Self::Ltr => "ltr",
            Self::Webtoon => "webtoon",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "rtl" => Some(Self::Rtl),
            "ltr" => Some(Self::Ltr),
            "webtoon" => Some(Self::Webtoon),
            _ => None,
        }
    }
}

/// Returns `true` when `name` is a comic page raster image (`jpg/jpeg/png/webp/gif/avif/bmp`).
pub fn is_comic_image_entry(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with('/') || lower.contains("__macosx") {
        return false;
    }
    let base = lower.rsplit('/').next().unwrap_or(&lower);
    if base.starts_with("._") {
        return false;
    }
    lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".png")
        || lower.ends_with(".webp")
        || lower.ends_with(".gif")
        || lower.ends_with(".avif")
        || lower.ends_with(".bmp")
}

/// Returns `true` for OS metadata clutter entries that should be skipped when re-packing archives.
pub fn is_junk_archive_entry(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with('/') || lower.contains("__macosx") {
        return true;
    }
    let base = lower.rsplit('/').next().unwrap_or(&lower);
    base == ".ds_store" || base == "thumbs.db" || base == "desktop.ini" || base.starts_with("._")
}

#[derive(Debug, Clone)]
pub struct ComicImageDirScan {
    pub image_files: Vec<PathBuf>,
    pub comic_info_path: Option<PathBuf>,
    pub cover_path: Option<PathBuf>,
}

/// Inspects a directory to determine whether it represents a standalone image-folder comic chapter
/// (contains sequential page images and no sub-chapters/books/videos), returning naturally sorted
/// page paths and optional `ComicInfo.xml`.
pub fn scan_comic_image_dir(dir: &Path) -> Option<ComicImageDirScan> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut images: Vec<(String, PathBuf)> = Vec::new();
    let mut comic_info_path: Option<PathBuf> = None;
    let mut has_child_archive_or_subdir_chapter = false;

    for entry in entries.flatten() {
        let path = entry.path();
        let Some(fname) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if fname.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            let sub_entries = std::fs::read_dir(&path).ok();
            if sub_entries.is_some_and(|mut it| {
                it.any(|child| {
                    child.ok().is_some_and(|ce| {
                        let cp = ce.path();
                        cp.is_file()
                            && (ComicFormatExt::from_path(&cp).is_some()
                                || cp
                                    .file_name()
                                    .and_then(|s| s.to_str())
                                    .is_some_and(is_comic_image_entry))
                    })
                })
            }) {
                has_child_archive_or_subdir_chapter = true;
            }
            continue;
        }
        if fname.eq_ignore_ascii_case("comicinfo.xml") {
            comic_info_path = Some(path);
            continue;
        }
        if ComicFormatExt::from_path(&path).is_some() {
            has_child_archive_or_subdir_chapter = true;
            continue;
        }
        if is_comic_image_entry(fname) {
            images.push((fname.to_string(), path));
        }
    }

    if has_child_archive_or_subdir_chapter || images.is_empty() {
        return None;
    }

    let non_artwork_pages = images
        .iter()
        .filter(|(name, _)| {
            let low = name.to_ascii_lowercase();
            !matches!(
                low.as_str(),
                "cover.jpg"
                    | "cover.jpeg"
                    | "cover.png"
                    | "cover.webp"
                    | "folder.jpg"
                    | "folder.jpeg"
                    | "folder.png"
                    | "folder.webp"
                    | "poster.jpg"
                    | "poster.png"
                    | "fanart.jpg"
                    | "fanart.png"
            )
        })
        .count();

    if comic_info_path.is_none() && non_artwork_pages < 2 {
        return None;
    }

    images.sort_by(|a, b| filename::natural_page_entry_cmp(&a.0, &b.0));
    let image_files: Vec<PathBuf> = images.into_iter().map(|(_, p)| p).collect();
    let cover_path = image_files.first().cloned();
    Some(ComicImageDirScan {
        image_files,
        comic_info_path,
        cover_path,
    })
}

pub fn is_comic_image_dir(dir: &Path) -> bool {
    scan_comic_image_dir(dir).is_some()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbedComicPage {
    pub index: usize,
    pub entry_name: String,
    pub file_size: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub page_type: Option<String>,
    pub double_page: Option<bool>,
}

/// Lists and probes page dimensions (header-only) inside a ZIP/CBZ archive or comic image directory.
pub fn probe_comic_pages(path: &Path) -> FormatResult<Vec<ProbedComicPage>> {
    if path.is_dir() {
        let scan = scan_comic_image_dir(path).ok_or_else(|| {
            FormatError::Validation(format!(
                "directory '{}' is not a comic image folder",
                path.display()
            ))
        })?;
        let pages_map: HashMap<usize, comicinfo::ComicInfoPageAttr> = scan
            .comic_info_path
            .as_ref()
            .and_then(|p| std::fs::read(p).ok())
            .map(|b| comicinfo::parse_comic_info_pages_map(&b))
            .unwrap_or_default();
        let mut out = Vec::with_capacity(scan.image_files.len());
        for (idx, img_path) in scan.image_files.into_iter().enumerate() {
            let file_size = std::fs::metadata(&img_path).map(|m| m.len()).unwrap_or(0);
            let entry_name = img_path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let existing = pages_map.get(&idx);
            let (width, height) = if let Some(ex) = existing
                && ex.width.is_some()
                && ex.height.is_some()
            {
                (ex.width, ex.height)
            } else if let Ok(file) = File::open(&img_path) {
                probe_reader_dimensions(BufReader::new(file), file_size)
                    .map_or((None, None), |(w, h)| (Some(w), Some(h)))
            } else {
                (None, None)
            };
            out.push(ProbedComicPage {
                index: idx,
                entry_name,
                file_size,
                width,
                height,
                page_type: existing.and_then(|e| e.page_type.clone()),
                double_page: existing.and_then(|e| e.double_page),
            });
        }
        return Ok(out);
    }

    let file =
        File::open(path).map_err(|e| FormatError::Io(format!("open comic archive failed: {e}")))?;
    let mut archive = ZipArchive::new(BufReader::new(file))
        .map_err(|e| FormatError::Validation(format!("invalid comic zip/cbz archive: {e}")))?;

    let mut pages_map: HashMap<usize, comicinfo::ComicInfoPageAttr> = HashMap::new();
    let mut image_entries: Vec<(String, u64)> = Vec::new();
    for idx in 0..archive.len() {
        let Ok(mut entry) = archive.by_index(idx) else {
            continue;
        };
        let Ok(name_raw) = entry.name() else {
            continue;
        };
        let name = name_raw.to_string();
        let lower = name.to_ascii_lowercase();
        if lower == "comicinfo.xml" || lower.ends_with("/comicinfo.xml") {
            let mut xml_buf = Vec::new();
            if entry.read_to_end(&mut xml_buf).is_ok() {
                pages_map = comicinfo::parse_comic_info_pages_map(&xml_buf);
            }
        } else if is_comic_image_entry(&name) {
            image_entries.push((name, entry.size()));
        }
    }

    image_entries.sort_by(|a, b| filename::natural_page_entry_cmp(&a.0, &b.0));

    let mut out = Vec::with_capacity(image_entries.len());
    for (idx, (entry_name, file_size)) in image_entries.into_iter().enumerate() {
        let existing = pages_map.get(&idx);
        let (width, height) = if let Some(ex) = existing
            && ex.width.is_some()
            && ex.height.is_some()
        {
            (ex.width, ex.height)
        } else if let Ok(entry) = archive.by_name(&entry_name) {
            probe_reader_dimensions(entry, file_size)
                .map_or((None, None), |(w, h)| (Some(w), Some(h)))
        } else {
            (None, None)
        };
        out.push(ProbedComicPage {
            index: idx,
            entry_name,
            file_size,
            width,
            height,
            page_type: existing.and_then(|e| e.page_type.clone()),
            double_page: existing.and_then(|e| e.double_page),
        });
    }
    Ok(out)
}

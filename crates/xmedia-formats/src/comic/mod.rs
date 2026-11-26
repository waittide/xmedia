pub mod comicinfo;
mod comicinfo_write;
pub mod filename;

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufReader, Read, Seek};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use zip::ZipArchive;

use crate::book::BookFormatExt;
use crate::book::opf::parse_opf_xml;
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

    fn from_extension(ext: &str) -> Option<Self> {
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
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "rtl" => Some(Self::Rtl),
            "ltr" => Some(Self::Ltr),
            "webtoon" => Some(Self::Webtoon),
            _ => None,
        }
    }
}

impl std::fmt::Display for ComicReadingDirection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Rtl => "rtl",
            Self::Ltr => "ltr",
            Self::Webtoon => "webtoon",
        };
        f.write_str(s)
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
        .filter(|(name, _)| !is_folder_artwork_filename(name))
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

fn is_folder_artwork_filename(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
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

fn build_probed_page(
    index: usize,
    entry_name: String,
    file_size: u64,
    existing: Option<&comicinfo::ComicInfoPageAttr>,
    probe_fallback: impl FnOnce() -> Option<(u32, u32)>,
) -> ProbedComicPage {
    let (width, height) = if let Some(ex) = existing
        && ex.width.is_some()
        && ex.height.is_some()
    {
        (ex.width, ex.height)
    } else {
        probe_fallback().map_or((None, None), |(w, h)| (Some(w), Some(h)))
    };
    ProbedComicPage {
        index,
        entry_name,
        file_size,
        width,
        height,
        page_type: existing.and_then(|e| e.page_type.clone()),
        double_page: existing.and_then(|e| e.double_page),
    }
}

#[derive(Debug, Clone)]
pub struct InspectedComicPackage {
    pub pages: Vec<ProbedComicPage>,
    pub comic_info: Option<comicinfo::ComicInfoMeta>,
}

fn ordered_epub_manifest_images(opf_bytes: &[u8], opf_path: &str) -> Vec<String> {
    let base_dir = opf_path
        .rsplit_once('/')
        .map(|(dir, _)| format!("{dir}/"))
        .unwrap_or_default();
    let opf = parse_opf_xml(opf_bytes, BookFormatExt::Epub, "");
    let mut seen = HashSet::new();
    let mut images = Vec::new();
    for item in opf.manifest {
        if item.media_type.starts_with("image/") || is_comic_image_entry(&item.href) {
            let full = format!("{base_dir}{}", item.href.trim_start_matches("./"));
            if seen.insert(full.clone()) {
                images.push(full);
            }
        }
    }
    images.sort_by(|a, b| filename::natural_page_entry_cmp(a, b));
    images
}

fn inspect_dir_package(dir: &Path) -> FormatResult<InspectedComicPackage> {
    let scan = scan_comic_image_dir(dir).ok_or_else(|| {
        FormatError::Validation(format!(
            "directory '{}' is not a comic image folder",
            dir.display()
        ))
    })?;
    let xml_bytes = scan
        .comic_info_path
        .as_ref()
        .and_then(|p| std::fs::read(p).ok());
    let pages_map = xml_bytes
        .as_deref()
        .map(comicinfo::parse_comic_info_pages_map)
        .unwrap_or_default();
    let comic_info = xml_bytes
        .as_deref()
        .and_then(comicinfo::parse_comic_info_xml);
    let pages = scan
        .image_files
        .into_iter()
        .enumerate()
        .map(|(idx, img_path)| {
            let file_size = std::fs::metadata(&img_path).map(|m| m.len()).unwrap_or(0);
            let entry_name = img_path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            build_probed_page(idx, entry_name, file_size, pages_map.get(&idx), || {
                File::open(&img_path)
                    .ok()
                    .and_then(|f| probe_reader_dimensions(BufReader::new(f), file_size))
            })
        })
        .collect();
    Ok(InspectedComicPackage { pages, comic_info })
}

/// Inspects pages (header-only dimensions) and metadata from a ZIP/CBZ/EPUB reader.
pub fn inspect_comic_reader<R: Read + Seek>(
    reader: R,
    is_epub: bool,
) -> FormatResult<InspectedComicPackage> {
    const MAX_ZIP_ENTRIES: usize = 65_536;
    const MAX_XML_ENTRY_BYTES: u64 = 4 * 1024 * 1024;

    let mut archive = ZipArchive::new(reader)
        .map_err(|e| FormatError::Validation(format!("invalid zip/cbz/epub archive: {e}")))?;
    if archive.len() > MAX_ZIP_ENTRIES {
        return Err(FormatError::Validation(format!(
            "comic archive exceeds maximum entry count ({MAX_ZIP_ENTRIES})"
        )));
    }

    let mut image_entries: Vec<(String, u64)> = Vec::new();
    let mut comic_info_entry: Option<String> = None;
    let mut opf_candidates: Vec<String> = Vec::new();

    for idx in 0..archive.len() {
        let Ok(entry) = archive.by_index_raw(idx) else {
            continue;
        };
        if entry.is_dir() {
            continue;
        }
        let Ok(name_raw) = entry.name() else {
            continue;
        };
        let name = name_raw.into_owned();
        let lower = name.to_ascii_lowercase();
        let base = lower.rsplit('/').next().unwrap_or(&lower);
        if base == "comicinfo.xml" {
            comic_info_entry = Some(name);
        } else if is_epub && lower.ends_with(".opf") {
            opf_candidates.push(name);
        } else if is_comic_image_entry(&name) {
            image_entries.push((name, entry.size()));
        }
    }

    let mut pages_map = HashMap::new();
    let mut comic_info: Option<comicinfo::ComicInfoMeta> = None;
    if let Some(ci_name) = comic_info_entry
        && let Ok(entry) = archive.by_name(&ci_name)
    {
        let mut xml = Vec::new();
        if entry
            .take(MAX_XML_ENTRY_BYTES + 1)
            .read_to_end(&mut xml)
            .is_ok()
            && (xml.len() as u64) <= MAX_XML_ENTRY_BYTES
        {
            pages_map = comicinfo::parse_comic_info_pages_map(&xml);
            comic_info = comicinfo::parse_comic_info_xml(&xml);
        }
    }

    let mut ordered_by_opf = false;
    if is_epub
        && let Some(opf_path) = opf_candidates.first()
        && let Ok(opf_entry) = archive.by_name(opf_path)
    {
        let mut opf_bytes = Vec::new();
        if opf_entry
            .take(MAX_XML_ENTRY_BYTES + 1)
            .read_to_end(&mut opf_bytes)
            .is_ok()
            && (opf_bytes.len() as u64) <= MAX_XML_ENTRY_BYTES
        {
            if comic_info.is_none() {
                let mut fallback = comicinfo::ComicInfoMeta::default();
                comicinfo::apply_epub_opf_into_comic_meta(&opf_bytes, &mut fallback);
                if fallback != comicinfo::ComicInfoMeta::default() {
                    comic_info = Some(fallback);
                }
            }
            let ordered_imgs = ordered_epub_manifest_images(&opf_bytes, opf_path);
            if !ordered_imgs.is_empty() {
                let sizes: HashMap<String, u64> = image_entries.iter().cloned().collect();
                let filtered: Vec<(String, u64)> = ordered_imgs
                    .into_iter()
                    .filter_map(|p| sizes.get(&p).map(|&sz| (p, sz)))
                    .collect();
                if !filtered.is_empty() {
                    image_entries = filtered;
                    ordered_by_opf = true;
                }
            }
        }
    }

    if !ordered_by_opf {
        image_entries.sort_by(|a, b| filename::natural_page_entry_cmp(&a.0, &b.0));
    }

    let pages = image_entries
        .into_iter()
        .enumerate()
        .map(|(idx, (entry_name, file_size))| {
            let existing = pages_map.get(&idx);
            let name_ref = entry_name.clone();
            build_probed_page(idx, entry_name, file_size, existing, || {
                archive
                    .by_name(&name_ref)
                    .ok()
                    .and_then(|entry| probe_reader_dimensions(entry, file_size))
            })
        })
        .collect();

    Ok(InspectedComicPackage { pages, comic_info })
}

/// Inspects pages and metadata from a comic archive file (`cbz/zip/epub`) or image directory.
pub fn inspect_comic_path(path: &Path) -> FormatResult<InspectedComicPackage> {
    if path.is_dir() {
        return inspect_dir_package(path);
    }
    let is_epub = ComicFormatExt::from_path(path) == Some(ComicFormatExt::Epub);
    let file =
        File::open(path).map_err(|e| FormatError::Io(format!("open comic archive failed: {e}")))?;
    inspect_comic_reader(BufReader::new(file), is_epub)
}

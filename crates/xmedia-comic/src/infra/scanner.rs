use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use xmedia_core::domain::error::{DomainError, DomainResult};

use crate::application::filename_parser::parse_chapter_filename;
use crate::application::ports::{
    ArchiveReaderPort, ComicScannerPort, ExistingChapterDigest, ScannedChapterEntry,
    ScannedSeriesEntry,
};
use crate::domain::chapter::ChapterSourceKind;
use crate::domain::natural::natural_cmp;
use crate::infra::archive::{
    fast_file_fingerprint, is_supported_archive_path, is_supported_image_name,
};

pub struct SmartComicScanner {
    archives: Arc<dyn ArchiveReaderPort>,
}

impl SmartComicScanner {
    pub fn new(archives: Arc<dyn ArchiveReaderPort>) -> Self {
        Self { archives }
    }
}

struct DiscoveredChapterCandidate {
    source_kind: ChapterSourceKind,
    path: PathBuf,
    display_name: String,
    fallback_index: u32,
    unchanged_hit: Option<ScannedChapterEntry>,
}

struct DiscoveredSeriesDir {
    title: String,
    source_dir: String,
    series_cover_bytes: Option<Vec<u8>>,
    candidates: Vec<DiscoveredChapterCandidate>,
}

struct DiscoveredLayout {
    series_dirs: Vec<DiscoveredSeriesDir>,
    standalone_archives: Vec<DiscoveredChapterCandidate>,
}

fn check_archive_unchanged(
    path: &Path,
    display_name: &str,
    fallback_index: u32,
    known_chapters: &HashMap<String, ExistingChapterDigest>,
) -> Option<ScannedChapterEntry> {
    let source_path = path.to_string_lossy().into_owned();
    let prev = known_chapters.get(&source_path)?;
    if prev.page_count == 0 {
        return None;
    }
    let (size, hash) = fast_file_fingerprint(path).ok()?;
    if size != prev.file_size || hash != prev.file_hash {
        return None;
    }
    if xmedia_core::infra::fingerprint::file_mtime_secs(path)
        .is_some_and(|m| m > prev.updated_at.as_second())
    {
        return None;
    }
    let parsed = parse_chapter_filename(display_name, fallback_index);
    Some(ScannedChapterEntry {
        kind: parsed.kind,
        number: parsed.number,
        volume_number: parsed.volume_number,
        title: parsed.clean_title,
        page_count: prev.page_count,
        source_kind: ChapterSourceKind::ArchiveFile,
        source_path,
        file_size: size,
        file_hash: hash,
        comic_info: None,
        unchanged: true,
        preserved_series_title: prev.series_title.clone(),
        preserved_series_source_dir: prev.series_source_dir.clone(),
    })
}

fn discover_comic_layout_blocking(
    root: &Path,
    known_chapters: &HashMap<String, ExistingChapterDigest>,
) -> DomainResult<DiscoveredLayout> {
    if !root.exists() {
        return Err(DomainError::Validation(format!(
            "library root directory '{}' does not exist",
            root.display()
        )));
    }

    let top_entries = read_sorted_dir(root)?;
    let mut series_dirs = Vec::new();
    let mut standalone_paths: Vec<PathBuf> = Vec::new();

    for entry in top_entries {
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }

        if ft.is_dir() {
            let mut archive_candidates: Vec<PathBuf> = Vec::new();
            let mut image_dir_candidates: Vec<PathBuf> = Vec::new();
            walk_series_tree(&path, 0, &mut archive_candidates, &mut image_dir_candidates)?;
            archive_candidates
                .sort_by(|a, b| natural_cmp(&a.to_string_lossy(), &b.to_string_lossy()));
            image_dir_candidates
                .sort_by(|a, b| natural_cmp(&a.to_string_lossy(), &b.to_string_lossy()));

            if archive_candidates.is_empty() && image_dir_candidates.is_empty() {
                continue;
            }

            let mut candidates =
                Vec::with_capacity(archive_candidates.len() + image_dir_candidates.len());
            let mut index = 1_u32;

            for archive_path in archive_candidates {
                let label = relative_display_name(&path, &archive_path);
                let unchanged_hit =
                    check_archive_unchanged(&archive_path, &label, index, known_chapters);
                candidates.push(DiscoveredChapterCandidate {
                    source_kind: ChapterSourceKind::ArchiveFile,
                    path: archive_path,
                    display_name: label,
                    fallback_index: index,
                    unchanged_hit,
                });
                index += 1;
            }

            for img_dir in image_dir_candidates {
                let label = if img_dir == path {
                    path.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "Chapter 1".into())
                } else {
                    relative_display_name(&path, &img_dir)
                };
                candidates.push(DiscoveredChapterCandidate {
                    source_kind: ChapterSourceKind::ImageDirectory,
                    path: img_dir,
                    display_name: label,
                    fallback_index: index,
                    unchanged_hit: None,
                });
                index += 1;
            }

            let series_cover_bytes = find_series_cover_bytes(&path);
            series_dirs.push(DiscoveredSeriesDir {
                title: name,
                source_dir: path.to_string_lossy().into_owned(),
                series_cover_bytes,
                candidates,
            });
        } else if ft.is_file() && is_supported_archive_path(&path) {
            standalone_paths.push(path);
        }
    }

    let mut standalone_archives = Vec::with_capacity(standalone_paths.len());
    for (idx, archive_path) in standalone_paths.into_iter().enumerate() {
        let filename = archive_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let fallback_index = (idx + 1) as u32;
        let unchanged_hit =
            check_archive_unchanged(&archive_path, &filename, fallback_index, known_chapters);
        standalone_archives.push(DiscoveredChapterCandidate {
            source_kind: ChapterSourceKind::ArchiveFile,
            path: archive_path,
            display_name: filename,
            fallback_index,
            unchanged_hit,
        });
    }

    Ok(DiscoveredLayout {
        series_dirs,
        standalone_archives,
    })
}

#[async_trait]
impl ComicScannerPort for SmartComicScanner {
    async fn scan_directory(
        &self,
        root_path: &str,
        known_chapters: &HashMap<String, ExistingChapterDigest>,
    ) -> DomainResult<Vec<ScannedSeriesEntry>> {
        let root = PathBuf::from(root_path);
        let root_for_task = root.clone();
        let known_owned = known_chapters.clone();
        let layout = tokio::task::spawn_blocking(move || {
            discover_comic_layout_blocking(&root_for_task, &known_owned)
        })
        .await
        .map_err(|e| DomainError::Storage(format!("comic directory scan panicked: {e}")))??;

        let mut series_map: HashMap<String, ScannedSeriesEntry> = HashMap::new();

        for dir in layout.series_dirs {
            let mut chapters = Vec::with_capacity(dir.candidates.len());
            for cand in dir.candidates {
                if let Some(ch) = self.resolve_candidate(cand, known_chapters).await {
                    chapters.push(ch);
                }
            }
            if !chapters.is_empty() {
                series_map.insert(
                    dir.source_dir.clone(),
                    ScannedSeriesEntry {
                        title: dir.title,
                        source_dir: dir.source_dir,
                        chapters,
                        series_cover_bytes: dir.series_cover_bytes,
                    },
                );
            }
        }

        for cand in layout.standalone_archives {
            let filename = cand.display_name.clone();
            let fallback_index = cand.fallback_index;
            if let Some(ch) = self.resolve_candidate(cand, known_chapters).await {
                let parsed = parse_chapter_filename(&filename, fallback_index);
                let (series_title, synthetic_dir) = if ch.unchanged
                    && let Some(prev_dir) = ch
                        .preserved_series_source_dir
                        .as_deref()
                        .filter(|s| !s.trim().is_empty())
                {
                    let title = ch
                        .preserved_series_title
                        .clone()
                        .filter(|s| !s.trim().is_empty())
                        .unwrap_or_else(|| parsed.clean_title.clone());
                    (title, prev_dir.to_owned())
                } else {
                    let title = ch
                        .comic_info
                        .as_ref()
                        .and_then(|ci| ci.series.clone())
                        .filter(|s| !s.trim().is_empty())
                        .or(parsed.inferred_series)
                        .unwrap_or_else(|| parsed.clean_title.clone());
                    let dir = format!("{}::{}", root.to_string_lossy(), title.trim());
                    (title, dir)
                };

                let entry =
                    series_map
                        .entry(synthetic_dir.clone())
                        .or_insert_with(|| ScannedSeriesEntry {
                            title: series_title,
                            source_dir: synthetic_dir,
                            chapters: Vec::new(),
                            series_cover_bytes: None,
                        });
                entry.chapters.push(ch);
            }
        }

        let mut results: Vec<ScannedSeriesEntry> = series_map.into_values().collect();
        results.sort_by(|a, b| natural_cmp(&a.title, &b.title));
        for s in &mut results {
            s.chapters.sort_by(|a, b| natural_cmp(&a.title, &b.title));
        }

        Ok(results)
    }
}

impl SmartComicScanner {
    async fn resolve_candidate(
        &self,
        cand: DiscoveredChapterCandidate,
        known_chapters: &HashMap<String, ExistingChapterDigest>,
    ) -> Option<ScannedChapterEntry> {
        if let Some(hit) = cand.unchanged_hit {
            return Some(hit);
        }

        let source_path = cand.path.to_string_lossy().into_owned();
        let parsed = parse_chapter_filename(&cand.display_name, cand.fallback_index);

        let inspection = self
            .archives
            .inspect_source(cand.source_kind, &source_path)
            .await
            .ok()?;
        if inspection.page_count == 0 {
            return None;
        }

        let unchanged = if let Some(prev) = known_chapters.get(&source_path)
            && prev.file_size == inspection.file_size
            && prev.file_hash == inspection.file_hash
            && prev.page_count == inspection.page_count
        {
            let p_owned = cand.path.clone();
            let updated_sec = prev.updated_at.as_second();
            tokio::task::spawn_blocking(move || {
                xmedia_core::infra::fingerprint::file_mtime_secs(&p_owned)
                    .is_none_or(|m| m <= updated_sec)
            })
            .await
            .unwrap_or(false)
        } else {
            false
        };

        let (preserved_series_title, preserved_series_source_dir) = known_chapters
            .get(&source_path)
            .map(|p| (p.series_title.clone(), p.series_source_dir.clone()))
            .unwrap_or((None, None));

        Some(ScannedChapterEntry {
            kind: parsed.kind,
            number: parsed.number,
            volume_number: parsed.volume_number,
            title: parsed.clean_title,
            page_count: inspection.page_count,
            source_kind: cand.source_kind,
            source_path,
            file_size: inspection.file_size,
            file_hash: inspection.file_hash,
            comic_info: inspection.comic_info,
            unchanged,
            preserved_series_title,
            preserved_series_source_dir,
        })
    }
}

fn find_series_cover_bytes(series_dir: &Path) -> Option<Vec<u8>> {
    const MAX_SERIES_COVER_BYTES: u64 = 32 * 1024 * 1024;
    const CANDIDATES: &[&str] = &[
        "cover.jpg",
        "cover.jpeg",
        "cover.png",
        "cover.webp",
        "folder.jpg",
        "folder.jpeg",
        "folder.png",
        "folder.webp",
    ];
    let canon_dir = fs::canonicalize(series_dir).ok()?;
    for name in CANDIDATES {
        let candidate = series_dir.join(name);
        let Ok(meta) = fs::symlink_metadata(&candidate) else {
            continue;
        };
        if meta.file_type().is_symlink() || !meta.is_file() || meta.len() > MAX_SERIES_COVER_BYTES {
            continue;
        }
        let Ok(canon_file) = fs::canonicalize(&candidate) else {
            continue;
        };
        if !canon_file.starts_with(&canon_dir) {
            continue;
        }
        if let Ok(bytes) = fs::read(&canon_file)
            && !bytes.is_empty()
        {
            return Some(bytes);
        }
    }
    None
}

fn walk_series_tree(
    dir: &Path,
    depth: usize,
    archives: &mut Vec<PathBuf>,
    image_dirs: &mut Vec<PathBuf>,
) -> DomainResult<()> {
    if depth > 4 {
        return Ok(());
    }
    let entries = read_sorted_dir(dir)?;
    let mut direct_images = 0_usize;
    let mut subdirs = Vec::new();

    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if ft.is_file() {
            if is_supported_archive_path(&path) {
                archives.push(path);
            } else if is_supported_image_name(&name)
                && !matches!(
                    name.to_ascii_lowercase().as_str(),
                    "cover.jpg"
                        | "cover.jpeg"
                        | "cover.png"
                        | "cover.webp"
                        | "folder.jpg"
                        | "folder.jpeg"
                        | "folder.png"
                        | "folder.webp"
                )
            {
                direct_images += 1;
            }
        } else if ft.is_dir() {
            subdirs.push(path);
        }
    }

    if direct_images > 0 && (depth > 0 || (subdirs.is_empty() && archives.is_empty())) {
        image_dirs.push(dir.to_path_buf());
    }

    for sub in subdirs {
        walk_series_tree(&sub, depth + 1, archives, image_dirs)?;
    }

    Ok(())
}

fn read_sorted_dir(dir: &Path) -> DomainResult<Vec<fs::DirEntry>> {
    let rd = fs::read_dir(dir).map_err(|e| {
        DomainError::Storage(format!("failed to read directory '{}': {e}", dir.display()))
    })?;
    let mut entries: Vec<fs::DirEntry> = rd.flatten().collect();
    entries.sort_by(|a, b| {
        natural_cmp(
            &a.file_name().to_string_lossy(),
            &b.file_name().to_string_lossy(),
        )
    });
    Ok(entries)
}

fn relative_display_name(series_root: &Path, item_path: &Path) -> String {
    item_path
        .strip_prefix(series_root)
        .unwrap_or(item_path)
        .to_string_lossy()
        .replace('/', " ")
}

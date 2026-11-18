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

#[async_trait]
impl ComicScannerPort for SmartComicScanner {
    async fn scan_directory(
        &self,
        root_path: &str,
        known_chapters: &HashMap<String, ExistingChapterDigest>,
    ) -> DomainResult<Vec<ScannedSeriesEntry>> {
        let root = PathBuf::from(root_path);
        if !root.exists() {
            return Err(DomainError::Validation(format!(
                "library root directory '{}' does not exist",
                root.display()
            )));
        }

        let top_entries = read_sorted_dir(&root)?;
        let mut series_map: HashMap<String, ScannedSeriesEntry> = HashMap::new();
        let mut standalone_archives: Vec<PathBuf> = Vec::new();

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
                let series_dir_str = path.to_string_lossy().into_owned();
                let chapters = self.collect_series_chapters(&path, known_chapters).await?;
                if !chapters.is_empty() {
                    series_map.insert(
                        series_dir_str.clone(),
                        ScannedSeriesEntry {
                            title: name,
                            source_dir: series_dir_str,
                            chapters,
                        },
                    );
                }
            } else if ft.is_file() && is_supported_archive_path(&path) {
                standalone_archives.push(path);
            }
        }

        for (idx, archive_path) in standalone_archives.into_iter().enumerate() {
            let filename = archive_path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if let Some(ch) = self
                .inspect_chapter_candidate(
                    ChapterSourceKind::ArchiveFile,
                    &archive_path,
                    &filename,
                    (idx + 1) as u32,
                    known_chapters,
                )
                .await
            {
                let parsed = parse_chapter_filename(&filename, (idx + 1) as u32);
                let series_title = ch
                    .comic_info
                    .as_ref()
                    .and_then(|ci| ci.series.clone())
                    .filter(|s| !s.trim().is_empty())
                    .or(parsed.inferred_series)
                    .unwrap_or_else(|| parsed.clean_title.clone());

                let synthetic_dir = format!("{}::{}", root.to_string_lossy(), series_title.trim());
                let entry =
                    series_map
                        .entry(synthetic_dir.clone())
                        .or_insert_with(|| ScannedSeriesEntry {
                            title: series_title,
                            source_dir: synthetic_dir,
                            chapters: Vec::new(),
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
    async fn collect_series_chapters(
        &self,
        series_dir: &Path,
        known_chapters: &HashMap<String, ExistingChapterDigest>,
    ) -> DomainResult<Vec<ScannedChapterEntry>> {
        let mut archive_candidates: Vec<PathBuf> = Vec::new();
        let mut image_dir_candidates: Vec<PathBuf> = Vec::new();
        walk_series_tree(
            series_dir,
            0,
            &mut archive_candidates,
            &mut image_dir_candidates,
        )?;

        archive_candidates.sort_by(|a, b| natural_cmp(&a.to_string_lossy(), &b.to_string_lossy()));
        image_dir_candidates
            .sort_by(|a, b| natural_cmp(&a.to_string_lossy(), &b.to_string_lossy()));

        let mut chapters = Vec::new();
        let mut index = 1_u32;

        for archive_path in archive_candidates {
            let label = relative_display_name(series_dir, &archive_path);
            if let Some(ch) = self
                .inspect_chapter_candidate(
                    ChapterSourceKind::ArchiveFile,
                    &archive_path,
                    &label,
                    index,
                    known_chapters,
                )
                .await
            {
                chapters.push(ch);
                index += 1;
            }
        }

        for img_dir in image_dir_candidates {
            let label = if img_dir == series_dir {
                series_dir
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Chapter 1".into())
            } else {
                relative_display_name(series_dir, &img_dir)
            };
            if let Some(ch) = self
                .inspect_chapter_candidate(
                    ChapterSourceKind::ImageDirectory,
                    &img_dir,
                    &label,
                    index,
                    known_chapters,
                )
                .await
            {
                chapters.push(ch);
                index += 1;
            }
        }

        Ok(chapters)
    }

    async fn inspect_chapter_candidate(
        &self,
        source_kind: ChapterSourceKind,
        path: &Path,
        display_name: &str,
        fallback_index: u32,
        known_chapters: &HashMap<String, ExistingChapterDigest>,
    ) -> Option<ScannedChapterEntry> {
        let source_path = path.to_string_lossy().into_owned();
        let parsed = parse_chapter_filename(display_name, fallback_index);

        if source_kind == ChapterSourceKind::ArchiveFile
            && let Some(prev) = known_chapters.get(&source_path)
            && let Ok((size, hash)) = fast_file_fingerprint(path)
            && size == prev.file_size
            && hash == prev.file_hash
            && prev.page_count > 0
            && xmedia_core::infra::fingerprint::file_mtime_secs(path)
                .is_none_or(|m| m <= prev.updated_at.as_second())
        {
            return Some(ScannedChapterEntry {
                kind: parsed.kind,
                number: parsed.number,
                volume_number: parsed.volume_number,
                title: parsed.clean_title,
                page_count: prev.page_count,
                source_kind,
                source_path,
                file_size: size,
                file_hash: hash,
                comic_info: None,
                unchanged: true,
            });
        }

        let inspection = self
            .archives
            .inspect_source(source_kind, &source_path)
            .await
            .ok()?;
        if inspection.page_count == 0 {
            return None;
        }

        let unchanged = known_chapters.get(&source_path).is_some_and(|prev| {
            prev.file_size == inspection.file_size
                && prev.file_hash == inspection.file_hash
                && prev.page_count == inspection.page_count
                && xmedia_core::infra::fingerprint::file_mtime_secs(path)
                    .is_none_or(|m| m <= prev.updated_at.as_second())
        });

        Some(ScannedChapterEntry {
            kind: parsed.kind,
            number: parsed.number,
            volume_number: parsed.volume_number,
            title: parsed.clean_title,
            page_count: inspection.page_count,
            source_kind,
            source_path,
            file_size: inspection.file_size,
            file_hash: inspection.file_hash,
            comic_info: inspection.comic_info,
            unchanged,
        })
    }
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
                    "cover.jpg" | "cover.jpeg" | "cover.png" | "cover.webp" | "folder.jpg"
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

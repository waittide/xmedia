use std::path::{Path, PathBuf};

use xmedia_core::domain::error::DomainResult;

use super::filename::{dir_has_single_video, is_season_dir};
use crate::infra::atomic_fs::atomic_write_bytes;

fn allow_dir_level_artwork(parent: &Path) -> bool {
    is_season_dir(parent) || parent.join("tvshow.nfo").is_file() || dir_has_single_video(parent)
}

pub(super) fn find_thumb_path(video_path: &Path) -> Option<PathBuf> {
    let parent = video_path.parent()?;
    let stem = video_path.file_stem()?.to_str()?;
    ["jpg", "jpeg", "png", "webp"]
        .into_iter()
        .map(|ext| parent.join(format!("{stem}-thumb.{ext}")))
        .find(|p| p.is_file())
}

pub(super) fn find_poster_path(video_path: &Path) -> Option<PathBuf> {
    let parent = video_path.parent()?;
    let stem = video_path.file_stem()?.to_str()?;
    let mut candidates = vec![
        parent.join(format!("{stem}-poster.jpg")),
        parent.join(format!("{stem}-poster.jpeg")),
        parent.join(format!("{stem}-poster.png")),
        parent.join(format!("{stem}-poster.webp")),
    ];
    if allow_dir_level_artwork(parent) {
        candidates.extend([
            parent.join("poster.jpg"),
            parent.join("poster.jpeg"),
            parent.join("poster.png"),
            parent.join("poster.webp"),
            parent.join("folder.jpg"),
            parent.join("folder.png"),
            parent.join("folder.webp"),
        ]);
    }
    if is_season_dir(parent)
        && let Some(show_dir) = parent.parent()
    {
        candidates.extend([
            show_dir.join("poster.jpg"),
            show_dir.join("poster.jpeg"),
            show_dir.join("poster.png"),
            show_dir.join("poster.webp"),
            show_dir.join("folder.jpg"),
            show_dir.join("folder.png"),
            show_dir.join("folder.webp"),
        ]);
    }
    candidates.into_iter().find(|p| p.is_file())
}

pub(super) fn find_fanart_path(video_path: &Path) -> Option<PathBuf> {
    let parent = video_path.parent()?;
    let stem = video_path.file_stem()?.to_str()?;
    let mut candidates = vec![
        parent.join(format!("{stem}-fanart.jpg")),
        parent.join(format!("{stem}-fanart.jpeg")),
        parent.join(format!("{stem}-fanart.png")),
        parent.join(format!("{stem}-fanart.webp")),
    ];
    if allow_dir_level_artwork(parent) {
        candidates.extend([
            parent.join("fanart.jpg"),
            parent.join("fanart.jpeg"),
            parent.join("fanart.png"),
            parent.join("fanart.webp"),
            parent.join("backdrop.jpg"),
            parent.join("backdrop.jpeg"),
            parent.join("backdrop.png"),
            parent.join("backdrop.webp"),
        ]);
    }
    if is_season_dir(parent)
        && let Some(show_dir) = parent.parent()
    {
        candidates.extend([
            show_dir.join("fanart.jpg"),
            show_dir.join("fanart.jpeg"),
            show_dir.join("fanart.png"),
            show_dir.join("fanart.webp"),
            show_dir.join("backdrop.jpg"),
            show_dir.join("backdrop.jpeg"),
            show_dir.join("backdrop.png"),
            show_dir.join("backdrop.webp"),
        ]);
    }
    candidates.into_iter().find(|p| p.is_file())
}

pub(super) fn find_clearlogo_path(video_path: &Path) -> Option<PathBuf> {
    let parent = video_path.parent()?;
    let stem = video_path.file_stem()?.to_str()?;
    for ext in ["png", "webp", "jpg"] {
        let p = parent.join(format!("{stem}-clearlogo.{ext}"));
        if p.is_file() {
            return Some(p);
        }
    }
    if allow_dir_level_artwork(parent) {
        for ext in ["png", "webp", "jpg"] {
            let p = parent.join(format!("clearlogo.{ext}"));
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// Atomically writes `{stem_base}.{ext}` inside `dir` and removes any stale sibling with other
/// image extensions (`.jpg`/`.jpeg`/`.png`/`.webp`) so old artwork never shadows the updated file.
pub(crate) fn write_image_replacing_opposite_ext(
    dir: &Path,
    stem_base: &str,
    ext: &str,
    bytes: &[u8],
) -> DomainResult<()> {
    let target = dir.join(format!("{stem_base}.{ext}"));
    atomic_write_bytes(&target, bytes)?;
    for stale_ext in ["jpg", "jpeg", "png", "webp"] {
        if stale_ext != ext {
            let stale_path = dir.join(format!("{stem_base}.{stale_ext}"));
            if stale_path.is_file() {
                let _ = std::fs::remove_file(stale_path);
            }
        }
    }
    Ok(())
}

use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use tempfile::NamedTempFile;
use xmedia_core::domain::error::{DomainError, DomainResult};

fn apply_target_permissions(target: &Path, staging: &Path) {
    if let Ok(meta) = std::fs::metadata(target) {
        let _ = std::fs::set_permissions(staging, meta.permissions());
    } else {
        #[cfg(unix)]
        {
            let _ = std::fs::set_permissions(staging, std::fs::Permissions::from_mode(0o644));
        }
    }
}

/// Writes `bytes` to a sibling temporary file, fsyncs, preserves file permissions, and atomically replaces `target`.
pub fn atomic_write_bytes(target: &Path, bytes: &[u8]) -> DomainResult<()> {
    let parent = target.parent().ok_or_else(|| {
        DomainError::Storage(format!("target path '{}' has no parent", target.display()))
    })?;
    std::fs::create_dir_all(parent).map_err(|e| {
        DomainError::Storage(format!(
            "failed to create parent directory '{}': {e}",
            parent.display()
        ))
    })?;

    let mut tmp = NamedTempFile::new_in(parent).map_err(|e| {
        DomainError::Storage(format!(
            "failed to create temp file in '{}': {e}",
            parent.display()
        ))
    })?;
    tmp.write_all(bytes)
        .map_err(|e| DomainError::Storage(format!("failed writing temp file: {e}")))?;
    tmp.as_file()
        .sync_all()
        .map_err(|e| DomainError::Storage(format!("failed syncing temp file: {e}")))?;
    apply_target_permissions(target, tmp.path());
    tmp.persist(target).map_err(|e| {
        DomainError::Storage(format!(
            "failed atomically persisting '{}': {e}",
            target.display()
        ))
    })?;
    Ok(())
}

/// Writes a fresh replacement for `target` via `build(temp_path)` without pre-copying `target`,
/// preserves permissions, fsyncs, and atomically replaces `target`.
pub fn atomic_write_via_temp<F>(target: &Path, build: F) -> DomainResult<()>
where
    F: FnOnce(&Path) -> DomainResult<()>,
{
    let parent = target.parent().ok_or_else(|| {
        DomainError::Storage(format!("target path '{}' has no parent", target.display()))
    })?;
    std::fs::create_dir_all(parent).map_err(|e| {
        DomainError::Storage(format!(
            "failed to create parent directory '{}': {e}",
            parent.display()
        ))
    })?;

    let tmp = NamedTempFile::new_in(parent).map_err(|e| {
        DomainError::Storage(format!(
            "failed to create temp file in '{}': {e}",
            parent.display()
        ))
    })?;

    build(tmp.path())?;

    let reopened = std::fs::File::open(tmp.path())
        .map_err(|e| DomainError::Storage(format!("failed reopening staging file: {e}")))?;
    let _ = reopened.sync_all();
    apply_target_permissions(target, tmp.path());

    tmp.persist(target).map_err(|e| {
        DomainError::Storage(format!(
            "failed atomically replacing '{}': {e}",
            target.display()
        ))
    })?;
    Ok(())
}

/// Copies `target` to a sibling temporary file, runs `mutate(temp_path)`, fsyncs, preserves
/// permissions, and atomically replaces `target` so the source file is never partially overwritten on error.
pub fn atomic_mutate_copy<F>(target: &Path, mutate: F) -> DomainResult<()>
where
    F: FnOnce(&Path) -> DomainResult<()>,
{
    let parent = target.parent().ok_or_else(|| {
        DomainError::Storage(format!("target path '{}' has no parent", target.display()))
    })?;
    let tmp = NamedTempFile::new_in(parent).map_err(|e| {
        DomainError::Storage(format!(
            "failed to create temp file in '{}': {e}",
            parent.display()
        ))
    })?;
    std::fs::copy(target, tmp.path()).map_err(|e| {
        DomainError::Storage(format!(
            "failed copying '{}' to staging file: {e}",
            target.display()
        ))
    })?;

    mutate(tmp.path())?;

    let reopened = std::fs::File::open(tmp.path())
        .map_err(|e| DomainError::Storage(format!("failed reopening staging file: {e}")))?;
    let _ = reopened.sync_all();
    apply_target_permissions(target, tmp.path());

    tmp.persist(target).map_err(|e| {
        DomainError::Storage(format!(
            "failed atomically replacing '{}': {e}",
            target.display()
        ))
    })?;
    Ok(())
}

/// Discovers companion sidecar files sharing the same filename stem in the same directory
/// (e.g. `song.lrc`, `book.opf`, `book.jpg`, `movie.nfo`, `movie.zh.srt`, `movie-poster.jpg`),
/// returning `(path, suffix_after_stem)`.
pub fn find_companion_sidecars(media_file: &Path) -> Vec<(PathBuf, String)> {
    let Some(parent) = media_file.parent() else {
        return Vec::new();
    };
    let Some(stem) = media_file.file_stem().and_then(|s| s.to_str()) else {
        return Vec::new();
    };
    let main_is_image = media_file
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|e| {
            matches!(
                e.to_ascii_lowercase().as_str(),
                "jpg" | "jpeg" | "png" | "webp" | "gif" | "bmp" | "avif"
            )
        });
    let Ok(entries) = std::fs::read_dir(parent) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for entry in entries.flatten() {
        let p = entry.path();
        if p == media_file || !p.is_file() {
            continue;
        }
        let Some(fname) = p.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let Some(rest) = fname.strip_prefix(stem) else {
            continue;
        };
        let lower = rest.to_ascii_lowercase();
        let is_companion = if rest.starts_with('.') {
            lower.ends_with(".lrc")
                || lower.ends_with(".nfo")
                || lower.ends_with(".opf")
                || lower.ends_with(".srt")
                || lower.ends_with(".ass")
                || lower.ends_with(".ssa")
                || lower.ends_with(".vtt")
                || (!main_is_image && matches!(lower.as_str(), ".jpg" | ".jpeg" | ".png" | ".webp"))
        } else if rest.starts_with('-') {
            matches!(
                lower.as_str(),
                "-poster.jpg"
                    | "-poster.jpeg"
                    | "-poster.png"
                    | "-poster.webp"
                    | "-fanart.jpg"
                    | "-fanart.jpeg"
                    | "-fanart.png"
                    | "-fanart.webp"
                    | "-thumb.jpg"
                    | "-thumb.jpeg"
                    | "-thumb.png"
                    | "-thumb.webp"
            )
        } else {
            false
        };
        if is_companion {
            let suffix = rest.to_string();
            out.push((p, suffix));
        }
    }
    out
}

fn is_orphan_jellyfin_dir_sidecar(file_name: &str) -> bool {
    matches!(
        file_name.to_ascii_lowercase().as_str(),
        "metadata.opf"
            | "movie.nfo"
            | "tvshow.nfo"
            | "season.nfo"
            | "poster.jpg"
            | "poster.jpeg"
            | "poster.png"
            | "poster.webp"
            | "fanart.jpg"
            | "fanart.jpeg"
            | "fanart.png"
            | "fanart.webp"
            | "backdrop.jpg"
            | "backdrop.jpeg"
            | "backdrop.png"
            | "backdrop.webp"
            | "folder.jpg"
            | "folder.jpeg"
            | "folder.png"
            | "folder.webp"
            | "cover.jpg"
            | "cover.jpeg"
            | "cover.png"
            | "cover.webp"
            | "front.jpg"
            | "front.jpeg"
            | "front.png"
            | "front.webp"
            | ".ds_store"
            | "thumbs.db"
    )
}

/// Removes vacated directories starting at `start_dir` walking upward, strictly stopping before `stop_at_root`.
///
/// If a vacated Jellyfin movie/season/series directory no longer contains any media or user files
/// (only orphan `movie.nfo` / `season.nfo` / `tvshow.nfo` / `poster.*` / `fanart.*`), those orphan
/// metadata files are removed first so no empty 0-file ghost folder is left behind on re-scrape/rename.
pub fn cleanup_empty_parent_dirs(start_dir: &Path, stop_at_root: &Path) {
    let mut cur = Some(start_dir);
    while let Some(dir) = cur {
        if dir == stop_at_root || !dir.starts_with(stop_at_root) {
            break;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            break;
        };
        let mut orphan_files = Vec::new();
        let mut has_non_orphan = false;
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                has_non_orphan = true;
                break;
            }
            let Some(fname) = p.file_name().and_then(|s| s.to_str()) else {
                has_non_orphan = true;
                break;
            };
            if is_orphan_jellyfin_dir_sidecar(fname) {
                orphan_files.push(p);
            } else {
                has_non_orphan = true;
                break;
            }
        }
        if has_non_orphan {
            break;
        }
        for orphan in orphan_files {
            let _ = std::fs::remove_file(orphan);
        }
        if std::fs::remove_dir(dir).is_ok() {
            cur = dir.parent();
        } else {
            break;
        }
    }
}

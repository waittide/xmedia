use std::io::Write;
use std::path::{Path, PathBuf};

use tempfile::NamedTempFile;
use xmedia_core::domain::error::{DomainError, DomainResult};

/// Writes `bytes` to a sibling temporary file, fsyncs, and atomically replaces `target`.
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
    tmp.persist(target).map_err(|e| {
        DomainError::Storage(format!(
            "failed atomically persisting '{}': {e}",
            target.display()
        ))
    })?;
    Ok(())
}

/// Copies `target` to a sibling temporary file, runs `mutate(temp_path)`, fsyncs, and
/// atomically replaces `target` so the source file is never partially overwritten on error.
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

    tmp.persist(target).map_err(|e| {
        DomainError::Storage(format!(
            "failed atomically replacing '{}': {e}",
            target.display()
        ))
    })?;
    Ok(())
}

/// Discovers companion sidecar files sharing the same filename stem in the same directory
/// (e.g. `song.lrc`, `movie.nfo`, `movie.zh.srt`, `movie-poster.jpg`), returning `(path, suffix_after_stem)`.
pub fn find_companion_sidecars(media_file: &Path) -> Vec<(PathBuf, String)> {
    let Some(parent) = media_file.parent() else {
        return Vec::new();
    };
    let Some(stem) = media_file.file_stem().and_then(|s| s.to_str()) else {
        return Vec::new();
    };
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
        if let Some(rest) = fname.strip_prefix(stem)
            && (rest.starts_with('.') || rest.starts_with('-'))
        {
            let lower = rest.to_ascii_lowercase();
            let is_companion = lower.ends_with(".lrc")
                || lower.ends_with(".nfo")
                || lower.ends_with(".srt")
                || lower.ends_with(".ass")
                || lower.ends_with(".ssa")
                || lower.ends_with(".vtt")
                || lower.ends_with("-poster.jpg")
                || lower.ends_with("-poster.png")
                || lower.ends_with("-fanart.jpg")
                || lower.ends_with("-fanart.png")
                || lower.ends_with("-thumb.jpg")
                || lower.ends_with("-thumb.png");
            if is_companion {
                let suffix = rest.to_string();
                out.push((p, suffix));
            }
        }
    }
    out
}

/// Removes empty directories starting at `start_dir` walking upward, strictly stopping before `stop_at_root`.
pub fn cleanup_empty_parent_dirs(start_dir: &Path, stop_at_root: &Path) {
    let mut cur = Some(start_dir);
    while let Some(dir) = cur {
        if dir == stop_at_root || !dir.starts_with(stop_at_root) {
            break;
        }
        let is_empty = std::fs::read_dir(dir)
            .ok()
            .is_some_and(|mut it| it.next().is_none());
        if is_empty && std::fs::remove_dir(dir).is_ok() {
            cur = dir.parent();
        } else {
            break;
        }
    }
}

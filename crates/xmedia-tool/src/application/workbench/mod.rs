mod browse;
mod metadata;
mod rename;
mod rename_exec;
mod tools;

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use uuid::Uuid;
use walkdir::WalkDir;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;

use crate::application::ports::MediaInspectorPort;
use crate::domain::config::RenameTemplates;
use crate::domain::media::{MediaFileEntry, MediaMetadataPayload, RenameConflictPolicy};
use crate::domain::template::render_template_target;
use crate::domain::workspace::{ManagedRoot, RootId, path_to_slash};

pub use browse::{WorkspaceArtworkOutput, WorkspaceBrowserService};
pub(in crate::application) use metadata::MetadataHistoryEvent;
pub use metadata::{BatchSyncReportDto, BatchSyncSpec, MetadataService};
pub use rename::RenameService;
pub use tools::{
    BookWorkbenchOp, BookWorkbenchResult, ComicWorkbenchOp, ComicWorkbenchResult,
    FormatToolsService, MusicWorkbenchOp, MusicWorkbenchResult, VideoWorkbenchOp,
    VideoWorkbenchResult,
};

#[derive(Debug, Clone, serde::Serialize)]
pub struct WorkbenchProgress {
    pub processed: u64,
    pub total: Option<u64>,
    pub detail: Option<String>,
}

pub(super) fn progress_step_for(total: u64) -> u64 {
    if total <= 100 {
        1
    } else if total <= 1000 {
        5
    } else {
        25
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct PreviewRenameOptions<'a> {
    pub template_override: Option<&'a str>,
    pub episode_template_override: Option<&'a str>,
    pub preferred_kind: Option<MediaKind>,
    pub conflict_policy: RenameConflictPolicy,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RenameMoveRecord {
    pub from_rel_path: String,
    pub to_rel_path: String,
    pub companion_moves: Vec<(String, String)>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RenameHistoryBatchDto {
    pub batch_id: Uuid,
    pub root_id: Uuid,
    pub created_at_unix: i64,
    pub moved_count: usize,
    pub moves: Vec<RenameMoveRecord>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct UndoRenameReportDto {
    pub batch_id: Uuid,
    pub restored_count: usize,
    pub skipped_conflicts: Vec<String>,
}

/// Shared per-root mutex registry serializing rename/archive mutations within a `ManagedRoot`.
#[derive(Clone, Default)]
pub struct RootLockRegistry {
    locks: Arc<std::sync::Mutex<HashMap<Uuid, Arc<tokio::sync::Mutex<()>>>>>,
}

impl RootLockRegistry {
    pub(in crate::application) fn lock_for(&self, root_id: RootId) -> Arc<tokio::sync::Mutex<()>> {
        let mut guard = self.locks.lock().unwrap_or_else(|e| e.into_inner());
        Arc::clone(
            guard
                .entry(root_id.0)
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(()))),
        )
    }
}

pub(super) fn find_inspector_for_path(
    inspectors: &[Arc<dyn MediaInspectorPort>],
    path: &Path,
    preferred_kind: Option<MediaKind>,
) -> Option<Arc<dyn MediaInspectorPort>> {
    if let Some(pref) = preferred_kind
        && let Some(matched) = inspectors
            .iter()
            .find(|ins| ins.media_kind() == pref && ins.supports_path(path))
    {
        return Some(Arc::clone(matched));
    }
    inspectors
        .iter()
        .find(|ins| ins.supports_path(path))
        .cloned()
}

pub(super) async fn build_file_entry(
    root: &ManagedRoot,
    abs_path: &Path,
    file_name: &str,
    inspector: &dyn MediaInspectorPort,
    rename_templates: &RenameTemplates,
) -> DomainResult<MediaFileEntry> {
    let fs_meta = tokio::fs::metadata(abs_path).await.map_err(|e| {
        DomainError::Storage(format!("failed stating '{}': {e}", abs_path.display()))
    })?;
    let modified_unix_secs = fs_meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let rel_path = abs_path
        .strip_prefix(&root.path)
        .map(path_to_slash)
        .unwrap_or_else(|_| file_name.to_string());
    let is_dir = abs_path.is_dir();
    let (raw_ext, ext, fallback_stem, file_size) = if is_dir {
        let dir_bytes = std::fs::read_dir(abs_path)
            .ok()
            .map(|rd| {
                rd.flatten()
                    .filter_map(|e| e.metadata().ok())
                    .filter(|m| m.is_file())
                    .map(|m| m.len())
                    .sum::<u64>()
            })
            .unwrap_or(0);
        ("", "folder".to_string(), file_name, dir_bytes)
    } else {
        let re = abs_path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        let fs = abs_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(file_name);
        (re, re.to_ascii_lowercase(), fs, fs_meta.len())
    };

    let mut quick = inspector.summarize_file(abs_path, fallback_stem).await;
    if let Some(ref meta) = quick.metadata {
        quick.completeness.is_organized =
            is_entry_organized(abs_path, &rel_path, raw_ext, meta, rename_templates);
    }

    Ok(MediaFileEntry {
        root_id: root.id,
        rel_path,
        file_name: file_name.to_string(),
        extension: ext,
        media_kind: inspector.media_kind(),
        file_size,
        modified_unix_secs,
        display_title: quick.display_title,
        display_subtitle: quick.display_subtitle,
        year: quick.year,
        completeness: quick.completeness,
    })
}

fn is_entry_organized(
    abs_path: &Path,
    rel_path: &str,
    raw_ext: &str,
    meta: &MediaMetadataPayload,
    templates: &RenameTemplates,
) -> bool {
    let tpl = match meta {
        MediaMetadataPayload::Book(_) => &templates.book,
        MediaMetadataPayload::Comic(_) => &templates.comic,
        MediaMetadataPayload::Music(_) => &templates.music,
        MediaMetadataPayload::Video(v) => {
            if v.is_episode {
                &templates.video_episode
            } else {
                &templates.video_movie
            }
        }
    };
    if let MediaMetadataPayload::Video(v) = meta
        && v.is_episode
        && v.episode_number.is_none()
        && tpl.to_ascii_lowercase().contains("{episode")
    {
        return false;
    }
    let parent_rel = Path::new(rel_path)
        .parent()
        .map(path_to_slash)
        .unwrap_or_default();
    let (to_rel, _) = render_template_target(tpl, &parent_rel, meta, abs_path, raw_ext);
    rel_path == to_rel
}

pub(super) fn dir_has_any_media_file(
    dir_path: &Path,
    inspectors: &[Arc<dyn MediaInspectorPort>],
) -> bool {
    WalkDir::new(dir_path)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            e.depth() == 0
                || (!e.path_is_symlink()
                    && !e.file_name().to_str().is_some_and(|n| n.starts_with('.')))
        })
        .flatten()
        .any(|entry| {
            entry.depth() > 0
                && !entry.file_type().is_symlink()
                && inspectors.iter().any(|ins| ins.supports_path(entry.path()))
        })
}

pub(super) fn is_same_physical_file(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let (Ok(ma), Ok(mb)) = (std::fs::metadata(a), std::fs::metadata(b)) {
            return ma.dev() == mb.dev() && ma.ino() == mb.ino();
        }
        false
    }
    #[cfg(not(unix))]
    {
        if let (Ok(ca), Ok(cb)) = (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
            return ca == cb;
        }
        false
    }
}

pub(super) fn rename_path_preserving_case(src: &Path, dst: &Path) -> std::io::Result<()> {
    if src == dst {
        return Ok(());
    }
    if is_same_physical_file(src, dst) {
        let tmp = src.with_file_name(format!(".xmedia_case_{}.tmp", Uuid::now_v7().simple()));
        std::fs::rename(src, &tmp)?;
        if let Err(err) = std::fs::rename(&tmp, dst) {
            let _ = std::fs::rename(&tmp, src);
            return Err(err);
        }
        return Ok(());
    }
    std::fs::rename(src, dst)
}

#[cfg(test)]
mod rename_case_tests {
    use super::{is_same_physical_file, rename_path_preserving_case};

    #[test]
    fn same_physical_file_and_two_step_rename_preserve_content() {
        let tmp = tempfile::tempdir().unwrap();
        let file_a = tmp.path().join("song.flac");
        let file_b = tmp.path().join("other.flac");
        std::fs::write(&file_a, b"audio-bytes").unwrap();
        std::fs::write(&file_b, b"other-bytes").unwrap();

        assert!(is_same_physical_file(&file_a, &file_a));
        assert!(!is_same_physical_file(&file_a, &file_b));

        #[cfg(unix)]
        {
            let link_a = tmp.path().join("Song.flac");
            std::fs::hard_link(&file_a, &link_a).unwrap();
            assert!(is_same_physical_file(&file_a, &link_a));
            rename_path_preserving_case(&file_a, &link_a).unwrap();
            assert_eq!(std::fs::read(&link_a).unwrap(), b"audio-bytes");
        }
    }
}

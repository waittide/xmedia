mod book_ops;
mod browse;
mod media_ops;
mod metadata;
mod rename;

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use tokio::sync::RwLock;
use uuid::Uuid;
use walkdir::WalkDir;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;
use xmedia_core::infra::ffmpeg::FfmpegToolkit;

use crate::application::batch_metadata::MetadataHistoryBatchDto;
use crate::application::settings::ToolSettingsService;
use crate::domain::config::RenameTemplates;
use crate::domain::media::{MediaFileEntry, MediaMetadataPayload, RenameConflictPolicy};
use crate::domain::ports::MediaInspectorPort;
use crate::domain::template::render_template_target;
use crate::domain::workspace::{ManagedRoot, RootId, SandboxPath, path_to_slash};
use crate::infra::workspace_fs::resolve_existing_in_root;

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

#[derive(Clone)]
pub struct MediaWorkbenchService {
    pub(super) settings: ToolSettingsService,
    pub(super) inspectors: Vec<Arc<dyn MediaInspectorPort>>,
    pub(super) ffmpeg: FfmpegToolkit,
    pub(super) rename_history: Arc<RwLock<Vec<RenameHistoryBatchDto>>>,
    pub(super) metadata_history: Arc<RwLock<Vec<MetadataHistoryBatchDto>>>,
    pub(super) root_locks: Arc<std::sync::Mutex<HashMap<Uuid, Arc<tokio::sync::Mutex<()>>>>>,
}

impl MediaWorkbenchService {
    pub fn new(
        settings: ToolSettingsService,
        inspectors: Vec<Arc<dyn MediaInspectorPort>>,
        ffmpeg: FfmpegToolkit,
    ) -> Self {
        Self {
            settings,
            inspectors,
            ffmpeg,
            rename_history: Arc::new(RwLock::new(Vec::new())),
            metadata_history: Arc::new(RwLock::new(Vec::new())),
            root_locks: Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }

    pub(crate) fn root_write_lock(&self, root_id: RootId) -> Arc<tokio::sync::Mutex<()>> {
        let mut guard = self.root_locks.lock().unwrap_or_else(|e| e.into_inner());
        Arc::clone(
            guard
                .entry(root_id.0)
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(()))),
        )
    }

    pub fn resolve_inspector_for_path(
        &self,
        path: &Path,
        preferred_kind: Option<MediaKind>,
    ) -> Option<Arc<dyn MediaInspectorPort>> {
        let ext = path
            .extension()
            .and_then(|s| s.to_str())?
            .to_ascii_lowercase();
        if let Some(pref) = preferred_kind
            && let Some(matched) = self
                .inspectors
                .iter()
                .find(|ins| ins.media_kind() == pref && ins.supports_extension(&ext))
        {
            return Some(Arc::clone(matched));
        }
        self.inspectors
            .iter()
            .find(|ins| ins.supports_extension(&ext))
            .cloned()
    }

    pub(super) async fn resolve_writable_media_file(
        &self,
        root_id: RootId,
        rel_path: &str,
        preferred_kind: Option<MediaKind>,
    ) -> DomainResult<(ManagedRoot, SandboxPath, Arc<dyn MediaInspectorPort>)> {
        let (root, sandbox, inspector) = self
            .resolve_media_file(root_id, rel_path, preferred_kind)
            .await?;
        root.ensure_writable()?;
        Ok((root, sandbox, inspector))
    }

    pub(super) async fn resolve_media_file(
        &self,
        root_id: RootId,
        rel_path: &str,
        preferred_kind: Option<MediaKind>,
    ) -> DomainResult<(ManagedRoot, SandboxPath, Arc<dyn MediaInspectorPort>)> {
        let root = self.settings.find_root(root_id).await?;
        let sandbox = resolve_existing_in_root(&root, rel_path)?;
        if !sandbox.abs_path.is_file() {
            return Err(DomainError::Validation(format!(
                "'{}' is not a regular file",
                sandbox.rel_slash_string()
            )));
        }
        let effective_pref = preferred_kind.or(root.default_kind);
        let inspector = self
            .resolve_inspector_for_path(&sandbox.abs_path, effective_pref)
            .ok_or_else(|| {
                DomainError::UnsupportedFormat(format!(
                    "unsupported media file '{}'",
                    sandbox.rel_slash_string()
                ))
            })?;
        Ok((root, sandbox, inspector))
    }

    pub(super) async fn build_file_entry(
        &self,
        root: &ManagedRoot,
        abs_path: &Path,
        file_name: &str,
        inspector: &dyn MediaInspectorPort,
        rename_templates: &RenameTemplates,
    ) -> DomainResult<MediaFileEntry> {
        let fs_meta = std::fs::metadata(abs_path).map_err(|e| {
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
        let raw_ext = abs_path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        let ext = raw_ext.to_ascii_lowercase();
        let fallback_stem = abs_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(file_name);

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
            file_size: fs_meta.len(),
            modified_unix_secs,
            display_title: quick.display_title,
            display_subtitle: quick.display_subtitle,
            year: quick.year,
            completeness: quick.completeness,
        })
    }
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

pub(super) fn count_subtree_children(
    dir_path: &Path,
    inspectors: &[Arc<dyn MediaInspectorPort>],
    domain_filter: Option<MediaKind>,
) -> (usize, usize) {
    let mut immediate_dirs = 0usize;
    if let Ok(entries) = std::fs::read_dir(dir_path) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir()
                && !p
                    .file_name()
                    .and_then(|s| s.to_str())
                    .is_some_and(|n| n.starts_with('.'))
            {
                immediate_dirs += 1;
            }
        }
    }

    let mut total_media_files = 0usize;
    for entry in WalkDir::new(dir_path)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            e.depth() == 0 || !e.file_name().to_str().is_some_and(|n| n.starts_with('.'))
        })
        .flatten()
    {
        if !entry.file_type().is_file() {
            continue;
        }
        if let Some(ext) = entry.path().extension().and_then(|s| s.to_str()) {
            let lower = ext.to_ascii_lowercase();
            let matched = inspectors.iter().any(|ins| {
                ins.supports_extension(&lower)
                    && domain_filter.is_none_or(|want| ins.media_kind() == want)
            });
            if matched {
                total_media_files += 1;
            }
        }
    }

    (immediate_dirs, total_media_files)
}

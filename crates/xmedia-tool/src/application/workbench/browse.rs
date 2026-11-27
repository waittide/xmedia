use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use tokio::sync::mpsc::Sender;
use walkdir::WalkDir;
use xmedia_core::application::ports::BinaryPayload;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;

use super::{
    WorkbenchProgress, build_file_entry, dir_has_any_media_file, find_inspector_for_path,
    progress_step_for,
};
use crate::application::ports::{ArtworkIo, MediaInspectorPort, WorkspaceFsPort};
use crate::application::settings::ToolSettingsService;
use crate::domain::config::RenameTemplates;
use crate::domain::media::{
    DeleteFileErrorDto, DeleteFilesReportDto, DirectorySnapshot, FolderEntryDto, MediaFileDetail,
    MediaFileEntry, RawFileEntryDto,
};
use crate::domain::workspace::{
    ManagedRoot, RootId, SandboxPath, is_shared_dir_sidecar_name, path_to_slash,
};

#[derive(Debug)]
pub enum WorkspaceArtworkOutput {
    Read(Option<BinaryPayload>),
    Written(Box<MediaFileDetail>),
}

type RawDirectoryScan = (
    String,
    Option<String>,
    Vec<FolderEntryDto>,
    Vec<RawFileEntryDto>,
    Vec<(String, PathBuf, Arc<dyn MediaInspectorPort>)>,
);

#[derive(Clone)]
pub struct WorkspaceBrowserService {
    settings: ToolSettingsService,
    inspectors: Vec<Arc<dyn MediaInspectorPort>>,
    fs: Arc<dyn WorkspaceFsPort>,
}

impl WorkspaceBrowserService {
    pub fn new(
        settings: ToolSettingsService,
        inspectors: Vec<Arc<dyn MediaInspectorPort>>,
        fs: Arc<dyn WorkspaceFsPort>,
    ) -> Self {
        Self {
            settings,
            inspectors,
            fs,
        }
    }

    pub(in crate::application) fn resolve_inspector(
        &self,
        path: &Path,
        preferred_kind: Option<MediaKind>,
    ) -> Option<Arc<dyn MediaInspectorPort>> {
        find_inspector_for_path(&self.inspectors, path, preferred_kind)
    }

    pub(super) fn dir_has_any_media(&self, dir_path: &Path) -> bool {
        dir_has_any_media_file(dir_path, &self.inspectors)
    }

    pub(super) async fn resolve_media_file(
        &self,
        root_id: RootId,
        rel_path: &str,
        preferred_kind: Option<MediaKind>,
    ) -> DomainResult<(ManagedRoot, SandboxPath, Arc<dyn MediaInspectorPort>)> {
        let raw_settings = self.settings.get_raw().await?;
        let root = raw_settings.find_root(root_id)?.clone();
        let sandbox = self.fs.resolve_existing_in_root(&root, rel_path)?;
        let effective_pref = preferred_kind.or(root.default_kind);
        if !sandbox.abs_path.is_file() && !sandbox.abs_path.is_dir() {
            return Err(DomainError::Validation(format!(
                "'{}' is not a regular file",
                sandbox.rel_slash_string()
            )));
        }
        let inspector = self
            .resolve_inspector(&sandbox.abs_path, effective_pref)
            .ok_or_else(|| {
                if sandbox.abs_path.is_dir() {
                    DomainError::Validation(format!(
                        "'{}' is not a regular file",
                        sandbox.rel_slash_string()
                    ))
                } else {
                    DomainError::UnsupportedFormat(format!(
                        "unsupported media file '{}'",
                        sandbox.rel_slash_string()
                    ))
                }
            })?;
        Ok((root, sandbox, inspector))
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

    pub(super) async fn build_entry(
        &self,
        root: &ManagedRoot,
        abs_path: &Path,
        file_name: &str,
        inspector: &dyn MediaInspectorPort,
        rename_templates: &RenameTemplates,
    ) -> DomainResult<MediaFileEntry> {
        build_file_entry(root, abs_path, file_name, inspector, rename_templates).await
    }

    pub async fn browse(
        &self,
        root_id: RootId,
        rel_path: &str,
        domain_filter: Option<MediaKind>,
        recursive: bool,
        progress: Option<Sender<WorkbenchProgress>>,
    ) -> DomainResult<DirectorySnapshot> {
        let raw_settings = self.settings.get_raw().await?;
        let root = raw_settings.find_root(root_id)?.clone();
        let rename_templates = raw_settings.rename_templates;
        let rel_path_owned = rel_path.to_string();
        let root_for_scan = root.clone();
        let inspectors = self.inspectors.clone();
        let fs = Arc::clone(&self.fs);
        let effective_pref = domain_filter.or(root.default_kind);
        let scan_prog = progress.clone();

        let (current_rel, parent_rel, folders, folder_raw_entries, raw_files) =
            tokio::task::spawn_blocking(move || {
                scan_directory_tree(
                    fs.as_ref(),
                    &root_for_scan,
                    &rel_path_owned,
                    &inspectors,
                    domain_filter,
                    effective_pref,
                    recursive,
                    scan_prog.as_ref(),
                )
            })
            .await
            .map_err(|e| DomainError::Storage(format!("directory browse task failed: {e}")))??;

        let total_files = raw_files.len() as u64;
        let step = progress_step_for(total_files);
        if let Some(ref tx) = progress {
            let _ = tx.try_send(WorkbenchProgress {
                processed: 0,
                total: Some(total_files),
                detail: None,
            });
        }

        let mut files = Vec::with_capacity(raw_files.len());
        for (idx, (fname, file_abs, inspector)) in raw_files.into_iter().enumerate() {
            let entry = build_file_entry(
                &root,
                &file_abs,
                &fname,
                inspector.as_ref(),
                &rename_templates,
            )
            .await?;
            files.push(entry);
            let done = (idx + 1) as u64;
            if let Some(ref tx) = progress
                && (done.is_multiple_of(step) || done == total_files)
            {
                let _ = tx.try_send(WorkbenchProgress {
                    processed: done,
                    total: Some(total_files),
                    detail: Some(fname),
                });
            }
        }

        Ok(DirectorySnapshot {
            root_id: root.id,
            root_name: root.name,
            current_rel_path: current_rel,
            parent_rel_path: parent_rel,
            folders,
            files,
            raw_files: folder_raw_entries,
        })
    }

    pub async fn inspect(
        &self,
        root_id: RootId,
        rel_path: &str,
        preferred_kind: Option<MediaKind>,
    ) -> DomainResult<MediaFileDetail> {
        let (root, sandbox, inspector) = self
            .resolve_media_file(root_id, rel_path, preferred_kind)
            .await?;
        let fname = sandbox
            .abs_path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let rename_templates = self.settings.get_raw().await?.rename_templates;
        let entry = build_file_entry(
            &root,
            &sandbox.abs_path,
            &fname,
            inspector.as_ref(),
            &rename_templates,
        )
        .await?;
        let metadata = inspector.inspect_file(&sandbox.abs_path).await?;
        Ok(MediaFileDetail { entry, metadata })
    }

    pub async fn delete(
        &self,
        root_id: RootId,
        rel_paths: &[String],
        delete_companions: bool,
    ) -> DomainResult<DeleteFilesReportDto> {
        let root = self.settings.get_raw().await?.find_root(root_id)?.clone();
        root.ensure_writable()?;
        let rel_paths_owned = rel_paths.to_vec();
        let inspectors = self.inspectors.clone();
        let fs = Arc::clone(&self.fs);

        tokio::task::spawn_blocking(move || {
            delete_entries_blocking(
                fs.as_ref(),
                &root,
                &rel_paths_owned,
                delete_companions,
                &inspectors,
            )
        })
        .await
        .map_err(|e| DomainError::Storage(format!("delete files task failed: {e}")))
    }

    pub async fn artwork(
        &self,
        root_id: RootId,
        rel_path: &str,
        preferred_kind: Option<MediaKind>,
        op: ArtworkIo<'_>,
    ) -> DomainResult<WorkspaceArtworkOutput> {
        match op {
            ArtworkIo::Read { .. } => {
                let (_root, sandbox, inspector) = self
                    .resolve_media_file(root_id, rel_path, preferred_kind)
                    .await?;
                let payload = inspector.artwork(&sandbox.abs_path, op).await?;
                Ok(WorkspaceArtworkOutput::Read(payload))
            }
            ArtworkIo::Write { .. } => {
                let (root, sandbox, inspector) = self
                    .resolve_writable_media_file(root_id, rel_path, preferred_kind)
                    .await?;
                inspector.artwork(&sandbox.abs_path, op).await?;
                let fname = sandbox
                    .abs_path
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let rename_templates = self.settings.get_raw().await?.rename_templates;
                let entry = build_file_entry(
                    &root,
                    &sandbox.abs_path,
                    &fname,
                    inspector.as_ref(),
                    &rename_templates,
                )
                .await?;
                let metadata = inspector.inspect_file(&sandbox.abs_path).await?;
                Ok(WorkspaceArtworkOutput::Written(Box::new(MediaFileDetail {
                    entry,
                    metadata,
                })))
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn scan_directory_tree(
    fs: &dyn WorkspaceFsPort,
    root: &ManagedRoot,
    rel_path: &str,
    inspectors: &[Arc<dyn MediaInspectorPort>],
    domain_filter: Option<MediaKind>,
    effective_pref: Option<MediaKind>,
    recursive: bool,
    progress: Option<&Sender<WorkbenchProgress>>,
) -> DomainResult<RawDirectoryScan> {
    let dir_sandbox = fs.resolve_existing_in_root(root, rel_path)?;
    if !dir_sandbox.abs_path.is_dir() {
        return Err(DomainError::Validation(format!(
            "'{}' is not a directory",
            dir_sandbox.rel_slash_string()
        )));
    }

    std::fs::read_dir(&dir_sandbox.abs_path).map_err(|e| {
        DomainError::Storage(format!(
            "failed reading directory '{}': {e}",
            dir_sandbox.abs_path.display()
        ))
    })?;

    let current_rel = dir_sandbox.rel_slash_string();
    let parent_rel = (!current_rel.is_empty()).then(|| {
        dir_sandbox
            .rel_path
            .parent()
            .map(path_to_slash)
            .unwrap_or_default()
    });

    if let Some(tx) = progress {
        let _ = tx.try_send(WorkbenchProgress {
            processed: 0,
            total: None,
            detail: Some(if current_rel.is_empty() {
                root.name.clone()
            } else {
                current_rel.clone()
            }),
        });
    }

    let mut raw_dirs: Vec<(String, PathBuf)> = Vec::new();
    let mut dir_counts: HashMap<String, (usize, usize)> = HashMap::new();
    let mut raw_files: Vec<(String, PathBuf, Arc<dyn MediaInspectorPort>)> = Vec::new();
    let mut folder_raw_entries: Vec<RawFileEntryDto> = Vec::new();

    for entry in WalkDir::new(&dir_sandbox.abs_path)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            e.depth() == 0
                || (!e.path_is_symlink()
                    && e.file_name().to_str().is_some_and(|n| !n.starts_with('.')))
        })
        .flatten()
    {
        let depth = entry.depth();
        if depth == 0 {
            continue;
        }
        let ft = entry.file_type();
        if ft.is_symlink() {
            continue;
        }
        let p = entry.path();
        let Some(fname) = p.file_name().and_then(|s| s.to_str()) else {
            continue;
        };

        if ft.is_dir() {
            let matched_dir_ins = find_inspector_for_path(inspectors, p, effective_pref)
                .filter(|ins| domain_filter.is_none_or(|want| ins.media_kind() == want));
            if depth == 1 {
                let name_owned = fname.to_string();
                dir_counts.entry(name_owned.clone()).or_insert((0, 0));
                raw_dirs.push((name_owned.clone(), p.to_path_buf()));
                if let Some(ref ins) = matched_dir_ins {
                    dir_counts.entry(name_owned.clone()).or_insert((0, 0)).1 += 1;
                    if !recursive {
                        raw_files.push((name_owned, p.to_path_buf(), ins.clone()));
                    }
                }
            } else if depth == 2
                && let Ok(rel_from_cur) = p.strip_prefix(&dir_sandbox.abs_path)
                && let Some(Component::Normal(top_os)) = rel_from_cur.components().next()
                && let Some(top_name) = top_os.to_str()
            {
                dir_counts.entry(top_name.to_string()).or_insert((0, 0)).0 += 1;
            }
            if depth >= 2
                && matched_dir_ins.is_some()
                && let Ok(rel_from_cur) = p.strip_prefix(&dir_sandbox.abs_path)
                && let Some(Component::Normal(top_os)) = rel_from_cur.components().next()
                && let Some(top_name) = top_os.to_str()
            {
                dir_counts.entry(top_name.to_string()).or_insert((0, 0)).1 += 1;
            }
            if recursive && let Some(ins) = matched_dir_ins {
                raw_files.push((fname.to_string(), p.to_path_buf(), ins));
            }
            continue;
        }

        if !ft.is_file() {
            continue;
        }

        let matched_ins = find_inspector_for_path(inspectors, p, effective_pref)
            .filter(|ins| domain_filter.is_none_or(|want| ins.media_kind() == want));

        if depth == 1 {
            let meta_fs = entry.metadata().ok();
            let file_size = meta_fs.as_ref().map(|m| m.len()).unwrap_or(0);
            let modified_unix_secs = meta_fs
                .as_ref()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let ext = p
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            let rel = p
                .strip_prefix(&root.path)
                .map(path_to_slash)
                .unwrap_or_else(|_| fname.to_string());
            folder_raw_entries.push(RawFileEntryDto {
                rel_path: rel,
                file_name: fname.to_string(),
                extension: ext,
                file_size,
                modified_unix_secs,
                is_media: matched_ins.is_some(),
                media_kind: matched_ins.as_ref().map(|i| i.media_kind()),
            });
            if !recursive && let Some(ref ins) = matched_ins {
                raw_files.push((fname.to_string(), p.to_path_buf(), ins.clone()));
            }
        } else if matched_ins.is_some()
            && let Ok(rel_from_cur) = p.strip_prefix(&dir_sandbox.abs_path)
            && let Some(Component::Normal(top_os)) = rel_from_cur.components().next()
            && let Some(top_name) = top_os.to_str()
        {
            dir_counts.entry(top_name.to_string()).or_insert((0, 0)).1 += 1;
        }

        if recursive && let Some(ins) = matched_ins {
            let fname_owned = fname.to_string();
            raw_files.push((fname_owned.clone(), p.to_path_buf(), ins));
            let discovered = raw_files.len() as u64;
            if let Some(tx) = progress
                && discovered.is_multiple_of(100)
            {
                let _ = tx.try_send(WorkbenchProgress {
                    processed: discovered,
                    total: None,
                    detail: Some(fname_owned),
                });
            }
        }
    }

    folder_raw_entries.sort_by_key(|a| a.file_name.to_lowercase());
    raw_dirs.sort_by_key(|a| a.0.to_lowercase());
    raw_files.sort_by_key(|a| {
        a.1.strip_prefix(&root.path)
            .map(path_to_slash)
            .unwrap_or_else(|_| a.0.clone())
            .to_lowercase()
    });

    let folders = raw_dirs
        .into_iter()
        .map(|(dir_name, dir_abs)| {
            let rel = dir_abs
                .strip_prefix(&root.path)
                .map(path_to_slash)
                .unwrap_or_else(|_| dir_name.clone());
            let (child_dir_count, media_file_count) =
                dir_counts.get(&dir_name).copied().unwrap_or((0, 0));
            FolderEntryDto {
                name: dir_name,
                rel_path: rel,
                child_dir_count,
                media_file_count,
            }
        })
        .collect();

    Ok((
        current_rel,
        parent_rel,
        folders,
        folder_raw_entries,
        raw_files,
    ))
}

fn delete_entries_blocking(
    fs: &dyn WorkspaceFsPort,
    root: &ManagedRoot,
    rel_paths: &[String],
    delete_companions: bool,
    inspectors: &[Arc<dyn MediaInspectorPort>],
) -> DeleteFilesReportDto {
    let mut deleted_paths = Vec::new();
    let mut deleted_companions = Vec::new();
    let mut failed = Vec::new();
    let mut removed_set = HashSet::new();
    let mut removed_dirs: Vec<String> = Vec::new();

    let is_media_path =
        |p: &Path| -> bool { !p.is_dir() && inspectors.iter().any(|ins| ins.supports_path(p)) };

    for rel in rel_paths {
        let trimmed = rel.trim().trim_matches('/');
        if trimmed.is_empty() {
            continue;
        }
        let normalized_input = trimmed.replace('\\', "/");
        if removed_dirs
            .iter()
            .any(|dir| normalized_input == *dir || normalized_input.starts_with(&format!("{dir}/")))
        {
            continue;
        }
        let sandbox = match fs.resolve_existing_in_root(root, trimmed) {
            Ok(sb) => sb,
            Err(err) => {
                failed.push(DeleteFileErrorDto {
                    rel_path: rel.clone(),
                    error: err.to_string(),
                });
                continue;
            }
        };
        if sandbox.abs_path.is_dir() {
            let rel_slash = sandbox.rel_slash_string();
            if rel_slash.is_empty() || sandbox.abs_path == root.path {
                failed.push(DeleteFileErrorDto {
                    rel_path: rel.clone(),
                    error: "不允许删除媒体库根目录".to_string(),
                });
                continue;
            }
            if delete_companions {
                failed.push(DeleteFileErrorDto {
                    rel_path: rel.clone(),
                    error: "主视图模式不支持直接删除目录，请在文件夹视图中操作".to_string(),
                });
                continue;
            }
            if let Err(err) = std::fs::remove_dir_all(&sandbox.abs_path) {
                failed.push(DeleteFileErrorDto {
                    rel_path: rel.clone(),
                    error: format!("删除文件夹失败: {err}"),
                });
                continue;
            }
            removed_set.insert(sandbox.abs_path.clone());
            removed_dirs.push(rel_slash.clone());
            deleted_paths.push(rel_slash);
            continue;
        }
        if !sandbox.abs_path.is_file() {
            failed.push(DeleteFileErrorDto {
                rel_path: rel.clone(),
                error: "不支持删除此路径类型".to_string(),
            });
            continue;
        }

        let old_parent = sandbox.abs_path.parent().map(Path::to_path_buf);
        let companions: Vec<PathBuf> = if delete_companions {
            fs.find_companion_sidecars(&sandbox.abs_path)
                .into_iter()
                .map(|(p, _)| p)
                .filter(|p| {
                    let is_shared = p
                        .file_name()
                        .and_then(|s| s.to_str())
                        .is_some_and(is_shared_dir_sidecar_name);
                    !is_shared && !is_media_path(p)
                })
                .collect()
        } else {
            Vec::new()
        };

        if let Err(err) = std::fs::remove_file(&sandbox.abs_path) {
            failed.push(DeleteFileErrorDto {
                rel_path: rel.clone(),
                error: format!("删除文件失败: {err}"),
            });
            continue;
        }

        let rel_slash = sandbox.rel_slash_string();
        removed_set.insert(sandbox.abs_path.clone());
        deleted_paths.push(rel_slash);

        if delete_companions {
            for comp_abs in companions {
                if removed_set.contains(&comp_abs) || !comp_abs.is_file() {
                    continue;
                }
                if std::fs::remove_file(&comp_abs).is_ok() {
                    removed_set.insert(comp_abs.clone());
                    if let Ok(comp_rel) = comp_abs.strip_prefix(&root.path) {
                        deleted_companions.push(path_to_slash(comp_rel));
                    }
                }
            }
            if let Some(ref prev_dir) = old_parent {
                fs.cleanup_empty_parent_dirs(prev_dir, &root.path);
            }
        }
    }

    DeleteFilesReportDto {
        deleted_count: deleted_paths.len(),
        deleted_paths,
        deleted_companions,
        failed,
    }
}

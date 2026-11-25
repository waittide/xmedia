use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use walkdir::WalkDir;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;

use super::{MediaWorkbenchService, WorkbenchProgress, progress_step_for};
use crate::domain::media::{
    DeleteFileErrorDto, DeleteFilesReportDto, DirectorySnapshot, FolderEntryDto, MediaFileDetail,
    RawFileEntryDto,
};
use crate::domain::ports::MediaInspectorPort;
use crate::domain::workspace::{RootId, path_to_slash};

impl MediaWorkbenchService {
    pub async fn browse_directory(
        &self,
        root_id: RootId,
        rel_path: &str,
        domain_filter: Option<MediaKind>,
        recursive: bool,
    ) -> DomainResult<DirectorySnapshot> {
        self.browse_directory_with_progress(root_id, rel_path, domain_filter, recursive, |_| {})
            .await
    }

    pub async fn browse_directory_with_progress<F>(
        &self,
        root_id: RootId,
        rel_path: &str,
        domain_filter: Option<MediaKind>,
        recursive: bool,
        mut on_progress: F,
    ) -> DomainResult<DirectorySnapshot>
    where
        F: FnMut(WorkbenchProgress) + Send,
    {
        let raw_settings = self.settings.get_raw().await?;
        let root = raw_settings
            .roots
            .iter()
            .find(|r| r.id == root_id)
            .cloned()
            .ok_or_else(|| DomainError::NotFound(format!("root '{root_id}' not found")))?;
        let rename_templates = raw_settings.rename_templates;
        let rel_path_owned = rel_path.to_string();
        let root_for_scan = root.clone();
        let inspectors = self.inspectors.clone();
        let fs = Arc::clone(&self.fs);
        let effective_pref = domain_filter.or(root.default_kind);

        let (progress_tx, mut progress_rx) =
            tokio::sync::mpsc::unbounded_channel::<WorkbenchProgress>();

        let scan_handle = tokio::task::spawn_blocking(move || {
            let dir_sandbox = fs.resolve_existing_in_root(&root_for_scan, &rel_path_owned)?;
            if !dir_sandbox.abs_path.is_dir() {
                return Err(DomainError::Validation(format!(
                    "'{}' is not a directory",
                    dir_sandbox.rel_slash_string()
                )));
            }

            // Ensure unreadable directories surface an explicit storage error
            std::fs::read_dir(&dir_sandbox.abs_path).map_err(|e| {
                DomainError::Storage(format!(
                    "failed reading directory '{}': {e}",
                    dir_sandbox.abs_path.display()
                ))
            })?;

            let current_rel = dir_sandbox.rel_slash_string();
            let parent_rel = if current_rel.is_empty() {
                None
            } else {
                Some(
                    dir_sandbox
                        .rel_path
                        .parent()
                        .map(path_to_slash)
                        .unwrap_or_default(),
                )
            };

            let _ = progress_tx.send(WorkbenchProgress {
                processed: 0,
                total: None,
                detail: Some(if current_rel.is_empty() {
                    root_for_scan.name.clone()
                } else {
                    current_rel.clone()
                }),
            });

            let resolve_ins = |p: &Path| -> Option<Arc<dyn MediaInspectorPort>> {
                let ext = p.extension().and_then(|s| s.to_str())?.to_ascii_lowercase();
                if let Some(pref) = effective_pref
                    && let Some(matched) = inspectors
                        .iter()
                        .find(|ins| ins.media_kind() == pref && ins.supports_extension(&ext))
                {
                    return Some(Arc::clone(matched));
                }
                inspectors
                    .iter()
                    .find(|ins| ins.supports_extension(&ext))
                    .cloned()
            };

            let resolve_dir_ins = |p: &Path| -> Option<Arc<dyn MediaInspectorPort>> {
                if let Some(pref) = effective_pref
                    && let Some(matched) = inspectors
                        .iter()
                        .find(|ins| ins.media_kind() == pref && ins.supports_directory(p))
                {
                    return Some(Arc::clone(matched));
                }
                inspectors
                    .iter()
                    .find(|ins| ins.supports_directory(p))
                    .cloned()
            };

            let mut raw_dirs: Vec<(String, PathBuf)> = Vec::new();
            let mut dir_counts: HashMap<String, (usize, usize)> = HashMap::new();
            let mut raw_files: Vec<(String, PathBuf, Arc<dyn MediaInspectorPort>)> = Vec::new();
            let mut folder_raw_entries: Vec<RawFileEntryDto> = Vec::new();

            // Single O(N) WalkDir pass computing immediate folders, child_dir_count,
            // subtree media_file_count, immediate raw_files, and target media files.
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
                    let matched_dir_ins = resolve_dir_ins(p)
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
                        let fname_owned = fname.to_string();
                        raw_files.push((fname_owned, p.to_path_buf(), ins));
                    }
                    continue;
                }

                if !ft.is_file() {
                    continue;
                }

                let matched_ins = resolve_ins(p)
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
                        .strip_prefix(&root_for_scan.path)
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
                    if discovered.is_multiple_of(100) {
                        let _ = progress_tx.send(WorkbenchProgress {
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
                a.1.strip_prefix(&root_for_scan.path)
                    .map(path_to_slash)
                    .unwrap_or_else(|_| a.0.clone())
                    .to_lowercase()
            });

            let mut folders = Vec::with_capacity(raw_dirs.len());
            for (dir_name, dir_abs) in raw_dirs {
                let rel = dir_abs
                    .strip_prefix(&root_for_scan.path)
                    .map(path_to_slash)
                    .unwrap_or_else(|_| dir_name.clone());
                let (child_dir_count, media_file_count) =
                    dir_counts.get(&dir_name).copied().unwrap_or((0, 0));
                folders.push(FolderEntryDto {
                    name: dir_name,
                    rel_path: rel,
                    child_dir_count,
                    media_file_count,
                });
            }

            Ok((
                current_rel,
                parent_rel,
                folders,
                folder_raw_entries,
                raw_files,
            ))
        });

        while let Some(prog) = progress_rx.recv().await {
            on_progress(prog);
        }

        let (current_rel, parent_rel, folders, folder_raw_entries, raw_files) = scan_handle
            .await
            .map_err(|e| DomainError::Storage(format!("directory browse task failed: {e}")))??;

        let total_files = raw_files.len() as u64;
        let step = progress_step_for(total_files);
        on_progress(WorkbenchProgress {
            processed: 0,
            total: Some(total_files),
            detail: None,
        });

        let mut files = Vec::with_capacity(raw_files.len());
        for (idx, (fname, file_abs, inspector)) in raw_files.into_iter().enumerate() {
            let entry = self
                .build_file_entry(
                    &root,
                    &file_abs,
                    &fname,
                    inspector.as_ref(),
                    &rename_templates,
                )
                .await?;
            files.push(entry);
            let done = (idx + 1) as u64;
            if done.is_multiple_of(step) || done == total_files {
                on_progress(WorkbenchProgress {
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

    pub async fn inspect_file(
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
        let entry = self
            .build_file_entry(
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

    pub async fn delete_files(
        &self,
        root_id: RootId,
        rel_paths: &[String],
        delete_companions: bool,
    ) -> DomainResult<DeleteFilesReportDto> {
        let root = self.settings.find_root(root_id).await?;
        root.ensure_writable()?;
        let rel_paths_owned = rel_paths.to_vec();
        let inspectors = self.inspectors.clone();
        let fs = Arc::clone(&self.fs);

        tokio::task::spawn_blocking(move || {
            let mut deleted_paths = Vec::new();
            let mut deleted_companions = Vec::new();
            let mut failed = Vec::new();
            let mut removed_set = std::collections::HashSet::new();
            let mut removed_dirs: Vec<String> = Vec::new();

            let is_media_path = |p: &Path| -> bool {
                p.extension()
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_ascii_lowercase())
                    .is_some_and(|ext| inspectors.iter().any(|ins| ins.supports_extension(&ext)))
            };

            for rel in &rel_paths_owned {
                let trimmed = rel.trim().trim_matches('/');
                if trimmed.is_empty() {
                    continue;
                }
                let normalized_input = trimmed.replace('\\', "/");
                if removed_dirs.iter().any(|dir| {
                    normalized_input == *dir || normalized_input.starts_with(&format!("{dir}/"))
                }) {
                    continue;
                }
                let sandbox = match fs.resolve_existing_in_root(&root, trimmed) {
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
                            // Never delete directory-shared sidecars directly; only clean them if directory becomes empty
                            let is_shared = p
                                .file_name()
                                .and_then(|s| s.to_str())
                                .is_some_and(|n| fs.is_shared_dir_sidecar_name(n));
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

            Ok(DeleteFilesReportDto {
                deleted_count: deleted_paths.len(),
                deleted_paths,
                deleted_companions,
                failed,
            })
        })
        .await
        .map_err(|e| DomainError::Storage(format!("delete files task failed: {e}")))?
    }
}

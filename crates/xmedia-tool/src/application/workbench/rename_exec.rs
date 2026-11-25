use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};

use super::{
    MediaWorkbenchService, RenameHistoryBatchDto, RenameMoveRecord, UndoRenameReportDto,
    WorkbenchProgress, dir_has_any_media_file, progress_step_for,
};
use crate::domain::media::RenamePlanItem;
use crate::domain::workspace::RootId;

impl MediaWorkbenchService {
    pub async fn list_rename_history(&self, root_id: Option<RootId>) -> Vec<RenameHistoryBatchDto> {
        let guard = self.rename_history.read().await;
        guard
            .iter()
            .rev()
            .filter(|b| root_id.is_none_or(|rid| b.root_id == rid.0))
            .cloned()
            .collect()
    }

    pub async fn undo_rename_batch(&self, batch_id: Uuid) -> DomainResult<UndoRenameReportDto> {
        let batch = {
            let guard = self.rename_history.read().await;
            guard
                .iter()
                .find(|b| b.batch_id == batch_id)
                .cloned()
                .ok_or_else(|| {
                    DomainError::NotFound(format!("rename history batch {batch_id} not found"))
                })?
        };
        let root_lock = self.root_write_lock(RootId(batch.root_id));
        let _root_guard = root_lock.lock().await;
        let root = self.settings.find_root(RootId(batch.root_id)).await?;
        root.ensure_writable()?;
        let moves = batch.moves;
        let fs = Arc::clone(&self.fs);
        let (restored_count, skipped_conflicts, remaining_moves, restored_path_pairs) =
            tokio::task::spawn_blocking(move || {
                let mut restored_count = 0usize;
                let mut skipped_conflicts = Vec::new();
                let mut remaining_moves = Vec::new();
                let mut restored_path_pairs = Vec::new();

                for mv in moves.iter().rev() {
                    let Ok(cur) = fs.resolve_existing_in_root(&root, &mv.to_rel_path) else {
                        skipped_conflicts.push(mv.to_rel_path.clone());
                        remaining_moves.push(mv.clone());
                        continue;
                    };
                    let Ok(orig) = fs.resolve_target_in_root(&root, &mv.from_rel_path) else {
                        skipped_conflicts.push(mv.from_rel_path.clone());
                        remaining_moves.push(mv.clone());
                        continue;
                    };
                    if orig.exists() && orig != cur.abs_path {
                        skipped_conflicts.push(mv.from_rel_path.clone());
                        remaining_moves.push(mv.clone());
                        continue;
                    }
                    let cur_parent = cur.abs_path.parent().map(Path::to_path_buf);
                    if let Some(p) = orig.parent() {
                        let _ = std::fs::create_dir_all(p);
                    }
                    if std::fs::rename(&cur.abs_path, &orig).is_err() {
                        skipped_conflicts.push(mv.to_rel_path.clone());
                        remaining_moves.push(mv.clone());
                        continue;
                    }
                    restored_count += 1;
                    restored_path_pairs.push((mv.to_rel_path.clone(), mv.from_rel_path.clone()));

                    for (comp_from, comp_to) in mv.companion_moves.iter().rev() {
                        if let Ok(comp_cur) = fs.resolve_existing_in_root(&root, comp_to)
                            && let Ok(comp_orig) = fs.resolve_target_in_root(&root, comp_from)
                            && !comp_orig.exists()
                        {
                            if let Some(p) = comp_orig.parent() {
                                let _ = std::fs::create_dir_all(p);
                            }
                            if let Err(err) = std::fs::rename(&comp_cur.abs_path, &comp_orig) {
                                tracing::warn!(from = %comp_to, to = %comp_from, error = %err, "failed to restore companion file during rename undo");
                                skipped_conflicts.push(comp_to.clone());
                            }
                        }
                    }

                    if let Some(ref prev_dir) = cur_parent {
                        fs.cleanup_empty_parent_dirs(prev_dir, &root.path);
                    }
                }
                remaining_moves.reverse();
                (
                    restored_count,
                    skipped_conflicts,
                    remaining_moves,
                    restored_path_pairs,
                )
            })
            .await
            .map_err(|e| DomainError::Storage(format!("undo rename task failed: {e}")))?;

        if restored_count > 0 {
            self.update_metadata_history_paths(RootId(batch.root_id), &restored_path_pairs)
                .await;
            let mut guard = self.rename_history.write().await;
            if remaining_moves.is_empty() {
                guard.retain(|b| b.batch_id != batch_id);
            } else if let Some(existing) = guard.iter_mut().find(|b| b.batch_id == batch_id) {
                existing.moved_count = remaining_moves.len();
                existing.moves = remaining_moves;
            }
        }

        Ok(UndoRenameReportDto {
            batch_id,
            restored_count,
            skipped_conflicts,
        })
    }

    pub async fn execute_rename(
        &self,
        root_id: RootId,
        plans: &[RenamePlanItem],
    ) -> DomainResult<usize> {
        self.execute_rename_with_progress(root_id, plans, |_| {})
            .await
    }

    pub async fn execute_rename_with_progress<F>(
        &self,
        root_id: RootId,
        plans: &[RenamePlanItem],
        mut on_progress: F,
    ) -> DomainResult<usize>
    where
        F: FnMut(WorkbenchProgress) + Send,
    {
        let root_lock = self.root_write_lock(root_id);
        let _root_guard = root_lock.lock().await;
        let root = self.settings.find_root(root_id).await?;
        root.ensure_writable()?;
        let total = plans
            .iter()
            .filter(|p| !p.unchanged && p.conflict.is_none())
            .count() as u64;
        let step = progress_step_for(total);
        on_progress(WorkbenchProgress {
            processed: 0,
            total: Some(total),
            detail: None,
        });

        let plans_owned = plans.to_vec();
        let root_for_exec = root.clone();
        let inspectors = self.inspectors.clone();
        let fs = Arc::clone(&self.fs);
        let (progress_tx, mut progress_rx) =
            tokio::sync::mpsc::unbounded_channel::<WorkbenchProgress>();

        let exec_handle = tokio::task::spawn_blocking(move || {
            let mut moved_count = 0usize;
            let mut recorded_moves: Vec<RenameMoveRecord> = Vec::new();
            let mut deferred_shared_sidecars: Vec<(PathBuf, PathBuf)> = Vec::new();
            let mut old_parents_to_clean: Vec<PathBuf> = Vec::new();
            let mut first_err: Option<DomainError> = None;

            for item in &plans_owned {
                if item.unchanged || item.conflict.is_some() {
                    continue;
                }
                let src = match fs.resolve_existing_in_root(&root_for_exec, &item.from_rel_path) {
                    Ok(s) => s,
                    Err(e) => {
                        first_err = Some(e);
                        break;
                    }
                };
                let old_parent = src.abs_path.parent().map(Path::to_path_buf);
                let src_stem_lower = src
                    .abs_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                let dst = match fs.resolve_target_in_root(&root_for_exec, &item.to_rel_path) {
                    Ok(d) => d,
                    Err(e) => {
                        first_err = Some(e);
                        break;
                    }
                };
                let mut overwrite_backup: Option<PathBuf> = None;
                if dst.exists() && dst != src.abs_path {
                    if !item.will_overwrite {
                        first_err = Some(DomainError::Conflict(format!(
                            "目标文件已存在：'{}'",
                            item.to_rel_path
                        )));
                        break;
                    }
                    if dst.is_dir() {
                        first_err = Some(DomainError::Conflict(format!(
                            "目标路径是目录，无法覆盖：'{}'",
                            item.to_rel_path
                        )));
                        break;
                    }
                    let backup_path = dst.with_file_name(format!(
                        ".xmedia_overwrite_{}.bak",
                        Uuid::now_v7().simple()
                    ));
                    if let Err(e) = std::fs::rename(&dst, &backup_path) {
                        first_err = Some(DomainError::Storage(format!(
                            "无法暂存待覆盖文件 '{}': {e}",
                            item.to_rel_path
                        )));
                        break;
                    }
                    overwrite_backup = Some(backup_path);
                }
                if let Some(parent) = dst.parent()
                    && let Err(e) = std::fs::create_dir_all(parent)
                {
                    if let Some(bak) = overwrite_backup.take() {
                        let _ = std::fs::rename(&bak, &dst);
                    }
                    first_err = Some(DomainError::Storage(format!("failed creating folder: {e}")));
                    break;
                }
                if let Err(e) = std::fs::rename(&src.abs_path, &dst) {
                    if let Some(bak) = overwrite_backup.take() {
                        let _ = std::fs::rename(&bak, &dst);
                    }
                    first_err = Some(DomainError::Storage(format!(
                        "failed renaming '{}' -> '{}': {e}",
                        item.from_rel_path, item.to_rel_path
                    )));
                    break;
                }
                if let Some(bak) = overwrite_backup {
                    let _ = std::fs::remove_file(&bak);
                }
                moved_count += 1;

                let mut moved_companions: Vec<(String, String)> = Vec::new();
                for (comp_from, comp_to) in &item.companion_moves {
                    if let Ok(comp_src) = fs.resolve_existing_in_root(&root_for_exec, comp_from)
                        && let Ok(comp_dst) = fs.resolve_target_in_root(&root_for_exec, comp_to)
                    {
                        let comp_name = comp_src
                            .abs_path
                            .file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or_default();
                        let is_shared_dir_sidecar = fs.is_shared_dir_sidecar_name(comp_name);
                        let comp_parent = comp_src.abs_path.parent();
                        let same_dir_stem_match = old_parent.as_deref() == comp_parent
                            && !src_stem_lower.is_empty()
                            && comp_name.to_ascii_lowercase().starts_with(&src_stem_lower);
                        let ancestor_shared_match = is_shared_dir_sidecar
                            && old_parent
                                .as_deref()
                                .is_some_and(|op| comp_parent.is_some_and(|cp| op.starts_with(cp)));
                        if !same_dir_stem_match && !ancestor_shared_match {
                            tracing::warn!(
                                from = %comp_from,
                                media = %item.from_rel_path,
                                "ignoring unverified companion move outside media file scope"
                            );
                            continue;
                        }
                        if !comp_dst.exists()
                            || (item.will_overwrite
                                && comp_dst.is_file()
                                && comp_dst != comp_src.abs_path)
                        {
                            if let Some(p) = comp_dst.parent() {
                                let _ = std::fs::create_dir_all(p);
                            }
                            if comp_dst.exists()
                                && item.will_overwrite
                                && comp_dst != comp_src.abs_path
                            {
                                let _ = std::fs::remove_file(&comp_dst);
                            }
                            if is_shared_dir_sidecar {
                                if std::fs::copy(&comp_src.abs_path, &comp_dst).is_ok() {
                                    deferred_shared_sidecars
                                        .push((comp_src.abs_path.clone(), comp_dst));
                                }
                            } else if std::fs::rename(&comp_src.abs_path, &comp_dst).is_ok() {
                                moved_companions.push((comp_from.clone(), comp_to.clone()));
                            }
                        } else if is_shared_dir_sidecar {
                            deferred_shared_sidecars.push((comp_src.abs_path.clone(), comp_dst));
                        }
                    }
                }

                if let Some(prev_dir) = old_parent {
                    fs.cleanup_empty_parent_dirs(&prev_dir, &root_for_exec.path);
                    if !old_parents_to_clean.contains(&prev_dir) {
                        old_parents_to_clean.push(prev_dir);
                    }
                }

                recorded_moves.push(RenameMoveRecord {
                    from_rel_path: item.from_rel_path.clone(),
                    to_rel_path: item.to_rel_path.clone(),
                    companion_moves: moved_companions,
                });

                let done = moved_count as u64;
                if done.is_multiple_of(step) || done == total {
                    let _ = progress_tx.send(WorkbenchProgress {
                        processed: done,
                        total: Some(total),
                        detail: Some(item.to_rel_path.clone()),
                    });
                }
            }

            let mut empty_media_dir_cache: std::collections::HashMap<PathBuf, bool> =
                std::collections::HashMap::new();
            for (src_sidecar, dst_sidecar) in deferred_shared_sidecars {
                if src_sidecar == dst_sidecar || !src_sidecar.is_file() || !dst_sidecar.is_file() {
                    continue;
                }
                if let Some(dir) = src_sidecar.parent() {
                    let has_media = *empty_media_dir_cache
                        .entry(dir.to_path_buf())
                        .or_insert_with(|| dir_has_any_media_file(dir, &inspectors));
                    if !has_media {
                        let _ = std::fs::remove_file(&src_sidecar);
                        fs.cleanup_empty_parent_dirs(dir, &root_for_exec.path);
                    }
                }
            }
            for prev_dir in old_parents_to_clean {
                fs.cleanup_empty_parent_dirs(&prev_dir, &root_for_exec.path);
            }

            (moved_count, recorded_moves, first_err)
        });

        while let Some(prog) = progress_rx.recv().await {
            on_progress(prog);
        }

        let (moved_count, recorded_moves, first_err) = exec_handle
            .await
            .map_err(|e| DomainError::Storage(format!("execute rename task failed: {e}")))?;

        if !recorded_moves.is_empty() {
            let path_pairs: Vec<(String, String)> = recorded_moves
                .iter()
                .map(|m| (m.from_rel_path.clone(), m.to_rel_path.clone()))
                .collect();
            self.update_metadata_history_paths(root_id, &path_pairs)
                .await;
            let now_unix = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let mut guard = self.rename_history.write().await;
            guard.push(RenameHistoryBatchDto {
                batch_id: Uuid::new_v4(),
                root_id: root_id.0,
                created_at_unix: now_unix,
                moved_count,
                moves: recorded_moves,
            });
            if guard.len() > 30 {
                let drop_n = guard.len() - 30;
                guard.drain(0..drop_n);
            }
            if let Ok(settings) = self.settings.get_raw().await
                && settings.xmedia_webhook.notify_on_rename
            {
                let webhook_cfg = settings.xmedia_webhook;
                let root_path = root.path.clone();
                let webhook = Arc::clone(&self.webhook);
                tokio::spawn(async move {
                    webhook.notify_rescan(&webhook_cfg, &root_path).await;
                });
            }
        }

        if let Some(err) = first_err {
            return Err(err);
        }

        Ok(moved_count)
    }
}

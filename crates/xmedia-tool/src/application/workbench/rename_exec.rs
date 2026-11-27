use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::mpsc::Sender;
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};

use super::browse::WorkspaceBrowserService;
use super::metadata::MetadataHistoryEvent;
use super::rename::RenameService;
use super::{
    RenameHistoryBatchDto, RenameMoveRecord, UndoRenameReportDto, WorkbenchProgress,
    is_same_physical_file, progress_step_for, rename_path_preserving_case,
};
use crate::application::ports::WorkspaceFsPort;
use crate::domain::media::RenamePlanItem;
use crate::domain::workspace::{ManagedRoot, RootId, is_shared_dir_sidecar_name};

impl RenameService {
    pub async fn list_history(&self, root_id: Option<RootId>) -> Vec<RenameHistoryBatchDto> {
        let guard = self.history.read().await;
        guard
            .iter()
            .rev()
            .filter(|b| root_id.is_none_or(|rid| b.root_id == rid.0))
            .cloned()
            .collect()
    }

    pub async fn undo_batch(&self, batch_id: Uuid) -> DomainResult<UndoRenameReportDto> {
        let batch = {
            let guard = self.history.read().await;
            guard
                .iter()
                .find(|b| b.batch_id == batch_id)
                .cloned()
                .ok_or_else(|| {
                    DomainError::NotFound(format!("rename history batch {batch_id} not found"))
                })?
        };
        let root_lock = self.root_locks.lock_for(RootId(batch.root_id));
        let _root_guard = root_lock.lock().await;
        let root = self
            .settings
            .get_raw()
            .await?
            .find_root(RootId(batch.root_id))?
            .clone();
        root.ensure_writable()?;
        let moves = batch.moves;
        let fs = Arc::clone(&self.fs);
        let outcome =
            tokio::task::spawn_blocking(move || undo_moves_blocking(fs.as_ref(), &root, &moves))
                .await
                .map_err(|e| DomainError::Storage(format!("undo rename task failed: {e}")))?;

        if outcome.restored_count > 0 {
            self.metadata
                .record_history_event(MetadataHistoryEvent::Relocated {
                    root_id: RootId(batch.root_id),
                    moves: outcome.restored_path_pairs,
                })
                .await;
            let mut guard = self.history.write().await;
            if outcome.remaining_moves.is_empty() {
                guard.retain(|b| b.batch_id != batch_id);
            } else if let Some(existing) = guard.iter_mut().find(|b| b.batch_id == batch_id) {
                existing.moved_count = outcome.remaining_moves.len();
                existing.moves = outcome.remaining_moves;
            }
        }

        Ok(UndoRenameReportDto {
            batch_id,
            restored_count: outcome.restored_count,
            skipped_conflicts: outcome.skipped_conflicts,
        })
    }

    pub async fn execute(
        &self,
        root_id: RootId,
        plans: &[RenamePlanItem],
        progress: Option<Sender<WorkbenchProgress>>,
    ) -> DomainResult<usize> {
        let root_lock = self.root_locks.lock_for(root_id);
        let _root_guard = root_lock.lock().await;
        let root = self.settings.get_raw().await?.find_root(root_id)?.clone();
        root.ensure_writable()?;
        let total = plans
            .iter()
            .filter(|p| !p.unchanged && p.conflict.is_none())
            .count() as u64;
        let step = progress_step_for(total);
        if let Some(ref tx) = progress {
            let _ = tx.try_send(WorkbenchProgress {
                processed: 0,
                total: Some(total),
                detail: None,
            });
        }

        let plans_owned = plans.to_vec();
        let root_for_exec = root.clone();
        let workspace_for_exec = self.workspace.clone();
        let fs = Arc::clone(&self.fs);

        let (moved_count, recorded_moves, first_err) = tokio::task::spawn_blocking(move || {
            execute_plans_blocking(
                fs.as_ref(),
                &root_for_exec,
                &workspace_for_exec,
                &plans_owned,
                total,
                step,
                progress.as_ref(),
            )
        })
        .await
        .map_err(|e| DomainError::Storage(format!("execute rename task failed: {e}")))?;

        if !recorded_moves.is_empty() {
            let path_pairs: Vec<(String, String)> = recorded_moves
                .iter()
                .map(|m| (m.from_rel_path.clone(), m.to_rel_path.clone()))
                .collect();
            self.metadata
                .record_history_event(MetadataHistoryEvent::Relocated {
                    root_id,
                    moves: path_pairs,
                })
                .await;
            let now_unix = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let mut guard = self.history.write().await;
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

struct UndoMovesOutcome {
    restored_count: usize,
    skipped_conflicts: Vec<String>,
    remaining_moves: Vec<RenameMoveRecord>,
    restored_path_pairs: Vec<(String, String)>,
}

fn undo_moves_blocking(
    fs: &dyn WorkspaceFsPort,
    root: &ManagedRoot,
    moves: &[RenameMoveRecord],
) -> UndoMovesOutcome {
    let mut restored_count = 0usize;
    let mut skipped_conflicts = Vec::new();
    let mut remaining_moves = Vec::new();
    let mut restored_path_pairs = Vec::new();

    for mv in moves.iter().rev() {
        let Ok(cur) = fs.resolve_existing_in_root(root, &mv.to_rel_path) else {
            skipped_conflicts.push(mv.to_rel_path.clone());
            remaining_moves.push(mv.clone());
            continue;
        };
        let Ok(orig) = fs.resolve_target_in_root(root, &mv.from_rel_path) else {
            skipped_conflicts.push(mv.from_rel_path.clone());
            remaining_moves.push(mv.clone());
            continue;
        };
        if orig.exists() && !is_same_physical_file(&orig, &cur.abs_path) {
            skipped_conflicts.push(mv.from_rel_path.clone());
            remaining_moves.push(mv.clone());
            continue;
        }
        let cur_parent = cur.abs_path.parent().map(Path::to_path_buf);
        if let Some(p) = orig.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        if rename_path_preserving_case(&cur.abs_path, &orig).is_err() {
            skipped_conflicts.push(mv.to_rel_path.clone());
            remaining_moves.push(mv.clone());
            continue;
        }
        restored_count += 1;
        restored_path_pairs.push((mv.to_rel_path.clone(), mv.from_rel_path.clone()));

        for (comp_from, comp_to) in mv.companion_moves.iter().rev() {
            if let Ok(comp_cur) = fs.resolve_existing_in_root(root, comp_to)
                && let Ok(comp_orig) = fs.resolve_target_in_root(root, comp_from)
                && (!comp_orig.exists() || is_same_physical_file(&comp_orig, &comp_cur.abs_path))
            {
                if let Some(p) = comp_orig.parent() {
                    let _ = std::fs::create_dir_all(p);
                }
                if let Err(err) = rename_path_preserving_case(&comp_cur.abs_path, &comp_orig) {
                    tracing::warn!(
                        from = %comp_to,
                        to = %comp_from,
                        error = %err,
                        "failed to restore companion file during rename undo"
                    );
                    skipped_conflicts.push(comp_to.clone());
                }
            }
        }

        if let Some(ref prev_dir) = cur_parent {
            fs.cleanup_empty_parent_dirs(prev_dir, &root.path);
        }
    }
    remaining_moves.reverse();
    UndoMovesOutcome {
        restored_count,
        skipped_conflicts,
        remaining_moves,
        restored_path_pairs,
    }
}

fn execute_plans_blocking(
    fs: &dyn WorkspaceFsPort,
    root: &ManagedRoot,
    workspace: &WorkspaceBrowserService,
    plans: &[RenamePlanItem],
    total: u64,
    step: u64,
    progress: Option<&Sender<WorkbenchProgress>>,
) -> (usize, Vec<RenameMoveRecord>, Option<DomainError>) {
    let mut moved_count = 0usize;
    let mut recorded_moves: Vec<RenameMoveRecord> = Vec::new();
    let mut deferred_shared_sidecars: Vec<(PathBuf, PathBuf)> = Vec::new();
    let mut old_parents_to_clean: Vec<PathBuf> = Vec::new();
    let mut first_err: Option<DomainError> = None;

    for item in plans {
        if item.unchanged || item.conflict.is_some() {
            continue;
        }
        let src = match fs.resolve_existing_in_root(root, &item.from_rel_path) {
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
        let dst = match fs.resolve_target_in_root(root, &item.to_rel_path) {
            Ok(d) => d,
            Err(e) => {
                first_err = Some(e);
                break;
            }
        };
        let mut overwrite_backup: Option<PathBuf> = None;
        if dst.exists() && !is_same_physical_file(&dst, &src.abs_path) {
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
            let backup_path =
                dst.with_file_name(format!(".xmedia_overwrite_{}.bak", Uuid::now_v7().simple()));
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
        if let Err(e) = rename_path_preserving_case(&src.abs_path, &dst) {
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
            if let Ok(comp_src) = fs.resolve_existing_in_root(root, comp_from)
                && let Ok(comp_dst) = fs.resolve_target_in_root(root, comp_to)
            {
                let comp_name = comp_src
                    .abs_path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default();
                let is_shared_sidecar = is_shared_dir_sidecar_name(comp_name);
                let comp_parent = comp_src.abs_path.parent();
                let same_dir_stem_match = old_parent.as_deref() == comp_parent
                    && !src_stem_lower.is_empty()
                    && comp_name.to_ascii_lowercase().starts_with(&src_stem_lower);
                let ancestor_shared_match = is_shared_sidecar
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
                let same_comp_file = is_same_physical_file(&comp_dst, &comp_src.abs_path);
                if !comp_dst.exists()
                    || same_comp_file
                    || (item.will_overwrite && comp_dst.is_file())
                {
                    if let Some(p) = comp_dst.parent() {
                        let _ = std::fs::create_dir_all(p);
                    }
                    if comp_dst.exists() && item.will_overwrite && !same_comp_file {
                        let _ = std::fs::remove_file(&comp_dst);
                    }
                    if is_shared_sidecar {
                        if same_comp_file {
                            if rename_path_preserving_case(&comp_src.abs_path, &comp_dst).is_ok() {
                                moved_companions.push((comp_from.clone(), comp_to.clone()));
                            }
                        } else if std::fs::copy(&comp_src.abs_path, &comp_dst).is_ok() {
                            deferred_shared_sidecars.push((comp_src.abs_path.clone(), comp_dst));
                        }
                    } else if rename_path_preserving_case(&comp_src.abs_path, &comp_dst).is_ok() {
                        moved_companions.push((comp_from.clone(), comp_to.clone()));
                    }
                } else if is_shared_sidecar {
                    deferred_shared_sidecars.push((comp_src.abs_path.clone(), comp_dst));
                }
            }
        }

        if let Some(prev_dir) = old_parent {
            fs.cleanup_empty_parent_dirs(&prev_dir, &root.path);
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
        if let Some(tx) = progress
            && (done.is_multiple_of(step) || done == total)
        {
            let _ = tx.try_send(WorkbenchProgress {
                processed: done,
                total: Some(total),
                detail: Some(item.to_rel_path.clone()),
            });
        }
    }

    let mut empty_media_dir_cache: HashMap<PathBuf, bool> = HashMap::new();
    for (src_sidecar, dst_sidecar) in deferred_shared_sidecars {
        if is_same_physical_file(&src_sidecar, &dst_sidecar)
            || !src_sidecar.is_file()
            || !dst_sidecar.is_file()
        {
            continue;
        }
        if let Some(dir) = src_sidecar.parent() {
            let has_media = *empty_media_dir_cache
                .entry(dir.to_path_buf())
                .or_insert_with(|| workspace.dir_has_any_media(dir));
            if !has_media {
                let _ = std::fs::remove_file(&src_sidecar);
                fs.cleanup_empty_parent_dirs(dir, &root.path);
            }
        }
    }
    for prev_dir in old_parents_to_clean {
        fs.cleanup_empty_parent_dirs(&prev_dir, &root.path);
    }

    (moved_count, recorded_moves, first_err)
}

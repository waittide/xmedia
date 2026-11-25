use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;

use super::{
    MediaWorkbenchService, PreviewRenameOptions, RenameHistoryBatchDto, RenameMoveRecord,
    UndoRenameReportDto, WorkbenchProgress, count_subtree_children, progress_step_for,
};
use crate::application::webhook::notify_xmedia_rescan;
use crate::domain::media::{MediaMetadataPayload, RenameConflictPolicy, RenamePlanItem};
use crate::domain::template::render_template_target;
use crate::domain::workspace::{RootId, path_to_slash};
use crate::infra::atomic_fs::{
    cleanup_empty_parent_dirs, find_companion_sidecars, is_shared_dir_sidecar_name,
};
use crate::infra::inspectors::book::dir_has_single_book;
use crate::infra::inspectors::comic::is_generic_comic_folder;
use crate::infra::inspectors::music::is_generic_music_folder;
use crate::infra::inspectors::video::dir_has_single_video;
use crate::infra::workspace_fs::{resolve_existing_in_root, resolve_target_in_root};

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
        let mut restored_count = 0usize;
        let mut skipped_conflicts = Vec::new();
        let mut remaining_moves = Vec::new();
        let mut restored_path_pairs = Vec::new();

        for mv in batch.moves.iter().rev() {
            let Ok(cur) = resolve_existing_in_root(&root, &mv.to_rel_path) else {
                skipped_conflicts.push(mv.to_rel_path.clone());
                remaining_moves.push(mv.clone());
                continue;
            };
            let Ok(orig) = resolve_target_in_root(&root, &mv.from_rel_path) else {
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
                if let Ok(comp_cur) = resolve_existing_in_root(&root, comp_to)
                    && let Ok(comp_orig) = resolve_target_in_root(&root, comp_from)
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
                cleanup_empty_parent_dirs(prev_dir, &root.path);
            }
        }
        remaining_moves.reverse();

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

    pub async fn preview_rename(
        &self,
        root_id: RootId,
        rel_paths: &[String],
        template_override: Option<&str>,
        episode_template_override: Option<&str>,
        preferred_kind: Option<MediaKind>,
    ) -> DomainResult<Vec<RenamePlanItem>> {
        self.preview_rename_with_options(
            root_id,
            rel_paths,
            PreviewRenameOptions {
                template_override,
                episode_template_override,
                preferred_kind,
                conflict_policy: RenameConflictPolicy::Skip,
            },
        )
        .await
    }

    pub async fn preview_rename_with_options(
        &self,
        root_id: RootId,
        rel_paths: &[String],
        options: PreviewRenameOptions<'_>,
    ) -> DomainResult<Vec<RenamePlanItem>> {
        self.preview_rename_with_progress(root_id, rel_paths, options, |_| {})
            .await
    }

    pub async fn preview_rename_with_progress<F>(
        &self,
        root_id: RootId,
        rel_paths: &[String],
        options: PreviewRenameOptions<'_>,
        mut on_progress: F,
    ) -> DomainResult<Vec<RenamePlanItem>>
    where
        F: FnMut(WorkbenchProgress) + Send,
    {
        let template_override = options.template_override;
        let episode_template_override = options.episode_template_override;
        let preferred_kind = options.preferred_kind;
        let conflict_policy = options.conflict_policy;
        let raw_settings = self.settings.get_raw().await?;
        let root = raw_settings
            .roots
            .iter()
            .find(|r| r.id == root_id)
            .cloned()
            .ok_or_else(|| DomainError::NotFound(format!("root '{root_id}' not found")))?;

        let mut plans = Vec::with_capacity(rel_paths.len());
        let mut planned_targets = std::collections::HashSet::new();

        let clean_primary_tpl = template_override.map(str::trim).filter(|s| !s.is_empty());
        let clean_ep_tpl = episode_template_override
            .map(str::trim)
            .filter(|s| !s.is_empty());

        let total = rel_paths.len() as u64;
        let step = progress_step_for(total);
        on_progress(WorkbenchProgress {
            processed: 0,
            total: Some(total),
            detail: None,
        });

        let effective_pref = preferred_kind.or(root.default_kind);
        for (idx, rel) in rel_paths.iter().enumerate() {
            let sandbox = resolve_existing_in_root(&root, rel)?;
            let Some(inspector) =
                self.resolve_inspector_for_path(&sandbox.abs_path, effective_pref)
            else {
                continue;
            };
            let meta = inspector.inspect_file(&sandbox.abs_path).await?;
            let ext = sandbox
                .abs_path
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or_default();

            let tpl = match &meta {
                MediaMetadataPayload::Book(_) => clean_primary_tpl
                    .map(str::to_owned)
                    .unwrap_or_else(|| raw_settings.rename_templates.book.clone()),
                MediaMetadataPayload::Comic(_) => clean_primary_tpl
                    .map(str::to_owned)
                    .unwrap_or_else(|| raw_settings.rename_templates.comic.clone()),
                MediaMetadataPayload::Music(_) => clean_primary_tpl
                    .map(str::to_owned)
                    .unwrap_or_else(|| raw_settings.rename_templates.music.clone()),
                MediaMetadataPayload::Video(v) => {
                    if v.is_episode {
                        clean_ep_tpl
                            .or(if episode_template_override.is_none() {
                                clean_primary_tpl
                            } else {
                                None
                            })
                            .map(str::to_owned)
                            .unwrap_or_else(|| raw_settings.rename_templates.video_episode.clone())
                    } else {
                        clean_primary_tpl
                            .map(str::to_owned)
                            .unwrap_or_else(|| raw_settings.rename_templates.video_movie.clone())
                    }
                }
            };

            let parent_rel = sandbox
                .rel_path
                .parent()
                .map(path_to_slash)
                .unwrap_or_default();
            let (mut to_rel, mut rendered_stem) =
                render_template_target(&tpl, &parent_rel, &meta, &sandbox.abs_path, ext);
            let target_parent_rel = Path::new(&to_rel)
                .parent()
                .map(path_to_slash)
                .unwrap_or_default();

            let from_rel = sandbox.rel_slash_string();
            let mut unchanged = from_rel == to_rel;

            let target_abs = resolve_target_in_root(&root, &to_rel)?;
            let mut conflict = None;
            let mut will_overwrite = false;
            if let MediaMetadataPayload::Video(ref v) = meta
                && v.is_episode
                && v.episode_number.is_none()
                && tpl.to_ascii_lowercase().contains("{episode")
            {
                conflict = Some("剧集模板缺少集号".to_string());
            } else if !unchanged {
                let exists_on_disk = target_abs.exists() && target_abs != sandbox.abs_path;
                let duplicate_in_batch = planned_targets.contains(&to_rel.to_lowercase());
                match conflict_policy {
                    RenameConflictPolicy::Skip => {
                        if exists_on_disk {
                            conflict = Some("目标文件已存在".to_string());
                        } else if duplicate_in_batch {
                            conflict = Some("批次内目标文件名重复".to_string());
                        } else {
                            planned_targets.insert(to_rel.to_lowercase());
                        }
                    }
                    RenameConflictPolicy::AutoSuffix => {
                        if exists_on_disk || duplicate_in_batch {
                            let mut resolved = false;
                            for suffix_idx in 2..=999usize {
                                let cand_stem = format!("{rendered_stem} ({suffix_idx})");
                                let cand_name = if ext.is_empty() {
                                    cand_stem.clone()
                                } else {
                                    format!("{cand_stem}.{ext}")
                                };
                                let cand_rel = if target_parent_rel.is_empty() {
                                    cand_name
                                } else {
                                    format!("{target_parent_rel}/{cand_name}")
                                };
                                let cand_abs = resolve_target_in_root(&root, &cand_rel)?;
                                let cand_exists = cand_abs.exists() && cand_abs != sandbox.abs_path;
                                let cand_dup = planned_targets.contains(&cand_rel.to_lowercase());
                                if !cand_exists && !cand_dup {
                                    to_rel = cand_rel;
                                    rendered_stem = cand_stem;
                                    unchanged = from_rel == to_rel;
                                    planned_targets.insert(to_rel.to_lowercase());
                                    resolved = true;
                                    break;
                                }
                            }
                            if !resolved {
                                conflict = Some("无法生成不冲突的目标文件名".to_string());
                            }
                        } else {
                            planned_targets.insert(to_rel.to_lowercase());
                        }
                    }
                    RenameConflictPolicy::Overwrite => {
                        if duplicate_in_batch {
                            conflict = Some("批次内目标文件名重复".to_string());
                        } else {
                            if exists_on_disk {
                                will_overwrite = true;
                            }
                            planned_targets.insert(to_rel.to_lowercase());
                        }
                    }
                }
            }

            let mut companion_moves = Vec::new();
            if !unchanged {
                for (comp_abs, suffix) in find_companion_sidecars(&sandbox.abs_path) {
                    if let Ok(comp_rel) = comp_abs.strip_prefix(&root.path) {
                        let comp_from = path_to_slash(comp_rel);
                        let next_comp_name = format!("{rendered_stem}{suffix}");
                        let comp_to = if target_parent_rel.is_empty() {
                            next_comp_name
                        } else {
                            format!("{target_parent_rel}/{next_comp_name}")
                        };
                        companion_moves.push((comp_from, comp_to));
                    }
                }

                // Carry Calibre single-book `metadata.opf` and `cover.*` when moving a dedicated book folder
                if matches!(meta, MediaMetadataPayload::Book(_))
                    && parent_rel != target_parent_rel
                    && !target_parent_rel.is_empty()
                    && let Some(old_dir) = sandbox.abs_path.parent()
                    && old_dir != root.path
                    && dir_has_single_book(old_dir)
                {
                    for fname in [
                        "metadata.opf",
                        "cover.jpg",
                        "cover.jpeg",
                        "cover.png",
                        "cover.webp",
                    ] {
                        let cand = old_dir.join(fname);
                        if cand.is_file()
                            && let Ok(rel_c) = cand.strip_prefix(&root.path)
                        {
                            companion_moves.push((
                                path_to_slash(rel_c),
                                format!("{target_parent_rel}/{fname}"),
                            ));
                        }
                    }
                }

                // Carry Navidrome/Picard album-level cover (`cover.*`, `folder.*`, `front.*`) when moving music folders
                if matches!(meta, MediaMetadataPayload::Music(_))
                    && parent_rel != target_parent_rel
                    && !target_parent_rel.is_empty()
                    && let Some(old_dir) = sandbox.abs_path.parent()
                    && old_dir != root.path
                    && old_dir
                        .file_name()
                        .and_then(|s| s.to_str())
                        .is_some_and(|n| !is_generic_music_folder(n))
                {
                    for fname in [
                        "cover.jpg",
                        "cover.jpeg",
                        "cover.png",
                        "cover.webp",
                        "folder.jpg",
                        "folder.jpeg",
                        "folder.png",
                        "folder.webp",
                        "front.jpg",
                        "front.jpeg",
                        "front.png",
                        "front.webp",
                    ] {
                        let cand = old_dir.join(fname);
                        if cand.is_file()
                            && let Ok(rel_c) = cand.strip_prefix(&root.path)
                        {
                            companion_moves.push((
                                path_to_slash(rel_c),
                                format!("{target_parent_rel}/{fname}"),
                            ));
                        }
                    }
                }

                // Carry Komga/Kavita series-level cover (`cover.*`, `folder.*`) when moving comic series folders
                if matches!(meta, MediaMetadataPayload::Comic(_))
                    && parent_rel != target_parent_rel
                    && !target_parent_rel.is_empty()
                    && let Some(old_dir) = sandbox.abs_path.parent()
                    && old_dir != root.path
                    && old_dir
                        .file_name()
                        .and_then(|s| s.to_str())
                        .is_some_and(|n| !is_generic_comic_folder(n))
                {
                    for fname in [
                        "cover.jpg",
                        "cover.jpeg",
                        "cover.png",
                        "cover.webp",
                        "folder.jpg",
                        "folder.jpeg",
                        "folder.png",
                        "folder.webp",
                    ] {
                        let cand = old_dir.join(fname);
                        if cand.is_file()
                            && let Ok(rel_c) = cand.strip_prefix(&root.path)
                        {
                            companion_moves.push((
                                path_to_slash(rel_c),
                                format!("{target_parent_rel}/{fname}"),
                            ));
                        }
                    }
                }

                // Also carry Jellyfin directory-level NFO & artwork (`movie.nfo`, `season.nfo`, `tvshow.nfo`, `poster.*`, `fanart.*`)
                if let MediaMetadataPayload::Video(ref v) = meta
                    && parent_rel != target_parent_rel
                    && !target_parent_rel.is_empty()
                    && let Some(old_dir) = sandbox.abs_path.parent()
                    && old_dir != root.path
                {
                    let dir_files: &[&str] = if v.is_episode {
                        &[
                            "season.nfo",
                            "poster.jpg",
                            "poster.jpeg",
                            "poster.png",
                            "poster.webp",
                            "fanart.jpg",
                            "fanart.jpeg",
                            "fanart.png",
                            "fanart.webp",
                        ]
                    } else if dir_has_single_video(old_dir) {
                        &[
                            "movie.nfo",
                            "poster.jpg",
                            "poster.jpeg",
                            "poster.png",
                            "poster.webp",
                            "fanart.jpg",
                            "fanart.jpeg",
                            "fanart.png",
                            "fanart.webp",
                        ]
                    } else {
                        &[]
                    };
                    for fname in dir_files {
                        let cand = old_dir.join(fname);
                        if cand.is_file()
                            && let Ok(rel_c) = cand.strip_prefix(&root.path)
                        {
                            companion_moves.push((
                                path_to_slash(rel_c),
                                format!("{target_parent_rel}/{fname}"),
                            ));
                        }
                    }

                    if v.is_episode
                        && let Some(old_show_dir) = old_dir.parent()
                        && old_show_dir != root.path
                        && old_show_dir.starts_with(&root.path)
                        && let Some(new_show_parent) = Path::new(&target_parent_rel).parent()
                    {
                        let new_show_rel = path_to_slash(new_show_parent);
                        if !new_show_rel.is_empty() {
                            for fname in [
                                "tvshow.nfo",
                                "poster.jpg",
                                "poster.jpeg",
                                "poster.png",
                                "poster.webp",
                                "fanart.jpg",
                                "fanart.jpeg",
                                "fanart.png",
                                "fanart.webp",
                            ] {
                                let cand = old_show_dir.join(fname);
                                if cand.is_file()
                                    && let Ok(rel_c) = cand.strip_prefix(&root.path)
                                {
                                    companion_moves.push((
                                        path_to_slash(rel_c),
                                        format!("{new_show_rel}/{fname}"),
                                    ));
                                }
                            }
                        }
                    }
                }
            }

            plans.push(RenamePlanItem {
                from_rel_path: from_rel,
                to_rel_path: to_rel,
                companion_moves,
                conflict,
                unchanged,
                will_overwrite,
            });

            let done = (idx + 1) as u64;
            if done.is_multiple_of(step) || done == total {
                on_progress(WorkbenchProgress {
                    processed: done,
                    total: Some(total),
                    detail: Some(rel.clone()),
                });
            }
        }

        Ok(plans)
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
        let mut moved_count = 0usize;
        let mut recorded_moves: Vec<RenameMoveRecord> = Vec::new();
        let mut first_err: Option<DomainError> = None;

        for item in plans {
            if item.unchanged || item.conflict.is_some() {
                continue;
            }
            let src = match resolve_existing_in_root(&root, &item.from_rel_path) {
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
            let dst = match resolve_target_in_root(&root, &item.to_rel_path) {
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
                let backup_path = dst
                    .with_file_name(format!(".xmedia_overwrite_{}.bak", Uuid::now_v7().simple()));
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
            let mut migrated_shared_dirs: Vec<(PathBuf, PathBuf)> = Vec::new();
            for (comp_from, comp_to) in &item.companion_moves {
                if let Ok(comp_src) = resolve_existing_in_root(&root, comp_from)
                    && let Ok(comp_dst) = resolve_target_in_root(&root, comp_to)
                {
                    let comp_name = comp_src
                        .abs_path
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or_default();
                    let is_shared_dir_sidecar = is_shared_dir_sidecar_name(comp_name);
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
                        if comp_dst.exists() && item.will_overwrite && comp_dst != comp_src.abs_path
                        {
                            let _ = std::fs::remove_file(&comp_dst);
                        }
                        if is_shared_dir_sidecar {
                            if std::fs::copy(&comp_src.abs_path, &comp_dst).is_ok() {
                                migrated_shared_dirs
                                    .push((comp_src.abs_path.clone(), comp_dst.clone()));
                            }
                        } else if std::fs::rename(&comp_src.abs_path, &comp_dst).is_ok() {
                            moved_companions.push((comp_from.clone(), comp_to.clone()));
                        }
                    } else if is_shared_dir_sidecar {
                        migrated_shared_dirs.push((comp_src.abs_path.clone(), comp_dst));
                    }
                }
            }

            for (src_sidecar, dst_sidecar) in migrated_shared_dirs {
                if dst_sidecar.is_file()
                    && src_sidecar != dst_sidecar
                    && let Some(dir) = src_sidecar.parent()
                    && count_subtree_children(dir, &self.inspectors, None).1 == 0
                {
                    let _ = std::fs::remove_file(&src_sidecar);
                }
            }

            if let Some(ref prev_dir) = old_parent {
                cleanup_empty_parent_dirs(prev_dir, &root.path);
            }

            recorded_moves.push(RenameMoveRecord {
                from_rel_path: item.from_rel_path.clone(),
                to_rel_path: item.to_rel_path.clone(),
                companion_moves: moved_companions,
            });

            let done = moved_count as u64;
            if done.is_multiple_of(step) || done == total {
                on_progress(WorkbenchProgress {
                    processed: done,
                    total: Some(total),
                    detail: Some(item.to_rel_path.clone()),
                });
            }
        }

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
                tokio::spawn(async move {
                    notify_xmedia_rescan(&webhook_cfg, &root_path).await;
                });
            }
        }

        if let Some(err) = first_err {
            return Err(err);
        }

        Ok(moved_count)
    }
}

use std::path::Path;
use std::sync::Arc;

use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;

use super::{MediaWorkbenchService, PreviewRenameOptions, WorkbenchProgress, progress_step_for};
use crate::domain::media::{MediaMetadataPayload, RenameConflictPolicy, RenamePlanItem};
use crate::domain::template::render_template_target;
use crate::domain::workspace::{RootId, path_to_slash};

impl MediaWorkbenchService {
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
            let root_for_resolve = root.clone();
            let rel_clone = rel.clone();
            let fs_for_resolve = Arc::clone(&self.fs);
            let sandbox = tokio::task::spawn_blocking(move || {
                fs_for_resolve.resolve_existing_in_root(&root_for_resolve, &rel_clone)
            })
            .await
            .map_err(|e| DomainError::Storage(format!("resolve rename path failed: {e}")))??;
            let Some(inspector) =
                self.resolve_inspector_for_path(&sandbox.abs_path, effective_pref)
            else {
                continue;
            };
            let meta = inspector.inspect_file(&sandbox.abs_path).await?;
            let ext = if sandbox.abs_path.is_dir() {
                String::new()
            } else {
                sandbox
                    .abs_path
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default()
                    .to_string()
            };

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

            let root_for_plan = root.clone();
            let fs = Arc::clone(&self.fs);
            let toolbox = Arc::clone(&self.toolbox);
            let planned_snapshot = std::mem::take(&mut planned_targets);
            let (plan_item, updated_targets) = tokio::task::spawn_blocking(
                move || -> DomainResult<(RenamePlanItem, std::collections::HashSet<String>)> {
                    let mut planned_targets = planned_snapshot;
                    let parent_rel = sandbox
                        .rel_path
                        .parent()
                        .map(path_to_slash)
                        .unwrap_or_default();
                    let (mut to_rel, mut rendered_stem) =
                        render_template_target(&tpl, &parent_rel, &meta, &sandbox.abs_path, &ext);
                    let target_parent_rel = Path::new(&to_rel)
                        .parent()
                        .map(path_to_slash)
                        .unwrap_or_default();

                    let from_rel = sandbox.rel_slash_string();
                    let mut unchanged = from_rel == to_rel;

                    let target_abs = fs.resolve_target_in_root(&root_for_plan, &to_rel)?;
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
                                        let cand_abs =
                                            fs.resolve_target_in_root(&root_for_plan, &cand_rel)?;
                                        let cand_exists =
                                            cand_abs.exists() && cand_abs != sandbox.abs_path;
                                        let cand_dup =
                                            planned_targets.contains(&cand_rel.to_lowercase());
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
                        for (comp_abs, suffix) in fs.find_companion_sidecars(&sandbox.abs_path) {
                            if let Ok(comp_rel) = comp_abs.strip_prefix(&root_for_plan.path) {
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

                        if matches!(meta, MediaMetadataPayload::Book(_))
                            && parent_rel != target_parent_rel
                            && !target_parent_rel.is_empty()
                            && let Some(old_dir) = sandbox.abs_path.parent()
                            && old_dir != root_for_plan.path
                            && toolbox.dir_has_single_book(old_dir)
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
                                    && let Ok(rel_c) = cand.strip_prefix(&root_for_plan.path)
                                {
                                    companion_moves.push((
                                        path_to_slash(rel_c),
                                        format!("{target_parent_rel}/{fname}"),
                                    ));
                                }
                            }
                        }

                        if matches!(meta, MediaMetadataPayload::Music(_))
                            && parent_rel != target_parent_rel
                            && !target_parent_rel.is_empty()
                            && let Some(old_dir) = sandbox.abs_path.parent()
                            && old_dir != root_for_plan.path
                            && old_dir
                                .file_name()
                                .and_then(|s| s.to_str())
                                .is_some_and(|n| !toolbox.is_generic_music_folder(n))
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
                                    && let Ok(rel_c) = cand.strip_prefix(&root_for_plan.path)
                                {
                                    companion_moves.push((
                                        path_to_slash(rel_c),
                                        format!("{target_parent_rel}/{fname}"),
                                    ));
                                }
                            }
                        }

                        if matches!(meta, MediaMetadataPayload::Comic(_))
                            && parent_rel != target_parent_rel
                            && !target_parent_rel.is_empty()
                            && let Some(old_dir) = sandbox.abs_path.parent()
                            && old_dir != root_for_plan.path
                            && old_dir
                                .file_name()
                                .and_then(|s| s.to_str())
                                .is_some_and(|n| !toolbox.is_generic_comic_folder(n))
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
                                    && let Ok(rel_c) = cand.strip_prefix(&root_for_plan.path)
                                {
                                    companion_moves.push((
                                        path_to_slash(rel_c),
                                        format!("{target_parent_rel}/{fname}"),
                                    ));
                                }
                            }
                        }

                        if let MediaMetadataPayload::Video(ref v) = meta
                            && parent_rel != target_parent_rel
                            && !target_parent_rel.is_empty()
                            && let Some(old_dir) = sandbox.abs_path.parent()
                            && old_dir != root_for_plan.path
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
                            } else if toolbox.dir_has_single_video(old_dir) {
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
                                    && let Ok(rel_c) = cand.strip_prefix(&root_for_plan.path)
                                {
                                    companion_moves.push((
                                        path_to_slash(rel_c),
                                        format!("{target_parent_rel}/{fname}"),
                                    ));
                                }
                            }

                            if v.is_episode
                                && let Some(old_show_dir) = old_dir.parent()
                                && old_show_dir != root_for_plan.path
                                && old_show_dir.starts_with(&root_for_plan.path)
                                && let Some(new_show_parent) =
                                    Path::new(&target_parent_rel).parent()
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
                                            && let Ok(rel_c) =
                                                cand.strip_prefix(&root_for_plan.path)
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

                    Ok((
                        RenamePlanItem {
                            from_rel_path: from_rel,
                            to_rel_path: to_rel,
                            companion_moves,
                            conflict,
                            unchanged,
                            will_overwrite,
                        },
                        planned_targets,
                    ))
                },
            )
            .await
            .map_err(|e| DomainError::Storage(format!("preview rename task failed: {e}")))??;
            planned_targets = updated_targets;
            plans.push(plan_item);

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
}

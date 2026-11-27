use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use tokio::sync::RwLock;
use tokio::sync::mpsc::Sender;
use xmedia_core::domain::error::{DomainError, DomainResult};

use super::browse::WorkspaceBrowserService;
use super::metadata::MetadataService;
use super::{
    PreviewRenameOptions, RenameHistoryBatchDto, RootLockRegistry, WorkbenchProgress,
    is_same_physical_file, progress_step_for,
};
use crate::application::ports::{HttpWebhookPort, WorkspaceFsPort};
use crate::application::settings::ToolSettingsService;
use crate::domain::media::{MediaMetadataPayload, RenameConflictPolicy, RenamePlanItem};
use crate::domain::template::render_template_target;
use crate::domain::workspace::{ManagedRoot, RootId, SandboxPath, path_to_slash};

#[derive(Clone)]
pub struct RenameService {
    pub(super) settings: ToolSettingsService,
    pub(super) workspace: WorkspaceBrowserService,
    pub(super) metadata: MetadataService,
    pub(super) fs: Arc<dyn WorkspaceFsPort>,
    pub(super) webhook: Arc<dyn HttpWebhookPort>,
    pub(super) root_locks: RootLockRegistry,
    pub(super) history: Arc<RwLock<Vec<RenameHistoryBatchDto>>>,
}

impl RenameService {
    pub fn new(
        settings: ToolSettingsService,
        workspace: WorkspaceBrowserService,
        metadata: MetadataService,
        fs: Arc<dyn WorkspaceFsPort>,
        webhook: Arc<dyn HttpWebhookPort>,
        root_locks: RootLockRegistry,
    ) -> Self {
        Self {
            settings,
            workspace,
            metadata,
            fs,
            webhook,
            root_locks,
            history: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub async fn preview(
        &self,
        root_id: RootId,
        rel_paths: &[String],
        options: PreviewRenameOptions<'_>,
        progress: Option<Sender<WorkbenchProgress>>,
    ) -> DomainResult<Vec<RenamePlanItem>> {
        let raw_settings = self.settings.get_raw().await?;
        let root = raw_settings.find_root(root_id)?.clone();

        let mut plans = Vec::with_capacity(rel_paths.len());
        let mut planned_targets = HashSet::new();

        let clean_primary_tpl = options
            .template_override
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let clean_ep_tpl = options
            .episode_template_override
            .map(str::trim)
            .filter(|s| !s.is_empty());

        let total = rel_paths.len() as u64;
        let step = progress_step_for(total);
        if let Some(ref tx) = progress {
            let _ = tx.try_send(WorkbenchProgress {
                processed: 0,
                total: Some(total),
                detail: None,
            });
        }

        let effective_pref = options.preferred_kind.or(root.default_kind);
        let conflict_policy = options.conflict_policy;

        for (idx, rel) in rel_paths.iter().enumerate() {
            let root_for_resolve = root.clone();
            let rel_clone = rel.clone();
            let fs_for_resolve = Arc::clone(&self.fs);
            let sandbox = tokio::task::spawn_blocking(move || {
                fs_for_resolve.resolve_existing_in_root(&root_for_resolve, &rel_clone)
            })
            .await
            .map_err(|e| DomainError::Storage(format!("resolve rename path failed: {e}")))??;
            let Some(inspector) = self
                .workspace
                .resolve_inspector(&sandbox.abs_path, effective_pref)
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

            let tpl = select_rename_template(
                &meta,
                &raw_settings.rename_templates,
                clean_primary_tpl,
                clean_ep_tpl,
                options.episode_template_override.is_none(),
            );

            let root_for_plan = root.clone();
            let fs = Arc::clone(&self.fs);
            let planned_snapshot = std::mem::take(&mut planned_targets);
            let (plan_item, updated_targets) = tokio::task::spawn_blocking(move || {
                plan_single_target(
                    fs.as_ref(),
                    &root_for_plan,
                    &sandbox,
                    &meta,
                    &tpl,
                    &ext,
                    conflict_policy,
                    planned_snapshot,
                )
            })
            .await
            .map_err(|e| DomainError::Storage(format!("preview rename task failed: {e}")))??;
            planned_targets = updated_targets;
            plans.push(plan_item);

            let done = (idx + 1) as u64;
            if let Some(ref tx) = progress
                && (done.is_multiple_of(step) || done == total)
            {
                let _ = tx.try_send(WorkbenchProgress {
                    processed: done,
                    total: Some(total),
                    detail: Some(rel.clone()),
                });
            }
        }

        Ok(plans)
    }
}

#[allow(clippy::too_many_arguments)]
fn plan_single_target(
    fs: &dyn WorkspaceFsPort,
    root: &ManagedRoot,
    sandbox: &SandboxPath,
    meta: &MediaMetadataPayload,
    tpl: &str,
    ext: &str,
    conflict_policy: RenameConflictPolicy,
    mut planned_targets: HashSet<String>,
) -> DomainResult<(RenamePlanItem, HashSet<String>)> {
    let parent_rel = sandbox
        .rel_path
        .parent()
        .map(path_to_slash)
        .unwrap_or_default();
    let (mut to_rel, mut rendered_stem) =
        render_template_target(tpl, &parent_rel, meta, &sandbox.abs_path, ext);
    let target_parent_rel = Path::new(&to_rel)
        .parent()
        .map(path_to_slash)
        .unwrap_or_default();

    let from_rel = sandbox.rel_slash_string();
    let mut unchanged = from_rel == to_rel;

    let target_abs = fs.resolve_target_in_root(root, &to_rel)?;
    let mut conflict = None;
    let mut will_overwrite = false;
    if let MediaMetadataPayload::Video(v) = meta
        && v.is_episode
        && v.episode_number.is_none()
        && tpl.to_ascii_lowercase().contains("{episode")
    {
        conflict = Some("剧集模板缺少集号".to_string());
    } else if !unchanged {
        let exists_on_disk =
            target_abs.exists() && !is_same_physical_file(&target_abs, &sandbox.abs_path);
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
                        let cand_abs = fs.resolve_target_in_root(root, &cand_rel)?;
                        let cand_exists = cand_abs.exists()
                            && !is_same_physical_file(&cand_abs, &sandbox.abs_path);
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

    let companion_moves = if unchanged {
        Vec::new()
    } else {
        fs.collect_relocated_companions(
            &root.path,
            &sandbox.abs_path,
            &rendered_stem,
            &parent_rel,
            &target_parent_rel,
            meta,
        )
    };

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
}

fn select_rename_template(
    meta: &MediaMetadataPayload,
    defaults: &crate::domain::config::RenameTemplates,
    clean_primary_tpl: Option<&str>,
    clean_ep_tpl: Option<&str>,
    inherit_primary_for_ep: bool,
) -> String {
    match meta {
        MediaMetadataPayload::Book(_) => clean_primary_tpl
            .map(str::to_owned)
            .unwrap_or_else(|| defaults.book.clone()),
        MediaMetadataPayload::Comic(_) => clean_primary_tpl
            .map(str::to_owned)
            .unwrap_or_else(|| defaults.comic.clone()),
        MediaMetadataPayload::Music(_) => clean_primary_tpl
            .map(str::to_owned)
            .unwrap_or_else(|| defaults.music.clone()),
        MediaMetadataPayload::Video(v) => {
            if v.is_episode {
                clean_ep_tpl
                    .or(if inherit_primary_for_ep {
                        clean_primary_tpl
                    } else {
                        None
                    })
                    .map(str::to_owned)
                    .unwrap_or_else(|| defaults.video_episode.clone())
            } else {
                clean_primary_tpl
                    .map(str::to_owned)
                    .unwrap_or_else(|| defaults.video_movie.clone())
            }
        }
    }
}

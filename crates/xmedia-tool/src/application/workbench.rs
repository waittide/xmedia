use std::path::Path;
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use walkdir::WalkDir;
use xmedia_core::application::ports::BinaryPayload;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;

use crate::application::settings::ToolSettingsService;
use crate::domain::media::{
    DirectorySnapshot, FolderEntryDto, MediaFileDetail, MediaFileEntry, MediaMetadataPatch,
    MediaMetadataPayload, RenamePlanItem,
};
use crate::domain::ports::MediaInspectorPort;
use crate::domain::template::render_template_target;
use crate::domain::workspace::{ManagedRoot, RootId, SandboxPath, path_to_slash};
use crate::infra::atomic_fs::{cleanup_empty_parent_dirs, find_companion_sidecars};
use crate::infra::inspectors::book::dir_has_single_book;
use crate::infra::inspectors::book_content::{
    BookContentDocumentDto, EditableBookChapterDto, InsertedEpubImageDto, PolishBookReportDto,
    SaveBookChapterInput, TransformBookContentResultDto, convert_txt_to_epub, insert_epub_image,
    polish_epub_book, read_book_content, read_epub_internal_asset, resegment_txt_file,
    save_book_content, transform_book_content_batch,
};
use crate::infra::inspectors::comic::is_generic_comic_folder;
use crate::infra::inspectors::music::is_generic_music_folder;
use crate::infra::inspectors::video::dir_has_single_video;
use crate::infra::inspectors::zh_text::BookTextTransformOptions;

#[derive(Debug, Clone, serde::Serialize)]
pub struct WorkbenchProgress {
    pub processed: u64,
    pub total: Option<u64>,
    pub detail: Option<String>,
}

fn progress_step_for(total: u64) -> u64 {
    if total <= 100 {
        1
    } else if total <= 1000 {
        5
    } else {
        25
    }
}

#[derive(Clone)]
pub struct MediaWorkbenchService {
    settings: ToolSettingsService,
    inspectors: Vec<Arc<dyn MediaInspectorPort>>,
}

impl MediaWorkbenchService {
    pub fn new(
        settings: ToolSettingsService,
        inspectors: Vec<Arc<dyn MediaInspectorPort>>,
    ) -> Self {
        Self {
            settings,
            inspectors,
        }
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
        let root = self.settings.find_root(root_id).await?;
        let dir_sandbox = root.resolve_existing(rel_path)?;
        if !dir_sandbox.abs_path.is_dir() {
            return Err(DomainError::Validation(format!(
                "'{}' is not a directory",
                dir_sandbox.rel_slash_string()
            )));
        }

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

        let mut raw_dirs = Vec::new();
        let mut raw_files = Vec::new();

        let entries = std::fs::read_dir(&dir_sandbox.abs_path).map_err(|e| {
            DomainError::Storage(format!(
                "failed reading directory '{}': {e}",
                dir_sandbox.abs_path.display()
            ))
        })?;

        for entry in entries.flatten() {
            let p = entry.path();
            let Some(fname) = p.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            if fname.starts_with('.') {
                continue;
            }
            if p.is_dir() {
                raw_dirs.push((fname.to_string(), p));
            } else if !recursive
                && p.is_file()
                && let Some(ins) = self.resolve_inspector_for_path(&p, domain_filter)
                && domain_filter.is_none_or(|want| ins.media_kind() == want)
            {
                raw_files.push((fname.to_string(), p, ins));
            }
        }

        on_progress(WorkbenchProgress {
            processed: 0,
            total: None,
            detail: Some(if current_rel.is_empty() {
                root.name.clone()
            } else {
                current_rel.clone()
            }),
        });

        if recursive {
            for entry in WalkDir::new(&dir_sandbox.abs_path)
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
                let p = entry.path().to_path_buf();
                let Some(fname) = p.file_name().and_then(|s| s.to_str()) else {
                    continue;
                };
                if let Some(ins) = self.resolve_inspector_for_path(&p, domain_filter)
                    && domain_filter.is_none_or(|want| ins.media_kind() == want)
                {
                    let fname_owned = fname.to_string();
                    raw_files.push((fname_owned.clone(), p, ins));
                    let discovered = raw_files.len() as u64;
                    if discovered.is_multiple_of(100) {
                        on_progress(WorkbenchProgress {
                            processed: discovered,
                            total: None,
                            detail: Some(fname_owned),
                        });
                    }
                }
            }
        }

        raw_dirs.sort_by_key(|a| a.0.to_lowercase());
        raw_files.sort_by_key(|a| {
            a.1.strip_prefix(&root.path)
                .map(path_to_slash)
                .unwrap_or_else(|_| a.0.clone())
                .to_lowercase()
        });

        let mut folders = Vec::with_capacity(raw_dirs.len());
        for (dir_name, dir_abs) in raw_dirs {
            let rel = dir_abs
                .strip_prefix(&root.path)
                .map(path_to_slash)
                .unwrap_or_else(|_| dir_name.clone());
            let (child_dir_count, media_file_count) =
                count_subtree_children(&dir_abs, &self.inspectors, domain_filter);
            folders.push(FolderEntryDto {
                name: dir_name,
                rel_path: rel,
                child_dir_count,
                media_file_count,
            });
        }

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
                .build_file_entry(&root, &file_abs, &fname, inspector.as_ref())
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
        let entry = self
            .build_file_entry(&root, &sandbox.abs_path, &fname, inspector.as_ref())
            .await?;
        let metadata = inspector.inspect_file(&sandbox.abs_path).await?;
        Ok(MediaFileDetail { entry, metadata })
    }

    pub async fn write_metadata(
        &self,
        root_id: RootId,
        rel_path: &str,
        patch: MediaMetadataPatch,
    ) -> DomainResult<MediaFileDetail> {
        let preferred_kind = Some(match &patch {
            MediaMetadataPatch::Book(_) => MediaKind::Book,
            MediaMetadataPatch::Comic(_) => MediaKind::Comic,
            MediaMetadataPatch::Music(_) => MediaKind::Music,
            MediaMetadataPatch::Video(_) => MediaKind::Video,
        });
        let (root, sandbox, inspector) = self
            .resolve_media_file(root_id, rel_path, preferred_kind)
            .await?;
        let metadata = inspector.write_metadata(&sandbox.abs_path, &patch).await?;
        let fname = sandbox
            .abs_path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let entry = self
            .build_file_entry(&root, &sandbox.abs_path, &fname, inspector.as_ref())
            .await?;
        Ok(MediaFileDetail { entry, metadata })
    }

    pub async fn read_artwork(
        &self,
        root_id: RootId,
        rel_path: &str,
        preferred_kind: Option<MediaKind>,
        slot: Option<&str>,
    ) -> DomainResult<Option<BinaryPayload>> {
        let (_root, sandbox, inspector) = self
            .resolve_media_file(root_id, rel_path, preferred_kind)
            .await?;
        inspector.read_artwork(&sandbox.abs_path, slot).await
    }

    pub async fn write_artwork(
        &self,
        root_id: RootId,
        rel_path: &str,
        bytes: &[u8],
        mime_type: &str,
        slot: &str,
        preferred_kind: Option<MediaKind>,
    ) -> DomainResult<MediaFileDetail> {
        let (root, sandbox, inspector) = self
            .resolve_media_file(root_id, rel_path, preferred_kind)
            .await?;
        inspector
            .write_artwork(&sandbox.abs_path, bytes, mime_type, slot)
            .await?;
        let fname = sandbox
            .abs_path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let entry = self
            .build_file_entry(&root, &sandbox.abs_path, &fname, inspector.as_ref())
            .await?;
        let metadata = inspector.inspect_file(&sandbox.abs_path).await?;
        Ok(MediaFileDetail { entry, metadata })
    }

    pub async fn preview_rename(
        &self,
        root_id: RootId,
        rel_paths: &[String],
        template_override: Option<&str>,
        episode_template_override: Option<&str>,
        preferred_kind: Option<MediaKind>,
    ) -> DomainResult<Vec<RenamePlanItem>> {
        self.preview_rename_with_progress(
            root_id,
            rel_paths,
            template_override,
            episode_template_override,
            preferred_kind,
            |_| {},
        )
        .await
    }

    pub async fn preview_rename_with_progress<F>(
        &self,
        root_id: RootId,
        rel_paths: &[String],
        template_override: Option<&str>,
        episode_template_override: Option<&str>,
        preferred_kind: Option<MediaKind>,
        mut on_progress: F,
    ) -> DomainResult<Vec<RenamePlanItem>>
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

        for (idx, rel) in rel_paths.iter().enumerate() {
            let sandbox = root.resolve_existing(rel)?;
            let Some(inspector) =
                self.resolve_inspector_for_path(&sandbox.abs_path, preferred_kind)
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
            let (to_rel, rendered_stem) =
                render_template_target(&tpl, &parent_rel, &meta, &sandbox.abs_path, ext);
            let target_parent_rel = Path::new(&to_rel)
                .parent()
                .map(path_to_slash)
                .unwrap_or_default();

            let from_rel = sandbox.rel_slash_string();
            let unchanged = from_rel == to_rel;

            let target_abs = root.resolve_target(&to_rel)?;
            let mut conflict = None;
            if let MediaMetadataPayload::Video(ref v) = meta
                && v.is_episode
                && v.episode_number.is_none()
                && tpl.to_ascii_lowercase().contains("{episode")
            {
                conflict = Some("episode number is missing for episode template".to_string());
            } else if !unchanged {
                if target_abs.exists() && target_abs != sandbox.abs_path {
                    conflict = Some("target file already exists on disk".to_string());
                } else if !planned_targets.insert(to_rel.to_lowercase()) {
                    conflict = Some("duplicate target filename in current batch".to_string());
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
        let root = self.settings.find_root(root_id).await?;
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

        for item in plans {
            if item.unchanged || item.conflict.is_some() {
                continue;
            }
            let src = root.resolve_existing(&item.from_rel_path)?;
            let old_parent = src.abs_path.parent().map(Path::to_path_buf);
            let dst = root.resolve_target(&item.to_rel_path)?;
            if dst.exists() && dst != src.abs_path {
                return Err(DomainError::Conflict(format!(
                    "target '{}' already exists",
                    item.to_rel_path
                )));
            }
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| DomainError::Storage(format!("failed creating folder: {e}")))?;
            }
            std::fs::rename(&src.abs_path, &dst).map_err(|e| {
                DomainError::Storage(format!(
                    "failed renaming '{}' -> '{}': {e}",
                    item.from_rel_path, item.to_rel_path
                ))
            })?;
            moved_count += 1;

            let mut migrated_shared_dirs: Vec<(std::path::PathBuf, std::path::PathBuf)> =
                Vec::new();
            for (comp_from, comp_to) in &item.companion_moves {
                if let Ok(comp_src) = root.resolve_existing(comp_from)
                    && let Ok(comp_dst) = root.resolve_target(comp_to)
                {
                    let is_shared_dir_sidecar = comp_src
                        .abs_path
                        .file_name()
                        .and_then(|s| s.to_str())
                        .is_some_and(|fname| {
                            matches!(
                                fname.to_ascii_lowercase().as_str(),
                                "metadata.opf"
                                    | "movie.nfo"
                                    | "season.nfo"
                                    | "tvshow.nfo"
                                    | "poster.jpg"
                                    | "poster.jpeg"
                                    | "poster.png"
                                    | "poster.webp"
                                    | "fanart.jpg"
                                    | "fanart.jpeg"
                                    | "fanart.png"
                                    | "fanart.webp"
                                    | "cover.jpg"
                                    | "cover.jpeg"
                                    | "cover.png"
                                    | "cover.webp"
                                    | "folder.jpg"
                                    | "folder.jpeg"
                                    | "folder.png"
                                    | "folder.webp"
                                    | "front.jpg"
                                    | "front.jpeg"
                                    | "front.png"
                                    | "front.webp"
                            )
                        });
                    if !comp_dst.exists() {
                        if let Some(p) = comp_dst.parent() {
                            let _ = std::fs::create_dir_all(p);
                        }
                        if is_shared_dir_sidecar {
                            let _ = std::fs::copy(&comp_src.abs_path, &comp_dst);
                            migrated_shared_dirs
                                .push((comp_src.abs_path.clone(), comp_dst.clone()));
                        } else {
                            let _ = std::fs::rename(&comp_src.abs_path, &comp_dst);
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

            let done = moved_count as u64;
            if done.is_multiple_of(step) || done == total {
                on_progress(WorkbenchProgress {
                    processed: done,
                    total: Some(total),
                    detail: Some(item.to_rel_path.clone()),
                });
            }
        }

        Ok(moved_count)
    }

    pub async fn read_epub_asset(
        &self,
        root_id: RootId,
        rel_path: &str,
        asset_path: &str,
    ) -> DomainResult<BinaryPayload> {
        let (_, sandbox, _) = self
            .resolve_media_file(root_id, rel_path, Some(MediaKind::Book))
            .await?;
        let abs = sandbox.abs_path;
        let asset_owned = asset_path.to_string();
        tokio::task::spawn_blocking(move || read_epub_internal_asset(&abs, &asset_owned))
            .await
            .map_err(|e| DomainError::Storage(format!("epub asset read task failed: {e}")))?
    }

    pub async fn get_book_content(
        &self,
        root_id: RootId,
        rel_path: &str,
    ) -> DomainResult<BookContentDocumentDto> {
        let (_, sandbox, _) = self
            .resolve_media_file(root_id, rel_path, Some(MediaKind::Book))
            .await?;
        let abs = sandbox.abs_path;
        tokio::task::spawn_blocking(move || read_book_content(&abs))
            .await
            .map_err(|e| DomainError::Storage(format!("book content read task failed: {e}")))?
    }

    pub async fn resegment_txt_book(
        &self,
        root_id: RootId,
        rel_path: &str,
        chapter_pattern: Option<String>,
        volume_pattern: Option<String>,
    ) -> DomainResult<Vec<EditableBookChapterDto>> {
        let (_, sandbox, _) = self
            .resolve_media_file(root_id, rel_path, Some(MediaKind::Book))
            .await?;
        let abs = sandbox.abs_path;
        tokio::task::spawn_blocking(move || {
            resegment_txt_file(&abs, chapter_pattern.as_deref(), volume_pattern.as_deref())
        })
        .await
        .map_err(|e| DomainError::Storage(format!("txt resegment task failed: {e}")))?
    }

    pub async fn save_book_content(
        &self,
        root_id: RootId,
        rel_path: &str,
        chapters: Vec<SaveBookChapterInput>,
        stylesheets: Option<Vec<String>>,
        force_horizontal_spine: bool,
    ) -> DomainResult<MediaFileDetail> {
        let (_, sandbox, _) = self
            .resolve_media_file(root_id, rel_path, Some(MediaKind::Book))
            .await?;
        let abs = sandbox.abs_path.clone();
        tokio::task::spawn_blocking(move || {
            save_book_content(
                &abs,
                &chapters,
                stylesheets.as_deref(),
                force_horizontal_spine,
            )
        })
        .await
        .map_err(|e| DomainError::Storage(format!("book content write task failed: {e}")))??;
        self.inspect_file(root_id, rel_path, Some(MediaKind::Book))
            .await
    }

    pub async fn convert_txt_book_to_epub(
        &self,
        root_id: RootId,
        rel_path: &str,
        chapters: Option<Vec<SaveBookChapterInput>>,
        stylesheets: Option<Vec<String>>,
        delete_source_txt: bool,
    ) -> DomainResult<MediaFileDetail> {
        let (root, sandbox, _) = self
            .resolve_media_file(root_id, rel_path, Some(MediaKind::Book))
            .await?;
        let abs = sandbox.abs_path.clone();
        let epub_abs = tokio::task::spawn_blocking(move || {
            convert_txt_to_epub(
                &abs,
                chapters.as_deref(),
                stylesheets.as_deref(),
                delete_source_txt,
            )
        })
        .await
        .map_err(|e| DomainError::Storage(format!("txt to epub task failed: {e}")))??;

        let new_rel = epub_abs
            .strip_prefix(&root.path)
            .map(path_to_slash)
            .map_err(|_| DomainError::Storage("generated epub outside root".into()))?;
        self.inspect_file(root_id, &new_rel, Some(MediaKind::Book))
            .await
    }

    pub async fn transform_book_content(
        &self,
        chapters: Vec<EditableBookChapterDto>,
        stylesheets: Vec<String>,
        options: BookTextTransformOptions,
    ) -> DomainResult<TransformBookContentResultDto> {
        tokio::task::spawn_blocking(move || {
            transform_book_content_batch(chapters, stylesheets, &options)
        })
        .await
        .map_err(|e| DomainError::Storage(format!("book transform task failed: {e}")))?
    }

    pub async fn insert_epub_chapter_image(
        &self,
        root_id: RootId,
        rel_path: &str,
        chapter_source_href: Option<String>,
        bytes: Vec<u8>,
        mime_hint: Option<String>,
    ) -> DomainResult<InsertedEpubImageDto> {
        let (_, sandbox, _) = self
            .resolve_media_file(root_id, rel_path, Some(MediaKind::Book))
            .await?;
        let abs = sandbox.abs_path;
        tokio::task::spawn_blocking(move || {
            insert_epub_image(
                &abs,
                chapter_source_href.as_deref(),
                &bytes,
                mime_hint.as_deref(),
            )
        })
        .await
        .map_err(|e| DomainError::Storage(format!("insert epub image task failed: {e}")))?
    }

    pub async fn polish_book(
        &self,
        root_id: RootId,
        rel_path: &str,
    ) -> DomainResult<(PolishBookReportDto, MediaFileDetail)> {
        let (_, sandbox, _) = self
            .resolve_media_file(root_id, rel_path, Some(MediaKind::Book))
            .await?;
        let abs = sandbox.abs_path;
        let report = tokio::task::spawn_blocking(move || polish_epub_book(&abs))
            .await
            .map_err(|e| DomainError::Storage(format!("polish epub task failed: {e}")))??;
        let detail = self
            .inspect_file(root_id, rel_path, Some(MediaKind::Book))
            .await?;
        Ok((report, detail))
    }

    async fn resolve_media_file(
        &self,
        root_id: RootId,
        rel_path: &str,
        preferred_kind: Option<MediaKind>,
    ) -> DomainResult<(ManagedRoot, SandboxPath, Arc<dyn MediaInspectorPort>)> {
        let root = self.settings.find_root(root_id).await?;
        let sandbox = root.resolve_existing(rel_path)?;
        if !sandbox.abs_path.is_file() {
            return Err(DomainError::Validation(format!(
                "'{}' is not a regular file",
                sandbox.rel_slash_string()
            )));
        }
        let inspector = self
            .resolve_inspector_for_path(&sandbox.abs_path, preferred_kind)
            .ok_or_else(|| {
                DomainError::UnsupportedFormat(format!(
                    "unsupported media file '{}'",
                    sandbox.rel_slash_string()
                ))
            })?;
        Ok((root, sandbox, inspector))
    }

    async fn build_file_entry(
        &self,
        root: &ManagedRoot,
        abs_path: &Path,
        file_name: &str,
        inspector: &dyn MediaInspectorPort,
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
        let ext = abs_path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let fallback_stem = abs_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(file_name);

        let quick = inspector.summarize_file(abs_path, fallback_stem).await;

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

fn count_subtree_children(
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

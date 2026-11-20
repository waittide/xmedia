use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::RwLock;
use uuid::Uuid;

use walkdir::WalkDir;
use xmedia_core::application::ports::BinaryPayload;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;

use crate::application::batch_metadata::{
    BatchMetadataEditInput, BatchMetadataEditReportDto, MetadataHistoryBatchDto,
    MetadataSnapshotRecord, UndoMetadataReportDto, build_batch_patch_for_payload,
    payload_to_restore_patch,
};
use crate::application::settings::ToolSettingsService;
use crate::application::webhook::notify_xmedia_rescan;
use crate::domain::config::RenameTemplates;
use crate::domain::media::{
    ComicMetaPatch, DeleteFileErrorDto, DeleteFilesReportDto, DirectorySnapshot, FolderEntryDto,
    MediaFileDetail, MediaFileEntry, MediaMetadataPatch, MediaMetadataPayload, MusicMetaPatch,
    RawFileEntryDto, RenameConflictPolicy, RenamePlanItem, VideoMetaPatch,
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
use crate::infra::inspectors::book_normalize::{
    NormalizeBookItemReportDto, NormalizeBookToEpubOptions, NormalizeBooksBatchReportDto,
    normalize_book_to_epub,
};
use crate::infra::inspectors::comic::is_generic_comic_folder;
use crate::infra::inspectors::comic_ops::{
    BatchComicSeriesSyncInput, BatchComicSeriesSyncReportDto, ComicImageReencodeFormat,
    ComicOptimizeReportDto, ComicPagesSnapshotDto, MergeComicsReportDto, SaveComicPageInput,
    list_comic_pages, merge_comics_to_cbz, optimize_comic_cbz, read_comic_page_image,
    save_comic_pages,
};
use crate::infra::inspectors::music::is_generic_music_folder;
use crate::infra::inspectors::music_ops::{
    BatchMusicAlbumSyncInput, BatchMusicAlbumSyncReportDto, CueSplitReportDto,
    MusicCoverOptimizeReportDto, MusicLyricsToolInput, MusicLyricsToolReportDto,
    ReplayGainReportDto, apply_replaygain_batch, optimize_music_cover, process_music_lyrics,
    split_cue_album,
};
use crate::infra::inspectors::video::{
    dir_has_single_video, read_base_video_meta, render_season_nfo_xml,
};
use crate::infra::inspectors::video_ops::{
    BatchRemapEpisodesInput, BatchRemapEpisodesReportDto, ExtractSubtitleReportDto,
    ProcessSubtitleInput, ProcessSubtitleReportDto, VideoSubtitleInfoDto,
    extract_embedded_subtitle, list_video_subtitles, process_video_subtitle,
};
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
    settings: ToolSettingsService,
    inspectors: Vec<Arc<dyn MediaInspectorPort>>,
    rename_history: Arc<RwLock<Vec<RenameHistoryBatchDto>>>,
    metadata_history: Arc<RwLock<Vec<MetadataHistoryBatchDto>>>,
}

impl MediaWorkbenchService {
    pub fn new(
        settings: ToolSettingsService,
        inspectors: Vec<Arc<dyn MediaInspectorPort>>,
    ) -> Self {
        Self {
            settings,
            inspectors,
            rename_history: Arc::new(RwLock::new(Vec::new())),
            metadata_history: Arc::new(RwLock::new(Vec::new())),
        }
    }

    async fn push_metadata_history(&self, batch: MetadataHistoryBatchDto) {
        let mut guard = self.metadata_history.write().await;
        guard.push(batch);
        if guard.len() > 60 {
            let drain_count = guard.len() - 60;
            guard.drain(0..drain_count);
        }
    }

    pub async fn list_metadata_history(
        &self,
        root_id: Option<RootId>,
    ) -> Vec<MetadataHistoryBatchDto> {
        let guard = self.metadata_history.read().await;
        guard
            .iter()
            .rev()
            .filter(|b| root_id.is_none_or(|rid| b.root_id == rid.0))
            .cloned()
            .collect()
    }

    pub async fn undo_metadata_batch(&self, batch_id: Uuid) -> DomainResult<UndoMetadataReportDto> {
        let batch = {
            let guard = self.metadata_history.read().await;
            guard
                .iter()
                .find(|b| b.batch_id == batch_id)
                .cloned()
                .ok_or_else(|| {
                    DomainError::NotFound(format!("metadata history batch {batch_id} not found"))
                })?
        };

        let mut restored_count = 0usize;
        let mut failed_paths = Vec::new();
        for rec in batch.records.iter().rev() {
            let restore_patch = payload_to_restore_patch(&rec.previous);
            match self
                .write_metadata_inner(RootId(batch.root_id), &rec.rel_path, restore_patch, None)
                .await
            {
                Ok(_) => restored_count += 1,
                Err(_) => failed_paths.push(rec.rel_path.clone()),
            }
        }

        if restored_count > 0 {
            let mut guard = self.metadata_history.write().await;
            guard.retain(|b| b.batch_id != batch_id);
        }

        Ok(UndoMetadataReportDto {
            batch_id,
            restored_count,
            failed_paths,
        })
    }

    pub async fn batch_edit_metadata(
        &self,
        root_id: RootId,
        input: BatchMetadataEditInput,
    ) -> DomainResult<BatchMetadataEditReportDto> {
        let mut updated_count = 0usize;
        let mut skipped_count = 0usize;
        let mut failed_count = 0usize;
        let mut errors = Vec::new();
        let mut snapshots = Vec::new();

        for (idx, rel_path) in input.paths.iter().enumerate() {
            let resolved = self
                .resolve_writable_media_file(root_id, rel_path, Some(input.media_kind))
                .await;
            let (_root, sandbox, inspector) = match resolved {
                Ok(v) => v,
                Err(e) => {
                    failed_count += 1;
                    errors.push(format!("{rel_path}: {e}"));
                    continue;
                }
            };
            let current = match inspector.inspect_file(&sandbox.abs_path).await {
                Ok(m) => m,
                Err(e) => {
                    failed_count += 1;
                    errors.push(format!("{rel_path}: {e}"));
                    continue;
                }
            };
            let file_name = sandbox
                .abs_path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let file_stem = sandbox
                .abs_path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| file_name.clone());

            let maybe_patch =
                match build_batch_patch_for_payload(&current, &file_stem, idx + 1, &input) {
                    Ok(p) => p,
                    Err(e) => {
                        return Err(e);
                    }
                };
            let Some(patch) = maybe_patch else {
                skipped_count += 1;
                continue;
            };

            match inspector.write_metadata(&sandbox.abs_path, &patch).await {
                Ok(_) => {
                    updated_count += 1;
                    snapshots.push(MetadataSnapshotRecord {
                        rel_path: rel_path.clone(),
                        file_name,
                        previous: current,
                    });
                }
                Err(e) => {
                    failed_count += 1;
                    errors.push(format!("{rel_path}: {e}"));
                }
            }
        }

        let batch_id = if !snapshots.is_empty() {
            let id = Uuid::new_v4();
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            self.push_metadata_history(MetadataHistoryBatchDto {
                batch_id: id,
                root_id: root_id.0,
                media_kind: input.media_kind,
                label: format!("批量编辑 ({updated_count} 项)"),
                created_at_unix: now,
                updated_count,
                records: snapshots,
            })
            .await;
            Some(id)
        } else {
            None
        };

        Ok(BatchMetadataEditReportDto {
            batch_id,
            updated_count,
            skipped_count,
            failed_count,
            errors,
        })
    }

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
        let root = self.settings.find_root(RootId(batch.root_id)).await?;
        root.ensure_writable()?;
        let mut restored_count = 0usize;
        let mut skipped_conflicts = Vec::new();

        for mv in batch.moves.iter().rev() {
            let Ok(cur) = root.resolve_existing(&mv.to_rel_path) else {
                skipped_conflicts.push(mv.to_rel_path.clone());
                continue;
            };
            let Ok(orig) = root.resolve_target(&mv.from_rel_path) else {
                skipped_conflicts.push(mv.from_rel_path.clone());
                continue;
            };
            if orig.exists() && orig != cur.abs_path {
                skipped_conflicts.push(mv.from_rel_path.clone());
                continue;
            }
            let cur_parent = cur.abs_path.parent().map(Path::to_path_buf);
            if let Some(p) = orig.parent() {
                let _ = std::fs::create_dir_all(p);
            }
            if std::fs::rename(&cur.abs_path, &orig).is_err() {
                skipped_conflicts.push(mv.to_rel_path.clone());
                continue;
            }
            restored_count += 1;

            for (comp_from, comp_to) in mv.companion_moves.iter().rev() {
                if let Ok(comp_cur) = root.resolve_existing(comp_to)
                    && let Ok(comp_orig) = root.resolve_target(comp_from)
                    && !comp_orig.exists()
                {
                    if let Some(p) = comp_orig.parent() {
                        let _ = std::fs::create_dir_all(p);
                    }
                    let _ = std::fs::rename(&comp_cur.abs_path, &comp_orig);
                }
            }

            if let Some(ref prev_dir) = cur_parent {
                cleanup_empty_parent_dirs(prev_dir, &root.path);
            }
        }

        let mut guard = self.rename_history.write().await;
        guard.retain(|b| b.batch_id != batch_id);

        Ok(UndoRenameReportDto {
            batch_id,
            restored_count,
            skipped_conflicts,
        })
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
        let raw_settings = self.settings.get_raw().await?;
        let root = raw_settings
            .roots
            .iter()
            .find(|r| r.id == root_id)
            .cloned()
            .ok_or_else(|| DomainError::NotFound(format!("root '{root_id}' not found")))?;
        let rename_templates = raw_settings.rename_templates;
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

        let effective_pref = domain_filter.or(root.default_kind);
        let mut raw_dirs = Vec::new();
        let mut raw_files = Vec::new();
        let mut folder_raw_entries = Vec::new();

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
            } else if p.is_file() {
                let matched_ins = self
                    .resolve_inspector_for_path(&p, effective_pref)
                    .filter(|ins| domain_filter.is_none_or(|want| ins.media_kind() == want));
                let meta_fs = std::fs::metadata(&p).ok();
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
                if !recursive && let Some(ins) = matched_ins {
                    raw_files.push((fname.to_string(), p, ins));
                }
            }
        }
        folder_raw_entries.sort_by_key(|a| a.file_name.to_lowercase());

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
                if let Some(ins) = self.resolve_inspector_for_path(&p, effective_pref)
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

    pub async fn write_metadata(
        &self,
        root_id: RootId,
        rel_path: &str,
        patch: MediaMetadataPatch,
    ) -> DomainResult<MediaFileDetail> {
        self.write_metadata_inner(root_id, rel_path, patch, Some("编辑/刮削写入"))
            .await
    }

    async fn write_metadata_inner(
        &self,
        root_id: RootId,
        rel_path: &str,
        patch: MediaMetadataPatch,
        history_label: Option<&str>,
    ) -> DomainResult<MediaFileDetail> {
        let media_kind = match &patch {
            MediaMetadataPatch::Book(_) => MediaKind::Book,
            MediaMetadataPatch::Comic(_) => MediaKind::Comic,
            MediaMetadataPatch::Music(_) => MediaKind::Music,
            MediaMetadataPatch::Video(_) => MediaKind::Video,
        };
        let (root, sandbox, inspector) = self
            .resolve_writable_media_file(root_id, rel_path, Some(media_kind))
            .await?;
        let previous = if history_label.is_some() {
            inspector.inspect_file(&sandbox.abs_path).await.ok()
        } else {
            None
        };
        let metadata = inspector.write_metadata(&sandbox.abs_path, &patch).await?;
        let fname = sandbox
            .abs_path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        if let (Some(label), Some(prev)) = (history_label, previous) {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            self.push_metadata_history(MetadataHistoryBatchDto {
                batch_id: Uuid::new_v4(),
                root_id: root_id.0,
                media_kind,
                label: format!("{label} · {fname}"),
                created_at_unix: now,
                updated_count: 1,
                records: vec![MetadataSnapshotRecord {
                    rel_path: rel_path.to_string(),
                    file_name: fname.clone(),
                    previous: prev,
                }],
            })
            .await;
        }
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
            .resolve_writable_media_file(root_id, rel_path, preferred_kind)
            .await?;
        inspector
            .write_artwork(&sandbox.abs_path, bytes, mime_type, slot)
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
            let sandbox = root.resolve_existing(rel)?;
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

            let target_abs = root.resolve_target(&to_rel)?;
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
                                let cand_abs = root.resolve_target(&cand_rel)?;
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

        for item in plans {
            if item.unchanged || item.conflict.is_some() {
                continue;
            }
            let src = root.resolve_existing(&item.from_rel_path)?;
            let old_parent = src.abs_path.parent().map(Path::to_path_buf);
            let dst = root.resolve_target(&item.to_rel_path)?;
            if dst.exists() && dst != src.abs_path {
                if !item.will_overwrite {
                    return Err(DomainError::Conflict(format!(
                        "目标文件已存在：'{}'",
                        item.to_rel_path
                    )));
                }
                if dst.is_dir() {
                    return Err(DomainError::Conflict(format!(
                        "目标路径是目录，无法覆盖：'{}'",
                        item.to_rel_path
                    )));
                }
                std::fs::remove_file(&dst).map_err(|e| {
                    DomainError::Storage(format!("无法覆盖已有文件 '{}': {e}", item.to_rel_path))
                })?;
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

            let mut moved_companions: Vec<(String, String)> = Vec::new();
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
                            let _ = std::fs::copy(&comp_src.abs_path, &comp_dst);
                            migrated_shared_dirs
                                .push((comp_src.abs_path.clone(), comp_dst.clone()));
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

        Ok(moved_count)
    }

    pub async fn delete_files(
        &self,
        root_id: RootId,
        rel_paths: &[String],
        delete_companions: bool,
    ) -> DomainResult<DeleteFilesReportDto> {
        let root = self.settings.find_root(root_id).await?;
        root.ensure_writable()?;

        let mut deleted_paths = Vec::new();
        let mut deleted_companions = Vec::new();
        let mut failed = Vec::new();
        let mut removed_set = std::collections::HashSet::new();
        let mut removed_dirs: Vec<String> = Vec::new();

        for rel in rel_paths {
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
            let sandbox = match root.resolve_existing(trimmed) {
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
            let companions: Vec<std::path::PathBuf> = if delete_companions {
                find_companion_sidecars(&sandbox.abs_path)
                    .into_iter()
                    .map(|(p, _)| p)
                    .filter(|p| {
                        // Never delete directory-shared sidecars directly; only clean them if directory becomes empty
                        let is_shared =
                            p.file_name().and_then(|s| s.to_str()).is_some_and(|fname| {
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
                        !is_shared && self.resolve_inspector_for_path(p, None).is_none()
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
                    cleanup_empty_parent_dirs(prev_dir, &root.path);
                }
            }
        }

        Ok(DeleteFilesReportDto {
            deleted_count: deleted_paths.len(),
            deleted_paths,
            deleted_companions,
            failed,
        })
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
            .resolve_writable_media_file(root_id, rel_path, Some(MediaKind::Book))
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
            .resolve_writable_media_file(root_id, rel_path, Some(MediaKind::Book))
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
            .resolve_writable_media_file(root_id, rel_path, Some(MediaKind::Book))
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
            .resolve_writable_media_file(root_id, rel_path, Some(MediaKind::Book))
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

    pub async fn normalize_books_to_epub(
        &self,
        root_id: RootId,
        rel_paths: &[String],
        options: NormalizeBookToEpubOptions,
    ) -> DomainResult<NormalizeBooksBatchReportDto> {
        if rel_paths.is_empty() {
            return Err(DomainError::Validation("请至少选择一本图书".into()));
        }

        let mut items = Vec::with_capacity(rel_paths.len());
        let mut succeeded = 0usize;
        let mut skipped = 0usize;
        let mut failed = 0usize;
        let mut total_before_bytes = 0u64;
        let mut total_after_bytes = 0u64;

        for rel in rel_paths {
            let resolved = self
                .resolve_writable_media_file(root_id, rel, Some(MediaKind::Book))
                .await;
            let (root, sandbox, _) = match resolved {
                Ok(v) => v,
                Err(err) => {
                    failed += 1;
                    items.push(NormalizeBookItemReportDto {
                        source_rel_path: rel.clone(),
                        output_rel_path: None,
                        backup_rel_path: None,
                        source_format: Path::new(rel)
                            .extension()
                            .and_then(|s| s.to_str())
                            .unwrap_or_default()
                            .to_ascii_lowercase(),
                        before_bytes: 0,
                        after_bytes: 0,
                        chapter_count: 0,
                        compressed_images: 0,
                        removed_orphans: 0,
                        deleted_source: false,
                        skipped: false,
                        skip_reason: None,
                        error: Some(err.to_string()),
                    });
                    continue;
                }
            };

            let abs = sandbox.abs_path;
            let root_path = root.path.clone();
            let opts_clone = options.clone();
            let result =
                tokio::task::spawn_blocking(move || normalize_book_to_epub(&abs, &opts_clone))
                    .await
                    .map_err(|e| DomainError::Storage(format!("normalize book task failed: {e}")))
                    .and_then(|r| r);

            match result {
                Ok(out) => {
                    if out.skipped {
                        skipped += 1;
                    } else {
                        succeeded += 1;
                        total_before_bytes = total_before_bytes.saturating_add(out.before_bytes);
                        total_after_bytes = total_after_bytes.saturating_add(out.after_bytes);
                    }
                    let output_rel = out
                        .output_path
                        .strip_prefix(&root_path)
                        .map(path_to_slash)
                        .ok();
                    let backup_rel = out
                        .backup_path
                        .as_ref()
                        .and_then(|bp| bp.strip_prefix(&root_path).ok())
                        .map(path_to_slash);
                    items.push(NormalizeBookItemReportDto {
                        source_rel_path: rel.clone(),
                        output_rel_path: output_rel,
                        backup_rel_path: backup_rel,
                        source_format: out.source_format,
                        before_bytes: out.before_bytes,
                        after_bytes: out.after_bytes,
                        chapter_count: out.chapter_count,
                        compressed_images: out.compressed_images,
                        removed_orphans: out.removed_orphans,
                        deleted_source: out.deleted_source,
                        skipped: out.skipped,
                        skip_reason: out.skip_reason,
                        error: None,
                    });
                }
                Err(err) => {
                    failed += 1;
                    items.push(NormalizeBookItemReportDto {
                        source_rel_path: rel.clone(),
                        output_rel_path: None,
                        backup_rel_path: None,
                        source_format: Path::new(rel)
                            .extension()
                            .and_then(|s| s.to_str())
                            .unwrap_or_default()
                            .to_ascii_lowercase(),
                        before_bytes: 0,
                        after_bytes: 0,
                        chapter_count: 0,
                        compressed_images: 0,
                        removed_orphans: 0,
                        deleted_source: false,
                        skipped: false,
                        skip_reason: None,
                        error: Some(err.to_string()),
                    });
                }
            }
        }

        Ok(NormalizeBooksBatchReportDto {
            total: rel_paths.len(),
            succeeded,
            skipped,
            failed,
            total_before_bytes,
            total_after_bytes,
            items,
        })
    }

    pub async fn get_comic_pages(
        &self,
        root_id: RootId,
        rel_path: &str,
    ) -> DomainResult<ComicPagesSnapshotDto> {
        let (_, sandbox, _) = self
            .resolve_media_file(root_id, rel_path, Some(MediaKind::Comic))
            .await?;
        let abs = sandbox.abs_path;
        tokio::task::spawn_blocking(move || list_comic_pages(&abs))
            .await
            .map_err(|e| DomainError::Storage(format!("list comic pages task failed: {e}")))?
    }

    pub async fn read_comic_page(
        &self,
        root_id: RootId,
        rel_path: &str,
        entry_name: &str,
        max_width: Option<u32>,
    ) -> DomainResult<BinaryPayload> {
        let (_, sandbox, _) = self
            .resolve_media_file(root_id, rel_path, Some(MediaKind::Comic))
            .await?;
        let abs = sandbox.abs_path;
        let entry = entry_name.to_string();
        tokio::task::spawn_blocking(move || read_comic_page_image(&abs, &entry, max_width))
            .await
            .map_err(|e| DomainError::Storage(format!("read comic page task failed: {e}")))?
    }

    pub async fn save_comic_pages(
        &self,
        root_id: RootId,
        rel_path: &str,
        pages: Vec<SaveComicPageInput>,
        reencode: ComicImageReencodeFormat,
        jpeg_quality: u8,
        max_width: Option<u32>,
    ) -> DomainResult<(ComicOptimizeReportDto, MediaFileDetail)> {
        let (_, sandbox, _) = self
            .resolve_writable_media_file(root_id, rel_path, Some(MediaKind::Comic))
            .await?;
        let abs = sandbox.abs_path;
        let report = tokio::task::spawn_blocking(move || {
            save_comic_pages(&abs, &pages, reencode, jpeg_quality, max_width)
        })
        .await
        .map_err(|e| DomainError::Storage(format!("save comic pages task failed: {e}")))??;
        let detail = self
            .inspect_file(root_id, rel_path, Some(MediaKind::Comic))
            .await?;
        Ok((report, detail))
    }

    pub async fn optimize_comic(
        &self,
        root_id: RootId,
        rel_path: &str,
        reencode: ComicImageReencodeFormat,
        jpeg_quality: u8,
        max_width: Option<u32>,
    ) -> DomainResult<(ComicOptimizeReportDto, MediaFileDetail)> {
        let (_, sandbox, _) = self
            .resolve_writable_media_file(root_id, rel_path, Some(MediaKind::Comic))
            .await?;
        let abs = sandbox.abs_path;
        let report = tokio::task::spawn_blocking(move || {
            optimize_comic_cbz(&abs, reencode, jpeg_quality, max_width)
        })
        .await
        .map_err(|e| DomainError::Storage(format!("optimize comic task failed: {e}")))??;
        let detail = self
            .inspect_file(root_id, rel_path, Some(MediaKind::Comic))
            .await?;
        Ok((report, detail))
    }

    pub async fn merge_comics(
        &self,
        root_id: RootId,
        source_paths: Vec<String>,
        output_rel_path: &str,
        series: Option<String>,
        title: Option<String>,
        volume: Option<i32>,
    ) -> DomainResult<(MergeComicsReportDto, MediaFileDetail)> {
        let root = self.settings.find_root(root_id).await?;
        root.ensure_writable()?;
        let mut abs_sources = Vec::with_capacity(source_paths.len());
        for rel in &source_paths {
            let sb = root.resolve_existing(rel)?;
            abs_sources.push(sb.abs_path);
        }
        let target_rel = if output_rel_path.to_ascii_lowercase().ends_with(".cbz") {
            output_rel_path.to_string()
        } else {
            format!("{output_rel_path}.cbz")
        };
        let target_abs = root.resolve_target(&target_rel)?;
        if let Some(parent) = target_abs.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                DomainError::Storage(format!("create merge output dir failed: {e}"))
            })?;
        }
        let target_rel_slash = target_abs
            .strip_prefix(&root.path)
            .map(path_to_slash)
            .unwrap_or_else(|_| target_rel.clone());
        let report_rel = target_rel_slash.clone();
        let report = tokio::task::spawn_blocking(move || {
            merge_comics_to_cbz(&abs_sources, &target_abs, report_rel, series, title, volume)
        })
        .await
        .map_err(|e| DomainError::Storage(format!("merge comics task failed: {e}")))??;
        let detail = self
            .inspect_file(root_id, &target_rel_slash, Some(MediaKind::Comic))
            .await?;
        Ok((report, detail))
    }

    pub async fn batch_sync_comic_series(
        &self,
        root_id: RootId,
        paths: Vec<String>,
        input: BatchComicSeriesSyncInput,
    ) -> DomainResult<BatchComicSeriesSyncReportDto> {
        let mut updated_count = 0usize;
        let mut failed_paths = Vec::new();
        let width = input.pad_width.clamp(1, 6);

        for (idx, rel_path) in paths.iter().enumerate() {
            let seq = input.start_index + idx as i32;
            let (number, volume) = match input.numbering_mode.as_str() {
                "chapter" => (Some(format!("{seq:0width$}")), None),
                "volume" => (None, Some(seq)),
                _ => (None, None),
            };
            let patch = MediaMetadataPatch::Comic(ComicMetaPatch {
                series: input.series.clone(),
                series_group: input.series_group.clone(),
                title: None,
                number,
                volume,
                summary: None,
                writers: input.writers.clone(),
                pencillers: input.pencillers.clone(),
                publisher: input.publisher.clone(),
                genres: input.genres.clone(),
                tags: input.tags.clone(),
                year: None,
                language_iso: None,
                reading_direction: input.reading_direction.clone(),
            });
            match self.write_metadata(root_id, rel_path, patch).await {
                Ok(_) => updated_count += 1,
                Err(_) => failed_paths.push(rel_path.clone()),
            }
        }

        Ok(BatchComicSeriesSyncReportDto {
            updated_count,
            failed_paths,
        })
    }

    pub async fn run_music_lyrics_tool(
        &self,
        root_id: RootId,
        rel_path: &str,
        input: MusicLyricsToolInput,
    ) -> DomainResult<(MusicLyricsToolReportDto, MediaFileDetail)> {
        let (_, sandbox, _) = self
            .resolve_writable_media_file(root_id, rel_path, Some(MediaKind::Music))
            .await?;
        let abs = sandbox.abs_path;
        let report = tokio::task::spawn_blocking(move || process_music_lyrics(&abs, &input))
            .await
            .map_err(|e| DomainError::Storage(format!("music lyrics tool task failed: {e}")))??;
        let detail = self
            .inspect_file(root_id, rel_path, Some(MediaKind::Music))
            .await?;
        Ok((report, detail))
    }

    pub async fn optimize_music_cover(
        &self,
        root_id: RootId,
        rel_path: &str,
        max_dimension: u32,
        jpeg_quality: u8,
    ) -> DomainResult<(MusicCoverOptimizeReportDto, MediaFileDetail)> {
        let (_, sandbox, _) = self
            .resolve_writable_media_file(root_id, rel_path, Some(MediaKind::Music))
            .await?;
        let abs = sandbox.abs_path;
        let report = tokio::task::spawn_blocking(move || {
            optimize_music_cover(&abs, max_dimension, jpeg_quality)
        })
        .await
        .map_err(|e| DomainError::Storage(format!("optimize music cover task failed: {e}")))??;
        let detail = self
            .inspect_file(root_id, rel_path, Some(MediaKind::Music))
            .await?;
        Ok((report, detail))
    }

    pub async fn apply_music_replaygain(
        &self,
        root_id: RootId,
        paths: Vec<String>,
        ffmpeg_bin: String,
    ) -> DomainResult<ReplayGainReportDto> {
        let root = self.settings.find_root(root_id).await?;
        root.ensure_writable()?;
        let mut resolved = Vec::with_capacity(paths.len());
        for rel in paths {
            if let Ok(sb) = root.resolve_existing(&rel)
                && sb.abs_path.is_file()
            {
                resolved.push((sb.rel_slash_string(), sb.abs_path));
            }
        }
        tokio::task::spawn_blocking(move || apply_replaygain_batch(&resolved, &ffmpeg_bin))
            .await
            .map_err(|e| DomainError::Storage(format!("replaygain task failed: {e}")))?
    }

    pub async fn split_music_cue(
        &self,
        root_id: RootId,
        rel_path: &str,
        ffmpeg_bin: String,
    ) -> DomainResult<CueSplitReportDto> {
        let root = self.settings.find_root(root_id).await?;
        root.ensure_writable()?;
        let sb = root.resolve_existing(rel_path)?;
        let abs = sb.abs_path;
        tokio::task::spawn_blocking(move || split_cue_album(&abs, &ffmpeg_bin))
            .await
            .map_err(|e| DomainError::Storage(format!("split cue task failed: {e}")))?
    }

    pub async fn batch_sync_music_album(
        &self,
        root_id: RootId,
        paths: Vec<String>,
        input: BatchMusicAlbumSyncInput,
    ) -> DomainResult<BatchMusicAlbumSyncReportDto> {
        let mut updated_count = 0usize;
        let mut failed_paths = Vec::new();

        for (idx, rel_path) in paths.iter().enumerate() {
            let track_number = if input.auto_number_tracks {
                Some(input.start_track + idx as u32)
            } else {
                None
            };
            let patch = MediaMetadataPatch::Music(MusicMetaPatch {
                title: None,
                artist: input.artist.clone(),
                album_artist: input.album_artist.clone(),
                album: input.album.clone(),
                track_number,
                disc_number: input.disc_number,
                year: input.year,
                genre: input.genre.clone(),
                comment: None,
                embedded_lyrics: None,
                sidecar_lrc: None,
            });
            match self.write_metadata(root_id, rel_path, patch).await {
                Ok(_) => updated_count += 1,
                Err(_) => failed_paths.push(rel_path.clone()),
            }
        }

        Ok(BatchMusicAlbumSyncReportDto {
            updated_count,
            failed_paths,
        })
    }

    pub async fn list_video_subtitles(
        &self,
        root_id: RootId,
        rel_path: &str,
    ) -> DomainResult<Vec<VideoSubtitleInfoDto>> {
        let (_, sandbox, _) = self
            .resolve_media_file(root_id, rel_path, Some(MediaKind::Video))
            .await?;
        let abs = sandbox.abs_path;
        tokio::task::spawn_blocking(move || list_video_subtitles(&abs))
            .await
            .map_err(|e| DomainError::Storage(format!("list video subtitles task failed: {e}")))?
    }

    pub async fn process_video_subtitle(
        &self,
        root_id: RootId,
        rel_path: &str,
        input: ProcessSubtitleInput,
    ) -> DomainResult<(ProcessSubtitleReportDto, MediaFileDetail)> {
        let (_, sandbox, _) = self
            .resolve_writable_media_file(root_id, rel_path, Some(MediaKind::Video))
            .await?;
        let abs = sandbox.abs_path;
        let report = tokio::task::spawn_blocking(move || process_video_subtitle(&abs, &input))
            .await
            .map_err(|e| DomainError::Storage(format!("process subtitle task failed: {e}")))??;
        let detail = self
            .inspect_file(root_id, rel_path, Some(MediaKind::Video))
            .await?;
        Ok((report, detail))
    }

    pub async fn extract_video_embedded_subtitle(
        &self,
        root_id: RootId,
        rel_path: &str,
        stream_index: u32,
        language_tag: Option<String>,
        output_ext: String,
        ffmpeg_bin: String,
    ) -> DomainResult<(ExtractSubtitleReportDto, MediaFileDetail)> {
        let (_, sandbox, _) = self
            .resolve_writable_media_file(root_id, rel_path, Some(MediaKind::Video))
            .await?;
        let abs = sandbox.abs_path;
        let report = tokio::task::spawn_blocking(move || {
            extract_embedded_subtitle(
                &abs,
                stream_index,
                language_tag.as_deref(),
                &output_ext,
                &ffmpeg_bin,
            )
        })
        .await
        .map_err(|e| DomainError::Storage(format!("extract subtitle task failed: {e}")))??;
        let detail = self
            .inspect_file(root_id, rel_path, Some(MediaKind::Video))
            .await?;
        Ok((report, detail))
    }

    pub async fn batch_remap_video_episodes(
        &self,
        root_id: RootId,
        paths: Vec<String>,
        input: BatchRemapEpisodesInput,
    ) -> DomainResult<BatchRemapEpisodesReportDto> {
        let root = self.settings.find_root(root_id).await?;
        let mut updated_count = 0usize;
        let mut failed_paths = Vec::new();
        let mut wrote_season_nfo = false;

        for (idx, rel_path) in paths.iter().enumerate() {
            let sb = match root.resolve_existing(rel_path) {
                Ok(sb) => sb,
                Err(_) => {
                    failed_paths.push(rel_path.clone());
                    continue;
                }
            };
            let base = read_base_video_meta(&sb.abs_path);
            let new_ep = if input.mode == "absolute_offset" {
                let cur = base.episode_number.unwrap_or((idx + 1) as u32) as i32;
                (cur + input.episode_offset).max(1) as u32
            } else {
                input.start_episode + idx as u32
            };

            let patch = MediaMetadataPatch::Video(VideoMetaPatch {
                is_episode: Some(true),
                show_title: input.show_title.clone(),
                season_number: input.season_number.map(|s| s as i32),
                episode_number: Some(new_ep),
                ..Default::default()
            });

            match self.write_metadata(root_id, rel_path, patch).await {
                Ok(_) => {
                    updated_count += 1;
                    if input.write_season_nfo
                        && !wrote_season_nfo
                        && let Some(season_num) = input.season_number
                        && let Some(parent) = sb.abs_path.parent()
                    {
                        let nfo = render_season_nfo_xml(
                            season_num,
                            None,
                            base.year,
                            None,
                            base.tmdb_id.as_deref(),
                            base.bangumi_id.as_deref(),
                        );
                        let _ = crate::infra::atomic_fs::atomic_write_bytes(
                            &parent.join("season.nfo"),
                            nfo.as_bytes(),
                        );
                        wrote_season_nfo = true;
                    }
                }
                Err(_) => failed_paths.push(rel_path.clone()),
            }
        }

        Ok(BatchRemapEpisodesReportDto {
            updated_count,
            failed_paths,
        })
    }

    async fn resolve_writable_media_file(
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

    async fn build_file_entry(
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

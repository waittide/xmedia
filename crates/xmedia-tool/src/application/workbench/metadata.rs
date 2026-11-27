use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::RwLock;
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;

use super::browse::WorkspaceBrowserService;
use crate::application::batch_metadata::{
    BatchMetadataEditInput, BatchMetadataEditReportDto, MetadataHistoryBatchDto,
    MetadataSnapshotRecord, UndoMetadataReportDto, build_batch_patch_for_payload,
    payload_to_restore_patch,
};
use crate::application::ops::{
    BatchComicSeriesSyncInput, BatchMusicAlbumSyncInput, BatchRemapEpisodesInput, ToolboxCommand,
    ToolboxOutput, VideoToolCommand, VideoToolOutput,
};
use crate::application::ports::{MediaToolboxPort, WorkspaceFsPort};
use crate::application::settings::ToolSettingsService;
use crate::domain::media::{
    ComicMetaPatch, MediaFileDetail, MediaMetadataPatch, MediaMetadataPayload, MusicMetaPatch,
    VideoMetaPatch,
};
use crate::domain::workspace::RootId;

pub(in crate::application) enum MetadataHistoryEvent {
    Relocated {
        root_id: RootId,
        moves: Vec<(String, String)>,
    },
    Snapshot {
        root_id: RootId,
        media_kind: MediaKind,
        rel_path: String,
        previous: Box<MediaMetadataPayload>,
    },
}

#[derive(Debug, Clone)]
pub enum BatchSyncSpec {
    ComicSeries(BatchComicSeriesSyncInput),
    MusicAlbum(BatchMusicAlbumSyncInput),
    VideoEpisodes(BatchRemapEpisodesInput),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BatchSyncReportDto {
    pub updated_count: usize,
    pub failed_paths: Vec<String>,
}

#[derive(Clone)]
pub struct MetadataService {
    settings: ToolSettingsService,
    workspace: WorkspaceBrowserService,
    fs: Arc<dyn WorkspaceFsPort>,
    toolbox: Arc<dyn MediaToolboxPort>,
    history: Arc<RwLock<Vec<MetadataHistoryBatchDto>>>,
}

impl MetadataService {
    pub fn new(
        settings: ToolSettingsService,
        workspace: WorkspaceBrowserService,
        fs: Arc<dyn WorkspaceFsPort>,
        toolbox: Arc<dyn MediaToolboxPort>,
    ) -> Self {
        Self {
            settings,
            workspace,
            fs,
            toolbox,
            history: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub(in crate::application) async fn record_history_event(&self, event: MetadataHistoryEvent) {
        match event {
            MetadataHistoryEvent::Relocated { root_id, moves } => {
                if moves.is_empty() {
                    return;
                }
                let mut guard = self.history.write().await;
                for batch in guard.iter_mut().filter(|b| b.root_id == root_id.0) {
                    for rec in &mut batch.records {
                        if let Some((_, to_rel)) =
                            moves.iter().find(|(from_rel, _)| from_rel == &rec.rel_path)
                        {
                            rec.rel_path = to_rel.clone();
                            if let Some(fname) = std::path::Path::new(to_rel)
                                .file_name()
                                .and_then(|s| s.to_str())
                            {
                                rec.file_name = fname.to_string();
                            }
                        }
                    }
                }
            }
            MetadataHistoryEvent::Snapshot {
                root_id,
                media_kind,
                rel_path,
                previous,
            } => {
                let file_name = std::path::Path::new(&rel_path)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or(&rel_path)
                    .to_string();
                let label = format!("编辑/刮削写入 · {file_name}");
                self.push_batch_snapshots(
                    root_id,
                    media_kind,
                    label,
                    vec![MetadataSnapshotRecord {
                        rel_path,
                        file_name,
                        previous: *previous,
                    }],
                )
                .await;
            }
        }
    }

    async fn push_history(&self, batch: MetadataHistoryBatchDto) {
        let mut guard = self.history.write().await;
        guard.push(batch);
        if guard.len() > 60 {
            let drain_count = guard.len() - 60;
            guard.drain(0..drain_count);
        }
    }

    async fn push_batch_snapshots(
        &self,
        root_id: RootId,
        media_kind: MediaKind,
        label: String,
        snapshots: Vec<MetadataSnapshotRecord>,
    ) -> Option<Uuid> {
        if snapshots.is_empty() {
            return None;
        }
        let batch_id = Uuid::new_v4();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let updated_count = snapshots.len();
        self.push_history(MetadataHistoryBatchDto {
            batch_id,
            root_id: root_id.0,
            media_kind,
            label,
            created_at_unix: now,
            updated_count,
            records: snapshots,
        })
        .await;
        Some(batch_id)
    }

    pub async fn list_history(&self, root_id: Option<RootId>) -> Vec<MetadataHistoryBatchDto> {
        let guard = self.history.read().await;
        guard
            .iter()
            .rev()
            .filter(|b| root_id.is_none_or(|rid| b.root_id == rid.0))
            .cloned()
            .collect()
    }

    pub async fn undo_batch(&self, batch_id: Uuid) -> DomainResult<UndoMetadataReportDto> {
        let batch = {
            let guard = self.history.read().await;
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
        let mut remaining_records = Vec::new();
        for rec in batch.records.iter().rev() {
            let restore_patch = payload_to_restore_patch(&rec.previous);
            match self
                .write_inner(RootId(batch.root_id), &rec.rel_path, restore_patch, None)
                .await
            {
                Ok(_) => restored_count += 1,
                Err(_) => {
                    failed_paths.push(rec.rel_path.clone());
                    remaining_records.push(rec.clone());
                }
            }
        }
        remaining_records.reverse();

        if restored_count > 0 {
            let mut guard = self.history.write().await;
            if remaining_records.is_empty() {
                guard.retain(|b| b.batch_id != batch_id);
            } else if let Some(existing) = guard.iter_mut().find(|b| b.batch_id == batch_id) {
                existing.records = remaining_records;
            }
        }

        Ok(UndoMetadataReportDto {
            batch_id,
            restored_count,
            failed_paths,
        })
    }

    pub async fn write(
        &self,
        root_id: RootId,
        rel_path: &str,
        patch: MediaMetadataPatch,
    ) -> DomainResult<MediaFileDetail> {
        self.write_inner(root_id, rel_path, patch, Some("编辑/刮削写入"))
            .await
    }

    async fn write_capturing_snapshot(
        &self,
        root_id: RootId,
        rel_path: &str,
        patch: MediaMetadataPatch,
    ) -> DomainResult<(MediaFileDetail, Option<MetadataSnapshotRecord>)> {
        let media_kind = patch_media_kind(&patch);
        let (root, sandbox, inspector) = self
            .workspace
            .resolve_writable_media_file(root_id, rel_path, Some(media_kind))
            .await?;
        let previous = inspector.inspect_file(&sandbox.abs_path).await.ok();
        let metadata = inspector.write_metadata(&sandbox.abs_path, &patch).await?;
        let fname = sandbox
            .abs_path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let snapshot = previous.map(|prev| MetadataSnapshotRecord {
            rel_path: rel_path.to_string(),
            file_name: fname.clone(),
            previous: prev,
        });
        let rename_templates = self.settings.get_raw().await?.rename_templates;
        let entry = self
            .workspace
            .build_entry(
                &root,
                &sandbox.abs_path,
                &fname,
                inspector.as_ref(),
                &rename_templates,
            )
            .await?;
        Ok((MediaFileDetail { entry, metadata }, snapshot))
    }

    async fn write_inner(
        &self,
        root_id: RootId,
        rel_path: &str,
        patch: MediaMetadataPatch,
        history_label: Option<&str>,
    ) -> DomainResult<MediaFileDetail> {
        let media_kind = patch_media_kind(&patch);
        if let Some(label) = history_label {
            let (detail, snapshot) = self
                .write_capturing_snapshot(root_id, rel_path, patch)
                .await?;
            if let Some(rec) = snapshot {
                let batch_label = format!("{label} · {}", rec.file_name);
                self.push_batch_snapshots(root_id, media_kind, batch_label, vec![rec])
                    .await;
            }
            return Ok(detail);
        }
        let (root, sandbox, inspector) = self
            .workspace
            .resolve_writable_media_file(root_id, rel_path, Some(media_kind))
            .await?;
        let metadata = inspector.write_metadata(&sandbox.abs_path, &patch).await?;
        let fname = sandbox
            .abs_path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let rename_templates = self.settings.get_raw().await?.rename_templates;
        let entry = self
            .workspace
            .build_entry(
                &root,
                &sandbox.abs_path,
                &fname,
                inspector.as_ref(),
                &rename_templates,
            )
            .await?;
        Ok(MediaFileDetail { entry, metadata })
    }

    pub async fn batch_edit(
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
                .workspace
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

            let Some(patch) = build_batch_patch_for_payload(&current, &file_stem, idx + 1, &input)?
            else {
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

        let label = format!("批量编辑 ({updated_count} 项)");
        let batch_id = self
            .push_batch_snapshots(root_id, input.media_kind, label, snapshots)
            .await;

        Ok(BatchMetadataEditReportDto {
            batch_id,
            updated_count,
            skipped_count,
            failed_count,
            errors,
        })
    }

    pub async fn batch_sync(
        &self,
        root_id: RootId,
        paths: Vec<String>,
        spec: BatchSyncSpec,
    ) -> DomainResult<BatchSyncReportDto> {
        let root = self.settings.get_raw().await?.find_root(root_id)?.clone();
        root.ensure_writable()?;

        match spec {
            BatchSyncSpec::ComicSeries(input) => {
                self.sync_comic_series(root_id, paths, input).await
            }
            BatchSyncSpec::MusicAlbum(input) => self.sync_music_album(root_id, paths, input).await,
            BatchSyncSpec::VideoEpisodes(input) => {
                self.remap_video_episodes(root_id, &root, paths, input)
                    .await
            }
        }
    }

    async fn sync_comic_series(
        &self,
        root_id: RootId,
        paths: Vec<String>,
        input: BatchComicSeriesSyncInput,
    ) -> DomainResult<BatchSyncReportDto> {
        let mut updated_count = 0usize;
        let mut failed_paths = Vec::new();
        let mut snapshots = Vec::new();
        let width = input.pad_width.clamp(1, 6);

        for (idx, rel_path) in paths.iter().enumerate() {
            let seq = input.start_index + idx as i32;
            let (number, volume) = match input.numbering_mode.as_str() {
                "chapter" => (Some(format!("{seq:0width$}")), None),
                "volume" => (None, Some(f64::from(seq))),
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
            match self
                .write_capturing_snapshot(root_id, rel_path, patch)
                .await
            {
                Ok((_, snapshot)) => {
                    updated_count += 1;
                    if let Some(rec) = snapshot {
                        snapshots.push(rec);
                    }
                }
                Err(_) => failed_paths.push(rel_path.clone()),
            }
        }

        self.push_batch_snapshots(
            root_id,
            MediaKind::Comic,
            format!("批量同步漫画系列 ({updated_count} 项)"),
            snapshots,
        )
        .await;

        Ok(BatchSyncReportDto {
            updated_count,
            failed_paths,
        })
    }

    async fn sync_music_album(
        &self,
        root_id: RootId,
        paths: Vec<String>,
        input: BatchMusicAlbumSyncInput,
    ) -> DomainResult<BatchSyncReportDto> {
        let mut updated_count = 0usize;
        let mut failed_paths = Vec::new();
        let mut snapshots = Vec::new();

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
            match self
                .write_capturing_snapshot(root_id, rel_path, patch)
                .await
            {
                Ok((_, snapshot)) => {
                    updated_count += 1;
                    if let Some(rec) = snapshot {
                        snapshots.push(rec);
                    }
                }
                Err(_) => failed_paths.push(rel_path.clone()),
            }
        }

        self.push_batch_snapshots(
            root_id,
            MediaKind::Music,
            format!("批量同步专辑信息 ({updated_count} 项)"),
            snapshots,
        )
        .await;

        Ok(BatchSyncReportDto {
            updated_count,
            failed_paths,
        })
    }

    async fn remap_video_episodes(
        &self,
        root_id: RootId,
        root: &crate::domain::workspace::ManagedRoot,
        paths: Vec<String>,
        input: BatchRemapEpisodesInput,
    ) -> DomainResult<BatchSyncReportDto> {
        let mut updated_count = 0usize;
        let mut failed_paths = Vec::new();
        let mut snapshots = Vec::new();
        let mut written_season_dirs: HashSet<PathBuf> = HashSet::new();

        for (idx, rel_path) in paths.iter().enumerate() {
            let Ok(sb) = self.fs.resolve_existing_in_root(root, rel_path) else {
                failed_paths.push(rel_path.clone());
                continue;
            };
            let abs_for_read = sb.abs_path.clone();
            let toolbox_read = Arc::clone(&self.toolbox);
            let base = match tokio::task::spawn_blocking(move || {
                toolbox_read.execute(ToolboxCommand::Video(VideoToolCommand::ReadBaseMeta {
                    path: abs_for_read,
                }))
            })
            .await
            {
                Ok(Ok(ToolboxOutput::Video(VideoToolOutput::BaseMeta(b)))) => *b,
                _ => {
                    failed_paths.push(rel_path.clone());
                    continue;
                }
            };

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

            match self
                .write_capturing_snapshot(root_id, rel_path, patch)
                .await
            {
                Ok((_, snapshot)) => {
                    updated_count += 1;
                    if let Some(rec) = snapshot {
                        snapshots.push(rec);
                    }
                    if input.write_season_nfo
                        && let Some(season_num) = input.season_number
                        && let Some(parent) = sb.abs_path.parent()
                        && !written_season_dirs.contains(parent)
                    {
                        let season_dir = parent.to_path_buf();
                        let season_dir_cmd = season_dir.clone();
                        let year = base.year;
                        let tmdb_id = base.tmdb_id.clone();
                        let bangumi_id = base.bangumi_id.clone();
                        let toolbox_nfo = Arc::clone(&self.toolbox);
                        if tokio::task::spawn_blocking(move || {
                            toolbox_nfo.execute(ToolboxCommand::Video(
                                VideoToolCommand::WriteSeasonNfo {
                                    season_dir: season_dir_cmd,
                                    season_num,
                                    year,
                                    tmdb_id,
                                    bangumi_id,
                                },
                            ))
                        })
                        .await
                        .is_ok_and(|r| r.is_ok())
                        {
                            written_season_dirs.insert(season_dir);
                        }
                    }
                }
                Err(_) => failed_paths.push(rel_path.clone()),
            }
        }

        self.push_batch_snapshots(
            root_id,
            MediaKind::Video,
            format!("批量重排剧集编号 ({updated_count} 项)"),
            snapshots,
        )
        .await;

        Ok(BatchSyncReportDto {
            updated_count,
            failed_paths,
        })
    }
}

fn patch_media_kind(patch: &MediaMetadataPatch) -> MediaKind {
    match patch {
        MediaMetadataPatch::Book(_) => MediaKind::Book,
        MediaMetadataPatch::Comic(_) => MediaKind::Comic,
        MediaMetadataPatch::Music(_) => MediaKind::Music,
        MediaMetadataPatch::Video(_) => MediaKind::Video,
    }
}

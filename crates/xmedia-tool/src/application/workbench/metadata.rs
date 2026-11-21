use std::time::{SystemTime, UNIX_EPOCH};

use uuid::Uuid;
use xmedia_core::application::ports::BinaryPayload;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;

use super::MediaWorkbenchService;
use crate::application::batch_metadata::{
    BatchMetadataEditInput, BatchMetadataEditReportDto, MetadataHistoryBatchDto,
    MetadataSnapshotRecord, UndoMetadataReportDto, build_batch_patch_for_payload,
    payload_to_restore_patch,
};
use crate::domain::media::{MediaFileDetail, MediaMetadataPatch};
use crate::domain::workspace::RootId;

impl MediaWorkbenchService {
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
        let mut remaining_records = Vec::new();
        for rec in batch.records.iter().rev() {
            let restore_patch = payload_to_restore_patch(&rec.previous);
            match self
                .write_metadata_inner(RootId(batch.root_id), &rec.rel_path, restore_patch, None)
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
            let mut guard = self.metadata_history.write().await;
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

    pub async fn write_metadata(
        &self,
        root_id: RootId,
        rel_path: &str,
        patch: MediaMetadataPatch,
    ) -> DomainResult<MediaFileDetail> {
        self.write_metadata_inner(root_id, rel_path, patch, Some("编辑/刮削写入"))
            .await
    }

    pub(super) async fn write_metadata_capturing_snapshot(
        &self,
        root_id: RootId,
        rel_path: &str,
        patch: MediaMetadataPatch,
    ) -> DomainResult<(MediaFileDetail, Option<MetadataSnapshotRecord>)> {
        let media_kind = match &patch {
            MediaMetadataPatch::Book(_) => MediaKind::Book,
            MediaMetadataPatch::Comic(_) => MediaKind::Comic,
            MediaMetadataPatch::Music(_) => MediaKind::Music,
            MediaMetadataPatch::Video(_) => MediaKind::Video,
        };
        let (root, sandbox, inspector) = self
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
            .build_file_entry(
                &root,
                &sandbox.abs_path,
                &fname,
                inspector.as_ref(),
                &rename_templates,
            )
            .await?;
        Ok((MediaFileDetail { entry, metadata }, snapshot))
    }

    pub(super) async fn push_batch_metadata_snapshots(
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
        self.push_metadata_history(MetadataHistoryBatchDto {
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
        if let Some(label) = history_label {
            let (detail, snapshot) = self
                .write_metadata_capturing_snapshot(root_id, rel_path, patch)
                .await?;
            if let Some(rec) = snapshot {
                let batch_label = format!("{label} · {}", rec.file_name);
                self.push_batch_metadata_snapshots(root_id, media_kind, batch_label, vec![rec])
                    .await;
            }
            return Ok(detail);
        }
        let (root, sandbox, inspector) = self
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
}

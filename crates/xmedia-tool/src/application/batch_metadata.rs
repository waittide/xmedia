mod patch_builder;
mod replacer;

#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::domain::library::MediaKind;

use crate::domain::media::{MediaMetadataPatch, MediaMetadataPayload};
use crate::domain::zh_text::ZhConvertDirection;

pub use patch_builder::PreparedBatchMetadataEdit;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataSnapshotRecord {
    pub rel_path: String,
    pub file_name: String,
    pub previous: MediaMetadataPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataHistoryBatchDto {
    pub batch_id: Uuid,
    pub root_id: Uuid,
    pub media_kind: MediaKind,
    pub label: String,
    pub created_at_unix: i64,
    pub updated_count: usize,
    pub records: Vec<MetadataSnapshotRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UndoMetadataReportDto {
    pub batch_id: Uuid,
    pub restored_count: usize,
    pub failed_paths: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BatchTextReplaceRule {
    pub find: String,
    #[serde(default)]
    pub replace: String,
    #[serde(default)]
    pub use_regex: bool,
    /// Target fields: "title", "series", "creator", "summary". Empty defaults to ["title", "series"].
    #[serde(default)]
    pub fields: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchMetadataEditInput {
    pub paths: Vec<String>,
    pub media_kind: MediaKind,
    /// Optional title template supporting `${filename}`, `${title}`, `${creator}`, `${series}`, `${year}`, `${counter}`.
    #[serde(default)]
    pub title_template: Option<String>,
    #[serde(default)]
    pub creator: Option<String>,
    #[serde(default)]
    pub series_or_album: Option<String>,
    #[serde(default)]
    pub publisher_or_studio: Option<String>,
    #[serde(default)]
    pub year: Option<i32>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub genre: Option<String>,
    #[serde(default)]
    pub add_tags: Vec<String>,
    #[serde(default)]
    pub remove_tags: Vec<String>,
    #[serde(default)]
    pub replace_rule: Option<BatchTextReplaceRule>,
    #[serde(default)]
    pub zh_direction: Option<ZhConvertDirection>,
}

impl Default for BatchMetadataEditInput {
    fn default() -> Self {
        Self {
            paths: Vec::new(),
            media_kind: MediaKind::Book,
            title_template: None,
            creator: None,
            series_or_album: None,
            publisher_or_studio: None,
            year: None,
            language: None,
            genre: None,
            add_tags: Vec::new(),
            remove_tags: Vec::new(),
            replace_rule: None,
            zh_direction: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchMetadataEditReportDto {
    pub batch_id: Option<Uuid>,
    pub updated_count: usize,
    pub skipped_count: usize,
    pub failed_count: usize,
    pub errors: Vec<String>,
}

/// Converts a full inspected `MediaMetadataPayload` back into a lossless `MediaMetadataPatch`
/// so a file can be rolled back to its exact pre-edit state.
pub fn payload_to_restore_patch(payload: &MediaMetadataPayload) -> MediaMetadataPatch {
    payload.to_restore_patch()
}

/// Computes a domain-specific `MediaMetadataPatch` for one file from a unified `BatchMetadataEditInput`.
/// Returns `Ok(None)` when no effective field changed for this file.
pub fn build_batch_patch_for_payload(
    payload: &MediaMetadataPayload,
    file_stem: &str,
    counter: usize,
    input: &BatchMetadataEditInput,
) -> DomainResult<Option<MediaMetadataPatch>> {
    let prepared = PreparedBatchMetadataEdit::prepare(input)?;
    Ok(prepared.build_patch(payload, file_stem, counter))
}

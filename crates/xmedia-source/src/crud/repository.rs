use async_trait::async_trait;
use jiff::Timestamp;
use uuid::Uuid;
use xmedia_core::domain::error::DomainResult;

use super::helpers::sync_raw_json_fields;
use crate::model::SourceRuntimeState;

/// Domain-agnostic persisted online rule-source record shared across book, comic, music, and video.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRecord {
    pub id: Uuid,
    pub name: String,
    pub source_url: String,
    pub group_name: String,
    pub enabled: bool,
    pub weight: i64,
    pub source_type: i64,
    pub raw_json: String,
    pub runtime_state: SourceRuntimeState,
    pub last_test_ok: Option<bool>,
    pub last_test_msg: Option<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl SourceRecord {
    /// Synchronizes mutable metadata (`enabled`, `weight`, `group_name`) back into `raw_json`
    /// using `group_key` so exported JSON reflects live edits and batch updates.
    pub fn sync_raw_json_fields(
        raw_json: &str,
        enabled: Option<bool>,
        weight: Option<i64>,
        group_name: Option<&str>,
        group_key: &str,
    ) -> String {
        sync_raw_json_fields(raw_json, enabled, weight, group_name, group_key)
    }
}

/// Atomic metadata patch applied to an existing `SourceRecord`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceMetadataUpdate {
    pub name: String,
    pub source_url: String,
    pub group_name: String,
    pub enabled: bool,
    pub weight: i64,
    pub source_type: i64,
    pub raw_json: String,
    pub variable: Option<String>,
    pub updated_at: Timestamp,
}

/// Persistence port for online rule-sources implemented by each media domain's Toasty table.
#[async_trait]
pub trait SourceRepository: Send + Sync {
    async fn list_all(&self) -> DomainResult<Vec<SourceRecord>>;

    async fn find_by_id(&self, id: Uuid) -> DomainResult<Option<SourceRecord>>;

    async fn save(&self, record: &SourceRecord) -> DomainResult<()>;

    async fn update_metadata_if_exists(
        &self,
        id: Uuid,
        patch: SourceMetadataUpdate,
    ) -> DomainResult<Option<SourceRecord>>;

    async fn update_runtime_state(
        &self,
        id: Uuid,
        runtime_state: SourceRuntimeState,
        updated_at: Timestamp,
    ) -> DomainResult<()>;

    async fn update_test_result(
        &self,
        id: Uuid,
        ok: bool,
        message: String,
        updated_at: Timestamp,
    ) -> DomainResult<()>;

    async fn update_weight_if_exists(
        &self,
        id: Uuid,
        weight: i64,
        updated_at: Timestamp,
    ) -> DomainResult<Option<SourceRecord>>;

    async fn update_batch_fields_if_exists(
        &self,
        id: Uuid,
        enabled: Option<bool>,
        group_name: Option<String>,
        updated_at: Timestamp,
    ) -> DomainResult<bool>;

    async fn delete(&self, id: Uuid) -> DomainResult<()>;
}

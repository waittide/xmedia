use std::sync::Arc;

use async_trait::async_trait;
use jiff::Timestamp;
use uuid::Uuid;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::domain::library::MediaKind;
use xmedia_core::domain::repository::{UserPluginRepository, UserPluginRow};
use xmedia_core::domain::user::UserId;

use super::helpers::sync_raw_json_fields;
use crate::model::{PluginFormat, SourceRuntimeState};

/// Domain-agnostic persisted plugin record isolated per `user_id` across book, comic, music, and video.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRecord {
    pub id: Uuid,
    pub user_id: Uuid,
    pub domain: MediaKind,
    pub format: PluginFormat,
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
    pub format: PluginFormat,
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

/// Persistence port for per-user plugins scoped to a media domain.
#[async_trait]
pub trait SourceRepository: Send + Sync {
    async fn list_all(&self) -> DomainResult<Vec<SourceRecord>>;

    async fn list_by_user(&self, user_id: Uuid) -> DomainResult<Vec<SourceRecord>> {
        Ok(self
            .list_all()
            .await?
            .into_iter()
            .filter(|r| r.user_id == user_id)
            .collect())
    }

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

/// Shared `SourceRepository` implementation backed by `xmedia_core`'s `user_plugins` table,
/// eliminating per-domain duplicate repository code.
#[derive(Clone)]
pub struct CoreUserPluginSourceRepository {
    plugins: Arc<dyn UserPluginRepository>,
    domain: MediaKind,
    group_key: &'static str,
}

impl CoreUserPluginSourceRepository {
    pub fn new(
        plugins: Arc<dyn UserPluginRepository>,
        domain: MediaKind,
        group_key: &'static str,
    ) -> Self {
        Self {
            plugins,
            domain,
            group_key,
        }
    }

    fn row_to_record(row: UserPluginRow) -> SourceRecord {
        let runtime_state = serde_json::from_value(row.runtime_state).unwrap_or_default();
        SourceRecord {
            id: row.id,
            user_id: row.user_id,
            domain: row.media_kind,
            format: PluginFormat::from_db_str(&row.format),
            name: row.name,
            source_url: row.source_url,
            group_name: row.group_name,
            enabled: row.enabled,
            weight: row.weight,
            source_type: row.source_type,
            raw_json: row.raw_source,
            runtime_state,
            last_test_ok: row.last_test_ok,
            last_test_msg: row.last_test_msg,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }

    fn record_to_row(&self, rec: &SourceRecord) -> UserPluginRow {
        UserPluginRow {
            id: rec.id,
            user_id: rec.user_id,
            media_kind: self.domain,
            format: rec.format.as_str().to_string(),
            name: rec.name.clone(),
            source_url: rec.source_url.clone(),
            group_name: rec.group_name.clone(),
            enabled: rec.enabled,
            weight: rec.weight,
            source_type: rec.source_type,
            raw_source: rec.raw_json.clone(),
            runtime_state: serde_json::to_value(&rec.runtime_state)
                .unwrap_or_else(|_| serde_json::json!({})),
            last_test_ok: rec.last_test_ok,
            last_test_msg: rec.last_test_msg.clone(),
            created_at: rec.created_at,
            updated_at: rec.updated_at,
        }
    }

    async fn load_domain_record(&self, id: Uuid) -> DomainResult<Option<SourceRecord>> {
        Ok(self
            .plugins
            .find_by_id(id)
            .await?
            .filter(|r| r.media_kind == self.domain)
            .map(Self::row_to_record))
    }
}

#[async_trait]
impl SourceRepository for CoreUserPluginSourceRepository {
    async fn list_all(&self) -> DomainResult<Vec<SourceRecord>> {
        let rows = self.plugins.list_all(Some(self.domain)).await?;
        Ok(rows.into_iter().map(Self::row_to_record).collect())
    }

    async fn list_by_user(&self, user_id: Uuid) -> DomainResult<Vec<SourceRecord>> {
        let rows = self
            .plugins
            .list_by_user(UserId(user_id), Some(self.domain))
            .await?;
        Ok(rows.into_iter().map(Self::row_to_record).collect())
    }

    async fn find_by_id(&self, id: Uuid) -> DomainResult<Option<SourceRecord>> {
        self.load_domain_record(id).await
    }

    async fn save(&self, record: &SourceRecord) -> DomainResult<()> {
        let row = self.record_to_row(record);
        self.plugins.save(&row).await
    }

    async fn update_metadata_if_exists(
        &self,
        id: Uuid,
        patch: SourceMetadataUpdate,
    ) -> DomainResult<Option<SourceRecord>> {
        let Some(mut rec) = self.load_domain_record(id).await? else {
            return Ok(None);
        };
        rec.format = patch.format;
        rec.name = patch.name;
        rec.source_url = patch.source_url;
        rec.group_name = patch.group_name;
        rec.enabled = patch.enabled;
        rec.weight = patch.weight;
        rec.source_type = patch.source_type;
        rec.raw_json = patch.raw_json;
        if let Some(v) = patch.variable {
            rec.runtime_state.variable = v;
        }
        rec.updated_at = patch.updated_at;
        self.save(&rec).await?;
        Ok(Some(rec))
    }

    async fn update_runtime_state(
        &self,
        id: Uuid,
        runtime_state: SourceRuntimeState,
        updated_at: Timestamp,
    ) -> DomainResult<()> {
        if let Some(mut rec) = self.load_domain_record(id).await? {
            rec.runtime_state = runtime_state;
            rec.updated_at = updated_at;
            self.save(&rec).await?;
        }
        Ok(())
    }

    async fn update_test_result(
        &self,
        id: Uuid,
        ok: bool,
        message: String,
        updated_at: Timestamp,
    ) -> DomainResult<()> {
        if let Some(mut rec) = self.load_domain_record(id).await? {
            rec.last_test_ok = Some(ok);
            rec.last_test_msg = Some(message);
            rec.updated_at = updated_at;
            self.save(&rec).await?;
        }
        Ok(())
    }

    async fn update_weight_if_exists(
        &self,
        id: Uuid,
        weight: i64,
        updated_at: Timestamp,
    ) -> DomainResult<Option<SourceRecord>> {
        let Some(mut rec) = self.load_domain_record(id).await? else {
            return Ok(None);
        };
        if rec.format == PluginFormat::JsonRule {
            rec.raw_json = SourceRecord::sync_raw_json_fields(
                &rec.raw_json,
                None,
                Some(weight),
                None,
                self.group_key,
            );
        }
        rec.weight = weight;
        rec.updated_at = updated_at;
        self.save(&rec).await?;
        Ok(Some(rec))
    }

    async fn update_batch_fields_if_exists(
        &self,
        id: Uuid,
        enabled: Option<bool>,
        group_name: Option<String>,
        updated_at: Timestamp,
    ) -> DomainResult<bool> {
        let Some(mut rec) = self.load_domain_record(id).await? else {
            return Ok(false);
        };
        if let Some(en) = enabled {
            rec.enabled = en;
        }
        if let Some(ref g) = group_name {
            rec.group_name = g.trim().to_string();
        }
        if rec.format == PluginFormat::JsonRule {
            rec.raw_json = SourceRecord::sync_raw_json_fields(
                &rec.raw_json,
                enabled,
                None,
                group_name.as_deref(),
                self.group_key,
            );
        }
        rec.updated_at = updated_at;
        self.save(&rec).await?;
        Ok(true)
    }

    async fn delete(&self, id: Uuid) -> DomainResult<()> {
        if self.load_domain_record(id).await?.is_some() {
            self.plugins.delete(id).await?;
        }
        Ok(())
    }
}

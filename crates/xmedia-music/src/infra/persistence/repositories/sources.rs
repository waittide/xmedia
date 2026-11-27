use std::sync::Arc;

use async_trait::async_trait;
use jiff::Timestamp;
use toasty::Db;
use uuid::Uuid;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::domain::library::MediaKind;
use xmedia_core::domain::repository::{UserPluginRepository, UserPluginRow};
use xmedia_core::infra::persistence::repositories::ToastyUserPluginRepository;
use xmedia_source::crud::{
    CoreUserPluginSourceRepository, SourceMetadataUpdate, SourceRecord, SourceRepository,
    SourceSpecAdapter,
};
use xmedia_source::model::{PluginFormat, SourceRuntimeState};

use super::db_err;
use crate::domain::online::MusicSourceSpec;
use crate::infra::persistence::models::MusicSourceRecordModel;

#[derive(Clone)]
pub struct ToastyMusicSourceRepository {
    inner: CoreUserPluginSourceRepository,
}

impl ToastyMusicSourceRepository {
    pub fn new(db: Db) -> Self {
        let plugins = Arc::new(ToastyUserPluginRepository::new(db));
        Self {
            inner: CoreUserPluginSourceRepository::new(
                plugins,
                MediaKind::Music,
                MusicSourceSpec::JSON_GROUP_KEY,
            ),
        }
    }
}

/// Migrates legacy unowned `music_sources` rows into `user_plugins` under `owner_user_id`.
pub async fn migrate_legacy_music_sources(
    db: &mut Db,
    owner_user_id: Uuid,
    plugins_repo: &dyn UserPluginRepository,
) -> DomainResult<usize> {
    let Ok(rows) = MusicSourceRecordModel::all().exec(db).await else {
        return Ok(0);
    };
    let mut migrated = 0usize;
    for m in rows {
        let id = m.id;
        if plugins_repo.find_by_id(id).await?.is_none() {
            plugins_repo
                .save(&UserPluginRow {
                    id,
                    user_id: owner_user_id,
                    media_kind: MediaKind::Music,
                    format: PluginFormat::JsonRule.as_str().to_string(),
                    name: m.name,
                    source_url: m.source_url,
                    group_name: m.group_name,
                    enabled: m.enabled,
                    weight: m.weight,
                    source_type: m.source_type,
                    raw_source: m.raw_json,
                    runtime_state: serde_json::to_value(m.runtime_state.0)
                        .unwrap_or_else(|_| serde_json::json!({})),
                    last_test_ok: m.last_test_ok,
                    last_test_msg: m.last_test_msg,
                    created_at: m.created_at,
                    updated_at: m.updated_at,
                })
                .await?;
            migrated += 1;
        }
        MusicSourceRecordModel::filter_by_id(id)
            .delete()
            .exec(db)
            .await
            .map_err(db_err)?;
    }
    Ok(migrated)
}

#[async_trait]
impl SourceRepository for ToastyMusicSourceRepository {
    async fn list_all(&self) -> DomainResult<Vec<SourceRecord>> {
        self.inner.list_all().await
    }

    async fn list_by_user(&self, user_id: Uuid) -> DomainResult<Vec<SourceRecord>> {
        self.inner.list_by_user(user_id).await
    }

    async fn find_by_id(&self, id: Uuid) -> DomainResult<Option<SourceRecord>> {
        self.inner.find_by_id(id).await
    }

    async fn save(&self, source: &SourceRecord) -> DomainResult<()> {
        self.inner.save(source).await
    }

    async fn update_metadata_if_exists(
        &self,
        id: Uuid,
        patch: SourceMetadataUpdate,
    ) -> DomainResult<Option<SourceRecord>> {
        self.inner.update_metadata_if_exists(id, patch).await
    }

    async fn update_runtime_state(
        &self,
        id: Uuid,
        runtime_state: SourceRuntimeState,
        updated_at: Timestamp,
    ) -> DomainResult<()> {
        self.inner
            .update_runtime_state(id, runtime_state, updated_at)
            .await
    }

    async fn update_test_result(
        &self,
        id: Uuid,
        ok: bool,
        message: String,
        updated_at: Timestamp,
    ) -> DomainResult<()> {
        self.inner
            .update_test_result(id, ok, message, updated_at)
            .await
    }

    async fn update_weight_if_exists(
        &self,
        id: Uuid,
        weight: i64,
        updated_at: Timestamp,
    ) -> DomainResult<Option<SourceRecord>> {
        self.inner
            .update_weight_if_exists(id, weight, updated_at)
            .await
    }

    async fn update_batch_fields_if_exists(
        &self,
        id: Uuid,
        enabled: Option<bool>,
        group_name: Option<String>,
        updated_at: Timestamp,
    ) -> DomainResult<bool> {
        self.inner
            .update_batch_fields_if_exists(id, enabled, group_name, updated_at)
            .await
    }

    async fn delete(&self, id: Uuid) -> DomainResult<()> {
        self.inner.delete(id).await
    }
}

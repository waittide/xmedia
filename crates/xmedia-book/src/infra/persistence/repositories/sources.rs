use std::path::PathBuf;

use async_trait::async_trait;
use jiff::Timestamp;
use toasty::Db;
use uuid::Uuid;
use xmedia_core::domain::error::DomainResult;
use xmedia_source::crud::{SourceMetadataUpdate, SourceRecord, SourceRepository};

use super::db_err;
use crate::domain::online::SourceRuntimeState;
use crate::infra::persistence::models::BookSourceRecordModel;

#[derive(Clone)]
pub struct ToastyBookSourceRepository {
    db: Db,
    cache_dir: Option<PathBuf>,
}

impl ToastyBookSourceRepository {
    pub fn new(db: Db) -> Self {
        Self {
            db,
            cache_dir: None,
        }
    }

    pub fn with_cache_dir(db: Db, cache_dir: PathBuf) -> Self {
        Self {
            db,
            cache_dir: Some(cache_dir),
        }
    }

    fn to_domain(m: BookSourceRecordModel) -> SourceRecord {
        SourceRecord {
            id: m.id,
            name: m.name,
            source_url: m.source_url,
            group_name: m.group_name,
            enabled: m.enabled,
            weight: m.weight,
            source_type: m.source_type,
            raw_json: m.raw_json,
            runtime_state: m.runtime_state.0,
            last_test_ok: m.last_test_ok,
            last_test_msg: m.last_test_msg,
            created_at: m.created_at,
            updated_at: m.updated_at,
        }
    }
}

#[async_trait]
impl SourceRepository for ToastyBookSourceRepository {
    async fn list_all(&self) -> DomainResult<Vec<SourceRecord>> {
        let mut db = self.db.clone();
        let rows = BookSourceRecordModel::all()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(rows.into_iter().map(Self::to_domain).collect())
    }

    async fn find_by_id(&self, id: Uuid) -> DomainResult<Option<SourceRecord>> {
        let mut db = self.db.clone();
        let row = BookSourceRecordModel::filter_by_id(id)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(row.map(Self::to_domain))
    }

    async fn save(&self, record: &SourceRecord) -> DomainResult<()> {
        let mut db = self.db.clone();
        let existing = BookSourceRecordModel::filter_by_id(record.id)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        if let Some(mut rec) = existing {
            rec.update()
                .name(record.name.clone())
                .source_url(record.source_url.clone())
                .group_name(record.group_name.clone())
                .enabled(record.enabled)
                .weight(record.weight)
                .source_type(record.source_type)
                .raw_json(record.raw_json.clone())
                .runtime_state(toasty::Json(record.runtime_state.clone()))
                .last_test_ok(record.last_test_ok)
                .last_test_msg(record.last_test_msg.clone())
                .updated_at(record.updated_at)
                .exec(&mut db)
                .await
                .map_err(db_err)?;
        } else {
            toasty::create!(BookSourceRecordModel {
                id: record.id,
                name: record.name.clone(),
                source_url: record.source_url.clone(),
                group_name: record.group_name.clone(),
                enabled: record.enabled,
                weight: record.weight,
                source_type: record.source_type,
                raw_json: record.raw_json.clone(),
                runtime_state: toasty::Json(record.runtime_state.clone()),
                last_test_ok: record.last_test_ok,
                last_test_msg: record.last_test_msg.clone(),
                created_at: record.created_at,
                updated_at: record.updated_at,
            })
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        }
        Ok(())
    }

    async fn update_metadata_if_exists(
        &self,
        id: Uuid,
        patch: SourceMetadataUpdate,
    ) -> DomainResult<Option<SourceRecord>> {
        let mut db = self.db.clone();
        let Some(mut rec) = BookSourceRecordModel::filter_by_id(id)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?
        else {
            return Ok(None);
        };
        let next_runtime_state = patch.variable.map(|v| {
            let mut rt = rec.runtime_state.0.clone();
            rt.variable = v;
            rt
        });
        if let Some(ref rt) = next_runtime_state {
            rec.update()
                .name(patch.name.clone())
                .source_url(patch.source_url.clone())
                .group_name(patch.group_name.clone())
                .enabled(patch.enabled)
                .weight(patch.weight)
                .source_type(patch.source_type)
                .raw_json(patch.raw_json.clone())
                .runtime_state(toasty::Json(rt.clone()))
                .updated_at(patch.updated_at)
                .exec(&mut db)
                .await
                .map_err(db_err)?;
        } else {
            rec.update()
                .name(patch.name.clone())
                .source_url(patch.source_url.clone())
                .group_name(patch.group_name.clone())
                .enabled(patch.enabled)
                .weight(patch.weight)
                .source_type(patch.source_type)
                .raw_json(patch.raw_json.clone())
                .updated_at(patch.updated_at)
                .exec(&mut db)
                .await
                .map_err(db_err)?;
        }
        let mut domain = Self::to_domain(rec);
        domain.name = patch.name;
        domain.source_url = patch.source_url;
        domain.group_name = patch.group_name;
        domain.enabled = patch.enabled;
        domain.weight = patch.weight;
        domain.source_type = patch.source_type;
        domain.raw_json = patch.raw_json;
        if let Some(rt) = next_runtime_state {
            domain.runtime_state = rt;
        }
        domain.updated_at = patch.updated_at;
        Ok(Some(domain))
    }

    async fn update_runtime_state(
        &self,
        id: Uuid,
        runtime_state: SourceRuntimeState,
        updated_at: Timestamp,
    ) -> DomainResult<()> {
        let mut db = self.db.clone();
        if let Some(mut rec) = BookSourceRecordModel::filter_by_id(id)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?
        {
            rec.update()
                .runtime_state(toasty::Json(runtime_state))
                .updated_at(updated_at)
                .exec(&mut db)
                .await
                .map_err(db_err)?;
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
        let mut db = self.db.clone();
        if let Some(mut rec) = BookSourceRecordModel::filter_by_id(id)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?
        {
            rec.update()
                .last_test_ok(Some(ok))
                .last_test_msg(Some(message))
                .updated_at(updated_at)
                .exec(&mut db)
                .await
                .map_err(db_err)?;
        }
        Ok(())
    }

    async fn update_weight_if_exists(
        &self,
        id: Uuid,
        weight: i64,
        updated_at: Timestamp,
    ) -> DomainResult<Option<SourceRecord>> {
        let mut db = self.db.clone();
        let Some(mut rec) = BookSourceRecordModel::filter_by_id(id)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?
        else {
            return Ok(None);
        };
        let next_raw = SourceRecord::sync_raw_json_fields(
            &rec.raw_json,
            None,
            Some(weight),
            None,
            "bookSourceGroup",
        );
        rec.update()
            .weight(weight)
            .raw_json(next_raw.clone())
            .updated_at(updated_at)
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        let mut domain = Self::to_domain(rec);
        domain.weight = weight;
        domain.raw_json = next_raw;
        domain.updated_at = updated_at;
        Ok(Some(domain))
    }

    async fn update_batch_fields_if_exists(
        &self,
        id: Uuid,
        enabled: Option<bool>,
        group_name: Option<String>,
        updated_at: Timestamp,
    ) -> DomainResult<bool> {
        let mut db = self.db.clone();
        let Some(mut rec) = BookSourceRecordModel::filter_by_id(id)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?
        else {
            return Ok(false);
        };
        let next_enabled = enabled.unwrap_or(rec.enabled);
        let next_group = group_name
            .as_deref()
            .map(|g| g.trim().to_string())
            .unwrap_or_else(|| rec.group_name.clone());
        let next_raw = SourceRecord::sync_raw_json_fields(
            &rec.raw_json,
            enabled,
            None,
            group_name.as_deref(),
            "bookSourceGroup",
        );
        rec.update()
            .enabled(next_enabled)
            .group_name(next_group)
            .raw_json(next_raw)
            .updated_at(updated_at)
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(true)
    }

    async fn delete(&self, id: Uuid) -> DomainResult<()> {
        let mut db = self.db.clone();
        BookSourceRecordModel::filter_by_id(id)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        if let Some(ref cache_dir) = self.cache_dir {
            let src_cache = cache_dir.join(id.to_string());
            if let Err(err) = tokio::fs::remove_dir_all(&src_cache).await
                && err.kind() != std::io::ErrorKind::NotFound
            {
                tracing::warn!(source_id = %id, error = %err, "failed to remove source cache dir");
            }
        }
        Ok(())
    }
}

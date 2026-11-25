use async_trait::async_trait;
use jiff::Timestamp;
use toasty::Db;
use uuid::Uuid;
use xmedia_core::domain::error::DomainResult;
use xmedia_source::crud::{
    SourceMetadataUpdate, SourceRecord, SourceRepository, SourceRuntimeState, SourceSpecAdapter,
};

use crate::domain::online::VideoSourceSpec;

use super::super::models::VideoSourceRecordModel;
use super::db_err;

#[derive(Clone)]
pub struct ToastyVideoSourceRepository {
    db: Db,
}

impl ToastyVideoSourceRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    fn to_record(m: VideoSourceRecordModel) -> SourceRecord {
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
impl SourceRepository for ToastyVideoSourceRepository {
    async fn list_all(&self) -> DomainResult<Vec<SourceRecord>> {
        let mut db = self.db.clone();
        let rows = VideoSourceRecordModel::all()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(rows.into_iter().map(Self::to_record).collect())
    }

    async fn find_by_id(&self, id: Uuid) -> DomainResult<Option<SourceRecord>> {
        let mut db = self.db.clone();
        let row = VideoSourceRecordModel::filter_by_id(id)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(row.map(Self::to_record))
    }

    async fn save(&self, source: &SourceRecord) -> DomainResult<()> {
        let mut db = self.db.clone();
        let existing = VideoSourceRecordModel::filter_by_id(source.id)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        if let Some(mut rec) = existing {
            rec.update()
                .name(source.name.clone())
                .source_url(source.source_url.clone())
                .group_name(source.group_name.clone())
                .enabled(source.enabled)
                .weight(source.weight)
                .source_type(source.source_type)
                .raw_json(source.raw_json.clone())
                .runtime_state(toasty::Json(source.runtime_state.clone()))
                .last_test_ok(source.last_test_ok)
                .last_test_msg(source.last_test_msg.clone())
                .updated_at(source.updated_at)
                .exec(&mut db)
                .await
                .map_err(db_err)?;
        } else {
            toasty::create!(VideoSourceRecordModel {
                id: source.id,
                name: source.name.clone(),
                source_url: source.source_url.clone(),
                group_name: source.group_name.clone(),
                enabled: source.enabled,
                weight: source.weight,
                source_type: source.source_type,
                raw_json: source.raw_json.clone(),
                runtime_state: toasty::Json(source.runtime_state.clone()),
                last_test_ok: source.last_test_ok,
                last_test_msg: source.last_test_msg.clone(),
                created_at: source.created_at,
                updated_at: source.updated_at,
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
        let Some(mut rec) = VideoSourceRecordModel::filter_by_id(id)
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
        let mut domain = Self::to_record(rec);
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
        if let Some(mut rec) = VideoSourceRecordModel::filter_by_id(id)
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
        if let Some(mut rec) = VideoSourceRecordModel::filter_by_id(id)
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
        let Some(mut rec) = VideoSourceRecordModel::filter_by_id(id)
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
            VideoSourceSpec::JSON_GROUP_KEY,
        );
        rec.update()
            .weight(weight)
            .raw_json(next_raw.clone())
            .updated_at(updated_at)
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        let mut domain = Self::to_record(rec);
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
        let Some(mut rec) = VideoSourceRecordModel::filter_by_id(id)
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
            VideoSourceSpec::JSON_GROUP_KEY,
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
        VideoSourceRecordModel::filter_by_id(id)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(())
    }
}

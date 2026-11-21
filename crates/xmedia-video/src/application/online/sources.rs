use std::collections::HashMap;
use std::sync::Arc;

use jiff::Timestamp;
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::user::{User, require_admin};
use xmedia_source::crud::{
    ParsedSourceSummaryMeta, SourceRecordSnapshot, build_source_summary, compute_reorder_targets,
    dedup_preserve_order_uuids, format_export_batch_json, merge_login_info_preserving_masked,
    merge_runtime_state,
};

use crate::domain::online::VideoSourceRecord;
use crate::source::model::{SourceRuntimeState, VideoSourceSpec};

use super::dto::{
    BatchVideoSourceAction, BatchVideoSourcesInput, BatchVideoSourcesReportDto,
    ImportVideoSourcesReportDto, LoginActionResponseDto, UpdateVideoSourceInput,
    VideoSourceSummaryDto,
};
use super::{VideoOnlineService, get_or_insert_weak_lock};

impl VideoOnlineService {
    pub(super) fn record_to_summary(
        m: &VideoSourceRecord,
        is_admin: bool,
    ) -> VideoSourceSummaryDto {
        let parsed = serde_json::from_str::<VideoSourceSpec>(&m.raw_json).ok();
        let login_ui = parsed
            .as_ref()
            .map(VideoSourceSpec::parse_login_ui)
            .unwrap_or_default();
        let has_login_url = parsed
            .as_ref()
            .and_then(|p| p.login_url.as_deref())
            .is_some_and(|s| !s.trim().is_empty())
            || !login_ui.is_empty();
        let comment = parsed.as_ref().and_then(|p| p.source_comment.clone());
        let check_keyword = parsed
            .as_ref()
            .and_then(|p| p.check_keyword.as_deref())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);

        build_source_summary(
            SourceRecordSnapshot {
                id: m.id,
                name: &m.name,
                source_url: &m.source_url,
                group_name: &m.group_name,
                enabled: m.enabled,
                weight: m.weight,
                source_type: m.source_type,
                runtime_state: &m.runtime_state,
                last_test_ok: m.last_test_ok,
                last_test_msg: m.last_test_msg.as_deref(),
                updated_at: m.updated_at,
            },
            ParsedSourceSummaryMeta {
                comment,
                check_keyword,
                has_login_url,
                login_ui,
            },
            is_admin,
        )
    }

    pub(super) async fn load_source(&self, id: Uuid) -> DomainResult<VideoSourceRecord> {
        self.sources
            .find_by_id(id)
            .await?
            .ok_or_else(|| DomainError::NotFound(format!("未找到影视源 {id}")))
    }

    pub(super) async fn load_source_for_actor(
        &self,
        actor: &User,
        id: Uuid,
    ) -> DomainResult<VideoSourceRecord> {
        let rec = self.load_source(id).await?;
        if !actor.role.is_admin() && !rec.enabled {
            return Err(DomainError::Forbidden("该影视源已被禁用".into()));
        }
        Ok(rec)
    }

    pub async fn get_source_variable(&self, actor: &User, id: Uuid) -> DomainResult<String> {
        require_admin(actor)?;
        let rec = self.load_source(id).await?;
        Ok(rec.runtime_state.variable)
    }

    pub(super) fn source_state_lock_for(&self, id: Uuid) -> Arc<tokio::sync::Mutex<()>> {
        get_or_insert_weak_lock(&self.source_state_locks, id)
    }

    pub(super) async fn persist_runtime_state(
        &self,
        id: Uuid,
        new_state: SourceRuntimeState,
    ) -> DomainResult<()> {
        let lock = self.source_state_lock_for(id);
        let _guard = lock.lock().await;
        let current = self.sources.find_by_id(id).await?.map(|r| r.runtime_state);
        let merged = merge_runtime_state(current, new_state);
        self.sources
            .update_runtime_state(id, merged, Timestamp::now())
            .await
    }

    pub async fn list_sources(&self, actor: &User) -> DomainResult<Vec<VideoSourceSummaryDto>> {
        let mut rows = self.sources.list_all().await?;
        if !actor.role.is_admin() {
            rows.retain(|r| r.enabled);
        }
        rows.sort_by(|a, b| b.weight.cmp(&a.weight).then_with(|| a.name.cmp(&b.name)));
        Ok(rows
            .iter()
            .map(|r| Self::record_to_summary(r, actor.role.is_admin()))
            .collect())
    }

    pub async fn reorder_sources(
        &self,
        actor: &User,
        ordered_ids: Vec<Uuid>,
    ) -> DomainResult<Vec<VideoSourceSummaryDto>> {
        require_admin(actor)?;
        let rows = self.sources.list_all().await?;
        let targets =
            compute_reorder_targets(rows, &ordered_ids, |r| r.id, |r| r.weight, |r| &r.name);

        let now = Timestamp::now();
        let mut persisted = Vec::with_capacity(targets.len());
        for (rec, target_weight) in targets {
            if rec.weight != target_weight {
                if let Some(updated) = self
                    .sources
                    .update_weight_if_exists(rec.id, target_weight, now)
                    .await?
                {
                    persisted.push(updated);
                }
            } else {
                persisted.push(rec);
            }
        }

        Ok(persisted
            .iter()
            .map(|r| Self::record_to_summary(r, true))
            .collect())
    }

    pub async fn import_sources(
        &self,
        actor: &User,
        raw_json: &str,
    ) -> DomainResult<ImportVideoSourcesReportDto> {
        require_admin(actor)?;
        let parsed_batch =
            VideoSourceSpec::parse_json_batch(raw_json).map_err(DomainError::Validation)?;

        let existing_all = self.sources.list_all().await?;
        let mut existing_by_key: HashMap<(String, String), VideoSourceRecord> = existing_all
            .into_iter()
            .map(|m| ((m.source_url.clone(), m.name.clone()), m))
            .collect();

        let mut imported_count = 0usize;
        let mut updated_count = 0usize;
        let mut out_by_id: HashMap<Uuid, usize> = HashMap::new();
        let mut out: Vec<VideoSourceSummaryDto> = Vec::with_capacity(parsed_batch.len());
        let now = Timestamp::now();

        for (src, item_raw) in parsed_batch {
            let group_name = src.source_group.clone().unwrap_or_default();
            let effective_url = src.effective_source_url().to_string();
            let source_type = src.source_type_code();
            let key = (effective_url.clone(), src.source_name.clone());

            if let Some(mut rec) = existing_by_key.remove(&key) {
                rec.group_name = group_name;
                rec.enabled = src.enabled;
                rec.weight = src.weight;
                rec.source_type = source_type;
                rec.raw_json = item_raw;
                rec.updated_at = now;
                self.sources.save(&rec).await?;
                let summary = Self::record_to_summary(&rec, true);
                if let Some(&pos) = out_by_id.get(&rec.id) {
                    out[pos] = summary;
                } else {
                    updated_count += 1;
                    out_by_id.insert(rec.id, out.len());
                    out.push(summary);
                }
                existing_by_key.insert(key, rec);
            } else {
                let mut rt = SourceRuntimeState::default();
                rt.ensure_android_id(&effective_url);
                let rec = VideoSourceRecord {
                    id: Uuid::now_v7(),
                    name: src.source_name.clone(),
                    source_url: effective_url,
                    group_name,
                    enabled: src.enabled,
                    weight: src.weight,
                    source_type,
                    raw_json: item_raw,
                    runtime_state: rt,
                    last_test_ok: None,
                    last_test_msg: None,
                    created_at: now,
                    updated_at: now,
                };
                self.sources.save(&rec).await?;
                imported_count += 1;
                out_by_id.insert(rec.id, out.len());
                out.push(Self::record_to_summary(&rec, true));
                existing_by_key.insert(key, rec);
            }
        }

        Ok(ImportVideoSourcesReportDto {
            imported_count,
            updated_count,
            sources: out,
        })
    }

    pub async fn update_source(
        &self,
        actor: &User,
        id: Uuid,
        input: UpdateVideoSourceInput,
    ) -> DomainResult<VideoSourceSummaryDto> {
        require_admin(actor)?;
        let mut rec = self.load_source(id).await?;
        let now = Timestamp::now();

        if let Some(raw) = input.raw_json {
            let mut batch =
                VideoSourceSpec::parse_json_batch(&raw).map_err(DomainError::Validation)?;
            if batch.len() != 1 {
                return Err(DomainError::Validation(
                    "编辑单条影视源时仅支持提交 1 条影视源 JSON".into(),
                ));
            }
            let (parsed, clean_raw) = batch.remove(0);
            rec.name = parsed.source_name.clone();
            rec.source_url = parsed.effective_source_url().to_string();
            rec.group_name = parsed.source_group.clone().unwrap_or_default();
            rec.enabled = parsed.enabled;
            rec.weight = parsed.weight;
            rec.source_type = parsed.source_type_code();
            rec.raw_json = clean_raw;
        }
        if let Some(en) = input.enabled {
            rec.enabled = en;
        }
        if let Some(w) = input.weight {
            rec.weight = w;
        }
        if let Some(ref g) = input.group_name {
            rec.group_name = g.trim().to_string();
        }
        if input.enabled.is_some() || input.weight.is_some() || input.group_name.is_some() {
            rec.raw_json = VideoSourceRecord::sync_raw_json_fields(
                &rec.raw_json,
                input.enabled,
                input.weight,
                input.group_name.as_deref(),
            );
        }
        if let Some(v) = input.variable {
            rec.runtime_state.variable = v;
        }
        rec.updated_at = now;

        if !self.sources.update_existing(&rec).await? {
            return Err(DomainError::NotFound(format!("未找到影视源 {id}")));
        }
        Ok(Self::record_to_summary(&rec, true))
    }

    pub async fn delete_source(&self, actor: &User, id: Uuid) -> DomainResult<()> {
        require_admin(actor)?;
        let _ = self.load_source(id).await?;
        self.sources.delete(id).await?;
        Ok(())
    }

    pub async fn export_source_json(&self, actor: &User, id: Uuid) -> DomainResult<String> {
        require_admin(actor)?;
        let rec = self.load_source(id).await?;
        Ok(rec.raw_json)
    }

    pub async fn export_sources_batch_json(
        &self,
        actor: &User,
        ids: Vec<Uuid>,
    ) -> DomainResult<String> {
        require_admin(actor)?;
        if ids.is_empty() {
            return Err(DomainError::Validation("请至少选择一条影视源".into()));
        }
        let all = self.sources.list_all().await?;
        let mut by_id: HashMap<Uuid, &str> =
            all.iter().map(|r| (r.id, r.raw_json.as_str())).collect();
        format_export_batch_json(&ids, &mut by_id).map_err(|msg| {
            if msg.contains("未找到") {
                DomainError::NotFound("未找到可导出的影视源".into())
            } else {
                DomainError::Storage(msg)
            }
        })
    }

    pub async fn batch_sources(
        &self,
        actor: &User,
        input: BatchVideoSourcesInput,
    ) -> DomainResult<BatchVideoSourcesReportDto> {
        require_admin(actor)?;
        if input.ids.is_empty() {
            return Err(DomainError::Validation("请至少选择一条影视源".into()));
        }

        let unique_ids = dedup_preserve_order_uuids(input.ids);
        let now = Timestamp::now();
        let mut affected_count = 0usize;

        for id in unique_ids {
            match &input.action {
                BatchVideoSourceAction::Enable => {
                    if self
                        .sources
                        .update_batch_fields_if_exists(id, Some(true), None, now)
                        .await?
                    {
                        affected_count += 1;
                    }
                }
                BatchVideoSourceAction::Disable => {
                    if self
                        .sources
                        .update_batch_fields_if_exists(id, Some(false), None, now)
                        .await?
                    {
                        affected_count += 1;
                    }
                }
                BatchVideoSourceAction::SetGroup { group_name } => {
                    if self
                        .sources
                        .update_batch_fields_if_exists(id, None, Some(group_name.clone()), now)
                        .await?
                    {
                        affected_count += 1;
                    }
                }
                BatchVideoSourceAction::Delete => {
                    if self.sources.find_by_id(id).await?.is_some() {
                        self.sources.delete(id).await?;
                        affected_count += 1;
                    }
                }
            }
        }

        Ok(BatchVideoSourcesReportDto { affected_count })
    }

    pub async fn run_login_action(
        &self,
        actor: &User,
        id: Uuid,
        login_info: HashMap<String, String>,
        action: Option<String>,
    ) -> DomainResult<LoginActionResponseDto> {
        require_admin(actor)?;
        let rec = self.load_source(id).await?;
        let parsed: VideoSourceSpec = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("影视源 JSON 无效: {e}")))?;

        let merged_login =
            merge_login_info_preserving_masked(&rec.runtime_state.login_info, login_info);

        let engine = self.engine.clone();
        let prev_state = rec.runtime_state.clone();
        let action_clone = action.clone();

        let out = tokio::task::spawn_blocking(move || {
            engine.run_login_action_sync(&parsed, prev_state, merged_login, action_clone.as_deref())
        })
        .await
        .map_err(|e| DomainError::Storage(format!("执行登录脚本线程失败: {e}")))?
        .map_err(DomainError::Validation)?;

        self.persist_runtime_state(id, out.state).await?;
        let refreshed = self.load_source(id).await?;

        Ok(LoginActionResponseDto {
            result: out.data,
            diagnostics: out.diagnostics,
            source: Self::record_to_summary(&refreshed, true),
        })
    }
}

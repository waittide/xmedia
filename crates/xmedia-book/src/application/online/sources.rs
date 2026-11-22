use std::collections::HashMap;
use std::sync::Arc;

use jiff::Timestamp;
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::user::{User, require_admin};
use xmedia_source::crud::{
    ParsedSourceSummaryMeta, SourceRecordSnapshot, build_source_summary, compute_reorder_targets,
    dedup_preserve_order_uuids, format_export_batch_json, merge_login_info_preserving_masked,
    merge_runtime_state_with_base,
};

use crate::domain::online::{BookSourceMetadataUpdate, BookSourceRecord};
use crate::source::model::{LegadoBookSource, SourceRuntimeState};

use super::BookOnlineService;
use super::dto::{
    BatchBookSourceAction, BatchBookSourcesInput, BatchBookSourcesReportDto, BookSourceSummaryDto,
    ImportBookSourcesReportDto, LoginActionResponseDto, UpdateBookSourceInput,
};
use super::helpers::get_or_insert_weak_lock;

impl BookOnlineService {
    fn record_to_summary(m: &BookSourceRecord, is_admin: bool) -> BookSourceSummaryDto {
        let parsed = serde_json::from_str::<LegadoBookSource>(&m.raw_json).ok();
        let login_ui = parsed
            .as_ref()
            .map(LegadoBookSource::parse_login_ui)
            .unwrap_or_default();
        let has_login_url = parsed
            .as_ref()
            .and_then(|p| p.login_url.as_deref())
            .is_some_and(|s| !s.trim().is_empty())
            || !login_ui.is_empty();
        let comment = parsed.as_ref().and_then(|p| p.book_source_comment.clone());
        let check_keyword = parsed
            .as_ref()
            .and_then(|p| p.rule_search.as_ref())
            .and_then(|r| r.check_key_word.as_deref())
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

    pub(super) async fn load_source(&self, id: Uuid) -> DomainResult<BookSourceRecord> {
        self.sources
            .find_by_id(id)
            .await?
            .ok_or_else(|| DomainError::NotFound(format!("未找到书源 {id}")))
    }

    pub(super) async fn load_source_for_actor(
        &self,
        actor: &User,
        id: Uuid,
    ) -> DomainResult<BookSourceRecord> {
        let rec = self.load_source(id).await?;
        if !actor.role.is_admin() && !rec.enabled {
            return Err(DomainError::Forbidden("该书源已被禁用".into()));
        }
        Ok(rec)
    }

    /// Returns the full untruncated runtime variable for a source (admin only).
    pub async fn get_source_variable(&self, actor: &User, id: Uuid) -> DomainResult<String> {
        require_admin(actor)?;
        let rec = self.load_source(id).await?;
        Ok(rec.runtime_state.variable)
    }

    pub(super) fn source_state_lock_for(&self, id: Uuid) -> Arc<tokio::sync::Mutex<()>> {
        get_or_insert_weak_lock(&self.source_state_locks, id)
    }

    pub(super) async fn persist_runtime_state_with_base(
        &self,
        id: Uuid,
        base_state: &SourceRuntimeState,
        new_state: SourceRuntimeState,
        allow_login_info_override: bool,
    ) -> DomainResult<()> {
        let lock = self.source_state_lock_for(id);
        let _guard = lock.lock().await;
        let current = self.sources.find_by_id(id).await?.map(|r| r.runtime_state);
        let merged = merge_runtime_state_with_base(
            base_state,
            current,
            new_state,
            allow_login_info_override,
        );
        self.sources
            .update_runtime_state(id, merged, Timestamp::now())
            .await
    }

    pub async fn list_sources(&self, actor: &User) -> DomainResult<Vec<BookSourceSummaryDto>> {
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
    ) -> DomainResult<Vec<BookSourceSummaryDto>> {
        require_admin(actor)?;
        let rows = self.sources.list_all().await?;
        let targets =
            compute_reorder_targets(rows, &ordered_ids, |r| r.id, |r| r.weight, |r| &r.name);

        let now = Timestamp::now();
        let mut persisted = Vec::with_capacity(targets.len());
        for (rec, target_weight) in targets {
            let lock = self.source_state_lock_for(rec.id);
            let _guard = lock.lock().await;
            if rec.weight != target_weight {
                if let Some(updated) = self
                    .sources
                    .update_weight_if_exists(rec.id, target_weight, now)
                    .await?
                {
                    persisted.push(updated);
                }
            } else if let Some(fresh) = self.sources.find_by_id(rec.id).await? {
                persisted.push(fresh);
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
    ) -> DomainResult<ImportBookSourcesReportDto> {
        require_admin(actor)?;
        let parsed_batch =
            LegadoBookSource::parse_json_batch(raw_json).map_err(DomainError::Validation)?;

        let _seed_guard = self.seed_import_lock.lock().await;
        let existing_all = self.sources.list_all().await?;
        let mut existing_by_key: HashMap<(String, String), BookSourceRecord> = existing_all
            .into_iter()
            .map(|m| ((m.source_url.clone(), m.name.clone()), m))
            .collect();

        let mut imported_count = 0usize;
        let mut updated_count = 0usize;
        let mut out_by_id: HashMap<Uuid, usize> = HashMap::new();
        let mut out: Vec<BookSourceSummaryDto> = Vec::with_capacity(parsed_batch.len());
        let now = Timestamp::now();

        for (src, item_raw) in parsed_batch {
            let group_name = src.book_source_group.clone().unwrap_or_default();
            let key = (src.book_source_url.clone(), src.book_source_name.clone());

            if let Some(existing_rec) = existing_by_key.remove(&key) {
                let id = existing_rec.id;
                let lock = self.source_state_lock_for(id);
                let _guard = lock.lock().await;
                let patch = BookSourceMetadataUpdate {
                    name: existing_rec.name.clone(),
                    source_url: existing_rec.source_url.clone(),
                    group_name: group_name.clone(),
                    enabled: src.enabled,
                    weight: src.weight,
                    source_type: src.book_source_type,
                    raw_json: item_raw.clone(),
                    variable: None,
                    updated_at: now,
                };
                if let Some(rec) = self.sources.update_metadata_if_exists(id, patch).await? {
                    let summary = Self::record_to_summary(&rec, true);
                    if let Some(&pos) = out_by_id.get(&rec.id) {
                        out[pos] = summary;
                    } else {
                        updated_count += 1;
                        out_by_id.insert(rec.id, out.len());
                        out.push(summary);
                    }
                    // Re-insert so duplicate keys within the same batch update this record
                    existing_by_key.insert(key, rec);
                    continue;
                }
            }

            let mut rt = SourceRuntimeState::default();
            rt.ensure_android_id(&src.book_source_url);
            let rec = BookSourceRecord {
                id: Uuid::now_v7(),
                name: src.book_source_name.clone(),
                source_url: src.book_source_url.clone(),
                group_name,
                enabled: src.enabled,
                weight: src.weight,
                source_type: src.book_source_type,
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

        Ok(ImportBookSourcesReportDto {
            imported_count,
            updated_count,
            sources: out,
        })
    }

    pub async fn update_source(
        &self,
        actor: &User,
        id: Uuid,
        input: UpdateBookSourceInput,
    ) -> DomainResult<BookSourceSummaryDto> {
        require_admin(actor)?;
        let parsed_raw = if let Some(raw) = input.raw_json {
            let mut batch =
                LegadoBookSource::parse_json_batch(&raw).map_err(DomainError::Validation)?;
            if batch.len() != 1 {
                return Err(DomainError::Validation(
                    "编辑单条书源时仅支持提交 1 条书源 JSON".into(),
                ));
            }
            Some(batch.remove(0))
        } else {
            None
        };

        let lock = self.source_state_lock_for(id);
        let _guard = lock.lock().await;
        let mut rec = self.load_source(id).await?;
        let now = Timestamp::now();

        if let Some((parsed, clean_raw)) = parsed_raw {
            rec.name = parsed.book_source_name;
            rec.source_url = parsed.book_source_url;
            rec.group_name = parsed.book_source_group.unwrap_or_default();
            rec.enabled = parsed.enabled;
            rec.weight = parsed.weight;
            rec.source_type = parsed.book_source_type;
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
            rec.raw_json = BookSourceRecord::sync_raw_json_fields(
                &rec.raw_json,
                input.enabled,
                input.weight,
                input.group_name.as_deref(),
            );
        }

        let patch = BookSourceMetadataUpdate {
            name: rec.name,
            source_url: rec.source_url,
            group_name: rec.group_name,
            enabled: rec.enabled,
            weight: rec.weight,
            source_type: rec.source_type,
            raw_json: rec.raw_json,
            variable: input.variable,
            updated_at: now,
        };
        let updated = self
            .sources
            .update_metadata_if_exists(id, patch)
            .await?
            .ok_or_else(|| DomainError::NotFound(format!("未找到书源 {id}")))?;
        Ok(Self::record_to_summary(&updated, true))
    }

    pub async fn delete_source(&self, actor: &User, id: Uuid) -> DomainResult<()> {
        require_admin(actor)?;
        let lock = self.source_state_lock_for(id);
        let _guard = lock.lock().await;
        let _ = self.load_source(id).await?;
        self.sources.delete(id).await?;
        let src_cache = self.cache_dir.join(id.to_string());
        if let Err(err) = tokio::fs::remove_dir_all(&src_cache).await
            && err.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(source_id = %id, error = %err, "failed to remove source cache dir");
        }
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
            return Err(DomainError::Validation("请至少选择一条书源".into()));
        }
        let all = self.sources.list_all().await?;
        let mut by_id: HashMap<Uuid, &str> =
            all.iter().map(|r| (r.id, r.raw_json.as_str())).collect();
        format_export_batch_json(&ids, &mut by_id).map_err(|msg| {
            if msg.contains("未找到") {
                DomainError::NotFound("未找到可导出的书源".into())
            } else {
                DomainError::Storage(msg)
            }
        })
    }

    pub async fn batch_sources(
        &self,
        actor: &User,
        input: BatchBookSourcesInput,
    ) -> DomainResult<BatchBookSourcesReportDto> {
        require_admin(actor)?;
        if input.ids.is_empty() {
            return Err(DomainError::Validation("请至少选择一条书源".into()));
        }

        let unique_ids = dedup_preserve_order_uuids(input.ids);
        let now = Timestamp::now();
        let mut affected_count = 0usize;

        for id in unique_ids {
            let lock = self.source_state_lock_for(id);
            let _guard = lock.lock().await;
            match &input.action {
                BatchBookSourceAction::Enable => {
                    if self
                        .sources
                        .update_batch_fields_if_exists(id, Some(true), None, now)
                        .await?
                    {
                        affected_count += 1;
                    }
                }
                BatchBookSourceAction::Disable => {
                    if self
                        .sources
                        .update_batch_fields_if_exists(id, Some(false), None, now)
                        .await?
                    {
                        affected_count += 1;
                    }
                }
                BatchBookSourceAction::SetGroup { group_name } => {
                    if self
                        .sources
                        .update_batch_fields_if_exists(id, None, Some(group_name.clone()), now)
                        .await?
                    {
                        affected_count += 1;
                    }
                }
                BatchBookSourceAction::Delete => {
                    if self.sources.find_by_id(id).await?.is_some() {
                        self.sources.delete(id).await?;
                        let src_cache = self.cache_dir.join(id.to_string());
                        if let Err(err) = tokio::fs::remove_dir_all(&src_cache).await
                            && err.kind() != std::io::ErrorKind::NotFound
                        {
                            tracing::warn!(source_id = %id, error = %err, "failed to remove source cache dir");
                        }
                        affected_count += 1;
                    }
                }
            }
        }

        Ok(BatchBookSourcesReportDto { affected_count })
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
        let parsed: LegadoBookSource = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("书源 JSON 无效: {e}")))?;

        let merged_login =
            merge_login_info_preserving_masked(&rec.runtime_state.login_info, login_info);

        let engine = self.engine.clone();
        let prev_state = rec.runtime_state.clone();
        let base_state = prev_state.clone();
        let action_clone = action.clone();

        let out = tokio::task::spawn_blocking(move || {
            engine.run_login_action_sync(&parsed, prev_state, merged_login, action_clone.as_deref())
        })
        .await
        .map_err(|e| DomainError::Storage(format!("执行登录脚本线程失败: {e}")))?
        .map_err(DomainError::Validation)?;

        self.persist_runtime_state_with_base(id, &base_state, out.state, true)
            .await?;
        let refreshed = self.load_source(id).await?;

        Ok(LoginActionResponseDto {
            result: out.data,
            diagnostics: out.diagnostics,
            source: Self::record_to_summary(&refreshed, true),
        })
    }
}

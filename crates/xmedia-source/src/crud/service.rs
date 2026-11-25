use std::borrow::Cow;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::{Arc, Weak};

use async_trait::async_trait;
use jiff::Timestamp;
use serde::de::DeserializeOwned;
use uuid::Uuid;
use xmedia_core::application::online_items::{OnlineSourceDirectory, OnlineSourceInfo};
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::user::User;

use super::dto::{
    BatchSourceAction, BatchSourcesInput, BatchSourcesReportDto, ImportSourcesReportDto,
    LoginActionResponseDto, SourceSummaryDto, UpdateSourceInput,
};
use super::helpers::{
    ParsedSourceSummaryMeta, SourceRecordSnapshot, build_source_summary, compute_reorder_targets,
    dedup_preserve_order_uuids, format_export_batch_json, get_or_insert_weak_lock,
    merge_login_info_preserving_masked, merge_runtime_state_with_base, parse_json_batch,
    sync_raw_json_fields,
};
use super::repository::{SourceMetadataUpdate, SourceRecord, SourceRepository};
use crate::model::{SourceCallError, SourceCallOutput, SourceRuntimeState};

/// Adapter connecting a domain-specific rule specification (`LegadoBookSource`, `ComicSourceSpec`,
/// `MusicSourceSpec`, `VideoSourceSpec`) to [`SourceCrudService`].
pub trait SourceSpecAdapter: DeserializeOwned + Send + Sync + 'static {
    /// Human-readable domain label used in validation & not-found messages (e.g. `"书源"`, `"影视源"`).
    const ENTITY_LABEL: &'static str;

    /// JSON property key storing the group name inside `raw_json` (e.g. `"bookSourceGroup"` or `"sourceGroup"`).
    const JSON_GROUP_KEY: &'static str;

    /// Returns `true` when a deserialized entry is empty/blank and should be skipped during batch import.
    fn is_blank(&self) -> bool {
        self.source_name().trim().is_empty() && self.effective_source_url().trim().is_empty()
    }

    /// Parses a single JSON object or array into validated `(Spec, raw_json)` pairs.
    fn parse_batch(raw_json: &str) -> Result<Vec<(Self, String)>, String> {
        let empty_hint = format!("未解析到有效的{}", Self::ENTITY_LABEL);
        parse_json_batch(raw_json, Self::ENTITY_LABEL, &empty_hint, Self::is_blank)
    }

    fn source_name(&self) -> &str;
    fn effective_source_url(&self) -> Cow<'_, str>;
    fn group_name(&self) -> Option<&str>;
    fn enabled(&self) -> bool;
    fn weight(&self) -> i64;
    fn source_type_code(&self) -> i64;
    fn summary_meta(&self) -> ParsedSourceSummaryMeta;
}

/// Generic online source CRUD, import/export, weight reorder, runtime-state merge, and login-action
/// application service reusable across all media domains.
pub struct SourceCrudService<Spec: SourceSpecAdapter> {
    repo: Arc<dyn SourceRepository>,
    seed_import_lock: tokio::sync::Mutex<()>,
    state_locks: std::sync::Mutex<HashMap<Uuid, Weak<tokio::sync::Mutex<()>>>>,
    _marker: PhantomData<fn() -> Spec>,
}

impl<Spec: SourceSpecAdapter> SourceCrudService<Spec> {
    pub fn new(repo: Arc<dyn SourceRepository>) -> Self {
        Self {
            repo,
            seed_import_lock: tokio::sync::Mutex::new(()),
            state_locks: std::sync::Mutex::new(HashMap::new()),
            _marker: PhantomData,
        }
    }

    pub fn repository(&self) -> &Arc<dyn SourceRepository> {
        &self.repo
    }

    pub fn state_lock_for(&self, id: Uuid) -> Arc<tokio::sync::Mutex<()>> {
        get_or_insert_weak_lock(&self.state_locks, id)
    }

    pub fn record_to_summary(rec: &SourceRecord, is_admin: bool) -> SourceSummaryDto {
        let meta = serde_json::from_str::<Spec>(&rec.raw_json)
            .ok()
            .map(|spec| spec.summary_meta())
            .unwrap_or_default();

        build_source_summary(
            SourceRecordSnapshot {
                id: rec.id,
                name: &rec.name,
                source_url: &rec.source_url,
                group_name: &rec.group_name,
                enabled: rec.enabled,
                weight: rec.weight,
                source_type: rec.source_type,
                runtime_state: &rec.runtime_state,
                last_test_ok: rec.last_test_ok,
                last_test_msg: rec.last_test_msg.as_deref(),
                updated_at: rec.updated_at,
            },
            meta,
            is_admin,
        )
    }

    pub async fn load_source(&self, id: Uuid) -> DomainResult<SourceRecord> {
        self.repo
            .find_by_id(id)
            .await?
            .ok_or_else(|| DomainError::NotFound(format!("未找到{} {id}", Spec::ENTITY_LABEL)))
    }

    pub async fn load_source_for_actor(
        &self,
        actor: &User,
        id: Uuid,
    ) -> DomainResult<SourceRecord> {
        let rec = self.load_source(id).await?;
        if !actor.role.is_admin() && !rec.enabled {
            return Err(DomainError::Forbidden(format!(
                "该{}已被禁用",
                Spec::ENTITY_LABEL
            )));
        }
        Ok(rec)
    }

    pub async fn get_source(&self, actor: &User, id: Uuid) -> DomainResult<SourceSummaryDto> {
        let rec = self.load_source_for_actor(actor, id).await?;
        Ok(Self::record_to_summary(&rec, actor.role.is_admin()))
    }

    pub async fn list_sources(&self, actor: &User) -> DomainResult<Vec<SourceSummaryDto>> {
        let mut rows = self.repo.list_all().await?;
        if !actor.role.is_admin() {
            rows.retain(|r| r.enabled);
        }
        rows.sort_by(|a, b| b.weight.cmp(&a.weight).then_with(|| a.name.cmp(&b.name)));
        Ok(rows
            .iter()
            .map(|r| Self::record_to_summary(r, actor.role.is_admin()))
            .collect())
    }

    pub async fn get_source_variable(&self, actor: &User, id: Uuid) -> DomainResult<String> {
        actor.ensure_admin()?;
        let rec = self.load_source(id).await?;
        Ok(rec.runtime_state.variable)
    }

    pub async fn persist_runtime_state_with_base(
        &self,
        id: Uuid,
        base_state: &SourceRuntimeState,
        new_state: SourceRuntimeState,
        allow_login_info_override: bool,
    ) -> DomainResult<()> {
        let lock = self.state_lock_for(id);
        let _guard = lock.lock().await;
        let current = self.repo.find_by_id(id).await?.map(|r| r.runtime_state);
        let merged = merge_runtime_state_with_base(
            base_state,
            current,
            new_state,
            allow_login_info_override,
        );
        self.repo
            .update_runtime_state(id, merged, Timestamp::now())
            .await
    }

    pub async fn record_test_outcome(
        &self,
        id: Uuid,
        ok: bool,
        message: String,
    ) -> DomainResult<()> {
        let lock = self.state_lock_for(id);
        let _guard = lock.lock().await;
        self.repo
            .update_test_result(id, ok, message, Timestamp::now())
            .await
    }

    /// Creates or upserts a single source from `raw_json` (admin only).
    pub async fn create_or_upsert_source(
        &self,
        actor: &User,
        raw_json: &str,
    ) -> DomainResult<SourceSummaryDto> {
        actor.ensure_admin()?;
        let parsed = Spec::parse_batch(raw_json).map_err(DomainError::Validation)?;
        if parsed.len() != 1 {
            return Err(DomainError::Validation(format!(
                "单条创建仅支持提交 1 条{} JSON",
                Spec::ENTITY_LABEL
            )));
        }
        let mut report = self.import_parsed_batch(parsed).await?;
        report
            .sources
            .pop()
            .ok_or_else(|| DomainError::Validation(format!("未解析到有效的{}", Spec::ENTITY_LABEL)))
    }

    pub async fn import_sources(
        &self,
        actor: &User,
        raw_json: &str,
    ) -> DomainResult<ImportSourcesReportDto> {
        actor.ensure_admin()?;
        let parsed_batch = Spec::parse_batch(raw_json).map_err(DomainError::Validation)?;
        self.import_parsed_batch(parsed_batch).await
    }

    /// Seeds built-in default sources without an actor check when the repository is empty or idempotently.
    pub async fn import_seed_sources(
        &self,
        raw_json: &str,
    ) -> DomainResult<ImportSourcesReportDto> {
        let parsed_batch = Spec::parse_batch(raw_json).map_err(DomainError::Validation)?;
        self.import_parsed_batch(parsed_batch).await
    }

    async fn import_parsed_batch(
        &self,
        parsed_batch: Vec<(Spec, String)>,
    ) -> DomainResult<ImportSourcesReportDto> {
        let _seed_guard = self.seed_import_lock.lock().await;
        let existing_all = self.repo.list_all().await?;
        let mut existing_by_key: HashMap<(String, String), SourceRecord> = existing_all
            .into_iter()
            .map(|m| ((m.source_url.clone(), m.name.clone()), m))
            .collect();

        let mut imported_count = 0usize;
        let mut updated_count = 0usize;
        let mut out_by_id: HashMap<Uuid, usize> = HashMap::new();
        let mut out: Vec<SourceSummaryDto> = Vec::with_capacity(parsed_batch.len());
        let now = Timestamp::now();

        for (src, item_raw) in parsed_batch {
            let name = src.source_name().trim().to_string();
            let group_name = src.group_name().unwrap_or_default().trim().to_string();
            let effective_url = src.effective_source_url().into_owned();
            let enabled = src.enabled();
            let weight = src.weight();
            let source_type = src.source_type_code();
            let key = (effective_url.clone(), name.clone());

            if let Some(existing_rec) = existing_by_key.remove(&key) {
                let id = existing_rec.id;
                let lock = self.state_lock_for(id);
                let _guard = lock.lock().await;
                let patch = SourceMetadataUpdate {
                    name: existing_rec.name.clone(),
                    source_url: existing_rec.source_url.clone(),
                    group_name: group_name.clone(),
                    enabled,
                    weight,
                    source_type,
                    raw_json: item_raw.clone(),
                    variable: None,
                    updated_at: now,
                };
                if let Some(rec) = self.repo.update_metadata_if_exists(id, patch).await? {
                    let summary = Self::record_to_summary(&rec, true);
                    if let Some(&pos) = out_by_id.get(&rec.id) {
                        out[pos] = summary;
                    } else {
                        updated_count += 1;
                        out_by_id.insert(rec.id, out.len());
                        out.push(summary);
                    }
                    existing_by_key.insert(key, rec);
                    continue;
                }
            }

            let mut rt = SourceRuntimeState::default();
            rt.ensure_android_id(&effective_url);
            let rec = SourceRecord {
                id: Uuid::now_v7(),
                name,
                source_url: effective_url,
                group_name,
                enabled,
                weight,
                source_type,
                raw_json: item_raw,
                runtime_state: rt,
                last_test_ok: None,
                last_test_msg: None,
                created_at: now,
                updated_at: now,
            };
            self.repo.save(&rec).await?;
            imported_count += 1;
            out_by_id.insert(rec.id, out.len());
            out.push(Self::record_to_summary(&rec, true));
            existing_by_key.insert(key, rec);
        }

        Ok(ImportSourcesReportDto {
            imported_count,
            updated_count,
            sources: out,
        })
    }

    pub async fn update_source(
        &self,
        actor: &User,
        id: Uuid,
        input: UpdateSourceInput,
    ) -> DomainResult<SourceSummaryDto> {
        actor.ensure_admin()?;
        let parsed_raw = if let Some(raw) = input.raw_json {
            let mut batch = Spec::parse_batch(&raw).map_err(DomainError::Validation)?;
            if batch.len() != 1 {
                return Err(DomainError::Validation(format!(
                    "编辑单条{}时仅支持提交 1 条{} JSON",
                    Spec::ENTITY_LABEL,
                    Spec::ENTITY_LABEL
                )));
            }
            Some(batch.remove(0))
        } else {
            None
        };

        let lock = self.state_lock_for(id);
        let _guard = lock.lock().await;
        let mut rec = self.load_source(id).await?;
        let now = Timestamp::now();

        if let Some((parsed, clean_raw)) = parsed_raw {
            rec.name = parsed.source_name().trim().to_string();
            rec.source_url = parsed.effective_source_url().into_owned();
            rec.group_name = parsed.group_name().unwrap_or_default().trim().to_string();
            rec.enabled = parsed.enabled();
            rec.weight = parsed.weight();
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
            rec.raw_json = sync_raw_json_fields(
                &rec.raw_json,
                input.enabled,
                input.weight,
                input.group_name.as_deref(),
                Spec::JSON_GROUP_KEY,
            );
        }

        let patch = SourceMetadataUpdate {
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
            .repo
            .update_metadata_if_exists(id, patch)
            .await?
            .ok_or_else(|| DomainError::NotFound(format!("未找到{} {id}", Spec::ENTITY_LABEL)))?;
        Ok(Self::record_to_summary(&updated, true))
    }

    pub async fn set_source_enabled(
        &self,
        actor: &User,
        id: Uuid,
        enabled: bool,
    ) -> DomainResult<SourceSummaryDto> {
        self.update_source(
            actor,
            id,
            UpdateSourceInput {
                enabled: Some(enabled),
                ..Default::default()
            },
        )
        .await
    }

    pub async fn reorder_sources(
        &self,
        actor: &User,
        ordered_ids: Vec<Uuid>,
    ) -> DomainResult<Vec<SourceSummaryDto>> {
        actor.ensure_admin()?;
        let rows = self.repo.list_all().await?;
        let targets =
            compute_reorder_targets(rows, &ordered_ids, |r| r.id, |r| r.weight, |r| &r.name);

        let now = Timestamp::now();
        let mut persisted = Vec::with_capacity(targets.len());
        for (rec, target_weight) in targets {
            let lock = self.state_lock_for(rec.id);
            let _guard = lock.lock().await;
            if rec.weight != target_weight {
                if let Some(updated) = self
                    .repo
                    .update_weight_if_exists(rec.id, target_weight, now)
                    .await?
                {
                    persisted.push(updated);
                }
            } else if let Some(fresh) = self.repo.find_by_id(rec.id).await? {
                persisted.push(fresh);
            }
        }

        Ok(persisted
            .iter()
            .map(|r| Self::record_to_summary(r, true))
            .collect())
    }

    pub async fn batch_sources(
        &self,
        actor: &User,
        input: BatchSourcesInput,
    ) -> DomainResult<BatchSourcesReportDto> {
        actor.ensure_admin()?;
        if input.ids.is_empty() {
            return Err(DomainError::Validation(format!(
                "请至少选择一条{}",
                Spec::ENTITY_LABEL
            )));
        }

        let unique_ids = dedup_preserve_order_uuids(input.ids);
        let now = Timestamp::now();
        let mut affected_count = 0usize;

        for id in unique_ids {
            let lock = self.state_lock_for(id);
            let _guard = lock.lock().await;
            match &input.action {
                BatchSourceAction::Enable => {
                    if self
                        .repo
                        .update_batch_fields_if_exists(id, Some(true), None, now)
                        .await?
                    {
                        affected_count += 1;
                    }
                }
                BatchSourceAction::Disable => {
                    if self
                        .repo
                        .update_batch_fields_if_exists(id, Some(false), None, now)
                        .await?
                    {
                        affected_count += 1;
                    }
                }
                BatchSourceAction::SetGroup { group_name } => {
                    if self
                        .repo
                        .update_batch_fields_if_exists(id, None, Some(group_name.clone()), now)
                        .await?
                    {
                        affected_count += 1;
                    }
                }
                BatchSourceAction::Delete => {
                    if self.repo.find_by_id(id).await?.is_some() {
                        self.repo.delete(id).await?;
                        affected_count += 1;
                    }
                }
            }
        }

        Ok(BatchSourcesReportDto { affected_count })
    }

    pub async fn export_source_json(&self, actor: &User, id: Uuid) -> DomainResult<String> {
        actor.ensure_admin()?;
        let rec = self.load_source(id).await?;
        Ok(rec.raw_json)
    }

    pub async fn export_sources_batch_json(
        &self,
        actor: &User,
        ids: Vec<Uuid>,
    ) -> DomainResult<String> {
        actor.ensure_admin()?;
        if ids.is_empty() {
            return Err(DomainError::Validation(format!(
                "请至少选择一条{}",
                Spec::ENTITY_LABEL
            )));
        }
        let all = self.repo.list_all().await?;
        let mut by_id: HashMap<Uuid, &str> =
            all.iter().map(|r| (r.id, r.raw_json.as_str())).collect();
        format_export_batch_json(&ids, &mut by_id).map_err(|msg| {
            if msg.contains("未找到") {
                DomainError::NotFound(format!("未找到可导出的{}", Spec::ENTITY_LABEL))
            } else {
                DomainError::Storage(msg)
            }
        })
    }

    pub async fn delete_source(&self, actor: &User, id: Uuid) -> DomainResult<()> {
        actor.ensure_admin()?;
        let lock = self.state_lock_for(id);
        let _guard = lock.lock().await;
        let _ = self.load_source(id).await?;
        self.repo.delete(id).await
    }

    /// Runs a blocking JS login action (`exec_sync`), merges its mutated `SourceRuntimeState`
    /// atomically against the pre-call base state while preserving masked credentials, and returns
    /// `LoginActionResponseDto`.
    pub async fn run_login_action_with<F>(
        &self,
        actor: &User,
        id: Uuid,
        login_info: HashMap<String, String>,
        action: Option<String>,
        exec_sync: F,
    ) -> DomainResult<LoginActionResponseDto>
    where
        F: FnOnce(
                &Spec,
                SourceRuntimeState,
                HashMap<String, String>,
                Option<&str>,
            ) -> Result<SourceCallOutput<String>, SourceCallError>
            + Send
            + 'static,
    {
        actor.ensure_admin()?;
        let rec = self.load_source(id).await?;
        let parsed: Spec = serde_json::from_str(&rec.raw_json).map_err(|e| {
            DomainError::Validation(format!("{} JSON 无效: {e}", Spec::ENTITY_LABEL))
        })?;

        let merged_login =
            merge_login_info_preserving_masked(&rec.runtime_state.login_info, login_info);
        let prev_state = rec.runtime_state.clone();
        let base_state = prev_state.clone();

        let out = tokio::task::spawn_blocking(move || {
            exec_sync(&parsed, prev_state, merged_login, action.as_deref())
        })
        .await
        .map_err(|e| DomainError::Storage(format!("执行登录脚本线程失败: {e}")))?
        .map_err(|e| DomainError::Validation(e.message))?;

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

#[async_trait]
impl<Spec: SourceSpecAdapter> OnlineSourceDirectory for SourceCrudService<Spec> {
    async fn list_sources(&self) -> DomainResult<HashMap<Uuid, OnlineSourceInfo>> {
        let rows = self.repo.list_all().await?;
        Ok(rows
            .into_iter()
            .map(|r| {
                (
                    r.id,
                    OnlineSourceInfo {
                        name: r.name,
                        enabled: r.enabled,
                    },
                )
            })
            .collect())
    }

    async fn find_source(&self, id: Uuid) -> DomainResult<Option<OnlineSourceInfo>> {
        Ok(self.repo.find_by_id(id).await?.map(|r| OnlineSourceInfo {
            name: r.name,
            enabled: r.enabled,
        }))
    }
}

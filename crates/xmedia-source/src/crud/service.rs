use std::borrow::Cow;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::Arc;

use async_trait::async_trait;
use jiff::Timestamp;
use serde::de::DeserializeOwned;
use uuid::Uuid;
use xmedia_core::application::online_items::{OnlineSourceDirectory, OnlineSourceInfo};
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;
use xmedia_core::domain::user::{User, UserId};

use super::dto::{
    BatchSourceAction, BatchSourcesInput, BatchSourcesReportDto, ImportSourcesReportDto,
    LoginActionResponseDto, SourceSummaryDto, UpdateSourceInput,
};
use super::helpers::{
    ParsedSourceSummaryMeta, SourceRecordSnapshot, build_source_summary, compute_reorder_targets,
    dedup_preserve_order_uuids, format_export_batch_json, merge_login_info_preserving_masked,
    merge_runtime_state_with_base, parse_json_batch, sync_raw_json_fields,
};
use super::repository::{SourceMetadataUpdate, SourceRecord, SourceRepository};
use crate::concurrency::{AsyncLockMap, weak_lock};
use crate::model::{PluginFormat, SourceCallError, SourceCallOutput, SourceRuntimeState};

/// Adapter connecting a domain-specific plugin specification (`LegadoBookSource`, `ComicSourceSpec`,
/// `MusicSourceSpec`, `VideoSourceSpec`) to [`SourceCrudService`].
pub trait SourceSpecAdapter: DeserializeOwned + Send + Sync + 'static {
    /// Media domain to which this plugin specification belongs.
    const MEDIA_KIND: MediaKind;

    /// Human-readable label used in validation & not-found messages (e.g. `"书籍插件"`, `"音乐插件"`).
    const ENTITY_LABEL: &'static str;

    /// JSON property key storing the group name inside `raw_json` for rule-mode plugins.
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

    /// Parses either a JSON rule batch or a JS module plugin into `(Spec, raw_source, PluginFormat)` tuples.
    fn parse_plugin_batch(raw_source: &str) -> Result<Vec<(Self, String, PluginFormat)>, String> {
        Self::parse_batch(raw_source).map(|items| {
            items
                .into_iter()
                .map(|(spec, raw)| (spec, raw, PluginFormat::JsonRule))
                .collect()
        })
    }

    fn source_name(&self) -> &str;
    fn effective_source_url(&self) -> Cow<'_, str>;
    fn group_name(&self) -> Option<&str>;
    fn enabled(&self) -> bool;
    fn weight(&self) -> i64;
    fn source_type_code(&self) -> i64;
    fn summary_meta(&self) -> ParsedSourceSummaryMeta;
}

/// Generic per-user isolated plugin CRUD, import/export, weight reorder, runtime-state merge, and
/// login-action application service reusable across all media domains.
pub struct SourceCrudService<Spec: SourceSpecAdapter> {
    repo: Arc<dyn SourceRepository>,
    user_import_locks: AsyncLockMap<Uuid>,
    state_locks: AsyncLockMap<Uuid>,
    _marker: PhantomData<fn() -> Spec>,
}

impl<Spec: SourceSpecAdapter> SourceCrudService<Spec> {
    pub fn new(repo: Arc<dyn SourceRepository>) -> Self {
        Self {
            repo,
            user_import_locks: Arc::new(std::sync::Mutex::new(HashMap::new())),
            state_locks: Arc::new(std::sync::Mutex::new(HashMap::new())),
            _marker: PhantomData,
        }
    }

    pub fn state_lock_for(&self, id: Uuid) -> Arc<tokio::sync::Mutex<()>> {
        weak_lock(&self.state_locks, id)
    }

    fn user_import_lock(&self, user_id: Uuid) -> Arc<tokio::sync::Mutex<()>> {
        weak_lock(&self.user_import_locks, user_id)
    }

    /// Loads candidate plugins owned by `actor` for search/browse ordered by weight desc then name asc.
    pub async fn select_sources(
        &self,
        actor: &User,
        source_ids: Option<&[Uuid]>,
    ) -> DomainResult<Vec<SourceRecord>> {
        actor.ensure_active()?;
        let mut sources = self.repo.list_by_user(actor.id.0).await?;
        if let Some(filter_ids) = source_ids.filter(|v| !v.is_empty()) {
            sources.retain(|s| filter_ids.contains(&s.id));
        } else {
            sources.retain(|s| s.enabled);
        }
        if sources.is_empty() {
            return Err(DomainError::Validation(format!(
                "没有可用的已启用{}",
                Spec::ENTITY_LABEL
            )));
        }
        sources.sort_by(|a, b| b.weight.cmp(&a.weight).then_with(|| a.name.cmp(&b.name)));
        Ok(sources)
    }

    pub fn record_to_summary(rec: &SourceRecord) -> SourceSummaryDto {
        let meta = serde_json::from_str::<Spec>(&rec.raw_json)
            .ok()
            .map(|spec| spec.summary_meta())
            .unwrap_or_default();

        build_source_summary(
            SourceRecordSnapshot {
                id: rec.id,
                domain: Spec::MEDIA_KIND,
                format: rec.format,
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
        )
    }

    /// Internal lookup by plugin `id` for background tasks whose item ownership was already verified.
    pub async fn load_source(&self, id: Uuid) -> DomainResult<SourceRecord> {
        self.repo
            .find_by_id(id)
            .await?
            .ok_or_else(|| DomainError::NotFound(format!("未找到{} {id}", Spec::ENTITY_LABEL)))
    }

    /// Loads a plugin record strictly owned by `actor`, returning `NotFound` if missing or owned by another user.
    pub async fn load_owned_source(&self, actor: &User, id: Uuid) -> DomainResult<SourceRecord> {
        actor.ensure_active()?;
        let rec = self.load_source(id).await?;
        if rec.user_id != actor.id.0 {
            return Err(DomainError::NotFound(format!(
                "未找到{} {id}",
                Spec::ENTITY_LABEL
            )));
        }
        Ok(rec)
    }

    pub async fn has_owned_source(&self, actor: &User, id: Uuid) -> DomainResult<bool> {
        actor.ensure_active()?;
        Ok(self
            .repo
            .find_by_id(id)
            .await?
            .is_some_and(|rec| rec.user_id == actor.id.0))
    }

    pub async fn load_source_for_actor(
        &self,
        actor: &User,
        id: Uuid,
    ) -> DomainResult<SourceRecord> {
        self.load_owned_source(actor, id).await
    }

    pub async fn get_source(&self, actor: &User, id: Uuid) -> DomainResult<SourceSummaryDto> {
        let rec = self.load_owned_source(actor, id).await?;
        Ok(Self::record_to_summary(&rec))
    }

    pub async fn list_sources(&self, actor: &User) -> DomainResult<Vec<SourceSummaryDto>> {
        actor.ensure_active()?;
        let mut rows = self.repo.list_by_user(actor.id.0).await?;
        rows.sort_by(|a, b| b.weight.cmp(&a.weight).then_with(|| a.name.cmp(&b.name)));
        Ok(rows.iter().map(Self::record_to_summary).collect())
    }

    pub async fn get_source_variable(&self, actor: &User, id: Uuid) -> DomainResult<String> {
        let rec = self.load_owned_source(actor, id).await?;
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

    /// Creates or upserts a single plugin owned by `actor`.
    pub async fn create_or_upsert_source(
        &self,
        actor: &User,
        raw_source: &str,
    ) -> DomainResult<SourceSummaryDto> {
        actor.ensure_active()?;
        let parsed = Spec::parse_plugin_batch(raw_source).map_err(DomainError::Validation)?;
        if parsed.len() != 1 {
            return Err(DomainError::Validation(format!(
                "单条创建仅支持提交 1 条{}",
                Spec::ENTITY_LABEL
            )));
        }
        let mut report = self.import_parsed_batch(actor.id.0, parsed).await?;
        report
            .sources
            .pop()
            .ok_or_else(|| DomainError::Validation(format!("未解析到有效的{}", Spec::ENTITY_LABEL)))
    }

    pub async fn import_sources(
        &self,
        actor: &User,
        raw_source: &str,
    ) -> DomainResult<ImportSourcesReportDto> {
        actor.ensure_active()?;
        let parsed_batch = Spec::parse_plugin_batch(raw_source).map_err(DomainError::Validation)?;
        self.import_parsed_batch(actor.id.0, parsed_batch).await
    }

    async fn import_parsed_batch(
        &self,
        user_id: Uuid,
        parsed_batch: Vec<(Spec, String, PluginFormat)>,
    ) -> DomainResult<ImportSourcesReportDto> {
        let user_lock = self.user_import_lock(user_id);
        let _import_guard = user_lock.lock().await;
        let existing_all = self.repo.list_by_user(user_id).await?;
        let mut existing_by_key: HashMap<(String, String), SourceRecord> = existing_all
            .into_iter()
            .map(|m| ((m.source_url.clone(), m.name.clone()), m))
            .collect();

        let mut imported_count = 0usize;
        let mut updated_count = 0usize;
        let mut out_by_id: HashMap<Uuid, usize> = HashMap::new();
        let mut out: Vec<SourceSummaryDto> = Vec::with_capacity(parsed_batch.len());
        let now = Timestamp::now();

        for (src, item_raw, format) in parsed_batch {
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
                    format,
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
                    let summary = Self::record_to_summary(&rec);
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
            rt.ensure_android_id(&format!("{user_id}:{effective_url}"));
            let rec = SourceRecord {
                id: Uuid::now_v7(),
                user_id,
                domain: Spec::MEDIA_KIND,
                format,
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
            out.push(Self::record_to_summary(&rec));
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
        actor.ensure_active()?;
        let parsed_raw = if let Some(raw) = input.raw_json {
            let mut batch = Spec::parse_plugin_batch(&raw).map_err(DomainError::Validation)?;
            if batch.len() != 1 {
                return Err(DomainError::Validation(format!(
                    "编辑单条{}时仅支持提交 1 条{}",
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
        let mut rec = self.load_owned_source(actor, id).await?;
        let now = Timestamp::now();

        if let Some((parsed, clean_raw, format)) = parsed_raw {
            rec.format = format;
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
        if rec.format == PluginFormat::JsonRule
            && (input.enabled.is_some() || input.weight.is_some() || input.group_name.is_some())
        {
            rec.raw_json = sync_raw_json_fields(
                &rec.raw_json,
                input.enabled,
                input.weight,
                input.group_name.as_deref(),
                Spec::JSON_GROUP_KEY,
            );
        }

        let patch = SourceMetadataUpdate {
            format: rec.format,
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
        Ok(Self::record_to_summary(&updated))
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
        actor.ensure_active()?;
        let rows = self.repo.list_by_user(actor.id.0).await?;
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

        Ok(persisted.iter().map(Self::record_to_summary).collect())
    }

    pub async fn batch_sources(
        &self,
        actor: &User,
        input: BatchSourcesInput,
    ) -> DomainResult<BatchSourcesReportDto> {
        actor.ensure_active()?;
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
            let Some(rec) = self.repo.find_by_id(id).await? else {
                continue;
            };
            if rec.user_id != actor.id.0 {
                continue;
            }
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
                    self.repo.delete(id).await?;
                    affected_count += 1;
                }
            }
        }

        Ok(BatchSourcesReportDto { affected_count })
    }

    pub async fn export_source(
        &self,
        actor: &User,
        id: Uuid,
    ) -> DomainResult<(String, PluginFormat)> {
        let rec = self.load_owned_source(actor, id).await?;
        Ok((rec.raw_json, rec.format))
    }

    pub async fn export_source_json(&self, actor: &User, id: Uuid) -> DomainResult<String> {
        let (raw, _) = self.export_source(actor, id).await?;
        Ok(raw)
    }

    pub async fn export_sources_batch_json(
        &self,
        actor: &User,
        ids: Vec<Uuid>,
    ) -> DomainResult<String> {
        actor.ensure_active()?;
        if ids.is_empty() {
            return Err(DomainError::Validation(format!(
                "请至少选择一条{}",
                Spec::ENTITY_LABEL
            )));
        }
        let all = self.repo.list_by_user(actor.id.0).await?;
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
        actor.ensure_active()?;
        let lock = self.state_lock_for(id);
        let _guard = lock.lock().await;
        let _ = self.load_owned_source(actor, id).await?;
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
        let rec = self.load_owned_source(actor, id).await?;
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
        let refreshed = self.load_owned_source(actor, id).await?;

        Ok(LoginActionResponseDto {
            result: out.data,
            diagnostics: out.diagnostics,
            source: Self::record_to_summary(&refreshed),
        })
    }

    async fn directory_sources(
        &self,
        user_id: UserId,
    ) -> DomainResult<HashMap<Uuid, OnlineSourceInfo>> {
        let rows = self.repo.list_by_user(user_id.0).await?;
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

    async fn directory_source(
        &self,
        user_id: UserId,
        id: Uuid,
    ) -> DomainResult<Option<OnlineSourceInfo>> {
        Ok(self
            .repo
            .find_by_id(id)
            .await?
            .filter(|r| r.user_id == user_id.0)
            .map(|r| OnlineSourceInfo {
                name: r.name,
                enabled: r.enabled,
            }))
    }
}

#[async_trait]
impl<Spec: SourceSpecAdapter> OnlineSourceDirectory for SourceCrudService<Spec> {
    async fn list_sources(&self, user_id: UserId) -> DomainResult<HashMap<Uuid, OnlineSourceInfo>> {
        self.directory_sources(user_id).await
    }

    async fn find_source(
        &self,
        user_id: UserId,
        id: Uuid,
    ) -> DomainResult<Option<OnlineSourceInfo>> {
        self.directory_source(user_id, id).await
    }
}

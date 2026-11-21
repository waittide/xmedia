use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use jiff::Timestamp;
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::user::{User, require_admin};

use crate::domain::online::BookSourceRecord;
use crate::source::model::{LegadoBookSource, LoginUiItem, SourceRuntimeState};

use super::BookOnlineService;
use super::dto::{
    BatchBookSourceAction, BatchBookSourcesInput, BatchBookSourcesReportDto, BookSourceSummaryDto,
    ImportBookSourcesReportDto, LoginActionResponseDto, UpdateBookSourceInput,
};
use super::helpers::get_or_insert_weak_lock;

impl BookOnlineService {
    fn is_sensitive_login_field(key: &str, login_ui: &[LoginUiItem]) -> bool {
        if login_ui
            .iter()
            .any(|item| item.name == key && item.item_type.eq_ignore_ascii_case("password"))
        {
            return true;
        }
        let lower = key.to_ascii_lowercase();
        lower.contains("password")
            || lower.contains("passwd")
            || lower.contains("pwd")
            || lower.contains("token")
            || lower.contains("secret")
            || lower.contains("cookie")
            || lower.contains("key")
            || key.contains("密码")
            || key.contains("口令")
            || key.contains("密钥")
            || key.contains("授权码")
            || key.contains("凭证")
    }

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

        let (login_info, variable_preview, cookie_hosts) = if is_admin {
            let mut masked = HashMap::new();
            for (k, v) in &m.runtime_state.login_info {
                let is_sensitive = Self::is_sensitive_login_field(k, &login_ui);
                masked.insert(
                    k.clone(),
                    if is_sensitive && !v.is_empty() {
                        "••••••••".to_string()
                    } else {
                        v.clone()
                    },
                );
            }
            let preview: String = m.runtime_state.variable.chars().take(240).collect();
            let mut hosts: Vec<String> = m.runtime_state.cookies.keys().cloned().collect();
            hosts.sort();
            (masked, preview, hosts)
        } else {
            (HashMap::new(), String::new(), Vec::new())
        };

        BookSourceSummaryDto {
            id: m.id,
            name: m.name.clone(),
            source_url: m.source_url.clone(),
            group_name: m.group_name.clone(),
            comment,
            check_keyword,
            enabled: m.enabled,
            weight: m.weight,
            source_type: m.source_type,
            has_login_url,
            login_ui: if is_admin { login_ui } else { Vec::new() },
            login_info,
            has_variable: !m.runtime_state.variable.trim().is_empty(),
            variable_preview,
            cookie_hosts,
            last_test_ok: m.last_test_ok,
            last_test_msg: m.last_test_msg.clone(),
            updated_at: m.updated_at,
        }
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

    fn source_state_lock_for(&self, id: Uuid) -> Arc<tokio::sync::Mutex<()>> {
        get_or_insert_weak_lock(&self.source_state_locks, id)
    }

    pub(super) async fn persist_runtime_state(
        &self,
        id: Uuid,
        new_state: SourceRuntimeState,
    ) -> DomainResult<()> {
        let lock = self.source_state_lock_for(id);
        let _guard = lock.lock().await;
        let merged = if let Some(current_rec) = self.sources.find_by_id(id).await? {
            let mut merged = current_rec.runtime_state;
            merged.cookies.extend(new_state.cookies);
            merged.cache.extend(new_state.cache);
            merged.login_info.extend(new_state.login_info);
            if !new_state.variable.is_empty() {
                merged.variable = new_state.variable;
            }
            if !new_state.android_id.is_empty() {
                merged.android_id = new_state.android_id;
            }
            merged
        } else {
            new_state
        };
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
        let mut rows = self.sources.list_all().await?;
        rows.sort_by(|a, b| b.weight.cmp(&a.weight).then_with(|| a.name.cmp(&b.name)));

        let mut by_id: HashMap<Uuid, BookSourceRecord> =
            rows.into_iter().map(|r| (r.id, r)).collect();

        let mut final_order: Vec<BookSourceRecord> = Vec::with_capacity(by_id.len());
        for id in ordered_ids {
            if let Some(rec) = by_id.remove(&id) {
                final_order.push(rec);
            }
        }
        // Preserve relative order of any sources not included in `ordered_ids`
        if !by_id.is_empty() {
            let mut rest: Vec<BookSourceRecord> = by_id.into_values().collect();
            rest.sort_by(|a, b| b.weight.cmp(&a.weight).then_with(|| a.name.cmp(&b.name)));
            final_order.extend(rest);
        }

        let total = final_order.len() as i64;
        let now = Timestamp::now();
        let mut persisted = Vec::with_capacity(final_order.len());
        for (idx, rec) in final_order.into_iter().enumerate() {
            let target_weight = (total - idx as i64) * 10;
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
    ) -> DomainResult<ImportBookSourcesReportDto> {
        require_admin(actor)?;
        let parsed_batch =
            LegadoBookSource::parse_json_batch(raw_json).map_err(DomainError::Validation)?;

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

            if let Some(mut rec) = existing_by_key.remove(&key) {
                rec.group_name = group_name;
                rec.enabled = src.enabled;
                rec.weight = src.weight;
                rec.source_type = src.book_source_type;
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
                // Re-insert so duplicate keys within the same batch update this record
                existing_by_key.insert(key, rec);
            } else {
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
        let mut rec = self.load_source(id).await?;
        let now = Timestamp::now();

        if let Some(raw) = input.raw_json {
            let mut batch =
                LegadoBookSource::parse_json_batch(&raw).map_err(DomainError::Validation)?;
            if batch.len() != 1 {
                return Err(DomainError::Validation(
                    "编辑单条书源时仅支持提交 1 条书源 JSON".into(),
                ));
            }
            let (parsed, clean_raw) = batch.remove(0);
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
        if let Some(v) = input.variable {
            rec.runtime_state.variable = v;
        }
        rec.updated_at = now;

        if !self.sources.update_existing(&rec).await? {
            return Err(DomainError::NotFound(format!("未找到书源 {id}")));
        }
        Ok(Self::record_to_summary(&rec, true))
    }

    pub async fn delete_source(&self, actor: &User, id: Uuid) -> DomainResult<()> {
        require_admin(actor)?;
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
        let mut by_id: HashMap<Uuid, BookSourceRecord> =
            all.into_iter().map(|r| (r.id, r)).collect();
        let mut arr: Vec<serde_json::Value> = Vec::new();
        for id in ids {
            if let Some(rec) = by_id.remove(&id)
                && let Ok(val) = serde_json::from_str::<serde_json::Value>(&rec.raw_json)
            {
                arr.push(val);
            }
        }
        if arr.is_empty() {
            return Err(DomainError::NotFound("未找到可导出的书源".into()));
        }
        serde_json::to_string_pretty(&arr)
            .map_err(|e| DomainError::Storage(format!("序列化书源导出 JSON 失败: {e}")))
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

        let mut seen = HashSet::with_capacity(input.ids.len());
        let unique_ids: Vec<Uuid> = input
            .ids
            .into_iter()
            .filter(|id| seen.insert(*id))
            .collect();

        let now = Timestamp::now();
        let mut affected_count = 0usize;

        for id in unique_ids {
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

        // Merge submitted fields onto persisted login_info so omitted keys are never wiped,
        // and preserve real secrets when the frontend echoes back the masked placeholder.
        let mut merged_login = rec.runtime_state.login_info.clone();
        for (k, v) in login_info {
            if v == "••••••••" && merged_login.contains_key(&k) {
                continue;
            }
            merged_login.insert(k, v);
        }

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

use std::collections::HashMap;
use std::sync::Arc;

use jiff::Timestamp;
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::user::{User, require_admin};
use xmedia_source::crud::{
    ParsedSourceSummaryMeta, SourceRecordSnapshot, build_source_summary, compute_reorder_targets,
    dedup_preserve_order_uuids, format_export_batch_json, merge_login_info_preserving_masked,
};

use crate::domain::online::{VideoSourceMetadataUpdate, VideoSourceRecord};
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

            if let Some(existing_rec) = existing_by_key.remove(&key) {
                let id = existing_rec.id;
                let lock = self.source_state_lock_for(id);
                let _guard = lock.lock().await;
                let patch = VideoSourceMetadataUpdate {
                    name: existing_rec.name.clone(),
                    source_url: existing_rec.source_url.clone(),
                    group_name: group_name.clone(),
                    enabled: src.enabled,
                    weight: src.weight,
                    source_type,
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
                    existing_by_key.insert(key, rec);
                    continue;
                }
            }

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
        let parsed_raw = if let Some(raw) = input.raw_json {
            let mut batch =
                VideoSourceSpec::parse_json_batch(&raw).map_err(DomainError::Validation)?;
            if batch.len() != 1 {
                return Err(DomainError::Validation(
                    "编辑单条影视源时仅支持提交 1 条影视源 JSON".into(),
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

        let patch = VideoSourceMetadataUpdate {
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
            .ok_or_else(|| DomainError::NotFound(format!("未找到影视源 {id}")))?;
        Ok(Self::record_to_summary(&updated, true))
    }

    pub async fn delete_source(&self, actor: &User, id: Uuid) -> DomainResult<()> {
        require_admin(actor)?;
        let lock = self.source_state_lock_for(id);
        let _guard = lock.lock().await;
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
            let lock = self.source_state_lock_for(id);
            let _guard = lock.lock().await;
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

fn merge_map_with_base(
    base: &HashMap<String, String>,
    merged: &mut HashMap<String, String>,
    updated: HashMap<String, String>,
) {
    for key in base.keys() {
        if !updated.contains_key(key) && merged.get(key) == base.get(key) {
            merged.remove(key);
        }
    }
    for (key, value) in updated {
        if base.get(&key) != Some(&value) {
            merged.insert(key, value);
        }
    }
}

pub(super) fn merge_runtime_state_with_base(
    base: &SourceRuntimeState,
    current: Option<SourceRuntimeState>,
    updated: SourceRuntimeState,
    allow_login_info_override: bool,
) -> SourceRuntimeState {
    let Some(mut merged) = current else {
        return updated;
    };

    merge_map_with_base(&base.cookies, &mut merged.cookies, updated.cookies);
    merge_map_with_base(&base.cache, &mut merged.cache, updated.cache);

    if updated.variable != base.variable {
        merged.variable = updated.variable;
    }
    if updated.android_id != base.android_id && !updated.android_id.is_empty() {
        merged.android_id = updated.android_id;
    }
    if allow_login_info_override {
        merge_map_with_base(&base.login_info, &mut merged.login_info, updated.login_info);
    }

    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_state(
        variable: &str,
        login_info: &[(&str, &str)],
        cookies: &[(&str, &str)],
        cache: &[(&str, &str)],
        android_id: &str,
    ) -> SourceRuntimeState {
        SourceRuntimeState {
            variable: variable.to_string(),
            login_info: login_info
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect(),
            cookies: cookies
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect(),
            cache: cache
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect(),
            android_id: android_id.to_string(),
        }
    }

    #[test]
    fn merge_runtime_state_with_base_when_current_none_returns_updated() {
        let base = sample_state("v1", &[("u", "a")], &[], &[], "d1");
        let updated = sample_state("v2", &[("u", "b")], &[("c", "1")], &[], "d2");
        let merged = merge_runtime_state_with_base(&base, None, updated.clone(), false);
        assert_eq!(merged, updated);
    }

    #[test]
    fn merge_runtime_state_with_base_preserves_concurrent_login_and_variable_when_unchanged() {
        let base = sample_state(
            "var_1",
            &[("username", "alice_1"), ("apiToken", "secret_1")],
            &[("sid", "s1")],
            &[("k1", "v1")],
            "dev1",
        );
        let current = sample_state(
            "var_2",
            &[("username", "bob_2"), ("apiToken", "secret_2")],
            &[("sid", "s2"), ("other", "o2")],
            &[("k1", "v2")],
            "dev2",
        );
        // Even if updated carries stale var_1 and stale/mutated login_info, non-login ops
        // with unchanged variable & allow_login_info_override=false must preserve current.
        let mut updated = base.clone();
        updated
            .login_info
            .insert("username".into(), "mutated_by_non_login".into());

        let merged = merge_runtime_state_with_base(&base, Some(current.clone()), updated, false);
        assert_eq!(merged.variable, "var_2");
        assert_eq!(merged.login_info, current.login_info);
        assert_eq!(merged.cookies, current.cookies);
        assert_eq!(merged.cache, current.cache);
        assert_eq!(merged.android_id, "dev2");
    }

    #[test]
    fn merge_runtime_state_with_base_applies_only_changed_variable_and_map_entries() {
        let base = sample_state(
            "var_1",
            &[("username", "alice_1")],
            &[
                ("keep", "1"),
                ("remove_ok", "r1"),
                ("remove_conflict", "c1"),
            ],
            &[("k1", "v1")],
            "dev1",
        );
        let current = sample_state(
            "var_2",
            &[("username", "bob_2")],
            &[
                ("keep", "1_concurrent"),
                ("remove_ok", "r1"),
                ("remove_conflict", "c2_modified"),
                ("added_concurrent", "ac"),
            ],
            &[("k1", "v1_concurrent")],
            "dev1",
        );
        let updated = sample_state(
            "var_3",
            &[("username", "stale_alice")],
            &[("keep", "1"), ("added_by_op", "ao")],
            &[("k1", "v1"), ("k2", "v2")],
            "dev_new",
        );

        let merged = merge_runtime_state_with_base(&base, Some(current), updated, false);
        assert_eq!(merged.variable, "var_3");
        assert_eq!(merged.android_id, "dev_new");
        assert_eq!(
            merged.login_info.get("username").map(String::as_str),
            Some("bob_2")
        );
        assert_eq!(
            merged.cookies.get("keep").map(String::as_str),
            Some("1_concurrent")
        );
        assert!(!merged.cookies.contains_key("remove_ok"));
        assert_eq!(
            merged.cookies.get("remove_conflict").map(String::as_str),
            Some("c2_modified")
        );
        assert_eq!(
            merged.cookies.get("added_concurrent").map(String::as_str),
            Some("ac")
        );
        assert_eq!(
            merged.cookies.get("added_by_op").map(String::as_str),
            Some("ao")
        );
        assert_eq!(
            merged.cache.get("k1").map(String::as_str),
            Some("v1_concurrent")
        );
        assert_eq!(merged.cache.get("k2").map(String::as_str), Some("v2"));
    }

    #[test]
    fn merge_runtime_state_with_base_allows_login_info_override_only_for_changed_keys() {
        let base = sample_state(
            "var_1",
            &[
                ("username", "alice_1"),
                ("apiToken", "secret_1"),
                ("drop", "d1"),
            ],
            &[],
            &[],
            "dev1",
        );
        let current = sample_state(
            "var_2",
            &[
                ("username", "bob_2"),
                ("apiToken", "secret_2"),
                ("drop", "d1"),
                ("extra", "e2"),
            ],
            &[],
            &[],
            "dev1",
        );
        let updated = sample_state(
            "var_1",
            &[("username", "charlie_3"), ("apiToken", "secret_1")],
            &[],
            &[],
            "dev1",
        );

        let merged = merge_runtime_state_with_base(&base, Some(current), updated, true);
        assert_eq!(merged.variable, "var_2");
        assert_eq!(
            merged.login_info.get("username").map(String::as_str),
            Some("charlie_3")
        );
        assert_eq!(
            merged.login_info.get("apiToken").map(String::as_str),
            Some("secret_2")
        );
        assert!(!merged.login_info.contains_key("drop"));
        assert_eq!(
            merged.login_info.get("extra").map(String::as_str),
            Some("e2")
        );
    }
}

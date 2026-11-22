use std::collections::{HashMap, HashSet};

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::model::{ExecutionDiagnostics, LoginUiItem, SourceRuntimeState};

pub const MASKED_SECRET_PLACEHOLDER: &str = "••••••••";
pub const VARIABLE_PREVIEW_MAX_CHARS: usize = 240;

// ---------------------------------------------------------------------------
// 1. Shared Wire DTOs (identical JSON schema to xmedia-book)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceSummaryDto {
    pub id: Uuid,
    pub name: String,
    pub source_url: String,
    pub group_name: String,
    pub comment: Option<String>,
    pub check_keyword: Option<String>,
    pub enabled: bool,
    pub weight: i64,
    pub source_type: i64,
    pub has_login_url: bool,
    pub login_ui: Vec<LoginUiItem>,
    pub login_info: HashMap<String, String>,
    pub has_variable: bool,
    pub variable_preview: String,
    pub cookie_hosts: Vec<String>,
    pub last_test_ok: Option<bool>,
    pub last_test_msg: Option<String>,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportSourcesReportDto<S = SourceSummaryDto> {
    pub imported_count: usize,
    pub updated_count: usize,
    pub sources: Vec<S>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct UpdateSourceInput {
    pub enabled: Option<bool>,
    pub weight: Option<i64>,
    pub group_name: Option<String>,
    pub raw_json: Option<String>,
    pub variable: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum BatchSourceAction {
    Enable,
    Disable,
    SetGroup { group_name: String },
    Delete,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BatchSourcesInput {
    pub ids: Vec<Uuid>,
    #[serde(flatten)]
    pub action: BatchSourceAction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchSourcesReportDto {
    pub affected_count: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ImportSourcesRequest {
    pub raw_json: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReorderSourcesRequest {
    pub ordered_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExportSourcesBatchRequest {
    pub ids: Vec<Uuid>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LoginActionRequest {
    #[serde(default, alias = "form_data")]
    pub login_info: HashMap<String, String>,
    pub action: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginActionResponseDto<S = SourceSummaryDto> {
    pub result: String,
    pub diagnostics: ExecutionDiagnostics,
    pub source: S,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceVariableResponse {
    pub variable: String,
}

// ---------------------------------------------------------------------------
// 2. Shared Test / Diagnostics DTOs & Helpers
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceTestStepDto {
    pub stage: String,
    pub label: String,
    pub ok: bool,
    pub elapsed_ms: u64,
    pub summary: String,
    pub error: Option<String>,
    pub diagnostics: ExecutionDiagnostics,
}

/// Generic test report usable by comic/music/video domains with flattened domain payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceTestReportDto<M, P = serde_json::Value> {
    pub ok: bool,
    pub mode: M,
    pub keyword: String,
    pub total_elapsed_ms: u64,
    pub result_count: usize,
    pub sample_titles: Vec<String>,
    pub error: Option<String>,
    pub diagnostics: ExecutionDiagnostics,
    pub steps: Vec<SourceTestStepDto>,
    #[serde(flatten)]
    pub payload: P,
}

pub fn merge_diagnostics(acc: &mut ExecutionDiagnostics, d: &ExecutionDiagnostics) {
    acc.logs.extend(d.logs.iter().cloned());
    acc.toasts.extend(d.toasts.iter().cloned());
    acc.browser_urls.extend(d.browser_urls.iter().cloned());
}

pub fn summarize_steps_outcome(steps: &[SourceTestStepDto]) -> (bool, Option<String>) {
    let ok = !steps.is_empty() && steps.iter().all(|s| s.ok);
    let first_error = steps.iter().find_map(|s| s.error.clone());
    (ok, first_error)
}

pub fn format_timeout_step(elapsed_ms: u64, message: impl Into<String>) -> SourceTestStepDto {
    let msg = message.into();
    SourceTestStepDto {
        stage: "timeout".into(),
        label: "总超时控制".into(),
        ok: false,
        elapsed_ms,
        summary: msg.clone(),
        error: Some(msg),
        diagnostics: ExecutionDiagnostics::default(),
    }
}

pub fn format_test_failure_summary(steps: &[SourceTestStepDto]) -> String {
    if let Some(failed) = steps.iter().find(|s| !s.ok) {
        let reason: String = failed
            .error
            .as_deref()
            .unwrap_or(&failed.summary)
            .chars()
            .take(120)
            .collect();
        format!("{} 失败：{reason}", failed.label)
    } else {
        "测试失败".into()
    }
}

// ---------------------------------------------------------------------------
// 3. Batch JSON Parsing & Raw JSON Field Synchronization
// ---------------------------------------------------------------------------

/// Parses a single JSON object or JSON array (stripping whitespace + UTF-8 BOM `\u{feff}`),
/// preserving each item's serialized raw JSON string and skipping entries where `is_blank(&item)`
/// returns true.
pub fn parse_json_batch<T, F>(
    raw: &str,
    entity_label: &str,
    empty_hint: &str,
    is_blank: F,
) -> Result<Vec<(T, String)>, String>
where
    T: serde::de::DeserializeOwned,
    F: Fn(&T) -> bool,
{
    let trimmed = raw.trim().trim_start_matches('\u{feff}').trim();
    if trimmed.is_empty() {
        return Err(format!("{entity_label} JSON 内容为空"));
    }
    let val: serde_json::Value =
        serde_json::from_str(trimmed).map_err(|e| format!("{entity_label} JSON 解析失败: {e}"))?;
    let items = match val {
        serde_json::Value::Array(arr) => arr,
        obj @ serde_json::Value::Object(_) => vec![obj],
        _ => return Err(format!("{entity_label} JSON 必须是对象或对象数组")),
    };

    let mut out = Vec::with_capacity(items.len());
    for (idx, item) in items.into_iter().enumerate() {
        let item_raw = serde_json::to_string(&item)
            .map_err(|e| format!("第 {} 条{entity_label}序列化失败: {e}", idx + 1))?;
        let parsed: T = serde_json::from_value(item)
            .map_err(|e| format!("第 {} 条{entity_label}结构解析失败: {e}", idx + 1))?;
        if is_blank(&parsed) {
            continue;
        }
        out.push((parsed, item_raw));
    }
    if out.is_empty() {
        return Err(empty_hint.to_string());
    }
    Ok(out)
}

/// Synchronizes mutable metadata (`enabled`, `weight`, `group_name`) into `raw_json`
/// using `group_key` (e.g. `"bookSourceGroup"` for Legado book/comic or `"sourceGroup"`).
pub fn sync_raw_json_fields(
    raw_json: &str,
    enabled: Option<bool>,
    weight: Option<i64>,
    group_name: Option<&str>,
    group_key: &str,
) -> String {
    let Ok(mut val) = serde_json::from_str::<serde_json::Value>(raw_json) else {
        return raw_json.to_string();
    };
    let Some(obj) = val.as_object_mut() else {
        return raw_json.to_string();
    };
    if let Some(en) = enabled {
        obj.insert("enabled".into(), serde_json::Value::Bool(en));
    }
    if let Some(w) = weight {
        obj.insert("weight".into(), serde_json::Value::from(w));
    }
    if let Some(g) = group_name {
        let trimmed = g.trim();
        if trimmed.is_empty() {
            obj.remove(group_key);
        } else {
            obj.insert(
                group_key.to_string(),
                serde_json::Value::String(trimmed.to_string()),
            );
        }
    }
    serde_json::to_string(&val).unwrap_or_else(|_| raw_json.to_string())
}

// ---------------------------------------------------------------------------
// 4. Non-Admin Credential Masking & Summary Construction
// ---------------------------------------------------------------------------

pub struct SourceRecordSnapshot<'a> {
    pub id: Uuid,
    pub name: &'a str,
    pub source_url: &'a str,
    pub group_name: &'a str,
    pub enabled: bool,
    pub weight: i64,
    pub source_type: i64,
    pub runtime_state: &'a SourceRuntimeState,
    pub last_test_ok: Option<bool>,
    pub last_test_msg: Option<&'a str>,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, Default)]
pub struct ParsedSourceSummaryMeta {
    pub comment: Option<String>,
    pub check_keyword: Option<String>,
    pub has_login_url: bool,
    pub login_ui: Vec<LoginUiItem>,
}

pub fn is_sensitive_login_field(key: &str, login_ui: &[LoginUiItem]) -> bool {
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

pub fn mask_login_info(
    login_info: &HashMap<String, String>,
    login_ui: &[LoginUiItem],
) -> HashMap<String, String> {
    let mut masked = HashMap::with_capacity(login_info.len());
    for (k, v) in login_info {
        let is_sensitive = is_sensitive_login_field(k, login_ui);
        masked.insert(
            k.clone(),
            if is_sensitive && !v.is_empty() {
                MASKED_SECRET_PLACEHOLDER.to_string()
            } else {
                v.clone()
            },
        );
    }
    masked
}

pub fn merge_login_info_preserving_masked(
    existing: &HashMap<String, String>,
    submitted: HashMap<String, String>,
) -> HashMap<String, String> {
    let mut merged = existing.clone();
    for (k, v) in submitted {
        if v == MASKED_SECRET_PLACEHOLDER && merged.contains_key(&k) {
            continue;
        }
        merged.insert(k, v);
    }
    merged
}

pub fn build_source_summary(
    rec: SourceRecordSnapshot<'_>,
    meta: ParsedSourceSummaryMeta,
    is_admin: bool,
) -> SourceSummaryDto {
    let (login_ui, login_info, variable_preview, cookie_hosts) = if is_admin {
        let masked = mask_login_info(&rec.runtime_state.login_info, &meta.login_ui);
        let preview: String = rec
            .runtime_state
            .variable
            .chars()
            .take(VARIABLE_PREVIEW_MAX_CHARS)
            .collect();
        let mut hosts: Vec<String> = rec.runtime_state.cookies.keys().cloned().collect();
        hosts.sort();
        (meta.login_ui, masked, preview, hosts)
    } else {
        (Vec::new(), HashMap::new(), String::new(), Vec::new())
    };

    SourceSummaryDto {
        id: rec.id,
        name: rec.name.to_string(),
        source_url: rec.source_url.to_string(),
        group_name: rec.group_name.to_string(),
        comment: meta.comment,
        check_keyword: meta.check_keyword,
        enabled: rec.enabled,
        weight: rec.weight,
        source_type: rec.source_type,
        has_login_url: meta.has_login_url,
        login_ui,
        login_info,
        has_variable: !rec.runtime_state.variable.trim().is_empty(),
        variable_preview,
        cookie_hosts,
        last_test_ok: rec.last_test_ok,
        last_test_msg: rec.last_test_msg.map(ToString::to_string),
        updated_at: rec.updated_at,
    }
}

// ---------------------------------------------------------------------------
// 5. Reorder, Dedup, Export & Runtime State Merge Helpers
// ---------------------------------------------------------------------------

pub fn dedup_preserve_order_uuids(ids: Vec<Uuid>) -> Vec<Uuid> {
    let mut seen = HashSet::with_capacity(ids.len());
    ids.into_iter().filter(|id| seen.insert(*id)).collect()
}

/// Computes descending `weight = (total - idx) * 10` for `ordered_ids` followed by
/// remaining items sorted by `(weight DESC, name ASC)`. Returns `(item, target_weight)` pairs.
pub fn compute_reorder_targets<T>(
    mut rows: Vec<T>,
    ordered_ids: &[Uuid],
    id_fn: impl Fn(&T) -> Uuid,
    weight_fn: impl Fn(&T) -> i64,
    name_fn: impl Fn(&T) -> &str,
) -> Vec<(T, i64)> {
    rows.sort_by(|a, b| {
        weight_fn(b)
            .cmp(&weight_fn(a))
            .then_with(|| name_fn(a).cmp(name_fn(b)))
    });

    let mut by_id: HashMap<Uuid, T> = rows.into_iter().map(|r| (id_fn(&r), r)).collect();

    let mut final_order: Vec<T> = Vec::with_capacity(by_id.len());
    for id in ordered_ids {
        if let Some(rec) = by_id.remove(id) {
            final_order.push(rec);
        }
    }
    if !by_id.is_empty() {
        let mut rest: Vec<T> = by_id.into_values().collect();
        rest.sort_by(|a, b| {
            weight_fn(b)
                .cmp(&weight_fn(a))
                .then_with(|| name_fn(a).cmp(name_fn(b)))
        });
        final_order.extend(rest);
    }

    let total = final_order.len() as i64;
    final_order
        .into_iter()
        .enumerate()
        .map(|(idx, rec)| (rec, (total - idx as i64) * 10))
        .collect()
}

pub fn merge_runtime_state(
    existing: Option<SourceRuntimeState>,
    new_state: SourceRuntimeState,
) -> SourceRuntimeState {
    let Some(mut merged) = existing else {
        return new_state;
    };
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
}

fn parse_cookie_pairs(raw: &str) -> Vec<(String, String)> {
    let mut pairs: Vec<(String, String)> = Vec::new();
    for part in raw.split(';') {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some((k, v)) = trimmed.split_once('=') {
            let key = k.trim().to_string();
            let val = v.trim().to_string();
            if key.is_empty() {
                continue;
            }
            if let Some(slot) = pairs.iter_mut().find(|(ek, _)| ek == &key) {
                slot.1 = val;
            } else {
                pairs.push((key, val));
            }
        }
    }
    pairs
}

fn serialize_cookie_pairs(pairs: Vec<(String, String)>) -> String {
    pairs
        .into_iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("; ")
}

fn merge_cookie_string_with_base(
    base_str: Option<&str>,
    cur_str: Option<&str>,
    upd_str: Option<&str>,
) -> Option<String> {
    let has_pairs = [base_str, cur_str, upd_str]
        .into_iter()
        .flatten()
        .any(|s| !parse_cookie_pairs(s).is_empty());
    if !has_pairs {
        return match upd_str {
            None => {
                if cur_str == base_str {
                    None
                } else {
                    cur_str.map(ToString::to_string)
                }
            }
            Some(u) => {
                if base_str == Some(u) {
                    cur_str.map(ToString::to_string)
                } else {
                    Some(u.to_string())
                }
            }
        };
    }

    let base_pairs = base_str.map(parse_cookie_pairs).unwrap_or_default();
    let mut cur_pairs = cur_str.map(parse_cookie_pairs).unwrap_or_default();
    let upd_pairs = upd_str.map(parse_cookie_pairs).unwrap_or_default();

    let base_map: HashMap<&str, &str> = base_pairs
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let upd_map: HashMap<&str, &str> = upd_pairs
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();

    cur_pairs.retain(|(name, cur_val)| {
        if upd_map.contains_key(name.as_str()) {
            true
        } else {
            base_map.get(name.as_str()) != Some(&cur_val.as_str())
        }
    });

    for (name, upd_val) in upd_pairs {
        if base_map.get(name.as_str()) != Some(&upd_val.as_str()) {
            if let Some(slot) = cur_pairs.iter_mut().find(|(cn, _)| cn == &name) {
                slot.1 = upd_val;
            } else {
                cur_pairs.push((name, upd_val));
            }
        }
    }

    if cur_pairs.is_empty() {
        if let Some("") = upd_str
            && base_str != Some("")
        {
            Some(String::new())
        } else {
            None
        }
    } else {
        Some(serialize_cookie_pairs(cur_pairs))
    }
}

fn merge_cookies_with_base(
    base: &HashMap<String, String>,
    merged: &mut HashMap<String, String>,
    updated: HashMap<String, String>,
) {
    let mut hosts: HashSet<String> = base.keys().cloned().collect();
    hosts.extend(merged.keys().cloned());
    hosts.extend(updated.keys().cloned());

    for host in hosts {
        let b = base.get(&host).map(String::as_str);
        let c = merged.get(&host).map(String::as_str);
        let u = updated.get(&host).map(String::as_str);
        match merge_cookie_string_with_base(b, c, u) {
            Some(val) => {
                merged.insert(host, val);
            }
            None => {
                merged.remove(&host);
            }
        }
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

pub fn merge_runtime_state_with_base(
    base: &SourceRuntimeState,
    current: Option<SourceRuntimeState>,
    updated: SourceRuntimeState,
    allow_login_info_override: bool,
) -> SourceRuntimeState {
    let Some(mut merged) = current else {
        return updated;
    };

    merge_cookies_with_base(&base.cookies, &mut merged.cookies, updated.cookies);
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

pub fn format_export_batch_json(
    ids: &[Uuid],
    raw_by_id: &mut HashMap<Uuid, &str>,
) -> Result<String, String> {
    let mut arr: Vec<serde_json::Value> = Vec::new();
    for id in ids {
        if let Some(raw) = raw_by_id.remove(id)
            && let Ok(val) = serde_json::from_str::<serde_json::Value>(raw)
        {
            arr.push(val);
        }
    }
    if arr.is_empty() {
        return Err("未找到可导出的数据源".into());
    }
    serde_json::to_string_pretty(&arr).map_err(|e| format!("序列化数据源导出 JSON 失败: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Deserialize)]
    struct SampleSpec {
        #[serde(default)]
        name: String,
        #[serde(default)]
        url: String,
    }

    #[test]
    fn parse_json_batch_handles_bom_single_array_and_errors() {
        let bom_single = "\u{feff}  {\"name\":\"A\",\"url\":\"https://a.test\"}  ";
        let res = parse_json_batch::<SampleSpec, _>(bom_single, "数据源", "为空", |s| {
            s.name.trim().is_empty() && s.url.trim().is_empty()
        })
        .unwrap();
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].0.name, "A");

        let array_with_blank = r#"[
            {"name":"","url":""},
            {"name":"B","url":"https://b.test"}
        ]"#;
        let res_arr =
            parse_json_batch::<SampleSpec, _>(array_with_blank, "数据源", "为空", |s| {
                s.name.trim().is_empty() && s.url.trim().is_empty()
            })
            .unwrap();
        assert_eq!(res_arr.len(), 1);
        assert_eq!(res_arr[0].0.name, "B");

        assert!(parse_json_batch::<SampleSpec, _>("  ", "数据源", "为空", |_| false).is_err());
        let bom_ws_err =
            parse_json_batch::<SampleSpec, _>("\u{feff}   ", "数据源", "为空", |_| false)
                .unwrap_err();
        assert_eq!(bom_ws_err, "数据源 JSON 内容为空");
        assert!(parse_json_batch::<SampleSpec, _>("123", "数据源", "为空", |_| false).is_err());
        assert!(parse_json_batch::<SampleSpec, _>("{bad", "数据源", "为空", |_| false).is_err());
        assert!(
            parse_json_batch::<SampleSpec, _>(
                r#"[{"name":"","url":""}]"#,
                "数据源",
                "未解析到有效条目",
                |s| s.name.trim().is_empty() && s.url.trim().is_empty()
            )
            .unwrap_err()
                == "未解析到有效条目"
        );
    }

    #[test]
    fn sync_raw_json_fields_updates_and_clears_group_key() {
        let raw = r#"{"name":"X","enabled":true,"weight":1,"extra":42}"#;
        let updated =
            sync_raw_json_fields(raw, Some(false), Some(99), Some("  漫画 "), "sourceGroup");
        let val: serde_json::Value = serde_json::from_str(&updated).unwrap();
        assert_eq!(val["enabled"], false);
        assert_eq!(val["weight"], 99);
        assert_eq!(val["sourceGroup"], "漫画");
        assert_eq!(val["extra"], 42);

        let cleared = sync_raw_json_fields(&updated, None, None, Some("   "), "sourceGroup");
        let val2: serde_json::Value = serde_json::from_str(&cleared).unwrap();
        assert!(val2.get("sourceGroup").is_none());
        assert_eq!(val2["weight"], 99);

        assert_eq!(
            sync_raw_json_fields("not-json", Some(false), None, None, "sourceGroup"),
            "not-json"
        );
    }

    #[test]
    fn masking_and_build_source_summary_for_admin_and_member() {
        let login_ui = vec![
            LoginUiItem {
                name: "用户名".into(),
                item_type: "text".into(),
                action: None,
                style: None,
            },
            LoginUiItem {
                name: "pinCode".into(),
                item_type: "PASSWORD".into(),
                action: None,
                style: None,
            },
        ];

        assert!(is_sensitive_login_field("pinCode", &login_ui));
        assert!(is_sensitive_login_field("apiToken", &login_ui));
        assert!(is_sensitive_login_field("登录密码", &login_ui));
        assert!(!is_sensitive_login_field("用户名", &login_ui));

        let mut rt = SourceRuntimeState {
            variable: "你".repeat(260),
            ..Default::default()
        };
        rt.login_info.insert("用户名".into(), "alice".into());
        rt.login_info.insert("pinCode".into(), "1234".into());
        rt.login_info.insert("emptyKey".into(), String::new());
        rt.cookies.insert("b.example.com".into(), "x=1".into());
        rt.cookies.insert("a.example.com".into(), "y=2".into());

        let snap = SourceRecordSnapshot {
            id: Uuid::nil(),
            name: "Test",
            source_url: "https://example.com",
            group_name: "G1",
            enabled: true,
            weight: 10,
            source_type: 2,
            runtime_state: &rt,
            last_test_ok: Some(true),
            last_test_msg: Some("ok"),
            updated_at: Timestamp::UNIX_EPOCH,
        };
        let meta = ParsedSourceSummaryMeta {
            comment: Some("c".into()),
            check_keyword: Some("kw".into()),
            has_login_url: true,
            login_ui: login_ui.clone(),
        };

        let admin_dto = build_source_summary(
            SourceRecordSnapshot {
                id: snap.id,
                name: snap.name,
                source_url: snap.source_url,
                group_name: snap.group_name,
                enabled: snap.enabled,
                weight: snap.weight,
                source_type: snap.source_type,
                runtime_state: snap.runtime_state,
                last_test_ok: snap.last_test_ok,
                last_test_msg: snap.last_test_msg,
                updated_at: snap.updated_at,
            },
            meta.clone(),
            true,
        );
        assert_eq!(
            admin_dto.login_info.get("用户名").map(String::as_str),
            Some("alice")
        );
        assert_eq!(
            admin_dto.login_info.get("pinCode").map(String::as_str),
            Some(MASKED_SECRET_PLACEHOLDER)
        );
        assert_eq!(
            admin_dto.login_info.get("emptyKey").map(String::as_str),
            Some("")
        );
        assert_eq!(admin_dto.variable_preview.chars().count(), 240);
        assert_eq!(
            admin_dto.cookie_hosts,
            vec!["a.example.com", "b.example.com"]
        );
        assert_eq!(admin_dto.login_ui.len(), 2);

        let member_dto = build_source_summary(snap, meta, false);
        assert!(member_dto.has_login_url);
        assert!(member_dto.has_variable);
        assert!(member_dto.login_ui.is_empty());
        assert!(member_dto.login_info.is_empty());
        assert!(member_dto.variable_preview.is_empty());
        assert!(member_dto.cookie_hosts.is_empty());

        let merged = merge_login_info_preserving_masked(
            &rt.login_info,
            HashMap::from([
                ("pinCode".into(), MASKED_SECRET_PLACEHOLDER.into()),
                ("用户名".into(), "bob".into()),
            ]),
        );
        assert_eq!(merged.get("pinCode").map(String::as_str), Some("1234"));
        assert_eq!(merged.get("用户名").map(String::as_str), Some("bob"));
    }

    #[test]
    fn test_report_diagnostics_reorder_dedup_and_export_helpers() {
        let mut acc = ExecutionDiagnostics::default();
        let d1 = ExecutionDiagnostics {
            toasts: vec!["t1".into()],
            logs: vec!["l1".into()],
            browser_urls: vec![],
        };
        merge_diagnostics(&mut acc, &d1);
        assert_eq!(acc.toasts, vec!["t1"]);
        assert_eq!(acc.logs, vec!["l1"]);

        assert_eq!(summarize_steps_outcome(&[]), (false, None));
        let long_err = "E".repeat(150);
        let steps = vec![SourceTestStepDto {
            stage: "search".into(),
            label: "搜索".into(),
            ok: false,
            elapsed_ms: 10,
            summary: "fail".into(),
            error: Some(long_err),
            diagnostics: ExecutionDiagnostics::default(),
        }];
        let (ok, first_err) = summarize_steps_outcome(&steps);
        assert!(!ok);
        assert!(first_err.is_some());
        let summary_msg = format_test_failure_summary(&steps);
        assert!(summary_msg.starts_with("搜索 失败："));
        assert_eq!(
            summary_msg.chars().count(),
            "搜索 失败：".chars().count() + 120
        );

        let timeout_step = format_timeout_step(5000, "timeout");
        assert_eq!(timeout_step.stage, "timeout");
        assert!(!timeout_step.ok);

        #[derive(Serialize)]
        #[serde(rename_all = "snake_case")]
        enum MockMode {
            Full,
        }
        #[derive(Serialize)]
        struct MockPayload {
            extra_count: usize,
        }
        let report = SourceTestReportDto {
            ok: true,
            mode: MockMode::Full,
            keyword: "kw".into(),
            total_elapsed_ms: 12,
            result_count: 1,
            sample_titles: vec!["T".into()],
            error: None,
            diagnostics: ExecutionDiagnostics::default(),
            steps: vec![],
            payload: MockPayload { extra_count: 7 },
        };
        let report_json = serde_json::to_value(&report).unwrap();
        assert_eq!(report_json["extra_count"], 7);

        let u1 = Uuid::from_u128(1);
        let u2 = Uuid::from_u128(2);
        let u3 = Uuid::from_u128(3);
        assert_eq!(
            dedup_preserve_order_uuids(vec![u2, u1, u2, u3]),
            vec![u2, u1, u3]
        );

        let rows = vec![
            (u1, 10i64, "Alpha"),
            (u2, 50i64, "Beta"),
            (u3, 10i64, "Gamma"),
        ];
        let reordered = compute_reorder_targets(rows, &[u3], |r| r.0, |r| r.1, |r| r.2);
        assert_eq!(reordered[0].0.0, u3);
        assert_eq!(reordered[0].1, 30);
        assert_eq!(reordered[1].0.0, u2);
        assert_eq!(reordered[1].1, 20);
        assert_eq!(reordered[2].0.0, u1);
        assert_eq!(reordered[2].1, 10);

        let mut raw_map = HashMap::from([(u1, r#"{"name":"One"}"#), (u2, r#"{"name":"Two"}"#)]);
        let exported = format_export_batch_json(&[u2, u2, u1], &mut raw_map).unwrap();
        let arr: Vec<serde_json::Value> = serde_json::from_str(&exported).unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["name"], "Two");
        assert_eq!(arr[1]["name"], "One");
    }

    #[test]
    fn login_action_request_accepts_login_info_and_form_data_alias() {
        let req1: LoginActionRequest =
            serde_json::from_str(r#"{"login_info":{"user":"alice"},"action":"login()"}"#).unwrap();
        assert_eq!(
            req1.login_info.get("user").map(String::as_str),
            Some("alice")
        );
        assert_eq!(req1.action.as_deref(), Some("login()"));

        let req2: LoginActionRequest =
            serde_json::from_str(r#"{"form_data":{"user":"bob"},"action":"doAuth()"}"#).unwrap();
        assert_eq!(req2.login_info.get("user").map(String::as_str), Some("bob"));
        assert_eq!(req2.action.as_deref(), Some("doAuth()"));
    }

    #[test]
    fn merge_runtime_state_with_base_supports_clear_delete_and_cookie_pair_merge() {
        let base = SourceRuntimeState {
            variable: "v1".into(),
            login_info: HashMap::from([("u".into(), "a".into())]),
            cookies: HashMap::from([("h.test".into(), "a=1; b=2".into())]),
            cache: HashMap::from([("k1".into(), "val1".into())]),
            android_id: "aid".into(),
        };
        let mut updated_clear = base.clone();
        updated_clear.variable.clear();
        updated_clear.cache.remove("k1");
        updated_clear.cookies.remove("h.test");

        let cleared = merge_runtime_state_with_base(&base, Some(base.clone()), updated_clear, true);
        assert_eq!(cleared.variable, "");
        assert!(!cleared.cache.contains_key("k1"));
        assert!(!cleared.cookies.contains_key("h.test"));

        let base_empty = SourceRuntimeState::default();
        let mut upd_a = SourceRuntimeState::default();
        upd_a.cookies.insert("same.test".into(), "kA=valA".into());
        let mut upd_b = SourceRuntimeState::default();
        upd_b.cookies.insert("same.test".into(), "kB=valB".into());
        let merged = merge_runtime_state_with_base(&base_empty, Some(upd_a), upd_b, false);
        let ck = merged.cookies.get("same.test").unwrap();
        assert!(ck.contains("kA=valA"));
        assert!(ck.contains("kB=valB"));
    }
}

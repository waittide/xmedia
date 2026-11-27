use std::collections::{HashMap, HashSet};

use jiff::Timestamp;
use uuid::Uuid;
use xmedia_core::domain::library::MediaKind;

use super::dto::{MASKED_SECRET_PLACEHOLDER, SourceSummaryDto, VARIABLE_PREVIEW_MAX_CHARS};
use crate::model::{LoginUiItem, PluginFormat, SourceRuntimeState};

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
/// using the domain-specific `group_key`.
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

pub struct SourceRecordSnapshot<'a> {
    pub id: Uuid,
    pub domain: MediaKind,
    pub format: PluginFormat,
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
    if let Some(item) = login_ui.iter().find(|item| item.name == key) {
        if item.item_type.eq_ignore_ascii_case("password") {
            return true;
        }
        if item.item_type.eq_ignore_ascii_case("image")
            || item.item_type.eq_ignore_ascii_case("img")
            || item.item_type.eq_ignore_ascii_case("qrcode")
        {
            return false;
        }
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
) -> SourceSummaryDto {
    let login_info = mask_login_info(&rec.runtime_state.login_info, &meta.login_ui);
    let variable_preview: String = rec
        .runtime_state
        .variable
        .chars()
        .take(VARIABLE_PREVIEW_MAX_CHARS)
        .collect();
    let mut cookie_hosts: Vec<String> = rec.runtime_state.cookies.keys().cloned().collect();
    cookie_hosts.sort();

    SourceSummaryDto {
        id: rec.id,
        domain: rec.domain,
        format: rec.format,
        name: rec.name.to_string(),
        source_url: rec.source_url.to_string(),
        group_name: rec.group_name.to_string(),
        comment: meta.comment,
        check_keyword: meta.check_keyword,
        enabled: rec.enabled,
        weight: rec.weight,
        source_type: rec.source_type,
        has_login_url: meta.has_login_url,
        login_ui: meta.login_ui,
        login_info,
        has_variable: !rec.runtime_state.variable.trim().is_empty(),
        variable_preview,
        cookie_hosts,
        last_test_ok: rec.last_test_ok,
        last_test_msg: rec.last_test_msg.map(ToString::to_string),
        updated_at: rec.updated_at,
    }
}

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
    merged.settings.extend(new_state.settings);
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
    for key in base.settings.keys() {
        if !updated.settings.contains_key(key) && merged.settings.get(key) == base.settings.get(key)
        {
            merged.settings.remove(key);
        }
    }
    for (key, value) in updated.settings {
        if base.settings.get(&key) != Some(&value) {
            merged.settings.insert(key, value);
        }
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

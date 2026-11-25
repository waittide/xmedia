use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use rquickjs::context::EvalOptions;
use rquickjs::{Array, CatchResultExt, Ctx, Function, Object, Value};

use crate::model::ExecutionDiagnostics;
use crate::rule::RuleValue;

pub(super) fn push_diag_log(diag: &Arc<Mutex<ExecutionDiagnostics>>, msg: impl Into<String>) {
    let clean = msg.into();
    let trimmed = clean.trim();
    if trimmed.is_empty() {
        return;
    }
    let mut g = diag.lock().unwrap_or_else(|e| e.into_inner());
    if g.logs.len() < 100 {
        g.logs.push(trimmed.to_string());
    }
}

pub(super) fn map_total_bytes(map: &HashMap<String, String>) -> usize {
    map.iter().map(|(k, v)| k.len() + v.len()).sum()
}

pub(super) fn insert_bounded_map(
    map: &mut HashMap<String, String>,
    key: String,
    val: String,
    max_entries: usize,
    max_bytes: usize,
) -> bool {
    let is_new = !map.contains_key(&key);
    if is_new && map.len() >= max_entries {
        return false;
    }
    let current = map_total_bytes(map);
    let old_bytes = map.get(&key).map(|v| key.len() + v.len()).unwrap_or(0);
    let next = current
        .saturating_sub(old_bytes)
        .saturating_add(key.len())
        .saturating_add(val.len());
    if next > max_bytes {
        return false;
    }
    map.insert(key, val);
    true
}

pub(super) fn sloppy_eval_options() -> EvalOptions {
    let mut opts = EvalOptions::default();
    opts.global = true;
    opts.strict = false;
    opts
}

pub(super) fn json_to_js_value<'js>(
    ctx: &Ctx<'js>,
    val: &serde_json::Value,
) -> Result<Value<'js>, String> {
    match val {
        serde_json::Value::Null => Ok(Value::new_null(ctx.clone())),
        serde_json::Value::Bool(b) => Ok(Value::new_bool(ctx.clone(), *b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                if (i32::MIN as i64..=i32::MAX as i64).contains(&i) {
                    return Ok(Value::new_int(ctx.clone(), i as i32));
                }
                return Ok(Value::new_float(ctx.clone(), i as f64));
            }
            if let Some(u) = n.as_u64() {
                if u <= i32::MAX as u64 {
                    return Ok(Value::new_int(ctx.clone(), u as i32));
                }
                return Ok(Value::new_float(ctx.clone(), u as f64));
            }
            Ok(Value::new_float(ctx.clone(), n.as_f64().unwrap_or(0.0)))
        }
        serde_json::Value::String(s) => rquickjs::String::from_str(ctx.clone(), s.as_str())
            .map(Value::from_string)
            .map_err(|e| format!("转换 JS 字符串失败: {e}")),
        serde_json::Value::Array(arr) => {
            let js_arr = Array::new(ctx.clone()).map_err(|e| format!("创建 JS 数组失败: {e}"))?;
            for (idx, item) in arr.iter().enumerate() {
                let js_item = json_to_js_value(ctx, item)?;
                js_arr
                    .set(idx, js_item)
                    .map_err(|e| format!("写入 JS 数组元素失败: {e}"))?;
            }
            Ok(js_arr.into_value())
        }
        serde_json::Value::Object(map) => {
            let js_obj = Object::new(ctx.clone()).map_err(|e| format!("创建 JS 对象失败: {e}"))?;
            for (k, v) in map {
                let js_v = json_to_js_value(ctx, v)?;
                js_obj
                    .set(k.as_str(), js_v)
                    .map_err(|e| format!("写入 JS 对象属性失败: {e}"))?;
            }
            Ok(js_obj.into_value())
        }
    }
}

pub(super) fn js_value_to_rule_value<'js>(
    ctx: &Ctx<'js>,
    val: Value<'js>,
) -> Result<RuleValue, String> {
    if val.is_undefined() || val.is_null() {
        return Ok(RuleValue::Text(String::new()));
    }
    if let Some(s) = val.as_string() {
        let rust_str = s.to_string().map_err(|e| e.to_string())?;
        return Ok(RuleValue::from_raw(rust_str));
    }
    if let Some(b) = val.as_bool() {
        return Ok(RuleValue::Text(b.to_string()));
    }
    if let Some(i) = val.as_int() {
        return Ok(RuleValue::Text(i.to_string()));
    }
    if let Some(f) = val.as_float() {
        return Ok(RuleValue::Text(f.to_string()));
    }

    let json_obj: rquickjs::Object<'js> = ctx
        .globals()
        .get("JSON")
        .catch(ctx)
        .map_err(|e| super::format_caught_js_error("获取 JSON 对象失败", e))?;
    let stringify: Function<'js> = json_obj
        .get("stringify")
        .catch(ctx)
        .map_err(|e| super::format_caught_js_error("获取 JSON.stringify 失败", e))?;
    let serialized: Option<String> = stringify
        .call((val,))
        .catch(ctx)
        .map_err(|e| super::format_caught_js_error("序列化 JS 返回值失败", e))?;
    if let Some(s) = serialized
        && let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&s)
    {
        return Ok(RuleValue::Json(parsed));
    }
    Ok(RuleValue::Text(String::new()))
}

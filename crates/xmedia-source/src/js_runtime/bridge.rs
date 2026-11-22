use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::{STANDARD as BASE64_STD, STANDARD_NO_PAD as BASE64_NO_PAD};
use encoding_rs::Encoding;
use jiff::Timestamp;
use md5::{Digest, Md5};
use rquickjs::{Ctx, Exception, Function};

use crate::model::BrowserRedirectHint;
use crate::rule::{RuleValue, eval_non_js_list, eval_non_js_single};

use super::convert::{insert_bounded_map, map_total_bytes, push_diag_log};
use super::{SharedBridgeState, now_mono_ms};

pub(super) fn install_native_bridges(
    ctx: &Ctx<'_>,
    b: &SharedBridgeState,
    eval_deadline_ms: &Arc<AtomicU64>,
) -> Result<(), String> {
    let globals = ctx.globals();
    let cfg = &b.sandbox;

    // 1. HTTP bridges (`__x_ajax`, `__x_connect`)
    {
        let http = b.http.clone();
        let base_ref = b.current_base_url.clone();
        let diag = b.diagnostics.clone();
        let op_dl = b.op_deadline_ms.clone();
        let eval_dl = eval_deadline_ms.clone();
        let cancel = b.cancel_flag.clone();
        let req_cnt = b.http_req_count.clone();
        let max_http_requests = cfg.max_http_requests;
        let f = Function::new(
            ctx.clone(),
            move |ctx_js: Ctx<'_>, raw_url: String| -> rquickjs::Result<String> {
                let now = now_mono_ms();
                let deadline = op_dl
                    .load(Ordering::Relaxed)
                    .min(eval_dl.load(Ordering::Relaxed));
                if cancel.load(Ordering::Relaxed) || now >= deadline {
                    let msg = "java.ajax 已中断：超过操作超时或已取消";
                    push_diag_log(&diag, msg);
                    return Err(Exception::throw_message(&ctx_js, msg));
                }
                let count = req_cnt.fetch_add(1, Ordering::Relaxed) + 1;
                if count > max_http_requests {
                    let msg = format!(
                        "java.ajax 请求被拒：超过单次操作 HTTP 请求上限 ({max_http_requests})"
                    );
                    push_diag_log(&diag, msg.clone());
                    return Err(Exception::throw_message(&ctx_js, &msg));
                }
                let remaining = Duration::from_millis(deadline.saturating_sub(now));
                let base = base_ref.lock().unwrap_or_else(|e| e.into_inner()).clone();
                match http.fetch_with_timeout(&raw_url, &base, Some(remaining)) {
                    Ok(res) => Ok(res.body),
                    Err(err) => {
                        push_diag_log(&diag, format!("java.ajax 失败: {err}"));
                        Ok(String::new())
                    }
                }
            },
        )
        .map_err(|e| e.to_string())?;
        globals.set("__x_ajax", f).map_err(|e| e.to_string())?;
    }

    {
        let http = b.http.clone();
        let base_ref = b.current_base_url.clone();
        let diag = b.diagnostics.clone();
        let op_dl = b.op_deadline_ms.clone();
        let eval_dl = eval_deadline_ms.clone();
        let cancel = b.cancel_flag.clone();
        let req_cnt = b.http_req_count.clone();
        let max_http_requests = cfg.max_http_requests;
        let f = Function::new(
            ctx.clone(),
            move |ctx_js: Ctx<'_>, raw_url: String| -> rquickjs::Result<String> {
                let now = now_mono_ms();
                let deadline = op_dl
                    .load(Ordering::Relaxed)
                    .min(eval_dl.load(Ordering::Relaxed));
                if cancel.load(Ordering::Relaxed) || now >= deadline {
                    let err = "java.connect 已中断：超过操作超时或已取消";
                    push_diag_log(&diag, err);
                    return Err(Exception::throw_message(&ctx_js, err));
                }
                let count = req_cnt.fetch_add(1, Ordering::Relaxed) + 1;
                if count > max_http_requests {
                    let err = format!(
                        "java.connect 请求被拒：超过单次操作 HTTP 请求上限 ({max_http_requests})"
                    );
                    push_diag_log(&diag, err.clone());
                    return Err(Exception::throw_message(&ctx_js, &err));
                }
                let remaining = Duration::from_millis(deadline.saturating_sub(now));
                let base = base_ref.lock().unwrap_or_else(|e| e.into_inner()).clone();
                match http.fetch_with_timeout(&raw_url, &base, Some(remaining)) {
                    Ok(res) => Ok(serde_json::json!({
                        "body": res.body,
                        "code": res.status,
                        "url": res.final_url,
                        "headers": res.headers,
                    })
                    .to_string()),
                    Err(err) => {
                        push_diag_log(&diag, format!("java.connect 失败: {err}"));
                        Ok(serde_json::json!({
                            "body": "",
                            "code": 0,
                            "url": raw_url,
                            "headers": {},
                            "error": err,
                        })
                        .to_string())
                    }
                }
            },
        )
        .map_err(|e| e.to_string())?;
        globals.set("__x_connect", f).map_err(|e| e.to_string())?;
    }

    // 2. Encoding, crypto & time helpers
    {
        let f = Function::new(ctx.clone(), |s: String| -> String {
            BASE64_STD.encode(s.as_bytes())
        })
        .map_err(|e| e.to_string())?;
        globals.set("__x_b64_enc", f).map_err(|e| e.to_string())?;
    }

    {
        let f = Function::new(ctx.clone(), |s: String| -> String {
            let cleaned: String = s.chars().filter(|c| !c.is_whitespace()).collect();
            let decoded = BASE64_STD
                .decode(&cleaned)
                .or_else(|_| BASE64_NO_PAD.decode(cleaned.trim_end_matches('=')))
                .unwrap_or_default();
            String::from_utf8_lossy(&decoded).into_owned()
        })
        .map_err(|e| e.to_string())?;
        globals.set("__x_b64_dec", f).map_err(|e| e.to_string())?;
    }

    {
        let f = Function::new(ctx.clone(), |hex_str: String| -> String {
            let cleaned: String = hex_str.chars().filter(|c| !c.is_whitespace()).collect();
            match hex::decode(&cleaned) {
                Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
                Err(_) => hex_str,
            }
        })
        .map_err(|e| e.to_string())?;
        globals.set("__x_hex_dec", f).map_err(|e| e.to_string())?;
    }

    {
        let f = Function::new(ctx.clone(), |s: String| -> String {
            hex::encode(s.as_bytes())
        })
        .map_err(|e| e.to_string())?;
        globals.set("__x_hex_enc", f).map_err(|e| e.to_string())?;
    }

    {
        let f = Function::new(ctx.clone(), |s: String| -> String {
            let mut h = Md5::new();
            h.update(s.as_bytes());
            hex::encode(h.finalize())
        })
        .map_err(|e| e.to_string())?;
        globals.set("__x_md5", f).map_err(|e| e.to_string())?;
    }

    {
        let f = Function::new(ctx.clone(), |s: String, enc_label: String| -> String {
            let enc =
                Encoding::for_label(enc_label.trim().as_bytes()).unwrap_or(encoding_rs::UTF_8);
            let (bytes, _, _) = enc.encode(&s);
            let mut out = String::with_capacity(bytes.len() * 3);
            for &byte in bytes.as_ref() {
                if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
                    out.push(byte as char);
                } else {
                    out.push_str(&format!("%{:02X}", byte));
                }
            }
            out
        })
        .map_err(|e| e.to_string())?;
        globals
            .set("__x_encode_uri", f)
            .map_err(|e| e.to_string())?;
    }

    {
        let f = Function::new(ctx.clone(), |ms: f64| -> String {
            let millis = ms as i64;
            let secs = millis.div_euclid(1000);
            let ts = Timestamp::from_second(secs + 8 * 3600).unwrap_or(Timestamp::UNIX_EPOCH);
            let iso = ts.to_string();
            iso.replace('T', " ").trim_end_matches('Z').to_string()
        })
        .map_err(|e| e.to_string())?;
        globals.set("__x_time_fmt", f).map_err(|e| e.to_string())?;
    }

    // 3. Toasts, logs & browser redirect hints
    {
        let diag = b.diagnostics.clone();
        let f = Function::new(ctx.clone(), move |msg: String| {
            let clean = msg.trim().to_string();
            if !clean.is_empty() {
                let mut g = diag.lock().unwrap_or_else(|e| e.into_inner());
                if g.toasts.len() < 30 {
                    g.toasts.push(clean);
                }
            }
        })
        .map_err(|e| e.to_string())?;
        globals.set("__x_toast", f).map_err(|e| e.to_string())?;
    }

    {
        let diag = b.diagnostics.clone();
        let f = Function::new(ctx.clone(), move |msg: String| {
            let clean = msg.trim().to_string();
            if !clean.is_empty() {
                let mut g = diag.lock().unwrap_or_else(|e| e.into_inner());
                if g.logs.len() < 100 {
                    g.logs.push(clean);
                }
            }
        })
        .map_err(|e| e.to_string())?;
        globals.set("__x_log", f).map_err(|e| e.to_string())?;
    }

    {
        let diag = b.diagnostics.clone();
        let f = Function::new(ctx.clone(), move |url: String, title: String| {
            let clean_url = url.trim().to_string();
            let is_safe_http = url::Url::parse(&clean_url)
                .ok()
                .is_some_and(|u| matches!(u.scheme(), "http" | "https"));
            if is_safe_http {
                let mut g = diag.lock().unwrap_or_else(|e| e.into_inner());
                if g.browser_urls.len() < 10 {
                    g.browser_urls.push(BrowserRedirectHint {
                        url: clean_url,
                        title: title.trim().to_string(),
                    });
                }
            }
        })
        .map_err(|e| e.to_string())?;
        globals.set("__x_browser", f).map_err(|e| e.to_string())?;
    }

    // 4. Temp vars (`java.put` / `java.get`)
    {
        let tv = b.temp_vars.clone();
        let diag = b.diagnostics.clone();
        let max_entries = cfg.max_temp_vars_entries;
        let max_bytes = cfg.max_temp_vars_bytes;
        let f = Function::new(ctx.clone(), move |k: String, v: String| -> String {
            let mut g = tv.lock().unwrap_or_else(|e| e.into_inner());
            if insert_bounded_map(&mut g, k, v.clone(), max_entries, max_bytes) {
                v
            } else {
                push_diag_log(&diag, "java.put 写入被拒：临时变量超过配额上限");
                String::new()
            }
        })
        .map_err(|e| e.to_string())?;
        globals.set("__x_put_temp", f).map_err(|e| e.to_string())?;
    }

    {
        let tv = b.temp_vars.clone();
        let f = Function::new(ctx.clone(), move |k: String| -> String {
            let g = tv.lock().unwrap_or_else(|e| e.into_inner());
            g.get(&k).cloned().unwrap_or_default()
        })
        .map_err(|e| e.to_string())?;
        globals.set("__x_get_temp", f).map_err(|e| e.to_string())?;
    }

    // 5. Source state (`source.getVariable/setVariable/getLoginInfo/putLoginInfo`)
    {
        let st = b.runtime_state.clone();
        let f = Function::new(ctx.clone(), move || -> String {
            st.lock()
                .unwrap_or_else(|e| e.into_inner())
                .variable
                .clone()
        })
        .map_err(|e| e.to_string())?;
        globals
            .set("__x_src_get_var", f)
            .map_err(|e| e.to_string())?;
    }

    {
        let st = b.runtime_state.clone();
        let diag = b.diagnostics.clone();
        let max_var_bytes = cfg.max_variable_bytes;
        let f = Function::new(ctx.clone(), move |val: String| {
            if val.len() > max_var_bytes {
                push_diag_log(
                    &diag,
                    format!(
                        "source.setVariable 写入被拒：超过 {} KiB 上限",
                        max_var_bytes / 1024
                    ),
                );
                return;
            }
            st.lock().unwrap_or_else(|e| e.into_inner()).variable = val;
        })
        .map_err(|e| e.to_string())?;
        globals
            .set("__x_src_set_var", f)
            .map_err(|e| e.to_string())?;
    }

    {
        let st = b.runtime_state.clone();
        let f = Function::new(ctx.clone(), move || -> String {
            let g = st.lock().unwrap_or_else(|e| e.into_inner());
            serde_json::to_string(&g.login_info).unwrap_or_else(|_| "{}".to_string())
        })
        .map_err(|e| e.to_string())?;
        globals
            .set("__x_src_get_login", f)
            .map_err(|e| e.to_string())?;
    }

    {
        let st = b.runtime_state.clone();
        let diag = b.diagnostics.clone();
        let max_entries = cfg.max_login_info_entries;
        let max_bytes = cfg.max_login_info_bytes;
        let f = Function::new(ctx.clone(), move |raw: String| {
            if let Ok(map) = serde_json::from_str::<HashMap<String, String>>(&raw) {
                if map.len() > max_entries || map_total_bytes(&map) > max_bytes {
                    push_diag_log(&diag, "source.putLoginInfo 写入被拒：登录信息超过配额上限");
                    return;
                }
                st.lock().unwrap_or_else(|e| e.into_inner()).login_info = map;
            }
        })
        .map_err(|e| e.to_string())?;
        globals
            .set("__x_src_put_login", f)
            .map_err(|e| e.to_string())?;
    }

    {
        let st = b.runtime_state.clone();
        let f = Function::new(ctx.clone(), move || -> String {
            st.lock()
                .unwrap_or_else(|e| e.into_inner())
                .android_id
                .clone()
        })
        .map_err(|e| e.to_string())?;
        globals
            .set("__x_android_id", f)
            .map_err(|e| e.to_string())?;
    }

    // 6. Cookies
    {
        let http = b.http.clone();
        let f = Function::new(ctx.clone(), move |url: String| -> String {
            http.get_cookie_header(&url)
        })
        .map_err(|e| e.to_string())?;
        globals
            .set("__x_cookie_get", f)
            .map_err(|e| e.to_string())?;
    }

    {
        let http = b.http.clone();
        let diag = b.diagnostics.clone();
        let f = Function::new(ctx.clone(), move |url: String, val: String| {
            if !http.set_cookie(&url, &val) {
                push_diag_log(&diag, "cookie.setCookie 写入被拒：Cookie 存储超过配额上限");
            }
        })
        .map_err(|e| e.to_string())?;
        globals
            .set("__x_cookie_set", f)
            .map_err(|e| e.to_string())?;
    }

    {
        let http = b.http.clone();
        let f = Function::new(ctx.clone(), move |url: String| {
            http.remove_cookie(&url);
        })
        .map_err(|e| e.to_string())?;
        globals
            .set("__x_cookie_del", f)
            .map_err(|e| e.to_string())?;
    }

    // 7. Persistent & memory caches
    {
        let st = b.runtime_state.clone();
        let f = Function::new(ctx.clone(), move |k: String| -> Option<String> {
            st.lock()
                .unwrap_or_else(|e| e.into_inner())
                .cache
                .get(&k)
                .cloned()
        })
        .map_err(|e| e.to_string())?;
        globals.set("__x_cache_get", f).map_err(|e| e.to_string())?;
    }

    {
        let st = b.runtime_state.clone();
        let diag = b.diagnostics.clone();
        let max_entries = cfg.max_persistent_cache_entries;
        let max_bytes = cfg.max_persistent_cache_bytes;
        let f = Function::new(ctx.clone(), move |k: String, v: String| {
            let mut g = st.lock().unwrap_or_else(|e| e.into_inner());
            if !insert_bounded_map(&mut g.cache, k, v, max_entries, max_bytes) {
                push_diag_log(&diag, "cache.put 写入被拒：持久化缓存超过配额上限");
            }
        })
        .map_err(|e| e.to_string())?;
        globals.set("__x_cache_put", f).map_err(|e| e.to_string())?;
    }

    {
        let st = b.runtime_state.clone();
        let f = Function::new(ctx.clone(), move |k: String| {
            st.lock()
                .unwrap_or_else(|e| e.into_inner())
                .cache
                .remove(&k);
        })
        .map_err(|e| e.to_string())?;
        globals.set("__x_cache_del", f).map_err(|e| e.to_string())?;
    }

    {
        let mc = b.memory_cache.clone();
        let f = Function::new(ctx.clone(), move |k: String| -> Option<String> {
            mc.lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(&k)
                .cloned()
        })
        .map_err(|e| e.to_string())?;
        globals.set("__x_mem_get", f).map_err(|e| e.to_string())?;
    }

    {
        let mc = b.memory_cache.clone();
        let diag = b.diagnostics.clone();
        let max_entries = cfg.max_memory_cache_entries;
        let max_bytes = cfg.max_memory_cache_bytes;
        let f = Function::new(ctx.clone(), move |k: String, v: String| {
            let mut g = mc.lock().unwrap_or_else(|e| e.into_inner());
            if v.is_empty() {
                g.remove(&k);
                return;
            }
            if !insert_bounded_map(&mut g, k, v, max_entries, max_bytes) {
                push_diag_log(&diag, "cache.putMemory 写入被拒：内存缓存超过配额上限");
            }
        })
        .map_err(|e| e.to_string())?;
        globals.set("__x_mem_put", f).map_err(|e| e.to_string())?;
    }

    // 8. Rule helpers callable from JS (`java.getString`, `java.getElements`)
    {
        let cur_res = b.current_result.clone();
        let cur_base = b.current_base_url.clone();
        let f = Function::new(ctx.clone(), move |rule: String| -> String {
            let res = cur_res.lock().unwrap_or_else(|e| e.into_inner()).clone();
            let base = cur_base.lock().unwrap_or_else(|e| e.into_inner()).clone();
            eval_non_js_single(&res, &rule, &base)
        })
        .map_err(|e| e.to_string())?;
        globals
            .set("__x_get_string", f)
            .map_err(|e| e.to_string())?;
    }

    {
        let cur_res = b.current_result.clone();
        let f = Function::new(ctx.clone(), move |rule: String| -> String {
            let res = cur_res.lock().unwrap_or_else(|e| e.into_inner()).clone();
            let list: Vec<serde_json::Value> = eval_non_js_list(&res, &rule)
                .into_iter()
                .map(|item| match item {
                    RuleValue::Json(v) => v,
                    RuleValue::Text(s) => serde_json::Value::String(s),
                })
                .collect();
            serde_json::to_string(&list).unwrap_or_else(|_| "[]".to_string())
        })
        .map_err(|e| e.to_string())?;
        globals
            .set("__x_get_elements", f)
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}

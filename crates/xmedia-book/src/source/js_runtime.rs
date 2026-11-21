use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::{STANDARD as BASE64_STD, STANDARD_NO_PAD as BASE64_NO_PAD};
use encoding_rs::Encoding;
use jiff::Timestamp;
use md5::{Digest, Md5};
use rquickjs::context::EvalOptions;
use rquickjs::{Array, CatchResultExt, Context, Ctx, Function, Object, Runtime, Value};

use crate::source::http::{DEFAULT_WEBVIEW_UA, LegadoHttpClient};
use crate::source::model::{
    BookJsContext, BrowserRedirectHint, ChapterJsContext, ExecutionDiagnostics, LegadoBookSource,
    SourceRuntimeState,
};
use crate::source::rule::{RuleValue, eval_non_js_list, eval_non_js_single};

const JS_MEMORY_LIMIT_BYTES: usize = 64 * 1024 * 1024;
// Keep QuickJS stack cap well below Linux's default 2 MiB Rust/Tokio worker thread stack
// so deep JS recursion raises a catchable InternalError instead of triggering OS SIGSEGV.
const JS_MAX_STACK_BYTES: usize = 384 * 1024;

pub const MAX_TEMP_VARS_ENTRIES: usize = 512;
pub const MAX_TEMP_VARS_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_MEMORY_CACHE_ENTRIES: usize = 512;
pub const MAX_MEMORY_CACHE_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_PERSISTENT_CACHE_ENTRIES: usize = 512;
pub const MAX_PERSISTENT_CACHE_BYTES: usize = 1024 * 1024;
pub const MAX_VARIABLE_BYTES: usize = 256 * 1024;
pub const MAX_LOGIN_INFO_ENTRIES: usize = 128;
pub const MAX_LOGIN_INFO_BYTES: usize = 64 * 1024;
pub const MAX_JS_HTTP_REQUESTS: usize = 32;

static PROCESS_EPOCH: LazyLock<Instant> = LazyLock::new(Instant::now);

#[inline]
pub(crate) fn now_mono_ms() -> u64 {
    PROCESS_EPOCH.elapsed().as_millis() as u64
}

fn push_diag_log(diag: &Arc<Mutex<ExecutionDiagnostics>>, msg: impl Into<String>) {
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

fn map_total_bytes(map: &HashMap<String, String>) -> usize {
    map.iter().map(|(k, v)| k.len() + v.len()).sum()
}

fn insert_bounded_map(
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

#[derive(Clone)]
struct SharedBridgeState {
    http: LegadoHttpClient,
    runtime_state: Arc<Mutex<SourceRuntimeState>>,
    temp_vars: Arc<Mutex<HashMap<String, String>>>,
    memory_cache: Arc<Mutex<HashMap<String, String>>>,
    diagnostics: Arc<Mutex<ExecutionDiagnostics>>,
    current_result: Arc<Mutex<RuleValue>>,
    current_base_url: Arc<Mutex<String>>,
    source_name: String,
    source_url: String,
    login_ui_raw: String,
    op_deadline_ms: Arc<AtomicU64>,
    cancel_flag: Arc<AtomicBool>,
    http_req_count: Arc<AtomicUsize>,
}

/// Reusable QuickJS sandbox session for one book source operation (compiles `jsLib` once).
pub struct LegadoJsSession {
    _runtime: Runtime,
    context: Context,
    eval_deadline_ms: Arc<AtomicU64>,
    op_deadline_ms: Arc<AtomicU64>,
    cancel_flag: Arc<AtomicBool>,
    timeout: Duration,
    bridge: SharedBridgeState,
}

impl LegadoJsSession {
    pub fn new(
        source: &LegadoBookSource,
        http: LegadoHttpClient,
        runtime_state: Arc<Mutex<SourceRuntimeState>>,
        diagnostics: Arc<Mutex<ExecutionDiagnostics>>,
        timeout: Duration,
    ) -> Result<Self, String> {
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let op_deadline_ms = Arc::new(AtomicU64::new(
            now_mono_ms().saturating_add(timeout.as_millis() as u64),
        ));
        Self::new_with_control(
            source,
            http,
            runtime_state,
            diagnostics,
            timeout,
            op_deadline_ms,
            cancel_flag,
        )
    }

    pub fn new_with_control(
        source: &LegadoBookSource,
        http: LegadoHttpClient,
        runtime_state: Arc<Mutex<SourceRuntimeState>>,
        diagnostics: Arc<Mutex<ExecutionDiagnostics>>,
        timeout: Duration,
        op_deadline_ms: Arc<AtomicU64>,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<Self, String> {
        let runtime = Runtime::new().map_err(|e| format!("初始化 QuickJS 运行时失败: {e}"))?;
        runtime.set_memory_limit(JS_MEMORY_LIMIT_BYTES);
        runtime.set_max_stack_size(JS_MAX_STACK_BYTES);

        let now = now_mono_ms();
        let op_end = op_deadline_ms.load(Ordering::Relaxed);
        let eval_end = now.saturating_add(timeout.as_millis() as u64).min(op_end);
        let eval_deadline_ms = Arc::new(AtomicU64::new(eval_end));

        let eval_check = eval_deadline_ms.clone();
        let op_check = op_deadline_ms.clone();
        let cancel_check = cancel_flag.clone();
        runtime.set_interrupt_handler(Some(Box::new(move || {
            if cancel_check.load(Ordering::Relaxed) {
                return true;
            }
            let now = now_mono_ms();
            now > eval_check.load(Ordering::Relaxed) || now > op_check.load(Ordering::Relaxed)
        })));

        let context =
            Context::full(&runtime).map_err(|e| format!("初始化 QuickJS 上下文失败: {e}"))?;

        let bridge = SharedBridgeState {
            http,
            runtime_state,
            temp_vars: Arc::new(Mutex::new(HashMap::new())),
            memory_cache: Arc::new(Mutex::new(HashMap::new())),
            diagnostics,
            current_result: Arc::new(Mutex::new(RuleValue::Text(String::new()))),
            current_base_url: Arc::new(Mutex::new(source.book_source_url.clone())),
            source_name: source.book_source_name.clone(),
            source_url: source.book_source_url.clone(),
            login_ui_raw: source.login_ui.clone().unwrap_or_default(),
            op_deadline_ms: op_deadline_ms.clone(),
            cancel_flag: cancel_flag.clone(),
            http_req_count: Arc::new(AtomicUsize::new(0)),
        };

        let mut session = Self {
            _runtime: runtime,
            context,
            eval_deadline_ms,
            op_deadline_ms,
            cancel_flag,
            timeout,
            bridge,
        };

        session.install_bridges()?;

        // Pre-load `jsLib` into global scope once (non-strict mode to match Legado Rhino `this === globalThis`)
        if let Some(ref js_lib) = source.js_lib
            && !js_lib.trim().is_empty()
        {
            session.check_active()?;
            session.reset_deadline();
            session.context.with(|ctx| {
                ctx.eval_with_options::<(), _>(js_lib.as_str(), sloppy_eval_options())
                    .catch(&ctx)
                    .map_err(|e| format!("加载书源 jsLib 失败: {e}"))
            })?;
        }

        Ok(session)
    }

    pub fn check_active(&self) -> Result<(), String> {
        if self.cancel_flag.load(Ordering::Relaxed) {
            return Err("执行已取消 (interrupted)".into());
        }
        if now_mono_ms() > self.op_deadline_ms.load(Ordering::Relaxed) {
            return Err("执行超过单次操作总超时上限 (interrupted)".into());
        }
        Ok(())
    }

    /// Refreshes the per-eval budget, clamped by the operation-wide `op_deadline_ms`.
    fn reset_deadline(&self) {
        let op_end = self.op_deadline_ms.load(Ordering::Relaxed);
        let eval_end = now_mono_ms()
            .saturating_add(self.timeout.as_millis() as u64)
            .min(op_end);
        self.eval_deadline_ms.store(eval_end, Ordering::Relaxed);
    }

    /// Resets per-operation deadline and HTTP request counter when reusing a worker session
    /// across independent chapters in batch downloads.
    pub fn reset_operation_budget(&self, op_timeout: Duration) {
        self.op_deadline_ms.store(
            now_mono_ms().saturating_add(op_timeout.as_millis() as u64),
            Ordering::Relaxed,
        );
        self.bridge.http_req_count.store(0, Ordering::Relaxed);
        self.reset_deadline();
    }

    fn install_bridges(&mut self) -> Result<(), String> {
        self.reset_deadline();
        let b = self.bridge.clone();
        self.context.with(|ctx| -> Result<(), String> {
            let globals = ctx.globals();

            // 1. Native helper functions bound to Rust bridge
            {
                let http = b.http.clone();
                let base_ref = b.current_base_url.clone();
                let diag = b.diagnostics.clone();
                let op_dl = b.op_deadline_ms.clone();
                let eval_dl = self.eval_deadline_ms.clone();
                let cancel = b.cancel_flag.clone();
                let req_cnt = b.http_req_count.clone();
                let f = Function::new(ctx.clone(), move |raw_url: String| -> String {
                    let now = now_mono_ms();
                    let deadline = op_dl
                        .load(Ordering::Relaxed)
                        .min(eval_dl.load(Ordering::Relaxed));
                    if cancel.load(Ordering::Relaxed) || now >= deadline {
                        push_diag_log(&diag, "java.ajax 已中断：超过操作超时或已取消");
                        return String::new();
                    }
                    let count = req_cnt.fetch_add(1, Ordering::Relaxed) + 1;
                    if count > MAX_JS_HTTP_REQUESTS {
                        push_diag_log(
                            &diag,
                            format!(
                                "java.ajax 请求被拒：超过单次操作 HTTP 请求上限 ({MAX_JS_HTTP_REQUESTS})"
                            ),
                        );
                        return String::new();
                    }
                    let remaining = Duration::from_millis(deadline.saturating_sub(now));
                    let base = base_ref.lock().unwrap_or_else(|e| e.into_inner()).clone();
                    match http.fetch_with_timeout(&raw_url, &base, Some(remaining)) {
                        Ok(res) => res.body,
                        Err(err) => {
                            push_diag_log(&diag, format!("java.ajax 失败: {err}"));
                            String::new()
                        }
                    }
                })
                .map_err(|e| e.to_string())?;
                globals.set("__x_ajax", f).map_err(|e| e.to_string())?;
            }

            {
                let http = b.http.clone();
                let base_ref = b.current_base_url.clone();
                let diag = b.diagnostics.clone();
                let op_dl = b.op_deadline_ms.clone();
                let eval_dl = self.eval_deadline_ms.clone();
                let cancel = b.cancel_flag.clone();
                let req_cnt = b.http_req_count.clone();
                let f = Function::new(ctx.clone(), move |raw_url: String| -> String {
                    let now = now_mono_ms();
                    let deadline = op_dl
                        .load(Ordering::Relaxed)
                        .min(eval_dl.load(Ordering::Relaxed));
                    if cancel.load(Ordering::Relaxed) || now >= deadline {
                        let err = "java.connect 已中断：超过操作超时或已取消";
                        push_diag_log(&diag, err);
                        return serde_json::json!({
                            "body": "",
                            "code": 0,
                            "url": raw_url,
                            "headers": {},
                            "error": err,
                        })
                        .to_string();
                    }
                    let count = req_cnt.fetch_add(1, Ordering::Relaxed) + 1;
                    if count > MAX_JS_HTTP_REQUESTS {
                        let err = format!(
                            "java.connect 请求被拒：超过单次操作 HTTP 请求上限 ({MAX_JS_HTTP_REQUESTS})"
                        );
                        push_diag_log(&diag, err.clone());
                        return serde_json::json!({
                            "body": "",
                            "code": 0,
                            "url": raw_url,
                            "headers": {},
                            "error": err,
                        })
                        .to_string();
                    }
                    let remaining = Duration::from_millis(deadline.saturating_sub(now));
                    let base = base_ref.lock().unwrap_or_else(|e| e.into_inner()).clone();
                    match http.fetch_with_timeout(&raw_url, &base, Some(remaining)) {
                        Ok(res) => serde_json::json!({
                            "body": res.body,
                            "code": res.status,
                            "url": res.final_url,
                            "headers": res.headers,
                        })
                        .to_string(),
                        Err(err) => {
                            push_diag_log(&diag, format!("java.connect 失败: {err}"));
                            serde_json::json!({
                                "body": "",
                                "code": 0,
                                "url": raw_url,
                                "headers": {},
                                "error": err,
                            })
                            .to_string()
                        }
                    }
                })
                .map_err(|e| e.to_string())?;
                globals.set("__x_connect", f).map_err(|e| e.to_string())?;
            }

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
                    let cleaned: String =
                        hex_str.chars().filter(|c| !c.is_whitespace()).collect();
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
                    let enc = Encoding::for_label(enc_label.trim().as_bytes())
                        .unwrap_or(encoding_rs::UTF_8);
                    let (bytes, _, _) = enc.encode(&s);
                    let mut out = String::with_capacity(bytes.len() * 3);
                    for &byte in bytes.as_ref() {
                        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~')
                        {
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
                    // Format in UTC+8 (Chinese Legado convention)
                    let ts = Timestamp::from_second(secs + 8 * 3600).unwrap_or(Timestamp::UNIX_EPOCH);
                    let iso = ts.to_string(); // e.g. 2026-01-02T03:04:05Z
                    iso.replace('T', " ").trim_end_matches('Z').to_string()
                })
                .map_err(|e| e.to_string())?;
                globals
                    .set("__x_time_fmt", f)
                    .map_err(|e| e.to_string())?;
            }

            // Toasts & Logs
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

            // Temp vars (`java.put` / `java.get`)
            {
                let tv = b.temp_vars.clone();
                let diag = b.diagnostics.clone();
                let f = Function::new(ctx.clone(), move |k: String, v: String| -> String {
                    let mut g = tv.lock().unwrap_or_else(|e| e.into_inner());
                    if insert_bounded_map(
                        &mut g,
                        k,
                        v.clone(),
                        MAX_TEMP_VARS_ENTRIES,
                        MAX_TEMP_VARS_BYTES,
                    ) {
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

            // Source state (`source.getVariable`, `setVariable`, `getLoginInfo`, `putLoginInfo`)
            {
                let st = b.runtime_state.clone();
                let f = Function::new(ctx.clone(), move || -> String {
                    st.lock().unwrap_or_else(|e| e.into_inner()).variable.clone()
                })
                .map_err(|e| e.to_string())?;
                globals
                    .set("__x_src_get_var", f)
                    .map_err(|e| e.to_string())?;
            }

            {
                let st = b.runtime_state.clone();
                let diag = b.diagnostics.clone();
                let f = Function::new(ctx.clone(), move |val: String| {
                    if val.len() > MAX_VARIABLE_BYTES {
                        push_diag_log(
                            &diag,
                            format!(
                                "source.setVariable 写入被拒：超过 {} KiB 上限",
                                MAX_VARIABLE_BYTES / 1024
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
                let f = Function::new(ctx.clone(), move |raw: String| {
                    if let Ok(map) = serde_json::from_str::<HashMap<String, String>>(&raw) {
                        if map.len() > MAX_LOGIN_INFO_ENTRIES
                            || map_total_bytes(&map) > MAX_LOGIN_INFO_BYTES
                        {
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

            // Cookies
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

            // Persistent & memory cache (`cache.get/put/delete/getFromMemory/putMemory`)
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
                globals
                    .set("__x_cache_get", f)
                    .map_err(|e| e.to_string())?;
            }

            {
                let st = b.runtime_state.clone();
                let diag = b.diagnostics.clone();
                let f = Function::new(ctx.clone(), move |k: String, v: String| {
                    let mut g = st.lock().unwrap_or_else(|e| e.into_inner());
                    if !insert_bounded_map(
                        &mut g.cache,
                        k,
                        v,
                        MAX_PERSISTENT_CACHE_ENTRIES,
                        MAX_PERSISTENT_CACHE_BYTES,
                    ) {
                        push_diag_log(&diag, "cache.put 写入被拒：持久化缓存超过配额上限");
                    }
                })
                .map_err(|e| e.to_string())?;
                globals
                    .set("__x_cache_put", f)
                    .map_err(|e| e.to_string())?;
            }

            {
                let st = b.runtime_state.clone();
                let f = Function::new(ctx.clone(), move |k: String| {
                    st.lock().unwrap_or_else(|e| e.into_inner()).cache.remove(&k);
                })
                .map_err(|e| e.to_string())?;
                globals
                    .set("__x_cache_del", f)
                    .map_err(|e| e.to_string())?;
            }

            {
                let mc = b.memory_cache.clone();
                let f = Function::new(ctx.clone(), move |k: String| -> Option<String> {
                    mc.lock().unwrap_or_else(|e| e.into_inner()).get(&k).cloned()
                })
                .map_err(|e| e.to_string())?;
                globals
                    .set("__x_mem_get", f)
                    .map_err(|e| e.to_string())?;
            }

            {
                let mc = b.memory_cache.clone();
                let diag = b.diagnostics.clone();
                let f = Function::new(ctx.clone(), move |k: String, v: String| {
                    let mut g = mc.lock().unwrap_or_else(|e| e.into_inner());
                    if v.is_empty() {
                        g.remove(&k);
                        return;
                    }
                    if !insert_bounded_map(
                        &mut g,
                        k,
                        v,
                        MAX_MEMORY_CACHE_ENTRIES,
                        MAX_MEMORY_CACHE_BYTES,
                    ) {
                        push_diag_log(&diag, "cache.putMemory 写入被拒：内存缓存超过配额上限");
                    }
                })
                .map_err(|e| e.to_string())?;
                globals
                    .set("__x_mem_put", f)
                    .map_err(|e| e.to_string())?;
            }

            // Rule helpers callable from JS (`java.getString`, `java.getElements`)
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

            // 2. Bootstrap full Legado JS runtime objects on globalThis
            let bootstrap = format!(
                r#"
                var __srcName = {src_name};
                var __srcUrl = {src_url};
                var __loginUiRaw = {login_ui};
                var __defaultUA = {ua};

                globalThis.cookie = {{
                    getCookie: function(url, key) {{
                        var raw = __x_cookie_get(String(url || ''));
                        if (!key) return raw;
                        var parts = raw.split(';');
                        for (var i = 0; i < parts.length; i++) {{
                            var p = parts[i].trim();
                            var eq = p.indexOf('=');
                            if (eq > 0 && p.slice(0, eq).trim() === String(key)) {{
                                return p.slice(eq + 1).trim();
                            }}
                        }}
                        return '';
                    }},
                    setCookie: function(url, val) {{
                        __x_cookie_set(String(url || ''), String(val || ''));
                    }},
                    removeCookie: function(url) {{
                        __x_cookie_del(String(url || ''));
                    }},
                    replaceCookie: function(url, val) {{
                        __x_cookie_del(String(url || ''));
                        __x_cookie_set(String(url || ''), String(val || ''));
                    }}
                }};

                globalThis.cache = {{
                    get: function(k) {{
                        var v = __x_cache_get(String(k));
                        return v === undefined ? null : v;
                    }},
                    put: function(k, v) {{
                        __x_cache_put(String(k), v == null ? '' : String(v));
                    }},
                    delete: function(k) {{
                        __x_cache_del(String(k));
                    }},
                    deleteMemory: function(k) {{
                        __x_mem_put(String(k), '');
                    }},
                    getFromMemory: function(k) {{
                        var v = __x_mem_get(String(k));
                        return v === undefined ? null : v;
                    }},
                    putMemory: function(k, v) {{
                        __x_mem_put(String(k), v == null ? '' : String(v));
                    }}
                }};

                globalThis.source = {{
                    bookSourceName: __srcName,
                    bookSourceUrl: __srcUrl,
                    getKey: function() {{ return __srcUrl; }},
                    getVariable: function() {{ return __x_src_get_var(); }},
                    setVariable: function(v) {{ __x_src_set_var(v == null ? '' : String(v)); }},
                    getLoginInfo: function() {{ return __x_src_get_login(); }},
                    getLoginInfoMap: function() {{
                        try {{ return JSON.parse(__x_src_get_login() || '{{}}'); }} catch (e) {{ return {{}}; }}
                    }},
                    putLoginInfo: function(v) {{
                        var s = typeof v === 'string' ? v : JSON.stringify(v || {{}});
                        __x_src_put_login(s);
                    }},
                    loginUi: function() {{ return __loginUiRaw; }}
                }};

                globalThis.java = {{
                    ajax: function(url) {{
                        return __x_ajax(String(url || ''));
                    }},
                    connect: function(url) {{
                        var resp = JSON.parse(__x_connect(String(url || '')));
                        return {{
                            body: function() {{ return resp.body || ''; }},
                            code: function() {{ return resp.code || 0; }},
                            url: function() {{ return resp.url || ''; }},
                            headers: function() {{ return resp.headers || {{}}; }}
                        }};
                    }},
                    get: function(url, headers) {{
                        var u = String(url || '');
                        if (headers && typeof headers === 'object') {{
                            u = u + ',' + JSON.stringify({{ method: 'GET', headers: headers }});
                        }}
                        return this.connect(u);
                    }},
                    post: function(url, body, headers) {{
                        var opt = {{ method: 'POST', body: body == null ? '' : (typeof body === 'string' ? body : JSON.stringify(body)) }};
                        if (headers && typeof headers === 'object') opt.headers = headers;
                        return this.connect(String(url || '') + ',' + JSON.stringify(opt));
                    }},
                    base64Encode: function(s) {{ return __x_b64_enc(s == null ? '' : String(s)); }},
                    base64Decode: function(s) {{ return __x_b64_dec(s == null ? '' : String(s)); }},
                    hexDecodeToString: function(s) {{ return __x_hex_dec(s == null ? '' : String(s)); }},
                    hexEncodeToString: function(s) {{ return __x_hex_enc(s == null ? '' : String(s)); }},
                    md5Encode: function(s) {{ return __x_md5(s == null ? '' : String(s)); }},
                    md5Encode16: function(s) {{ return __x_md5(s == null ? '' : String(s)).slice(8, 24); }},
                    encodeURI: function(s, enc) {{ return __x_encode_uri(s == null ? '' : String(s), enc ? String(enc) : 'utf-8'); }},
                    timeFormat: function(ts) {{
                        var ms = ts instanceof Date ? ts.getTime() : Number(ts);
                        if (!isFinite(ms)) ms = Date.now();
                        return __x_time_fmt(ms);
                    }},
                    getCookie: function(url, key) {{ return globalThis.cookie.getCookie(url, key); }},
                    getWebViewUA: function() {{ return __defaultUA; }},
                    androidId: function() {{ return __x_android_id(); }},
                    toast: function(msg) {{ __x_toast(msg == null ? '' : String(msg)); }},
                    longToast: function(msg) {{ __x_toast(msg == null ? '' : String(msg)); }},
                    log: function(msg) {{ __x_log(msg == null ? '' : (typeof msg === 'object' ? JSON.stringify(msg) : String(msg))); }},
                    put: function(k, v) {{ return __x_put_temp(String(k), v == null ? '' : String(v)); }},
                    get: function(k) {{ return __x_get_temp(String(k)); }},
                    getString: function(rule) {{ return __x_get_string(String(rule || '')); }},
                    getElements: function(rule) {{
                        try {{ return JSON.parse(__x_get_elements(String(rule || ''))); }} catch (e) {{ return []; }}
                    }},
                    refreshExplore: function() {{}},
                    startBrowser: function(url, title) {{
                        __x_browser(String(url || ''), String(title || ''));
                    }},
                    startBrowserAwait: function(url, title) {{
                        var u = String(url || '');
                        if (u.indexOf('data:text/html') === 0) {{
                            throw new Error('Web 端不支持内置 HTML 弹窗设置，请使用下方备用设置按钮');
                        }}
                        __x_browser(u, String(title || ''));
                        return {{ body: function() {{ return ''; }} }};
                    }}
                }};

                globalThis.console = {{
                    log: function() {{
                        var parts = [];
                        for (var i = 0; i < arguments.length; i++) parts.push(String(arguments[i]));
                        globalThis.java.log(parts.join(' '));
                    }},
                    warn: function() {{ globalThis.console.log.apply(null, arguments); }},
                    error: function() {{ globalThis.console.log.apply(null, arguments); }}
                }};

                globalThis.Packages = {{
                    java: {{
                        lang: {{
                            Thread: {{
                                sleep: function() {{}}
                            }}
                        }}
                    }}
                }};

                globalThis.book = {{
                    name: '',
                    author: '',
                    bookUrl: '',
                    tocUrl: '',
                    coverUrl: '',
                    abstract: '',
                    durChapterIndex: 0,
                    durChapterTitle: '',
                    order: 0,
                    type: 8,
                    readConfig: {{ useReplaceRule: false }},
                    _vars: {{}},
                    getVariable: function(k) {{ return this._vars[String(k)] || ''; }},
                    putVariable: function(k, v) {{ this._vars[String(k)] = v == null ? '' : String(v); }},
                    setUseReplaceRule: function(v) {{ this.readConfig.useReplaceRule = Boolean(v); }}
                }};

                globalThis.chapter = {{
                    index: 0,
                    title: '',
                    url: ''
                }};

                globalThis.result = '';
                globalThis.baseUrl = __srcUrl;
                globalThis.key = '';
                globalThis.page = 1;
                globalThis.__x_eval_rule = function(__x_code) {{
                    return (function() {{ return eval(__x_code); }}).call(globalThis);
                }};
                "#,
                src_name = serde_json::to_string(&b.source_name).unwrap_or_else(|_| "\"\"".into()),
                src_url = serde_json::to_string(&b.source_url).unwrap_or_else(|_| "\"\"".into()),
                login_ui =
                    serde_json::to_string(&b.login_ui_raw).unwrap_or_else(|_| "\"\"".into()),
                ua = serde_json::to_string(DEFAULT_WEBVIEW_UA).unwrap_or_else(|_| "\"\"".into()),
            );

            ctx.eval_with_options::<(), _>(bootstrap.as_str(), sloppy_eval_options())
                .catch(&ctx)
                .map_err(|e| format!("初始化 Legado JS 桥接环境失败: {e}"))?;
            Ok(())
        })
    }

    pub fn set_search_vars(&mut self, keyword: &str, page: u32) -> Result<(), String> {
        self.reset_deadline();
        let kw = keyword.to_string();
        self.context.with(|ctx| {
            let g = ctx.globals();
            g.set("key", kw).map_err(|e| e.to_string())?;
            g.set("page", page as i32).map_err(|e| e.to_string())?;
            Ok(())
        })
    }

    pub fn set_book_context(&mut self, book: &BookJsContext) -> Result<(), String> {
        self.reset_deadline();
        self.context.with(|ctx| {
            let book_obj: Object<'_> = ctx
                .globals()
                .get("book")
                .map_err(|e| format!("获取 book 上下文失败: {e}"))?;
            book_obj
                .set("name", book.name.as_str())
                .map_err(|e| format!("注入 book 上下文失败: {e}"))?;
            book_obj
                .set("author", book.author.as_str())
                .map_err(|e| format!("注入 book 上下文失败: {e}"))?;
            book_obj
                .set("bookUrl", book.book_url.as_str())
                .map_err(|e| format!("注入 book 上下文失败: {e}"))?;
            book_obj
                .set("tocUrl", book.toc_url.as_str())
                .map_err(|e| format!("注入 book 上下文失败: {e}"))?;
            book_obj
                .set("coverUrl", book.cover_url.as_deref().unwrap_or(""))
                .map_err(|e| format!("注入 book 上下文失败: {e}"))?;
            book_obj
                .set("abstract", book.intro.as_deref().unwrap_or(""))
                .map_err(|e| format!("注入 book 上下文失败: {e}"))?;
            book_obj
                .set("durChapterIndex", book.dur_chapter_index as i64)
                .map_err(|e| format!("注入 book 上下文失败: {e}"))?;
            book_obj
                .set("durChapterTitle", book.dur_chapter_title.as_str())
                .map_err(|e| format!("注入 book 上下文失败: {e}"))?;
            if let Ok(vars_obj) = book_obj.get::<_, Object<'_>>("_vars") {
                let _ = vars_obj.set("custom", book.custom_variable.as_str());
            }
            Ok(())
        })
    }

    pub fn set_chapter_context(&mut self, chapter: &ChapterJsContext) -> Result<(), String> {
        self.reset_deadline();
        self.context.with(|ctx| {
            let ch_obj: Object<'_> = ctx
                .globals()
                .get("chapter")
                .map_err(|e| format!("获取 chapter 上下文失败: {e}"))?;
            ch_obj
                .set("index", chapter.index as i64)
                .map_err(|e| format!("注入 chapter 上下文失败: {e}"))?;
            ch_obj
                .set("title", chapter.title.as_str())
                .map_err(|e| format!("注入 chapter 上下文失败: {e}"))?;
            ch_obj
                .set("url", chapter.url.as_str())
                .map_err(|e| format!("注入 chapter 上下文失败: {e}"))?;
            Ok(())
        })
    }

    /// Evaluates a rule's JS snippet with `result` and `baseUrl` bound via `globals.set`, returning a `RuleValue`.
    pub fn eval_rule_js(
        &mut self,
        code: &str,
        input: &RuleValue,
        base_url: &str,
    ) -> Result<RuleValue, String> {
        self.check_active()?;
        self.reset_deadline();

        {
            *self
                .bridge
                .current_result
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = input.clone();
            *self
                .bridge
                .current_base_url
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = base_url.to_string();
        }

        self.context.with(|ctx| {
            let globals = ctx.globals();
            let js_result = match input {
                RuleValue::Json(v) => json_to_js_value(&ctx, v)?,
                RuleValue::Text(s) => rquickjs::String::from_str(ctx.clone(), s.as_str())
                    .map(Value::from_string)
                    .map_err(|e| format!("设置 JS result 失败: {e}"))?,
            };
            globals
                .set("result", js_result)
                .map_err(|e| format!("设置 JS result 失败: {e}"))?;
            globals
                .set("baseUrl", base_url)
                .map_err(|e| format!("设置 JS baseUrl 失败: {e}"))?;

            let runner: Function<'_> = globals
                .get("__x_eval_rule")
                .map_err(|e| format!("获取 JS 规则执行器失败: {e}"))?;
            let val: Value<'_> = runner
                .call((code,))
                .catch(&ctx)
                .map_err(|e| format!("执行书源 JS 规则失败: {e}"))?;

            js_value_to_rule_value(&ctx, val)
        })
    }

    /// Evaluates `loginUrl` definitions followed by an optional `action` call (e.g. `login(true)`).
    pub fn eval_login_script(
        &mut self,
        login_url_js: &str,
        action_js: Option<&str>,
        login_info: &HashMap<String, String>,
    ) -> Result<String, String> {
        self.check_active()?;
        self.reset_deadline();
        let login_val = serde_json::to_value(login_info)
            .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new()));
        self.context.with(|ctx| {
            let js_login = json_to_js_value(&ctx, &login_val)?;
            ctx.globals()
                .set("result", js_login)
                .map_err(|e| format!("设置登录表单数据失败: {e}"))?;

            if !login_url_js.trim().is_empty() {
                ctx.eval_with_options::<(), _>(login_url_js, sloppy_eval_options())
                    .catch(&ctx)
                    .map_err(|e| format!("加载书源 loginUrl 脚本失败: {e}"))?;
            }

            if let Some(act) = action_js
                && !act.trim().is_empty()
            {
                let val: Value<'_> = ctx
                    .eval_with_options(act, sloppy_eval_options())
                    .catch(&ctx)
                    .map_err(|e| format!("执行登录动作 '{act}' 失败: {e}"))?;
                return js_value_to_rule_value(&ctx, val).map(|rv| rv.as_text());
            }

            Ok(String::new())
        })
    }
}

fn json_to_js_value<'js>(ctx: &Ctx<'js>, val: &serde_json::Value) -> Result<Value<'js>, String> {
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

fn sloppy_eval_options() -> EvalOptions {
    let mut opts = EvalOptions::default();
    opts.global = true;
    opts.strict = false;
    opts
}

fn js_value_to_rule_value<'js>(ctx: &Ctx<'js>, val: Value<'js>) -> Result<RuleValue, String> {
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

    // Object / Array -> JSON.stringify
    let json_obj: rquickjs::Object<'js> = ctx.globals().get("JSON").map_err(|e| e.to_string())?;
    let stringify: Function<'js> = json_obj.get("stringify").map_err(|e| e.to_string())?;
    let serialized: Option<String> = stringify.call((val,)).map_err(|e| e.to_string())?;
    if let Some(s) = serialized
        && let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&s)
    {
        return Ok(RuleValue::Json(parsed));
    }
    Ok(RuleValue::Text(String::new()))
}

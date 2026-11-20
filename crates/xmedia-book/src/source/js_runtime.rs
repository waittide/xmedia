use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::{STANDARD as BASE64_STD, STANDARD_NO_PAD as BASE64_NO_PAD};
use encoding_rs::Encoding;
use jiff::Timestamp;
use md5::{Digest, Md5};
use rquickjs::context::EvalOptions;
use rquickjs::{CatchResultExt, Context, Ctx, Function, Runtime, Value};

use crate::source::http::{DEFAULT_WEBVIEW_UA, LegadoHttpClient};
use crate::source::model::{
    BookJsContext, BrowserRedirectHint, ChapterJsContext, ExecutionDiagnostics, LegadoBookSource,
    SourceRuntimeState,
};
use crate::source::rule::{RuleValue, eval_non_js_list, eval_non_js_single};

const JS_MEMORY_LIMIT_BYTES: usize = 64 * 1024 * 1024;
const JS_MAX_STACK_BYTES: usize = 2 * 1024 * 1024;

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
}

/// Reusable QuickJS sandbox session for one book source operation (compiles `jsLib` once).
pub struct LegadoJsSession {
    _runtime: Runtime,
    context: Context,
    deadline: Arc<Mutex<Instant>>,
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
        let runtime = Runtime::new().map_err(|e| format!("初始化 QuickJS 运行时失败: {e}"))?;
        runtime.set_memory_limit(JS_MEMORY_LIMIT_BYTES);
        runtime.set_max_stack_size(JS_MAX_STACK_BYTES);

        let deadline = Arc::new(Mutex::new(Instant::now() + timeout));
        let deadline_check = deadline.clone();
        runtime.set_interrupt_handler(Some(Box::new(move || {
            let guard = deadline_check.lock().unwrap_or_else(|e| e.into_inner());
            Instant::now() > *guard
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
        };

        let mut session = Self {
            _runtime: runtime,
            context,
            deadline,
            timeout,
            bridge,
        };

        session.install_bridges()?;

        // Pre-load `jsLib` into global scope once (non-strict mode to match Legado Rhino `this === globalThis`)
        if let Some(ref js_lib) = source.js_lib
            && !js_lib.trim().is_empty()
        {
            session.reset_deadline();
            session.context.with(|ctx| {
                ctx.eval_with_options::<(), _>(js_lib.as_str(), sloppy_eval_options())
                    .catch(&ctx)
                    .map_err(|e| format!("加载书源 jsLib 失败: {e}"))
            })?;
        }

        Ok(session)
    }

    fn reset_deadline(&self) {
        let mut guard = self.deadline.lock().unwrap_or_else(|e| e.into_inner());
        *guard = Instant::now() + self.timeout;
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
                let f = Function::new(ctx.clone(), move |raw_url: String| -> String {
                    let base = base_ref.lock().unwrap_or_else(|e| e.into_inner()).clone();
                    match http.fetch(&raw_url, &base) {
                        Ok(res) => res.body,
                        Err(_) => String::new(),
                    }
                })
                .map_err(|e| e.to_string())?;
                globals.set("__x_ajax", f).map_err(|e| e.to_string())?;
            }

            {
                let http = b.http.clone();
                let base_ref = b.current_base_url.clone();
                let f = Function::new(ctx.clone(), move |raw_url: String| -> String {
                    let base = base_ref.lock().unwrap_or_else(|e| e.into_inner()).clone();
                    match http.fetch(&raw_url, &base) {
                        Ok(res) => serde_json::json!({
                            "body": res.body,
                            "code": res.status,
                            "url": res.final_url,
                            "headers": res.headers,
                        })
                        .to_string(),
                        Err(err) => serde_json::json!({
                            "body": "",
                            "code": 0,
                            "url": raw_url,
                            "headers": {},
                            "error": err,
                        })
                        .to_string(),
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
                    if !clean_url.is_empty() && !clean_url.starts_with("data:") {
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
                let f = Function::new(ctx.clone(), move |k: String, v: String| -> String {
                    let mut g = tv.lock().unwrap_or_else(|e| e.into_inner());
                    g.insert(k, v.clone());
                    v
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
                let f = Function::new(ctx.clone(), move |val: String| {
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
                let f = Function::new(ctx.clone(), move |raw: String| {
                    if let Ok(map) = serde_json::from_str::<HashMap<String, String>>(&raw) {
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
                let f = Function::new(ctx.clone(), move |url: String, val: String| {
                    http.set_cookie(&url, &val);
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
                let f = Function::new(ctx.clone(), move |k: String, v: String| {
                    st.lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .cache
                        .insert(k, v);
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
                let f = Function::new(ctx.clone(), move |k: String, v: String| {
                    mc.lock().unwrap_or_else(|e| e.into_inner()).insert(k, v);
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
        let script = format!(
            r#"
            globalThis.book.name = {name};
            globalThis.book.author = {author};
            globalThis.book.bookUrl = {book_url};
            globalThis.book.tocUrl = {toc_url};
            globalThis.book.coverUrl = {cover_url};
            globalThis.book.abstract = {intro};
            globalThis.book.durChapterIndex = {dur_idx};
            globalThis.book.durChapterTitle = {dur_title};
            globalThis.book._vars.custom = {custom_var};
            "#,
            name = serde_json::to_string(&book.name).unwrap_or_else(|_| "\"\"".into()),
            author = serde_json::to_string(&book.author).unwrap_or_else(|_| "\"\"".into()),
            book_url = serde_json::to_string(&book.book_url).unwrap_or_else(|_| "\"\"".into()),
            toc_url = serde_json::to_string(&book.toc_url).unwrap_or_else(|_| "\"\"".into()),
            cover_url = serde_json::to_string(book.cover_url.as_deref().unwrap_or(""))
                .unwrap_or_else(|_| "\"\"".into()),
            intro = serde_json::to_string(book.intro.as_deref().unwrap_or(""))
                .unwrap_or_else(|_| "\"\"".into()),
            dur_idx = book.dur_chapter_index,
            dur_title =
                serde_json::to_string(&book.dur_chapter_title).unwrap_or_else(|_| "\"\"".into()),
            custom_var =
                serde_json::to_string(&book.custom_variable).unwrap_or_else(|_| "\"\"".into()),
        );
        self.context.with(|ctx| {
            ctx.eval::<(), _>(script.as_str())
                .catch(&ctx)
                .map_err(|e| format!("注入 book 上下文失败: {e}"))
        })
    }

    pub fn set_chapter_context(&mut self, chapter: &ChapterJsContext) -> Result<(), String> {
        self.reset_deadline();
        let script = format!(
            r#"
            globalThis.chapter.index = {index};
            globalThis.chapter.title = {title};
            globalThis.chapter.url = {url};
            "#,
            index = chapter.index,
            title = serde_json::to_string(&chapter.title).unwrap_or_else(|_| "\"\"".into()),
            url = serde_json::to_string(&chapter.url).unwrap_or_else(|_| "\"\"".into()),
        );
        self.context.with(|ctx| {
            ctx.eval::<(), _>(script.as_str())
                .catch(&ctx)
                .map_err(|e| format!("注入 chapter 上下文失败: {e}"))
        })
    }

    /// Evaluates a rule's JS snippet with `result` and `baseUrl` bound, returning a `RuleValue`.
    pub fn eval_rule_js(
        &mut self,
        code: &str,
        input: &RuleValue,
        base_url: &str,
    ) -> Result<RuleValue, String> {
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

        let set_input_js = match input {
            RuleValue::Json(v) => format!(
                "globalThis.result = {}; globalThis.baseUrl = {};",
                serde_json::to_string(v).unwrap_or_else(|_| "null".into()),
                serde_json::to_string(base_url).unwrap_or_else(|_| "\"\"".into()),
            ),
            RuleValue::Text(s) => format!(
                "globalThis.result = {}; globalThis.baseUrl = {};",
                serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into()),
                serde_json::to_string(base_url).unwrap_or_else(|_| "\"\"".into()),
            ),
        };

        self.context.with(|ctx| {
            ctx.eval_with_options::<(), _>(set_input_js.as_str(), sloppy_eval_options())
                .catch(&ctx)
                .map_err(|e| format!("设置 JS result/baseUrl 失败: {e}"))?;

            // Wrap in block-friendly evaluation via direct eval (sloppy mode) so `let`/`const` declarations
            // inside separate rule snippets never collide in global scope, while `this === globalThis`!
            let wrapped = format!(
                "(function() {{ return eval({}); }}).call(globalThis)",
                serde_json::to_string(code).unwrap_or_else(|_| "\"\"".into())
            );
            let val: Value<'_> = ctx
                .eval_with_options(wrapped.as_str(), sloppy_eval_options())
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
        self.reset_deadline();
        let login_json = serde_json::to_string(login_info).unwrap_or_else(|_| "{}".to_string());
        self.context.with(|ctx| {
            let init_res = format!("globalThis.result = {login_json};");
            ctx.eval_with_options::<(), _>(init_res.as_str(), sloppy_eval_options())
                .catch(&ctx)
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

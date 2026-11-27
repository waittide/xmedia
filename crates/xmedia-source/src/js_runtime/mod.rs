mod bootstrap;
mod bridge;
mod convert;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use rquickjs::{CatchResultExt, Context, Function, Object, Runtime, Value};

use crate::http::{HttpFetchResult, LegadoHttpClient};
use crate::model::{ExecutionDiagnostics, SourceMetadata, SourceRuntimeState};
use crate::rule::{JsRuleEvaluator, RuleValue, eval_rule_list, eval_rule_string, rule_needs_js};

use self::convert::{js_value_to_rule_value, json_to_js_value, sloppy_eval_options};

pub const DEFAULT_JS_MEMORY_LIMIT_BYTES: usize = 64 * 1024 * 1024;
// Keep QuickJS stack cap well below Linux's default 2 MiB Rust/Tokio worker thread stack
// so deep JS recursion raises a catchable InternalError instead of triggering OS SIGSEGV.
pub const DEFAULT_JS_MAX_STACK_BYTES: usize = 384 * 1024;

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

#[derive(Debug, Clone)]
pub struct JsSandboxConfig {
    pub memory_limit_bytes: usize,
    pub max_stack_bytes: usize,
    pub eval_timeout: Duration,
    pub max_http_requests: usize,
    pub max_temp_vars_entries: usize,
    pub max_temp_vars_bytes: usize,
    pub max_memory_cache_entries: usize,
    pub max_memory_cache_bytes: usize,
    pub max_persistent_cache_entries: usize,
    pub max_persistent_cache_bytes: usize,
    pub max_variable_bytes: usize,
    pub max_login_info_entries: usize,
    pub max_login_info_bytes: usize,
}

impl Default for JsSandboxConfig {
    fn default() -> Self {
        Self {
            memory_limit_bytes: DEFAULT_JS_MEMORY_LIMIT_BYTES,
            max_stack_bytes: DEFAULT_JS_MAX_STACK_BYTES,
            eval_timeout: Duration::from_secs(45),
            max_http_requests: MAX_JS_HTTP_REQUESTS,
            max_temp_vars_entries: MAX_TEMP_VARS_ENTRIES,
            max_temp_vars_bytes: MAX_TEMP_VARS_BYTES,
            max_memory_cache_entries: MAX_MEMORY_CACHE_ENTRIES,
            max_memory_cache_bytes: MAX_MEMORY_CACHE_BYTES,
            max_persistent_cache_entries: MAX_PERSISTENT_CACHE_ENTRIES,
            max_persistent_cache_bytes: MAX_PERSISTENT_CACHE_BYTES,
            max_variable_bytes: MAX_VARIABLE_BYTES,
            max_login_info_entries: MAX_LOGIN_INFO_ENTRIES,
            max_login_info_bytes: MAX_LOGIN_INFO_BYTES,
        }
    }
}

impl JsSandboxConfig {
    pub fn with_timeout(eval_timeout: Duration) -> Self {
        Self {
            eval_timeout,
            ..Self::default()
        }
    }
}

static PROCESS_EPOCH: LazyLock<Instant> = LazyLock::new(Instant::now);

#[inline]
pub fn now_mono_ms() -> u64 {
    PROCESS_EPOCH.elapsed().as_millis() as u64
}

#[derive(Clone)]
pub(super) struct SharedBridgeState {
    pub(super) http: LegadoHttpClient,
    pub(super) runtime_state: Arc<Mutex<SourceRuntimeState>>,
    pub(super) temp_vars: Arc<Mutex<HashMap<String, String>>>,
    pub(super) memory_cache: Arc<Mutex<HashMap<String, String>>>,
    pub(super) diagnostics: Arc<Mutex<ExecutionDiagnostics>>,
    pub(super) current_result: Arc<Mutex<RuleValue>>,
    pub(super) current_base_url: Arc<Mutex<String>>,
    pub(super) source_name: String,
    pub(super) source_url: String,
    pub(super) header_raw: String,
    pub(super) login_url_raw: String,
    pub(super) login_ui_raw: String,
    pub(super) raw_json_str: String,
    pub(super) op_deadline_ms: Arc<AtomicU64>,
    pub(super) cancel_flag: Arc<AtomicBool>,
    pub(super) http_req_count: Arc<AtomicUsize>,
    pub(super) sandbox: JsSandboxConfig,
}

pub(super) fn format_caught_js_error(prefix: &str, err: rquickjs::CaughtError<'_>) -> String {
    match err {
        rquickjs::CaughtError::Exception(ex) => {
            let name = ex
                .as_object()
                .get::<_, Option<String>>("name")
                .ok()
                .flatten()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "Error".to_string());
            let msg = ex.message().unwrap_or_default();
            let stack = ex.stack().unwrap_or_default();
            if stack.trim().is_empty() {
                format!("{prefix}: {name}: {msg}")
            } else {
                format!("{prefix}: {name}: {msg}\n{stack}")
            }
        }
        other => format!("{prefix}: {other}"),
    }
}

/// Reusable QuickJS sandbox session for one source operation (compiles `jsLib` once).
pub struct JsExecutionContext {
    _runtime: Runtime,
    context: Context,
    eval_deadline_ms: Arc<AtomicU64>,
    op_deadline_ms: Arc<AtomicU64>,
    cancel_flag: Arc<AtomicBool>,
    timeout: Duration,
    bridge: SharedBridgeState,
}

impl JsExecutionContext {
    pub fn new(
        source: &SourceMetadata,
        http: LegadoHttpClient,
        runtime_state: Arc<Mutex<SourceRuntimeState>>,
        diagnostics: Arc<Mutex<ExecutionDiagnostics>>,
        config: JsSandboxConfig,
    ) -> Result<Self, String> {
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let op_deadline_ms = Arc::new(AtomicU64::new(
            now_mono_ms().saturating_add(config.eval_timeout.as_millis() as u64),
        ));
        Self::new_with_control(
            source,
            http,
            runtime_state,
            diagnostics,
            config,
            op_deadline_ms,
            cancel_flag,
        )
    }

    pub fn new_with_control(
        source: &SourceMetadata,
        http: LegadoHttpClient,
        runtime_state: Arc<Mutex<SourceRuntimeState>>,
        diagnostics: Arc<Mutex<ExecutionDiagnostics>>,
        config: JsSandboxConfig,
        op_deadline_ms: Arc<AtomicU64>,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<Self, String> {
        let runtime = Runtime::new().map_err(|e| format!("初始化 QuickJS 运行时失败: {e}"))?;
        runtime.set_memory_limit(config.memory_limit_bytes);
        runtime.set_max_stack_size(config.max_stack_bytes);

        let timeout = config.eval_timeout;
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
            current_base_url: Arc::new(Mutex::new(source.source_url.clone())),
            source_name: source.source_name.clone(),
            source_url: source.source_url.clone(),
            header_raw: source.header.clone().unwrap_or_default(),
            login_url_raw: source.login_url.clone().unwrap_or_default(),
            login_ui_raw: source.login_ui.clone().unwrap_or_default(),
            raw_json_str: source.raw_json.clone().unwrap_or_default(),
            op_deadline_ms: op_deadline_ms.clone(),
            cancel_flag: cancel_flag.clone(),
            http_req_count: Arc::new(AtomicUsize::new(0)),
            sandbox: config,
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
                    .map_err(|e| format_caught_js_error("加载数据源 jsLib 失败", e))
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
    /// across independent chapters/items in batch operations.
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
        let eval_deadline_ms = self.eval_deadline_ms.clone();
        self.context.with(|ctx| -> Result<(), String> {
            bridge::install_native_bridges(&ctx, &b, &eval_deadline_ms)?;
            bootstrap::install_bootstrap_globals(&ctx, &b)
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

    /// Replaces `globalThis[name]` with any serializable JSON value.
    pub fn set_json_global(&mut self, name: &str, value: &serde_json::Value) -> Result<(), String> {
        self.reset_deadline();
        self.context.with(|ctx| {
            let js_val = json_to_js_value(&ctx, value)?;
            ctx.globals()
                .set(name, js_val)
                .map_err(|e| format!("注入全局对象 '{name}' 失败: {e}"))
        })
    }

    /// Merges object fields into `globalThis[name]` (preserving existing JS methods on that object).
    pub fn merge_object_global(
        &mut self,
        name: &str,
        fields: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<(), String> {
        self.reset_deadline();
        self.context.with(|ctx| {
            let globals = ctx.globals();
            let obj: Object<'_> = match globals.get::<_, Value<'_>>(name) {
                Ok(v) if v.is_object() => v
                    .into_object()
                    .ok_or_else(|| format!("全局对象 '{name}' 转换失败"))?,
                _ => Object::new(ctx.clone())
                    .map_err(|e| format!("创建全局对象 '{name}' 失败: {e}"))?,
            };
            for (k, v) in fields {
                let js_v = json_to_js_value(&ctx, v)?;
                obj.set(k.as_str(), js_v)
                    .map_err(|e| format!("写入 '{name}.{k}' 失败: {e}"))?;
            }
            globals
                .set(name, obj)
                .map_err(|e| format!("设置全局对象 '{name}' 失败: {e}"))
        })
    }

    /// Batch-injects domain globals (e.g. `[("comic", val), ("chapter", val)]`).
    pub fn set_domain_globals<'a, I>(&mut self, globals: I) -> Result<(), String>
    where
        I: IntoIterator<Item = (&'a str, serde_json::Value)>,
    {
        for (name, value) in globals {
            self.set_json_global(name, &value)?;
        }
        Ok(())
    }

    /// Runs a domain-specific sloppy-mode bootstrap snippet once (e.g. installing `book.*` methods).
    pub fn eval_bootstrap_snippet(&mut self, code: &str) -> Result<(), String> {
        if code.trim().is_empty() {
            return Ok(());
        }
        self.check_active()?;
        self.reset_deadline();
        self.context.with(|ctx| {
            ctx.eval_with_options::<(), _>(code, sloppy_eval_options())
                .catch(&ctx)
                .map_err(|e| format_caught_js_error("执行域 JS 初始化脚本失败", e))
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
                .map_err(|e| format_caught_js_error("执行数据源 JS 规则失败", e))?;

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
                    .map_err(|e| format_caught_js_error("加载数据源 loginUrl 脚本失败", e))?;
            }

            if let Some(act) = action_js
                && !act.trim().is_empty()
            {
                let val: Value<'_> = ctx
                    .eval_with_options(act, sloppy_eval_options())
                    .catch(&ctx)
                    .map_err(|e| {
                        format_caught_js_error(&format!("执行登录动作 '{act}' 失败"), e)
                    })?;
                return js_value_to_rule_value(&ctx, val).map(|rv| rv.as_text());
            }

            Ok(String::new())
        })
    }

    /// Evaluates a Plugin v2 JS module (ESM `export default` or `definePlugin(...)` / CommonJS)
    /// and binds `globalThis.__x_v2_plugin` plus `globalThis.__x_v2_ctx`.
    pub fn load_v2_plugin(
        &mut self,
        js_code: &str,
        settings: &serde_json::Value,
    ) -> Result<(), String> {
        self.check_active()?;
        self.reset_deadline();
        let settings_json = serde_json::to_string(settings).unwrap_or_else(|_| "{}".into());

        let v2_bootstrap = format!(
            r#"
            globalThis.__x_v2_plugin = null;
            globalThis.definePlugin = function(def) {{
                globalThis.__x_v2_plugin = def;
                return def;
            }};
            (function() {{
                var settingsObj = Object.freeze({settings_json});
                function buildQueryString(query) {{
                    if (!query || typeof query !== 'object') return '';
                    var pairs = [];
                    var keys = Object.keys(query);
                    for (var i = 0; i < keys.length; i++) {{
                        var k = keys[i];
                        var v = query[k];
                        if (v === undefined || v === null) continue;
                        pairs.push(encodeURIComponent(k) + '=' + encodeURIComponent(String(v)));
                    }}
                    return pairs.join('&');
                }}
                function appendQuery(url, query) {{
                    var qs = buildQueryString(query);
                    if (!qs) return url;
                    return url + (url.indexOf('?') >= 0 ? '&' : '?') + qs;
                }}
                function findHeaderCaseInsensitive(headers, target) {{
                    var lower = target.toLowerCase();
                    var keys = Object.keys(headers);
                    for (var i = 0; i < keys.length; i++) {{
                        if (keys[i].toLowerCase() === lower) return keys[i];
                    }}
                    return null;
                }}
                var httpApi = {{
                    request: function(opts) {{
                        if (typeof opts === 'string') opts = {{ url: opts }};
                        opts = opts || {{}};
                        var rawUrl = appendQuery(String(opts.url || ''), opts.query);
                        var method = String(opts.method || 'GET').toUpperCase();
                        var headers = Object.assign({{}}, opts.headers || {{}});
                        var bodyStr = undefined;
                        if (opts.form && typeof opts.form === 'object') {{
                            bodyStr = buildQueryString(opts.form);
                            if (!findHeaderCaseInsensitive(headers, 'content-type')) {{
                                headers['Content-Type'] = 'application/x-www-form-urlencoded; charset=UTF-8';
                            }}
                        }} else if (opts.json !== undefined) {{
                            bodyStr = JSON.stringify(opts.json);
                            if (!findHeaderCaseInsensitive(headers, 'content-type')) {{
                                headers['Content-Type'] = 'application/json; charset=UTF-8';
                            }}
                        }} else if (opts.body !== undefined && opts.body !== null) {{
                            bodyStr = typeof opts.body === 'string' ? opts.body : JSON.stringify(opts.body);
                        }}
                        var reqSpec = {{ method: method, headers: headers }};
                        if (bodyStr !== undefined) reqSpec.body = bodyStr;
                        var combined = (method === 'GET' && bodyStr === undefined && Object.keys(headers).length === 0)
                            ? rawUrl
                            : (rawUrl + ',' + JSON.stringify(reqSpec));
                        var resp = JSON.parse(__x_connect(combined));
                        if (resp.error) {{
                            throw new Error(resp.error);
                        }}
                        var status = Number(resp.code || 0);
                        return {{
                            status: status,
                            ok: status >= 200 && status < 300,
                            url: resp.url || rawUrl,
                            headers: resp.headers || {{}},
                            text: resp.body || '',
                            bodyBase64: resp.bodyBase64 || '',
                            dataUrl: resp.dataUrl || '',
                            json: function() {{
                                return JSON.parse(resp.body || 'null');
                            }}
                        }};
                    }},
                    get: function(url, opts) {{
                        return this.request(Object.assign({{}}, opts || {{}}, {{ url: url, method: 'GET' }}));
                    }},
                    post: function(url, bodyOrOpts, opts) {{
                        if (opts === undefined && bodyOrOpts && typeof bodyOrOpts === 'object' &&
                            ('form' in bodyOrOpts || 'json' in bodyOrOpts || 'body' in bodyOrOpts || 'headers' in bodyOrOpts || 'query' in bodyOrOpts)) {{
                            return this.request(Object.assign({{}}, bodyOrOpts, {{ url: url, method: 'POST' }}));
                        }}
                        var merged = Object.assign({{}}, opts || {{}}, {{ url: url, method: 'POST' }});
                        if (typeof bodyOrOpts === 'string') merged.body = bodyOrOpts;
                        else if (bodyOrOpts !== undefined && bodyOrOpts !== null) merged.json = bodyOrOpts;
                        return this.request(merged);
                    }}
                }};
                var cryptoApi = {{
                    md5: function(data, inEnc, outEnc) {{ return __x_crypto_hash('md5', String(data == null ? '' : data), inEnc, outEnc); }},
                    sha1: function(data, inEnc, outEnc) {{ return __x_crypto_hash('sha1', String(data == null ? '' : data), inEnc, outEnc); }},
                    sha256: function(data, inEnc, outEnc) {{ return __x_crypto_hash('sha256', String(data == null ? '' : data), inEnc, outEnc); }},
                    hmacSha1: function(key, data, opts) {{
                        var o = typeof opts === 'string' ? {{ outputEncoding: opts }} : (opts || {{}});
                        return __x_crypto_hmac('sha1', String(key || ''), String(data == null ? '' : data), JSON.stringify(o));
                    }},
                    hmacSha256: function(key, data, opts) {{
                        var o = typeof opts === 'string' ? {{ outputEncoding: opts }} : (opts || {{}});
                        return __x_crypto_hmac('sha256', String(key || ''), String(data == null ? '' : data), JSON.stringify(o));
                    }},
                    aesEncrypt: function(opts) {{ return __x_crypto_aes_enc(JSON.stringify(opts || {{}})); }},
                    aesDecrypt: function(opts) {{ return __x_crypto_aes_dec(JSON.stringify(opts || {{}})); }},
                    rsaEncrypt: function(opts) {{ return __x_crypto_rsa_enc(JSON.stringify(opts || {{}})); }},
                    randomBytes: function(len, outEnc) {{ return __x_crypto_rand(Number(len || 16), outEnc); }},
                    base64Encode: function(s) {{ return __x_b64_enc(String(s == null ? '' : s)); }},
                    base64Decode: function(s) {{ return __x_b64_dec(String(s == null ? '' : s)); }},
                    hexEncode: function(s) {{ return __x_hex_enc(String(s == null ? '' : s)); }},
                    hexDecode: function(s) {{ return __x_hex_dec(String(s == null ? '' : s)); }}
                }};
                var cookiesApi = {{
                    get: function(url, name) {{ return globalThis.cookie.getCookie(url || __srcUrl, name); }},
                    getAll: function(url) {{
                        var raw = globalThis.cookie.getCookie(url || __srcUrl);
                        var map = {{}};
                        if (!raw) return map;
                        var parts = raw.split(';');
                        for (var i = 0; i < parts.length; i++) {{
                            var p = parts[i].trim();
                            var eq = p.indexOf('=');
                            if (eq > 0) map[p.slice(0, eq).trim()] = p.slice(eq + 1).trim();
                        }}
                        return map;
                    }},
                    set: function(url, val) {{ globalThis.cookie.setCookie(url || __srcUrl, val); }},
                    remove: function(url) {{ globalThis.cookie.removeCookie(url || __srcUrl); }},
                    clear: function() {{ __x_cookie_clear_all(); }}
                }};
                var storageApi = {{
                    get: function(k) {{ return globalThis.cache.get(k); }},
                    set: function(k, v) {{ globalThis.cache.put(k, v); }},
                    remove: function(k) {{ globalThis.cache.delete(k); }},
                    getJson: function(k) {{
                        var raw = globalThis.cache.get(k);
                        if (!raw) return null;
                        try {{ return JSON.parse(raw); }} catch (e) {{ return null; }}
                    }},
                    setJson: function(k, v) {{ globalThis.cache.put(k, JSON.stringify(v)); }},
                    clear: function() {{ __x_cache_clear_all(); }}
                }};
                var htmlApi = {{
                    selectText: function(content, rule, baseUrl) {{
                        return __x_select_text(String(content == null ? '' : content), String(rule || ''), baseUrl ? String(baseUrl) : __srcUrl);
                    }},
                    selectList: function(content, rule) {{
                        try {{ return JSON.parse(__x_select_list(String(content == null ? '' : content), String(rule || ''))); }}
                        catch (e) {{ return []; }}
                    }}
                }};
                globalThis.__x_v2_ctx = Object.freeze({{
                    http: Object.freeze(httpApi),
                    crypto: Object.freeze(cryptoApi),
                    cookies: Object.freeze(cookiesApi),
                    storage: Object.freeze(storageApi),
                    settings: settingsObj,
                    html: Object.freeze(htmlApi),
                    log: function() {{ globalThis.console.log.apply(null, arguments); }},
                    toast: function(msg) {{ globalThis.java.toast(msg); }}
                }});
            }})();
            "#
        );

        self.context.with(|ctx| -> Result<(), String> {
            ctx.eval_with_options::<(), _>(v2_bootstrap.as_str(), sloppy_eval_options())
                .catch(&ctx)
                .map_err(|e| format_caught_js_error("初始化 Plugin v2 运行环境失败", e))?;

            let trimmed = js_code.trim().trim_start_matches('\u{feff}');
            let looks_like_esm = trimmed.lines().any(|line| {
                let l = line.trim_start();
                l.starts_with("export ") || l.starts_with("import ")
            });

            if looks_like_esm {
                let declared = rquickjs::Module::declare(ctx.clone(), "plugin.mjs", trimmed)
                    .catch(&ctx)
                    .map_err(|e| format_caught_js_error("编译 ES Module 插件失败", e))?;
                let (module, promise) = declared
                    .eval()
                    .catch(&ctx)
                    .map_err(|e| format_caught_js_error("执行 ES Module 插件失败", e))?;
                while ctx.execute_pending_job() {
                    self.check_active()?;
                }
                promise
                    .finish::<()>()
                    .catch(&ctx)
                    .map_err(|e| format_caught_js_error("完成 ES Module 初始化失败", e))?;
                if let Ok(def_val) = module.get::<_, Value<'_>>("default")
                    && def_val.is_object()
                {
                    ctx.globals()
                        .set("__x_v2_plugin", def_val)
                        .map_err(|e| e.to_string())?;
                }
            } else {
                let wrapped = format!(
                    "var module = {{ exports: {{}} }}; var exports = module.exports;\n{trimmed}\n\
                     if (!globalThis.__x_v2_plugin) {{\n\
                       var __exp = (module.exports && module.exports.default) ? module.exports.default : module.exports;\n\
                       if (__exp && typeof __exp === 'object' && Object.keys(__exp).length > 0) globalThis.__x_v2_plugin = __exp;\n\
                     }}"
                );
                let eval_val: Value<'_> = ctx
                    .eval_with_options(wrapped.as_str(), sloppy_eval_options())
                    .catch(&ctx)
                    .map_err(|e| format_caught_js_error("加载 JS 插件脚本失败", e))?;
                let has_global = ctx
                    .globals()
                    .get::<_, Value<'_>>("__x_v2_plugin")
                    .ok()
                    .is_some_and(|v| v.is_object());
                if !has_global && eval_val.is_object() {
                    ctx.globals()
                        .set("__x_v2_plugin", eval_val)
                        .map_err(|e| e.to_string())?;
                }
            }

            let plugin_val: Value<'_> = ctx
                .globals()
                .get("__x_v2_plugin")
                .map_err(|e| e.to_string())?;
            if !plugin_val.is_object() {
                return Err(
                    "JS 插件未导出有效的插件定义对象（请使用 export default definePlugin({ ... })）"
                        .into(),
                );
            }
            Ok(())
        })
    }

    /// Evaluates a JS expression against `globalThis.__x_v2_plugin` and returns JSON.
    pub fn eval_v2_json(&mut self, expr: &str) -> Result<serde_json::Value, String> {
        self.check_active()?;
        self.reset_deadline();
        self.context.with(|ctx| {
            let val: Value<'_> = ctx
                .eval_with_options(expr, sloppy_eval_options())
                .catch(&ctx)
                .map_err(|e| format_caught_js_error("读取插件元数据失败", e))?;
            js_value_to_json_strict(&ctx, val)
        })
    }

    /// Calls a method path (e.g. `&["search"]` or `&["auth", "qrCode", "create"]`) on
    /// `globalThis.__x_v2_plugin`, passing `(ctx, ...args)`, awaiting any returned Promise,
    /// and returning the resolved JSON value.
    pub fn call_v2_method(
        &mut self,
        method_path: &[&str],
        args: &[serde_json::Value],
    ) -> Result<serde_json::Value, String> {
        self.check_active()?;
        self.reset_deadline();
        let path_json = serde_json::to_string(method_path).map_err(|e| e.to_string())?;
        let args_json = serde_json::to_string(args).map_err(|e| e.to_string())?;

        let invoke_script = format!(
            r#"
            (function() {{
                globalThis.__x_v2_done = false;
                globalThis.__x_v2_res = null;
                globalThis.__x_v2_err = null;
                var path = {path_json};
                var extraArgs = {args_json};
                var owner = null;
                var target = globalThis.__x_v2_plugin;
                for (var i = 0; i < path.length; i++) {{
                    if (target == null) break;
                    owner = target;
                    target = target[path[i]];
                }}
                if (typeof target !== 'function') {{
                    throw new Error('插件未实现方法: ' + path.join('.'));
                }}
                var callArgs = [globalThis.__x_v2_ctx].concat(extraArgs);
                var out = target.apply(owner, callArgs);
                if (out && (typeof out === 'object' || typeof out === 'function') && typeof out.then === 'function') {{
                    out.then(
                        function(v) {{ globalThis.__x_v2_done = true; globalThis.__x_v2_res = (v === undefined ? null : v); }},
                        function(e) {{
                            globalThis.__x_v2_done = true;
                            globalThis.__x_v2_err = e instanceof Error ? (e.message + (e.stack ? '\n' + e.stack : '')) : String(e);
                        }}
                    );
                }} else {{
                    globalThis.__x_v2_done = true;
                    globalThis.__x_v2_res = (out === undefined ? null : out);
                }}
            }})();
            "#
        );

        self.context
            .with(|ctx| -> Result<serde_json::Value, String> {
                ctx.eval_with_options::<(), _>(invoke_script.as_str(), sloppy_eval_options())
                    .catch(&ctx)
                    .map_err(|e| {
                        format_caught_js_error(
                            &format!("调用插件方法 {} 失败", method_path.join(".")),
                            e,
                        )
                    })?;

                while ctx.execute_pending_job() {
                    self.check_active()?;
                }

                let err_opt: Option<String> = ctx.globals().get("__x_v2_err").ok().flatten();
                if let Some(err_msg) = err_opt {
                    return Err(format!(
                        "调用插件方法 {} 失败: {err_msg}",
                        method_path.join(".")
                    ));
                }

                let done: bool = ctx.globals().get("__x_v2_done").unwrap_or(false);
                if !done {
                    return Err(format!(
                        "插件方法 {} 返回了未决的 Promise（不支持外部计时器挂起）",
                        method_path.join(".")
                    ));
                }

                let res_val: Value<'_> =
                    ctx.globals().get("__x_v2_res").map_err(|e| e.to_string())?;
                js_value_to_json_strict(&ctx, res_val)
            })
    }
}

fn js_value_to_json_strict<'js>(
    ctx: &rquickjs::Ctx<'js>,
    val: Value<'js>,
) -> Result<serde_json::Value, String> {
    if val.is_null() || val.is_undefined() {
        return Ok(serde_json::Value::Null);
    }
    if let Some(b) = val.as_bool() {
        return Ok(serde_json::Value::Bool(b));
    }
    if let Some(i) = val.as_int() {
        return Ok(serde_json::Value::from(i));
    }
    if let Some(f) = val.as_float() {
        return Ok(serde_json::Number::from_f64(f)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null));
    }
    if val.is_string() {
        let s: String = val.get().map_err(|e| e.to_string())?;
        return Ok(serde_json::Value::String(s));
    }
    match js_value_to_rule_value(ctx, val)? {
        RuleValue::Json(v) => Ok(v),
        RuleValue::Text(s) => Ok(serde_json::Value::String(s)),
    }
}

impl JsRuleEvaluator for JsExecutionContext {
    fn eval_rule_js(
        &mut self,
        code: &str,
        input: &RuleValue,
        base_url: &str,
    ) -> Result<RuleValue, String> {
        JsExecutionContext::eval_rule_js(self, code, input, base_url)
    }

    fn check_active(&self) -> Result<(), String> {
        JsExecutionContext::check_active(self)
    }
}

/// Lazy-VM, thread-confined reusable session for all domain engines (book, comic, music, video).
pub struct ReusableSourceSession {
    metadata: SourceMetadata,
    http: LegadoHttpClient,
    state_arc: Arc<Mutex<SourceRuntimeState>>,
    cookies_arc: Arc<Mutex<HashMap<String, String>>>,
    diag_arc: Arc<Mutex<ExecutionDiagnostics>>,
    config: JsSandboxConfig,
    op_timeout: Duration,
    op_deadline_ms: Arc<AtomicU64>,
    cancel_flag: Arc<AtomicBool>,
    js: Option<JsExecutionContext>,
    bootstrap_snippets: Vec<String>,
    pending_search: Option<(String, u32)>,
    pending_domain_globals: Vec<(String, serde_json::Value, bool)>,
}

impl ReusableSourceSession {
    pub fn new(
        metadata: SourceMetadata,
        http: LegadoHttpClient,
        state_arc: Arc<Mutex<SourceRuntimeState>>,
        cookies_arc: Arc<Mutex<HashMap<String, String>>>,
        config: JsSandboxConfig,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Self {
        let op_deadline_ms = Arc::new(AtomicU64::new(
            now_mono_ms().saturating_add(op_timeout.as_millis() as u64),
        ));
        Self {
            metadata,
            http,
            state_arc,
            cookies_arc,
            diag_arc: Arc::new(Mutex::new(ExecutionDiagnostics::default())),
            config,
            op_timeout,
            op_deadline_ms,
            cancel_flag,
            js: None,
            bootstrap_snippets: Vec::new(),
            pending_search: None,
            pending_domain_globals: Vec::new(),
        }
    }

    pub fn fetch(&self, target_url: &str, base_url: &str) -> Result<HttpFetchResult, String> {
        self.check_active()?;
        let remaining = self.remaining_budget();
        if remaining.is_zero() {
            return Err("执行超过单次操作总超时上限 (interrupted)".into());
        }
        self.http
            .fetch_with_timeout(target_url, base_url, Some(remaining))
    }

    pub fn fetch_resilient(
        &self,
        target_url: &str,
        base_url: &str,
        max_retries: usize,
    ) -> Result<HttpFetchResult, String> {
        self.check_active()?;
        let remaining = self.remaining_budget();
        if remaining.is_zero() {
            return Err("执行超过单次操作总超时上限 (interrupted)".into());
        }
        self.http.fetch_with_timeout_resilient(
            target_url,
            base_url,
            Some(remaining),
            max_retries,
            Some(&self.cancel_flag),
        )
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

    pub fn remaining_budget(&self) -> Duration {
        let deadline = self.op_deadline_ms.load(Ordering::Relaxed);
        Duration::from_millis(deadline.saturating_sub(now_mono_ms()))
    }

    pub fn reset_operation_budget(&mut self, op_timeout: Duration) {
        self.op_timeout = op_timeout;
        self.op_deadline_ms.store(
            now_mono_ms().saturating_add(op_timeout.as_millis() as u64),
            Ordering::Relaxed,
        );
        if let Some(ref js) = self.js {
            js.reset_operation_budget(op_timeout);
        }
    }

    pub fn add_bootstrap_snippet(&mut self, code: impl Into<String>) {
        let snippet = code.into();
        if !snippet.trim().is_empty() {
            self.bootstrap_snippets.push(snippet);
        }
    }

    pub fn set_search_vars(&mut self, keyword: &str, page: u32) -> Result<(), String> {
        self.pending_search = Some((keyword.to_string(), page));
        if let Some(ref mut js) = self.js {
            js.set_search_vars(keyword, page)?;
        }
        Ok(())
    }

    pub fn set_domain_global(
        &mut self,
        name: impl Into<String>,
        value: serde_json::Value,
        merge: bool,
    ) -> Result<(), String> {
        let key = name.into();
        if let Some((_, existing_val, existing_merge)) = self
            .pending_domain_globals
            .iter_mut()
            .find(|(n, _, _)| n == &key)
        {
            if merge
                && let (serde_json::Value::Object(dst), serde_json::Value::Object(src)) =
                    (existing_val, &value)
            {
                for (k, v) in src {
                    dst.insert(k.clone(), v.clone());
                }
                *existing_merge = *existing_merge || merge;
            } else {
                *self
                    .pending_domain_globals
                    .iter_mut()
                    .find(|(n, _, _)| n == &key)
                    .expect("found above") = (key.clone(), value.clone(), merge);
            }
        } else {
            self.pending_domain_globals
                .push((key.clone(), value.clone(), merge));
        }

        if let Some(ref mut js) = self.js {
            if merge && let serde_json::Value::Object(ref map) = value {
                js.merge_object_global(&key, map)?;
            } else {
                js.set_json_global(&key, &value)?;
            }
        }
        Ok(())
    }

    pub fn set_domain_globals<'a, I>(&mut self, globals: I) -> Result<(), String>
    where
        I: IntoIterator<Item = (&'a str, serde_json::Value)>,
    {
        for (name, val) in globals {
            self.set_domain_global(name, val, false)?;
        }
        Ok(())
    }

    pub fn ensure_js(&mut self) -> Result<&mut JsExecutionContext, String> {
        self.check_active()?;
        if self.js.is_none() {
            let mut js = JsExecutionContext::new_with_control(
                &self.metadata,
                self.http.clone(),
                self.state_arc.clone(),
                self.diag_arc.clone(),
                self.config.clone(),
                self.op_deadline_ms.clone(),
                self.cancel_flag.clone(),
            )?;
            for snippet in &self.bootstrap_snippets {
                js.eval_bootstrap_snippet(snippet)?;
            }
            if let Some((ref kw, page)) = self.pending_search {
                js.set_search_vars(kw, page)?;
            }
            for (name, val, merge) in &self.pending_domain_globals {
                if *merge && let serde_json::Value::Object(map) = val {
                    js.merge_object_global(name, map)?;
                } else {
                    js.set_json_global(name, val)?;
                }
            }
            self.js = Some(js);
        }
        Ok(self.js.as_mut().expect("js session initialized"))
    }

    pub fn eval_string(
        &mut self,
        input: &RuleValue,
        rule: &str,
        base_url: &str,
    ) -> Result<String, String> {
        self.check_active()?;
        if rule_needs_js(rule) {
            let js = self.ensure_js()?;
            eval_rule_string(input, rule, base_url, Some(js))
        } else {
            eval_rule_string(input, rule, base_url, None)
        }
    }

    pub fn eval_list(
        &mut self,
        input: &RuleValue,
        rule: &str,
        base_url: &str,
    ) -> Result<Vec<RuleValue>, String> {
        self.check_active()?;
        if rule_needs_js(rule) {
            let js = self.ensure_js()?;
            eval_rule_list(input, rule, base_url, Some(js))
        } else {
            eval_rule_list(input, rule, base_url, None)
        }
    }

    pub fn eval_opt(
        &mut self,
        input: &RuleValue,
        rule_opt: Option<&str>,
        base_url: &str,
    ) -> String {
        let Some(rule) = rule_opt.map(str::trim).filter(|s| !s.is_empty()) else {
            return String::new();
        };
        match self.eval_string(input, rule, base_url) {
            Ok(val) => val.trim().to_string(),
            Err(err) => {
                if rule_needs_js(rule) {
                    let mut g = self.diag_arc.lock().unwrap_or_else(|e| e.into_inner());
                    if g.logs.len() < 100 {
                        g.logs.push(format!("字段规则求值失败: {err}"));
                    }
                }
                String::new()
            }
        }
    }

    pub fn eval_login_script(
        &mut self,
        login_url_js: &str,
        action_js: Option<&str>,
        login_info: &HashMap<String, String>,
    ) -> Result<String, String> {
        self.ensure_js()?
            .eval_login_script(login_url_js, action_js, login_info)
    }

    pub fn snapshot_diagnostics(&self) -> ExecutionDiagnostics {
        self.diag_arc
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn push_diagnostic_log(&self, msg: impl Into<String>) {
        let mut g = self.diag_arc.lock().unwrap_or_else(|e| e.into_inner());
        if g.logs.len() < 100 {
            g.logs.push(msg.into());
        }
    }

    pub fn finalize_state(&self) -> (SourceRuntimeState, ExecutionDiagnostics) {
        let mut st = self
            .state_arc
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        st.cookies = self
            .cookies_arc
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let diag = self.snapshot_diagnostics();
        (st, diag)
    }
}

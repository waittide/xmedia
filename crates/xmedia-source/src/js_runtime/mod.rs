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

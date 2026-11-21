mod bootstrap;
mod bridge;
mod convert;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use rquickjs::{CatchResultExt, Context, Function, Object, Runtime, Value};

use crate::source::http::LegadoHttpClient;
use crate::source::model::{
    BookJsContext, ChapterJsContext, ExecutionDiagnostics, LegadoBookSource, SourceRuntimeState,
};
use crate::source::rule::RuleValue;

use self::convert::{js_value_to_rule_value, json_to_js_value, sloppy_eval_options};

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
    pub(super) login_ui_raw: String,
    pub(super) op_deadline_ms: Arc<AtomicU64>,
    pub(super) cancel_flag: Arc<AtomicBool>,
    pub(super) http_req_count: Arc<AtomicUsize>,
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

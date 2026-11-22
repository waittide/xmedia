use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::engine::{apply_content_replace_regex, clean_chapter_html_to_text};
use crate::source::http::{HttpFetchResult, LegadoHttpClient, resolve_relative_url};
use crate::source::js_runtime::{JsSandboxConfig, ReusableSourceSession};
use crate::source::model::{
    BookJsContext, ChapterJsContext, ExecutionDiagnostics, LegadoBookSource, OnlineChapterContent,
    OnlineChapterItem, SourceMetadata, SourceRuntimeState,
};
use crate::source::rule::RuleValue;

pub(super) const BOOK_JS_BOOTSTRAP: &str = r#"
globalThis.book = {
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
    readConfig: { useReplaceRule: false },
    _vars: {},
    getVariable: function(k) { return this._vars[String(k)] || ''; },
    putVariable: function(k, v) { this._vars[String(k)] = v == null ? '' : String(v); },
    setUseReplaceRule: function(v) { this.readConfig.useReplaceRule = Boolean(v); }
};
Object.defineProperty(globalThis.book, '_customVariable', {
    set: function(v) { this._vars.custom = v == null ? '' : String(v); },
    enumerable: false,
    configurable: true
});
globalThis.chapter = {
    index: 0,
    title: '',
    url: ''
};
"#;

/// Shared per-source context for batch operations (e.g. multi-chapter downloads).
/// Reuses parsed `LegadoBookSource`, pooled `LegadoHttpClient`, cookies, and runtime state.
#[derive(Clone)]
pub struct ReusableSourceContext {
    pub(super) source: Arc<LegadoBookSource>,
    pub(super) http: LegadoHttpClient,
    pub(super) state_arc: Arc<Mutex<SourceRuntimeState>>,
    pub(super) cookies_arc: Arc<Mutex<HashMap<String, String>>>,
    pub(super) js_timeout: Duration,
    pub(super) cancel_flag: Arc<AtomicBool>,
}

impl ReusableSourceContext {
    pub fn source(&self) -> &LegadoBookSource {
        &self.source
    }

    pub fn snapshot_state(&self) -> SourceRuntimeState {
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
        st
    }

    /// Creates a thread-local worker session that lazily initializes QuickJS at most once per worker.
    pub fn create_worker_session(&self) -> ReusableWorkerSession {
        Self::new_worker_session(
            Arc::clone(&self.source),
            self.http.clone(),
            self.state_arc.clone(),
            self.cookies_arc.clone(),
            self.js_timeout,
            self.js_timeout,
            self.cancel_flag.clone(),
        )
    }

    pub(super) fn new_worker_session(
        source: Arc<LegadoBookSource>,
        http: LegadoHttpClient,
        state_arc: Arc<Mutex<SourceRuntimeState>>,
        cookies_arc: Arc<Mutex<HashMap<String, String>>>,
        js_timeout: Duration,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> ReusableWorkerSession {
        let metadata = SourceMetadata::from(source.as_ref());
        let mut inner = ReusableSourceSession::new(
            metadata,
            http,
            state_arc,
            cookies_arc,
            JsSandboxConfig::with_timeout(js_timeout),
            op_timeout,
            cancel_flag,
        );
        inner.add_bootstrap_snippet(BOOK_JS_BOOTSTRAP);
        ReusableWorkerSession {
            source,
            inner,
            js_timeout,
        }
    }

    pub fn fetch_toc_sync(
        &self,
        toc_url: &str,
        book_ctx: &BookJsContext,
    ) -> Result<Vec<OnlineChapterItem>, String> {
        let mut worker = self.create_worker_session();
        worker.fetch_toc(toc_url, book_ctx)
    }
}

/// Thread-confined worker session (owns lazy QuickJS VM if any rule requires JS).
pub struct ReusableWorkerSession {
    pub(super) source: Arc<LegadoBookSource>,
    pub(super) inner: ReusableSourceSession,
    pub(super) js_timeout: Duration,
}

impl ReusableWorkerSession {
    /// Resets per-chapter deadline and JS HTTP request quota when a download worker
    /// starts processing the next independent chapter.
    pub fn reset_chapter_budget(&mut self) {
        self.inner.reset_operation_budget(self.js_timeout);
    }

    pub(super) fn check_active(&self) -> Result<(), String> {
        self.inner.check_active()
    }

    pub(super) fn fetch_http(
        &self,
        target_url: &str,
        base_url: &str,
    ) -> Result<HttpFetchResult, String> {
        self.check_active()?;
        let remaining = self.inner.remaining_budget();
        if remaining.is_zero() {
            return Err("执行超过单次操作总超时上限 (interrupted)".into());
        }
        self.inner
            .http()
            .fetch_with_timeout(target_url, base_url, Some(remaining))
    }

    pub(super) fn snapshot_diagnostics(&self) -> ExecutionDiagnostics {
        self.inner.snapshot_diagnostics()
    }

    pub(super) fn finalize_state(&self) -> (SourceRuntimeState, ExecutionDiagnostics) {
        self.inner.finalize_state()
    }

    pub(super) fn set_search_vars(&mut self, keyword: &str, page: u32) -> Result<(), String> {
        self.inner.set_search_vars(keyword, page)
    }

    pub(super) fn set_book_context(&mut self, book: &BookJsContext) -> Result<(), String> {
        let payload = serde_json::json!({
            "name": book.name,
            "author": book.author,
            "bookUrl": book.book_url,
            "tocUrl": book.toc_url,
            "coverUrl": book.cover_url.as_deref().unwrap_or(""),
            "abstract": book.intro.as_deref().unwrap_or(""),
            "durChapterIndex": book.dur_chapter_index,
            "durChapterTitle": book.dur_chapter_title,
            "_customVariable": book.custom_variable,
        });
        self.inner.set_domain_global("book", payload, true)
    }

    fn set_chapter_context(&mut self, chapter: &ChapterJsContext) -> Result<(), String> {
        let payload = serde_json::json!({
            "index": chapter.index,
            "title": chapter.title,
            "url": chapter.url,
        });
        self.inner.set_domain_global("chapter", payload, true)
    }

    pub(super) fn eval_string(
        &mut self,
        input: &RuleValue,
        rule: &str,
        base_url: &str,
    ) -> Result<String, String> {
        self.inner.eval_string(input, rule, base_url)
    }

    pub(super) fn eval_list(
        &mut self,
        input: &RuleValue,
        rule: &str,
        base_url: &str,
    ) -> Result<Vec<RuleValue>, String> {
        self.inner.eval_list(input, rule, base_url)
    }

    pub(super) fn eval_opt(
        &mut self,
        input: &RuleValue,
        rule_opt: Option<&str>,
        base_url: &str,
    ) -> String {
        self.inner.eval_opt(input, rule_opt, base_url)
    }

    pub fn fetch_toc(
        &mut self,
        toc_url: &str,
        book_ctx: &BookJsContext,
    ) -> Result<Vec<OnlineChapterItem>, String> {
        let rule_toc = self
            .source
            .rule_toc
            .clone()
            .ok_or_else(|| format!("书源「{}」未配置 ruleToc", self.source.book_source_name))?;
        let chapter_list_rule = rule_toc
            .chapter_list
            .as_deref()
            .ok_or_else(|| "ruleToc 缺少 chapterList 规则".to_string())?;

        self.set_book_context(book_ctx)?;

        let mut chapters = Vec::new();
        let mut next_url_opt = Some(toc_url.to_string());
        let mut visited = std::collections::HashSet::new();
        const MAX_TOC_PAGES: usize = 30;

        while let Some(page_url) = next_url_opt.take() {
            self.check_active()?;
            if visited.len() >= MAX_TOC_PAGES || !visited.insert(page_url.clone()) {
                break;
            }
            let fetched = self.fetch_http(&page_url, &self.source.book_source_url)?;
            self.check_active()?;
            let raw_input = RuleValue::from_raw(fetched.body);

            let items = self.eval_list(&raw_input, chapter_list_rule, &fetched.final_url)?;

            for item in items {
                self.check_active()?;
                let title =
                    self.eval_opt(&item, rule_toc.chapter_name.as_deref(), &fetched.final_url);
                let url_raw =
                    self.eval_opt(&item, rule_toc.chapter_url.as_deref(), &fetched.final_url);
                if title.is_empty() {
                    continue;
                }
                let update_time =
                    self.eval_opt(&item, rule_toc.update_time.as_deref(), &fetched.final_url);
                let is_vol_str =
                    self.eval_opt(&item, rule_toc.is_volume.as_deref(), &fetched.final_url);
                let trimmed_url = url_raw.trim();
                let is_volume = matches!(
                    is_vol_str.trim().to_ascii_lowercase().as_str(),
                    "true" | "1"
                ) || trimmed_url.is_empty()
                    || trimmed_url == "#"
                    || trimmed_url.to_ascii_lowercase().starts_with("javascript:");

                let resolved = if url_raw.is_empty() {
                    String::new()
                } else {
                    resolve_relative_url(&url_raw, &fetched.final_url)
                };

                chapters.push(OnlineChapterItem {
                    index: chapters.len(),
                    title,
                    url: resolved,
                    update_time: (!update_time.is_empty()).then_some(update_time),
                    is_volume,
                });
            }

            if let Some(ref next_rule) = rule_toc.next_toc_url
                && !next_rule.trim().is_empty()
            {
                let next_raw = self.eval_opt(&raw_input, Some(next_rule), &fetched.final_url);
                if !next_raw.is_empty() {
                    next_url_opt = Some(resolve_relative_url(&next_raw, &fetched.final_url));
                }
            }
        }

        Ok(chapters)
    }

    pub fn fetch_chapter_content(
        &mut self,
        chapter_url: &str,
        book_ctx: &BookJsContext,
        chapter_ctx: &ChapterJsContext,
    ) -> Result<OnlineChapterContent, String> {
        let rule_content =
            self.source.rule_content.clone().ok_or_else(|| {
                format!("书源「{}」未配置 ruleContent", self.source.book_source_name)
            })?;
        let content_rule = rule_content
            .content
            .as_deref()
            .ok_or_else(|| "ruleContent 缺少 content 规则".to_string())?;

        self.set_book_context(book_ctx)?;
        self.set_chapter_context(chapter_ctx)?;

        let mut pages_html = Vec::new();
        let mut resolved_title = chapter_ctx.title.clone();
        let mut next_url_opt = Some(chapter_url.to_string());
        let mut visited = std::collections::HashSet::new();
        const MAX_CONTENT_PAGES: usize = 20;

        while let Some(page_url) = next_url_opt.take() {
            self.check_active()?;
            if visited.len() >= MAX_CONTENT_PAGES || !visited.insert(page_url.clone()) {
                break;
            }
            let fetched = self.fetch_http(&page_url, &self.source.book_source_url)?;
            self.check_active()?;
            let raw_input = RuleValue::from_raw(fetched.body);

            if pages_html.is_empty()
                && let Some(ref title_rule) = rule_content.title
                && !title_rule.trim().is_empty()
            {
                let t = self.eval_opt(&raw_input, Some(title_rule), &fetched.final_url);
                if !t.is_empty() {
                    resolved_title = t;
                }
            }

            let part = self.eval_string(&raw_input, content_rule, &fetched.final_url)?;
            if !part.trim().is_empty() {
                pages_html.push(part);
            }

            if let Some(ref next_rule) = rule_content.next_content_url
                && !next_rule.trim().is_empty()
            {
                let next_raw = self.eval_opt(&raw_input, Some(next_rule), &fetched.final_url);
                if !next_raw.is_empty() {
                    next_url_opt = Some(resolve_relative_url(&next_raw, &fetched.final_url));
                }
            }
        }

        let mut raw_html = pages_html.join("\n");
        if let Some(ref rep_rule) = rule_content.replace_regex
            && !rep_rule.trim().is_empty()
        {
            raw_html = apply_content_replace_regex(&raw_html, rep_rule);
        }

        let clean_text = clean_chapter_html_to_text(&raw_html);
        Ok(OnlineChapterContent {
            title: resolved_title,
            content: clean_text,
            raw_html,
        })
    }
}

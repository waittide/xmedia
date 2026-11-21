use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::engine::{apply_content_replace_regex, clean_chapter_html_to_text};
use crate::source::http::{LegadoHttpClient, resolve_relative_url};
use crate::source::js_runtime::{LegadoJsSession, now_mono_ms};
use crate::source::model::{
    BookJsContext, ChapterJsContext, ExecutionDiagnostics, LegadoBookSource, OnlineChapterContent,
    OnlineChapterItem, SourceRuntimeState,
};
use crate::source::rule::{RuleValue, eval_rule_list, eval_rule_string, rule_needs_js};

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
        let op_deadline_ms = Arc::new(AtomicU64::new(
            now_mono_ms().saturating_add(self.js_timeout.as_millis() as u64),
        ));
        ReusableWorkerSession {
            source: Arc::clone(&self.source),
            http: self.http.clone(),
            state_arc: self.state_arc.clone(),
            diag_arc: Arc::new(Mutex::new(ExecutionDiagnostics::default())),
            js_timeout: self.js_timeout,
            op_deadline_ms,
            cancel_flag: self.cancel_flag.clone(),
            js: None,
            pending_search: None,
            pending_book: None,
            pending_chapter: None,
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
    pub(super) http: LegadoHttpClient,
    pub(super) state_arc: Arc<Mutex<SourceRuntimeState>>,
    pub(super) diag_arc: Arc<Mutex<ExecutionDiagnostics>>,
    pub(super) js_timeout: Duration,
    pub(super) op_deadline_ms: Arc<AtomicU64>,
    pub(super) cancel_flag: Arc<AtomicBool>,
    pub(super) js: Option<LegadoJsSession>,
    pub(super) pending_search: Option<(String, u32)>,
    pub(super) pending_book: Option<BookJsContext>,
    pub(super) pending_chapter: Option<ChapterJsContext>,
}

impl ReusableWorkerSession {
    /// Resets per-chapter deadline and JS HTTP request quota when a download worker
    /// starts processing the next independent chapter.
    pub fn reset_chapter_budget(&mut self) {
        self.op_deadline_ms.store(
            now_mono_ms().saturating_add(self.js_timeout.as_millis() as u64),
            Ordering::Relaxed,
        );
        if let Some(ref js) = self.js {
            js.reset_operation_budget(self.js_timeout);
        }
    }

    pub(super) fn check_active(&self) -> Result<(), String> {
        if self.cancel_flag.load(Ordering::Relaxed) {
            return Err("执行已取消 (interrupted)".into());
        }
        if now_mono_ms() > self.op_deadline_ms.load(Ordering::Relaxed) {
            return Err("执行超过单次操作总超时上限 (interrupted)".into());
        }
        Ok(())
    }

    pub(super) fn snapshot_diagnostics(&self) -> ExecutionDiagnostics {
        self.diag_arc
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub(super) fn set_search_vars(&mut self, keyword: &str, page: u32) -> Result<(), String> {
        self.pending_search = Some((keyword.to_string(), page));
        if let Some(ref mut js) = self.js {
            js.set_search_vars(keyword, page)?;
        }
        Ok(())
    }

    pub(super) fn set_book_context(&mut self, book: &BookJsContext) -> Result<(), String> {
        self.pending_book = Some(book.clone());
        if let Some(ref mut js) = self.js {
            js.set_book_context(book)?;
        }
        Ok(())
    }

    fn set_chapter_context(&mut self, chapter: &ChapterJsContext) -> Result<(), String> {
        self.pending_chapter = Some(chapter.clone());
        if let Some(ref mut js) = self.js {
            js.set_chapter_context(chapter)?;
        }
        Ok(())
    }

    pub(super) fn ensure_js(&mut self) -> Result<&mut LegadoJsSession, String> {
        self.check_active()?;
        if self.js.is_none() {
            let mut js = LegadoJsSession::new_with_control(
                &self.source,
                self.http.clone(),
                self.state_arc.clone(),
                self.diag_arc.clone(),
                self.js_timeout,
                self.op_deadline_ms.clone(),
                self.cancel_flag.clone(),
            )?;
            if let Some((ref kw, page)) = self.pending_search {
                js.set_search_vars(kw, page)?;
            }
            if let Some(ref book) = self.pending_book {
                js.set_book_context(book)?;
            }
            if let Some(ref ch) = self.pending_chapter {
                js.set_chapter_context(ch)?;
            }
            self.js = Some(js);
        }
        Ok(self.js.as_mut().expect("js session initialized"))
    }

    pub(super) fn eval_string(
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

    pub(super) fn eval_list(
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

    pub(super) fn eval_opt(
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
            let fetched = self.http.fetch(&page_url, &self.source.book_source_url)?;
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
            let fetched = self.http.fetch(&page_url, &self.source.book_source_url)?;
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

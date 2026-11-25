mod html_pages;
mod pagination;

use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use xmedia_source::http::{HttpFetchResult, LegadoHttpClient};
use xmedia_source::js_runtime::{JsSandboxConfig, ReusableSourceSession};
use xmedia_source::rule::RuleValue;

pub(crate) use html_pages::extract_img_srcs_from_html;

use self::html_pages::{
    PageSegment, apply_replace_regex, looks_like_mixed_plain_url, normalize_extracted_urls,
    segment_page_content,
};
use self::pagination::{PaginationCycleGuard, is_placeholder_or_fragment_url};
use super::engine::resolve_relative_url_preserving_options;
use crate::domain::online::{
    ComicChapterJsContext, ComicJsContext, ComicSourceSpec, ExecutionDiagnostics,
    OnlineComicChapter, SourceMetadata, SourceRuntimeState,
};

pub(super) const COMIC_JS_BOOTSTRAP: &str = r#"
globalThis.book = {
    name: '',
    author: '',
    bookUrl: '',
    comicUrl: '',
    tocUrl: '',
    coverUrl: '',
    abstract: '',
    readingDirection: 'rtl',
    durChapterIndex: 0,
    durChapterTitle: '',
    order: 0,
    type: 2,
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
globalThis.comic = globalThis.book;
globalThis.chapter = {
    index: 0,
    title: '',
    url: ''
};
"#;

#[derive(Clone)]
pub struct ReusableComicContext {
    pub(super) source: Arc<ComicSourceSpec>,
    pub(super) http: LegadoHttpClient,
    pub(super) state_arc: Arc<Mutex<SourceRuntimeState>>,
    pub(super) cookies_arc: Arc<Mutex<HashMap<String, String>>>,
    pub(super) js_timeout: Duration,
    pub(super) cancel_flag: Arc<AtomicBool>,
}

impl ReusableComicContext {
    pub fn source(&self) -> &ComicSourceSpec {
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

    pub fn create_worker_session(&self) -> ReusableComicSession {
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
        source: Arc<ComicSourceSpec>,
        http: LegadoHttpClient,
        state_arc: Arc<Mutex<SourceRuntimeState>>,
        cookies_arc: Arc<Mutex<HashMap<String, String>>>,
        js_timeout: Duration,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> ReusableComicSession {
        let metadata = SourceMetadata::from(source.as_ref());
        let mut inner = ReusableSourceSession::new(
            metadata,
            http,
            state_arc,
            cookies_arc,
            JsSandboxConfig::with_timeout(js_timeout),
            op_timeout,
            cancel_flag.clone(),
        );
        inner.add_bootstrap_snippet(COMIC_JS_BOOTSTRAP);
        ReusableComicSession {
            source,
            inner,
            op_timeout,
            op_deadline: Instant::now() + op_timeout,
            cancel_flag,
        }
    }
}

pub struct ReusableComicSession {
    pub(super) source: Arc<ComicSourceSpec>,
    pub(super) inner: ReusableSourceSession,
    pub(super) op_timeout: Duration,
    pub(super) op_deadline: Instant,
    pub(super) cancel_flag: Arc<AtomicBool>,
}

impl ReusableComicSession {
    pub(super) fn check_active(&self) -> Result<(), String> {
        self.inner.check_active()
    }

    pub(super) fn http_fetch(&self, raw_url: &str) -> Result<HttpFetchResult, String> {
        self.check_active()?;
        let remaining = self.op_deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("执行超过单次操作总超时上限 (interrupted)".into());
        }
        const MIN_RETRY_OP_BUDGET: Duration = Duration::from_secs(8);
        const MIN_RETRY_REMAINING: Duration = Duration::from_secs(7);
        if self.op_timeout >= MIN_RETRY_OP_BUDGET && remaining >= MIN_RETRY_REMAINING {
            self.inner.http().fetch_with_timeout_resilient(
                raw_url,
                &self.source.source_url,
                Some(remaining),
                1,
                Some(&self.cancel_flag),
            )
        } else {
            self.inner
                .http()
                .fetch_with_timeout(raw_url, &self.source.source_url, Some(remaining))
        }
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

    pub(super) fn set_comic_context(&mut self, comic: &ComicJsContext) -> Result<(), String> {
        let dir = comic
            .reading_direction
            .as_deref()
            .or(self.source.reading_direction.as_deref())
            .unwrap_or("rtl");
        let payload = serde_json::json!({
            "name": comic.name,
            "author": comic.author,
            "bookUrl": comic.book_url,
            "comicUrl": comic.book_url,
            "tocUrl": comic.toc_url,
            "coverUrl": comic.cover_url.as_deref().unwrap_or(""),
            "abstract": comic.intro.as_deref().unwrap_or(""),
            "readingDirection": dir,
            "durChapterIndex": comic.dur_chapter_index,
            "durChapterTitle": comic.dur_chapter_title,
            "_customVariable": comic.custom_variable,
        });
        self.inner.set_domain_global("book", payload, true)
    }

    fn set_chapter_context(&mut self, chapter: &ComicChapterJsContext) -> Result<(), String> {
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

    pub fn fetch_chapters(
        &mut self,
        toc_url: &str,
        comic_ctx: &ComicJsContext,
    ) -> Result<Vec<OnlineComicChapter>, String> {
        let rule_toc = self
            .source
            .rule_toc
            .clone()
            .ok_or_else(|| format!("漫画源「{}」未配置 ruleToc", self.source.source_name))?;
        let chapter_list_rule = rule_toc
            .chapter_list
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| "ruleToc 缺少 chapterList 规则".to_string())?;

        self.set_comic_context(comic_ctx)?;

        let mut chapters = Vec::new();
        let mut next_url_opt = Some(toc_url.to_string());
        let mut guard = PaginationCycleGuard::default();
        let mut pages_fetched = 0usize;
        const MAX_TOC_PAGES: usize = 30;

        while let Some(page_url) = next_url_opt.take() {
            self.check_active()?;
            if pages_fetched >= MAX_TOC_PAGES {
                break;
            }
            let Some(req_key) = guard.try_begin(&page_url, &self.source.source_url) else {
                break;
            };
            pages_fetched += 1;
            let fetched = match self.http_fetch(&page_url) {
                Ok(f) => f,
                Err(err) => {
                    let remaining = self.op_deadline.saturating_duration_since(Instant::now());
                    if pages_fetched > 1
                        && !chapters.is_empty()
                        && self.op_timeout >= Duration::from_secs(8)
                        && self.check_active().is_ok()
                        && remaining > Duration::from_millis(500)
                    {
                        self.inner.push_diagnostic_log(format!(
                            "fetch_chapters 在第 {pages_fetched} 页发生瞬时失败，保留已获取的 {} 个章节: {err}",
                            chapters.len()
                        ));
                        break;
                    }
                    return Err(err);
                }
            };
            if !guard.record_fetched(&req_key, &fetched.final_url, &fetched.body) {
                break;
            }
            let mut raw_input = RuleValue::from_raw(fetched.body);

            if let Some(ref pre_js) = rule_toc.pre_update_js
                && !pre_js.trim().is_empty()
            {
                let out = self.eval_string(&raw_input, pre_js, &fetched.final_url)?;
                if !out.trim().is_empty() {
                    raw_input = RuleValue::from_raw(out);
                }
            }

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
                let is_placeholder =
                    is_placeholder_or_fragment_url(trimmed_url, &fetched.final_url);
                let is_volume = matches!(
                    is_vol_str.trim().to_ascii_lowercase().as_str(),
                    "true" | "1"
                ) || is_placeholder;

                let resolved = if is_placeholder {
                    String::new()
                } else {
                    resolve_relative_url_preserving_options(trimmed_url, &fetched.final_url)
                };

                chapters.push(OnlineComicChapter {
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
                let next_trimmed = next_raw.trim();
                if !is_placeholder_or_fragment_url(next_trimmed, &fetched.final_url) {
                    let resolved_next =
                        resolve_relative_url_preserving_options(next_trimmed, &fetched.final_url);
                    if guard.can_enqueue(&resolved_next, &self.source.source_url) {
                        next_url_opt = Some(resolved_next);
                    }
                }
            }
        }

        Ok(chapters)
    }

    pub fn fetch_pages(
        &mut self,
        chapter_url: &str,
        comic_ctx: &ComicJsContext,
        chapter_ctx: &ComicChapterJsContext,
    ) -> Result<Vec<String>, String> {
        let rule_pages =
            self.source.rule_pages.clone().ok_or_else(|| {
                format!("漫画源「{}」未配置 ruleContent", self.source.source_name)
            })?;
        let content_rule = rule_pages
            .content
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| "ruleContent 缺少 content/pages 规则".to_string())?;

        self.set_comic_context(comic_ctx)?;
        self.set_chapter_context(chapter_ctx)?;

        let mut pages = Vec::new();
        let mut seen_urls = HashSet::new();
        let mut next_url_opt = Some(chapter_url.to_string());
        let mut guard = PaginationCycleGuard::default();
        let mut pages_fetched = 0usize;
        const MAX_CONTENT_PAGES: usize = 20;

        while let Some(page_url) = next_url_opt.take() {
            self.check_active()?;
            if pages_fetched >= MAX_CONTENT_PAGES {
                break;
            }
            let Some(req_key) = guard.try_begin(&page_url, &self.source.source_url) else {
                break;
            };
            pages_fetched += 1;
            let fetched = self.http_fetch(&page_url)?;
            if !guard.record_fetched(&req_key, &fetched.final_url, &fetched.body) {
                break;
            }
            let raw_input = RuleValue::from_raw(fetched.body);

            let evaluated = self.eval_string(&raw_input, content_rule, &fetched.final_url)?;
            let cleaned = if let Some(ref rep_rule) = rule_pages.replace_regex
                && !rep_rule.trim().is_empty()
            {
                apply_replace_regex(&evaluated, rep_rule)
            } else {
                evaluated
            };

            let extracted = self.extract_page_urls(
                &cleaned,
                rule_pages.image_url.as_deref(),
                &fetched.final_url,
            );
            for url in extracted {
                if seen_urls.insert(url.clone()) {
                    pages.push(url);
                }
            }

            if let Some(ref next_rule) = rule_pages.next_content_url
                && !next_rule.trim().is_empty()
            {
                let next_raw = self.eval_opt(&raw_input, Some(next_rule), &fetched.final_url);
                let next_trimmed = next_raw.trim();
                if !is_placeholder_or_fragment_url(next_trimmed, &fetched.final_url) {
                    let resolved_next =
                        resolve_relative_url_preserving_options(next_trimmed, &fetched.final_url);
                    if guard.can_enqueue(&resolved_next, &self.source.source_url) {
                        next_url_opt = Some(resolved_next);
                    }
                }
            }
        }

        Ok(pages)
    }

    fn extract_page_urls(
        &mut self,
        evaluated: &str,
        image_url_rule: Option<&str>,
        base_url: &str,
    ) -> Vec<String> {
        let trimmed = evaluated.trim();
        if trimmed.is_empty() {
            return Vec::new();
        }

        let mut raw_candidates: Vec<String> = Vec::new();
        let img_rule_opt = image_url_rule.filter(|r| !r.trim().is_empty());

        if trimmed.starts_with('[')
            && let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(trimmed)
        {
            for item in arr {
                if let Some(img_rule) = img_rule_opt {
                    let rv = match item {
                        serde_json::Value::String(s) => RuleValue::from_raw(s),
                        other => RuleValue::Json(other),
                    };
                    let extracted = self.eval_opt(&rv, Some(img_rule), base_url);
                    if !extracted.is_empty() {
                        raw_candidates.push(extracted);
                    }
                } else if let Some(s) = item.as_str() {
                    raw_candidates.push(s.to_string());
                }
            }
            return normalize_extracted_urls(raw_candidates, base_url);
        }

        let (segments, has_html_tokens) = segment_page_content(trimmed);
        let mut html_buf = String::new();

        for seg in segments {
            match seg {
                PageSegment::Html(slice) => {
                    html_buf.push_str(slice);
                }
                PageSegment::TopLevelText(slice) => {
                    if slice.trim().is_empty() {
                        if !html_buf.is_empty() {
                            html_buf.push_str(slice);
                        }
                        continue;
                    }
                    self.flush_html_page_candidates(
                        &mut html_buf,
                        img_rule_opt,
                        base_url,
                        &mut raw_candidates,
                    );
                    for line in slice.lines() {
                        let l = line.trim();
                        if l.is_empty() {
                            continue;
                        }
                        if has_html_tokens && !looks_like_mixed_plain_url(l) {
                            continue;
                        }
                        self.push_page_candidate(
                            l.to_string(),
                            img_rule_opt,
                            base_url,
                            &mut raw_candidates,
                        );
                    }
                }
            }
        }
        self.flush_html_page_candidates(&mut html_buf, img_rule_opt, base_url, &mut raw_candidates);

        normalize_extracted_urls(raw_candidates, base_url)
    }

    fn flush_html_page_candidates(
        &mut self,
        html_buf: &mut String,
        img_rule_opt: Option<&str>,
        base_url: &str,
        out: &mut Vec<String>,
    ) {
        if html_buf.is_empty() {
            return;
        }
        let mut srcs = Vec::new();
        extract_img_srcs_from_html(html_buf, &mut srcs);
        html_buf.clear();
        for src in srcs {
            self.push_page_candidate(src, img_rule_opt, base_url, out);
        }
    }

    fn push_page_candidate(
        &mut self,
        candidate: String,
        img_rule_opt: Option<&str>,
        base_url: &str,
        out: &mut Vec<String>,
    ) {
        if let Some(img_rule) = img_rule_opt {
            let rv = RuleValue::from_raw(candidate);
            let extracted = self.eval_opt(&rv, Some(img_rule), base_url);
            if !extracted.trim().is_empty() {
                out.push(extracted);
            }
        } else {
            out.push(candidate);
        }
    }
}

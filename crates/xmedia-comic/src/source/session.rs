use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use scraper::{Html, Selector};
use xmedia_source::http::{LegadoHttpClient, resolve_relative_url};
use xmedia_source::js_runtime::{JsSandboxConfig, ReusableSourceSession};
use xmedia_source::rule::{RuleValue, cached_regex};

use crate::source::model::{
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
            cancel_flag,
        );
        inner.add_bootstrap_snippet(COMIC_JS_BOOTSTRAP);
        ReusableComicSession { source, inner }
    }
}

pub struct ReusableComicSession {
    pub(super) source: Arc<ComicSourceSpec>,
    pub(super) inner: ReusableSourceSession,
}

impl ReusableComicSession {
    pub(super) fn check_active(&self) -> Result<(), String> {
        self.inner.check_active()
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
        let mut visited = HashSet::new();
        const MAX_TOC_PAGES: usize = 30;

        while let Some(page_url) = next_url_opt.take() {
            self.check_active()?;
            if visited.len() >= MAX_TOC_PAGES || !visited.insert(page_url.clone()) {
                break;
            }
            let fetched = self
                .inner
                .http()
                .fetch(&page_url, &self.source.source_url)?;
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
                let is_volume = matches!(
                    is_vol_str.trim().to_ascii_lowercase().as_str(),
                    "true" | "1"
                ) || trimmed_url.is_empty()
                    || trimmed_url == "#"
                    || trimmed_url.to_ascii_lowercase().starts_with("javascript:");

                let resolved = if trimmed_url.is_empty() || is_volume && trimmed_url == "#" {
                    String::new()
                } else {
                    resolve_relative_url(trimmed_url, &fetched.final_url)
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
                if !next_raw.is_empty() {
                    let resolved_next = resolve_relative_url(&next_raw, &fetched.final_url);
                    if !visited.contains(&resolved_next) {
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
        let mut visited_pages = HashSet::new();
        const MAX_CONTENT_PAGES: usize = 20;

        while let Some(page_url) = next_url_opt.take() {
            self.check_active()?;
            if visited_pages.len() >= MAX_CONTENT_PAGES || !visited_pages.insert(page_url.clone()) {
                break;
            }
            let fetched = self
                .inner
                .http()
                .fetch(&page_url, &self.source.source_url)?;
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
                if !next_raw.is_empty() {
                    let resolved_next = resolve_relative_url(&next_raw, &fetched.final_url);
                    if !visited_pages.contains(&resolved_next) {
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

        // 1. JSON array output (from JS rules or JSONPath arrays serialized as JSON)
        if trimmed.starts_with('[')
            && let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(trimmed)
        {
            for item in arr {
                match item {
                    serde_json::Value::String(s) => raw_candidates.push(s),
                    other => {
                        if let Some(img_rule) = image_url_rule.filter(|r| !r.trim().is_empty()) {
                            let rv = RuleValue::Json(other);
                            let extracted = self.eval_opt(&rv, Some(img_rule), base_url);
                            if !extracted.is_empty() {
                                raw_candidates.push(extracted);
                            }
                        } else if let Some(s) = other.as_str() {
                            raw_candidates.push(s.to_string());
                        }
                    }
                }
            }
            return normalize_extracted_urls(raw_candidates, base_url);
        }

        // 2. Line-by-line / HTML <img> extraction (preserving HTML without stripping!)
        for line in trimmed.lines() {
            let l = line.trim();
            if l.is_empty() {
                continue;
            }
            let lower = l.to_ascii_lowercase();
            if lower.contains("<img") {
                extract_img_srcs_from_html(l, &mut raw_candidates);
            } else if let Some(img_rule) = image_url_rule.filter(|r| !r.trim().is_empty()) {
                let rv = RuleValue::from_raw(l.to_string());
                let extracted = self.eval_opt(&rv, Some(img_rule), base_url);
                if !extracted.is_empty() {
                    raw_candidates.push(extracted);
                }
            } else {
                raw_candidates.push(l.to_string());
            }
        }

        normalize_extracted_urls(raw_candidates, base_url)
    }
}

pub(crate) fn extract_img_srcs_from_html(html_fragment: &str, out: &mut Vec<String>) {
    let Ok(img_sel) = Selector::parse("img") else {
        return;
    };
    let doc = Html::parse_fragment(html_fragment);
    for el in doc.select(&img_sel) {
        let attr = el
            .value()
            .attr("data-url")
            .or_else(|| el.value().attr("data-original"))
            .or_else(|| el.value().attr("data-src"))
            .or_else(|| el.value().attr("src"))
            .map(str::trim)
            .filter(|s| !s.is_empty());
        if let Some(val) = attr {
            out.push(val.to_string());
        }
    }
}

fn normalize_extracted_urls(candidates: Vec<String>, base_url: &str) -> Vec<String> {
    let mut out = Vec::with_capacity(candidates.len());
    for raw in candidates {
        let t = raw.trim();
        if t.is_empty() || t == "#" {
            continue;
        }
        let lower = t.to_ascii_lowercase();
        if lower.starts_with("javascript:")
            || lower.starts_with("about:")
            || lower.starts_with("data:image/svg+xml")
        {
            continue;
        }
        let resolved = resolve_relative_url(t, base_url);
        if !resolved.trim().is_empty() {
            out.push(resolved);
        }
    }
    out
}

pub(super) fn apply_replace_regex(content: &str, replace_rule: &str) -> String {
    let mut out = content.to_string();
    for line in replace_rule.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some((pat, rep)) = trimmed.strip_prefix("##").and_then(|s| s.split_once("##"))
            && let Some(re) = cached_regex(pat)
        {
            out = re.replace_all(&out, rep).into_owned();
        } else if let Some(re) = cached_regex(trimmed) {
            out = re.replace_all(&out, "").into_owned();
        }
    }
    out
}

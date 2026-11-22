use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use scraper::{Html, Selector};
use xmedia_source::http::{
    HttpFetchResult, LegadoHttpClient, LegadoUrlOptions, is_forbidden_custom_header,
    parse_legado_url, resolve_relative_url, split_legado_url_and_raw_options,
};
use xmedia_source::js_runtime::{JsSandboxConfig, ReusableSourceSession};
use xmedia_source::rule::{RuleValue, cached_regex};

use super::engine::resolve_relative_url_preserving_options;
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
        if self.op_timeout >= MIN_RETRY_OP_BUDGET && remaining >= MIN_RETRY_OP_BUDGET {
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

        // 1. JSON array output (from JS rules or JSONPath arrays serialized as JSON)
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

        // 2. HTML5-aware segmenter: coalesce HTML blocks for scraper while preserving verbatim
        // top-level plain URLs and filtering stray top-level prose in mixed HTML mode.
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CanonicalRequestKey {
    url_no_frag: String,
    method: String,
    body: Option<String>,
    headers: Vec<(String, String)>,
    charset: Option<String>,
}

impl CanonicalRequestKey {
    fn from_raw(raw_url: &str, source_url: &str) -> Self {
        let parsed = parse_legado_url(raw_url);
        let resolved = resolve_relative_url(&parsed.url, source_url);
        let url_no_frag = strip_fragment(&resolved).to_string();
        let method = parsed
            .options
            .method
            .as_deref()
            .unwrap_or("GET")
            .trim()
            .to_ascii_uppercase();
        let body = if method != "GET" && method != "HEAD" {
            match parsed.options.body {
                Some(serde_json::Value::String(s)) => Some(s),
                Some(serde_json::Value::Null) | None => None,
                Some(other) => Some(other.to_string()),
            }
        } else {
            None
        };
        let mut headers = Vec::new();
        if let Some(serde_json::Value::Object(map)) = parsed.options.headers {
            for (k, v) in map {
                if is_forbidden_custom_header(&k) {
                    continue;
                }
                let val_str = match v {
                    serde_json::Value::String(s) => s,
                    other => other.to_string(),
                };
                headers.push((k.to_ascii_lowercase(), val_str));
            }
            headers.sort();
        }
        let charset = parsed
            .options
            .charset
            .as_deref()
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty());
        Self {
            url_no_frag,
            method,
            body,
            headers,
            charset,
        }
    }

    fn plain_get(url_no_frag: &str) -> Self {
        Self {
            url_no_frag: url_no_frag.to_string(),
            method: "GET".to_string(),
            body: None,
            headers: Vec::new(),
            charset: None,
        }
    }

    fn is_plain_get_or_head(&self) -> bool {
        matches!(self.method.as_str(), "GET" | "HEAD")
            && self.body.is_none()
            && self.headers.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RedirectTargetKey {
    final_url: String,
    method: String,
    body: Option<String>,
    headers: Vec<(String, String)>,
}

#[derive(Default)]
struct PaginationCycleGuard {
    visited_requests: HashSet<CanonicalRequestKey>,
    visited_get_bodies: HashMap<String, u64>,
    redirected_post_final_urls: HashSet<String>,
    visited_non_get_bodies: HashMap<String, HashSet<u64>>,
    visited_redirect_targets: HashSet<RedirectTargetKey>,
}

fn hash_response_body(body: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    body.hash(&mut hasher);
    hasher.finish()
}

impl PaginationCycleGuard {
    fn try_begin(&mut self, page_url: &str, source_url: &str) -> Option<CanonicalRequestKey> {
        let key = CanonicalRequestKey::from_raw(page_url, source_url);
        self.visited_requests.insert(key.clone()).then_some(key)
    }

    fn record_fetched(
        &mut self,
        req_key: &CanonicalRequestKey,
        final_url: &str,
        body: &str,
    ) -> bool {
        let final_no_frag = strip_fragment(final_url).to_string();
        let body_hash = hash_response_body(body);
        if req_key.is_plain_get_or_head() {
            if self.visited_get_bodies.contains_key(&final_no_frag)
                || self
                    .visited_non_get_bodies
                    .get(&final_no_frag)
                    .is_some_and(|hashes| hashes.contains(&body_hash))
            {
                return false;
            }
            self.visited_get_bodies
                .insert(final_no_frag.clone(), body_hash);
            if final_no_frag != req_key.url_no_frag {
                self.visited_requests
                    .insert(CanonicalRequestKey::plain_get(&final_no_frag));
            }
            true
        } else {
            let target_key = RedirectTargetKey {
                final_url: final_no_frag.clone(),
                method: req_key.method.clone(),
                body: req_key.body.clone(),
                headers: req_key.headers.clone(),
            };
            if !self.visited_redirect_targets.insert(target_key) {
                return false;
            }
            if self.visited_get_bodies.get(&final_no_frag) == Some(&body_hash) {
                return false;
            }
            if self
                .visited_non_get_bodies
                .get(&final_no_frag)
                .is_some_and(|hashes| hashes.contains(&body_hash))
            {
                return false;
            }
            if final_no_frag != req_key.url_no_frag {
                self.redirected_post_final_urls
                    .insert(final_no_frag.clone());
            }
            self.visited_non_get_bodies
                .entry(final_no_frag)
                .or_default()
                .insert(body_hash);
            true
        }
    }

    fn can_enqueue(&self, next_url: &str, source_url: &str) -> bool {
        let next_key = CanonicalRequestKey::from_raw(next_url, source_url);
        if self.visited_requests.contains(&next_key) {
            return false;
        }
        if next_key.is_plain_get_or_head() {
            if self.visited_get_bodies.contains_key(&next_key.url_no_frag)
                || self
                    .redirected_post_final_urls
                    .contains(&next_key.url_no_frag)
            {
                return false;
            }
        } else if self.visited_redirect_targets.contains(&RedirectTargetKey {
            final_url: next_key.url_no_frag.clone(),
            method: next_key.method.clone(),
            body: next_key.body.clone(),
            headers: next_key.headers.clone(),
        }) {
            return false;
        }
        true
    }
}

fn strip_fragment(url: &str) -> &str {
    url.split('#').next().unwrap_or(url)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PageSegment<'a> {
    Html(&'a str),
    TopLevelText(&'a str),
}

fn is_html_token_start(slice: &str) -> bool {
    let b = slice.as_bytes();
    if b.len() < 2 || b[0] != b'<' {
        return false;
    }
    if slice.starts_with("<!--") || b[1] == b'!' || b[1] == b'?' {
        return true;
    }
    if b[1] == b'/' {
        return b
            .get(2)
            .is_some_and(|c| c.is_ascii_alphabetic() || *c == b'>');
    }
    b[1].is_ascii_alphabetic()
}

fn is_void_html_element(tag: &str) -> bool {
    matches!(
        tag,
        "area"
            | "base"
            | "basefont"
            | "bgsound"
            | "br"
            | "col"
            | "embed"
            | "frame"
            | "hr"
            | "img"
            | "input"
            | "keygen"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}

fn is_raw_text_html_element(tag: &str) -> bool {
    matches!(
        tag,
        "script" | "style" | "textarea" | "title" | "xmp" | "iframe" | "noembed" | "noframes"
    )
}

fn apply_html5_implicit_close(stack: &mut Vec<String>, next_tag: &str) {
    if matches!(
        next_tag,
        "address"
            | "article"
            | "aside"
            | "blockquote"
            | "details"
            | "dialog"
            | "dd"
            | "div"
            | "dl"
            | "dt"
            | "fieldset"
            | "figcaption"
            | "figure"
            | "footer"
            | "form"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "header"
            | "hgroup"
            | "hr"
            | "li"
            | "main"
            | "menu"
            | "nav"
            | "ol"
            | "p"
            | "pre"
            | "search"
            | "section"
            | "table"
            | "ul"
    ) && stack.last().is_some_and(|t| t == "p")
    {
        stack.pop();
    }
    if (next_tag == "li" && stack.last().is_some_and(|t| t == "li"))
        || (matches!(next_tag, "dt" | "dd") && stack.last().is_some_and(|t| t == "dt" || t == "dd"))
        || (matches!(next_tag, "td" | "th") && stack.last().is_some_and(|t| t == "td" || t == "th"))
    {
        stack.pop();
    } else if next_tag == "tr" {
        while stack
            .last()
            .is_some_and(|t| matches!(t.as_str(), "td" | "th" | "tr"))
        {
            stack.pop();
        }
    }
}

fn parse_tag_name(bytes: &[u8], start: usize) -> (String, usize) {
    let mut end = start;
    while end < bytes.len()
        && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'-' || bytes[end] == b':')
    {
        end += 1;
    }
    let name = std::str::from_utf8(&bytes[start..end])
        .unwrap_or("")
        .to_ascii_lowercase();
    (name, end)
}

fn find_raw_text_end(bytes: &[u8], from: usize, tag_name: &str) -> Option<usize> {
    let tag_bytes = tag_name.as_bytes();
    let needle_len = 2 + tag_bytes.len();
    let mut i = from;
    while i + needle_len <= bytes.len() {
        if bytes[i] == b'<'
            && bytes[i + 1] == b'/'
            && bytes[i + 2..i + needle_len].eq_ignore_ascii_case(tag_bytes)
        {
            let next_b = bytes.get(i + needle_len).copied();
            if next_b.is_some_and(|b| b.is_ascii_whitespace() || b == b'>' || b == b'/') {
                let mut j = i + needle_len;
                while j < bytes.len() {
                    if bytes[j] == b'>' {
                        return Some(j + 1);
                    }
                    j += 1;
                }
                return Some(bytes.len());
            }
        }
        i += 1;
    }
    None
}

fn segment_page_content(input: &str) -> (Vec<PageSegment<'_>>, bool) {
    let bytes = input.as_bytes();
    let mut segments = Vec::new();
    let mut element_stack: Vec<String> = Vec::new();
    let mut pos = 0usize;
    let mut text_start = 0usize;
    let mut html_start: Option<usize> = None;
    let mut has_html_tokens = false;

    while pos < bytes.len() {
        let Some(rel_lt) = input[pos..].find('<') else {
            break;
        };
        let lt_pos = pos + rel_lt;
        if !is_html_token_start(&input[lt_pos..]) {
            pos = lt_pos + 1;
            continue;
        }

        has_html_tokens = true;
        if html_start.is_none() {
            if lt_pos > text_start {
                segments.push(PageSegment::TopLevelText(&input[text_start..lt_pos]));
            }
            html_start = Some(lt_pos);
        }

        let tail = &input[lt_pos..];
        if tail.starts_with("<!--") {
            if tail.starts_with("<!-->") {
                pos = lt_pos + 5;
            } else if tail.starts_with("<!--->") {
                pos = lt_pos + 6;
            } else {
                let comment_body = &input[lt_pos + 4..];
                let end_std = comment_body.find("-->").map(|idx| lt_pos + 4 + idx + 3);
                let end_bang = comment_body.find("--!>").map(|idx| lt_pos + 4 + idx + 4);
                pos = match (end_std, end_bang) {
                    (Some(a), Some(b)) => a.min(b),
                    (Some(a), None) | (None, Some(a)) => a,
                    (None, None) => bytes.len(),
                };
            }
            if element_stack.is_empty()
                && let Some(start) = html_start.take()
            {
                segments.push(PageSegment::Html(&input[start..pos]));
                text_start = pos;
            }
        } else if tail.starts_with("<![CDATA[") {
            pos = if let Some(rel_end) = input[lt_pos + 9..].find("]]>") {
                lt_pos + 9 + rel_end + 3
            } else {
                bytes.len()
            };
            if element_stack.is_empty()
                && let Some(start) = html_start.take()
            {
                segments.push(PageSegment::Html(&input[start..pos]));
                text_start = pos;
            }
        } else if tail.starts_with("<!") || tail.starts_with("<?") {
            pos = if let Some(rel_end) = input[lt_pos + 2..].find('>') {
                lt_pos + 2 + rel_end + 1
            } else {
                bytes.len()
            };
            if element_stack.is_empty()
                && let Some(start) = html_start.take()
            {
                segments.push(PageSegment::Html(&input[start..pos]));
                text_start = pos;
            }
        } else if tail.starts_with("</") {
            let (tag_name, name_end) = parse_tag_name(bytes, lt_pos + 2);
            pos = if let Some(rel_gt) = input[name_end..].find('>') {
                name_end + rel_gt + 1
            } else {
                bytes.len()
            };
            if !tag_name.is_empty()
                && let Some(idx) = element_stack.iter().rposition(|t| t == &tag_name)
            {
                element_stack.truncate(idx);
            }
            if element_stack.is_empty()
                && let Some(start) = html_start.take()
            {
                segments.push(PageSegment::Html(&input[start..pos]));
                text_start = pos;
            }
        } else {
            let (tag_name, attr_start) = parse_tag_name(bytes, lt_pos + 1);
            let mut quote: Option<char> = None;
            let mut last_non_ws: Option<u8> = None;
            let mut closed_at: Option<usize> = None;
            for (rel_idx, ch) in input[attr_start..].char_indices() {
                if let Some(q) = quote {
                    if ch == q {
                        quote = None;
                        last_non_ws = Some(q as u8);
                    }
                } else if ch == '"' || ch == '\'' {
                    quote = Some(ch);
                } else if ch == '>' {
                    closed_at = Some(attr_start + rel_idx + 1);
                    break;
                } else if !ch.is_ascii_whitespace() {
                    last_non_ws = Some(if ch.is_ascii() { ch as u8 } else { b'x' });
                }
            }
            let Some(next_pos) = closed_at else {
                break;
            };
            pos = next_pos;
            let self_closing = last_non_ws == Some(b'/');

            apply_html5_implicit_close(&mut element_stack, &tag_name);
            let in_foreign = element_stack.iter().any(|t| t == "svg" || t == "math");

            if is_void_html_element(&tag_name)
                || (self_closing && (in_foreign || tag_name == "svg" || tag_name == "math"))
            {
                if element_stack.is_empty()
                    && let Some(start) = html_start.take()
                {
                    segments.push(PageSegment::Html(&input[start..pos]));
                    text_start = pos;
                }
            } else if is_raw_text_html_element(&tag_name) {
                element_stack.push(tag_name.clone());
                if let Some(end_pos) = find_raw_text_end(bytes, pos, &tag_name) {
                    pos = end_pos;
                    element_stack.pop();
                } else {
                    pos = bytes.len();
                }
                if element_stack.is_empty()
                    && let Some(start) = html_start.take()
                {
                    segments.push(PageSegment::Html(&input[start..pos]));
                    text_start = pos;
                }
            } else {
                element_stack.push(tag_name);
            }
        }
    }

    if let Some(start) = html_start.take() {
        if start < bytes.len() {
            segments.push(PageSegment::Html(&input[start..]));
        }
    } else if text_start < bytes.len() {
        segments.push(PageSegment::TopLevelText(&input[text_start..]));
    }

    (segments, has_html_tokens)
}

fn looks_like_mixed_plain_url(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.chars().any(char::is_whitespace) {
        return false;
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("data:") {
        return true;
    }
    if let Some(rest) = trimmed.strip_prefix("//") {
        return !rest.is_empty();
    }
    if let Some(rest) = trimmed.strip_prefix("../") {
        return !rest.is_empty();
    }
    if let Some(rest) = trimmed.strip_prefix("./") {
        return !rest.is_empty();
    }
    if let Some(rest) = trimmed.strip_prefix('/') {
        return !rest.is_empty();
    }
    let path_part = trimmed.split(['?', '#']).next().unwrap_or(trimmed);
    if path_part.contains('/') {
        return true;
    }
    path_part.rsplit_once('.').is_some_and(|(stem, ext)| {
        !stem.is_empty()
            && (2..=8).contains(&ext.len())
            && ext.bytes().all(|b| b.is_ascii_alphanumeric())
    })
}

fn is_invalid_image_candidate(raw: &str) -> bool {
    let t = raw.trim();
    if t.is_empty() || t == "#" || t.starts_with('#') {
        return true;
    }
    let lower = t.to_ascii_lowercase();
    lower.starts_with("javascript:")
        || lower.starts_with("about:")
        || lower.starts_with("data:image/svg+xml")
}

fn is_placeholder_or_fragment_url(raw_url: &str, final_url: &str) -> bool {
    let t = raw_url.trim();
    if t.is_empty() {
        return true;
    }
    let (url_part, has_valid_opts) = if let (part, Some(opt_json)) =
        split_legado_url_and_raw_options(t)
        && serde_json::from_str::<LegadoUrlOptions>(opt_json).is_ok()
    {
        (part.trim(), true)
    } else {
        (t, false)
    };
    if url_part.is_empty() {
        return !has_valid_opts;
    }
    if url_part == "#" || url_part.starts_with('#') {
        return true;
    }
    let lower = url_part.to_ascii_lowercase();
    if lower.starts_with("javascript:") || lower.starts_with("about:") {
        return true;
    }
    let final_no_frag = strip_fragment(final_url);
    url_part
        .strip_prefix(final_no_frag)
        .is_some_and(|rest| rest.starts_with('#'))
}

pub(crate) fn extract_img_srcs_from_html(html_fragment: &str, out: &mut Vec<String>) {
    let Ok(img_sel) = Selector::parse("img") else {
        return;
    };
    let doc = Html::parse_fragment(html_fragment);
    for el in doc.select(&img_sel) {
        let attr = ["data-url", "data-original", "data-src", "src"]
            .into_iter()
            .filter_map(|name| el.value().attr(name))
            .map(str::trim)
            .find(|val| !is_invalid_image_candidate(val));
        if let Some(val) = attr {
            out.push(val.to_string());
        }
    }
}

fn normalize_extracted_urls(candidates: Vec<String>, base_url: &str) -> Vec<String> {
    let mut out = Vec::with_capacity(candidates.len());
    for raw in candidates {
        let t = raw.trim();
        if is_invalid_image_candidate(t) {
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

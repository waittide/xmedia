pub mod http;
pub mod js_runtime;
pub mod model;
pub mod packager;
pub mod rule;

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use regex::Regex;

use crate::source::http::{HttpClientConfig, LegadoHttpClient, resolve_relative_url};
use crate::source::js_runtime::LegadoJsSession;
use crate::source::model::{
    BookJsContext, ChapterJsContext, ExecutionDiagnostics, LegadoBookSource, OnlineBookDetail,
    OnlineChapterContent, OnlineChapterItem, SearchBookResult, SourceRuntimeState,
};
use crate::source::rule::{
    RuleValue, cached_regex, eval_rule_list, eval_rule_string, rule_needs_js, split_js_pipeline,
};

static RE_COMMENT_IMG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?is)<div\s+rs-native[^>]*>.*?</div>|<img[^>]+(?:data:image/svg\+xml|qtbzs|showCmt|chapter_review_svg)[^>]*>"#,
    )
    .expect("valid comment img regex")
});
static RE_SCRIPT_STYLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?is)<(?:script|style|comment)[^>]*>.*?</(?:script|style|comment)>|<comment[^>]*/>",
    )
    .expect("valid script style regex")
});
static RE_HTML_BREAKS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)<br\s*/?>|</p>|</div>|</h[1-6]>|</li>|</blockquote>")
        .expect("valid html breaks regex")
});
static RE_HTML_TAGS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)<[^>]+>").expect("valid html tags regex"));

#[derive(Debug, Clone)]
pub struct SourceCallOutput<T> {
    pub data: T,
    pub state: SourceRuntimeState,
    pub diagnostics: ExecutionDiagnostics,
}

#[derive(Debug, Clone)]
pub struct BookSourceEngine {
    http_config: HttpClientConfig,
    js_timeout: Duration,
}

impl Default for BookSourceEngine {
    fn default() -> Self {
        Self {
            http_config: HttpClientConfig::default(),
            js_timeout: Duration::from_secs(45),
        }
    }
}

#[cfg(test)]
type SessionTuple = (
    LegadoHttpClient,
    Arc<Mutex<SourceRuntimeState>>,
    Arc<Mutex<HashMap<String, String>>>,
    Arc<Mutex<ExecutionDiagnostics>>,
    LegadoJsSession,
);

/// Shared per-source context for batch operations (e.g. multi-chapter downloads).
/// Reuses parsed `LegadoBookSource`, pooled `LegadoHttpClient`, cookies, and runtime state.
#[derive(Clone)]
pub struct ReusableSourceContext {
    source: Arc<LegadoBookSource>,
    http: LegadoHttpClient,
    state_arc: Arc<Mutex<SourceRuntimeState>>,
    cookies_arc: Arc<Mutex<HashMap<String, String>>>,
    js_timeout: Duration,
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
        ReusableWorkerSession {
            source: Arc::clone(&self.source),
            http: self.http.clone(),
            state_arc: self.state_arc.clone(),
            diag_arc: Arc::new(Mutex::new(ExecutionDiagnostics::default())),
            js_timeout: self.js_timeout,
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
    source: Arc<LegadoBookSource>,
    http: LegadoHttpClient,
    state_arc: Arc<Mutex<SourceRuntimeState>>,
    diag_arc: Arc<Mutex<ExecutionDiagnostics>>,
    js_timeout: Duration,
    js: Option<LegadoJsSession>,
    pending_search: Option<(String, u32)>,
    pending_book: Option<BookJsContext>,
    pending_chapter: Option<ChapterJsContext>,
}

impl ReusableWorkerSession {
    fn set_search_vars(&mut self, keyword: &str, page: u32) -> Result<(), String> {
        self.pending_search = Some((keyword.to_string(), page));
        if let Some(ref mut js) = self.js {
            js.set_search_vars(keyword, page)?;
        }
        Ok(())
    }

    fn set_book_context(&mut self, book: &BookJsContext) -> Result<(), String> {
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

    fn ensure_js(&mut self) -> Result<&mut LegadoJsSession, String> {
        if self.js.is_none() {
            let mut js = LegadoJsSession::new(
                &self.source,
                self.http.clone(),
                self.state_arc.clone(),
                self.diag_arc.clone(),
                self.js_timeout,
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

    fn eval_string(
        &mut self,
        input: &RuleValue,
        rule: &str,
        base_url: &str,
    ) -> Result<String, String> {
        if rule_needs_js(rule) {
            let js = self.ensure_js()?;
            eval_rule_string(input, rule, base_url, Some(js))
        } else {
            eval_rule_string(input, rule, base_url, None)
        }
    }

    fn eval_list(
        &mut self,
        input: &RuleValue,
        rule: &str,
        base_url: &str,
    ) -> Result<Vec<RuleValue>, String> {
        if rule_needs_js(rule) {
            let js = self.ensure_js()?;
            eval_rule_list(input, rule, base_url, Some(js))
        } else {
            eval_rule_list(input, rule, base_url, None)
        }
    }

    fn eval_opt(&mut self, input: &RuleValue, rule_opt: Option<&str>, base_url: &str) -> String {
        let Some(rule) = rule_opt.map(str::trim).filter(|s| !s.is_empty()) else {
            return String::new();
        };
        self.eval_string(input, rule, base_url)
            .unwrap_or_default()
            .trim()
            .to_string()
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
            if visited.len() >= MAX_TOC_PAGES || !visited.insert(page_url.clone()) {
                break;
            }
            let fetched = self.http.fetch(&page_url, &self.source.book_source_url)?;
            let raw_input = RuleValue::from_raw(fetched.body);

            let items = self.eval_list(&raw_input, chapter_list_rule, &fetched.final_url)?;

            for item in items {
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
                let is_volume = matches!(
                    is_vol_str.trim().to_ascii_lowercase().as_str(),
                    "true" | "1"
                ) || url_raw.is_empty()
                    || (!url_raw.starts_with("http://")
                        && !url_raw.starts_with("https://")
                        && !url_raw.starts_with("data:")
                        && !url_raw.contains('/'));

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

fn search_url_needs_js(raw_tpl: &str) -> bool {
    if split_js_pipeline(raw_tpl).is_some() {
        return true;
    }
    let mut rest = raw_tpl;
    while let Some(open) = rest.find("{{") {
        let after_open = &rest[open + 2..];
        let Some(close) = after_open.find("}}") else {
            break;
        };
        let expr = after_open[..close].trim();
        if expr != "key" && expr != "page" {
            return true;
        }
        rest = &after_open[close + 2..];
    }
    false
}

fn substitute_simple_search_url(raw_tpl: &str, keyword: &str, page: u32) -> String {
    let page_str = page.to_string();
    let mut out = String::with_capacity(raw_tpl.len() + keyword.len());
    let mut rest = raw_tpl;
    while let Some(open) = rest.find("{{") {
        out.push_str(&rest[..open]);
        let after_open = &rest[open + 2..];
        let Some(close) = after_open.find("}}") else {
            out.push_str(&rest[open..]);
            return out;
        };
        match after_open[..close].trim() {
            "key" => out.push_str(keyword),
            "page" => out.push_str(&page_str),
            other => {
                out.push_str("{{");
                out.push_str(other);
                out.push_str("}}");
            }
        }
        rest = &after_open[close + 2..];
    }
    out.push_str(rest);
    out
}

impl BookSourceEngine {
    pub fn new(http_config: HttpClientConfig, js_timeout: Duration) -> Self {
        Self {
            http_config,
            js_timeout,
        }
    }

    pub fn prepare_reusable_context(
        &self,
        source: Arc<LegadoBookSource>,
        mut initial_state: SourceRuntimeState,
    ) -> Result<ReusableSourceContext, String> {
        initial_state.ensure_android_id(&source.book_source_url);
        let cookies_arc = Arc::new(Mutex::new(initial_state.cookies.clone()));
        let state_arc = Arc::new(Mutex::new(initial_state));
        let http = LegadoHttpClient::new(
            self.http_config.clone(),
            cookies_arc.clone(),
            source.header.as_deref(),
            source.enabled_cookie_jar,
        )?;
        Ok(ReusableSourceContext {
            source,
            http,
            state_arc,
            cookies_arc,
            js_timeout: self.js_timeout,
        })
    }

    #[cfg(test)]
    fn build_session(
        &self,
        source: &LegadoBookSource,
        mut initial_state: SourceRuntimeState,
    ) -> Result<SessionTuple, String> {
        initial_state.ensure_android_id(&source.book_source_url);
        let cookies_arc = Arc::new(Mutex::new(initial_state.cookies.clone()));
        let state_arc = Arc::new(Mutex::new(initial_state));
        let diag_arc = Arc::new(Mutex::new(ExecutionDiagnostics::default()));

        let http = LegadoHttpClient::new(
            self.http_config.clone(),
            cookies_arc.clone(),
            source.header.as_deref(),
            source.enabled_cookie_jar,
        )?;
        let js = LegadoJsSession::new(
            source,
            http.clone(),
            state_arc.clone(),
            diag_arc.clone(),
            self.js_timeout,
        )?;

        Ok((http, state_arc, cookies_arc, diag_arc, js))
    }

    fn build_lazy_session(
        &self,
        source: &LegadoBookSource,
        mut initial_state: SourceRuntimeState,
    ) -> Result<(ReusableWorkerSession, Arc<Mutex<HashMap<String, String>>>), String> {
        initial_state.ensure_android_id(&source.book_source_url);
        let cookies_arc = Arc::new(Mutex::new(initial_state.cookies.clone()));
        let state_arc = Arc::new(Mutex::new(initial_state));
        let diag_arc = Arc::new(Mutex::new(ExecutionDiagnostics::default()));

        let http = LegadoHttpClient::new(
            self.http_config.clone(),
            cookies_arc.clone(),
            source.header.as_deref(),
            source.enabled_cookie_jar,
        )?;

        Ok((
            ReusableWorkerSession {
                source: Arc::new(source.clone()),
                http,
                state_arc,
                diag_arc,
                js_timeout: self.js_timeout,
                js: None,
                pending_search: None,
                pending_book: None,
                pending_chapter: None,
            },
            cookies_arc,
        ))
    }

    fn finalize_state(
        state_arc: &Arc<Mutex<SourceRuntimeState>>,
        cookies_arc: &Arc<Mutex<HashMap<String, String>>>,
        diag_arc: &Arc<Mutex<ExecutionDiagnostics>>,
    ) -> (SourceRuntimeState, ExecutionDiagnostics) {
        let mut st = state_arc.lock().unwrap_or_else(|e| e.into_inner()).clone();
        st.cookies = cookies_arc
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let diag = diag_arc.lock().unwrap_or_else(|e| e.into_inner()).clone();
        (st, diag)
    }

    /// Evaluates a Legado `searchUrl` template (supporting `{{key}}`, `{{page}}`, `<js>...</js>`, `@js:`).
    pub fn build_search_url(
        &self,
        source: &LegadoBookSource,
        js: &mut LegadoJsSession,
        keyword: &str,
        page: u32,
    ) -> Result<String, String> {
        let raw_tpl = source
            .search_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| format!("书源「{}」未配置 searchUrl", source.book_source_name))?;

        let evaluated = if search_url_needs_js(raw_tpl) {
            js.set_search_vars(keyword, page)?;
            let input = RuleValue::Text(String::new());
            eval_rule_string(&input, raw_tpl, &source.book_source_url, Some(js))?
        } else {
            substitute_simple_search_url(raw_tpl, keyword, page)
        };

        let trimmed = evaluated.trim();
        if trimmed.is_empty() {
            return Err("searchUrl 求值结果为空".into());
        }
        Ok(resolve_relative_url(trimmed, &source.book_source_url))
    }

    fn build_search_url_lazy(
        &self,
        source: &LegadoBookSource,
        session: &mut ReusableWorkerSession,
        keyword: &str,
        page: u32,
    ) -> Result<String, String> {
        let raw_tpl = source
            .search_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| format!("书源「{}」未配置 searchUrl", source.book_source_name))?;

        let evaluated = if search_url_needs_js(raw_tpl) {
            session.set_search_vars(keyword, page)?;
            let input = RuleValue::Text(String::new());
            session.eval_string(&input, raw_tpl, &source.book_source_url)?
        } else {
            substitute_simple_search_url(raw_tpl, keyword, page)
        };

        let trimmed = evaluated.trim();
        if trimmed.is_empty() {
            return Err("searchUrl 求值结果为空".into());
        }
        Ok(resolve_relative_url(trimmed, &source.book_source_url))
    }

    /// Searches books from one Legado book source.
    pub fn search_sync(
        &self,
        source: &LegadoBookSource,
        state: SourceRuntimeState,
        keyword: &str,
        page: u32,
    ) -> Result<SourceCallOutput<Vec<SearchBookResult>>, String> {
        let rule_search = source
            .rule_search
            .as_ref()
            .ok_or_else(|| format!("书源「{}」未配置 ruleSearch", source.book_source_name))?;

        let (mut session, cookies_arc) = self.build_lazy_session(source, state)?;
        session.set_search_vars(keyword, page)?;
        let search_url = self.build_search_url_lazy(source, &mut session, keyword, page)?;
        let fetched = session.http.fetch(&search_url, &source.book_source_url)?;
        let raw_input = RuleValue::from_raw(fetched.body);

        let book_list_rule = rule_search
            .book_list
            .as_deref()
            .ok_or_else(|| "ruleSearch 缺少 bookList 规则".to_string())?;
        let items = session.eval_list(&raw_input, book_list_rule, &fetched.final_url)?;

        let mut results = Vec::with_capacity(items.len());
        for item in items {
            let name = session.eval_opt(&item, rule_search.name.as_deref(), &fetched.final_url);
            let book_url_raw =
                session.eval_opt(&item, rule_search.book_url.as_deref(), &fetched.final_url);
            if name.is_empty() || book_url_raw.is_empty() {
                continue;
            }
            let author = session.eval_opt(&item, rule_search.author.as_deref(), &fetched.final_url);
            let cover_raw =
                session.eval_opt(&item, rule_search.cover_url.as_deref(), &fetched.final_url);
            let intro = session.eval_opt(&item, rule_search.intro.as_deref(), &fetched.final_url);
            let kind = session.eval_opt(&item, rule_search.kind.as_deref(), &fetched.final_url);
            let last_chapter = session.eval_opt(
                &item,
                rule_search.last_chapter.as_deref(),
                &fetched.final_url,
            );
            let word_count =
                session.eval_opt(&item, rule_search.word_count.as_deref(), &fetched.final_url);

            let book_url = resolve_relative_url(&book_url_raw, &fetched.final_url);
            let cover_url = (!cover_raw.is_empty())
                .then(|| resolve_relative_url(&cover_raw, &fetched.final_url));

            results.push(SearchBookResult {
                name,
                author,
                book_url,
                cover_url,
                intro: (!intro.is_empty()).then_some(intro),
                kind: (!kind.is_empty()).then_some(kind),
                last_chapter: (!last_chapter.is_empty()).then_some(last_chapter),
                word_count: (!word_count.is_empty()).then_some(word_count),
            });
        }

        let (final_state, diagnostics) =
            Self::finalize_state(&session.state_arc, &cookies_arc, &session.diag_arc);
        Ok(SourceCallOutput {
            data: results,
            state: final_state,
            diagnostics,
        })
    }

    /// Fetches book detail info (`ruleBookInfo`) and resolves `toc_url`.
    pub fn fetch_book_info_sync(
        &self,
        source: &LegadoBookSource,
        state: SourceRuntimeState,
        book_url: &str,
        fallback_hint: Option<&BookJsContext>,
    ) -> Result<SourceCallOutput<OnlineBookDetail>, String> {
        let (mut session, cookies_arc) = self.build_lazy_session(source, state)?;
        if let Some(hint) = fallback_hint {
            session.set_book_context(hint)?;
        }

        let fetched = session.http.fetch(book_url, &source.book_source_url)?;
        let mut info_input = RuleValue::from_raw(fetched.body);

        let Some(rule_info) = source.rule_book_info.as_ref() else {
            let detail = OnlineBookDetail {
                name: fallback_hint.map(|h| h.name.clone()).unwrap_or_default(),
                author: fallback_hint.map(|h| h.author.clone()).unwrap_or_default(),
                book_url: book_url.to_string(),
                toc_url: fetched.final_url,
                cover_url: fallback_hint.and_then(|h| h.cover_url.clone()),
                intro: fallback_hint.and_then(|h| h.intro.clone()),
                kind: None,
                last_chapter: None,
                word_count: None,
            };
            let (final_state, diagnostics) =
                Self::finalize_state(&session.state_arc, &cookies_arc, &session.diag_arc);
            return Ok(SourceCallOutput {
                data: detail,
                state: final_state,
                diagnostics,
            });
        };

        if let Some(ref init_rule) = rule_info.init
            && !init_rule.trim().is_empty()
        {
            let init_out = session.eval_string(&info_input, init_rule, &fetched.final_url)?;
            if !init_out.trim().is_empty() {
                info_input = RuleValue::from_raw(init_out);
            }
        }

        let name_extracted =
            session.eval_opt(&info_input, rule_info.name.as_deref(), &fetched.final_url);
        let author_extracted =
            session.eval_opt(&info_input, rule_info.author.as_deref(), &fetched.final_url);
        let cover_extracted = session.eval_opt(
            &info_input,
            rule_info.cover_url.as_deref(),
            &fetched.final_url,
        );
        let intro_extracted =
            session.eval_opt(&info_input, rule_info.intro.as_deref(), &fetched.final_url);
        let kind_extracted =
            session.eval_opt(&info_input, rule_info.kind.as_deref(), &fetched.final_url);
        let last_chapter_extracted = session.eval_opt(
            &info_input,
            rule_info.last_chapter.as_deref(),
            &fetched.final_url,
        );
        let word_count_extracted = session.eval_opt(
            &info_input,
            rule_info.word_count.as_deref(),
            &fetched.final_url,
        );
        let toc_raw = session.eval_opt(
            &info_input,
            rule_info.toc_url.as_deref(),
            &fetched.final_url,
        );

        let toc_url = if toc_raw.is_empty() {
            fetched.final_url.clone()
        } else {
            resolve_relative_url(&toc_raw, &fetched.final_url)
        };

        let detail = OnlineBookDetail {
            name: if !name_extracted.is_empty() {
                name_extracted
            } else {
                fallback_hint.map(|h| h.name.clone()).unwrap_or_default()
            },
            author: if !author_extracted.is_empty() {
                author_extracted
            } else {
                fallback_hint.map(|h| h.author.clone()).unwrap_or_default()
            },
            book_url: book_url.to_string(),
            toc_url,
            cover_url: if !cover_extracted.is_empty() {
                Some(resolve_relative_url(&cover_extracted, &fetched.final_url))
            } else {
                fallback_hint.and_then(|h| h.cover_url.clone())
            },
            intro: if !intro_extracted.is_empty() {
                Some(intro_extracted)
            } else {
                fallback_hint.and_then(|h| h.intro.clone())
            },
            kind: (!kind_extracted.is_empty()).then_some(kind_extracted),
            last_chapter: (!last_chapter_extracted.is_empty()).then_some(last_chapter_extracted),
            word_count: (!word_count_extracted.is_empty()).then_some(word_count_extracted),
        };

        let (final_state, diagnostics) =
            Self::finalize_state(&session.state_arc, &cookies_arc, &session.diag_arc);
        Ok(SourceCallOutput {
            data: detail,
            state: final_state,
            diagnostics,
        })
    }

    /// Fetches chapter list (`ruleToc`, supporting optional multi-page `nextTocUrl`).
    pub fn fetch_toc_sync(
        &self,
        source: &LegadoBookSource,
        state: SourceRuntimeState,
        toc_url: &str,
        book_ctx: &BookJsContext,
    ) -> Result<SourceCallOutput<Vec<OnlineChapterItem>>, String> {
        let (mut session, cookies_arc) = self.build_lazy_session(source, state)?;
        let chapters = session.fetch_toc(toc_url, book_ctx)?;
        let (final_state, diagnostics) =
            Self::finalize_state(&session.state_arc, &cookies_arc, &session.diag_arc);
        Ok(SourceCallOutput {
            data: chapters,
            state: final_state,
            diagnostics,
        })
    }

    /// Fetches a single chapter's content (`ruleContent`, supporting `nextContentUrl` pagination).
    pub fn fetch_chapter_content_sync(
        &self,
        source: &LegadoBookSource,
        state: SourceRuntimeState,
        chapter_url: &str,
        book_ctx: &BookJsContext,
        chapter_ctx: &ChapterJsContext,
    ) -> Result<SourceCallOutput<OnlineChapterContent>, String> {
        let (mut session, cookies_arc) = self.build_lazy_session(source, state)?;
        let data = session.fetch_chapter_content(chapter_url, book_ctx, chapter_ctx)?;
        let (final_state, diagnostics) =
            Self::finalize_state(&session.state_arc, &cookies_arc, &session.diag_arc);
        Ok(SourceCallOutput {
            data,
            state: final_state,
            diagnostics,
        })
    }

    /// Runs a `loginUi` button action (`loginUrl` + `action` JS) and persists updated variables/cookies.
    pub fn run_login_action_sync(
        &self,
        source: &LegadoBookSource,
        mut state: SourceRuntimeState,
        login_info: HashMap<String, String>,
        action_js: Option<&str>,
    ) -> Result<SourceCallOutput<String>, String> {
        state.login_info = login_info.clone();
        let (mut session, cookies_arc) = self.build_lazy_session(source, state)?;
        let login_url_js = source.login_url.as_deref().unwrap_or("");
        let out = session
            .ensure_js()?
            .eval_login_script(login_url_js, action_js, &login_info)?;
        let (final_state, diagnostics) =
            Self::finalize_state(&session.state_arc, &cookies_arc, &session.diag_arc);
        Ok(SourceCallOutput {
            data: out,
            state: final_state,
            diagnostics,
        })
    }
}

fn apply_content_replace_regex(content: &str, replace_rule: &str) -> String {
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

/// Converts Legado chapter HTML (including client-specific paragraph-comment SVG img tags)
/// into clean plain-text paragraphs suitable for reading and EPUB 3.0 packaging.
pub fn clean_chapter_html_to_text(raw: &str) -> String {
    if raw.trim().is_empty() {
        return String::new();
    }

    // Strip Legado client interactive comment/review SVG `<img>` tags & `<div rs-native>` wrappers
    let s = RE_COMMENT_IMG.replace_all(raw, "");
    let s = RE_SCRIPT_STYLE.replace_all(&s, "");
    // Convert block boundaries into newlines
    let s = RE_HTML_BREAKS.replace_all(&s, "\n");
    // Strip remaining HTML tags
    let s = RE_HTML_TAGS.replace_all(&s, "");

    // Decode common HTML entities
    let decoded = s
        .replace("&nbsp;", " ")
        .replace("&#160;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'");

    decoded
        .lines()
        .map(|l| l.trim().trim_start_matches('\u{3000}').trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::Path;
    use std::time::Duration;

    use axum::Router;
    use axum::routing::{get, post};

    use super::*;
    use crate::source::http::{
        HttpClientConfig, parse_legado_url, try_resolve_data_url, validate_target_url,
    };
    use crate::source::model::{
        BookJsContext, ChapterJsContext, LegadoBookSource, SourceRuntimeState,
    };

    #[test]
    fn data_url_with_type_option_encodes_bytes_as_hex_matching_legado() {
        let raw = r#"data:;base64,eyJrZXkiOiLmtYvor5UiLCJwYWdlIjoxfQ==,{"type":"gysearch"}"#;
        let parsed = parse_legado_url(raw);
        assert_eq!(parsed.options.content_type.as_deref(), Some("gysearch"));
        let res = try_resolve_data_url(&parsed).unwrap().unwrap();
        let decoded = String::from_utf8(hex::decode(&res.body).unwrap()).unwrap();
        assert_eq!(decoded, r#"{"key":"测试","page":1}"#);
    }

    #[test]
    fn ssrf_guard_blocks_loopback_and_private_ips() {
        assert!(validate_target_url("http://127.0.0.1:8080/api", false).is_err());
        assert!(validate_target_url("http://localhost/admin", false).is_err());
        assert!(validate_target_url("http://169.254.169.254/latest/meta-data", false).is_err());
        assert!(validate_target_url("http://10.0.0.1/secret", false).is_err());
        assert!(validate_target_url("http://192.168.1.1/", false).is_err());
        assert!(validate_target_url("file:///etc/passwd", false).is_err());
        // Public IP literal allowed
        assert!(validate_target_url("https://1.1.1.1/", false).is_ok());
    }

    #[test]
    fn js_sandbox_interrupts_infinite_loop_within_timeout() {
        let src = LegadoBookSource {
            book_source_name: "LoopTest".into(),
            book_source_url: "https://example.com".into(),
            search_url: Some("<js>while(true){}</js>".into()),
            rule_search: Some(Default::default()),
            ..Default::default()
        };
        let engine = BookSourceEngine::new(
            HttpClientConfig {
                allow_private_ips: true,
                ..Default::default()
            },
            Duration::from_millis(250),
        );
        let err = engine
            .search_sync(&src, SourceRuntimeState::default(), "test", 1)
            .unwrap_err();
        assert!(
            err.contains("interrupted") || err.contains("失败"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn end_to_end_classic_html_css_legado_source() {
        let app = Router::new()
            .route(
                "/search",
                get(|| async {
                    r#"<html><body>
                      <div class="result-list">
                        <div class="book-item">
                          <a class="title" href="/book/101">诡秘之主（精校版）</a>
                          <span class="author">爱潜水的乌贼</span>
                          <img class="cover" src="/img/101.jpg"/>
                          <p class="desc">蒸汽与机械的浪潮中</p>
                        </div>
                      </div>
                    </body></html>"#
                }),
            )
            .route(
                "/book/101",
                get(|| async {
                    r#"<html><body>
                      <h1 id="name">诡秘之主</h1>
                      <div class="meta"><span class="writer">爱潜水的乌贼</span></div>
                      <div id="intro">谁能触及非凡？</div>
                      <a id="toc-link" href="/book/101/toc">目录</a>
                    </body></html>"#
                }),
            )
            .route(
                "/book/101/toc",
                get(|| async {
                    r#"<html><body>
                      <ul id="chapters">
                        <li><a href="/book/101/ch1">第一章 绯红</a></li>
                        <li><a href="/book/101/ch2">第二章 情况</a></li>
                      </ul>
                    </body></html>"#
                }),
            )
            .route(
                "/book/101/ch1",
                get(|| async {
                    r#"<html><body>
                      <h2 class="ch-title">第一章 绯红</h2>
                      <div id="content"><p>痛！好痛！</p><p>头好痛！</p></div>
                    </body></html>"#
                }),
            );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_handle = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let base = format!("http://{addr}");
        let raw_json = serde_json::json!({
            "bookSourceName": "经典HTML书源",
            "bookSourceUrl": base,
            "searchUrl": "/search?q={{key}}&p={{page}}",
            "ruleSearch": {
                "bookList": "class.book-item",
                "name": "class.title@text##（精校版）",
                "author": "class.author@text",
                "bookUrl": "class.title@href",
                "coverUrl": "class.cover@src",
                "intro": "class.desc@text"
            },
            "ruleBookInfo": {
                "name": "id.name@text",
                "author": "class.writer@text",
                "intro": "id.intro@text",
                "tocUrl": "id.toc-link@href"
            },
            "ruleToc": {
                "chapterList": "#chapters li a",
                "chapterName": "text",
                "chapterUrl": "href"
            },
            "ruleContent": {
                "title": "class.ch-title@text",
                "content": "id.content@html"
            }
        })
        .to_string();

        let (src, _) = LegadoBookSource::parse_json_batch(&raw_json)
            .unwrap()
            .pop()
            .unwrap();
        let engine = BookSourceEngine::new(
            HttpClientConfig {
                allow_private_ips: true,
                ..Default::default()
            },
            Duration::from_secs(10),
        );

        let search_out = tokio::task::spawn_blocking({
            let engine = engine.clone();
            let src = src.clone();
            move || engine.search_sync(&src, SourceRuntimeState::default(), "诡秘", 1)
        })
        .await
        .unwrap()
        .unwrap();

        assert_eq!(search_out.data.len(), 1);
        assert_eq!(search_out.data[0].name, "诡秘之主");
        assert_eq!(search_out.data[0].author, "爱潜水的乌贼");
        assert!(search_out.data[0].book_url.ends_with("/book/101"));

        let book_url = search_out.data[0].book_url.clone();
        let detail_out = tokio::task::spawn_blocking({
            let engine = engine.clone();
            let src = src.clone();
            let state = search_out.state.clone();
            move || engine.fetch_book_info_sync(&src, state, &book_url, None)
        })
        .await
        .unwrap()
        .unwrap();

        assert_eq!(detail_out.data.name, "诡秘之主");
        assert!(detail_out.data.toc_url.ends_with("/book/101/toc"));

        let toc_url = detail_out.data.toc_url.clone();
        let toc_out = tokio::task::spawn_blocking({
            let engine = engine.clone();
            let src = src.clone();
            let state = detail_out.state.clone();
            let book_ctx = BookJsContext {
                name: detail_out.data.name.clone(),
                author: detail_out.data.author.clone(),
                ..Default::default()
            };
            move || engine.fetch_toc_sync(&src, state, &toc_url, &book_ctx)
        })
        .await
        .unwrap()
        .unwrap();

        assert_eq!(toc_out.data.len(), 2);
        assert_eq!(toc_out.data[0].title, "第一章 绯红");

        let ch0 = toc_out.data[0].clone();
        let content_out = tokio::task::spawn_blocking({
            let engine = engine.clone();
            let src = src.clone();
            let state = toc_out.state.clone();
            move || {
                engine.fetch_chapter_content_sync(
                    &src,
                    state,
                    &ch0.url,
                    &BookJsContext::default(),
                    &ChapterJsContext {
                        index: ch0.index,
                        title: ch0.title.clone(),
                        url: ch0.url.clone(),
                    },
                )
            }
        })
        .await
        .unwrap()
        .unwrap();

        assert_eq!(content_out.data.title, "第一章 绯红");
        assert_eq!(content_out.data.content, "痛！好痛！\n\n头好痛！");

        server_handle.abort();
    }

    #[tokio::test]
    async fn end_to_end_jslib_data_url_json_aggregator_source() {
        let app = Router::new()
            .route(
                "/search",
                get(|| async {
                    serde_json::json!({
                        "code": 0,
                        "data": [
                            {
                                "book_id": "bk99",
                                "book_name": "凡人修仙传（别名：凡人）",
                                "author": "忘语",
                                "source": "番茄",
                                "tab": "小说",
                                "thumb_url": "https://img.example.com/99.jpg",
                                "abstract": "一个普通山村穷小子",
                                "status": "完结",
                                "score": "9.6",
                                "tags": "仙侠",
                                "last_chapter_title": "大结局",
                                "word_number": "770万"
                            }
                        ]
                    })
                    .to_string()
                }),
            )
            .route(
                "/detail",
                post(|| async {
                    serde_json::json!({
                        "code": 0,
                        "data": {
                            "book_id": "bk99",
                            "book_name": "凡人修仙传",
                            "author": "忘语",
                            "source": "番茄",
                            "tab": "小说",
                            "abstract": "山村小子踏入修仙路"
                        }
                    })
                    .to_string()
                }),
            )
            .route(
                "/catalog",
                post(|| async {
                    serde_json::json!({
                        "code": 0,
                        "data": [
                            { "item_id": "c1", "title": "第一章 山边小村", "source": "番茄", "tab": "小说" },
                            { "item_id": "c2", "title": "第二章 青牛镇", "source": "番茄", "tab": "小说" }
                        ]
                    })
                    .to_string()
                }),
            )
            .route(
                "/content",
                post(|| async {
                    serde_json::json!({
                        "code": 0,
                        "content": "<p>二愣子睁大着双眼。</p><p>直直望着茅草屋顶。</p><img src=\"data:image/svg+xml;base64,PHN2Zz48L3N2Zz4=\">"
                    })
                    .to_string()
                }),
            );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_handle = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let api_base = format!("http://{addr}");
        let raw_json = serde_json::json!({
            "bookSourceName": "模拟聚合JS源",
            "bookSourceUrl": "聚合源",
            "jsLib": format!(r#"
                function BaseUrl() {{ return "{api_base}"; }}
                function req(path, method, body) {{
                    var opts = {{ method: method || 'GET', headers: {{ 'Content-Type': 'application/json' }}, body: body ? JSON.stringify(body) : '' }};
                    return this.java.ajax(this.BaseUrl() + path + ',' + JSON.stringify(opts));
                }}
            "#),
            "searchUrl": r#"<js>
                let payload = java.base64Encode(JSON.stringify({ key: key, page: page }));
                `data:;base64,${payload},{"type":"gysearch"}`;
            </js>"#,
            "ruleSearch": {
                "bookList": "<js>\nconst r = JSON.parse(java.hexDecodeToString(result));\nreq(`/search?q=${r.key}&p=${r.page}`);\n</js>\n$.data",
                "name": "$.book_name##（别名：.*?）",
                "author": "$.author",
                "bookUrl": "<js>\nlet p = java.base64Encode(JSON.stringify({ book_id: result.book_id, source: result.source }));\n`data:;base64,${p},{\"type\":\"gydetail\"}`;\n</js>",
                "coverUrl": "$.thumb_url",
                "intro": "$.abstract",
                "kind": "{{$.status}},{{$.score}},{{$.tags}}",
                "lastChapter": "{{$.source}} {{$.last_chapter_title}}",
                "wordCount": "$.word_number"
            },
            "ruleBookInfo": {
                "init": "<js>\nconst r = JSON.parse(java.hexDecodeToString(result));\nreq('/detail', 'POST', r);\n</js>$.data",
                "name": "$.book_name",
                "author": "$.author",
                "intro": "$.abstract",
                "tocUrl": "<js>\nlet p = java.base64Encode(JSON.stringify({ book_id: result.book_id }));\n`data:;base64,${p},{\"type\":\"gycatalog\"}`;\n</js>"
            },
            "ruleToc": {
                "chapterList": "<js>\nconst r = JSON.parse(java.hexDecodeToString(result));\njava.put('book_id', r.book_id);\nreq('/catalog', 'POST', r);\n</js>$.data",
                "chapterName": "$.title",
                "chapterUrl": "<js>\nlet p = java.base64Encode(JSON.stringify({ book_id: java.get('book_id'), item_id: result.item_id, title: result.title, sources: result.source }));\n`data:;base64,${p},{\"type\":\"gycontent\"}`;\n</js>"
            },
            "ruleContent": {
                "title": "@js:\nresult = String(java.hexDecodeToString(result));\nJSON.parse(result)['title']",
                "content": "<js>\nconst r = JSON.parse(java.hexDecodeToString(result));\nreq('/content', 'POST', r);\n</js>$.content"
            }
        })
        .to_string();

        let (src, _) = LegadoBookSource::parse_json_batch(&raw_json)
            .unwrap()
            .pop()
            .unwrap();
        let engine = BookSourceEngine::new(
            HttpClientConfig {
                allow_private_ips: true,
                ..Default::default()
            },
            Duration::from_secs(10),
        );

        let search_out = tokio::task::spawn_blocking({
            let engine = engine.clone();
            let src = src.clone();
            move || engine.search_sync(&src, SourceRuntimeState::default(), "凡人", 1)
        })
        .await
        .unwrap()
        .unwrap();

        assert_eq!(search_out.data.len(), 1);
        assert_eq!(search_out.data[0].name, "凡人修仙传");
        assert_eq!(search_out.data[0].kind.as_deref(), Some("完结,9.6,仙侠"));
        assert_eq!(
            search_out.data[0].last_chapter.as_deref(),
            Some("番茄 大结局")
        );

        let book_url = search_out.data[0].book_url.clone();
        let detail_out = tokio::task::spawn_blocking({
            let engine = engine.clone();
            let src = src.clone();
            let state = search_out.state.clone();
            move || engine.fetch_book_info_sync(&src, state, &book_url, None)
        })
        .await
        .unwrap()
        .unwrap();
        assert_eq!(detail_out.data.name, "凡人修仙传");

        let toc_url = detail_out.data.toc_url.clone();
        let toc_out = tokio::task::spawn_blocking({
            let engine = engine.clone();
            let src = src.clone();
            let state = detail_out.state.clone();
            move || engine.fetch_toc_sync(&src, state, &toc_url, &BookJsContext::default())
        })
        .await
        .unwrap()
        .unwrap();
        assert_eq!(toc_out.data.len(), 2);
        assert_eq!(toc_out.data[0].title, "第一章 山边小村");

        let ch0 = toc_out.data[0].clone();
        let content_out = tokio::task::spawn_blocking({
            let engine = engine.clone();
            let src = src.clone();
            let state = toc_out.state.clone();
            move || {
                engine.fetch_chapter_content_sync(
                    &src,
                    state,
                    &ch0.url,
                    &BookJsContext::default(),
                    &ChapterJsContext {
                        index: 0,
                        title: ch0.title.clone(),
                        url: ch0.url.clone(),
                    },
                )
            }
        })
        .await
        .unwrap()
        .unwrap();

        assert_eq!(content_out.data.title, "第一章 山边小村");
        assert_eq!(
            content_out.data.content,
            "二愣子睁大着双眼。\n\n直直望着茅草屋顶。"
        );

        server_handle.abort();
    }

    #[test]
    fn real_user_book_source_json_parses_and_executes_jslib_and_login_actions() {
        let user_path = Path::new("/data/home/下载/bookSource.json");
        if !user_path.is_file() {
            return;
        }
        let raw = std::fs::read_to_string(user_path).unwrap();
        let parsed = LegadoBookSource::parse_json_batch(&raw).unwrap();
        assert_eq!(parsed.len(), 1);
        let (src, _) = &parsed[0];
        assert!(src.book_source_name.contains("光遇聚合"));
        assert!(!src.parse_login_ui().is_empty());

        let engine = BookSourceEngine::default();
        // Verify jsLib loads cleanly, checkEnv() == "安卓", searchUrl builds valid data:;base64 URL,
        // and loginUi backup toggles (`ste('段评')`, `setSearchSource()`) persist variables cleanly!
        let action_out = engine
            .run_login_action_sync(
                src,
                SourceRuntimeState::default(),
                HashMap::from([(
                    "搜索来源(支持的平台请前往源变量中查看,多个来源用英文逗号分割)".to_string(),
                    "番茄,七猫".to_string(),
                )]),
                Some("setSearchSource(); ste('段评'); checkEnv();"),
            )
            .unwrap();

        assert_eq!(action_out.data, "安卓");
        assert!(action_out.state.variable.contains("番茄,七猫"));
        assert!(!action_out.diagnostics.toasts.is_empty());

        // Verify searchUrl evaluation with persisted state
        let (http, state_arc, cookies_arc, diag_arc, mut js) =
            engine.build_session(src, action_out.state).unwrap();
        let search_url = engine
            .build_search_url(src, &mut js, "诡秘之主", 1)
            .unwrap();
        assert!(search_url.starts_with("data:;base64,"));
        let resolved = http.fetch(&search_url, &src.book_source_url).unwrap();
        let decoded_payload = String::from_utf8(hex::decode(&resolved.body).unwrap()).unwrap();
        assert!(decoded_payload.contains("诡秘之主"));
        assert!(decoded_payload.contains("番茄,七猫"));
        let _ = (state_arc, cookies_arc, diag_arc);
    }
}

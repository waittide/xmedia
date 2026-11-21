use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use regex::Regex;

use super::session::{ReusableSourceContext, ReusableWorkerSession};
use crate::source::http::{HttpClientConfig, LegadoHttpClient, resolve_relative_url};
use crate::source::js_runtime::now_mono_ms;
use crate::source::model::{
    BookJsContext, ChapterJsContext, ExecutionDiagnostics, LegadoBookSource, OnlineBookDetail,
    OnlineChapterContent, OnlineChapterItem, SearchBookResult, SourceRuntimeState,
};
use crate::source::rule::{RuleValue, cached_regex, split_js_pipeline};

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
pub struct SourceCallError {
    pub message: String,
    pub diagnostics: ExecutionDiagnostics,
}

impl SourceCallError {
    pub fn new(message: impl Into<String>, diagnostics: ExecutionDiagnostics) -> Self {
        Self {
            message: message.into(),
            diagnostics,
        }
    }
}

impl fmt::Display for SourceCallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl From<SourceCallError> for String {
    fn from(value: SourceCallError) -> Self {
        value.message
    }
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

type LazySessionPair = (ReusableWorkerSession, Arc<Mutex<HashMap<String, String>>>);

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
        initial_state: SourceRuntimeState,
    ) -> Result<ReusableSourceContext, String> {
        self.prepare_reusable_context_with_cancel(
            source,
            initial_state,
            Arc::new(AtomicBool::new(false)),
        )
    }

    pub fn prepare_reusable_context_with_cancel(
        &self,
        source: Arc<LegadoBookSource>,
        mut initial_state: SourceRuntimeState,
        cancel_flag: Arc<AtomicBool>,
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
            cancel_flag,
        })
    }

    fn build_lazy_session(
        &self,
        source: &LegadoBookSource,
        mut initial_state: SourceRuntimeState,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<LazySessionPair, String> {
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

        let op_deadline_ms = Arc::new(AtomicU64::new(
            now_mono_ms().saturating_add(op_timeout.as_millis() as u64),
        ));

        Ok((
            ReusableWorkerSession {
                source: Arc::new(source.clone()),
                http,
                state_arc,
                diag_arc,
                js_timeout: self.js_timeout,
                op_deadline_ms,
                cancel_flag,
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
        self.search_sync_with_control(
            source,
            state,
            keyword,
            page,
            self.js_timeout,
            Arc::new(AtomicBool::new(false)),
        )
        .map_err(Into::into)
    }

    pub fn search_sync_with_control(
        &self,
        source: &LegadoBookSource,
        state: SourceRuntimeState,
        keyword: &str,
        page: u32,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<Vec<SearchBookResult>>, SourceCallError> {
        let rule_search = source.rule_search.as_ref().ok_or_else(|| {
            SourceCallError::new(
                format!("书源「{}」未配置 ruleSearch", source.book_source_name),
                ExecutionDiagnostics::default(),
            )
        })?;

        let (mut session, cookies_arc) = self
            .build_lazy_session(source, state, op_timeout, cancel_flag)
            .map_err(|e| SourceCallError::new(e, ExecutionDiagnostics::default()))?;
        let res: Result<Vec<SearchBookResult>, String> = (|| {
            session.set_search_vars(keyword, page)?;
            let search_url = self.build_search_url_lazy(source, &mut session, keyword, page)?;
            session.check_active()?;
            let fetched = session.http.fetch(&search_url, &source.book_source_url)?;
            let raw_input = RuleValue::from_raw(fetched.body);

            let book_list_rule = rule_search
                .book_list
                .as_deref()
                .ok_or_else(|| "ruleSearch 缺少 bookList 规则".to_string())?;
            let items = session.eval_list(&raw_input, book_list_rule, &fetched.final_url)?;

            let mut results = Vec::with_capacity(items.len());
            for item in items {
                session.check_active()?;
                let name = session.eval_opt(&item, rule_search.name.as_deref(), &fetched.final_url);
                let book_url_raw =
                    session.eval_opt(&item, rule_search.book_url.as_deref(), &fetched.final_url);
                if name.is_empty() || book_url_raw.is_empty() {
                    continue;
                }
                let author =
                    session.eval_opt(&item, rule_search.author.as_deref(), &fetched.final_url);
                let cover_raw =
                    session.eval_opt(&item, rule_search.cover_url.as_deref(), &fetched.final_url);
                let intro =
                    session.eval_opt(&item, rule_search.intro.as_deref(), &fetched.final_url);
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
            Ok(results)
        })();

        match res {
            Ok(results) => {
                let (final_state, diagnostics) =
                    Self::finalize_state(&session.state_arc, &cookies_arc, &session.diag_arc);
                Ok(SourceCallOutput {
                    data: results,
                    state: final_state,
                    diagnostics,
                })
            }
            Err(err) => Err(SourceCallError::new(err, session.snapshot_diagnostics())),
        }
    }

    /// Fetches book detail info (`ruleBookInfo`) and resolves `toc_url`.
    pub fn fetch_book_info_sync(
        &self,
        source: &LegadoBookSource,
        state: SourceRuntimeState,
        book_url: &str,
        fallback_hint: Option<&BookJsContext>,
    ) -> Result<SourceCallOutput<OnlineBookDetail>, String> {
        self.fetch_book_info_sync_with_control(
            source,
            state,
            book_url,
            fallback_hint,
            self.js_timeout,
            Arc::new(AtomicBool::new(false)),
        )
        .map_err(Into::into)
    }

    pub fn fetch_book_info_sync_with_control(
        &self,
        source: &LegadoBookSource,
        state: SourceRuntimeState,
        book_url: &str,
        fallback_hint: Option<&BookJsContext>,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<OnlineBookDetail>, SourceCallError> {
        let (mut session, cookies_arc) = self
            .build_lazy_session(source, state, op_timeout, cancel_flag)
            .map_err(|e| SourceCallError::new(e, ExecutionDiagnostics::default()))?;

        let res: Result<OnlineBookDetail, String> = (|| {
            if let Some(hint) = fallback_hint {
                session.set_book_context(hint)?;
            }
            session.check_active()?;
            let fetched = session.http.fetch(book_url, &source.book_source_url)?;
            let mut info_input = RuleValue::from_raw(fetched.body);

            let Some(rule_info) = source.rule_book_info.as_ref() else {
                return Ok(OnlineBookDetail {
                    name: fallback_hint.map(|h| h.name.clone()).unwrap_or_default(),
                    author: fallback_hint.map(|h| h.author.clone()).unwrap_or_default(),
                    book_url: book_url.to_string(),
                    toc_url: fetched.final_url,
                    cover_url: fallback_hint.and_then(|h| h.cover_url.clone()),
                    intro: fallback_hint.and_then(|h| h.intro.clone()),
                    kind: None,
                    last_chapter: None,
                    word_count: None,
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

            Ok(OnlineBookDetail {
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
                last_chapter: (!last_chapter_extracted.is_empty())
                    .then_some(last_chapter_extracted),
                word_count: (!word_count_extracted.is_empty()).then_some(word_count_extracted),
            })
        })();

        match res {
            Ok(detail) => {
                let (final_state, diagnostics) =
                    Self::finalize_state(&session.state_arc, &cookies_arc, &session.diag_arc);
                Ok(SourceCallOutput {
                    data: detail,
                    state: final_state,
                    diagnostics,
                })
            }
            Err(err) => Err(SourceCallError::new(err, session.snapshot_diagnostics())),
        }
    }

    /// Fetches chapter list (`ruleToc`, supporting optional multi-page `nextTocUrl`).
    pub fn fetch_toc_sync(
        &self,
        source: &LegadoBookSource,
        state: SourceRuntimeState,
        toc_url: &str,
        book_ctx: &BookJsContext,
    ) -> Result<SourceCallOutput<Vec<OnlineChapterItem>>, String> {
        self.fetch_toc_sync_with_control(
            source,
            state,
            toc_url,
            book_ctx,
            self.js_timeout,
            Arc::new(AtomicBool::new(false)),
        )
        .map_err(Into::into)
    }

    pub fn fetch_toc_sync_with_control(
        &self,
        source: &LegadoBookSource,
        state: SourceRuntimeState,
        toc_url: &str,
        book_ctx: &BookJsContext,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<Vec<OnlineChapterItem>>, SourceCallError> {
        let (mut session, cookies_arc) = self
            .build_lazy_session(source, state, op_timeout, cancel_flag)
            .map_err(|e| SourceCallError::new(e, ExecutionDiagnostics::default()))?;
        match session.fetch_toc(toc_url, book_ctx) {
            Ok(chapters) => {
                let (final_state, diagnostics) =
                    Self::finalize_state(&session.state_arc, &cookies_arc, &session.diag_arc);
                Ok(SourceCallOutput {
                    data: chapters,
                    state: final_state,
                    diagnostics,
                })
            }
            Err(err) => Err(SourceCallError::new(err, session.snapshot_diagnostics())),
        }
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
        self.fetch_chapter_content_sync_with_control(
            source,
            state,
            chapter_url,
            book_ctx,
            chapter_ctx,
            self.js_timeout,
            Arc::new(AtomicBool::new(false)),
        )
        .map_err(Into::into)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn fetch_chapter_content_sync_with_control(
        &self,
        source: &LegadoBookSource,
        state: SourceRuntimeState,
        chapter_url: &str,
        book_ctx: &BookJsContext,
        chapter_ctx: &ChapterJsContext,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<OnlineChapterContent>, SourceCallError> {
        let (mut session, cookies_arc) = self
            .build_lazy_session(source, state, op_timeout, cancel_flag)
            .map_err(|e| SourceCallError::new(e, ExecutionDiagnostics::default()))?;
        match session.fetch_chapter_content(chapter_url, book_ctx, chapter_ctx) {
            Ok(data) => {
                let (final_state, diagnostics) =
                    Self::finalize_state(&session.state_arc, &cookies_arc, &session.diag_arc);
                Ok(SourceCallOutput {
                    data,
                    state: final_state,
                    diagnostics,
                })
            }
            Err(err) => Err(SourceCallError::new(err, session.snapshot_diagnostics())),
        }
    }

    /// Runs a `loginUi` button action (`loginUrl` + `action` JS) and persists updated variables/cookies.
    /// Merges submitted `login_info` into existing `state.login_info` rather than wiping omitted keys.
    pub fn run_login_action_sync(
        &self,
        source: &LegadoBookSource,
        mut state: SourceRuntimeState,
        login_info: HashMap<String, String>,
        action_js: Option<&str>,
    ) -> Result<SourceCallOutput<String>, String> {
        for (k, v) in login_info {
            state.login_info.insert(k, v);
        }
        let merged_login = state.login_info.clone();
        let (mut session, cookies_arc) = self.build_lazy_session(
            source,
            state,
            self.js_timeout,
            Arc::new(AtomicBool::new(false)),
        )?;
        let login_url_js = source.login_url.as_deref().unwrap_or("");
        let out = session
            .ensure_js()?
            .eval_login_script(login_url_js, action_js, &merged_login)?;
        let (final_state, diagnostics) =
            Self::finalize_state(&session.state_arc, &cookies_arc, &session.diag_arc);
        Ok(SourceCallOutput {
            data: out,
            state: final_state,
            diagnostics,
        })
    }
}

pub(super) fn apply_content_replace_regex(content: &str, replace_rule: &str) -> String {
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

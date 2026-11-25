use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use regex::Regex;

use super::BookSourceEngine;
use crate::application::ports::{BookBatchContextPort, BookSourceEnginePort};
use crate::domain::online::{
    BookJsContext, ChapterJsContext, ExecutionDiagnostics, LegadoBookSource, OnlineBookDetail,
    OnlineChapterContent, OnlineChapterItem, SearchBookResult, SourceCallError, SourceCallOutput,
    SourceRuntimeState,
};
use crate::infra::source::rule::{cached_regex, strip_js_wrapper};

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

impl BookSourceEngine {
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
        let mut session = self
            .build_lazy_session(source, state, op_timeout, cancel_flag)
            .map_err(|e| SourceCallError::new(e, ExecutionDiagnostics::default()))?;
        match session.fetch_toc(toc_url, book_ctx) {
            Ok(chapters) => {
                let (final_state, diagnostics) = session.finalize_state();
                Ok(SourceCallOutput {
                    data: chapters,
                    state: final_state,
                    diagnostics,
                })
            }
            Err(err) => Err(SourceCallError::new(err, session.snapshot_diagnostics())),
        }
    }

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
        let mut session = self
            .build_lazy_session(source, state, op_timeout, cancel_flag)
            .map_err(|e| SourceCallError::new(e, ExecutionDiagnostics::default()))?;
        match session.fetch_chapter_content(chapter_url, book_ctx, chapter_ctx) {
            Ok(data) => {
                let (final_state, diagnostics) = session.finalize_state();
                Ok(SourceCallOutput {
                    data,
                    state: final_state,
                    diagnostics,
                })
            }
            Err(err) => Err(SourceCallError::new(err, session.snapshot_diagnostics())),
        }
    }

    pub fn run_login_action_sync(
        &self,
        source: &LegadoBookSource,
        state: SourceRuntimeState,
        login_info: HashMap<String, String>,
        action_js: Option<&str>,
    ) -> Result<SourceCallOutput<String>, String> {
        self.run_login_action_sync_with_diagnostics(source, state, login_info, action_js)
            .map_err(Into::into)
    }

    pub fn run_login_action_sync_with_diagnostics(
        &self,
        source: &LegadoBookSource,
        mut state: SourceRuntimeState,
        login_info: HashMap<String, String>,
        action_js: Option<&str>,
    ) -> Result<SourceCallOutput<String>, SourceCallError> {
        for (k, v) in login_info {
            state.login_info.insert(k, v);
        }
        let merged_login = state.login_info.clone();
        let mut session = self
            .build_lazy_session(
                source,
                state,
                self.js_timeout,
                Arc::new(AtomicBool::new(false)),
            )
            .map_err(|e| SourceCallError::new(e, ExecutionDiagnostics::default()))?;
        let raw_login = source.login_url.as_deref().unwrap_or("").trim();
        let is_plain_http_login_url = raw_login
            .get(..7)
            .is_some_and(|p| p.eq_ignore_ascii_case("http://"))
            || raw_login
                .get(..8)
                .is_some_and(|p| p.eq_ignore_ascii_case("https://"));
        let login_url_js = if is_plain_http_login_url {
            ""
        } else {
            strip_js_wrapper(raw_login)
        };
        let login_check_stripped = source
            .login_check_js
            .as_deref()
            .map(strip_js_wrapper)
            .filter(|s| !s.is_empty());
        let effective_action = match action_js.map(strip_js_wrapper).filter(|s| !s.is_empty()) {
            Some(act) => Some(act),
            None => login_check_stripped,
        };
        match session
            .inner
            .eval_login_script(login_url_js, effective_action, &merged_login)
        {
            Ok(out) => {
                let (final_state, diagnostics) = session.finalize_state();
                Ok(SourceCallOutput {
                    data: out,
                    state: final_state,
                    diagnostics,
                })
            }
            Err(err) => Err(SourceCallError::new(err, session.snapshot_diagnostics())),
        }
    }
}

impl BookSourceEnginePort for BookSourceEngine {
    fn prepare_batch_context(
        &self,
        source: Arc<LegadoBookSource>,
        initial_state: SourceRuntimeState,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<Arc<dyn BookBatchContextPort>, String> {
        let ctx = self.prepare_reusable_context_with_cancel(source, initial_state, cancel_flag)?;
        Ok(Arc::new(ctx))
    }

    fn search_sync_with_control(
        &self,
        source: &LegadoBookSource,
        state: SourceRuntimeState,
        keyword: &str,
        page: u32,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<Vec<SearchBookResult>>, SourceCallError> {
        Self::search_sync_with_control(self, source, state, keyword, page, op_timeout, cancel_flag)
    }

    fn fetch_book_info_sync_with_control(
        &self,
        source: &LegadoBookSource,
        state: SourceRuntimeState,
        book_url: &str,
        fallback_hint: Option<&BookJsContext>,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<OnlineBookDetail>, SourceCallError> {
        Self::fetch_book_info_sync_with_control(
            self,
            source,
            state,
            book_url,
            fallback_hint,
            op_timeout,
            cancel_flag,
        )
    }

    fn fetch_toc_sync_with_control(
        &self,
        source: &LegadoBookSource,
        state: SourceRuntimeState,
        toc_url: &str,
        book_ctx: &BookJsContext,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<Vec<OnlineChapterItem>>, SourceCallError> {
        Self::fetch_toc_sync_with_control(
            self,
            source,
            state,
            toc_url,
            book_ctx,
            op_timeout,
            cancel_flag,
        )
    }

    fn fetch_chapter_content_sync_with_control(
        &self,
        source: &LegadoBookSource,
        state: SourceRuntimeState,
        chapter_url: &str,
        book_ctx: &BookJsContext,
        chapter_ctx: &ChapterJsContext,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<OnlineChapterContent>, SourceCallError> {
        Self::fetch_chapter_content_sync_with_control(
            self,
            source,
            state,
            chapter_url,
            book_ctx,
            chapter_ctx,
            op_timeout,
            cancel_flag,
        )
    }

    fn run_login_action_sync(
        &self,
        source: &LegadoBookSource,
        state: SourceRuntimeState,
        login_info: HashMap<String, String>,
        action_js: Option<&str>,
    ) -> Result<SourceCallOutput<String>, SourceCallError> {
        self.run_login_action_sync_with_diagnostics(source, state, login_info, action_js)
    }
}

pub(in crate::infra::source) fn apply_content_replace_regex(
    content: &str,
    replace_rule: &str,
) -> String {
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

pub fn clean_chapter_html_to_text(raw: &str) -> String {
    if raw.trim().is_empty() {
        return String::new();
    }

    let s = RE_COMMENT_IMG.replace_all(raw, "");
    let s = RE_SCRIPT_STYLE.replace_all(&s, "");
    let s = RE_HTML_BREAKS.replace_all(&s, "\n");
    let s = RE_HTML_TAGS.replace_all(&s, "");

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

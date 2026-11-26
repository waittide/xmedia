mod list_eval;
mod url_template;

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use xmedia_source::http::{
    HttpClientConfig, LegadoHttpClient, resolve_relative_url,
    resolve_relative_url_preserving_options,
};
pub use xmedia_source::model::{SourceCallError, SourceCallOutput};
use xmedia_source::rule::{RuleValue, strip_js_wrapper};

use super::session::{ReusableComicContext, ReusableComicSession};
use crate::application::ports::ComicSourceEnginePort;
use crate::domain::online::{
    ComicChapterJsContext, ComicJsContext, ComicSourceSpec, ExecutionDiagnostics,
    ExploreCategoryItem, OnlineComicChapter, OnlineComicDetail, OnlineComicItem,
    SourceRuntimeState,
};
use crate::domain::series::ReadingDirection;

#[derive(Debug, Clone)]
pub struct ComicSourceEngine {
    http_config: HttpClientConfig,
    js_timeout: Duration,
}

impl Default for ComicSourceEngine {
    fn default() -> Self {
        Self {
            http_config: HttpClientConfig::default(),
            js_timeout: Duration::from_secs(45),
        }
    }
}

impl ComicSourceEngine {
    pub fn new(http_config: HttpClientConfig, js_timeout: Duration) -> Self {
        Self {
            http_config,
            js_timeout,
        }
    }

    pub(super) fn js_timeout(&self) -> Duration {
        self.js_timeout
    }

    pub fn prepare_reusable_context_with_cancel(
        &self,
        source: Arc<ComicSourceSpec>,
        mut initial_state: SourceRuntimeState,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<ReusableComicContext, String> {
        initial_state.ensure_android_id(&source.source_url);
        let cookies_arc = Arc::new(Mutex::new(initial_state.cookies.clone()));
        let state_arc = Arc::new(Mutex::new(initial_state));
        let http = LegadoHttpClient::new(
            self.http_config.clone(),
            cookies_arc.clone(),
            source.header.as_deref(),
            source.enabled_cookie_jar,
        )?;
        Ok(ReusableComicContext {
            source,
            http,
            state_arc,
            cookies_arc,
            js_timeout: self.js_timeout,
            cancel_flag,
        })
    }

    pub(super) fn build_lazy_session(
        &self,
        source: &ComicSourceSpec,
        mut initial_state: SourceRuntimeState,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<ReusableComicSession, String> {
        initial_state.ensure_android_id(&source.source_url);
        let cookies_arc = Arc::new(Mutex::new(initial_state.cookies.clone()));
        let state_arc = Arc::new(Mutex::new(initial_state));

        let http = LegadoHttpClient::new(
            self.http_config.clone(),
            cookies_arc.clone(),
            source.header.as_deref(),
            source.enabled_cookie_jar,
        )?;

        Ok(ReusableComicContext::new_worker_session(
            Arc::new(source.clone()),
            http,
            state_arc,
            cookies_arc,
            self.js_timeout,
            op_timeout,
            cancel_flag,
        ))
    }

    pub fn fetch_detail_sync(
        &self,
        source: &ComicSourceSpec,
        state: SourceRuntimeState,
        comic_url: &str,
        fallback_hint: Option<&ComicJsContext>,
    ) -> Result<SourceCallOutput<OnlineComicDetail>, String> {
        self.fetch_detail_sync_with_control(
            source,
            state,
            comic_url,
            fallback_hint,
            self.js_timeout,
            Arc::new(AtomicBool::new(false)),
        )
        .map_err(Into::into)
    }

    pub fn fetch_detail_sync_with_control(
        &self,
        source: &ComicSourceSpec,
        state: SourceRuntimeState,
        comic_url: &str,
        fallback_hint: Option<&ComicJsContext>,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<OnlineComicDetail>, SourceCallError> {
        let mut session = self
            .build_lazy_session(source, state, op_timeout, cancel_flag)
            .map_err(|e| SourceCallError::new(e, ExecutionDiagnostics::default()))?;

        let res: Result<OnlineComicDetail, String> = (|| {
            if let Some(hint) = fallback_hint {
                session.set_comic_context(hint)?;
            }
            session.check_active()?;
            let fetched = session.http_fetch(comic_url)?;
            if fetched.status >= 400 {
                return Err(format!("详情页请求失败: HTTP {}", fetched.status));
            }
            let mut info_input = RuleValue::from_raw(fetched.body);

            let rule_detail = match source.detail_rule_or_fallback(
                comic_url,
                &fetched.final_url,
                fallback_hint,
            ) {
                Ok(r) => r,
                Err(fallback_detail) => return Ok(*fallback_detail),
            };

            if let Some(ref init_rule) = rule_detail.init
                && !init_rule.trim().is_empty()
            {
                let init_out = session.eval_string(&info_input, init_rule, &fetched.final_url)?;
                if !init_out.trim().is_empty() {
                    info_input = RuleValue::from_raw(init_out);
                }
            }

            let name_extracted =
                session.eval_opt(&info_input, rule_detail.name.as_deref(), &fetched.final_url);
            let author_extracted = session.eval_opt(
                &info_input,
                rule_detail.author.as_deref(),
                &fetched.final_url,
            );
            let cover_extracted = session.eval_opt(
                &info_input,
                rule_detail.cover_url.as_deref(),
                &fetched.final_url,
            );
            let intro_extracted = session.eval_opt(
                &info_input,
                rule_detail.intro.as_deref(),
                &fetched.final_url,
            );
            let kind_extracted =
                session.eval_opt(&info_input, rule_detail.kind.as_deref(), &fetched.final_url);
            let status_extracted = session.eval_opt(
                &info_input,
                rule_detail.status.as_deref(),
                &fetched.final_url,
            );
            let last_chapter_extracted = session.eval_opt(
                &info_input,
                rule_detail.last_chapter.as_deref(),
                &fetched.final_url,
            );
            let word_count_extracted = session.eval_opt(
                &info_input,
                rule_detail.word_count.as_deref(),
                &fetched.final_url,
            );
            let reading_direction = rule_detail
                .reading_direction
                .as_deref()
                .and_then(|s| s.parse::<ReadingDirection>().ok())
                .or_else(|| {
                    let dir_extracted = session.eval_opt(
                        &info_input,
                        rule_detail.reading_direction.as_deref(),
                        &fetched.final_url,
                    );
                    dir_extracted.parse::<ReadingDirection>().ok()
                })
                .or_else(|| Some(source.effective_reading_direction()));
            let toc_raw = session.eval_opt(
                &info_input,
                rule_detail.toc_url.as_deref(),
                &fetched.final_url,
            );

            let toc_trimmed = toc_raw.trim();
            let toc_url = if toc_trimmed.is_empty() {
                fetched.final_url.clone()
            } else {
                resolve_relative_url_preserving_options(toc_trimmed, &fetched.final_url)
            };

            Ok(OnlineComicDetail {
                title: if !name_extracted.is_empty() {
                    name_extracted
                } else {
                    fallback_hint.map(|h| h.name.clone()).unwrap_or_default()
                },
                author: if !author_extracted.is_empty() {
                    author_extracted
                } else {
                    fallback_hint.map(|h| h.author.clone()).unwrap_or_default()
                },
                comic_url: comic_url.to_string(),
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
                status: (!status_extracted.is_empty()).then_some(status_extracted),
                last_chapter: (!last_chapter_extracted.is_empty())
                    .then_some(last_chapter_extracted),
                word_count: (!word_count_extracted.is_empty()).then_some(word_count_extracted),
                reading_direction,
            })
        })();

        match res {
            Ok(detail) => {
                let (final_state, diagnostics) = session.finalize_state();
                Ok(SourceCallOutput {
                    data: detail,
                    state: final_state,
                    diagnostics,
                })
            }
            Err(err) => Err(SourceCallError::new(err, session.snapshot_diagnostics())),
        }
    }

    pub fn fetch_chapters_sync(
        &self,
        source: &ComicSourceSpec,
        state: SourceRuntimeState,
        toc_url: &str,
        comic_ctx: &ComicJsContext,
    ) -> Result<SourceCallOutput<Vec<OnlineComicChapter>>, String> {
        self.fetch_chapters_sync_with_control(
            source,
            state,
            toc_url,
            comic_ctx,
            self.js_timeout,
            Arc::new(AtomicBool::new(false)),
        )
        .map_err(Into::into)
    }

    pub fn fetch_chapters_sync_with_control(
        &self,
        source: &ComicSourceSpec,
        state: SourceRuntimeState,
        toc_url: &str,
        comic_ctx: &ComicJsContext,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<Vec<OnlineComicChapter>>, SourceCallError> {
        let mut session = self
            .build_lazy_session(source, state, op_timeout, cancel_flag)
            .map_err(|e| SourceCallError::new(e, ExecutionDiagnostics::default()))?;
        match session.fetch_chapters(toc_url, comic_ctx) {
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

    pub fn fetch_pages_sync(
        &self,
        source: &ComicSourceSpec,
        state: SourceRuntimeState,
        chapter_url: &str,
        comic_ctx: &ComicJsContext,
        chapter_ctx: &ComicChapterJsContext,
    ) -> Result<SourceCallOutput<Vec<String>>, String> {
        self.fetch_pages_sync_with_control(
            source,
            state,
            chapter_url,
            comic_ctx,
            chapter_ctx,
            self.js_timeout,
            Arc::new(AtomicBool::new(false)),
        )
        .map_err(Into::into)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn fetch_pages_sync_with_control(
        &self,
        source: &ComicSourceSpec,
        state: SourceRuntimeState,
        chapter_url: &str,
        comic_ctx: &ComicJsContext,
        chapter_ctx: &ComicChapterJsContext,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<Vec<String>>, SourceCallError> {
        let mut session = self
            .build_lazy_session(source, state, op_timeout, cancel_flag)
            .map_err(|e| SourceCallError::new(e, ExecutionDiagnostics::default()))?;
        match session.fetch_pages(chapter_url, comic_ctx, chapter_ctx) {
            Ok(pages) => {
                let (final_state, diagnostics) = session.finalize_state();
                Ok(SourceCallOutput {
                    data: pages,
                    state: final_state,
                    diagnostics,
                })
            }
            Err(err) => Err(SourceCallError::new(err, session.snapshot_diagnostics())),
        }
    }

    pub fn run_login_action_checked(
        &self,
        source: &ComicSourceSpec,
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

    pub fn run_login_action_sync(
        &self,
        source: &ComicSourceSpec,
        state: SourceRuntimeState,
        login_info: HashMap<String, String>,
        action_js: Option<&str>,
    ) -> Result<SourceCallOutput<String>, String> {
        self.run_login_action_checked(source, state, login_info, action_js)
            .map_err(|e| e.message)
    }
}

impl ComicSourceEnginePort for ComicSourceEngine {
    fn parse_explore_categories_with_control(
        &self,
        source: &ComicSourceSpec,
        state: SourceRuntimeState,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<Vec<ExploreCategoryItem>>, SourceCallError> {
        ComicSourceEngine::parse_explore_categories_with_control(
            self,
            source,
            state,
            op_timeout,
            cancel_flag,
        )
    }

    fn search_sync_with_control(
        &self,
        source: &ComicSourceSpec,
        state: SourceRuntimeState,
        keyword: &str,
        page: u32,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<Vec<OnlineComicItem>>, SourceCallError> {
        ComicSourceEngine::search_sync_with_control(
            self,
            source,
            state,
            keyword,
            page,
            op_timeout,
            cancel_flag,
        )
    }

    fn explore_sync_with_categories_and_control(
        &self,
        source: &ComicSourceSpec,
        state: SourceRuntimeState,
        explore_url: &str,
        page: u32,
        precomputed_categories: Option<&[ExploreCategoryItem]>,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<Vec<OnlineComicItem>>, SourceCallError> {
        ComicSourceEngine::explore_sync_with_categories_and_control(
            self,
            source,
            state,
            explore_url,
            page,
            precomputed_categories,
            op_timeout,
            cancel_flag,
        )
    }

    fn fetch_detail_sync_with_control(
        &self,
        source: &ComicSourceSpec,
        state: SourceRuntimeState,
        comic_url: &str,
        fallback_hint: Option<&ComicJsContext>,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<OnlineComicDetail>, SourceCallError> {
        ComicSourceEngine::fetch_detail_sync_with_control(
            self,
            source,
            state,
            comic_url,
            fallback_hint,
            op_timeout,
            cancel_flag,
        )
    }

    fn fetch_chapters_sync_with_control(
        &self,
        source: &ComicSourceSpec,
        state: SourceRuntimeState,
        toc_url: &str,
        comic_ctx: &ComicJsContext,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<Vec<OnlineComicChapter>>, SourceCallError> {
        ComicSourceEngine::fetch_chapters_sync_with_control(
            self,
            source,
            state,
            toc_url,
            comic_ctx,
            op_timeout,
            cancel_flag,
        )
    }

    fn fetch_pages_sync_with_control(
        &self,
        source: &ComicSourceSpec,
        state: SourceRuntimeState,
        chapter_url: &str,
        comic_ctx: &ComicJsContext,
        chapter_ctx: &ComicChapterJsContext,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<Vec<String>>, SourceCallError> {
        ComicSourceEngine::fetch_pages_sync_with_control(
            self,
            source,
            state,
            chapter_url,
            comic_ctx,
            chapter_ctx,
            op_timeout,
            cancel_flag,
        )
    }

    fn run_login_action_sync(
        &self,
        source: &ComicSourceSpec,
        state: SourceRuntimeState,
        login_info: HashMap<String, String>,
        action_js: Option<&str>,
    ) -> Result<SourceCallOutput<String>, SourceCallError> {
        self.run_login_action_checked(source, state, login_info, action_js)
    }
}

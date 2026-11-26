mod rules;
mod template;

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use xmedia_source::http::{
    HttpClientConfig, LegadoHttpClient, build_sanitized_headers, resolve_relative_url,
};
use xmedia_source::js_runtime::{JsSandboxConfig, ReusableSourceSession};
use xmedia_source::rule::strip_js_wrapper;

use crate::application::ports::VideoSourceEnginePort;
use crate::domain::online::{
    ExecutionDiagnostics, OnlineVideoCategory, OnlineVideoDetail, OnlineVideoPlayResult,
    OnlineVideoSummary, SourceCallError, SourceCallOutput, SourceMetadata, SourceRuntimeState,
    VideoBrowseOutput, VideoSourceBackend, VideoSourceSpec,
};
use crate::infra::source::applecms;

use self::rules::{
    eval_rule_browse, eval_rule_categories, eval_rule_detail, eval_rule_play, eval_rule_search,
    is_http_url,
};

#[derive(Debug, Clone)]
pub struct VideoSourceEngine {
    http_config: HttpClientConfig,
    js_timeout: Duration,
}

impl Default for VideoSourceEngine {
    fn default() -> Self {
        Self {
            http_config: HttpClientConfig::default(),
            js_timeout: Duration::from_secs(45),
        }
    }
}

const VIDEO_JS_BOOTSTRAP: &str = r#"
globalThis.video = {
    vodId: '',
    title: '',
    detailUrl: '',
    playUrl: '',
    groupName: ''
};
"#;

impl VideoSourceEngine {
    pub fn new(http_config: HttpClientConfig, js_timeout: Duration) -> Self {
        Self {
            http_config,
            js_timeout,
        }
    }

    fn build_session(
        &self,
        source: &VideoSourceSpec,
        mut initial_state: SourceRuntimeState,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<ReusableSourceSession, String> {
        let base_url = source.effective_source_url();
        initial_state.ensure_android_id(base_url);
        let cookies_arc = Arc::new(Mutex::new(initial_state.cookies.clone()));
        let state_arc = Arc::new(Mutex::new(initial_state));

        let metadata = SourceMetadata::from(source);
        let http = LegadoHttpClient::new(
            self.http_config.clone(),
            cookies_arc.clone(),
            metadata.header.as_deref(),
            metadata.enabled_cookie_jar,
        )?;
        let mut session = ReusableSourceSession::new(
            metadata,
            http,
            state_arc,
            cookies_arc,
            JsSandboxConfig::with_timeout(self.js_timeout),
            op_timeout,
            cancel_flag,
        );
        session.add_bootstrap_snippet(VIDEO_JS_BOOTSTRAP);
        Ok(session)
    }

    pub fn fetch_categories_sync_with_control(
        &self,
        source: &VideoSourceSpec,
        state: SourceRuntimeState,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<Vec<OnlineVideoCategory>>, SourceCallError> {
        let mut session = self
            .build_session(source, state, op_timeout, cancel_flag)
            .map_err(|e| SourceCallError::new(e, ExecutionDiagnostics::default()))?;

        let res = match source.backend() {
            VideoSourceBackend::Applecms { .. } => {
                applecms::fetch_categories(source, |u, b| session.fetch(u, b))
            }
            VideoSourceBackend::Rule { categories, .. } => {
                eval_rule_categories(&mut session, source.effective_source_url(), categories)
            }
        };

        finalize_session_result(session, res)
    }

    pub fn search_sync_with_control(
        &self,
        source: &VideoSourceSpec,
        state: SourceRuntimeState,
        keyword: &str,
        page: u32,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<Vec<OnlineVideoSummary>>, SourceCallError> {
        let mut session = self
            .build_session(source, state, op_timeout, cancel_flag)
            .map_err(|e| SourceCallError::new(e, ExecutionDiagnostics::default()))?;

        let page_num = page.max(1);
        let res = match source.backend() {
            VideoSourceBackend::Applecms { .. } => {
                applecms::search(source, |u, b| session.fetch(u, b), keyword, page_num)
            }
            VideoSourceBackend::Rule {
                search_url,
                search_rule,
                ..
            } => {
                if !source.searchable {
                    Err(format!("影视源「{}」不支持关键词搜索", source.source_name))
                } else {
                    eval_rule_search(
                        &mut session,
                        &source.source_name,
                        source.effective_source_url(),
                        search_url,
                        search_rule,
                        keyword,
                        page_num,
                    )
                }
            }
        };

        finalize_session_result(session, res)
    }

    pub fn browse_sync_with_control(
        &self,
        source: &VideoSourceSpec,
        state: SourceRuntimeState,
        category_id: Option<&str>,
        page: u32,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<VideoBrowseOutput>, SourceCallError> {
        let mut session = self
            .build_session(source, state, op_timeout, cancel_flag)
            .map_err(|e| SourceCallError::new(e, ExecutionDiagnostics::default()))?;

        let page_num = page.max(1);
        let res = match source.backend() {
            VideoSourceBackend::Applecms { .. } => {
                applecms::browse(source, |u, b| session.fetch(u, b), category_id, page_num).map(
                    |p| VideoBrowseOutput {
                        items: p.items,
                        total: p.total,
                        page: p.page,
                        page_count: p.page_count,
                    },
                )
            }
            VideoSourceBackend::Rule {
                explore_url,
                explore_rule,
                ..
            } => eval_rule_browse(
                &mut session,
                &source.source_name,
                source.effective_source_url(),
                explore_url,
                explore_rule,
                category_id,
                page_num,
            ),
        };

        finalize_session_result(session, res)
    }

    pub fn fetch_detail_sync_with_control(
        &self,
        source: &VideoSourceSpec,
        state: SourceRuntimeState,
        video_id: Option<&str>,
        detail_url: Option<&str>,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<OnlineVideoDetail>, SourceCallError> {
        let mut session = self
            .build_session(source, state, op_timeout, cancel_flag)
            .map_err(|e| SourceCallError::new(e, ExecutionDiagnostics::default()))?;

        let res = match source.backend() {
            VideoSourceBackend::Applecms { .. } => {
                applecms::fetch_detail(source, |u, b| session.fetch(u, b), video_id, detail_url)
            }
            VideoSourceBackend::Rule { detail_rule, .. } => eval_rule_detail(
                &mut session,
                &source.source_name,
                source.effective_source_url(),
                detail_rule,
                video_id,
                detail_url,
            ),
        };

        finalize_session_result(session, res)
    }

    pub fn resolve_play_sync_with_control(
        &self,
        source: &VideoSourceSpec,
        state: SourceRuntimeState,
        play_url: &str,
        group_name: Option<&str>,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<OnlineVideoPlayResult>, SourceCallError> {
        let mut session = self
            .build_session(source, state, op_timeout, cancel_flag)
            .map_err(|e| SourceCallError::new(e, ExecutionDiagnostics::default()))?;

        let trimmed_play = play_url.trim();
        let base_url = source.effective_source_url();
        let effective_media_headers = source.build_media_headers(base_url, None);
        let headers_applied = !build_sanitized_headers(&effective_media_headers).is_empty();

        let res: Result<OnlineVideoPlayResult, String> = if trimmed_play.is_empty() {
            Err("play_url 不能为空".into())
        } else {
            let resolved_input_play = resolve_relative_url(trimmed_play, base_url);
            match source.backend() {
                VideoSourceBackend::Rule {
                    play_rule: Some(rule_play),
                    ..
                } => eval_rule_play(
                    &mut session,
                    base_url,
                    rule_play,
                    resolved_input_play,
                    group_name,
                    headers_applied,
                ),
                _ => {
                    if !is_http_url(&resolved_input_play) {
                        Err("未能解析出有效的 HTTP(S) 视频流地址".into())
                    } else {
                        let stype = applecms::infer_stream_type(&resolved_input_play).to_string();
                        Ok(OnlineVideoPlayResult {
                            stream_url: resolved_input_play,
                            proxy_stream_url: String::new(),
                            stream_type: stype,
                            headers_applied,
                            subtitles: Vec::new(),
                        })
                    }
                }
            }
        };

        finalize_session_result(session, res)
    }

    pub fn run_login_action_sync(
        &self,
        source: &VideoSourceSpec,
        mut state: SourceRuntimeState,
        login_info: HashMap<String, String>,
        action: Option<&str>,
    ) -> Result<SourceCallOutput<String>, String> {
        state.login_info = login_info.clone();
        let mut session = self.build_session(
            source,
            state,
            self.js_timeout,
            Arc::new(AtomicBool::new(false)),
        )?;

        let raw_login = source.login_url.as_deref().unwrap_or("").trim();
        let login_url_js = if is_http_url(raw_login) {
            ""
        } else {
            strip_js_wrapper(raw_login)
        };
        let login_check_stripped = source
            .login_check_js
            .as_deref()
            .map(strip_js_wrapper)
            .filter(|s| !s.is_empty());
        let effective_action = action
            .map(strip_js_wrapper)
            .filter(|s| !s.is_empty())
            .or(login_check_stripped);
        let result = session.eval_login_script(login_url_js, effective_action, &login_info)?;
        let (final_state, diagnostics) = session.finalize_state();
        Ok(SourceCallOutput {
            data: result,
            state: final_state,
            diagnostics,
        })
    }
}

fn finalize_session_result<T>(
    session: ReusableSourceSession,
    res: Result<T, String>,
) -> Result<SourceCallOutput<T>, SourceCallError> {
    let (final_state, diagnostics) = session.finalize_state();
    match res {
        Ok(data) => Ok(SourceCallOutput {
            data,
            state: final_state,
            diagnostics,
        }),
        Err(msg) => Err(SourceCallError::new(msg, diagnostics)),
    }
}

impl VideoSourceEnginePort for VideoSourceEngine {
    fn fetch_categories_sync_with_control(
        &self,
        source: &VideoSourceSpec,
        state: SourceRuntimeState,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<Vec<OnlineVideoCategory>>, SourceCallError> {
        VideoSourceEngine::fetch_categories_sync_with_control(
            self,
            source,
            state,
            op_timeout,
            cancel_flag,
        )
    }

    fn search_sync_with_control(
        &self,
        source: &VideoSourceSpec,
        state: SourceRuntimeState,
        keyword: &str,
        page: u32,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<Vec<OnlineVideoSummary>>, SourceCallError> {
        VideoSourceEngine::search_sync_with_control(
            self,
            source,
            state,
            keyword,
            page,
            op_timeout,
            cancel_flag,
        )
    }

    fn browse_sync_with_control(
        &self,
        source: &VideoSourceSpec,
        state: SourceRuntimeState,
        category_id: Option<&str>,
        page: u32,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<VideoBrowseOutput>, SourceCallError> {
        VideoSourceEngine::browse_sync_with_control(
            self,
            source,
            state,
            category_id,
            page,
            op_timeout,
            cancel_flag,
        )
    }

    fn fetch_detail_sync_with_control(
        &self,
        source: &VideoSourceSpec,
        state: SourceRuntimeState,
        video_id: Option<&str>,
        detail_url: Option<&str>,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<OnlineVideoDetail>, SourceCallError> {
        VideoSourceEngine::fetch_detail_sync_with_control(
            self,
            source,
            state,
            video_id,
            detail_url,
            op_timeout,
            cancel_flag,
        )
    }

    fn resolve_play_sync_with_control(
        &self,
        source: &VideoSourceSpec,
        state: SourceRuntimeState,
        play_url: &str,
        group_name: Option<&str>,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<OnlineVideoPlayResult>, SourceCallError> {
        VideoSourceEngine::resolve_play_sync_with_control(
            self,
            source,
            state,
            play_url,
            group_name,
            op_timeout,
            cancel_flag,
        )
    }

    fn run_login_action_sync(
        &self,
        source: &VideoSourceSpec,
        state: SourceRuntimeState,
        login_info: HashMap<String, String>,
        action: Option<&str>,
    ) -> Result<SourceCallOutput<String>, String> {
        VideoSourceEngine::run_login_action_sync(self, source, state, login_info, action)
    }
}

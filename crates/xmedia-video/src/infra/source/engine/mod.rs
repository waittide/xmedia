mod rules;

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use xmedia_source::http::{
    HttpClientConfig, HttpFetchResult, LegadoHttpClient, build_sanitized_headers,
    eval_json_options_template, has_suspicious_option_chars, resolve_relative_url,
    resolve_relative_url_preserving_options, split_legado_url_and_raw_options,
    split_legado_url_template_and_options, substitute_legado_options_vars,
    substitute_legado_url_template_vars, substitute_url_part_vars, verify_js_pipeline_url_options,
};
use xmedia_source::js_runtime::{JsSandboxConfig, ReusableSourceSession};
use xmedia_source::rule::{RuleValue, split_js_pipeline, strip_js_wrapper};

use crate::application::ports::VideoSourceEnginePort;
use crate::domain::online::{
    ExecutionDiagnostics, OnlineVideoCategory, OnlineVideoDetail, OnlineVideoPlayResult,
    OnlineVideoSummary, SourceCallError, SourceCallOutput, SourceMetadata, SourceRuntimeState,
    VideoBrowseOutput, VideoJsContext, VideoSourceFormat, VideoSourceSpec,
};
use crate::infra::source::applecms;

use self::rules::{
    eval_play_subtitles, eval_rule_categories, eval_rule_detail, eval_video_summary_list,
    is_http_url, non_empty_opt,
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

fn template_needs_js(raw_tpl: &str) -> bool {
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
        if !matches!(
            expr,
            "key" | "page" | "catId" | "vodId" | "playUrl" | "groupName"
        ) {
            return true;
        }
        rest = &after_open[close + 2..];
    }
    false
}

fn detect_passthrough_url_var<'a>(
    url_tpl: &str,
    vars: &'a [(&'a str, &'a str)],
) -> Option<&'a str> {
    let trimmed = url_tpl.trim();
    if let Some(inner) = trimmed
        .strip_prefix("{{")
        .and_then(|s| s.strip_suffix("}}"))
    {
        let name = inner.trim();
        if !name.contains("{{") && !name.contains("}}") && name != "key" && name.ends_with("Url") {
            return vars.iter().find(|(k, _)| *k == name).map(|(k, _)| *k);
        }
    }
    None
}

fn substitute_template_vars(raw_tpl: &str, vars: &[(&str, &str)]) -> Result<String, String> {
    let (url_part, _) = split_legado_url_template_and_options(raw_tpl);
    let passthrough = detect_passthrough_url_var(url_part, vars);
    substitute_legado_url_template_vars(raw_tpl, vars, passthrough).map_err(|e| e.to_string())
}

pub(super) fn remaining_fetch_timeout(session: &ReusableSourceSession) -> Result<Duration, String> {
    session.check_active()?;
    let remaining = session.remaining_budget();
    if remaining.is_zero() {
        return Err("执行超过单次操作总超时上限 (interrupted)".into());
    }
    Ok(remaining)
}

pub(super) fn fetch_session_api(
    session: &ReusableSourceSession,
    target_url: &str,
    base_url: &str,
) -> Result<HttpFetchResult, String> {
    let remaining = remaining_fetch_timeout(session)?;
    session
        .http()
        .fetch_with_timeout(target_url, base_url, Some(remaining))
}

impl VideoSourceEngine {
    pub fn new(http_config: HttpClientConfig, js_timeout: Duration) -> Self {
        Self {
            http_config,
            js_timeout,
        }
    }

    pub fn http_config(&self) -> &HttpClientConfig {
        &self.http_config
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

        let http = LegadoHttpClient::new(
            self.http_config.clone(),
            cookies_arc.clone(),
            source.header.as_deref(),
            source.enabled_cookie_jar,
        )?;
        let metadata = SourceMetadata::from(source);
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

    pub(super) fn set_video_globals(
        session: &mut ReusableSourceSession,
        ctx: &VideoJsContext,
        cat_id: Option<&str>,
    ) -> Result<(), String> {
        session.set_domain_global("catId", serde_json::json!(cat_id.unwrap_or("")), false)?;
        session.set_domain_global("vodId", serde_json::json!(ctx.vod_id), false)?;
        session.set_domain_global("playUrl", serde_json::json!(ctx.play_url), false)?;
        session.set_domain_global("groupName", serde_json::json!(ctx.group_name), false)?;
        session.set_domain_global(
            "video",
            serde_json::json!({
                "vodId": ctx.vod_id,
                "title": ctx.title,
                "detailUrl": ctx.detail_url,
                "playUrl": ctx.play_url,
                "groupName": ctx.group_name,
            }),
            true,
        )?;
        Ok(())
    }

    fn eval_js_pipeline_url_guarded(
        session: &mut ReusableSourceSession,
        tpl: &str,
        base_url: &str,
        js_ctx: &VideoJsContext,
        cat_id: Option<&str>,
        keyword: &str,
        page: u32,
    ) -> Result<String, String> {
        let input = RuleValue::Text(String::new());
        let out = session.eval_string(&input, tpl, base_url)?;
        let caller_vals = [
            keyword,
            cat_id.unwrap_or(""),
            js_ctx.vod_id.as_str(),
            js_ctx.detail_url.as_str(),
            js_ctx.play_url.as_str(),
            js_ctx.group_name.as_str(),
        ];
        if has_suspicious_option_chars(&caller_vals)
            && split_legado_url_and_raw_options(out.trim()).1.is_some()
        {
            let probe_keyword = if keyword.is_empty() {
                ""
            } else {
                "__xmedia_probe__"
            };
            let probe_cat_id = cat_id.map(|c| if c.is_empty() { "" } else { "__xmedia_probe__" });
            let probe_ctx = VideoJsContext {
                vod_id: if js_ctx.vod_id.is_empty() {
                    String::new()
                } else {
                    "__xmedia_probe__".into()
                },
                title: js_ctx.title.clone(),
                detail_url: if js_ctx.detail_url.is_empty() {
                    String::new()
                } else {
                    "__xmedia_probe__".into()
                },
                play_url: if js_ctx.play_url.is_empty() {
                    String::new()
                } else {
                    "__xmedia_probe__".into()
                },
                group_name: if js_ctx.group_name.is_empty() {
                    String::new()
                } else {
                    "__xmedia_probe__".into()
                },
            };
            session.set_search_vars(probe_keyword, page)?;
            Self::set_video_globals(session, &probe_ctx, probe_cat_id)?;
            let probe_res = session.eval_string(&input, tpl, base_url);

            let _ = session.set_search_vars(keyword, page);
            let _ = Self::set_video_globals(session, js_ctx, cat_id);

            let probe_out = probe_res?;
            verify_js_pipeline_url_options(&out, &probe_out, "请求 URL")?;
        }
        Ok(out)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn eval_url_template(
        session: &mut ReusableSourceSession,
        raw_tpl: &str,
        base_url: &str,
        vars: &[(&str, &str)],
        js_ctx: &VideoJsContext,
        cat_id: Option<&str>,
        keyword: &str,
        page: u32,
    ) -> Result<String, String> {
        let evaluated = if template_needs_js(raw_tpl) {
            let (url_tpl, opt_tpl) = split_legado_url_template_and_options(raw_tpl);
            if let Some(raw_opts) = opt_tpl {
                let rendered_url = if template_needs_js(url_tpl) {
                    if split_js_pipeline(url_tpl).is_some() {
                        Self::eval_js_pipeline_url_guarded(
                            session, url_tpl, base_url, js_ctx, cat_id, keyword, page,
                        )?
                    } else {
                        let passthrough = detect_passthrough_url_var(url_tpl, vars);
                        let pre_url = substitute_url_part_vars(url_tpl, vars, passthrough);
                        let input = RuleValue::Text(String::new());
                        let out_url = session.eval_string(&input, &pre_url, base_url)?;
                        if split_legado_url_and_raw_options(&out_url).1.is_some() {
                            return Err("请求 URL 模板不允许通过变量注入请求配置".into());
                        }
                        out_url
                    }
                } else {
                    let passthrough = detect_passthrough_url_var(url_tpl, vars);
                    substitute_url_part_vars(url_tpl, vars, passthrough)
                };
                let rendered_opts = if template_needs_js(raw_opts) {
                    eval_json_options_template(
                        raw_opts,
                        vars,
                        |expr_tpl| {
                            let input = RuleValue::Text(String::new());
                            session.eval_string(&input, expr_tpl, base_url)
                        },
                        "请求 URL",
                    )?
                } else {
                    substitute_legado_options_vars(raw_opts, vars).map_err(|e| e.to_string())?
                };
                format!("{rendered_url},{rendered_opts}")
            } else if split_js_pipeline(raw_tpl).is_some() {
                Self::eval_js_pipeline_url_guarded(
                    session, raw_tpl, base_url, js_ctx, cat_id, keyword, page,
                )?
            } else {
                let passthrough = detect_passthrough_url_var(raw_tpl, vars);
                let pre_tpl = substitute_url_part_vars(raw_tpl, vars, passthrough);
                let input = RuleValue::Text(String::new());
                let out = session.eval_string(&input, &pre_tpl, base_url)?;
                if split_legado_url_and_raw_options(&out).1.is_some() {
                    return Err("请求 URL 模板不允许通过变量注入请求配置".into());
                }
                out
            }
        } else {
            substitute_template_vars(raw_tpl, vars)?
        };
        let trimmed = evaluated.trim();
        if trimmed.is_empty() {
            return Err("请求 URL 模板求值结果为空".into());
        }
        Ok(resolve_relative_url_preserving_options(trimmed, base_url))
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

        let res = match source.effective_format() {
            VideoSourceFormat::ApplecmsV10 => remaining_fetch_timeout(&session).and_then(|t| {
                applecms::fetch_categories_with_timeout(source, session.http(), Some(t))
            }),
            VideoSourceFormat::Rule => eval_rule_categories(&mut session, source),
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
        let res = match source.effective_format() {
            VideoSourceFormat::ApplecmsV10 => remaining_fetch_timeout(&session).and_then(|t| {
                applecms::search_with_timeout(source, session.http(), keyword, page_num, Some(t))
            }),
            VideoSourceFormat::Rule => (|| {
                if !source.searchable {
                    return Err(format!("影视源「{}」不支持关键词搜索", source.source_name));
                }
                let rule_search = source
                    .rule_search
                    .as_ref()
                    .ok_or_else(|| format!("影视源「{}」未配置 ruleSearch", source.source_name))?;
                let search_tpl = source
                    .search_url
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| format!("影视源「{}」未配置 searchUrl", source.source_name))?;

                let base_url = source.effective_source_url();
                let ctx = VideoJsContext::default();
                Self::set_video_globals(&mut session, &ctx, None)?;
                session.set_search_vars(keyword, page_num)?;

                let page_str = page_num.to_string();
                let url = Self::eval_url_template(
                    &mut session,
                    search_tpl,
                    base_url,
                    &[("key", keyword), ("page", &page_str)],
                    &ctx,
                    None,
                    keyword,
                    page_num,
                )?;
                let fetched = fetch_session_api(&session, &url, base_url)?;
                let raw_input = RuleValue::from_raw(fetched.body);
                eval_video_summary_list(&mut session, &raw_input, rule_search, &fetched.final_url)
            })(),
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
        let res = match source.effective_format() {
            VideoSourceFormat::ApplecmsV10 => remaining_fetch_timeout(&session).and_then(|t| {
                applecms::browse_with_timeout(
                    source,
                    session.http(),
                    category_id,
                    page_num,
                    Some(t),
                )
                .map(|p| VideoBrowseOutput {
                    items: p.items,
                    total: p.total,
                    page: p.page,
                    page_count: p.page_count,
                })
            }),
            VideoSourceFormat::Rule => (|| {
                let explore_tpl = source
                    .explore_url
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| format!("影视源「{}」未配置 exploreUrl", source.source_name))?;
                let rule = source
                    .rule_explore
                    .as_ref()
                    .or(source.rule_search.as_ref())
                    .ok_or_else(|| {
                        format!(
                            "影视源「{}」未配置 ruleExplore / ruleSearch",
                            source.source_name
                        )
                    })?;

                let base_url = source.effective_source_url();
                let ctx = VideoJsContext::default();
                let clean_cat = category_id.map(str::trim).unwrap_or("");
                Self::set_video_globals(&mut session, &ctx, Some(clean_cat))?;
                session.set_search_vars("", page_num)?;

                let page_str = page_num.to_string();
                let url = Self::eval_url_template(
                    &mut session,
                    explore_tpl,
                    base_url,
                    &[("catId", clean_cat), ("page", &page_str)],
                    &ctx,
                    Some(clean_cat),
                    "",
                    page_num,
                )?;
                let fetched = fetch_session_api(&session, &url, base_url)?;
                let raw_input = RuleValue::from_raw(fetched.body);
                let items =
                    eval_video_summary_list(&mut session, &raw_input, rule, &fetched.final_url)?;
                Ok(VideoBrowseOutput {
                    items,
                    total: None,
                    page: page_num,
                    page_count: None,
                })
            })(),
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

        let res = match source.effective_format() {
            VideoSourceFormat::ApplecmsV10 => remaining_fetch_timeout(&session).and_then(|t| {
                applecms::fetch_detail_with_timeout(
                    source,
                    session.http(),
                    video_id,
                    detail_url,
                    Some(t),
                )
            }),
            VideoSourceFormat::Rule => eval_rule_detail(&mut session, source, video_id, detail_url),
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
        let mut effective_media_headers = source.parse_media_header_map();
        effective_media_headers.retain(|k, _| !k.eq_ignore_ascii_case("range"));
        let headers_applied = !build_sanitized_headers(&effective_media_headers).is_empty();

        let res: Result<OnlineVideoPlayResult, String> = (|| {
            if trimmed_play.is_empty() {
                return Err("play_url 不能为空".into());
            }
            let base_url = source.effective_source_url();
            let resolved_input_play = resolve_relative_url(trimmed_play, base_url);

            if source.effective_format() == VideoSourceFormat::ApplecmsV10
                || source.rule_play.is_none()
            {
                if !is_http_url(&resolved_input_play) {
                    return Err("未能解析出有效的 HTTP(S) 视频流地址".into());
                }
                let stype = applecms::infer_stream_type(&resolved_input_play).to_string();
                return Ok(OnlineVideoPlayResult {
                    stream_url: resolved_input_play,
                    proxy_stream_url: String::new(),
                    stream_type: stype,
                    headers_applied,
                    subtitles: Vec::new(),
                });
            }

            let rule_play = source.rule_play.as_ref().expect("checked above");
            let g_name = group_name.map(str::trim).unwrap_or("");
            let ctx = VideoJsContext {
                play_url: resolved_input_play.clone(),
                group_name: g_name.to_string(),
                ..Default::default()
            };
            Self::set_video_globals(&mut session, &ctx, None)?;
            session.set_search_vars("", 1)?;

            let mut target_input = RuleValue::Text(resolved_input_play.clone());
            let mut eval_base_url = base_url.to_string();
            if let Some(ref req_tpl) = rule_play.request_url
                && !req_tpl.trim().is_empty()
            {
                let req_url = Self::eval_url_template(
                    &mut session,
                    req_tpl,
                    base_url,
                    &[("playUrl", &resolved_input_play), ("groupName", g_name)],
                    &ctx,
                    None,
                    "",
                    1,
                )?;
                let fetched = fetch_session_api(&session, &req_url, base_url)?;
                eval_base_url = fetched.final_url;
                target_input = RuleValue::from_raw(fetched.body);
            }

            let stream_raw = if let Some(ref stream_rule) = rule_play.stream_url
                && !stream_rule.trim().is_empty()
            {
                session.eval_string(&target_input, stream_rule, &eval_base_url)?
            } else {
                resolved_input_play
            };
            let stream_url = resolve_relative_url(stream_raw.trim(), &eval_base_url);
            if stream_url.is_empty() || !is_http_url(&stream_url) {
                return Err("未能解析出有效的 HTTP(S) 视频流地址".into());
            }

            let raw_type = non_empty_opt(session.eval_opt(
                &target_input,
                rule_play.stream_type.as_deref(),
                &eval_base_url,
            ));
            let stream_type = match raw_type.as_deref().map(str::to_ascii_lowercase).as_deref() {
                Some("hls") => "hls".to_string(),
                Some("mp4") => "mp4".to_string(),
                _ => applecms::infer_stream_type(&stream_url).to_string(),
            };

            let subtitles = if let Some(ref sub_rule) = rule_play.subtitles
                && !sub_rule.trim().is_empty()
            {
                eval_play_subtitles(&mut session, &target_input, sub_rule, &eval_base_url)
            } else {
                Vec::new()
            };

            Ok(OnlineVideoPlayResult {
                stream_url,
                proxy_stream_url: String::new(),
                stream_type,
                headers_applied,
                subtitles,
            })
        })();

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
        let effective_action = match action.map(strip_js_wrapper).filter(|s| !s.is_empty()) {
            Some(act) => Some(act),
            None => login_check_stripped,
        };
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

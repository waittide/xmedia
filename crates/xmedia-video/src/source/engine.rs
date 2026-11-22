use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use xmedia_source::http::{
    HttpClientConfig, LegadoHttpClient, json_structure_signature, resolve_relative_url,
    resolve_relative_url_preserving_options, split_legado_url_and_raw_options,
    split_legado_url_template_and_options, substitute_legado_options_vars,
    substitute_legado_url_template_vars, substitute_url_part_vars,
};
use xmedia_source::js_runtime::{JsSandboxConfig, ReusableSourceSession};
use xmedia_source::rule::{RuleValue, rule_needs_js, split_js_pipeline};

use crate::source::applecms;
use crate::source::model::{
    ExecutionDiagnostics, OnlineVideoCategory, OnlineVideoDetail, OnlineVideoEpisode,
    OnlineVideoPlayGroup, OnlineVideoPlayResult, OnlineVideoSubtitle, OnlineVideoSummary,
    SourceCallError, SourceCallOutput, SourceMetadata, SourceRuntimeState, VideoJsContext,
    VideoSourceFormat, VideoSourceSpec, parse_header_json_map,
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

#[derive(Debug, Clone)]
pub struct VideoBrowseOutput {
    pub items: Vec<OnlineVideoSummary>,
    pub total: Option<u64>,
    pub page: u32,
    pub page_count: Option<u32>,
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

    fn set_video_globals(
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

    fn eval_url_template(
        session: &mut ReusableSourceSession,
        raw_tpl: &str,
        base_url: &str,
        vars: &[(&str, &str)],
    ) -> Result<String, String> {
        let evaluated = if template_needs_js(raw_tpl) {
            let (url_tpl, opt_tpl) = split_legado_url_template_and_options(raw_tpl);
            if let Some(raw_opts) = opt_tpl {
                let rendered_url = if template_needs_js(url_tpl) {
                    let input = RuleValue::Text(String::new());
                    session.eval_string(&input, url_tpl, base_url)?
                } else {
                    let passthrough = detect_passthrough_url_var(url_tpl, vars);
                    substitute_url_part_vars(url_tpl, vars, passthrough)
                };
                let rendered_opts = if template_needs_js(raw_opts) {
                    let expected_sig = serde_json::from_str::<serde_json::Value>(raw_opts)
                        .ok()
                        .map(|v| json_structure_signature(&v));
                    let input = RuleValue::Text(String::new());
                    let out_opts = session.eval_string(&input, raw_opts, base_url)?;
                    let parsed_out: serde_json::Value = serde_json::from_str(out_opts.trim())
                        .map_err(|e| format!("请求 URL 配置 JS 求值后 JSON 无效: {e}"))?;
                    if let Some(sig) = expected_sig
                        && json_structure_signature(&parsed_out) != sig
                    {
                        return Err("请求 URL 配置 JSON 结构在模板求值后发生异常变化".into());
                    }
                    out_opts
                } else {
                    substitute_legado_options_vars(raw_opts, vars).map_err(|e| e.to_string())?
                };
                format!("{rendered_url},{rendered_opts}")
            } else {
                let input = RuleValue::Text(String::new());
                let out = session.eval_string(&input, raw_tpl, base_url)?;
                let (_, post_opts) = split_legado_url_and_raw_options(&out);
                if post_opts.is_some() && !raw_tpl.contains("@js:") && !raw_tpl.contains("<js>") {
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

        let res: Result<Vec<OnlineVideoCategory>, String> = match source.effective_format() {
            VideoSourceFormat::ApplecmsV10 => applecms::fetch_categories(source, session.http()),
            VideoSourceFormat::Rule => (|| {
                let Some(cat_url_tpl) = source
                    .categories_url
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                else {
                    return Ok(Vec::new());
                };
                let Some(rule_cat) = source.rule_categories.as_ref() else {
                    return Ok(Vec::new());
                };
                let base_url = source.effective_source_url();
                let ctx = VideoJsContext::default();
                Self::set_video_globals(&mut session, &ctx, None)?;
                session.set_search_vars("", 1)?;
                let url = Self::eval_url_template(&mut session, cat_url_tpl, base_url, &[])?;
                session.check_active()?;
                let fetched = session.http().fetch(&url, base_url)?;
                let raw_input = RuleValue::from_raw(fetched.body);

                let list_rule = rule_cat
                    .list
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| "ruleCategories 缺少 list 规则".to_string())?;
                let items = session.eval_list(&raw_input, list_rule, &fetched.final_url)?;
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    session.check_active()?;
                    let id = session.eval_opt(&item, rule_cat.id.as_deref(), &fetched.final_url);
                    let name =
                        session.eval_opt(&item, rule_cat.name.as_deref(), &fetched.final_url);
                    if id.is_empty() || name.is_empty() {
                        continue;
                    }
                    out.push(OnlineVideoCategory {
                        id,
                        name,
                        parent_id: None,
                    });
                }
                Ok(out)
            })(),
        };

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
        let res: Result<Vec<OnlineVideoSummary>, String> = match source.effective_format() {
            VideoSourceFormat::ApplecmsV10 => {
                applecms::search(source, session.http(), keyword, page_num)
            }
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
                )?;
                session.check_active()?;
                let fetched = session.http().fetch(&url, base_url)?;
                let raw_input = RuleValue::from_raw(fetched.body);
                eval_video_summary_list(&mut session, &raw_input, rule_search, &fetched.final_url)
            })(),
        };

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
        let res: Result<VideoBrowseOutput, String> = match source.effective_format() {
            VideoSourceFormat::ApplecmsV10 => {
                applecms::browse(source, session.http(), category_id, page_num).map(|p| {
                    VideoBrowseOutput {
                        items: p.items,
                        total: p.total,
                        page: p.page,
                        page_count: p.page_count,
                    }
                })
            }
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
                )?;
                session.check_active()?;
                let fetched = session.http().fetch(&url, base_url)?;
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

        let res: Result<OnlineVideoDetail, String> = match source.effective_format() {
            VideoSourceFormat::ApplecmsV10 => {
                applecms::fetch_detail(source, session.http(), video_id, detail_url)
            }
            VideoSourceFormat::Rule => (|| {
                let rule_detail = source
                    .rule_detail
                    .as_ref()
                    .ok_or_else(|| format!("影视源「{}」未配置 ruleDetail", source.source_name))?;

                let base_url = source.effective_source_url();
                let clean_vid = video_id.map(str::trim).unwrap_or("");
                let clean_durl = detail_url.map(str::trim).unwrap_or("");
                let ctx = VideoJsContext {
                    vod_id: clean_vid.to_string(),
                    detail_url: clean_durl.to_string(),
                    ..Default::default()
                };
                Self::set_video_globals(&mut session, &ctx, None)?;
                session.set_search_vars("", 1)?;

                let target_url = if !clean_durl.is_empty() {
                    resolve_relative_url(clean_durl, base_url)
                } else if let Some(ref req_tpl) = rule_detail.request_url
                    && !req_tpl.trim().is_empty()
                {
                    Self::eval_url_template(
                        &mut session,
                        req_tpl,
                        base_url,
                        &[("vodId", clean_vid)],
                    )?
                } else {
                    return Err("缺少 detail_url 且 ruleDetail 未配置 requestUrl".into());
                };

                session.check_active()?;
                let fetched = session.http().fetch(&target_url, base_url)?;
                let mut raw_input = RuleValue::from_raw(fetched.body);
                if let Some(ref init_rule) = rule_detail.init
                    && !init_rule.trim().is_empty()
                {
                    let items = session.eval_list(&raw_input, init_rule, &fetched.final_url)?;
                    if let Some(first) = items.into_iter().next() {
                        raw_input = first;
                    }
                }

                let title =
                    session.eval_opt(&raw_input, rule_detail.name.as_deref(), &fetched.final_url);
                let cover_raw = session.eval_opt(
                    &raw_input,
                    rule_detail.cover_url.as_deref(),
                    &fetched.final_url,
                );
                let cover_url = (!cover_raw.is_empty())
                    .then(|| resolve_relative_url(&cover_raw, &fetched.final_url));
                let intro = non_empty_opt(session.eval_opt(
                    &raw_input,
                    rule_detail.intro.as_deref(),
                    &fetched.final_url,
                ));
                let actors = non_empty_opt(session.eval_opt(
                    &raw_input,
                    rule_detail.actors.as_deref(),
                    &fetched.final_url,
                ));
                let director = non_empty_opt(session.eval_opt(
                    &raw_input,
                    rule_detail.director.as_deref(),
                    &fetched.final_url,
                ));
                let year = non_empty_opt(session.eval_opt(
                    &raw_input,
                    rule_detail.year.as_deref(),
                    &fetched.final_url,
                ));
                let area = non_empty_opt(session.eval_opt(
                    &raw_input,
                    rule_detail.area.as_deref(),
                    &fetched.final_url,
                ));
                let remarks = non_empty_opt(session.eval_opt(
                    &raw_input,
                    rule_detail.remarks.as_deref(),
                    &fetched.final_url,
                ));

                let mut play_groups: Vec<OnlineVideoPlayGroup> = Vec::new();
                if let Some(ref groups_rule) = rule_detail.play_groups
                    && !groups_rule.trim().is_empty()
                {
                    let group_items =
                        session.eval_list(&raw_input, groups_rule, &fetched.final_url)?;
                    for (g_idx, g_item) in group_items.into_iter().enumerate() {
                        let g_name = non_empty_opt(session.eval_opt(
                            &g_item,
                            rule_detail.group_name.as_deref(),
                            &fetched.final_url,
                        ))
                        .unwrap_or_else(|| format!("线路{}", g_idx + 1));

                        if let Some(ref ep_rule) = rule_detail.episodes
                            && !ep_rule.trim().is_empty()
                        {
                            let episodes = eval_episodes_list(
                                &mut session,
                                &g_item,
                                ep_rule,
                                rule_detail.episode_name.as_deref(),
                                rule_detail.episode_url.as_deref(),
                                &fetched.final_url,
                            )?;
                            if !episodes.is_empty() {
                                play_groups.push(OnlineVideoPlayGroup {
                                    name: g_name,
                                    episodes,
                                });
                            }
                        }
                    }
                } else if let Some(ref ep_rule) = rule_detail.episodes
                    && !ep_rule.trim().is_empty()
                {
                    let episodes = eval_episodes_list(
                        &mut session,
                        &raw_input,
                        ep_rule,
                        rule_detail.episode_name.as_deref(),
                        rule_detail.episode_url.as_deref(),
                        &fetched.final_url,
                    )?;
                    if !episodes.is_empty() {
                        let g_name = non_empty_opt(session.eval_opt(
                            &raw_input,
                            rule_detail.group_name.as_deref(),
                            &fetched.final_url,
                        ))
                        .unwrap_or_else(|| "播放列表".to_string());
                        play_groups.push(OnlineVideoPlayGroup {
                            name: g_name,
                            episodes,
                        });
                    }
                } else if let Some(ref play_rule) = rule_detail.play_url
                    && !play_rule.trim().is_empty()
                {
                    let single_play =
                        session.eval_opt(&raw_input, Some(play_rule.as_str()), &fetched.final_url);
                    if !single_play.is_empty() {
                        let resolved_play = resolve_relative_url(&single_play, &fetched.final_url);
                        play_groups.push(OnlineVideoPlayGroup {
                            name: "正片".to_string(),
                            episodes: vec![OnlineVideoEpisode {
                                index: 0,
                                name: "播放".to_string(),
                                play_url: resolved_play,
                            }],
                        });
                    }
                }

                let resolved_id = if !clean_vid.is_empty() {
                    clean_vid.to_string()
                } else {
                    fetched.final_url.clone()
                };

                Ok(OnlineVideoDetail {
                    id: resolved_id,
                    title,
                    cover_url,
                    proxy_cover_url: None,
                    intro,
                    year,
                    area,
                    actors,
                    director,
                    remarks,
                    play_groups,
                })
            })(),
        };

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
        let headers_applied = !source.parse_media_header_map().is_empty();

        let res: Result<OnlineVideoPlayResult, String> = (|| {
            if trimmed_play.is_empty() {
                return Err("play_url 不能为空".into());
            }
            let base_url = source.effective_source_url();
            let resolved_input_play = resolve_relative_url(trimmed_play, base_url);

            if source.effective_format() == VideoSourceFormat::ApplecmsV10
                || source.rule_play.is_none()
            {
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
                )?;
                session.check_active()?;
                let fetched = session.http().fetch(&req_url, base_url)?;
                eval_base_url = fetched.final_url;
                target_input = RuleValue::from_raw(fetched.body);
            }

            let stream_raw = if let Some(ref stream_rule) = rule_play.stream_url
                && !stream_rule.trim().is_empty()
            {
                session.eval_string(&target_input, stream_rule, &eval_base_url)?
            } else {
                resolved_input_play.clone()
            };
            let stream_url = resolve_relative_url(stream_raw.trim(), &eval_base_url);
            if stream_url.is_empty() {
                return Err("未能解析出有效视频流地址".into());
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

            let extra_headers_applied = headers_applied
                || rule_play
                    .headers
                    .as_deref()
                    .is_some_and(|r| !parse_header_json_map(Some(r)).is_empty());

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
                headers_applied: extra_headers_applied,
                subtitles,
            })
        })();

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

        let login_url_js = source.login_url.as_deref().unwrap_or("");
        let result = session.eval_login_script(login_url_js, action, &login_info)?;
        let (final_state, diagnostics) = session.finalize_state();
        Ok(SourceCallOutput {
            data: result,
            state: final_state,
            diagnostics,
        })
    }
}

fn non_empty_opt(s: String) -> Option<String> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn eval_video_summary_list(
    session: &mut ReusableSourceSession,
    raw_input: &RuleValue,
    rule: &crate::source::model::VideoSearchRule,
    base_url: &str,
) -> Result<Vec<OnlineVideoSummary>, String> {
    let list_rule = rule
        .list
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "搜索/浏览规则缺少 list 规则".to_string())?;

    let items = session.eval_list(raw_input, list_rule, base_url)?;
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        session.check_active()?;
        let title = session.eval_opt(&item, rule.name.as_deref(), base_url);
        let detail_raw = session.eval_opt(&item, rule.detail_url.as_deref(), base_url);
        let vod_id_raw = session.eval_opt(&item, rule.vod_id.as_deref(), base_url);
        let detail_url = if detail_raw.is_empty() {
            String::new()
        } else {
            resolve_relative_url(&detail_raw, base_url)
        };
        let vod_id = if !vod_id_raw.is_empty() {
            vod_id_raw
        } else {
            detail_url.clone()
        };
        if title.is_empty() || (vod_id.is_empty() && detail_url.is_empty()) {
            continue;
        }
        let cover_raw = session.eval_opt(&item, rule.cover_url.as_deref(), base_url);
        let cover_url = (!cover_raw.is_empty()).then(|| resolve_relative_url(&cover_raw, base_url));
        out.push(OnlineVideoSummary {
            vod_id,
            title,
            cover_url,
            proxy_cover_url: None,
            remarks: non_empty_opt(session.eval_opt(&item, rule.remarks.as_deref(), base_url)),
            year: non_empty_opt(session.eval_opt(&item, rule.year.as_deref(), base_url)),
            area: non_empty_opt(session.eval_opt(&item, rule.area.as_deref(), base_url)),
            type_name: non_empty_opt(session.eval_opt(&item, rule.type_name.as_deref(), base_url)),
            detail_url,
        });
    }
    Ok(out)
}

fn eval_episodes_list(
    session: &mut ReusableSourceSession,
    input: &RuleValue,
    ep_list_rule: &str,
    ep_name_rule: Option<&str>,
    ep_url_rule: Option<&str>,
    base_url: &str,
) -> Result<Vec<OnlineVideoEpisode>, String> {
    let ep_items = session.eval_list(input, ep_list_rule, base_url)?;
    let mut episodes = Vec::with_capacity(ep_items.len());
    for ep_item in ep_items {
        session.check_active()?;
        let name = non_empty_opt(session.eval_opt(&ep_item, ep_name_rule, base_url))
            .unwrap_or_else(|| format!("第{}集", episodes.len() + 1));
        let url_raw = session.eval_opt(&ep_item, ep_url_rule, base_url);
        if url_raw.is_empty() {
            continue;
        }
        let play_url = resolve_relative_url(&url_raw, base_url);
        let idx = episodes.len();
        episodes.push(OnlineVideoEpisode {
            index: idx,
            name,
            play_url,
        });
    }
    Ok(episodes)
}

fn eval_play_subtitles(
    session: &mut ReusableSourceSession,
    input: &RuleValue,
    sub_rule: &str,
    base_url: &str,
) -> Vec<OnlineVideoSubtitle> {
    let trimmed = sub_rule.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let js_parts = split_js_pipeline(trimmed);
    let use_string_eval = (trimmed.contains("{{") && trimmed.contains("}}") && js_parts.is_none())
        || (js_parts.is_none() && (trimmed.starts_with(':') || trimmed.contains("##")))
        || js_parts.is_some_and(|(_, _, suffix)| !suffix.is_empty() && suffix.contains("##"));

    if !use_string_eval {
        match session.eval_list(input, trimmed, base_url) {
            Ok(items) => {
                let mut out = Vec::new();
                for item in items {
                    match item {
                        RuleValue::Json(val) => {
                            collect_subtitles_from_json_value(&val, base_url, &mut out);
                        }
                        RuleValue::Text(text) => {
                            collect_subtitles_from_text(&text, base_url, &mut out);
                        }
                    }
                }
                return out;
            }
            Err(err) => {
                if rule_needs_js(trimmed) {
                    let mut g = session.diag_arc().lock().unwrap_or_else(|e| e.into_inner());
                    if g.logs.len() < 100 {
                        g.logs.push(format!("字段规则求值失败: {err}"));
                    }
                }
                return Vec::new();
            }
        }
    }

    let raw_subs = session.eval_opt(input, Some(trimmed), base_url);
    parse_subtitles_json(&raw_subs, base_url)
}

fn looks_like_subtitle_url(s: &str) -> bool {
    let trimmed = s.trim();
    if trimmed.is_empty() || trimmed.chars().any(char::is_whitespace) {
        return false;
    }
    if trimmed.starts_with("http://")
        || trimmed.starts_with("https://")
        || trimmed.starts_with('/')
        || trimmed.starts_with("./")
        || trimmed.starts_with("../")
    {
        return true;
    }
    let path_part = trimmed.split(['?', '#']).next().unwrap_or(trimmed);
    let lower = path_part.to_ascii_lowercase();
    lower.ends_with(".vtt")
        || lower.ends_with(".srt")
        || lower.ends_with(".ass")
        || lower.ends_with(".ssa")
}

fn collect_subtitle_object(
    obj: &serde_json::Map<String, serde_json::Value>,
    base_url: &str,
    out: &mut Vec<OnlineVideoSubtitle>,
) {
    let Some(url_raw) = obj.get("url").and_then(|v| v.as_str()) else {
        return;
    };
    let clean_url = url_raw.trim();
    if clean_url.is_empty() {
        return;
    }
    let label = obj
        .get("label")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("字幕 {}", out.len() + 1));
    let lang = obj
        .get("lang")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string);
    out.push(OnlineVideoSubtitle {
        label,
        lang,
        url: resolve_relative_url(clean_url, base_url),
    });
}

fn collect_subtitles_from_json_value(
    val: &serde_json::Value,
    base_url: &str,
    out: &mut Vec<OnlineVideoSubtitle>,
) {
    match val {
        serde_json::Value::Array(arr) => {
            for item in arr {
                collect_subtitles_from_json_value(item, base_url, out);
            }
        }
        serde_json::Value::Object(map) => {
            collect_subtitle_object(map, base_url, out);
        }
        serde_json::Value::String(s) => {
            collect_subtitles_from_text(s, base_url, out);
        }
        _ => {}
    }
}

fn collect_subtitles_from_text(raw: &str, base_url: &str, out: &mut Vec<OnlineVideoSubtitle>) {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return;
    }
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
        collect_subtitles_from_json_value(&val, base_url, out);
        return;
    }
    if trimmed.contains('\n') {
        for line in trimmed.lines() {
            let line_trimmed = line.trim();
            if !line_trimmed.is_empty() {
                collect_subtitles_from_text(line_trimmed, base_url, out);
            }
        }
        return;
    }
    if looks_like_subtitle_url(trimmed) {
        out.push(OnlineVideoSubtitle {
            label: format!("字幕 {}", out.len() + 1),
            lang: None,
            url: resolve_relative_url(trimmed, base_url),
        });
    }
}

fn parse_subtitles_json(raw: &str, base_url: &str) -> Vec<OnlineVideoSubtitle> {
    let mut out = Vec::new();
    collect_subtitles_from_text(raw, base_url, &mut out);
    out
}

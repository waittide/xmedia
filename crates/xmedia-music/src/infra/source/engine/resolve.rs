use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use xmedia_source::http::{resolve_relative_url, resolve_relative_url_preserving_options};
use xmedia_source::model::{
    ExecutionDiagnostics, SourceCallError, SourceCallOutput, SourceRuntimeState,
};
use xmedia_source::rule::{RuleValue, strip_js_wrapper};

use super::template::{
    MusicTemplateVars, apply_stream_headers, eval_rule_or_literal, fetch_api,
    is_static_rule_literal, non_empty_opt, normalize_quality_placeholder, resolve_literal_http_url,
    template_references_quality,
};
use super::{MusicSourceEngine, infer_audio_format, is_http_url};
use crate::domain::catalog::StructuredLyrics;
use crate::domain::online::{
    MusicSourceSpec, MusicTrackContext, OnlineMusicLyricsResult, OnlineMusicStreamResult,
};

impl MusicSourceEngine {
    pub fn resolve_stream_sync(
        &self,
        source: &MusicSourceSpec,
        initial_state: SourceRuntimeState,
        ctx: &MusicTrackContext,
        quality: Option<&str>,
    ) -> Result<SourceCallOutput<OnlineMusicStreamResult>, SourceCallError> {
        self.resolve_stream_sync_with_control(
            source,
            initial_state,
            ctx,
            quality,
            self.js_timeout,
            Arc::new(AtomicBool::new(false)),
        )
    }

    pub fn resolve_stream_sync_with_control(
        &self,
        source: &MusicSourceSpec,
        initial_state: SourceRuntimeState,
        ctx: &MusicTrackContext,
        quality: Option<&str>,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<OnlineMusicStreamResult>, SourceCallError> {
        let mut session = self
            .build_session(source, initial_state, op_timeout, cancel_flag)
            .map_err(|e| SourceCallError::new(e, ExecutionDiagnostics::default()))?;

        let mut run = || -> Result<OnlineMusicStreamResult, String> {
            let requested_quality =
                quality
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .or_else(|| {
                        ctx.quality
                            .as_deref()
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                    });
            let (effective_quality, quality_matches_or_absent) =
                source.resolve_quality(requested_quality);

            let mut ctx_with_q = ctx.clone();
            ctx_with_q.quality = Some(effective_quality.to_string());
            session.set_track_context(&ctx_with_q)?;

            let mut headers = source.parse_media_headers();

            let Some(ref rule) = source.rule_stream else {
                let direct = ctx
                    .play_url
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| {
                        format!(
                            "音源「{}」未配置 ruleStream 且缺少 playUrl",
                            source.source_name
                        )
                    })?;
                let resolved_url = resolve_literal_http_url(direct, &source.source_url, "playUrl")?;
                let format = infer_audio_format(&resolved_url).unwrap_or_else(|| "mp3".into());
                return Ok(OnlineMusicStreamResult {
                    stream_url: resolved_url,
                    format,
                    bitrate: None,
                    headers,
                    expires_in_secs: None,
                });
            };

            let vars = MusicTemplateVars::from_track(&ctx_with_q, effective_quality);
            let empty_input = RuleValue::Text(String::new());

            let direct_play_url = ctx
                .play_url
                .as_deref()
                .map(str::trim)
                .filter(|s| is_http_url(s))
                .and_then(|s| resolve_literal_http_url(s, &source.source_url, "playUrl").ok());
            let has_rule_headers = rule
                .headers
                .as_deref()
                .is_some_and(|s| !s.trim().is_empty());
            let static_metadata_only = is_static_rule_literal(rule.format.as_deref())
                && is_static_rule_literal(rule.bitrate.as_deref())
                && is_static_rule_literal(rule.expires_in_secs.as_deref());

            if let Some(req_tpl) = rule
                .request_url
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                if !has_rule_headers
                    && static_metadata_only
                    && quality_matches_or_absent
                    && !template_references_quality(req_tpl)
                    && let Some(direct) = direct_play_url
                {
                    let format = rule
                        .format
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                        .or_else(|| infer_audio_format(&direct))
                        .unwrap_or_else(|| "mp3".into());
                    let bitrate = rule
                        .bitrate
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .and_then(|s| s.parse::<u32>().ok());
                    return Ok(OnlineMusicStreamResult {
                        stream_url: direct,
                        format,
                        bitrate,
                        headers,
                        expires_in_secs: rule
                            .expires_in_secs
                            .as_deref()
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .and_then(|s| s.parse::<u64>().ok()),
                    });
                }

                let evaluated_req =
                    vars.eval(&mut session, req_tpl, &empty_input, &source.source_url)?;
                if evaluated_req.trim().is_empty() {
                    return Err("ruleStream.requestUrl 求值结果为空".into());
                }
                let target_url = resolve_relative_url_preserving_options(
                    evaluated_req.trim(),
                    &source.source_url,
                );
                let fetched = fetch_api(&session, &target_url, &source.source_url)?;
                let input = RuleValue::from_raw(fetched.body);

                let stream_rule = rule
                    .stream_url
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| "ruleStream 缺少 streamUrl 规则".to_string())?;
                let normalized_stream_rule = normalize_quality_placeholder(stream_rule);
                let stream_raw =
                    session.eval_string(&input, &normalized_stream_rule, &fetched.final_url)?;
                let stream_trimmed = stream_raw.trim();
                if stream_trimmed.is_empty() {
                    return Err("未能从 ruleStream 解析出有效音频流地址".into());
                }
                let resolved_stream = resolve_relative_url(stream_trimmed, &fetched.final_url);
                if !is_http_url(&resolved_stream) {
                    return Err(format!(
                        "解析到的音频流地址不是有效的 HTTP(S) URL: {resolved_stream}"
                    ));
                }

                let format = eval_rule_or_literal(
                    &mut session,
                    &input,
                    rule.format.as_deref(),
                    &fetched.final_url,
                )
                .or_else(|| infer_audio_format(&resolved_stream))
                .unwrap_or_else(|| "mp3".into());

                let bitrate = eval_rule_or_literal(
                    &mut session,
                    &input,
                    rule.bitrate.as_deref(),
                    &fetched.final_url,
                )
                .and_then(|s| s.trim().parse::<f64>().ok())
                .filter(|v| v.is_finite() && *v > 0.0)
                .map(|v| v.round() as u32);

                let expires_in_secs = eval_rule_or_literal(
                    &mut session,
                    &input,
                    rule.expires_in_secs.as_deref(),
                    &fetched.final_url,
                )
                .and_then(|s| s.trim().parse::<u64>().ok());

                apply_stream_headers(
                    &mut session,
                    &input,
                    rule.headers.as_deref(),
                    &fetched.final_url,
                    &mut headers,
                );

                Ok(OnlineMusicStreamResult {
                    stream_url: resolved_stream,
                    format,
                    bitrate,
                    headers,
                    expires_in_secs,
                })
            } else {
                let resolved_stream = if let Some(tpl) = rule
                    .stream_url
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    vars.eval(&mut session, tpl, &empty_input, &source.source_url)?
                } else {
                    resolve_literal_http_url(
                        ctx.play_url.as_deref().unwrap_or(""),
                        &source.source_url,
                        "playUrl",
                    )?
                };

                let trimmed = resolved_stream.trim();
                if trimmed.is_empty() {
                    return Err("未能解析出有效音频流地址".into());
                }
                let final_url =
                    resolve_relative_url_preserving_options(trimmed, &source.source_url);
                if !is_http_url(&final_url) {
                    return Err(format!(
                        "解析到的音频流地址不是有效的 HTTP(S) URL: {final_url}"
                    ));
                }
                let format = eval_rule_or_literal(
                    &mut session,
                    &empty_input,
                    rule.format.as_deref(),
                    &source.source_url,
                )
                .or_else(|| infer_audio_format(&final_url))
                .unwrap_or_else(|| "mp3".into());
                let bitrate = eval_rule_or_literal(
                    &mut session,
                    &empty_input,
                    rule.bitrate.as_deref(),
                    &source.source_url,
                )
                .and_then(|s| s.trim().parse::<f64>().ok())
                .filter(|v| v.is_finite() && *v > 0.0)
                .map(|v| v.round() as u32);
                let expires_in_secs = eval_rule_or_literal(
                    &mut session,
                    &empty_input,
                    rule.expires_in_secs.as_deref(),
                    &source.source_url,
                )
                .and_then(|s| s.trim().parse::<u64>().ok());

                apply_stream_headers(
                    &mut session,
                    &empty_input,
                    rule.headers.as_deref(),
                    &source.source_url,
                    &mut headers,
                );

                Ok(OnlineMusicStreamResult {
                    stream_url: final_url,
                    format,
                    bitrate,
                    headers,
                    expires_in_secs,
                })
            }
        };

        match run() {
            Ok(data) => {
                let (state, diagnostics) = session.finalize_state();
                Ok(SourceCallOutput {
                    data,
                    state,
                    diagnostics,
                })
            }
            Err(msg) => Err(SourceCallError::new(msg, session.snapshot_diagnostics())),
        }
    }

    pub fn resolve_cover_sync(
        &self,
        source: &MusicSourceSpec,
        initial_state: SourceRuntimeState,
        ctx: &MusicTrackContext,
    ) -> Result<SourceCallOutput<Option<String>>, SourceCallError> {
        self.resolve_cover_sync_with_control(
            source,
            initial_state,
            ctx,
            self.js_timeout,
            Arc::new(AtomicBool::new(false)),
        )
    }

    pub fn resolve_cover_sync_with_control(
        &self,
        source: &MusicSourceSpec,
        initial_state: SourceRuntimeState,
        ctx: &MusicTrackContext,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<Option<String>>, SourceCallError> {
        let mut session = self
            .build_session(source, initial_state, op_timeout, cancel_flag)
            .map_err(|e| SourceCallError::new(e, ExecutionDiagnostics::default()))?;

        let mut run = || -> Result<Option<String>, String> {
            session.set_track_context(ctx)?;
            let Some(ref rule) = source.rule_cover else {
                return Ok(ctx
                    .cover_url
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|u| resolve_relative_url(u, &source.source_url)));
            };

            let req_tpl_opt = rule
                .request_url
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty());

            if let Some(ref existing) = ctx.cover_url
                && !existing.trim().is_empty()
                && req_tpl_opt.is_none()
            {
                return Ok(Some(resolve_relative_url(existing, &source.source_url)));
            }

            let vars = MusicTemplateVars::from_track(ctx, ctx.quality.as_deref().unwrap_or(""));
            let empty_input = RuleValue::Text(String::new());

            if let Some(req_tpl) = req_tpl_opt {
                let has_cover_or_id = ctx
                    .cover_id
                    .as_deref()
                    .is_some_and(|s| !s.trim().is_empty())
                    || !ctx.track_id.trim().is_empty();
                if !has_cover_or_id {
                    return Ok(ctx.cover_url.clone());
                }
                let evaluated_req =
                    vars.eval(&mut session, req_tpl, &empty_input, &source.source_url)?;
                if evaluated_req.trim().is_empty() {
                    return Ok(ctx.cover_url.clone());
                }
                let target_url = resolve_relative_url_preserving_options(
                    evaluated_req.trim(),
                    &source.source_url,
                );
                let fetched = fetch_api(&session, &target_url, &source.source_url)?;
                let input = RuleValue::from_raw(fetched.body);
                let cover_raw =
                    session.eval_opt(&input, rule.cover_url.as_deref(), &fetched.final_url);
                let clean = cover_raw.trim();
                if clean.is_empty() {
                    Ok(ctx.cover_url.clone())
                } else {
                    Ok(Some(resolve_relative_url(clean, &fetched.final_url)))
                }
            } else if let Some(cover_tpl) = rule
                .cover_url
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                let cover_raw =
                    vars.eval(&mut session, cover_tpl, &empty_input, &source.source_url)?;
                let clean = cover_raw.trim();
                if clean.is_empty() {
                    Ok(ctx.cover_url.clone())
                } else {
                    Ok(Some(resolve_relative_url(clean, &source.source_url)))
                }
            } else {
                Ok(ctx.cover_url.clone())
            }
        };

        match run() {
            Ok(data) => {
                let (state, diagnostics) = session.finalize_state();
                Ok(SourceCallOutput {
                    data,
                    state,
                    diagnostics,
                })
            }
            Err(msg) => Err(SourceCallError::new(msg, session.snapshot_diagnostics())),
        }
    }

    pub fn resolve_lyrics_sync(
        &self,
        source: &MusicSourceSpec,
        initial_state: SourceRuntimeState,
        ctx: &MusicTrackContext,
    ) -> Result<SourceCallOutput<Option<OnlineMusicLyricsResult>>, SourceCallError> {
        self.resolve_lyrics_sync_with_control(
            source,
            initial_state,
            ctx,
            self.js_timeout,
            Arc::new(AtomicBool::new(false)),
        )
    }

    pub fn resolve_lyrics_sync_with_control(
        &self,
        source: &MusicSourceSpec,
        initial_state: SourceRuntimeState,
        ctx: &MusicTrackContext,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<Option<OnlineMusicLyricsResult>>, SourceCallError> {
        let mut session = self
            .build_session(source, initial_state, op_timeout, cancel_flag)
            .map_err(|e| SourceCallError::new(e, ExecutionDiagnostics::default()))?;

        let mut run = || -> Result<Option<OnlineMusicLyricsResult>, String> {
            session.set_track_context(ctx)?;
            let vars = MusicTemplateVars::from_track(ctx, ctx.quality.as_deref().unwrap_or(""));
            let empty_input = RuleValue::Text(String::new());

            let (raw_lyric, raw_tlyric) = if let Some(ref rule) = source.rule_lyric {
                if let Some(req_tpl) = rule
                    .request_url
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    let evaluated_req =
                        vars.eval(&mut session, req_tpl, &empty_input, &source.source_url)?;
                    if evaluated_req.trim().is_empty() {
                        return Ok(None);
                    }
                    let target_url = resolve_relative_url_preserving_options(
                        evaluated_req.trim(),
                        &source.source_url,
                    );
                    let fetched = fetch_api(&session, &target_url, &source.source_url)?;
                    let input = RuleValue::from_raw(fetched.body);

                    let mut lrc =
                        session.eval_opt(&input, rule.lyric.as_deref(), &fetched.final_url);
                    if lrc.trim().is_empty()
                        && let Some(ref plain_rule) = rule.plain_lyric
                    {
                        lrc = session.eval_opt(&input, Some(plain_rule), &fetched.final_url);
                    }
                    let tlrc = non_empty_opt(session.eval_opt(
                        &input,
                        rule.translated_lyric.as_deref(),
                        &fetched.final_url,
                    ));
                    (lrc, tlrc)
                } else {
                    (String::new(), None)
                }
            } else if let Some(ref detail_rule) = source.rule_track_detail
                && let Some(ref lyric_rule) = detail_rule.lyric
                && !lyric_rule.trim().is_empty()
            {
                let target_url = if let Some(req_tpl) = detail_rule
                    .request_url
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    let evaluated_req =
                        vars.eval(&mut session, req_tpl, &empty_input, &source.source_url)?;
                    if evaluated_req.trim().is_empty() {
                        None
                    } else {
                        Some(resolve_relative_url_preserving_options(
                            evaluated_req.trim(),
                            &source.source_url,
                        ))
                    }
                } else if let Some(detail_url) = ctx
                    .detail_url
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    Some(resolve_literal_http_url(
                        detail_url,
                        &source.source_url,
                        "detailUrl",
                    )?)
                } else {
                    None
                };

                if let Some(target_url) = target_url {
                    let fetched = fetch_api(&session, &target_url, &source.source_url)?;
                    let input = RuleValue::from_raw(fetched.body);
                    let lrc = session.eval_opt(&input, Some(lyric_rule), &fetched.final_url);
                    (lrc, None)
                } else {
                    (String::new(), None)
                }
            } else {
                (String::new(), None)
            };

            if raw_lyric.trim().is_empty() {
                return Ok(None);
            }

            let mut parsed = StructuredLyrics::from(raw_lyric.as_str());
            if parsed.display_title.is_none() && !ctx.title.trim().is_empty() {
                parsed.display_title = Some(ctx.title.clone());
            }
            if parsed.display_artist.is_none() && !ctx.artist.trim().is_empty() {
                parsed.display_artist = Some(ctx.artist.clone());
            }
            if parsed.is_empty() {
                return Ok(None);
            }

            let translated_lyrics = raw_tlyric.as_deref().and_then(|raw_t| {
                let t_parsed = StructuredLyrics::from(raw_t);
                (!t_parsed.is_empty()).then_some(t_parsed)
            });

            Ok(Some(OnlineMusicLyricsResult {
                lyrics: parsed,
                translated_lyrics,
                raw_lyric,
                raw_translated_lyric: raw_tlyric,
            }))
        };

        match run() {
            Ok(data) => {
                let (state, diagnostics) = session.finalize_state();
                Ok(SourceCallOutput {
                    data,
                    state,
                    diagnostics,
                })
            }
            Err(msg) => Err(SourceCallError::new(msg, session.snapshot_diagnostics())),
        }
    }

    pub fn run_login_action_sync(
        &self,
        source: &MusicSourceSpec,
        mut initial_state: SourceRuntimeState,
        login_info: HashMap<String, String>,
        action_js: Option<&str>,
    ) -> Result<SourceCallOutput<String>, SourceCallError> {
        let merged_login = xmedia_source::crud::merge_login_info_preserving_masked(
            &initial_state.login_info,
            login_info,
        );
        initial_state.login_info = merged_login.clone();

        let mut session = self
            .build_session(
                source,
                initial_state,
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
        if login_url_js.is_empty() && effective_action.is_none() {
            return Err(SourceCallError::new(
                "当前音源未配置 loginUrl 或可执行的登录脚本",
                ExecutionDiagnostics::default(),
            ));
        }

        match session.eval_login_script(login_url_js, effective_action, &merged_login) {
            Ok(result) => {
                let (state, diagnostics) = session.finalize_state();
                Ok(SourceCallOutput {
                    data: result,
                    state,
                    diagnostics,
                })
            }
            Err(msg) => Err(SourceCallError::new(msg, session.snapshot_diagnostics())),
        }
    }
}

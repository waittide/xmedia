use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use xmedia_source::http::{resolve_relative_url, resolve_relative_url_preserving_options};
use xmedia_source::model::{
    ExecutionDiagnostics, SourceCallError, SourceCallOutput, SourceRuntimeState,
};
use xmedia_source::rule::{RuleValue, split_js_pipeline};

use super::super::session::MusicSourceSession;
use super::template::{
    MusicTemplateVars, eval_rule_or_literal, fetch_api, non_empty_opt, resolve_literal_http_url,
    substitute_static_explore_placeholders,
};
use super::{MusicSourceEngine, infer_audio_format, normalize_artist, parse_duration_secs};
use crate::domain::online::{
    MusicSearchRule, MusicSourceSpec, MusicTrackContext, OnlineMusicExploreCategory,
    OnlineMusicExploreResult, OnlineMusicTrackItem,
};

fn extract_tracks_with_rule(
    session: &mut MusicSourceSession,
    raw_input: &RuleValue,
    rule: &MusicSearchRule,
    base_url: &str,
) -> Result<Vec<OnlineMusicTrackItem>, String> {
    let list_rule = rule
        .track_list
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("$");

    let items = session.eval_list(raw_input, list_rule, base_url)?;
    let mut tracks = Vec::with_capacity(items.len());
    for item in items {
        session.check_active()?;
        let track_id = session.eval_opt(&item, rule.track_id.as_deref(), base_url);
        let title = session.eval_opt(&item, rule.title.as_deref(), base_url);
        if title.trim().is_empty() && track_id.trim().is_empty() {
            continue;
        }
        let artist_raw = session.eval_opt(&item, rule.artist.as_deref(), base_url);
        let artist = normalize_artist(&artist_raw);
        let album = non_empty_opt(session.eval_opt(&item, rule.album.as_deref(), base_url));
        let duration_raw = session.eval_opt(&item, rule.duration.as_deref(), base_url);
        let duration_secs = parse_duration_secs(&duration_raw, rule.duration_unit.as_deref());

        let cover_url = non_empty_opt(session.eval_opt(&item, rule.cover_url.as_deref(), base_url))
            .map(|u| resolve_relative_url(&u, base_url));
        let cover_id = non_empty_opt(session.eval_opt(&item, rule.cover_id.as_deref(), base_url));
        let url_id = non_empty_opt(session.eval_opt(&item, rule.url_id.as_deref(), base_url));
        let lyric_id = non_empty_opt(session.eval_opt(&item, rule.lyric_id.as_deref(), base_url));
        let play_url = non_empty_opt(session.eval_opt(&item, rule.play_url.as_deref(), base_url))
            .map(|u| resolve_relative_url(&u, base_url));
        let detail_url =
            non_empty_opt(session.eval_opt(&item, rule.detail_url.as_deref(), base_url))
                .map(|u| resolve_relative_url(&u, base_url));
        let format = eval_rule_or_literal(session, &item, rule.format.as_deref(), base_url)
            .or_else(|| play_url.as_deref().and_then(infer_audio_format));

        tracks.push(OnlineMusicTrackItem {
            track_id,
            title,
            artist,
            album,
            duration_secs,
            cover_url,
            cover_id,
            url_id,
            lyric_id,
            play_url,
            detail_url,
            format,
        });
    }
    Ok(tracks)
}

impl MusicSourceEngine {
    pub fn search_sync(
        &self,
        source: &MusicSourceSpec,
        initial_state: SourceRuntimeState,
        keyword: &str,
        page: u32,
        limit: u32,
    ) -> Result<SourceCallOutput<Vec<OnlineMusicTrackItem>>, SourceCallError> {
        self.search_sync_with_control(
            source,
            initial_state,
            keyword,
            page,
            limit,
            self.js_timeout,
            Arc::new(AtomicBool::new(false)),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn search_sync_with_control(
        &self,
        source: &MusicSourceSpec,
        initial_state: SourceRuntimeState,
        keyword: &str,
        page: u32,
        limit: u32,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<Vec<OnlineMusicTrackItem>>, SourceCallError> {
        let mut session = self
            .build_session(source, initial_state, op_timeout, cancel_flag)
            .map_err(|e| SourceCallError::new(e, ExecutionDiagnostics::default()))?;

        let mut run = || -> Result<Vec<OnlineMusicTrackItem>, String> {
            let rule = source.list_rule(false)?;
            let raw_tpl = source
                .search_url
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| format!("音源「{}」未配置 searchUrl", source.source_name))?;

            let page_clamped = page.max(1);
            let limit_clamped = limit.clamp(1, 50);
            session.set_search_vars(keyword, page_clamped, limit_clamped)?;

            let vars = MusicTemplateVars::for_search(keyword, page_clamped, limit_clamped);
            let empty_input = RuleValue::Text(String::new());
            let evaluated = vars.eval(&mut session, raw_tpl, &empty_input, &source.source_url)?;
            if evaluated.trim().is_empty() {
                return Err("searchUrl 求值结果为空".into());
            }
            let target_url =
                resolve_relative_url_preserving_options(evaluated.trim(), &source.source_url);
            let fetched = fetch_api(&session, &target_url, &source.source_url)?;
            let raw_input = RuleValue::from_raw(fetched.body);
            extract_tracks_with_rule(&mut session, &raw_input, rule, &fetched.final_url)
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

    pub fn explore_sync(
        &self,
        source: &MusicSourceSpec,
        initial_state: SourceRuntimeState,
        explore_url_override: Option<&str>,
        page: u32,
        limit: u32,
    ) -> Result<SourceCallOutput<OnlineMusicExploreResult>, SourceCallError> {
        self.explore_sync_with_control(
            source,
            initial_state,
            explore_url_override,
            page,
            limit,
            self.js_timeout,
            Arc::new(AtomicBool::new(false)),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn explore_sync_with_control(
        &self,
        source: &MusicSourceSpec,
        initial_state: SourceRuntimeState,
        explore_url_override: Option<&str>,
        page: u32,
        limit: u32,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<OnlineMusicExploreResult>, SourceCallError> {
        let mut session = self
            .build_session(source, initial_state, op_timeout, cancel_flag)
            .map_err(|e| SourceCallError::new(e, ExecutionDiagnostics::default()))?;

        let mut run = || -> Result<OnlineMusicExploreResult, String> {
            let categories = source
                .explore_url
                .as_deref()
                .map(|raw| OnlineMusicExploreCategory::parse_list(raw, &source.source_url))
                .unwrap_or_default();

            let rule = source.list_rule(true)?;

            let page_clamped = page.max(1);
            let limit_clamped = limit.clamp(1, 50);

            let override_trimmed = explore_url_override
                .map(str::trim)
                .filter(|s| !s.is_empty());

            let target_url = if let Some(raw_override) = override_trimmed {
                if let Some(matched_cat) = categories.iter().find(|c| c.url == raw_override) {
                    session.set_search_vars("", page_clamped, limit_clamped)?;
                    let vars = MusicTemplateVars::for_explore(page_clamped, limit_clamped);
                    let empty_input = RuleValue::Text(String::new());
                    let evaluated = vars.eval(
                        &mut session,
                        &matched_cat.url,
                        &empty_input,
                        &source.source_url,
                    )?;
                    if evaluated.trim().is_empty() {
                        return Err("exploreUrl 求值结果为空".into());
                    }
                    resolve_relative_url_preserving_options(evaluated.trim(), &source.source_url)
                } else {
                    if split_js_pipeline(raw_override).is_some() {
                        return Err("自定义 exploreUrl 不允许包含 JS 脚本".into());
                    }
                    let substituted = substitute_static_explore_placeholders(
                        raw_override,
                        page_clamped,
                        limit_clamped,
                    );
                    resolve_literal_http_url(&substituted, &source.source_url, "exploreUrl")?
                }
            } else {
                let default_cat = categories
                    .first()
                    .ok_or_else(|| format!("音源「{}」未配置 exploreUrl", source.source_name))?;
                session.set_search_vars("", page_clamped, limit_clamped)?;
                let vars = MusicTemplateVars::for_explore(page_clamped, limit_clamped);
                let empty_input = RuleValue::Text(String::new());
                let evaluated = vars.eval(
                    &mut session,
                    &default_cat.url,
                    &empty_input,
                    &source.source_url,
                )?;
                if evaluated.trim().is_empty() {
                    return Err("exploreUrl 求值结果为空".into());
                }
                resolve_relative_url_preserving_options(evaluated.trim(), &source.source_url)
            };

            let fetched = fetch_api(&session, &target_url, &source.source_url)?;
            let raw_input = RuleValue::from_raw(fetched.body);
            let tracks =
                extract_tracks_with_rule(&mut session, &raw_input, rule, &fetched.final_url)?;
            Ok(OnlineMusicExploreResult { tracks, categories })
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

    pub fn fetch_track_detail_sync(
        &self,
        source: &MusicSourceSpec,
        initial_state: SourceRuntimeState,
        ctx: &MusicTrackContext,
    ) -> Result<SourceCallOutput<OnlineMusicTrackItem>, SourceCallError> {
        self.fetch_track_detail_sync_with_control(
            source,
            initial_state,
            ctx,
            self.js_timeout,
            Arc::new(AtomicBool::new(false)),
        )
    }

    pub fn fetch_track_detail_sync_with_control(
        &self,
        source: &MusicSourceSpec,
        initial_state: SourceRuntimeState,
        ctx: &MusicTrackContext,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<OnlineMusicTrackItem>, SourceCallError> {
        let mut session = self
            .build_session(source, initial_state, op_timeout, cancel_flag)
            .map_err(|e| SourceCallError::new(e, ExecutionDiagnostics::default()))?;

        let mut run = || -> Result<OnlineMusicTrackItem, String> {
            session.set_track_context(ctx)?;
            let Some(ref rule) = source.rule_track_detail else {
                return Ok(OnlineMusicTrackItem {
                    track_id: ctx.track_id.clone(),
                    title: ctx.title.clone(),
                    artist: ctx.artist.clone(),
                    album: ctx.album.clone(),
                    duration_secs: ctx.duration_secs,
                    cover_url: ctx.cover_url.clone(),
                    cover_id: ctx.cover_id.clone(),
                    url_id: ctx.url_id.clone(),
                    lyric_id: ctx.lyric_id.clone(),
                    play_url: ctx.play_url.clone(),
                    detail_url: ctx.detail_url.clone(),
                    format: ctx.play_url.as_deref().and_then(infer_audio_format),
                });
            };

            let quality = ctx.quality.as_deref().unwrap_or("");
            let vars = MusicTemplateVars::from_track(ctx, quality);
            let target_url = if let Some(req_tpl) = rule
                .request_url
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                let empty_input = RuleValue::Text(String::new());
                let evaluated_url =
                    vars.eval(&mut session, req_tpl, &empty_input, &source.source_url)?;
                if evaluated_url.trim().is_empty() {
                    return Err("ruleTrackDetail.requestUrl 求值结果为空".into());
                }
                resolve_relative_url_preserving_options(evaluated_url.trim(), &source.source_url)
            } else if let Some(detail_url) = ctx
                .detail_url
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                resolve_literal_http_url(detail_url, &source.source_url, "detailUrl")?
            } else {
                return Err("缺少 track detail 请求 URL".to_string());
            };
            let fetched = fetch_api(&session, &target_url, &source.source_url)?;
            let mut input = RuleValue::from_raw(fetched.body);

            if let Some(ref init_rule) = rule.init
                && !init_rule.trim().is_empty()
            {
                let init_str = session.eval_string(&input, init_rule, &fetched.final_url)?;
                if !init_str.trim().is_empty() {
                    input = RuleValue::from_raw(init_str);
                }
            }

            let track_id = non_empty_opt(session.eval_opt(
                &input,
                rule.track_id.as_deref(),
                &fetched.final_url,
            ))
            .unwrap_or_else(|| ctx.track_id.clone());
            let title =
                non_empty_opt(session.eval_opt(&input, rule.title.as_deref(), &fetched.final_url))
                    .unwrap_or_else(|| ctx.title.clone());
            let artist_eval = normalize_artist(&session.eval_opt(
                &input,
                rule.artist.as_deref(),
                &fetched.final_url,
            ));
            let artist = if artist_eval.is_empty() {
                ctx.artist.clone()
            } else {
                artist_eval
            };
            let album =
                non_empty_opt(session.eval_opt(&input, rule.album.as_deref(), &fetched.final_url))
                    .or_else(|| ctx.album.clone());
            let duration_raw =
                session.eval_opt(&input, rule.duration.as_deref(), &fetched.final_url);
            let duration_secs = parse_duration_secs(&duration_raw, rule.duration_unit.as_deref())
                .or(ctx.duration_secs);
            let cover_url = non_empty_opt(session.eval_opt(
                &input,
                rule.cover_url.as_deref(),
                &fetched.final_url,
            ))
            .map(|u| resolve_relative_url(&u, &fetched.final_url))
            .or_else(|| ctx.cover_url.clone());
            let cover_id = non_empty_opt(session.eval_opt(
                &input,
                rule.cover_id.as_deref(),
                &fetched.final_url,
            ))
            .or_else(|| ctx.cover_id.clone());
            let url_id =
                non_empty_opt(session.eval_opt(&input, rule.url_id.as_deref(), &fetched.final_url))
                    .or_else(|| ctx.url_id.clone());
            let lyric_id = non_empty_opt(session.eval_opt(
                &input,
                rule.lyric_id.as_deref(),
                &fetched.final_url,
            ))
            .or_else(|| ctx.lyric_id.clone());
            let play_url = non_empty_opt(session.eval_opt(
                &input,
                rule.play_url.as_deref(),
                &fetched.final_url,
            ))
            .map(|u| resolve_relative_url(&u, &fetched.final_url))
            .or_else(|| ctx.play_url.clone());
            let format = eval_rule_or_literal(
                &mut session,
                &input,
                rule.format.as_deref(),
                &fetched.final_url,
            )
            .or_else(|| play_url.as_deref().and_then(infer_audio_format));

            Ok(OnlineMusicTrackItem {
                track_id,
                title,
                artist,
                album,
                duration_secs,
                cover_url,
                cover_id,
                url_id,
                lyric_id,
                play_url,
                detail_url: Some(target_url),
                format,
            })
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
}

use xmedia_source::http::resolve_relative_url;
use xmedia_source::js_runtime::ReusableSourceSession;
use xmedia_source::rule::{RuleValue, rule_needs_js, split_js_pipeline};

use crate::domain::online::{
    OnlineVideoCategory, OnlineVideoDetail, OnlineVideoEpisode, OnlineVideoPlayGroup,
    OnlineVideoSubtitle, OnlineVideoSummary, VideoJsContext, VideoSearchRule, VideoSourceSpec,
};

use super::{VideoSourceEngine, fetch_session_api};

pub(super) fn is_http_url(u: &str) -> bool {
    let trimmed = u.trim();
    trimmed
        .get(..7)
        .is_some_and(|p| p.eq_ignore_ascii_case("http://"))
        || trimmed
            .get(..8)
            .is_some_and(|p| p.eq_ignore_ascii_case("https://"))
}

pub(super) fn non_empty_opt(s: String) -> Option<String> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub(super) fn eval_rule_categories(
    session: &mut ReusableSourceSession,
    source: &VideoSourceSpec,
) -> Result<Vec<OnlineVideoCategory>, String> {
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
    VideoSourceEngine::set_video_globals(session, &ctx, None)?;
    session.set_search_vars("", 1)?;
    let url = VideoSourceEngine::eval_url_template(
        session,
        cat_url_tpl,
        base_url,
        &[],
        &ctx,
        None,
        "",
        1,
    )?;
    let fetched = fetch_session_api(session, &url, base_url)?;
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
        let name = session.eval_opt(&item, rule_cat.name.as_deref(), &fetched.final_url);
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
}

pub(super) fn eval_rule_detail(
    session: &mut ReusableSourceSession,
    source: &VideoSourceSpec,
    video_id: Option<&str>,
    detail_url: Option<&str>,
) -> Result<OnlineVideoDetail, String> {
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
    VideoSourceEngine::set_video_globals(session, &ctx, None)?;
    session.set_search_vars("", 1)?;

    let target_url = if !clean_durl.is_empty() {
        resolve_relative_url(clean_durl, base_url)
    } else if let Some(ref req_tpl) = rule_detail.request_url
        && !req_tpl.trim().is_empty()
    {
        VideoSourceEngine::eval_url_template(
            session,
            req_tpl,
            base_url,
            &[("vodId", clean_vid)],
            &ctx,
            None,
            "",
            1,
        )?
    } else {
        return Err("缺少 detail_url 且 ruleDetail 未配置 requestUrl".into());
    };

    let fetched = fetch_session_api(session, &target_url, base_url)?;
    let mut raw_input = RuleValue::from_raw(fetched.body);
    if let Some(ref init_rule) = rule_detail.init
        && !init_rule.trim().is_empty()
    {
        let items = session.eval_list(&raw_input, init_rule, &fetched.final_url)?;
        if let Some(first) = items.into_iter().next() {
            raw_input = first;
        }
    }

    let title = session.eval_opt(&raw_input, rule_detail.name.as_deref(), &fetched.final_url);
    let cover_raw = session.eval_opt(
        &raw_input,
        rule_detail.cover_url.as_deref(),
        &fetched.final_url,
    );
    let cover_url =
        (!cover_raw.is_empty()).then(|| resolve_relative_url(&cover_raw, &fetched.final_url));
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
        let group_items = session.eval_list(&raw_input, groups_rule, &fetched.final_url)?;
        for (g_idx, g_item) in group_items.into_iter().enumerate() {
            session.check_active()?;
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
                    session,
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
            session,
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
        fetched.final_url
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
}

pub(super) fn eval_video_summary_list(
    session: &mut ReusableSourceSession,
    raw_input: &RuleValue,
    rule: &VideoSearchRule,
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

pub(super) fn eval_play_subtitles(
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
    let resolved_url = resolve_relative_url(clean_url, base_url);
    if !is_http_url(&resolved_url) {
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
        url: resolved_url,
        proxy_url: None,
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
        let resolved_url = resolve_relative_url(trimmed, base_url);
        if is_http_url(&resolved_url) {
            out.push(OnlineVideoSubtitle {
                label: format!("字幕 {}", out.len() + 1),
                lang: None,
                url: resolved_url,
                proxy_url: None,
            });
        }
    }
}

fn parse_subtitles_json(raw: &str, base_url: &str) -> Vec<OnlineVideoSubtitle> {
    let mut out = Vec::new();
    collect_subtitles_from_text(raw, base_url, &mut out);
    out
}

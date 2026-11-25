use std::collections::HashMap;
use std::sync::LazyLock;
use std::time::Duration;

use regex::Regex;
use serde::Deserialize;
use url::Url;
use xmedia_source::http::{LegadoHttpClient, resolve_relative_url};
use xmedia_source::rule::cached_regex;

use crate::domain::online::{
    OnlineVideoCategory, OnlineVideoDetail, OnlineVideoEpisode, OnlineVideoPlayGroup,
    OnlineVideoSummary, VideoSourceSpec,
};

static RE_SCRIPT_STYLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?is)<(?:script|style)\b[^>]*>[\s\S]*?</(?:script|style)>").expect("valid regex")
});
static RE_HTML_BREAKS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?is)<\s*(?:br\s*/?|/\s*(?:p|div|li|tr|h[1-6]|blockquote)\s*)>")
        .expect("valid regex")
});
static RE_HTML_TAGS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)<[^>]+>").expect("valid regex"));

fn strip_html_intro(raw: &str) -> Option<String> {
    let s = RE_SCRIPT_STYLE.replace_all(raw, "");
    let s = RE_HTML_BREAKS.replace_all(&s, "\n");
    let s = RE_HTML_TAGS.replace_all(&s, "");
    let decoded = s
        .replace("&nbsp;", " ")
        .replace("&#160;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&amp;", "&");
    let cleaned = decoded
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    (!cleaned.is_empty()).then_some(cleaned)
}

fn is_obvious_share_page_url(raw_url: &str) -> bool {
    if let Ok(parsed) = Url::parse(raw_url.trim()) {
        return parsed.path().to_ascii_lowercase().contains("/share/");
    }
    raw_url.to_ascii_lowercase().contains("/share/")
}

#[derive(Debug, Deserialize)]
struct AppleCmsResponse {
    #[serde(default)]
    code: Option<serde_json::Value>,
    #[serde(default)]
    msg: Option<String>,
    #[serde(default)]
    page: Option<serde_json::Value>,
    #[serde(default)]
    pagecount: Option<serde_json::Value>,
    #[serde(default)]
    total: Option<serde_json::Value>,
    #[serde(default)]
    class: Vec<AppleCmsClass>,
    #[serde(default)]
    list: Vec<AppleCmsVod>,
}

#[derive(Debug, Deserialize)]
struct AppleCmsClass {
    type_id: serde_json::Value,
    #[serde(default)]
    type_pid: Option<serde_json::Value>,
    #[serde(default)]
    type_name: String,
}

#[derive(Debug, Deserialize)]
struct AppleCmsVod {
    vod_id: serde_json::Value,
    #[serde(default)]
    vod_name: String,
    #[serde(default)]
    type_name: Option<String>,
    #[serde(default)]
    vod_pic: Option<String>,
    #[serde(default)]
    vod_remarks: Option<String>,
    #[serde(default)]
    vod_year: Option<String>,
    #[serde(default)]
    vod_area: Option<String>,
    #[serde(default)]
    vod_actor: Option<String>,
    #[serde(default)]
    vod_director: Option<String>,
    #[serde(default)]
    vod_blurb: Option<String>,
    #[serde(default)]
    vod_content: Option<String>,
    #[serde(default)]
    vod_play_from: Option<String>,
    #[serde(default)]
    vod_play_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AppleCmsBrowsePage {
    pub items: Vec<OnlineVideoSummary>,
    pub total: Option<u64>,
    pub page: u32,
    pub page_count: Option<u32>,
}

fn json_val_to_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.trim().to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        _ => String::new(),
    }
}

fn json_val_to_u64(v: &serde_json::Value) -> Option<u64> {
    match v {
        serde_json::Value::Number(n) => n.as_u64(),
        serde_json::Value::String(s) => s.trim().parse::<u64>().ok(),
        _ => None,
    }
}

fn non_empty_opt(s: Option<String>) -> Option<String> {
    s.map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

fn build_api_url(base_api: &str, query_pairs: &[(&str, String)]) -> Result<String, String> {
    let base = base_api.trim();
    if base.is_empty() {
        return Err("AppleCMS apiUrl 为空".into());
    }
    let mut parsed = url::Url::parse(base).map_err(|e| format!("无效的 apiUrl '{base}': {e}"))?;
    {
        let mut qp = parsed.query_pairs_mut();
        for (k, v) in query_pairs {
            qp.append_pair(k, v);
        }
    }
    Ok(parsed.to_string())
}

fn parse_applecms_json(
    status: u16,
    body: &str,
    context_label: &str,
) -> Result<AppleCmsResponse, String> {
    let trimmed = body.trim().trim_start_matches('\u{feff}').trim();
    if status == 403 {
        return Err(format!("{context_label}被上游拒绝 (HTTP 403 Forbidden)"));
    }
    if status >= 400 {
        return Err(format!("{context_label}上游返回异常状态 HTTP {status}"));
    }
    if trimmed.is_empty() {
        return Err(format!("{context_label}返回内容为空"));
    }
    if !trimmed.starts_with('{') && !trimmed.starts_with('[') {
        let lower = trimmed.to_ascii_lowercase();
        let preview: String = trimmed.chars().take(120).collect();
        if lower.starts_with('<') || lower.contains("<html") {
            return Err(format!(
                "{context_label}返回了 HTML 页面而非 JSON: {preview}"
            ));
        }
        return Err(format!("{context_label}返回了非 JSON 响应: {preview}"));
    }

    let resp: AppleCmsResponse =
        serde_json::from_str(trimmed).map_err(|e| format!("{context_label} JSON 解析失败: {e}"))?;

    if let Some(code_val) = &resp.code
        && let Some(code_num) = json_val_to_u64(code_val)
        && code_num != 1
        && code_num != 200
        && resp.list.is_empty()
        && resp.class.is_empty()
    {
        let msg = resp.msg.as_deref().unwrap_or("未知错误").trim();
        if !msg.is_empty() {
            return Err(format!("{context_label}上游返回错误 ({code_num}): {msg}"));
        }
    }
    Ok(resp)
}

pub fn infer_stream_type(url: &str) -> &'static str {
    let trimmed = url.trim();
    if let Ok(parsed) = url::Url::parse(trimmed) {
        let path_lower = parsed.path().to_ascii_lowercase();
        if path_lower.ends_with(".m3u8") || path_lower.ends_with(".m3u") {
            return "hls";
        }
        if path_lower.ends_with(".mp4")
            || path_lower.ends_with(".mkv")
            || path_lower.ends_with(".webm")
            || path_lower.ends_with(".mov")
            || path_lower.ends_with(".m4v")
        {
            return "mp4";
        }
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.contains(".m3u8") || lower.contains("mpegurl") {
        "hls"
    } else if lower.contains(".mp4") || lower.contains(".mkv") || lower.contains(".webm") {
        "mp4"
    } else {
        "auto"
    }
}

pub fn fetch_categories(
    spec: &VideoSourceSpec,
    http: &LegadoHttpClient,
) -> Result<Vec<OnlineVideoCategory>, String> {
    fetch_categories_with_timeout(spec, http, None)
}

pub fn fetch_categories_with_timeout(
    spec: &VideoSourceSpec,
    http: &LegadoHttpClient,
    timeout: Option<Duration>,
) -> Result<Vec<OnlineVideoCategory>, String> {
    let api_url = spec.effective_api_url();
    let url = build_api_url(api_url, &[("ac", "list".into()), ("pg", "1".into())])?;
    let fetched = http.fetch_with_timeout(&url, spec.effective_source_url(), timeout)?;
    let parsed = parse_applecms_json(fetched.status, &fetched.body, "获取分类")?;

    let mut out = Vec::with_capacity(parsed.class.len());
    for c in parsed.class {
        let id = json_val_to_string(&c.type_id);
        let name = c.type_name.trim().to_string();
        if id.is_empty() || name.is_empty() {
            continue;
        }
        let parent_id = c
            .type_pid
            .as_ref()
            .map(json_val_to_string)
            .filter(|s| !s.is_empty() && s != "0");
        out.push(OnlineVideoCategory {
            id,
            name,
            parent_id,
        });
    }
    Ok(out)
}

pub fn search(
    spec: &VideoSourceSpec,
    http: &LegadoHttpClient,
    keyword: &str,
    page: u32,
) -> Result<Vec<OnlineVideoSummary>, String> {
    search_with_timeout(spec, http, keyword, page, None)
}

pub fn search_with_timeout(
    spec: &VideoSourceSpec,
    http: &LegadoHttpClient,
    keyword: &str,
    page: u32,
    timeout: Option<Duration>,
) -> Result<Vec<OnlineVideoSummary>, String> {
    if !spec.searchable {
        return Err(format!("影视源「{}」不支持关键词搜索", spec.source_name));
    }
    let api_url = spec.effective_api_url();
    let page_num = page.max(1);
    let url = build_api_url(
        api_url,
        &[
            ("ac", "detail".into()),
            ("wd", keyword.trim().to_string()),
            ("pg", page_num.to_string()),
        ],
    )?;
    let fetched = http.fetch_with_timeout(&url, spec.effective_source_url(), timeout)?;
    let parsed = parse_applecms_json(fetched.status, &fetched.body, "关键词搜索")?;
    Ok(parsed
        .list
        .into_iter()
        .filter_map(|v| vod_to_summary(v, api_url, &fetched.final_url))
        .collect())
}

pub fn browse(
    spec: &VideoSourceSpec,
    http: &LegadoHttpClient,
    category_id: Option<&str>,
    page: u32,
) -> Result<AppleCmsBrowsePage, String> {
    browse_with_timeout(spec, http, category_id, page, None)
}

pub fn browse_with_timeout(
    spec: &VideoSourceSpec,
    http: &LegadoHttpClient,
    category_id: Option<&str>,
    page: u32,
    timeout: Option<Duration>,
) -> Result<AppleCmsBrowsePage, String> {
    let api_url = spec.effective_api_url();
    let page_num = page.max(1);
    let mut params = vec![("ac", "detail".into()), ("pg", page_num.to_string())];
    if let Some(cat) = category_id.map(str::trim).filter(|s| !s.is_empty()) {
        params.push(("t", cat.to_string()));
    }
    let url = build_api_url(api_url, &params)?;
    let fetched = http.fetch_with_timeout(&url, spec.effective_source_url(), timeout)?;
    let parsed = parse_applecms_json(fetched.status, &fetched.body, "分类浏览")?;

    let items = parsed
        .list
        .into_iter()
        .filter_map(|v| vod_to_summary(v, api_url, &fetched.final_url))
        .collect();
    let total = parsed.total.as_ref().and_then(json_val_to_u64);
    let resp_page = parsed
        .page
        .as_ref()
        .and_then(json_val_to_u64)
        .and_then(|n| u32::try_from(n).ok())
        .unwrap_or(page_num);
    let page_count = parsed
        .pagecount
        .as_ref()
        .and_then(json_val_to_u64)
        .and_then(|n| u32::try_from(n).ok());

    Ok(AppleCmsBrowsePage {
        items,
        total,
        page: resp_page,
        page_count,
    })
}

pub fn fetch_detail(
    spec: &VideoSourceSpec,
    http: &LegadoHttpClient,
    video_id: Option<&str>,
    detail_url: Option<&str>,
) -> Result<OnlineVideoDetail, String> {
    fetch_detail_with_timeout(spec, http, video_id, detail_url, None)
}

pub fn fetch_detail_with_timeout(
    spec: &VideoSourceSpec,
    http: &LegadoHttpClient,
    video_id: Option<&str>,
    detail_url: Option<&str>,
    timeout: Option<Duration>,
) -> Result<OnlineVideoDetail, String> {
    let api_url = spec.effective_api_url();
    let target_url = if let Some(vid) = video_id.map(str::trim).filter(|s| !s.is_empty()) {
        build_api_url(
            api_url,
            &[("ac", "detail".into()), ("ids", vid.to_string())],
        )?
    } else if let Some(durl) = detail_url.map(str::trim).filter(|s| !s.is_empty()) {
        resolve_relative_url(durl, spec.effective_source_url())
    } else {
        return Err("缺少 video_id 或 detail_url".into());
    };

    let fetched = http.fetch_with_timeout(&target_url, spec.effective_source_url(), timeout)?;
    let parsed = parse_applecms_json(fetched.status, &fetched.body, "获取影视详情")?;
    let vod = parsed
        .list
        .into_iter()
        .next()
        .ok_or_else(|| "上游未返回该影视的详情条目".to_string())?;

    let id = json_val_to_string(&vod.vod_id);
    let title = vod.vod_name.trim().to_string();
    let cover_url =
        non_empty_opt(vod.vod_pic).map(|u| resolve_relative_url(&u, &fetched.final_url));
    let intro = non_empty_opt(vod.vod_content)
        .and_then(|s| strip_html_intro(&s))
        .or_else(|| non_empty_opt(vod.vod_blurb).and_then(|s| strip_html_intro(&s)));
    let play_groups = parse_vod_play_groups(
        vod.vod_play_from.as_deref(),
        vod.vod_play_url.as_deref(),
        &spec.preferred_play_from,
        spec.effective_play_filter_regex(),
    );

    Ok(OnlineVideoDetail {
        id,
        title,
        cover_url,
        proxy_cover_url: None,
        intro,
        year: non_empty_opt(vod.vod_year),
        area: non_empty_opt(vod.vod_area),
        actors: non_empty_opt(vod.vod_actor),
        director: non_empty_opt(vod.vod_director),
        remarks: non_empty_opt(vod.vod_remarks),
        play_groups,
    })
}

fn vod_to_summary(vod: AppleCmsVod, api_url: &str, final_url: &str) -> Option<OnlineVideoSummary> {
    let vod_id = json_val_to_string(&vod.vod_id);
    let title = vod.vod_name.trim().to_string();
    if vod_id.is_empty() || title.is_empty() {
        return None;
    }
    let detail_url = build_api_url(api_url, &[("ac", "detail".into()), ("ids", vod_id.clone())])
        .unwrap_or_default();
    let cover_url = non_empty_opt(vod.vod_pic).map(|u| resolve_relative_url(&u, final_url));

    Some(OnlineVideoSummary {
        vod_id,
        title,
        cover_url,
        proxy_cover_url: None,
        remarks: non_empty_opt(vod.vod_remarks),
        year: non_empty_opt(vod.vod_year),
        area: non_empty_opt(vod.vod_area),
        type_name: non_empty_opt(vod.type_name),
        detail_url,
    })
}

fn is_http_url(u: &str) -> bool {
    let lower = u.trim().to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

/// Parses AppleCMS v10 `vod_play_from` (`$$$`) and `vod_play_url` (`$$$` -> `#` -> `$`),
/// filters non-media URLs via `play_filter_regex` (falling back to all valid HTTP(S) episodes
/// if zero episodes match across all groups), and sorts groups by `preferred_play_from`.
pub fn parse_vod_play_groups(
    vod_play_from: Option<&str>,
    vod_play_url: Option<&str>,
    preferred_play_from: &[String],
    play_filter_regex: &str,
) -> Vec<OnlineVideoPlayGroup> {
    let raw_urls = vod_play_url.unwrap_or("").trim();
    if raw_urls.is_empty() {
        return Vec::new();
    }

    let from_parts: Vec<&str> = vod_play_from
        .unwrap_or("")
        .split("$$$")
        .map(str::trim)
        .collect();
    let url_groups: Vec<&str> = raw_urls.split("$$$").collect();

    let mut raw_groups: Vec<(usize, String, Vec<OnlineVideoEpisode>)> =
        Vec::with_capacity(url_groups.len());

    for (g_idx, raw_group) in url_groups.into_iter().enumerate() {
        let group_name = from_parts
            .get(g_idx)
            .copied()
            .filter(|s| !s.is_empty())
            .map(ToString::to_string)
            .unwrap_or_else(|| format!("线路{}", g_idx + 1));

        let mut episodes = Vec::new();
        for seg in raw_group.split('#') {
            let trimmed_seg = seg.trim();
            if trimmed_seg.is_empty() {
                continue;
            }
            let (ep_name, ep_url) = if let Some((label, url)) = trimmed_seg.split_once('$') {
                let clean_label = label.trim();
                let clean_url = url.trim();
                let final_label = if clean_label.is_empty() {
                    format!("第{}集", episodes.len() + 1)
                } else {
                    clean_label.to_string()
                };
                (final_label, clean_url.to_string())
            } else {
                (
                    format!("第{}集", episodes.len() + 1),
                    trimmed_seg.to_string(),
                )
            };

            if !is_http_url(&ep_url) {
                continue;
            }
            let idx = episodes.len();
            episodes.push(OnlineVideoEpisode {
                index: idx,
                name: ep_name,
                play_url: ep_url,
            });
        }

        if !episodes.is_empty() {
            raw_groups.push((g_idx, group_name, episodes));
        }
    }

    if raw_groups.is_empty() {
        return Vec::new();
    }

    if let Some(re) = cached_regex(play_filter_regex) {
        let any_match = raw_groups
            .iter()
            .any(|(_, _, eps)| eps.iter().any(|ep| re.is_match(&ep.play_url)));
        let mut filtered = Vec::with_capacity(raw_groups.len());
        for (orig_idx, name, eps) in raw_groups {
            let mut kept: Vec<OnlineVideoEpisode> = if any_match {
                eps.into_iter()
                    .filter(|ep| re.is_match(&ep.play_url))
                    .collect()
            } else {
                eps.into_iter()
                    .filter(|ep| !is_obvious_share_page_url(&ep.play_url))
                    .collect()
            };
            if !kept.is_empty() {
                for (i, ep) in kept.iter_mut().enumerate() {
                    ep.index = i;
                }
                filtered.push((orig_idx, name, kept));
            }
        }
        raw_groups = filtered;
    }

    let mut pref_rank: HashMap<String, usize> = HashMap::new();
    for (idx, p) in preferred_play_from.iter().enumerate() {
        let lower = p.trim().to_ascii_lowercase();
        if !lower.is_empty() {
            pref_rank.entry(lower).or_insert(idx);
        }
    }

    raw_groups.sort_by_key(|(orig_idx, name, _)| {
        let rank = pref_rank
            .get(&name.to_ascii_lowercase())
            .copied()
            .unwrap_or(usize::MAX);
        (rank, *orig_idx)
    });

    raw_groups
        .into_iter()
        .map(|(_, name, episodes)| OnlineVideoPlayGroup { name, episodes })
        .collect()
}

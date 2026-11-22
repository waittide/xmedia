use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use xmedia_source::http::{HttpClientConfig, LegadoHttpClient, resolve_relative_url};
use xmedia_source::model::{
    ExecutionDiagnostics, SourceCallError, SourceCallOutput, SourceRuntimeState,
};
use xmedia_source::rule::{RuleValue, split_js_pipeline};

use super::model::{
    MusicSearchRule, MusicSourceSpec, MusicTrackContext, OnlineMusicExploreCategory,
    OnlineMusicExploreResult, OnlineMusicLyricsResult, OnlineMusicStreamResult,
    OnlineMusicTrackItem,
};
use super::session::MusicSourceSession;
use crate::domain::catalog::AudioFormat;
use crate::infra::lyrics::parse_lyrics_str;

#[derive(Debug, Clone)]
pub struct MusicSourceEngine {
    http_config: HttpClientConfig,
    js_timeout: Duration,
}

impl Default for MusicSourceEngine {
    fn default() -> Self {
        Self {
            http_config: HttpClientConfig::default(),
            js_timeout: Duration::from_secs(45),
        }
    }
}

#[derive(Debug, Default)]
struct MusicTemplateVars<'a> {
    key: &'a str,
    page: u32,
    limit: u32,
    quality: &'a str,
    track_id: &'a str,
    url_id: &'a str,
    cover_id: &'a str,
    lyric_id: &'a str,
    play_url: &'a str,
    detail_url: &'a str,
    title: &'a str,
    artist: &'a str,
    album: &'a str,
}

impl<'a> MusicTemplateVars<'a> {
    fn from_track(ctx: &'a MusicTrackContext, quality: &'a str) -> Self {
        let url_id = ctx
            .url_id
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(ctx.track_id.as_str());
        let cover_id = ctx
            .cover_id
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(ctx.track_id.as_str());
        let lyric_id = ctx
            .lyric_id
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(ctx.track_id.as_str());
        Self {
            key: "",
            page: 1,
            limit: 20,
            quality,
            track_id: ctx.track_id.as_str(),
            url_id,
            cover_id,
            lyric_id,
            play_url: ctx.play_url.as_deref().unwrap_or(""),
            detail_url: ctx.detail_url.as_deref().unwrap_or(""),
            title: ctx.title.as_str(),
            artist: ctx.artist.as_str(),
            album: ctx.album.as_deref().unwrap_or(""),
        }
    }
}

/// Percent-encodes a query/text component (UTF-8 bytes outside RFC 3986 unreserved set).
pub fn url_encode_component(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() * 3);
    for b in raw.as_bytes() {
        let ch = *b;
        if ch.is_ascii_alphanumeric() || matches!(ch, b'-' | b'_' | b'.' | b'~') {
            out.push(ch as char);
        } else {
            out.push('%');
            out.push(hex_upper(ch >> 4));
            out.push(hex_upper(ch & 0x0F));
        }
    }
    out
}

fn hex_upper(nibble: u8) -> char {
    match nibble {
        0..=9 => (b'0' + nibble) as char,
        10..=15 => (b'A' + (nibble - 10)) as char,
        _ => '0',
    }
}

/// Normalizes multi-value artist outputs (`"A\nB"`) into `"A / B"`.
pub fn normalize_artist(raw: &str) -> String {
    raw.lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" / ")
}

/// Parses duration values formatted as `hh:mm:ss`, `mm:ss`, seconds, or milliseconds.
pub fn parse_duration_secs(raw: &str, unit: Option<&str>) -> Option<u32> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.contains(':') {
        let parts: Vec<&str> = trimmed.split(':').collect();
        match parts.len() {
            2 => {
                let mins = parts[0].trim().parse::<f64>().ok()?;
                let secs = parts[1].trim().parse::<f64>().ok()?;
                if mins < 0.0 || secs < 0.0 {
                    return None;
                }
                return Some((mins * 60.0 + secs).round() as u32);
            }
            3 => {
                let hrs = parts[0].trim().parse::<f64>().ok()?;
                let mins = parts[1].trim().parse::<f64>().ok()?;
                let secs = parts[2].trim().parse::<f64>().ok()?;
                if hrs < 0.0 || mins < 0.0 || secs < 0.0 {
                    return None;
                }
                return Some((hrs * 3600.0 + mins * 60.0 + secs).round() as u32);
            }
            _ => return None,
        }
    }
    let val = trimmed.parse::<f64>().ok()?;
    if !val.is_finite() || val < 0.0 {
        return None;
    }
    let unit_norm = unit
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_ascii_lowercase);
    let secs = match unit_norm.as_deref() {
        Some("ms") => val / 1000.0,
        Some("s") | Some("sec") | Some("secs") => val,
        _ => {
            if val >= 36_000.0 {
                val / 1000.0
            } else {
                val
            }
        }
    };
    Some(secs.round() as u32)
}

/// Infers audio format from URL path extension (`mp3`, `m4a`, `flac`, `ogg`, `opus`, `wav`).
pub fn infer_audio_format(url: &str) -> Option<String> {
    let without_frag = url.split('#').next().unwrap_or(url);
    let without_query = without_frag.split('?').next().unwrap_or(without_frag);
    let last_seg = without_query.rsplit('/').next().unwrap_or(without_query);
    let ext = last_seg.rsplit_once('.')?.1;
    AudioFormat::from_extension(ext).map(|fmt| fmt.as_str().to_string())
}

fn is_known_template_placeholder(expr: &str) -> bool {
    expr.eq_ignore_ascii_case("quality")
        || matches!(
            expr,
            "key"
                | "page"
                | "limit"
                | "urlId"
                | "trackId"
                | "coverId"
                | "lyricId"
                | "playUrl"
                | "detailUrl"
                | "title"
                | "artist"
                | "album"
        )
}

fn resolve_static_placeholder(expr: &str, vars: &MusicTemplateVars<'_>) -> Option<String> {
    if expr.eq_ignore_ascii_case("quality") {
        return Some(vars.quality.to_string());
    }
    match expr {
        "key" => Some(url_encode_component(vars.key)),
        "title" => Some(url_encode_component(vars.title)),
        "artist" => Some(url_encode_component(vars.artist)),
        "album" => Some(url_encode_component(vars.album)),
        "page" => Some(vars.page.to_string()),
        "limit" => Some(vars.limit.to_string()),
        "urlId" => Some(vars.url_id.to_string()),
        "trackId" => Some(vars.track_id.to_string()),
        "coverId" => Some(vars.cover_id.to_string()),
        "lyricId" => Some(vars.lyric_id.to_string()),
        "playUrl" => Some(vars.play_url.to_string()),
        "detailUrl" => Some(vars.detail_url.to_string()),
        _ => None,
    }
}

fn substitute_static_explore_placeholders(raw: &str, page: u32, limit: u32) -> String {
    let page_str = page.to_string();
    let limit_str = limit.to_string();
    let mut out = String::with_capacity(raw.len() + 16);
    let mut rest = raw;
    while let Some(open) = rest.find("{{") {
        out.push_str(&rest[..open]);
        let after_open = &rest[open + 2..];
        let Some(close) = after_open.find("}}") else {
            out.push_str(&rest[open..]);
            rest = "";
            break;
        };
        let expr = after_open[..close].trim();
        if expr == "page" {
            out.push_str(&page_str);
        } else if expr == "limit" {
            out.push_str(&limit_str);
        } else {
            out.push_str(&rest[open..open + 2 + close + 2]);
        }
        rest = &after_open[close + 2..];
    }
    out.push_str(rest);
    out
}

fn resolve_literal_http_url(raw: &str, base_url: &str, label: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(format!("{label} 不能为空"));
    }
    if split_js_pipeline(trimmed).is_some() {
        return Err(format!("{label} 不允许包含 JS 脚本"));
    }
    let resolved = resolve_relative_url(trimmed, base_url);
    if !resolved.starts_with("http://") && !resolved.starts_with("https://") {
        return Err(format!("{label} 必须是有效的 HTTP(S) URL: {resolved}"));
    }
    Ok(resolved)
}

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
        let is_non_js_rule = is_known_template_placeholder(expr)
            || expr.starts_with("$.")
            || expr.starts_with("$[")
            || expr.starts_with("@css:")
            || expr.starts_with("@CSS:")
            || expr.starts_with("@Json:")
            || expr.starts_with("@json:");
        if !is_non_js_rule {
            return true;
        }
        rest = &after_open[close + 2..];
    }
    false
}

fn template_references_quality(raw_tpl: &str) -> bool {
    if template_needs_js(raw_tpl) {
        return true;
    }
    let mut rest = raw_tpl;
    while let Some(open) = rest.find("{{") {
        let after_open = &rest[open + 2..];
        let Some(close) = after_open.find("}}") else {
            break;
        };
        let expr = after_open[..close].trim();
        if expr.eq_ignore_ascii_case("quality") {
            return true;
        }
        rest = &after_open[close + 2..];
    }
    false
}

fn split_top_level_or(input: &str) -> Vec<&str> {
    let bytes = input.as_bytes();
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    let mut mustache_depth = 0usize;
    let mut depth_bracket = 0usize;
    let mut depth_paren = 0usize;
    let mut in_single = false;
    let mut in_double = false;
    while i < bytes.len() {
        let ch = bytes[i];
        if !in_single && !in_double {
            if i + 1 < bytes.len() && bytes[i] == b'{' && bytes[i + 1] == b'{' {
                mustache_depth += 1;
                i += 2;
                continue;
            }
            if i + 1 < bytes.len() && bytes[i] == b'}' && bytes[i + 1] == b'}' && mustache_depth > 0
            {
                mustache_depth -= 1;
                i += 2;
                continue;
            }
            if ch == b'[' {
                depth_bracket += 1;
            } else if ch == b']' && depth_bracket > 0 {
                depth_bracket -= 1;
            } else if ch == b'(' {
                depth_paren += 1;
            } else if ch == b')' && depth_paren > 0 {
                depth_paren -= 1;
            } else if mustache_depth == 0
                && depth_bracket == 0
                && depth_paren == 0
                && i + 1 < bytes.len()
                && bytes[i] == b'|'
                && bytes[i + 1] == b'|'
            {
                parts.push(input[start..i].trim());
                i += 2;
                start = i;
                continue;
            }
        }
        if (mustache_depth > 0 || depth_bracket > 0 || depth_paren > 0)
            && ch == b'\''
            && !in_double
            && (i == 0 || bytes[i - 1] != b'\\')
        {
            in_single = !in_single;
        } else if (mustache_depth > 0 || depth_bracket > 0 || depth_paren > 0)
            && ch == b'"'
            && !in_single
            && (i == 0 || bytes[i - 1] != b'\\')
        {
            in_double = !in_double;
        }
        i += 1;
    }
    parts.push(input[start..].trim());
    parts
}

fn is_bare_literal_token(rule: &str) -> bool {
    let t = rule.trim();
    !t.is_empty()
        && t.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}

fn is_static_rule_literal(rule_opt: Option<&str>) -> bool {
    let Some(t) = rule_opt.map(str::trim).filter(|s| !s.is_empty()) else {
        return true;
    };
    if t.starts_with('$')
        || t.starts_with('@')
        || t.contains("{{")
        || split_js_pipeline(t).is_some()
    {
        return false;
    }
    is_bare_literal_token(t)
}

fn eval_rule_or_literal(
    session: &mut MusicSourceSession,
    input: &RuleValue,
    rule_opt: Option<&str>,
    base_url: &str,
) -> Option<String> {
    let rule = rule_opt.map(str::trim).filter(|s| !s.is_empty())?;
    let val = session.eval_opt(input, Some(rule), base_url);
    if !val.trim().is_empty() {
        return Some(val.trim().to_string());
    }
    if is_bare_literal_token(rule) {
        return Some(rule.to_string());
    }
    None
}

fn eval_template_branch_single_pass(
    session: &mut MusicSourceSession,
    branch: &str,
    vars: &MusicTemplateVars<'_>,
    input: &RuleValue,
    base_url: &str,
) -> Result<String, String> {
    if !branch.contains("{{") || !branch.contains("}}") {
        if branch.starts_with("$.")
            || branch.starts_with("$[")
            || branch == "$"
            || branch.starts_with('@')
        {
            return Ok(session.eval_opt(input, Some(branch), base_url));
        }
        return Ok(branch.to_string());
    }

    let mut out = String::with_capacity(branch.len() + 32);
    let mut rest = branch;
    while let Some(open) = rest.find("{{") {
        out.push_str(&rest[..open]);
        let after_open = &rest[open + 2..];
        let Some(close) = after_open.find("}}") else {
            out.push_str(&rest[open..]);
            rest = "";
            break;
        };
        let expr = after_open[..close].trim();
        if let Some(static_val) = resolve_static_placeholder(expr, vars) {
            out.push_str(&static_val);
        } else {
            let token_tpl = format!("{{{{{expr}}}}}");
            let evaluated = session.eval_string(input, &token_tpl, base_url)?;
            out.push_str(&evaluated);
        }
        rest = &after_open[close + 2..];
    }
    out.push_str(rest);
    Ok(out)
}

fn eval_template_expression(
    session: &mut MusicSourceSession,
    raw_tpl: &str,
    vars: &MusicTemplateVars<'_>,
    input: &RuleValue,
    base_url: &str,
) -> Result<String, String> {
    if split_js_pipeline(raw_tpl).is_some() {
        return session.eval_string(input, raw_tpl, base_url);
    }

    for branch in split_top_level_or(raw_tpl) {
        if branch.is_empty() {
            continue;
        }
        let evaluated = eval_template_branch_single_pass(session, branch, vars, input, base_url)?;
        let trimmed = evaluated.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }
    Ok(String::new())
}

fn apply_stream_headers(
    session: &mut MusicSourceSession,
    input: &RuleValue,
    hdr_rule_opt: Option<&str>,
    base_url: &str,
    headers: &mut HashMap<String, String>,
) {
    let Some(hdr_rule) = hdr_rule_opt.map(str::trim).filter(|s| !s.is_empty()) else {
        return;
    };
    let hdr_raw = if hdr_rule.starts_with('{')
        && !hdr_rule.contains("{{")
        && split_js_pipeline(hdr_rule).is_none()
    {
        hdr_rule.to_string()
    } else {
        session.eval_opt(input, Some(hdr_rule), base_url)
    };
    for (k, v) in MusicSourceSpec::parse_header_map(Some(&hdr_raw)) {
        headers.retain(|existing_k, _| !existing_k.eq_ignore_ascii_case(&k));
        headers.insert(k, v);
    }
}

fn normalize_explore_category_url(raw_url: &str, base_url: &str) -> String {
    let trimmed = raw_url.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if split_js_pipeline(trimmed).is_some() || trimmed.contains("{{") || trimmed.contains("}}") {
        return trimmed.to_string();
    }
    resolve_relative_url(trimmed, base_url)
}

fn split_explore_entries_outside_js(raw: &str) -> Vec<&str> {
    let bytes = raw.as_bytes();
    let mut entries = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    let mut in_tag_js = false;
    let mut in_at_js = false;

    while i < bytes.len() {
        if !in_tag_js
            && bytes
                .get(i..i + 4)
                .is_some_and(|s| s.eq_ignore_ascii_case(b"<js>"))
        {
            in_tag_js = true;
            in_at_js = false;
            i += 4;
            continue;
        }
        if in_tag_js
            && bytes
                .get(i..i + 5)
                .is_some_and(|s| s.eq_ignore_ascii_case(b"</js>"))
        {
            in_tag_js = false;
            i += 5;
            continue;
        }
        if !in_tag_js
            && !in_at_js
            && bytes
                .get(i..i + 4)
                .is_some_and(|s| s.eq_ignore_ascii_case(b"@js:"))
        {
            in_at_js = true;
            i += 4;
            continue;
        }
        if !in_tag_js {
            if bytes[i] == b'\n' || bytes[i] == b'\r' {
                entries.push(&raw[start..i]);
                in_at_js = false;
                i += 1;
                start = i;
                continue;
            }
            if !in_at_js && bytes.get(i..i + 2) == Some(b"&&") {
                entries.push(&raw[start..i]);
                i += 2;
                start = i;
                continue;
            }
        }
        i += 1;
    }
    if start < bytes.len() {
        entries.push(&raw[start..]);
    }
    entries
}

fn split_title_and_explore_url(entry: &str) -> Option<(&str, &str)> {
    let lower = entry.to_ascii_lowercase();
    let js_offset = [lower.find("@js:"), lower.find("<js>")]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or(entry.len());
    let (title, url) = entry.split_once("::")?;
    if title.len() < js_offset {
        Some((title, url))
    } else {
        None
    }
}

pub fn parse_explore_categories(raw: &str, base_url: &str) -> Vec<OnlineMusicExploreCategory> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    if trimmed.starts_with('[')
        && let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(trimmed)
    {
        let mut out = Vec::new();
        for item in arr {
            let title = item
                .get("title")
                .or_else(|| item.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            let url = item
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            if !title.is_empty() && !url.is_empty() {
                out.push(OnlineMusicExploreCategory {
                    title: title.to_string(),
                    url: normalize_explore_category_url(url, base_url),
                });
            }
        }
        if !out.is_empty() {
            return out;
        }
    }

    let mut out = Vec::new();
    for raw_entry in split_explore_entries_outside_js(trimmed) {
        let entry = raw_entry.trim();
        if entry.is_empty() {
            continue;
        }
        if let Some((title, url)) = split_title_and_explore_url(entry) {
            let t = title.trim();
            let u = url.trim();
            if !t.is_empty() && !u.is_empty() {
                out.push(OnlineMusicExploreCategory {
                    title: t.to_string(),
                    url: normalize_explore_category_url(u, base_url),
                });
            }
        } else if entry.starts_with("http://")
            || entry.starts_with("https://")
            || entry.starts_with('/')
            || split_js_pipeline(entry).is_some()
        {
            out.push(OnlineMusicExploreCategory {
                title: "默认榜单".to_string(),
                url: normalize_explore_category_url(entry, base_url),
            });
        }
    }
    out
}

fn fetch_api(
    session: &MusicSourceSession,
    target_url: &str,
    base_url: &str,
) -> Result<xmedia_source::http::HttpFetchResult, String> {
    let fetched = session.http().fetch(target_url, base_url)?;
    if fetched.status >= 400 {
        return Err(format!(
            "上游接口返回异常状态 HTTP {} ({})",
            fetched.status, fetched.final_url
        ));
    }
    Ok(fetched)
}

fn non_empty_opt(s: String) -> Option<String> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

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
        source: &MusicSourceSpec,
        mut initial_state: SourceRuntimeState,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<MusicSourceSession, String> {
        initial_state.ensure_android_id(&source.source_url);
        let cookies_arc = Arc::new(Mutex::new(initial_state.cookies.clone()));
        let state_arc = Arc::new(Mutex::new(initial_state));
        let http = LegadoHttpClient::new(
            self.http_config.clone(),
            cookies_arc.clone(),
            source.header.as_deref(),
            source.enabled_cookie_jar,
        )?;
        Ok(MusicSourceSession::new(
            source,
            http,
            state_arc,
            cookies_arc,
            self.js_timeout,
            op_timeout,
            cancel_flag,
        ))
    }

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
            let rule = source
                .rule_search
                .as_ref()
                .ok_or_else(|| format!("音源「{}」未配置 ruleSearch", source.source_name))?;
            let raw_tpl = source
                .search_url
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| format!("音源「{}」未配置 searchUrl", source.source_name))?;

            let page_clamped = page.max(1);
            let limit_clamped = limit.clamp(1, 50);
            session.set_search_vars(keyword, page_clamped, limit_clamped)?;

            let vars = MusicTemplateVars {
                key: keyword,
                page: page_clamped,
                limit: limit_clamped,
                ..Default::default()
            };
            let empty_input = RuleValue::Text(String::new());
            let evaluated = eval_template_expression(
                &mut session,
                raw_tpl,
                &vars,
                &empty_input,
                &source.source_url,
            )?;
            if evaluated.trim().is_empty() {
                return Err("searchUrl 求值结果为空".into());
            }
            let target_url = resolve_relative_url(evaluated.trim(), &source.source_url);
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
                .map(|raw| parse_explore_categories(raw, &source.source_url))
                .unwrap_or_default();

            let rule = source
                .rule_explore
                .as_ref()
                .or(source.rule_search.as_ref())
                .ok_or_else(|| format!("音源「{}」未配置浏览解析规则", source.source_name))?;

            let page_clamped = page.max(1);
            let limit_clamped = limit.clamp(1, 50);

            let override_trimmed = explore_url_override
                .map(str::trim)
                .filter(|s| !s.is_empty());

            let target_url = if let Some(raw_override) = override_trimmed {
                if let Some(matched_cat) = categories.iter().find(|c| c.url == raw_override) {
                    session.set_search_vars("", page_clamped, limit_clamped)?;
                    let vars = MusicTemplateVars {
                        page: page_clamped,
                        limit: limit_clamped,
                        ..Default::default()
                    };
                    let empty_input = RuleValue::Text(String::new());
                    let evaluated = eval_template_expression(
                        &mut session,
                        &matched_cat.url,
                        &vars,
                        &empty_input,
                        &source.source_url,
                    )?;
                    if evaluated.trim().is_empty() {
                        return Err("exploreUrl 求值结果为空".into());
                    }
                    resolve_relative_url(evaluated.trim(), &source.source_url)
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
                let vars = MusicTemplateVars {
                    page: page_clamped,
                    limit: limit_clamped,
                    ..Default::default()
                };
                let empty_input = RuleValue::Text(String::new());
                let evaluated = eval_template_expression(
                    &mut session,
                    &default_cat.url,
                    &vars,
                    &empty_input,
                    &source.source_url,
                )?;
                if evaluated.trim().is_empty() {
                    return Err("exploreUrl 求值结果为空".into());
                }
                resolve_relative_url(evaluated.trim(), &source.source_url)
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
                let evaluated_url = eval_template_expression(
                    &mut session,
                    req_tpl,
                    &vars,
                    &empty_input,
                    &source.source_url,
                )?;
                if evaluated_url.trim().is_empty() {
                    return Err("ruleTrackDetail.requestUrl 求值结果为空".into());
                }
                resolve_relative_url(evaluated_url.trim(), &source.source_url)
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
            let effective_quality = requested_quality
                .or_else(|| source.supported_qualities.first().map(String::as_str))
                .unwrap_or("320");

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
                let resolved_url = resolve_relative_url(direct, &source.source_url);
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
                .filter(|s| s.starts_with("http://") || s.starts_with("https://"));
            let has_rule_headers = rule
                .headers
                .as_deref()
                .is_some_and(|s| !s.trim().is_empty());
            let static_metadata_only = is_static_rule_literal(rule.format.as_deref())
                && is_static_rule_literal(rule.bitrate.as_deref())
                && is_static_rule_literal(rule.expires_in_secs.as_deref());
            let quality_matches_or_absent = requested_quality.is_none_or(|q| {
                source.supported_qualities.len() == 1
                    && source.supported_qualities[0].eq_ignore_ascii_case(q)
            });

            if let Some(req_tpl) = rule
                .request_url
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                // Short-circuit only when all metadata/headers are static and request_url doesn't depend on quality or JS.
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
                        .or_else(|| infer_audio_format(direct))
                        .unwrap_or_else(|| "mp3".into());
                    let bitrate = rule
                        .bitrate
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .and_then(|s| s.parse::<u32>().ok());
                    return Ok(OnlineMusicStreamResult {
                        stream_url: direct.to_string(),
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

                let evaluated_req = eval_template_expression(
                    &mut session,
                    req_tpl,
                    &vars,
                    &empty_input,
                    &source.source_url,
                )?;
                if evaluated_req.trim().is_empty() {
                    return Err("ruleStream.requestUrl 求值结果为空".into());
                }
                let target_url = resolve_relative_url(evaluated_req.trim(), &source.source_url);
                let fetched = fetch_api(&session, &target_url, &source.source_url)?;
                let input = RuleValue::from_raw(fetched.body);

                let stream_rule = rule
                    .stream_url
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| "ruleStream 缺少 streamUrl 规则".to_string())?;
                let stream_raw = session.eval_string(&input, stream_rule, &fetched.final_url)?;
                let stream_trimmed = stream_raw.trim();
                if stream_trimmed.is_empty() {
                    return Err("未能从 ruleStream 解析出有效音频流地址".into());
                }
                let resolved_stream = resolve_relative_url(stream_trimmed, &fetched.final_url);

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
                // Direct/template stream evaluation (no HTTP fetch on audio stream URL!).
                let resolved_stream = if let Some(tpl) = rule
                    .stream_url
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    eval_template_expression(
                        &mut session,
                        tpl,
                        &vars,
                        &empty_input,
                        &source.source_url,
                    )?
                } else {
                    ctx.play_url.clone().unwrap_or_default()
                };

                let trimmed = resolved_stream.trim();
                if trimmed.is_empty() {
                    return Err("未能解析出有效音频流地址".into());
                }
                let final_url = resolve_relative_url(trimmed, &source.source_url);
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
            if let Some(ref existing) = ctx.cover_url
                && !existing.trim().is_empty()
                && source
                    .rule_cover
                    .as_ref()
                    .and_then(|r| r.request_url.as_deref())
                    .map(str::trim)
                    .is_none_or(str::is_empty)
            {
                return Ok(Some(resolve_relative_url(existing, &source.source_url)));
            }

            let Some(ref rule) = source.rule_cover else {
                return Ok(ctx
                    .cover_url
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|u| resolve_relative_url(u, &source.source_url)));
            };

            let vars = MusicTemplateVars::from_track(ctx, ctx.quality.as_deref().unwrap_or(""));
            let empty_input = RuleValue::Text(String::new());

            if let Some(req_tpl) = rule
                .request_url
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                let has_cover_or_id = ctx
                    .cover_id
                    .as_deref()
                    .is_some_and(|s| !s.trim().is_empty())
                    || !ctx.track_id.trim().is_empty();
                if !has_cover_or_id {
                    return Ok(ctx.cover_url.clone());
                }
                let evaluated_req = eval_template_expression(
                    &mut session,
                    req_tpl,
                    &vars,
                    &empty_input,
                    &source.source_url,
                )?;
                if evaluated_req.trim().is_empty() {
                    return Ok(ctx.cover_url.clone());
                }
                let target_url = resolve_relative_url(evaluated_req.trim(), &source.source_url);
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
                let cover_raw = eval_template_expression(
                    &mut session,
                    cover_tpl,
                    &vars,
                    &empty_input,
                    &source.source_url,
                )?;
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
                    let evaluated_req = eval_template_expression(
                        &mut session,
                        req_tpl,
                        &vars,
                        &empty_input,
                        &source.source_url,
                    )?;
                    if evaluated_req.trim().is_empty() {
                        return Ok(None);
                    }
                    let target_url = resolve_relative_url(evaluated_req.trim(), &source.source_url);
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
                    let evaluated_req = eval_template_expression(
                        &mut session,
                        req_tpl,
                        &vars,
                        &empty_input,
                        &source.source_url,
                    )?;
                    if evaluated_req.trim().is_empty() {
                        None
                    } else {
                        Some(resolve_relative_url(
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

            let mut parsed = parse_lyrics_str(&raw_lyric);
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
                let t_parsed = parse_lyrics_str(raw_t);
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

        let login_url_trimmed = source.login_url.as_deref().unwrap_or("").trim();
        let login_url = if login_url_trimmed
            .get(..4)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("@js:"))
        {
            login_url_trimmed[4..].trim()
        } else {
            login_url_trimmed
        };
        if login_url.trim().is_empty() && action_js.is_none_or(|a| a.trim().is_empty()) {
            return Err(SourceCallError::new(
                "当前音源未配置 loginUrl 或可执行的登录脚本",
                ExecutionDiagnostics::default(),
            ));
        }

        match session.eval_login_script(login_url, action_js, &merged_login) {
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

#[cfg(test)]
mod tests {
    use super::split_top_level_or;

    #[test]
    fn split_top_level_or_ignores_depth_zero_unmatched_apostrophes_and_preserves_scoped_quotes() {
        assert_eq!(
            split_top_level_or("$.don't || $.fallback"),
            vec!["$.don't", "$.fallback"]
        );
        assert_eq!(
            split_top_level_or("{{'a||b'}} || $.fallback"),
            vec!["{{'a||b'}}", "$.fallback"]
        );
        assert_eq!(
            split_top_level_or("$.items[?(@.a=='x||y')] || $.fallback"),
            vec!["$.items[?(@.a=='x||y')]", "$.fallback"]
        );
    }
}

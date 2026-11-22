use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use xmedia_source::http::{
    HttpClientConfig, LegadoHttpClient, eval_json_options_template, has_suspicious_option_chars,
    resolve_relative_url, resolve_relative_url_preserving_options,
    split_legado_url_and_raw_options, split_legado_url_template_and_options,
    substitute_legado_options_vars, substitute_url_part_vars, verify_js_pipeline_url_options,
};
use xmedia_source::model::{
    ExecutionDiagnostics, SourceCallError, SourceCallOutput, SourceRuntimeState,
};
use xmedia_source::rule::{RuleValue, split_js_pipeline, strip_js_wrapper};

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
    cover_url: Option<&'a str>,
    raw_ctx: Option<&'a MusicTrackContext>,
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
            cover_url: ctx.cover_url.as_deref(),
            raw_ctx: Some(ctx),
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

impl<'a> MusicTemplateVars<'a> {
    fn to_pairs(&self, page_str: &'a str, limit_str: &'a str) -> [(&'a str, &'a str); 13] {
        [
            ("key", self.key),
            ("page", page_str),
            ("limit", limit_str),
            ("quality", self.quality),
            ("trackId", self.track_id),
            ("urlId", self.url_id),
            ("coverId", self.cover_id),
            ("lyricId", self.lyric_id),
            ("playUrl", self.play_url),
            ("detailUrl", self.detail_url),
            ("title", self.title),
            ("artist", self.artist),
            ("album", self.album),
        ]
    }

    fn has_suspicious_option_chars(&self) -> bool {
        has_suspicious_option_chars(&[
            self.key,
            self.quality,
            self.track_id,
            self.url_id,
            self.cover_id,
            self.lyric_id,
            self.play_url,
            self.detail_url,
            self.title,
            self.artist,
            self.album,
            self.cover_url.unwrap_or(""),
        ])
    }
}

fn detect_passthrough_url_var<'a>(
    url_tpl: &str,
    vars: &'a [(&'a str, &'a str)],
) -> Option<(&'a str, &'a str)> {
    let trimmed = url_tpl.trim();
    if let Some(inner) = trimmed
        .strip_prefix("{{")
        .and_then(|s| s.strip_suffix("}}"))
    {
        let name = inner.trim();
        if matches!(name, "playUrl" | "detailUrl") {
            return vars.iter().find(|(k, _)| *k == name).map(|(k, v)| (*k, *v));
        }
    }
    None
}

fn normalize_quality_placeholder(tpl: &str) -> Cow<'_, str> {
    if !tpl.contains("{{") {
        return Cow::Borrowed(tpl);
    }
    let mut out = String::new();
    let mut rest = tpl;
    let mut changed = false;
    while let Some(open) = rest.find("{{") {
        let after_open = &rest[open + 2..];
        let Some(close) = after_open.find("}}") else {
            if changed {
                out.push_str(rest);
            }
            break;
        };
        let expr = after_open[..close].trim();
        if expr.eq_ignore_ascii_case("quality") && expr != "quality" {
            if !changed {
                out.reserve(tpl.len());
                let consumed = tpl.len() - rest.len();
                out.push_str(&tpl[..consumed]);
                changed = true;
            }
            out.push_str(&rest[..open]);
            out.push_str("{{quality}}");
        } else if changed {
            out.push_str(&rest[..open + 2 + close + 2]);
        }
        rest = &after_open[close + 2..];
    }
    if changed {
        if !rest.is_empty() && !rest.contains("{{") {
            out.push_str(rest);
        }
        Cow::Owned(out)
    } else {
        Cow::Borrowed(tpl)
    }
}

fn probe_non_empty_opt(opt: Option<&str>) -> Option<String> {
    opt.filter(|s| !s.is_empty())
        .map(|_| "__xmedia_probe__".to_string())
}

fn eval_js_pipeline_guarded(
    session: &mut MusicSourceSession,
    raw_tpl: &str,
    vars: &MusicTemplateVars<'_>,
    input: &RuleValue,
    base_url: &str,
) -> Result<String, String> {
    let out = session.eval_string(input, raw_tpl, base_url)?;
    if vars.has_suspicious_option_chars()
        && split_legado_url_and_raw_options(out.trim()).1.is_some()
    {
        let probe_key = if vars.key.is_empty() {
            ""
        } else {
            "__xmedia_probe__"
        };
        let probe_ctx = vars.raw_ctx.map(|orig| MusicTrackContext {
            track_id: if orig.track_id.is_empty() {
                String::new()
            } else {
                "__xmedia_probe__".into()
            },
            title: if orig.title.is_empty() {
                String::new()
            } else {
                "__xmedia_probe__".into()
            },
            artist: if orig.artist.is_empty() {
                String::new()
            } else {
                "__xmedia_probe__".into()
            },
            album: probe_non_empty_opt(orig.album.as_deref()),
            duration_secs: orig.duration_secs,
            cover_url: probe_non_empty_opt(orig.cover_url.as_deref()),
            cover_id: probe_non_empty_opt(orig.cover_id.as_deref()),
            url_id: probe_non_empty_opt(orig.url_id.as_deref()),
            lyric_id: probe_non_empty_opt(orig.lyric_id.as_deref()),
            play_url: probe_non_empty_opt(orig.play_url.as_deref()),
            detail_url: probe_non_empty_opt(orig.detail_url.as_deref()),
            quality: (!vars.quality.is_empty()).then(|| "__xmedia_probe__".into()),
        });
        let probe_res = match session.set_search_vars(probe_key, vars.page, vars.limit) {
            Ok(()) => {
                if let Some(ref pctx) = probe_ctx {
                    session
                        .set_track_context(pctx)
                        .and_then(|()| session.eval_string(input, raw_tpl, base_url))
                } else {
                    session.eval_string(input, raw_tpl, base_url)
                }
            }
            Err(e) => Err(e),
        };

        // Restore caller vars/context unconditionally before checking probe result.
        let _ = session.set_search_vars(vars.key, vars.page, vars.limit);
        if let Some(orig) = vars.raw_ctx {
            let _ = session.set_track_context(orig);
        }

        let probe_out = probe_res?;
        verify_js_pipeline_url_options(&out, &probe_out, "请求 URL")?;
    }
    Ok(out)
}

fn eval_template_branch_single_pass(
    session: &mut MusicSourceSession,
    branch: &str,
    vars: &MusicTemplateVars<'_>,
    input: &RuleValue,
    base_url: &str,
) -> Result<String, String> {
    let normalized_branch = normalize_quality_placeholder(branch);
    let (raw_url_part, opt_part) = split_legado_url_template_and_options(&normalized_branch);

    if opt_part.is_none() {
        if split_js_pipeline(&normalized_branch).is_some() {
            return eval_js_pipeline_guarded(session, &normalized_branch, vars, input, base_url);
        }
        if !normalized_branch.contains("{{") || !normalized_branch.contains("}}") {
            if normalized_branch.starts_with("$.")
                || normalized_branch.starts_with("$[")
                || normalized_branch == "$"
                || normalized_branch.starts_with('@')
            {
                return Ok(session.eval_opt(input, Some(&normalized_branch), base_url));
            }
            return Ok(normalized_branch.into_owned());
        }
    }

    let page_str = vars.page.to_string();
    let limit_str = vars.limit.to_string();
    let var_pairs = vars.to_pairs(&page_str, &limit_str);

    let rendered_url = if split_js_pipeline(raw_url_part).is_some() {
        eval_js_pipeline_guarded(session, raw_url_part, vars, input, base_url)?
    } else if let Some((var_name, literal_val)) =
        detect_passthrough_url_var(raw_url_part, &var_pairs)
    {
        if literal_val.trim().is_empty() {
            String::new()
        } else {
            resolve_literal_http_url(literal_val, base_url, var_name)?
        }
    } else {
        let pre_url = substitute_url_part_vars(raw_url_part, &var_pairs, None);
        let evaluated_url = if pre_url.contains("{{") && pre_url.contains("}}") {
            session.eval_string(input, &pre_url, base_url)?
        } else {
            pre_url
        };
        if split_legado_url_and_raw_options(evaluated_url.trim())
            .1
            .is_some()
        {
            return Err("请求 URL 模板参数不允许附加请求选项".into());
        }
        evaluated_url
    };

    if rendered_url.trim().is_empty() {
        return Ok(String::new());
    }

    if let Some(raw_opts) = opt_part {
        let rendered_opts = if template_needs_js(raw_opts) {
            eval_json_options_template(
                raw_opts,
                &var_pairs,
                |expr_tpl| session.eval_string(input, expr_tpl, base_url),
                "音源请求 URL",
            )?
        } else {
            substitute_legado_options_vars(raw_opts, &var_pairs).map_err(|e| e.to_string())?
        };
        Ok(format!("{},{rendered_opts}", rendered_url.trim()))
    } else {
        Ok(rendered_url)
    }
}

fn eval_template_expression(
    session: &mut MusicSourceSession,
    raw_tpl: &str,
    vars: &MusicTemplateVars<'_>,
    input: &RuleValue,
    base_url: &str,
) -> Result<String, String> {
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

pub(crate) fn is_http_url(u: &str) -> bool {
    let trimmed = u.trim();
    trimmed
        .get(..7)
        .is_some_and(|p| p.eq_ignore_ascii_case("http://"))
        || trimmed
            .get(..8)
            .is_some_and(|p| p.eq_ignore_ascii_case("https://"))
}

fn resolve_literal_http_url(raw: &str, base_url: &str, label: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(format!("{label} 不能为空"));
    }
    if split_js_pipeline(trimmed).is_some() {
        return Err(format!("{label} 不允许包含 JS 脚本"));
    }
    xmedia_source::http::resolve_literal_http_url(trimmed, Some(base_url))
        .map_err(|e| format!("{label}: {e}"))
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
    let mut depth_brace = 0usize;
    let mut in_single = false;
    let mut in_double = false;
    while i < bytes.len() {
        let ch = bytes[i];
        if !in_single && !in_double {
            if mustache_depth == 0 && depth_bracket == 0 && depth_paren == 0 && depth_brace == 0 {
                if bytes.len() - i >= 4 && bytes[i..i + 4].eq_ignore_ascii_case(b"<js>") {
                    let mut j = i + 4;
                    let mut found_end = None;
                    while j + 5 <= bytes.len() {
                        if bytes[j..j + 5].eq_ignore_ascii_case(b"</js>") {
                            found_end = Some(j + 5);
                            break;
                        }
                        j += 1;
                    }
                    if let Some(end_idx) = found_end {
                        i = end_idx;
                        continue;
                    }
                }
                if bytes.len() - i >= 4 && bytes[i..i + 4].eq_ignore_ascii_case(b"@js:") {
                    break;
                }
            }
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
            if ch == b'{' {
                depth_brace += 1;
            } else if ch == b'}' && depth_brace > 0 {
                depth_brace -= 1;
            } else if ch == b'[' {
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
                && depth_brace == 0
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
        if (mustache_depth > 0 || depth_bracket > 0 || depth_paren > 0 || depth_brace > 0)
            && ((ch == b'\'' && !in_double) || (ch == b'"' && !in_single))
        {
            let mut backslashes = 0usize;
            let mut k = i;
            while k > 0 && bytes[k - 1] == b'\\' {
                backslashes += 1;
                k -= 1;
            }
            if backslashes.is_multiple_of(2) {
                if ch == b'\'' {
                    in_single = !in_single;
                } else {
                    in_double = !in_double;
                }
            }
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
    resolve_relative_url_preserving_options(trimmed, base_url)
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
    session.check_active()?;
    let remaining = session.remaining_budget();
    if remaining.is_zero() {
        return Err("执行超过单次操作总超时上限 (interrupted)".into());
    }
    let fetched = session
        .http()
        .fetch_with_timeout(target_url, base_url, Some(remaining))?;
    session.check_active()?;
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

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn resolve_literal_http_url_rejects_legado_options_and_data_urls() {
        assert_eq!(
            resolve_literal_http_url("/track/1", "https://music.example.com", "detailUrl").unwrap(),
            "https://music.example.com/track/1"
        );
        assert!(
            resolve_literal_http_url(
                "https://music.example.com/track/1,{\"method\":\"POST\",\"headers\":{\"X-Evil\":\"1\"}}",
                "https://music.example.com",
                "detailUrl",
            )
            .is_err()
        );
        assert!(
            resolve_literal_http_url(
                "data:text/plain,evil",
                "https://music.example.com",
                "detailUrl"
            )
            .is_err()
        );
    }

    #[tokio::test]
    async fn music_engine_neutralizes_param_injection_and_preserves_relative_options() {
        use axum::Router;
        use axum::body::{Body, to_bytes};
        use axum::http::{HeaderMap, StatusCode};
        use axum::response::Response;
        use axum::routing::{any, get};
        use std::sync::atomic::{AtomicUsize, Ordering};

        let admin_hits = Arc::new(AtomicUsize::new(0));
        let admin_hits_c = admin_hits.clone();
        let captured_requests: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
        let cap_c = captured_requests.clone();

        let app = Router::new()
            .route(
                "/admin",
                any(move || {
                    let hits = admin_hits_c.clone();
                    async move {
                        hits.fetch_add(1, Ordering::SeqCst);
                        Response::builder()
                            .status(StatusCode::OK)
                            .body(Body::from(r#"{"url":"https://evil/pwn.mp3"}"#))
                            .unwrap()
                    }
                }),
            )
            .route(
                "/api/{*rest}",
                any(move |req: axum::extract::Request| {
                    let cap = cap_c.clone();
                    async move {
                        let method = req.method().to_string();
                        let uri = req.uri().to_string();
                        let body_bytes = to_bytes(req.into_body(), 64 * 1024)
                            .await
                            .unwrap_or_default();
                        let body = String::from_utf8_lossy(&body_bytes).to_string();
                        cap.lock().unwrap().push((format!("{method} {uri}"), body));
                        let json = if uri.contains("/search") {
                            serde_json::json!({
                                "items": [{ "id": "1", "title": "Song1", "artist": "Art1" }]
                            })
                        } else if uri.contains("/stream") {
                            serde_json::json!({ "url": "/media/1.mp3", "br": "320" })
                        } else if uri.contains("/cover") {
                            serde_json::json!({ "url": "/cov/1.jpg" })
                        } else if uri.contains("/lyric") {
                            serde_json::json!({ "lyric": "[00:01.00]Safe Lyric" })
                        } else {
                            serde_json::json!({ "id": "1", "title": "Song1", "artist": "Art1" })
                        };
                        Response::builder()
                            .status(StatusCode::OK)
                            .header("content-type", "application/json")
                            .body(Body::from(json.to_string()))
                            .unwrap()
                    }
                }),
            )
            .route(
                "/direct.mp3",
                get(|| async { Response::new(Body::from("ok")) }),
            );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let srv = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let base = format!("http://{addr}");

        let spec: MusicSourceSpec = serde_json::from_value(serde_json::json!({
            "sourceName": "RelOptsMusic",
            "sourceUrl": base,
            "searchUrl": "/api/search?q={{key}},{\"method\":\"POST\",\"body\":\"k={{key}}&p={{page}}\"}",
            "ruleSearch": { "trackList": "$.items[*]", "trackId": "$.id", "title": "$.title", "artist": "$.artist" },
            "ruleTrackDetail": { "requestUrl": "/api/tracks/{{trackId}}", "trackId": "$.id", "title": "$.title" },
            "ruleStream": { "requestUrl": "/api/stream?id={{urlId}}&br={{quality}}", "streamUrl": "$.url" },
            "ruleCover": { "requestUrl": "/api/cover/{{coverId}}", "coverUrl": "$.url" },
            "ruleLyric": { "requestUrl": "/api/lyric?id={{lyricId}}", "lyric": "$.lyric" }
        }))
        .unwrap();

        let passthrough_spec: MusicSourceSpec = serde_json::from_value(serde_json::json!({
            "sourceName": "PassthroughMusic",
            "sourceUrl": base,
            "ruleStream": { "streamUrl": "{{playUrl}}" }
        }))
        .unwrap();

        let engine = super::MusicSourceEngine::new(
            HttpClientConfig {
                allow_private_ips: true,
                ..Default::default()
            },
            Duration::from_secs(10),
        );

        let evil_opts = r#"1,{"method":"POST","headers":{"X-Pwned":"1"},"body":"evil=1"}"#;
        let evil_trav = "../../admin";
        let evil_query = "1&extra=1";

        let res = tokio::task::spawn_blocking(move || {
            let search_res = engine
                .search_sync(&spec, SourceRuntimeState::default(), "a&admin=1", 2, 10)
                .unwrap();
            assert_eq!(search_res.data[0].title, "Song1");

            let ctx_stream = MusicTrackContext {
                track_id: evil_trav.into(),
                url_id: Some(evil_opts.into()),
                cover_id: Some(evil_trav.into()),
                lyric_id: Some(evil_query.into()),
                ..Default::default()
            };
            let s_res = engine
                .resolve_stream_sync(
                    &spec,
                    SourceRuntimeState::default(),
                    &ctx_stream,
                    Some(evil_opts),
                )
                .unwrap();
            assert!(s_res.data.stream_url.ends_with("/media/1.mp3"));

            let d_res = engine
                .fetch_track_detail_sync(&spec, SourceRuntimeState::default(), &ctx_stream)
                .unwrap();
            assert_eq!(d_res.data.title, "Song1");

            let c_res = engine
                .resolve_cover_sync(&spec, SourceRuntimeState::default(), &ctx_stream)
                .unwrap();
            assert!(c_res.data.unwrap().ends_with("/cov/1.jpg"));

            let l_res = engine
                .resolve_lyrics_sync(&spec, SourceRuntimeState::default(), &ctx_stream)
                .unwrap();
            assert!(l_res.data.is_some());

            // Passthrough playUrl rejects Legado options & data: URLs
            assert!(
                engine
                    .resolve_stream_sync(
                        &passthrough_spec,
                        SourceRuntimeState::default(),
                        &MusicTrackContext {
                            play_url: Some(format!("{base}/direct.mp3,{{\"method\":\"POST\"}}")),
                            ..Default::default()
                        },
                        None,
                    )
                    .is_err()
            );
            assert!(
                engine
                    .resolve_stream_sync(
                        &passthrough_spec,
                        SourceRuntimeState::default(),
                        &MusicTrackContext {
                            play_url: Some("data:text/html,<h1>x</h1>".into()),
                            ..Default::default()
                        },
                        None,
                    )
                    .is_err()
            );
        })
        .await;
        res.unwrap();

        assert_eq!(admin_hits.load(Ordering::SeqCst), 0);
        let reqs = captured_requests.lock().unwrap();
        assert_eq!(reqs.len(), 5);
        assert_eq!(reqs[0].0, "POST /api/search?q=a%26admin%3D1");
        assert_eq!(reqs[0].1, "k=a%26admin%3D1&p=2");
        assert!(
            reqs[1]
                .0
                .starts_with("GET /api/stream?id=1%2C%7B%22method%22")
        );
        assert!(reqs[1].0.contains("&br=1%2C%7B%22method%22"));
        assert_eq!(reqs[2].0, "GET /api/tracks/..%2F..%2Fadmin");
        assert_eq!(reqs[3].0, "GET /api/cover/..%2F..%2Fadmin");
        assert_eq!(reqs[4].0, "GET /api/lyric?id=1%26extra%3D1");

        let _ = HeaderMap::new();
        srv.abort();
    }

    #[tokio::test]
    async fn music_eval_js_pipeline_probe_preserves_track_cover_url_and_duration() {
        use axum::Router;
        use axum::body::Body;
        use axum::http::StatusCode;
        use axum::response::Response;
        use axum::routing::get;

        let app = Router::new().route(
            "/api/stream",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"ok":true}"#))
                    .unwrap()
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let srv = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let base = format!("http://{addr}");

        let spec: MusicSourceSpec = serde_json::from_value(serde_json::json!({
            "sourceName": "ProbeRestoreMusic",
            "sourceUrl": base,
            "ruleStream": {
                "requestUrl": "@js:'/api/stream?t=' + encodeURIComponent(track.title) + ',{\"method\":\"GET\"}'",
                "streamUrl": "@js:'https://cdn.example.com/audio.mp3?dur=' + track.duration + '&cover=' + encodeURIComponent(track.coverUrl)"
            }
        }))
        .unwrap();

        let engine = super::MusicSourceEngine::new(
            HttpClientConfig {
                allow_private_ips: true,
                ..Default::default()
            },
            Duration::from_secs(5),
        );
        let ctx = MusicTrackContext {
            track_id: "t1".into(),
            title: "Song \"Remix\"".into(),
            artist: "Artist".into(),
            duration_secs: Some(245),
            cover_url: Some("https://cdn.example.com/cover.jpg".into()),
            ..Default::default()
        };

        let out = tokio::task::spawn_blocking(move || {
            engine
                .resolve_stream_sync(&spec, SourceRuntimeState::default(), &ctx, None)
                .unwrap()
        })
        .await
        .unwrap();

        assert_eq!(
            out.data.stream_url,
            "https://cdn.example.com/audio.mp3?dur=245&cover=https%3A%2F%2Fcdn.example.com%2Fcover.jpg"
        );
        srv.abort();
    }

    #[tokio::test]
    async fn music_split_tag_js_url_and_options_evaluates_both_and_blocks_injection() {
        use axum::Router;
        use axum::body::{Body, to_bytes};
        use axum::http::StatusCode;
        use axum::response::Response;
        use axum::routing::any;

        let captured: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
        let cap_c = captured.clone();

        let app = Router::new().route(
            "/search",
            any(move |req: axum::extract::Request| {
                let cap = cap_c.clone();
                async move {
                    let method = req.method().to_string();
                    let uri = req.uri().to_string();
                    let body_bytes = to_bytes(req.into_body(), 64 * 1024)
                        .await
                        .unwrap_or_default();
                    let body = String::from_utf8_lossy(&body_bytes).to_string();
                    cap.lock().unwrap().push((format!("{method} {uri}"), body));
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .body(Body::from(
                            r#"{"items":[{"id":"1","title":"SplitSong","artist":"Art"}]}"#,
                        ))
                        .unwrap()
                }
            }),
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let srv = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let base = format!("http://{addr}");

        let split_ok_spec: MusicSourceSpec = serde_json::from_value(serde_json::json!({
            "sourceName": "SplitTagJsMusic",
            "sourceUrl": base,
            "searchUrl": "<js>var sep = 0 || 1; '/search?q=' + encodeURIComponent(key) + '&s=' + sep</js>,{\"method\":\"POST\",\"body\":\"k={{key}}\"}",
            "ruleSearch": { "trackList": "$.items[*]", "trackId": "$.id", "title": "$.title", "artist": "$.artist" }
        }))
        .unwrap();

        let split_unsafe_url_spec: MusicSourceSpec = serde_json::from_value(serde_json::json!({
            "sourceName": "SplitUnsafeUrlMusic",
            "sourceUrl": base,
            "searchUrl": "<js>'/search?q=' + key</js>,{\"method\":\"POST\",\"body\":\"k={{key}}\"}",
            "ruleSearch": { "trackList": "$.items[*]", "trackId": "$.id", "title": "$.title", "artist": "$.artist" }
        }))
        .unwrap();

        let split_or_fallback_spec: MusicSourceSpec = serde_json::from_value(serde_json::json!({
            "sourceName": "SplitOrFallbackMusic",
            "sourceUrl": base,
            "searchUrl": "{{nonExistentVar}} || <js>'/search?q=' + encodeURIComponent(key)</js>,{\"method\":\"POST\",\"body\":\"k={{key}}\"}",
            "ruleSearch": { "trackList": "$.items[*]", "trackId": "$.id", "title": "$.title", "artist": "$.artist" }
        }))
        .unwrap();

        let engine = super::MusicSourceEngine::new(
            HttpClientConfig {
                allow_private_ips: true,
                ..Default::default()
            },
            Duration::from_secs(5),
        );

        tokio::task::spawn_blocking(move || {
            let res = engine
                .search_sync(
                    &split_ok_spec,
                    SourceRuntimeState::default(),
                    "hello world",
                    1,
                    10,
                )
                .unwrap();
            assert_eq!(res.data[0].title, "SplitSong");

            // Option injection via key in <js> url_tpl must be rejected.
            let evil = r#"hello,{"method":"DELETE","headers":{"X-Evil":"1"}}"#;
            let url_err = engine
                .search_sync(
                    &split_unsafe_url_spec,
                    SourceRuntimeState::default(),
                    evil,
                    1,
                    10,
                )
                .expect_err("option injection in <js> url_tpl must be rejected");
            assert!(
                url_err.message.contains("选项"),
                "unexpected error: {url_err}"
            );

            // Breakout in options side is safely form-encoded without altering method/headers.
            let breakout = r#"a","method":"DELETE"#;
            let res2 = engine
                .search_sync(
                    &split_or_fallback_spec,
                    SourceRuntimeState::default(),
                    breakout,
                    1,
                    10,
                )
                .unwrap();
            assert_eq!(res2.data[0].title, "SplitSong");
        })
        .await
        .unwrap();

        let reqs = captured.lock().unwrap();
        assert_eq!(reqs.len(), 2);
        assert_eq!(reqs[0].0, "POST /search?q=hello%20world&s=1");
        assert_eq!(reqs[0].1, "k=hello%20world");
        assert!(reqs[1].0.starts_with("POST /search?q=a%22"));
        assert!(reqs[1].1.starts_with("k=a%22%2C%22method%22%3A%22DELETE"));
        srv.abort();
    }

    #[tokio::test]
    async fn music_eval_js_pipeline_probe_preserves_empty_branching_and_blocks_injection() {
        use axum::Router;
        use axum::body::Body;
        use axum::http::StatusCode;
        use axum::response::Response;
        use axum::routing::{get, post};

        let app = Router::new()
            .route(
                "/api/resolve",
                post(|| async {
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .body(Body::from(
                            r#"{"url":"https://cdn.example.com/resolved.mp3"}"#,
                        ))
                        .unwrap()
                }),
            )
            .route(
                "/api/direct-meta",
                get(|| async {
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .body(Body::from(
                            r#"{"url":"https://cdn.example.com/direct.mp3"}"#,
                        ))
                        .unwrap()
                }),
            );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let srv = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let base = format!("http://{addr}");

        let branching_spec: MusicSourceSpec = serde_json::from_value(serde_json::json!({
            "sourceName": "BranchingStreamMusic",
            "sourceUrl": base,
            "ruleStream": {
                "requestUrl": "@js: if (track.playUrl) { '/api/direct-meta?u=' + encodeURIComponent(track.playUrl); } else { '/api/resolve,' + JSON.stringify({ method: 'POST', body: JSON.stringify({ id: track.trackId, title: track.title }) }); }",
                "streamUrl": "$.url"
            }
        }))
        .unwrap();

        let unsafe_spec: MusicSourceSpec = serde_json::from_value(serde_json::json!({
            "sourceName": "UnsafeBranchingMusic",
            "sourceUrl": base,
            "ruleStream": {
                "requestUrl": "@js: if (track.playUrl) { '/api/direct-meta'; } else { '/api/resolve?t=' + track.title + ',{\"method\":\"GET\"}'; }",
                "streamUrl": "$.url"
            }
        }))
        .unwrap();

        let engine = super::MusicSourceEngine::new(
            HttpClientConfig {
                allow_private_ips: true,
                ..Default::default()
            },
            Duration::from_secs(5),
        );

        tokio::task::spawn_blocking(move || {
            for title in ["Hello, World", "Song [Remix]", "Track \"Quoted\""] {
                let ctx = MusicTrackContext {
                    track_id: "t1".into(),
                    title: title.into(),
                    artist: "Artist".into(),
                    play_url: None,
                    ..Default::default()
                };
                let res = engine
                    .resolve_stream_sync(&branching_spec, SourceRuntimeState::default(), &ctx, None)
                    .unwrap();
                assert_eq!(res.data.stream_url, "https://cdn.example.com/resolved.mp3");
            }

            // Actual option injection via title when playUrl is empty must still be blocked.
            let evil_ctx = MusicTrackContext {
                track_id: "t1".into(),
                title: r#"x,{"method":"DELETE","headers":{"X-Pwn":"1"}}"#.into(),
                artist: "Artist".into(),
                play_url: None,
                ..Default::default()
            };
            let err = engine
                .resolve_stream_sync(&unsafe_spec, SourceRuntimeState::default(), &evil_ctx, None)
                .expect_err("actual option injection must still be rejected");
            assert!(
                err.message.contains("选项") || err.message.contains("结构"),
                "unexpected error: {err}"
            );
        })
        .await
        .unwrap();

        srv.abort();
    }

    #[test]
    fn music_run_login_action_strips_case_insensitive_js_wrappers_and_preserves_http_login_url() {
        let engine = super::MusicSourceEngine::default();
        let cases = [
            (
                "@JS: function login() { return 'upper-at:' + (result.user || ''); }",
                None,
                Some("login()"),
                "upper-at:bob",
            ),
            (
                "<js>function check() { return 'lower-tag:' + (result.user || ''); }</js>",
                Some("@JS: check()"),
                None,
                "lower-tag:bob",
            ),
            (
                "<JS>function check() { return 'upper-tag:' + (result.user || ''); }</JS>",
                Some("<JS>check()</JS>"),
                None,
                "upper-tag:bob",
            ),
            (
                "https://music.example.test/login",
                Some("<JS>'http-check:' + (result.user || '')</JS>"),
                None,
                "http-check:bob",
            ),
        ];

        for (login_url, login_check_js, action, expected) in cases {
            let spec: MusicSourceSpec = serde_json::from_value(serde_json::json!({
                "sourceName": "LoginWrapperMusic",
                "sourceUrl": "https://music.example.test",
                "loginUrl": login_url,
                "loginCheckJs": login_check_js,
            }))
            .unwrap();
            let out = engine
                .run_login_action_sync(
                    &spec,
                    SourceRuntimeState::default(),
                    HashMap::from([("user".to_string(), "bob".to_string())]),
                    action,
                )
                .unwrap();
            assert_eq!(out.data, expected);
        }

        // Plain HTTP loginUrl with no action or loginCheckJs still fails cleanly.
        let empty_action_spec: MusicSourceSpec = serde_json::from_value(serde_json::json!({
            "sourceName": "PlainHttpOnlyMusic",
            "sourceUrl": "https://music.example.test",
            "loginUrl": "https://music.example.test/login",
        }))
        .unwrap();
        assert!(
            engine
                .run_login_action_sync(
                    &empty_action_spec,
                    SourceRuntimeState::default(),
                    HashMap::new(),
                    None,
                )
                .is_err()
        );
    }

    #[test]
    fn split_top_level_or_handles_consecutive_backslashes_before_closing_quotes() {
        assert_eq!(
            split_top_level_or(r#"{{java.get("missing\\")}} || fallback"#),
            vec![r#"{{java.get("missing\\")}}"#, "fallback"]
        );
        assert_eq!(
            split_top_level_or(r#"{{java.get('missing\\')}} || fallback"#),
            vec![r#"{{java.get('missing\\')}}"#, "fallback"]
        );
        assert_eq!(
            split_top_level_or(r#"{{java.get("a\\\"b || c")}} || fallback"#),
            vec![r#"{{java.get("a\\\"b || c")}}"#, "fallback"]
        );
        assert_eq!(
            split_top_level_or(r#"{{java.get('a\\\'b || c')}} || fallback"#),
            vec![r#"{{java.get('a\\\'b || c')}}"#, "fallback"]
        );
        assert_eq!(
            split_top_level_or(r#"$.don't || $.fallback"#),
            vec!["$.don't", "$.fallback"]
        );
    }

    #[tokio::test]
    async fn music_resolve_stream_handles_backslash_fallback_and_uppercase_quality_in_js_pipelines()
    {
        use axum::{Router, routing::get};

        let app = Router::new()
            .route("/meta", get(|| async { r#"{"url":"/cdn/meta.mp3"}"# }))
            .route(
                "/flac.mp3",
                get(|axum::extract::Query(q): axum::extract::Query<HashMap<String, String>>| async move {
                    let v = q.get("v").cloned().unwrap_or_default();
                    serde_json::json!({ "url": format!("https://cdn.example/req-flac.mp3?v={v}") })
                        .to_string()
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base = format!("http://{addr}");
        let srv = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let base_c = base.clone();
        tokio::task::spawn_blocking(move || {
            let engine = super::MusicSourceEngine::new(
                HttpClientConfig {
                    allow_private_ips: true,
                    ..Default::default()
                },
                Duration::from_secs(5),
            );
            let ctx = MusicTrackContext {
                track_id: "t1".into(),
                title: "Song".into(),
                artist: "Artist".into(),
                ..Default::default()
            };

            // 1. Consecutive backslashes before closing quote must still trigger `||` fallback.
            for req_tpl in [
                format!(r#"{{{{java.get("missing\\")}}}} || {base_c}/meta"#),
                format!(r#"{{{{java.get('missing\\')}}}} || {base_c}/meta"#),
            ] {
                let spec: MusicSourceSpec = serde_json::from_value(serde_json::json!({
                    "sourceName": "BackslashFallbackMusic",
                    "sourceUrl": base_c,
                    "ruleStream": {
                        "requestUrl": req_tpl,
                        "streamUrl": "$.url"
                    }
                }))
                .unwrap();
                let out = engine
                    .resolve_stream_sync(&spec, SourceRuntimeState::default(), &ctx, Some("flac"))
                    .unwrap();
                assert_eq!(out.data.stream_url, format!("{base_c}/cdn/meta.mp3"));
            }

            // 2. {{QUALITY}} inside no-options JS pipeline requestUrl AND inside streamUrl
            let req_js_spec: MusicSourceSpec = serde_json::from_value(serde_json::json!({
                "sourceName": "QualityNoOptsReqJsMusic",
                "sourceUrl": base_c,
                "ruleStream": {
                    "requestUrl": format!("{base_c}/{{{{QUALITY}}}}.mp3@js:result + '?v=1'"),
                    "streamUrl": "$.url"
                }
            }))
            .unwrap();
            let req_out = engine
                .resolve_stream_sync(
                    &req_js_spec,
                    SourceRuntimeState::default(),
                    &ctx,
                    Some("flac"),
                )
                .unwrap();
            assert_eq!(
                req_out.data.stream_url,
                "https://cdn.example/req-flac.mp3?v=1"
            );

            let fetched_stream_js_spec: MusicSourceSpec =
                serde_json::from_value(serde_json::json!({
                    "sourceName": "QualityStreamJsFetchedMusic",
                    "sourceUrl": base_c,
                    "ruleStream": {
                        "requestUrl": format!("{base_c}/meta"),
                        "streamUrl": "https://cdn.example/{{QUALITY}}.mp3@js:result + '?v=1'"
                    }
                }))
                .unwrap();
            let fetched_out = engine
                .resolve_stream_sync(
                    &fetched_stream_js_spec,
                    SourceRuntimeState::default(),
                    &ctx,
                    Some("flac"),
                )
                .unwrap();
            assert_eq!(
                fetched_out.data.stream_url,
                "https://cdn.example/flac.mp3?v=1"
            );

            let direct_stream_js_spec: MusicSourceSpec =
                serde_json::from_value(serde_json::json!({
                    "sourceName": "QualityStreamJsDirectMusic",
                    "sourceUrl": base_c,
                    "ruleStream": {
                        "streamUrl": "https://cdn.example/{{QUALITY}}.mp3@js:result + '?v=1'"
                    }
                }))
                .unwrap();
            let direct_out = engine
                .resolve_stream_sync(
                    &direct_stream_js_spec,
                    SourceRuntimeState::default(),
                    &ctx,
                    Some("flac"),
                )
                .unwrap();
            assert_eq!(
                direct_out.data.stream_url,
                "https://cdn.example/flac.mp3?v=1"
            );
        })
        .await
        .unwrap();

        srv.abort();
    }

    #[tokio::test]
    async fn music_fetch_api_clamps_single_and_multistep_requests_to_remaining_budget() {
        use axum::{Router, routing::get};
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::time::Instant;

        let slow_stream_hits = Arc::new(AtomicUsize::new(0));
        let step1_hits = Arc::new(AtomicUsize::new(0));
        let step2_hits = Arc::new(AtomicUsize::new(0));

        let slow_stream_hits_c = slow_stream_hits.clone();
        let step1_hits_c = step1_hits.clone();
        let step2_hits_c = step2_hits.clone();

        let app = Router::new()
            .route(
                "/slow-stream",
                get(move || {
                    let hits = slow_stream_hits_c.clone();
                    async move {
                        hits.fetch_add(1, Ordering::SeqCst);
                        tokio::time::sleep(Duration::from_millis(1200)).await;
                        r#"{"url":"https://cdn.example/late.mp3"}"#
                    }
                }),
            )
            .route(
                "/step1",
                get(move || {
                    let hits = step1_hits_c.clone();
                    async move {
                        hits.fetch_add(1, Ordering::SeqCst);
                        tokio::time::sleep(Duration::from_millis(150)).await;
                        r#"{"token":"ok"}"#
                    }
                }),
            )
            .route(
                "/step2",
                get(move || {
                    let hits = step2_hits_c.clone();
                    async move {
                        hits.fetch_add(1, Ordering::SeqCst);
                        tokio::time::sleep(Duration::from_millis(1200)).await;
                        r#"{"items":[{"id":"1","title":"Late","artist":"Art"}]}"#
                    }
                }),
            );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base = format!("http://{addr}");
        let srv = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let base_c = base.clone();
        tokio::task::spawn_blocking(move || {
            let engine = super::MusicSourceEngine::new(
                HttpClientConfig {
                    timeout_secs: 2,
                    allow_private_ips: true,
                    proxy_url: None,
                },
                Duration::from_secs(5),
            );
            let op_timeout = Duration::from_millis(220);
            let cancel = Arc::new(AtomicBool::new(false));

            // 1. Single fetch_api call in resolve_stream_sync_with_control
            let stream_spec: MusicSourceSpec = serde_json::from_value(serde_json::json!({
                "sourceName": "SlowStreamBudgetMusic",
                "sourceUrl": base_c,
                "ruleStream": {
                    "requestUrl": format!("{base_c}/slow-stream"),
                    "streamUrl": "$.url"
                }
            }))
            .unwrap();
            let ctx = MusicTrackContext {
                track_id: "t1".into(),
                title: "Song".into(),
                artist: "Artist".into(),
                ..Default::default()
            };

            let t_single = Instant::now();
            let single_err = engine
                .resolve_stream_sync_with_control(
                    &stream_spec,
                    SourceRuntimeState::default(),
                    &ctx,
                    None,
                    op_timeout,
                    cancel.clone(),
                )
                .expect_err("slow single fetch_api must abort within op_timeout");
            let single_elapsed = t_single.elapsed();
            assert!(
                single_elapsed < Duration::from_millis(500),
                "single fetch_api took {:?} (expected < 500ms), err={single_err}",
                single_elapsed
            );
            assert!(
                single_err.message.contains("超时") || single_err.message.contains("interrupted"),
                "unexpected single_err message: {}",
                single_err.message
            );

            // 2. Multi-step search_sync_with_control: @js:java.ajax(150ms) + fetch_api(1200ms)
            let search_spec: MusicSourceSpec = serde_json::from_value(serde_json::json!({
                "sourceName": "MultiStepBudgetMusic",
                "sourceUrl": base_c,
                "searchUrl": format!("@js:java.ajax('{base_c}/step1'); '{base_c}/step2?q=' + encodeURIComponent(key)"),
                "ruleSearch": {
                    "trackList": "$.items[*]",
                    "trackId": "$.id",
                    "title": "$.title",
                    "artist": "$.artist"
                }
            }))
            .unwrap();

            let t_multi = Instant::now();
            let multi_err = engine
                .search_sync_with_control(
                    &search_spec,
                    SourceRuntimeState::default(),
                    "test",
                    1,
                    10,
                    op_timeout,
                    cancel.clone(),
                )
                .expect_err("multi-step search fetch_api must abort within remaining budget");
            let multi_elapsed = t_multi.elapsed();
            assert!(
                multi_elapsed < Duration::from_millis(500),
                "multi-step search took {:?} (expected < 500ms), err={multi_err}",
                multi_elapsed
            );
            assert!(
                multi_err.message.contains("超时") || multi_err.message.contains("interrupted"),
                "unexpected multi_err message: {}",
                multi_err.message
            );

            // 3. Zero remaining budget rejects immediately before making any HTTP call
            let zero_err = engine
                .resolve_stream_sync_with_control(
                    &stream_spec,
                    SourceRuntimeState::default(),
                    &ctx,
                    None,
                    Duration::ZERO,
                    cancel,
                )
                .expect_err("zero op_timeout must reject before HTTP call");
            assert!(
                zero_err.message.contains("interrupted"),
                "unexpected zero_err message: {}",
                zero_err.message
            );
        })
        .await
        .unwrap();

        assert_eq!(slow_stream_hits.load(Ordering::SeqCst), 1);
        assert_eq!(step1_hits.load(Ordering::SeqCst), 1);
        assert_eq!(step2_hits.load(Ordering::SeqCst), 1);

        srv.abort();
    }
}

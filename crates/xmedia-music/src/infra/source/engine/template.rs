use std::borrow::Cow;
use std::collections::HashMap;

use xmedia_source::http::{
    eval_json_options_template, has_suspicious_option_chars, split_legado_url_and_raw_options,
    split_legado_url_template_and_options, substitute_legado_options_vars,
    substitute_url_part_vars, verify_js_pipeline_url_options,
};
use xmedia_source::rule::{RuleValue, split_js_pipeline};

use super::super::session::MusicSourceSession;
use crate::domain::online::{MusicSourceSpec, MusicTrackContext};

#[derive(Debug, Default)]
pub(super) struct MusicTemplateVars<'a> {
    pub(super) key: &'a str,
    pub(super) page: u32,
    pub(super) limit: u32,
    pub(super) quality: &'a str,
    pub(super) track_id: &'a str,
    pub(super) url_id: &'a str,
    pub(super) cover_id: &'a str,
    pub(super) lyric_id: &'a str,
    pub(super) play_url: &'a str,
    pub(super) detail_url: &'a str,
    pub(super) title: &'a str,
    pub(super) artist: &'a str,
    pub(super) album: &'a str,
    pub(super) cover_url: Option<&'a str>,
    pub(super) raw_ctx: Option<&'a MusicTrackContext>,
}

impl<'a> MusicTemplateVars<'a> {
    pub(super) fn from_track(ctx: &'a MusicTrackContext, quality: &'a str) -> Self {
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

pub(super) fn normalize_quality_placeholder(tpl: &str) -> Cow<'_, str> {
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

pub(super) fn eval_template_expression(
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

pub(super) fn substitute_static_explore_placeholders(raw: &str, page: u32, limit: u32) -> String {
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

pub(super) fn resolve_literal_http_url(
    raw: &str,
    base_url: &str,
    label: &str,
) -> Result<String, String> {
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

pub(super) fn template_references_quality(raw_tpl: &str) -> bool {
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

pub(super) fn split_top_level_or(input: &str) -> Vec<&str> {
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

pub(super) fn is_static_rule_literal(rule_opt: Option<&str>) -> bool {
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

pub(super) fn eval_rule_or_literal(
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

pub(super) fn apply_stream_headers(
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

pub(super) fn fetch_api(
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

pub(super) fn non_empty_opt(s: String) -> Option<String> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

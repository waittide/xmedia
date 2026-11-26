use xmedia_source::http::{
    eval_json_options_template, has_suspicious_option_chars, resolve_relative_url,
    resolve_relative_url_preserving_options, split_legado_url_and_raw_options,
    split_legado_url_template_and_options, substitute_url_part_vars,
    verify_js_pipeline_url_options,
};
use xmedia_source::rule::{RuleValue, split_js_pipeline};

use super::ComicSourceEngine;
use crate::domain::online::{ComicSourceSpec, ExploreCategoryItem};
use crate::infra::source::session::ReusableComicSession;

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
        if expr != "key" && expr != "page" {
            return true;
        }
        rest = &after_open[close + 2..];
    }
    false
}

fn url_encode_component(raw: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(raw.len() * 3);
    for &b in raw.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push('%');
            out.push(HEX[(b >> 4) as usize] as char);
            out.push(HEX[(b & 0x0F) as usize] as char);
        }
    }
    out
}

fn substitute_simple_template(raw_tpl: &str, keyword: &str, page: u32, encode_key: bool) -> String {
    let page_str = page.to_string();
    let encoded_kw = if encode_key {
        std::borrow::Cow::Owned(url_encode_component(keyword))
    } else {
        std::borrow::Cow::Borrowed(keyword)
    };
    let mut out = String::with_capacity(raw_tpl.len() + encoded_kw.len());
    let mut rest = raw_tpl;
    while let Some(open) = rest.find("{{") {
        out.push_str(&rest[..open]);
        let after_open = &rest[open + 2..];
        let Some(close) = after_open.find("}}") else {
            out.push_str(&rest[open..]);
            return out;
        };
        match after_open[..close].trim() {
            "key" => out.push_str(&encoded_kw),
            "page" => out.push_str(&page_str),
            other => {
                out.push_str("{{");
                out.push_str(other);
                out.push_str("}}");
            }
        }
        rest = &after_open[close + 2..];
    }
    out.push_str(rest);
    out
}

pub(super) fn substitute_static_explore_page(raw: &str, page: u32) -> String {
    let page_str = page.to_string();
    let mut out = String::with_capacity(raw.len() + 8);
    let mut rest = raw;
    while let Some(open) = rest.find("{{") {
        out.push_str(&rest[..open]);
        let after_open = &rest[open + 2..];
        let Some(close) = after_open.find("}}") else {
            out.push_str(&rest[open..]);
            return out;
        };
        if after_open[..close].trim() == "page" {
            out.push_str(&page_str);
        } else {
            out.push_str(&rest[open..open + 2 + close + 2]);
        }
        rest = &after_open[close + 2..];
    }
    out.push_str(rest);
    out
}

pub(super) fn has_non_page_mustache(raw: &str) -> bool {
    let mut rest = raw;
    while let Some(open) = rest.find("{{") {
        let after_open = &rest[open + 2..];
        let Some(close) = after_open.find("}}") else {
            return true;
        };
        if after_open[..close].trim() != "page" {
            return true;
        }
        rest = &after_open[close + 2..];
    }
    false
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
    if split_legado_url_and_raw_options(trimmed).1.is_some() {
        return Err(format!("{label} 不允许附加请求选项"));
    }
    let resolved = resolve_relative_url(trimmed, base_url);
    if !resolved.starts_with("http://") && !resolved.starts_with("https://") {
        return Err(format!("{label} 必须是有效的 HTTP(S) URL: {resolved}"));
    }
    Ok(resolved)
}

fn normalize_explore_url(raw_url: &str, base_url: &str) -> String {
    let trimmed = raw_url.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.contains("{{")
        || trimmed.contains("}}")
        || split_js_pipeline(trimmed).is_some()
        || split_legado_url_and_raw_options(trimmed).1.is_some()
    {
        return trimmed.to_string();
    }
    resolve_relative_url(trimmed, base_url)
}

fn parse_explore_text(raw: &str, base_url: &str) -> Vec<ExploreCategoryItem> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    if trimmed.starts_with('[')
        && let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(trimmed)
    {
        let mut out = Vec::new();
        for entry in arr {
            let Some(obj) = entry.as_object() else {
                continue;
            };
            let title = obj
                .get("title")
                .or_else(|| obj.get("name"))
                .and_then(|v| v.as_str())
                .map(str::trim)
                .unwrap_or("");
            let url = obj
                .get("url")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .unwrap_or("");
            if title.is_empty() || url.is_empty() {
                continue;
            }
            out.push(ExploreCategoryItem {
                title: title.to_string(),
                url: normalize_explore_url(url, base_url),
            });
        }
        return out;
    }

    let mut items = Vec::new();
    for line in trimmed.lines() {
        let line_trimmed = line.trim();
        if line_trimmed.is_empty() {
            continue;
        }
        for segment in line_trimmed.split("&&") {
            let seg = segment.trim();
            if seg.is_empty() {
                continue;
            }
            if let Some((title_part, url_part)) = seg.split_once("::") {
                let title = title_part.trim();
                let url = url_part.trim();
                if title.is_empty() || url.is_empty() {
                    continue;
                }
                items.push(ExploreCategoryItem {
                    title: title.to_string(),
                    url: normalize_explore_url(url, base_url),
                });
            } else {
                items.push(ExploreCategoryItem {
                    title: "发现".to_string(),
                    url: normalize_explore_url(seg, base_url),
                });
            }
        }
    }
    items
}

pub(super) fn parse_explore_categories_in_session(
    source: &ComicSourceSpec,
    session: &mut ReusableComicSession,
) -> Result<Vec<ExploreCategoryItem>, String> {
    let Some(raw_explore) = source
        .explore_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        return Ok(Vec::new());
    };
    let evaluated = if split_js_pipeline(raw_explore).is_some() {
        session.set_search_vars("", 1)?;
        session.eval_string(
            &RuleValue::Text(String::new()),
            raw_explore,
            &source.source_url,
        )?
    } else {
        raw_explore.to_string()
    };
    Ok(parse_explore_text(&evaluated, &source.source_url))
}

impl ComicSourceEngine {
    fn eval_js_pipeline_url_guarded(
        session: &mut ReusableComicSession,
        tpl: &str,
        base_url: &str,
        keyword: &str,
        page: u32,
        label: &str,
    ) -> Result<String, String> {
        let input = RuleValue::Text(String::new());
        let out = session.eval_string(&input, tpl, base_url)?;
        if !keyword.is_empty()
            && has_suspicious_option_chars(&[keyword])
            && split_legado_url_and_raw_options(out.trim()).1.is_some()
        {
            let probe_res = match session.set_search_vars("__xmedia_kw_probe__", page) {
                Ok(()) => session.eval_string(&input, tpl, base_url),
                Err(e) => Err(e),
            };
            let _ = session.set_search_vars(keyword, page);
            let probe_out = probe_res?;
            verify_js_pipeline_url_options(&out, &probe_out, label)?;
        }
        Ok(out)
    }

    pub(super) fn eval_url_template(
        &self,
        source: &ComicSourceSpec,
        session: &mut ReusableComicSession,
        raw_tpl: &str,
        keyword: &str,
        page: u32,
        label: &str,
    ) -> Result<String, String> {
        let (raw_url_part, opt_tpl) = split_legado_url_template_and_options(raw_tpl);
        let evaluated = if let Some(raw_opt_json) = opt_tpl {
            let page_str = page.to_string();
            let vars = [("key", keyword), ("page", page_str.as_str())];
            if template_needs_js(raw_tpl) {
                session.set_search_vars(keyword, page)?;
            }
            let eval_url_part = if template_needs_js(raw_url_part) {
                if split_js_pipeline(raw_url_part).is_some() {
                    Self::eval_js_pipeline_url_guarded(
                        session,
                        raw_url_part,
                        &source.source_url,
                        keyword,
                        page,
                        label,
                    )?
                } else {
                    let pre_url = substitute_url_part_vars(raw_url_part, &vars, None);
                    let input = RuleValue::Text(String::new());
                    let out = session.eval_string(&input, &pre_url, &source.source_url)?;
                    if split_legado_url_and_raw_options(out.trim()).1.is_some() {
                        return Err(format!("{label} 模板参数不允许附加请求选项"));
                    }
                    out
                }
            } else {
                let unencoded = substitute_simple_template(raw_url_part, keyword, page, false);
                if split_legado_url_and_raw_options(unencoded.trim())
                    .1
                    .is_some()
                {
                    return Err(format!("{label} 模板参数不允许附加请求选项"));
                }
                substitute_simple_template(raw_url_part, keyword, page, true)
            };
            let needs_js_opts = template_needs_js(raw_opt_json);
            let eval_opt_json = eval_json_options_template(
                raw_opt_json,
                &vars,
                |expr_tpl| {
                    if !needs_js_opts {
                        Ok(expr_tpl.to_string())
                    } else {
                        session.eval_string(
                            &RuleValue::Text(String::new()),
                            expr_tpl,
                            &source.source_url,
                        )
                    }
                },
                label,
            )?;
            format!("{},{eval_opt_json}", eval_url_part.trim())
        } else if split_js_pipeline(raw_tpl).is_some() {
            session.set_search_vars(keyword, page)?;
            Self::eval_js_pipeline_url_guarded(
                session,
                raw_tpl,
                &source.source_url,
                keyword,
                page,
                label,
            )?
        } else {
            let sub = if template_needs_js(raw_tpl) {
                let page_str = page.to_string();
                let vars = [("key", keyword), ("page", page_str.as_str())];
                let pre_tpl = substitute_url_part_vars(raw_tpl, &vars, None);
                session.set_search_vars(keyword, page)?;
                let input = RuleValue::Text(String::new());
                session.eval_string(&input, &pre_tpl, &source.source_url)?
            } else {
                let unencoded = substitute_simple_template(raw_tpl, keyword, page, false);
                if split_legado_url_and_raw_options(unencoded.trim())
                    .1
                    .is_some()
                {
                    return Err(format!("{label} 模板参数不允许附加请求选项"));
                }
                substitute_simple_template(raw_tpl, keyword, page, true)
            };
            if split_legado_url_and_raw_options(sub.trim()).1.is_some() {
                return Err(format!("{label} 模板参数不允许附加请求选项"));
            }
            sub
        };

        let trimmed = evaluated.trim();
        if trimmed.is_empty() {
            return Err(format!("{label} 求值结果为空"));
        }
        Ok(resolve_relative_url_preserving_options(
            trimmed,
            &source.source_url,
        ))
    }
}

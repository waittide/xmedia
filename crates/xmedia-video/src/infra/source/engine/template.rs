use xmedia_source::http::{
    eval_json_options_template, has_suspicious_option_chars,
    resolve_relative_url_preserving_options, split_legado_url_and_raw_options,
    split_legado_url_template_and_options, substitute_legado_options_vars,
    substitute_legado_url_template_vars, substitute_url_part_vars, verify_js_pipeline_url_options,
};
use xmedia_source::js_runtime::ReusableSourceSession;
use xmedia_source::rule::{RuleValue, split_js_pipeline};

use crate::domain::online::VideoJsContext;

use super::VideoSourceEngine;

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
}

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use xmedia_source::http::{
    HttpClientConfig, LegadoHttpClient, LegadoUrlOptions, eval_json_options_template,
    has_suspicious_option_chars, resolve_relative_url, split_legado_url_template_and_options,
    substitute_url_part_vars, verify_js_pipeline_url_options,
};
pub use xmedia_source::model::{SourceCallError, SourceCallOutput};
use xmedia_source::rule::{RuleValue, split_js_pipeline};

use super::session::{ReusableComicContext, ReusableComicSession};
use crate::source::model::{
    ComicChapterJsContext, ComicJsContext, ComicSearchResult, ComicSearchRule, ComicSourceSpec,
    ExecutionDiagnostics, ExploreCategoryItem, OnlineComicChapter, OnlineComicDetail,
    SourceRuntimeState, parse_reading_direction_opt,
};

#[derive(Debug, Clone)]
pub struct ComicSourceEngine {
    http_config: HttpClientConfig,
    js_timeout: Duration,
}

impl Default for ComicSourceEngine {
    fn default() -> Self {
        Self {
            http_config: HttpClientConfig::default(),
            js_timeout: Duration::from_secs(45),
        }
    }
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

fn substitute_static_explore_page(raw: &str, page: u32) -> String {
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

fn has_non_page_mustache(raw: &str) -> bool {
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

fn resolve_literal_http_url(raw: &str, base_url: &str, label: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(format!("{label} 不能为空"));
    }
    if split_js_pipeline(trimmed).is_some() {
        return Err(format!("{label} 不允许包含 JS 脚本"));
    }
    if split_legado_url_and_raw_options(trimmed).is_some() {
        return Err(format!("{label} 不允许附加请求选项"));
    }
    let resolved = resolve_relative_url(trimmed, base_url);
    if !resolved.starts_with("http://") && !resolved.starts_with("https://") {
        return Err(format!("{label} 必须是有效的 HTTP(S) URL: {resolved}"));
    }
    Ok(resolved)
}

fn pick_rule_field<'a>(
    primary: Option<&'a ComicSearchRule>,
    fallback: Option<&'a ComicSearchRule>,
    get: impl Fn(&'a ComicSearchRule) -> Option<&'a str>,
) -> Option<&'a str> {
    primary
        .and_then(&get)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            fallback
                .and_then(&get)
                .map(str::trim)
                .filter(|s| !s.is_empty())
        })
}

pub(super) fn split_legado_url_and_raw_options(input: &str) -> Option<(&str, &str)> {
    let trimmed = input.trim();
    if !trimmed.ends_with('}') {
        return None;
    }
    let bytes = trimmed.as_bytes();
    let mut depth = 0i32;
    let mut in_string = false;

    for idx in (0..bytes.len()).rev() {
        let b = bytes[idx];
        if b == b'"' {
            let mut backslashes = 0usize;
            let mut k = idx;
            while k > 0 && bytes[k - 1] == b'\\' {
                backslashes += 1;
                k -= 1;
            }
            if backslashes.is_multiple_of(2) {
                in_string = !in_string;
            }
            continue;
        }
        if in_string {
            continue;
        }
        match b {
            b'}' => depth += 1,
            b'{' => {
                depth -= 1;
                if depth == 0 {
                    let url_part = trimmed[..idx].trim_end().strip_suffix(',')?;
                    return Some((url_part.trim(), &trimmed[idx..]));
                }
            }
            _ => {}
        }
    }
    None
}

fn normalize_explore_url(raw_url: &str, base_url: &str) -> String {
    let trimmed = raw_url.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.contains("{{")
        || trimmed.contains("}}")
        || split_js_pipeline(trimmed).is_some()
        || split_legado_url_and_raw_options(trimmed).is_some()
    {
        return trimmed.to_string();
    }
    resolve_relative_url(trimmed, base_url)
}

pub(super) fn resolve_relative_url_preserving_options(raw_url: &str, base_url: &str) -> String {
    let trimmed = raw_url.trim();
    if let Some((url_part, opt_json)) = split_legado_url_and_raw_options(trimmed)
        && serde_json::from_str::<LegadoUrlOptions>(opt_json).is_ok()
    {
        let resolved_part = if url_part.trim().is_empty() {
            base_url.to_string()
        } else {
            resolve_relative_url(url_part, base_url)
        };
        return format!("{resolved_part},{opt_json}");
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

fn parse_explore_categories_in_session(
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
    pub fn new(http_config: HttpClientConfig, js_timeout: Duration) -> Self {
        Self {
            http_config,
            js_timeout,
        }
    }

    pub fn prepare_reusable_context_with_cancel(
        &self,
        source: Arc<ComicSourceSpec>,
        mut initial_state: SourceRuntimeState,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<ReusableComicContext, String> {
        initial_state.ensure_android_id(&source.source_url);
        let cookies_arc = Arc::new(Mutex::new(initial_state.cookies.clone()));
        let state_arc = Arc::new(Mutex::new(initial_state));
        let http = LegadoHttpClient::new(
            self.http_config.clone(),
            cookies_arc.clone(),
            source.header.as_deref(),
            source.enabled_cookie_jar,
        )?;
        Ok(ReusableComicContext {
            source,
            http,
            state_arc,
            cookies_arc,
            js_timeout: self.js_timeout,
            cancel_flag,
        })
    }

    fn build_lazy_session(
        &self,
        source: &ComicSourceSpec,
        mut initial_state: SourceRuntimeState,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<ReusableComicSession, String> {
        initial_state.ensure_android_id(&source.source_url);
        let cookies_arc = Arc::new(Mutex::new(initial_state.cookies.clone()));
        let state_arc = Arc::new(Mutex::new(initial_state));

        let http = LegadoHttpClient::new(
            self.http_config.clone(),
            cookies_arc.clone(),
            source.header.as_deref(),
            source.enabled_cookie_jar,
        )?;

        Ok(ReusableComicContext::new_worker_session(
            Arc::new(source.clone()),
            http,
            state_arc,
            cookies_arc,
            self.js_timeout,
            op_timeout,
            cancel_flag,
        ))
    }

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
            && xmedia_source::http::split_legado_url_and_raw_options(out.trim())
                .1
                .is_some()
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

    fn eval_url_template(
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
                    if xmedia_source::http::split_legado_url_and_raw_options(out.trim())
                        .1
                        .is_some()
                    {
                        return Err(format!("{label} 模板参数不允许附加请求选项"));
                    }
                    out
                }
            } else {
                let unencoded = substitute_simple_template(raw_url_part, keyword, page, false);
                if split_legado_url_and_raw_options(unencoded.trim()).is_some() {
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
                if split_legado_url_and_raw_options(unencoded.trim()).is_some() {
                    return Err(format!("{label} 模板参数不允许附加请求选项"));
                }
                substitute_simple_template(raw_tpl, keyword, page, true)
            };
            if split_legado_url_and_raw_options(sub.trim()).is_some() {
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

    fn eval_comic_list(
        &self,
        session: &mut ReusableComicSession,
        request_url: &str,
        primary_rule: Option<&ComicSearchRule>,
        fallback_rule: Option<&ComicSearchRule>,
        rule_label: &str,
    ) -> Result<Vec<ComicSearchResult>, String> {
        session.check_active()?;
        let fetched = session.http_fetch(request_url)?;
        if fetched.status >= 400 {
            return Err(format!(
                "{rule_label}请求失败，HTTP 状态码 {}",
                fetched.status
            ));
        }
        let is_json_ct = fetched
            .headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
            .map(|(_, v)| v.to_ascii_lowercase())
            .is_some_and(|ct| ct.contains("application/json") || ct.contains("+json"));

        let book_list_rule =
            pick_rule_field(primary_rule, fallback_rule, |r| r.book_list.as_deref())
                .ok_or_else(|| format!("{rule_label} 缺少 bookList 规则"))?;
        let is_jsonpath_rule = {
            let trimmed = book_list_rule.trim().trim_start_matches(['-', '+']).trim();
            trimmed.starts_with("@Json:")
                || trimmed.starts_with("@json:")
                || trimmed.starts_with("$.")
                || trimmed.starts_with("$[")
                || trimmed == "$"
        };
        let non_empty_body = !fetched.body.trim().is_empty();
        let raw_input = if is_json_ct || is_jsonpath_rule {
            RuleValue::from_json_or_raw(fetched.body)
        } else {
            RuleValue::from_raw(fetched.body)
        };
        if non_empty_body && (is_json_ct || is_jsonpath_rule) && raw_input.as_json().is_none() {
            return Err("上游返回无效 JSON".into());
        }
        let name_rule = pick_rule_field(primary_rule, fallback_rule, |r| r.name.as_deref());
        let book_url_rule = pick_rule_field(primary_rule, fallback_rule, |r| r.book_url.as_deref());
        let author_rule = pick_rule_field(primary_rule, fallback_rule, |r| r.author.as_deref());
        let cover_rule = pick_rule_field(primary_rule, fallback_rule, |r| r.cover_url.as_deref());
        let intro_rule = pick_rule_field(primary_rule, fallback_rule, |r| r.intro.as_deref());
        let kind_rule = pick_rule_field(primary_rule, fallback_rule, |r| r.kind.as_deref());
        let last_chapter_rule =
            pick_rule_field(primary_rule, fallback_rule, |r| r.last_chapter.as_deref());
        let word_count_rule =
            pick_rule_field(primary_rule, fallback_rule, |r| r.word_count.as_deref());

        let items = session.eval_list(&raw_input, book_list_rule, &fetched.final_url)?;
        let mut results = Vec::with_capacity(items.len());
        for item in items {
            session.check_active()?;
            let title = session.eval_opt(&item, name_rule, &fetched.final_url);
            let comic_url_raw = session.eval_opt(&item, book_url_rule, &fetched.final_url);
            if title.is_empty() || comic_url_raw.is_empty() {
                continue;
            }
            let author = session.eval_opt(&item, author_rule, &fetched.final_url);
            let cover_raw = session.eval_opt(&item, cover_rule, &fetched.final_url);
            let intro = session.eval_opt(&item, intro_rule, &fetched.final_url);
            let kind = session.eval_opt(&item, kind_rule, &fetched.final_url);
            let last_chapter = session.eval_opt(&item, last_chapter_rule, &fetched.final_url);
            let word_count = session.eval_opt(&item, word_count_rule, &fetched.final_url);

            let comic_url =
                resolve_relative_url_preserving_options(&comic_url_raw, &fetched.final_url);
            let cover_url = (!cover_raw.is_empty())
                .then(|| resolve_relative_url(&cover_raw, &fetched.final_url));

            results.push(ComicSearchResult {
                title,
                author,
                comic_url,
                cover_url,
                intro: (!intro.is_empty()).then_some(intro),
                kind: (!kind.is_empty()).then_some(kind),
                last_chapter: (!last_chapter.is_empty()).then_some(last_chapter),
                word_count: (!word_count.is_empty()).then_some(word_count),
            });
        }
        Ok(results)
    }

    pub fn parse_explore_categories(
        &self,
        source: &ComicSourceSpec,
        state: SourceRuntimeState,
    ) -> Result<SourceCallOutput<Vec<ExploreCategoryItem>>, SourceCallError> {
        self.parse_explore_categories_with_control(
            source,
            state,
            self.js_timeout,
            Arc::new(AtomicBool::new(false)),
        )
    }

    pub fn parse_explore_categories_with_control(
        &self,
        source: &ComicSourceSpec,
        state: SourceRuntimeState,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<Vec<ExploreCategoryItem>>, SourceCallError> {
        if source
            .explore_url
            .as_deref()
            .map(str::trim)
            .is_none_or(str::is_empty)
        {
            return Ok(SourceCallOutput {
                data: Vec::new(),
                state,
                diagnostics: ExecutionDiagnostics::default(),
            });
        }

        let mut session = self
            .build_lazy_session(source, state, op_timeout, cancel_flag)
            .map_err(|e| SourceCallError::new(e, ExecutionDiagnostics::default()))?;

        let res = parse_explore_categories_in_session(source, &mut session);

        match res {
            Ok(cats) => {
                let (final_state, diagnostics) = session.finalize_state();
                Ok(SourceCallOutput {
                    data: cats,
                    state: final_state,
                    diagnostics,
                })
            }
            Err(err) => Err(SourceCallError::new(err, session.snapshot_diagnostics())),
        }
    }

    pub fn search_sync(
        &self,
        source: &ComicSourceSpec,
        state: SourceRuntimeState,
        keyword: &str,
        page: u32,
    ) -> Result<SourceCallOutput<Vec<ComicSearchResult>>, String> {
        self.search_sync_with_control(
            source,
            state,
            keyword,
            page,
            self.js_timeout,
            Arc::new(AtomicBool::new(false)),
        )
        .map_err(Into::into)
    }

    pub fn search_sync_with_control(
        &self,
        source: &ComicSourceSpec,
        state: SourceRuntimeState,
        keyword: &str,
        page: u32,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<Vec<ComicSearchResult>>, SourceCallError> {
        let rule_search = source.rule_search.as_ref().ok_or_else(|| {
            SourceCallError::new(
                format!("漫画源「{}」未配置 ruleSearch", source.source_name),
                ExecutionDiagnostics::default(),
            )
        })?;
        let raw_tpl = source
            .search_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                SourceCallError::new(
                    format!("漫画源「{}」未配置 searchUrl", source.source_name),
                    ExecutionDiagnostics::default(),
                )
            })?;

        let mut session = self
            .build_lazy_session(source, state, op_timeout, cancel_flag)
            .map_err(|e| SourceCallError::new(e, ExecutionDiagnostics::default()))?;

        let res: Result<Vec<ComicSearchResult>, String> = (|| {
            session.set_search_vars(keyword, page)?;
            let search_url =
                self.eval_url_template(source, &mut session, raw_tpl, keyword, page, "searchUrl")?;
            self.eval_comic_list(
                &mut session,
                &search_url,
                Some(rule_search),
                None,
                "ruleSearch",
            )
        })();

        match res {
            Ok(results) => {
                let (final_state, diagnostics) = session.finalize_state();
                Ok(SourceCallOutput {
                    data: results,
                    state: final_state,
                    diagnostics,
                })
            }
            Err(err) => Err(SourceCallError::new(err, session.snapshot_diagnostics())),
        }
    }

    pub fn explore_sync_with_control(
        &self,
        source: &ComicSourceSpec,
        state: SourceRuntimeState,
        explore_url: &str,
        page: u32,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<Vec<ComicSearchResult>>, SourceCallError> {
        self.explore_sync_with_categories_and_control(
            source,
            state,
            explore_url,
            page,
            None,
            op_timeout,
            cancel_flag,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn explore_sync_with_categories_and_control(
        &self,
        source: &ComicSourceSpec,
        state: SourceRuntimeState,
        explore_url: &str,
        page: u32,
        precomputed_categories: Option<&[ExploreCategoryItem]>,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<Vec<ComicSearchResult>>, SourceCallError> {
        if !source.enabled_explore {
            return Err(SourceCallError::new(
                format!("漫画源「{}」未启用发现分类", source.source_name),
                ExecutionDiagnostics::default(),
            ));
        }
        if source.rule_explore.is_none() && source.rule_search.is_none() {
            return Err(SourceCallError::new(
                format!(
                    "漫画源「{}」未配置 ruleExplore 或 ruleSearch",
                    source.source_name
                ),
                ExecutionDiagnostics::default(),
            ));
        }
        let tpl = explore_url.trim();
        if tpl.is_empty() {
            return Err(SourceCallError::new(
                "发现分类 URL 不能为空".to_string(),
                ExecutionDiagnostics::default(),
            ));
        }

        let mut session = self
            .build_lazy_session(source, state, op_timeout, cancel_flag)
            .map_err(|e| SourceCallError::new(e, ExecutionDiagnostics::default()))?;

        let res: Result<Vec<ComicSearchResult>, String> = (|| {
            let owned_categories;
            let categories: &[ExploreCategoryItem] = match precomputed_categories {
                Some(cats) => cats,
                None => {
                    owned_categories = parse_explore_categories_in_session(source, &mut session)?;
                    &owned_categories
                }
            };
            let is_configured_cat = categories.iter().any(|c| c.url == tpl);
            let resolved_url = if is_configured_cat {
                session.set_search_vars("", page)?;
                self.eval_url_template(source, &mut session, tpl, "", page, "exploreUrl")?
            } else {
                if split_js_pipeline(tpl).is_some() {
                    return Err("自定义 exploreUrl 不允许包含 JS 脚本".to_string());
                }
                if has_non_page_mustache(tpl) {
                    return Err("自定义 exploreUrl 不允许包含动态模板表达式".to_string());
                }
                if split_legado_url_and_raw_options(tpl).is_some() {
                    return Err("自定义 exploreUrl 不允许附加请求选项".to_string());
                }
                let substituted = substitute_static_explore_page(tpl, page);
                resolve_literal_http_url(&substituted, &source.source_url, "exploreUrl")?
            };
            self.eval_comic_list(
                &mut session,
                &resolved_url,
                source.rule_explore.as_ref(),
                source.rule_search.as_ref(),
                "ruleExplore",
            )
        })();

        match res {
            Ok(results) => {
                let (final_state, diagnostics) = session.finalize_state();
                Ok(SourceCallOutput {
                    data: results,
                    state: final_state,
                    diagnostics,
                })
            }
            Err(err) => Err(SourceCallError::new(err, session.snapshot_diagnostics())),
        }
    }

    pub fn fetch_detail_sync(
        &self,
        source: &ComicSourceSpec,
        state: SourceRuntimeState,
        comic_url: &str,
        fallback_hint: Option<&ComicJsContext>,
    ) -> Result<SourceCallOutput<OnlineComicDetail>, String> {
        self.fetch_detail_sync_with_control(
            source,
            state,
            comic_url,
            fallback_hint,
            self.js_timeout,
            Arc::new(AtomicBool::new(false)),
        )
        .map_err(Into::into)
    }

    pub fn fetch_detail_sync_with_control(
        &self,
        source: &ComicSourceSpec,
        state: SourceRuntimeState,
        comic_url: &str,
        fallback_hint: Option<&ComicJsContext>,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<OnlineComicDetail>, SourceCallError> {
        let mut session = self
            .build_lazy_session(source, state, op_timeout, cancel_flag)
            .map_err(|e| SourceCallError::new(e, ExecutionDiagnostics::default()))?;

        let res: Result<OnlineComicDetail, String> = (|| {
            if let Some(hint) = fallback_hint {
                session.set_comic_context(hint)?;
            }
            session.check_active()?;
            let fetched = session.http_fetch(comic_url)?;
            if fetched.status >= 400 {
                return Err(format!("详情页请求失败: HTTP {}", fetched.status));
            }
            let mut info_input = RuleValue::from_raw(fetched.body);

            let Some(rule_detail) = source.rule_detail.as_ref() else {
                return Ok(OnlineComicDetail {
                    title: fallback_hint.map(|h| h.name.clone()).unwrap_or_default(),
                    author: fallback_hint.map(|h| h.author.clone()).unwrap_or_default(),
                    comic_url: comic_url.to_string(),
                    toc_url: fetched.final_url,
                    cover_url: fallback_hint.and_then(|h| h.cover_url.clone()),
                    intro: fallback_hint.and_then(|h| h.intro.clone()),
                    kind: None,
                    status: None,
                    last_chapter: None,
                    word_count: None,
                    reading_direction: Some(source.effective_reading_direction()),
                });
            };

            if let Some(ref init_rule) = rule_detail.init
                && !init_rule.trim().is_empty()
            {
                let init_out = session.eval_string(&info_input, init_rule, &fetched.final_url)?;
                if !init_out.trim().is_empty() {
                    info_input = RuleValue::from_raw(init_out);
                }
            }

            let name_extracted =
                session.eval_opt(&info_input, rule_detail.name.as_deref(), &fetched.final_url);
            let author_extracted = session.eval_opt(
                &info_input,
                rule_detail.author.as_deref(),
                &fetched.final_url,
            );
            let cover_extracted = session.eval_opt(
                &info_input,
                rule_detail.cover_url.as_deref(),
                &fetched.final_url,
            );
            let intro_extracted = session.eval_opt(
                &info_input,
                rule_detail.intro.as_deref(),
                &fetched.final_url,
            );
            let kind_extracted =
                session.eval_opt(&info_input, rule_detail.kind.as_deref(), &fetched.final_url);
            let status_extracted = session.eval_opt(
                &info_input,
                rule_detail.status.as_deref(),
                &fetched.final_url,
            );
            let last_chapter_extracted = session.eval_opt(
                &info_input,
                rule_detail.last_chapter.as_deref(),
                &fetched.final_url,
            );
            let word_count_extracted = session.eval_opt(
                &info_input,
                rule_detail.word_count.as_deref(),
                &fetched.final_url,
            );
            let reading_direction = rule_detail
                .reading_direction
                .as_deref()
                .and_then(parse_reading_direction_opt)
                .or_else(|| {
                    let dir_extracted = session.eval_opt(
                        &info_input,
                        rule_detail.reading_direction.as_deref(),
                        &fetched.final_url,
                    );
                    parse_reading_direction_opt(&dir_extracted)
                })
                .or_else(|| Some(source.effective_reading_direction()));
            let toc_raw = session.eval_opt(
                &info_input,
                rule_detail.toc_url.as_deref(),
                &fetched.final_url,
            );

            let toc_trimmed = toc_raw.trim();
            let toc_url = if toc_trimmed.is_empty() {
                fetched.final_url.clone()
            } else {
                resolve_relative_url_preserving_options(toc_trimmed, &fetched.final_url)
            };

            Ok(OnlineComicDetail {
                title: if !name_extracted.is_empty() {
                    name_extracted
                } else {
                    fallback_hint.map(|h| h.name.clone()).unwrap_or_default()
                },
                author: if !author_extracted.is_empty() {
                    author_extracted
                } else {
                    fallback_hint.map(|h| h.author.clone()).unwrap_or_default()
                },
                comic_url: comic_url.to_string(),
                toc_url,
                cover_url: if !cover_extracted.is_empty() {
                    Some(resolve_relative_url(&cover_extracted, &fetched.final_url))
                } else {
                    fallback_hint.and_then(|h| h.cover_url.clone())
                },
                intro: if !intro_extracted.is_empty() {
                    Some(intro_extracted)
                } else {
                    fallback_hint.and_then(|h| h.intro.clone())
                },
                kind: (!kind_extracted.is_empty()).then_some(kind_extracted),
                status: (!status_extracted.is_empty()).then_some(status_extracted),
                last_chapter: (!last_chapter_extracted.is_empty())
                    .then_some(last_chapter_extracted),
                word_count: (!word_count_extracted.is_empty()).then_some(word_count_extracted),
                reading_direction,
            })
        })();

        match res {
            Ok(detail) => {
                let (final_state, diagnostics) = session.finalize_state();
                Ok(SourceCallOutput {
                    data: detail,
                    state: final_state,
                    diagnostics,
                })
            }
            Err(err) => Err(SourceCallError::new(err, session.snapshot_diagnostics())),
        }
    }

    pub fn fetch_chapters_sync(
        &self,
        source: &ComicSourceSpec,
        state: SourceRuntimeState,
        toc_url: &str,
        comic_ctx: &ComicJsContext,
    ) -> Result<SourceCallOutput<Vec<OnlineComicChapter>>, String> {
        self.fetch_chapters_sync_with_control(
            source,
            state,
            toc_url,
            comic_ctx,
            self.js_timeout,
            Arc::new(AtomicBool::new(false)),
        )
        .map_err(Into::into)
    }

    pub fn fetch_chapters_sync_with_control(
        &self,
        source: &ComicSourceSpec,
        state: SourceRuntimeState,
        toc_url: &str,
        comic_ctx: &ComicJsContext,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<Vec<OnlineComicChapter>>, SourceCallError> {
        let mut session = self
            .build_lazy_session(source, state, op_timeout, cancel_flag)
            .map_err(|e| SourceCallError::new(e, ExecutionDiagnostics::default()))?;
        match session.fetch_chapters(toc_url, comic_ctx) {
            Ok(chapters) => {
                let (final_state, diagnostics) = session.finalize_state();
                Ok(SourceCallOutput {
                    data: chapters,
                    state: final_state,
                    diagnostics,
                })
            }
            Err(err) => Err(SourceCallError::new(err, session.snapshot_diagnostics())),
        }
    }

    pub fn fetch_pages_sync(
        &self,
        source: &ComicSourceSpec,
        state: SourceRuntimeState,
        chapter_url: &str,
        comic_ctx: &ComicJsContext,
        chapter_ctx: &ComicChapterJsContext,
    ) -> Result<SourceCallOutput<Vec<String>>, String> {
        self.fetch_pages_sync_with_control(
            source,
            state,
            chapter_url,
            comic_ctx,
            chapter_ctx,
            self.js_timeout,
            Arc::new(AtomicBool::new(false)),
        )
        .map_err(Into::into)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn fetch_pages_sync_with_control(
        &self,
        source: &ComicSourceSpec,
        state: SourceRuntimeState,
        chapter_url: &str,
        comic_ctx: &ComicJsContext,
        chapter_ctx: &ComicChapterJsContext,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<Vec<String>>, SourceCallError> {
        let mut session = self
            .build_lazy_session(source, state, op_timeout, cancel_flag)
            .map_err(|e| SourceCallError::new(e, ExecutionDiagnostics::default()))?;
        match session.fetch_pages(chapter_url, comic_ctx, chapter_ctx) {
            Ok(pages) => {
                let (final_state, diagnostics) = session.finalize_state();
                Ok(SourceCallOutput {
                    data: pages,
                    state: final_state,
                    diagnostics,
                })
            }
            Err(err) => Err(SourceCallError::new(err, session.snapshot_diagnostics())),
        }
    }

    pub fn run_login_action_sync(
        &self,
        source: &ComicSourceSpec,
        mut state: SourceRuntimeState,
        login_info: HashMap<String, String>,
        action_js: Option<&str>,
    ) -> Result<SourceCallOutput<String>, String> {
        for (k, v) in login_info {
            state.login_info.insert(k, v);
        }
        let merged_login = state.login_info.clone();
        let mut session = self.build_lazy_session(
            source,
            state,
            self.js_timeout,
            Arc::new(AtomicBool::new(false)),
        )?;
        let raw_login = source.login_url.as_deref().unwrap_or("").trim();
        let is_plain_http_login_url =
            raw_login.starts_with("http://") || raw_login.starts_with("https://");
        let login_url_js = if let Some(stripped) = raw_login.strip_prefix("@js:") {
            stripped
        } else if let Some(inner) = raw_login
            .strip_prefix("<js>")
            .and_then(|s| s.strip_suffix("</js>"))
        {
            inner
        } else if is_plain_http_login_url {
            ""
        } else {
            raw_login
        };
        let login_check_stripped = source
            .login_check_js
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| {
                if let Some(stripped) = s.strip_prefix("@js:") {
                    stripped.trim()
                } else if let Some(inner) = s
                    .strip_prefix("<js>")
                    .and_then(|rest| rest.strip_suffix("</js>"))
                {
                    inner.trim()
                } else {
                    s
                }
            });
        let effective_action = match action_js.map(str::trim).filter(|s| !s.is_empty()) {
            Some(act) => Some(act),
            None => login_check_stripped,
        };
        let out = session
            .inner
            .eval_login_script(login_url_js, effective_action, &merged_login)?;
        let (final_state, diagnostics) = session.finalize_state();
        Ok(SourceCallOutput {
            data: out,
            state: final_state,
            diagnostics,
        })
    }
}

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use xmedia_source::http::{
    resolve_relative_url, resolve_relative_url_preserving_options, split_legado_url_and_raw_options,
};
use xmedia_source::model::{SourceCallError, SourceCallOutput};
use xmedia_source::rule::{RuleValue, split_js_pipeline};

use super::ComicSourceEngine;
use super::url_template::{
    has_non_page_mustache, parse_explore_categories_in_session, resolve_literal_http_url,
    substitute_static_explore_page,
};
use crate::domain::online::{
    ComicSearchResult, ComicSearchRule, ComicSourceSpec, ExecutionDiagnostics, ExploreCategoryItem,
    SourceRuntimeState,
};
use crate::infra::source::session::ReusableComicSession;

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

impl ComicSourceEngine {
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
            self.js_timeout(),
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
        if let Some((ref manifest, _)) = source.v2_plugin {
            let cats = manifest
                .explore_categories
                .iter()
                .map(|c| ExploreCategoryItem {
                    title: c.title.clone(),
                    url: c.id.clone(),
                })
                .collect();
            return Ok(SourceCallOutput {
                data: cats,
                state,
                diagnostics: ExecutionDiagnostics::default(),
            });
        }
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
            self.js_timeout(),
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
        if let Some((ref manifest, ref js_code)) = source.v2_plugin {
            return self.search_v2_sync(
                manifest,
                js_code,
                state,
                keyword,
                page,
                op_timeout,
                cancel_flag,
            );
        }
        let (rule_search, _) = source.list_rules(false)?;
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
            self.eval_comic_list(&mut session, &search_url, rule_search, None, "ruleSearch")
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
        if let Some((ref manifest, ref js_code)) = source.v2_plugin {
            return self.explore_v2_sync(
                manifest,
                js_code,
                state,
                explore_url,
                page,
                op_timeout,
                cancel_flag,
            );
        }
        if !source.enabled_explore {
            return Err(SourceCallError::new(
                format!("漫画源「{}」未启用发现分类", source.source_name),
                ExecutionDiagnostics::default(),
            ));
        }
        let (rule_explore, rule_search_fallback) = source.list_rules(true)?;
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
                if split_legado_url_and_raw_options(tpl).1.is_some() {
                    return Err("自定义 exploreUrl 不允许附加请求选项".to_string());
                }
                let substituted = substitute_static_explore_page(tpl, page);
                resolve_literal_http_url(&substituted, &source.source_url, "exploreUrl")?
            };
            self.eval_comic_list(
                &mut session,
                &resolved_url,
                rule_explore,
                rule_search_fallback,
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
}

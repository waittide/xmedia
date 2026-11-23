use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use jiff::Timestamp;
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::user::{User, require_admin};
use xmedia_source::crud::{
    format_test_failure_summary, format_timeout_step, merge_diagnostics,
    merge_runtime_state_with_base, summarize_steps_outcome,
};

use crate::source::model::{
    ComicChapterJsContext, ComicJsContext, ComicSearchResult, ComicSourceSpec,
    ExecutionDiagnostics, ExploreCategoryItem, OnlineComicChapter, OnlineComicDetail,
};

use super::dto::{
    ComicSourceTestInput, ComicSourceTestMode, ComicSourceTestPayload, ComicSourceTestReportDto,
    OnlineComicPageDto, SourceDebugStepDto,
};
use super::helpers::{CancelOnDrop, build_image_proxy_url};
use super::{ComicOnlineService, SOURCE_TEST_TOTAL_TIMEOUT};

impl ComicOnlineService {
    pub async fn test_source(
        &self,
        actor: &User,
        id: Uuid,
        input: ComicSourceTestInput,
    ) -> DomainResult<ComicSourceTestReportDto> {
        require_admin(actor)?;
        let rec = self.load_source(id).await?;
        let parsed: ComicSourceSpec = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("漫画源 JSON 无效: {e}")))?;

        let raw_input = input
            .keyword
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);
        let target_url = input
            .target_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);

        let default_kw = parsed
            .rule_search
            .as_ref()
            .and_then(|r| r.check_key_word.as_deref())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("海贼王")
            .to_string();

        let is_direct_url = |s: &str| {
            s.starts_with("http://") || s.starts_with("https://") || s.starts_with("data:")
        };

        let explicit_explore_url = input
            .explore_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string)
            .or_else(|| {
                (input.mode == ComicSourceTestMode::Explore)
                    .then(|| target_url.clone())
                    .flatten()
            });
        let explicit_comic_url = input
            .comic_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string)
            .or_else(|| {
                matches!(
                    input.mode,
                    ComicSourceTestMode::Full | ComicSourceTestMode::Detail
                )
                .then(|| target_url.clone())
                .flatten()
            });
        let explicit_toc_url = input
            .toc_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string)
            .or_else(|| {
                (input.mode == ComicSourceTestMode::Chapters)
                    .then(|| target_url.clone())
                    .flatten()
            });
        let explicit_chapter_url = input
            .chapter_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string)
            .or_else(|| {
                (input.mode == ComicSourceTestMode::Pages)
                    .then(|| target_url.clone())
                    .flatten()
            });

        match input.mode {
            ComicSourceTestMode::Detail => {
                if explicit_comic_url.is_none() && raw_input.is_none() {
                    return Err(DomainError::Validation("请输入漫画详情页 URL".into()));
                }
            }
            ComicSourceTestMode::Chapters => {
                if explicit_toc_url.is_none() && explicit_comic_url.is_none() && raw_input.is_none()
                {
                    return Err(DomainError::Validation("请输入漫画目录页 URL".into()));
                }
            }
            ComicSourceTestMode::Pages => {
                if explicit_chapter_url.is_none() && raw_input.is_none() {
                    return Err(DomainError::Validation("请输入漫画章节页 URL".into()));
                }
            }
            ComicSourceTestMode::Full
            | ComicSourceTestMode::Search
            | ComicSourceTestMode::Explore => {}
        }

        let effective_keyword = match input.mode {
            ComicSourceTestMode::Explore => explicit_explore_url
                .clone()
                .or_else(|| raw_input.clone())
                .unwrap_or_else(|| "explore".into()),
            ComicSourceTestMode::Detail => explicit_comic_url
                .clone()
                .or_else(|| raw_input.clone())
                .unwrap_or_else(|| default_kw.clone()),
            ComicSourceTestMode::Chapters => explicit_toc_url
                .clone()
                .or_else(|| explicit_comic_url.clone())
                .or_else(|| raw_input.clone())
                .unwrap_or_else(|| default_kw.clone()),
            ComicSourceTestMode::Pages => explicit_chapter_url
                .clone()
                .or_else(|| raw_input.clone())
                .unwrap_or_else(|| default_kw.clone()),
            ComicSourceTestMode::Full => explicit_comic_url
                .clone()
                .or_else(|| raw_input.clone())
                .unwrap_or_else(|| default_kw.clone()),
            ComicSourceTestMode::Search => raw_input.clone().unwrap_or_else(|| default_kw.clone()),
        };

        let mode = input.mode;
        let chapter_index_override = input.chapter_index;
        let chapter_title_override = input
            .chapter_title
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);
        let engine = self.engine.clone();
        let prev_state = rec.runtime_state.clone();
        let base_state = prev_state.clone();
        let kw_clone = effective_keyword.clone();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_worker = cancel_flag.clone();
        let mut cancel_guard = CancelOnDrop::new(cancel_flag.clone());

        let timed_res = tokio::time::timeout(
            SOURCE_TEST_TOTAL_TIMEOUT,
            tokio::task::spawn_blocking(move || {
                let total_start = Instant::now();
                let remaining_budget = || {
                    SOURCE_TEST_TOTAL_TIMEOUT
                        .saturating_sub(total_start.elapsed())
                        .max(Duration::from_millis(1))
                };
                let mut state = prev_state;
                let mut steps: Vec<SourceDebugStepDto> = Vec::new();
                let mut all_diag = ExecutionDiagnostics::default();

                let mut search_items: Vec<ComicSearchResult> = Vec::new();
                let mut explore_categories: Vec<ExploreCategoryItem> = Vec::new();
                let mut search_total_count: usize = 0;
                let mut comic_detail: Option<OnlineComicDetail> = None;
                let mut chapters: Vec<OnlineComicChapter> = Vec::new();
                let mut chapters_total_count: usize = 0;
                let mut out_chapter_index: Option<usize> = None;
                let mut out_chapter_title: Option<String> = None;
                let mut out_chapter_url: Option<String> = None;
                let mut pages: Vec<OnlineComicPageDto> = Vec::new();
                let mut page_count: usize = 0;

                let mut next_comic_url: Option<String> = explicit_comic_url.clone();
                let mut next_fallback_hint: Option<ComicJsContext> = None;
                let mut next_toc_url: Option<String> = explicit_toc_url.clone();
                let mut next_chapter_url: Option<String> = explicit_chapter_url.clone();
                let mut next_chapter_title = chapter_title_override.unwrap_or_default();
                let mut next_chapter_idx = chapter_index_override.unwrap_or(0);

                if mode == ComicSourceTestMode::Explore {
                    let t0 = Instant::now();
                    match engine.parse_explore_categories_with_control(
                        &parsed,
                        state.clone(),
                        remaining_budget(),
                        cancel_worker.clone(),
                    ) {
                        Ok(cat_out) => {
                            state = cat_out.state;
                            merge_diagnostics(&mut all_diag, &cat_out.diagnostics);
                            explore_categories = cat_out.data;
                        }
                        Err(err) => {
                            merge_diagnostics(&mut all_diag, &err.diagnostics);
                        }
                    }

                    let chosen_url = explicit_explore_url
                        .clone()
                        .or_else(|| {
                            raw_input
                                .as_deref()
                                .filter(|u| is_direct_url(u))
                                .map(ToString::to_string)
                        })
                        .or_else(|| explore_categories.first().map(|c| c.url.clone()));

                    if let Some(exp_url) = chosen_url {
                        match engine.explore_sync_with_categories_and_control(
                            &parsed,
                            state.clone(),
                            &exp_url,
                            1,
                            Some(&explore_categories),
                            remaining_budget(),
                            cancel_worker.clone(),
                        ) {
                            Ok(out) => {
                                let elapsed_ms = t0.elapsed().as_millis() as u64;
                                state = out.state;
                                merge_diagnostics(&mut all_diag, &out.diagnostics);
                                search_total_count = out.data.len();
                                let ok = search_total_count > 0;
                                let summary = if ok {
                                    format!("发现分类命中 {search_total_count} 部漫画")
                                } else {
                                    "发现分类未匹配到漫画".into()
                                };
                                let err =
                                    (!ok).then(|| "ruleExplore 未返回条目，请检查发现规则".into());
                                search_items = out.data.into_iter().take(30).collect();
                                steps.push(SourceDebugStepDto {
                                    stage: "explore".into(),
                                    label: "发现规则 (ruleExplore)".into(),
                                    ok,
                                    elapsed_ms,
                                    summary,
                                    error: err,
                                    diagnostics: out.diagnostics,
                                });
                            }
                            Err(err) => {
                                let elapsed_ms = t0.elapsed().as_millis() as u64;
                                merge_diagnostics(&mut all_diag, &err.diagnostics);
                                steps.push(SourceDebugStepDto {
                                    stage: "explore".into(),
                                    label: "发现规则 (ruleExplore)".into(),
                                    ok: false,
                                    elapsed_ms,
                                    summary: "发现请求或解析失败".into(),
                                    error: Some(err.message),
                                    diagnostics: err.diagnostics,
                                });
                            }
                        }
                    } else {
                        steps.push(SourceDebugStepDto {
                            stage: "explore".into(),
                            label: "发现规则 (ruleExplore)".into(),
                            ok: false,
                            elapsed_ms: t0.elapsed().as_millis() as u64,
                            summary: "未配置发现分类 URL".into(),
                            error: Some("漫画源未配置 exploreUrl 或未传入目标发现 URL".into()),
                            diagnostics: ExecutionDiagnostics::default(),
                        });
                    }
                }

                let skip_search_for_url = mode == ComicSourceTestMode::Full
                    && (next_comic_url.is_some()
                        || raw_input.as_deref().is_some_and(is_direct_url));
                if skip_search_for_url && next_comic_url.is_none() {
                    next_comic_url = raw_input.clone();
                }

                // Stage 1: ruleSearch
                if mode == ComicSourceTestMode::Search
                    || (mode == ComicSourceTestMode::Full && !skip_search_for_url)
                {
                    let t0 = Instant::now();
                    match engine.search_sync_with_control(
                        &parsed,
                        state.clone(),
                        &kw_clone,
                        1,
                        remaining_budget(),
                        cancel_worker.clone(),
                    ) {
                        Ok(out) => {
                            let elapsed_ms = t0.elapsed().as_millis() as u64;
                            state = out.state;
                            merge_diagnostics(&mut all_diag, &out.diagnostics);
                            search_total_count = out.data.len();
                            let ok = search_total_count > 0;
                            let summary = if ok {
                                format!("命中 {search_total_count} 部（关键词：{kw_clone}）")
                            } else {
                                format!("关键词「{kw_clone}」未匹配到漫画")
                            };
                            let err = (!ok).then(|| {
                                "搜索未返回条目，请检查关键词、是否需要登录或漫画源规则".into()
                            });
                            search_items = out.data.into_iter().take(30).collect();
                            if let Some(first) = search_items.first() {
                                next_comic_url = Some(first.comic_url.clone());
                                next_fallback_hint = Some(ComicJsContext {
                                    name: first.title.clone(),
                                    author: first.author.clone(),
                                    book_url: first.comic_url.clone(),
                                    toc_url: String::new(),
                                    cover_url: first.cover_url.clone(),
                                    intro: first.intro.clone(),
                                    reading_direction: parsed.reading_direction.clone(),
                                    dur_chapter_index: 0,
                                    dur_chapter_title: String::new(),
                                    custom_variable: String::new(),
                                });
                            }
                            steps.push(SourceDebugStepDto {
                                stage: "search".into(),
                                label: "搜索规则 (ruleSearch)".into(),
                                ok,
                                elapsed_ms,
                                summary,
                                error: err,
                                diagnostics: out.diagnostics,
                            });
                        }
                        Err(err) => {
                            let elapsed_ms = t0.elapsed().as_millis() as u64;
                            merge_diagnostics(&mut all_diag, &err.diagnostics);
                            steps.push(SourceDebugStepDto {
                                stage: "search".into(),
                                label: "搜索规则 (ruleSearch)".into(),
                                ok: false,
                                elapsed_ms,
                                summary: "搜索请求或解析失败".into(),
                                error: Some(err.message),
                                diagnostics: err.diagnostics,
                            });
                        }
                    }
                }

                // Stage 2: ruleDetail / ruleBookInfo
                let run_detail = match mode {
                    ComicSourceTestMode::Detail => {
                        if next_comic_url.is_none() {
                            next_comic_url = raw_input.clone();
                        }
                        next_comic_url.is_some()
                    }
                    ComicSourceTestMode::Full => next_comic_url.is_some(),
                    _ => false,
                };

                if run_detail && let Some(ref c_url) = next_comic_url {
                    let t0 = Instant::now();
                    match engine.fetch_detail_sync_with_control(
                        &parsed,
                        state.clone(),
                        c_url,
                        next_fallback_hint.as_ref(),
                        remaining_budget(),
                        cancel_worker.clone(),
                    ) {
                        Ok(out) => {
                            let elapsed_ms = t0.elapsed().as_millis() as u64;
                            state = out.state;
                            merge_diagnostics(&mut all_diag, &out.diagnostics);
                            let detail = out.data;
                            let ok = !detail.title.is_empty()
                                || (mode == ComicSourceTestMode::Full
                                    && parsed.rule_detail.is_none());
                            let summary = if ok {
                                if detail.title.is_empty() {
                                    "未配置 ruleDetail，直接进入目录解析".into()
                                } else {
                                    let author_part = if detail.author.is_empty() {
                                        String::new()
                                    } else {
                                        format!(" / {}", detail.author)
                                    };
                                    format!("《{}》{}", detail.title, author_part)
                                }
                            } else if parsed.rule_detail.is_none() {
                                "漫画源未配置 ruleDetail".into()
                            } else {
                                "详情页未解析出漫画标题".into()
                            };
                            let err = (!ok).then(|| {
                                if parsed.rule_detail.is_none() {
                                    "漫画源未配置 ruleDetail / ruleBookInfo".into()
                                } else {
                                    "ruleDetail 未能提取有效漫画标题".into()
                                }
                            });
                            if next_toc_url.is_none() {
                                next_toc_url = Some(if detail.toc_url.is_empty() {
                                    c_url.clone()
                                } else {
                                    detail.toc_url.clone()
                                });
                            }
                            comic_detail = Some(detail);
                            steps.push(SourceDebugStepDto {
                                stage: "detail".into(),
                                label: "详情规则 (ruleDetail)".into(),
                                ok,
                                elapsed_ms,
                                summary,
                                error: err,
                                diagnostics: out.diagnostics,
                            });
                        }
                        Err(err) => {
                            let elapsed_ms = t0.elapsed().as_millis() as u64;
                            merge_diagnostics(&mut all_diag, &err.diagnostics);
                            steps.push(SourceDebugStepDto {
                                stage: "detail".into(),
                                label: "详情规则 (ruleDetail)".into(),
                                ok: false,
                                elapsed_ms,
                                summary: "详情请求或解析失败".into(),
                                error: Some(err.message),
                                diagnostics: err.diagnostics,
                            });
                        }
                    }
                }

                let comic_ctx = if let Some(ref d) = comic_detail {
                    ComicJsContext {
                        name: d.title.clone(),
                        author: d.author.clone(),
                        book_url: d.comic_url.clone(),
                        toc_url: d.toc_url.clone(),
                        cover_url: d.cover_url.clone(),
                        intro: d.intro.clone(),
                        reading_direction: d.reading_direction.map(|dir| dir.as_str().to_string()),
                        dur_chapter_index: next_chapter_idx,
                        dur_chapter_title: next_chapter_title.clone(),
                        custom_variable: String::new(),
                    }
                } else if let Some(ref hint) = next_fallback_hint {
                    hint.clone()
                } else {
                    ComicJsContext {
                        book_url: next_comic_url.clone().unwrap_or_default(),
                        toc_url: next_toc_url.clone().unwrap_or_default(),
                        reading_direction: parsed.reading_direction.clone(),
                        ..ComicJsContext::default()
                    }
                };

                // Stage 3: ruleToc
                let run_chapters = match mode {
                    ComicSourceTestMode::Chapters => {
                        if next_toc_url.is_none() {
                            next_toc_url = next_comic_url.clone().or_else(|| raw_input.clone());
                        }
                        next_toc_url.is_some()
                    }
                    ComicSourceTestMode::Full => {
                        next_toc_url.is_some()
                            && steps.last().is_none_or(|s| s.stage != "detail" || s.ok)
                    }
                    _ => false,
                };

                let mut toc_succeeded_without_chapter_url = false;
                if run_chapters && let Some(ref t_url) = next_toc_url {
                    let t0 = Instant::now();
                    match engine.fetch_chapters_sync_with_control(
                        &parsed,
                        state.clone(),
                        t_url,
                        &comic_ctx,
                        remaining_budget(),
                        cancel_worker.clone(),
                    ) {
                        Ok(out) => {
                            let elapsed_ms = t0.elapsed().as_millis() as u64;
                            state = out.state;
                            merge_diagnostics(&mut all_diag, &out.diagnostics);
                            let full_toc = out.data;
                            chapters_total_count = full_toc.len();
                            let ok = chapters_total_count > 0;
                            let summary = if ok {
                                format!("解析到 {chapters_total_count} 话")
                            } else {
                                "目录未解析到任何章节".into()
                            };
                            let err = (!ok).then(|| "ruleToc 未匹配到章节列表".into());

                            if next_chapter_url.is_none() {
                                let chosen = chapter_index_override
                                    .and_then(|idx| full_toc.iter().find(|c| c.index == idx))
                                    .or_else(|| {
                                        full_toc
                                            .iter()
                                            .find(|c| !c.is_volume && !c.url.trim().is_empty())
                                    })
                                    .or_else(|| full_toc.iter().find(|c| !c.url.trim().is_empty()));
                                if let Some(ch) = chosen {
                                    next_chapter_url = Some(ch.url.clone());
                                    next_chapter_title = ch.title.clone();
                                    next_chapter_idx = ch.index;
                                } else if ok {
                                    toc_succeeded_without_chapter_url = true;
                                }
                            }

                            chapters = full_toc.into_iter().take(500).collect();

                            steps.push(SourceDebugStepDto {
                                stage: "chapters".into(),
                                label: "目录规则 (ruleToc)".into(),
                                ok,
                                elapsed_ms,
                                summary,
                                error: err,
                                diagnostics: out.diagnostics,
                            });
                        }
                        Err(err) => {
                            let elapsed_ms = t0.elapsed().as_millis() as u64;
                            merge_diagnostics(&mut all_diag, &err.diagnostics);
                            steps.push(SourceDebugStepDto {
                                stage: "chapters".into(),
                                label: "目录规则 (ruleToc)".into(),
                                ok: false,
                                elapsed_ms,
                                summary: "目录请求或解析失败".into(),
                                error: Some(err.message),
                                diagnostics: err.diagnostics,
                            });
                        }
                    }
                }

                // Stage 4: rulePages / ruleContent
                let run_pages = match mode {
                    ComicSourceTestMode::Pages => {
                        if next_chapter_url.is_none() {
                            next_chapter_url = raw_input.clone();
                        }
                        next_chapter_url.is_some()
                    }
                    ComicSourceTestMode::Full => {
                        next_chapter_url
                            .as_deref()
                            .is_some_and(|u| !u.trim().is_empty())
                            && steps.last().is_none_or(|s| s.stage != "chapters" || s.ok)
                    }
                    _ => false,
                };

                if run_pages && let Some(ref ch_url) = next_chapter_url {
                    let t0 = Instant::now();
                    let ch_ctx = ComicChapterJsContext {
                        index: next_chapter_idx,
                        title: next_chapter_title.clone(),
                        url: ch_url.clone(),
                    };
                    let mut pages_comic_ctx = comic_ctx.clone();
                    pages_comic_ctx.dur_chapter_index = next_chapter_idx;
                    pages_comic_ctx.dur_chapter_title = next_chapter_title.clone();

                    match engine.fetch_pages_sync_with_control(
                        &parsed,
                        state.clone(),
                        ch_url,
                        &pages_comic_ctx,
                        &ch_ctx,
                        remaining_budget(),
                        cancel_worker.clone(),
                    ) {
                        Ok(out) => {
                            let elapsed_ms = t0.elapsed().as_millis() as u64;
                            state = out.state;
                            merge_diagnostics(&mut all_diag, &out.diagnostics);
                            page_count = out.data.len();
                            let resolved_title = if !next_chapter_title.is_empty() {
                                next_chapter_title.clone()
                            } else {
                                format!("第 {} 话", next_chapter_idx + 1)
                            };
                            let ok = page_count > 0;
                            let summary = if ok {
                                format!("《{resolved_title}》解析到 {page_count} 页图片")
                            } else {
                                format!("《{resolved_title}》未提取到图片页")
                            };
                            let err = (!ok).then(|| "ruleContent 未能提取出有效的图片 URL".into());
                            out_chapter_index = Some(next_chapter_idx);
                            out_chapter_title = Some(resolved_title);
                            out_chapter_url = Some(ch_url.clone());
                            pages = out
                                .data
                                .into_iter()
                                .take(50)
                                .enumerate()
                                .map(|(idx, image_url)| OnlineComicPageDto {
                                    index: idx,
                                    proxy_url: build_image_proxy_url(id, &image_url),
                                    image_url,
                                })
                                .collect();
                            steps.push(SourceDebugStepDto {
                                stage: "pages".into(),
                                label: "图片页规则 (ruleContent)".into(),
                                ok,
                                elapsed_ms,
                                summary,
                                error: err,
                                diagnostics: out.diagnostics,
                            });
                        }
                        Err(err) => {
                            let elapsed_ms = t0.elapsed().as_millis() as u64;
                            merge_diagnostics(&mut all_diag, &err.diagnostics);
                            out_chapter_index = Some(next_chapter_idx);
                            out_chapter_title = (!next_chapter_title.is_empty())
                                .then_some(next_chapter_title.clone());
                            out_chapter_url = Some(ch_url.clone());
                            steps.push(SourceDebugStepDto {
                                stage: "pages".into(),
                                label: "图片页规则 (ruleContent)".into(),
                                ok: false,
                                elapsed_ms,
                                summary: "图片页请求或解析失败".into(),
                                error: Some(err.message),
                                diagnostics: err.diagnostics,
                            });
                        }
                    }
                } else if mode == ComicSourceTestMode::Full && toc_succeeded_without_chapter_url {
                    steps.push(SourceDebugStepDto {
                        stage: "pages".into(),
                        label: "图片页规则 (ruleContent)".into(),
                        ok: false,
                        elapsed_ms: 0,
                        summary: "目录未包含有效章节 URL".into(),
                        error: Some(
                            "目录仅返回无有效链接的卷名或空链接，无法继续解析图片页".into(),
                        ),
                        diagnostics: ExecutionDiagnostics::default(),
                    });
                }

                let total_elapsed_ms = total_start.elapsed().as_millis() as u64;
                let (overall_ok, first_error) = summarize_steps_outcome(&steps);
                let sample_titles: Vec<String> = search_items
                    .iter()
                    .take(5)
                    .map(|b| b.title.clone())
                    .collect();

                let report = ComicSourceTestReportDto {
                    ok: overall_ok,
                    mode,
                    keyword: kw_clone,
                    total_elapsed_ms,
                    result_count: search_total_count,
                    sample_titles,
                    error: first_error,
                    diagnostics: all_diag,
                    steps,
                    payload: ComicSourceTestPayload {
                        search_items,
                        explore_categories,
                        comic_detail,
                        chapters,
                        chapters_total_count,
                        chapter_index: out_chapter_index,
                        chapter_title: out_chapter_title,
                        chapter_url: out_chapter_url,
                        pages,
                        page_count,
                    },
                };
                (state, report)
            }),
        )
        .await;

        let (maybe_final_state, report) = match timed_res {
            Ok(Ok((state, report))) => {
                cancel_guard.disarm();
                (Some(state), report)
            }
            Ok(Err(e)) => {
                cancel_guard.disarm();
                return Err(DomainError::Storage(format!("测试漫画源线程失败: {e}")));
            }
            Err(_) => {
                cancel_flag.store(true, Ordering::Relaxed);
                let timeout_msg = format!(
                    "漫画源调试总耗时超过 {} 秒，已强制中止",
                    SOURCE_TEST_TOTAL_TIMEOUT.as_secs()
                );
                let report = ComicSourceTestReportDto {
                    ok: false,
                    mode,
                    keyword: effective_keyword,
                    total_elapsed_ms: SOURCE_TEST_TOTAL_TIMEOUT.as_millis() as u64,
                    result_count: 0,
                    sample_titles: Vec::new(),
                    error: Some(timeout_msg.clone()),
                    diagnostics: ExecutionDiagnostics::default(),
                    steps: vec![format_timeout_step(
                        SOURCE_TEST_TOTAL_TIMEOUT.as_millis() as u64,
                        timeout_msg,
                    )],
                    payload: ComicSourceTestPayload::default(),
                };
                (None, report)
            }
        };

        let now = Timestamp::now();
        let last_msg = if report.ok {
            match report.mode {
                ComicSourceTestMode::Full => {
                    if report.result_count > 0 {
                        format!(
                            "全链路通过（搜索 {} 部 / 目录 {} 话 / 图片 {} 页）",
                            report.result_count,
                            report.payload.chapters_total_count,
                            report.payload.page_count
                        )
                    } else {
                        format!(
                            "详情→目录({}话)→图片({}页) 通过",
                            report.payload.chapters_total_count, report.payload.page_count
                        )
                    }
                }
                _ => report
                    .steps
                    .last()
                    .map(|s| s.summary.clone())
                    .unwrap_or_else(|| "测试通过".into()),
            }
        } else {
            format_test_failure_summary(&report.steps)
        };

        let lock = self.source_state_lock_for(id);
        let _guard = lock.lock().await;
        let current = self.sources.find_by_id(id).await?.map(|r| r.runtime_state);
        let merged_state = match maybe_final_state {
            Some(final_state) => {
                merge_runtime_state_with_base(&base_state, current, final_state, false)
            }
            None => current.unwrap_or(base_state),
        };

        let _ = self
            .sources
            .update_test_result(id, merged_state, report.ok, Some(last_msg), now)
            .await?;

        Ok(report)
    }
}

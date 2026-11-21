use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use jiff::Timestamp;
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::user::{User, require_admin};
use xmedia_source::crud::{
    format_test_failure_summary, format_timeout_step, merge_diagnostics, summarize_steps_outcome,
};

use crate::source::model::{
    BookJsContext, ChapterJsContext, ExecutionDiagnostics, LegadoBookSource, OnlineBookDetail,
    OnlineChapterItem, SearchBookResult,
};

use super::dto::{SourceDebugStepDto, SourceTestInput, SourceTestMode, SourceTestReportDto};
use super::{BookOnlineService, SOURCE_TEST_TOTAL_TIMEOUT};

impl BookOnlineService {
    pub async fn test_source(
        &self,
        actor: &User,
        id: Uuid,
        input: SourceTestInput,
    ) -> DomainResult<SourceTestReportDto> {
        require_admin(actor)?;
        let rec = self.load_source(id).await?;
        let parsed: LegadoBookSource = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("书源 JSON 无效: {e}")))?;

        let raw_input = input
            .keyword
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
            .unwrap_or("我的")
            .to_string();

        let is_direct_url = |s: &str| {
            s.starts_with("http://") || s.starts_with("https://") || s.starts_with("data:")
        };

        let explicit_book_url = input
            .book_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);
        let explicit_toc_url = input
            .toc_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);
        let explicit_chapter_url = input
            .chapter_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);

        match input.mode {
            SourceTestMode::BookInfo => {
                if explicit_book_url.is_none() && raw_input.is_none() {
                    return Err(DomainError::Validation(
                        "请输入书籍详情页 URL，或在搜索结果中点击「测此书详情」".into(),
                    ));
                }
            }
            SourceTestMode::Toc => {
                if explicit_toc_url.is_none() && explicit_book_url.is_none() && raw_input.is_none()
                {
                    return Err(DomainError::Validation(
                        "请输入目录页 URL，或先运行详情测试".into(),
                    ));
                }
            }
            SourceTestMode::Content => {
                if explicit_chapter_url.is_none() && raw_input.is_none() {
                    return Err(DomainError::Validation(
                        "请输入章节正文页 URL，或在目录列表中点击「测本章正文」".into(),
                    ));
                }
            }
            SourceTestMode::Full | SourceTestMode::Search => {}
        }

        let effective_keyword = match input.mode {
            SourceTestMode::BookInfo => explicit_book_url
                .clone()
                .or_else(|| raw_input.clone())
                .unwrap_or_else(|| default_kw.clone()),
            SourceTestMode::Toc => explicit_toc_url
                .clone()
                .or_else(|| explicit_book_url.clone())
                .or_else(|| raw_input.clone())
                .unwrap_or_else(|| default_kw.clone()),
            SourceTestMode::Content => explicit_chapter_url
                .clone()
                .or_else(|| raw_input.clone())
                .unwrap_or_else(|| default_kw.clone()),
            SourceTestMode::Full => explicit_book_url
                .clone()
                .or_else(|| raw_input.clone())
                .unwrap_or_else(|| default_kw.clone()),
            SourceTestMode::Search => raw_input.clone().unwrap_or_else(|| default_kw.clone()),
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
        let kw_clone = effective_keyword.clone();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_worker = cancel_flag.clone();

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

                let mut search_items: Vec<SearchBookResult> = Vec::new();
                let mut search_total_count: usize = 0;
                let mut book_detail: Option<OnlineBookDetail> = None;
                let mut toc_chapters: Vec<OnlineChapterItem> = Vec::new();
                let mut toc_total_count: usize = 0;
                let mut content_chapter_index: Option<usize> = None;
                let mut content_chapter_title: Option<String> = None;
                let mut content_chapter_url: Option<String> = None;
                let mut content_text: Option<String> = None;
                let mut content_char_count: usize = 0;

                let mut next_book_url: Option<String> = explicit_book_url.clone();
                let mut next_fallback_hint: Option<BookJsContext> = None;
                let mut next_toc_url: Option<String> = explicit_toc_url.clone();
                let mut next_chapter_url: Option<String> = explicit_chapter_url.clone();
                let mut next_chapter_title = chapter_title_override.unwrap_or_default();
                let mut next_chapter_idx = chapter_index_override.unwrap_or(0);

                // In Full mode, pasting a direct URL (including data:) into the keyword box jumps straight to book_info
                let skip_search_for_url = mode == SourceTestMode::Full
                    && (next_book_url.is_some() || raw_input.as_deref().is_some_and(is_direct_url));
                if skip_search_for_url && next_book_url.is_none() {
                    next_book_url = raw_input.clone();
                }

                // Stage 1: ruleSearch
                if mode == SourceTestMode::Search
                    || (mode == SourceTestMode::Full && !skip_search_for_url)
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
                                format!("命中 {search_total_count} 本（关键词：{kw_clone}）")
                            } else {
                                format!("关键词「{kw_clone}」未匹配到书籍")
                            };
                            let err = if ok {
                                None
                            } else {
                                Some("搜索未返回条目，请检查关键词、是否需要登录或书源规则".into())
                            };
                            search_items = out.data.into_iter().take(30).collect();
                            if let Some(first) = search_items.first() {
                                next_book_url = Some(first.book_url.clone());
                                next_fallback_hint = Some(BookJsContext {
                                    name: first.name.clone(),
                                    author: first.author.clone(),
                                    book_url: first.book_url.clone(),
                                    toc_url: String::new(),
                                    cover_url: first.cover_url.clone(),
                                    intro: first.intro.clone(),
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

                // Stage 2: ruleBookInfo
                let run_book_info = match mode {
                    SourceTestMode::BookInfo => {
                        if next_book_url.is_none() {
                            next_book_url = raw_input.clone();
                        }
                        next_book_url.is_some()
                    }
                    SourceTestMode::Full => next_book_url.is_some(),
                    _ => false,
                };

                if run_book_info && let Some(ref b_url) = next_book_url {
                    let t0 = Instant::now();
                    match engine.fetch_book_info_sync_with_control(
                        &parsed,
                        state.clone(),
                        b_url,
                        next_fallback_hint.as_ref(),
                        remaining_budget(),
                        cancel_worker.clone(),
                    ) {
                        Ok(out) => {
                            let elapsed_ms = t0.elapsed().as_millis() as u64;
                            state = out.state;
                            merge_diagnostics(&mut all_diag, &out.diagnostics);
                            let detail = out.data;
                            let ok = !detail.name.is_empty() || !detail.toc_url.is_empty();
                            let summary = if ok {
                                let author_part = if detail.author.is_empty() {
                                    String::new()
                                } else {
                                    format!(" / {}", detail.author)
                                };
                                format!("《{}》{}", detail.name, author_part)
                            } else {
                                "详情页未解析出书名与目录地址".into()
                            };
                            let err = if ok {
                                None
                            } else {
                                Some("ruleBookInfo 未能提取有效书名或目录 URL".into())
                            };
                            if next_toc_url.is_none() {
                                next_toc_url = Some(if detail.toc_url.is_empty() {
                                    b_url.clone()
                                } else {
                                    detail.toc_url.clone()
                                });
                            }
                            book_detail = Some(detail);
                            steps.push(SourceDebugStepDto {
                                stage: "book_info".into(),
                                label: "详情规则 (ruleBookInfo)".into(),
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
                                stage: "book_info".into(),
                                label: "详情规则 (ruleBookInfo)".into(),
                                ok: false,
                                elapsed_ms,
                                summary: "详情请求或解析失败".into(),
                                error: Some(err.message),
                                diagnostics: err.diagnostics,
                            });
                        }
                    }
                }

                // Build BookJsContext for toc / content stages
                let book_ctx = if let Some(ref d) = book_detail {
                    BookJsContext {
                        name: d.name.clone(),
                        author: d.author.clone(),
                        book_url: d.book_url.clone(),
                        toc_url: d.toc_url.clone(),
                        cover_url: d.cover_url.clone(),
                        intro: d.intro.clone(),
                        dur_chapter_index: next_chapter_idx,
                        dur_chapter_title: next_chapter_title.clone(),
                        custom_variable: String::new(),
                    }
                } else if let Some(ref hint) = next_fallback_hint {
                    hint.clone()
                } else {
                    BookJsContext {
                        book_url: next_book_url.clone().unwrap_or_default(),
                        toc_url: next_toc_url.clone().unwrap_or_default(),
                        ..BookJsContext::default()
                    }
                };

                // Stage 3: ruleToc
                let run_toc = match mode {
                    SourceTestMode::Toc => {
                        if next_toc_url.is_none() {
                            next_toc_url = next_book_url.clone().or_else(|| raw_input.clone());
                        }
                        next_toc_url.is_some()
                    }
                    SourceTestMode::Full => {
                        next_toc_url.is_some()
                            && steps.last().is_none_or(|s| s.stage != "book_info" || s.ok)
                    }
                    _ => false,
                };

                let mut toc_succeeded_without_chapter_url = false;
                if run_toc && let Some(ref t_url) = next_toc_url {
                    let t0 = Instant::now();
                    match engine.fetch_toc_sync_with_control(
                        &parsed,
                        state.clone(),
                        t_url,
                        &book_ctx,
                        remaining_budget(),
                        cancel_worker.clone(),
                    ) {
                        Ok(out) => {
                            let elapsed_ms = t0.elapsed().as_millis() as u64;
                            state = out.state;
                            merge_diagnostics(&mut all_diag, &out.diagnostics);
                            let full_toc = out.data;
                            toc_total_count = full_toc.len();
                            let ok = toc_total_count > 0;
                            let summary = if ok {
                                format!("解析到 {toc_total_count} 章")
                            } else {
                                "目录未解析到任何章节".into()
                            };
                            let err = if ok {
                                None
                            } else {
                                Some("ruleToc 未匹配到章节列表".into())
                            };

                            // Look up `chapter_index_override` from FULL TOC before truncating to 500
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

                            toc_chapters = full_toc.into_iter().take(500).collect();

                            steps.push(SourceDebugStepDto {
                                stage: "toc".into(),
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
                                stage: "toc".into(),
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

                // Stage 4: ruleContent
                let run_content = match mode {
                    SourceTestMode::Content => {
                        if next_chapter_url.is_none() {
                            next_chapter_url = raw_input.clone();
                        }
                        next_chapter_url.is_some()
                    }
                    SourceTestMode::Full => {
                        next_chapter_url
                            .as_deref()
                            .is_some_and(|u| !u.trim().is_empty())
                            && steps.last().is_none_or(|s| s.stage != "toc" || s.ok)
                    }
                    _ => false,
                };

                if run_content && let Some(ref c_url) = next_chapter_url {
                    let t0 = Instant::now();
                    let ch_ctx = ChapterJsContext {
                        index: next_chapter_idx,
                        title: next_chapter_title.clone(),
                        url: c_url.clone(),
                    };
                    let mut content_book_ctx = book_ctx.clone();
                    content_book_ctx.dur_chapter_index = next_chapter_idx;
                    content_book_ctx.dur_chapter_title = next_chapter_title.clone();

                    match engine.fetch_chapter_content_sync_with_control(
                        &parsed,
                        state.clone(),
                        c_url,
                        &content_book_ctx,
                        &ch_ctx,
                        remaining_budget(),
                        cancel_worker.clone(),
                    ) {
                        Ok(out) => {
                            let elapsed_ms = t0.elapsed().as_millis() as u64;
                            state = out.state;
                            merge_diagnostics(&mut all_diag, &out.diagnostics);
                            let char_count = out.data.content.chars().count();
                            let resolved_title = if !out.data.title.trim().is_empty() {
                                out.data.title.clone()
                            } else if !next_chapter_title.is_empty() {
                                next_chapter_title.clone()
                            } else {
                                format!("第 {} 章", next_chapter_idx + 1)
                            };
                            let ok = char_count > 0;
                            let summary = if ok {
                                format!("《{resolved_title}》解析正文 {char_count} 字")
                            } else {
                                format!("《{resolved_title}》正文内容为空")
                            };
                            let err = if ok {
                                None
                            } else {
                                Some("ruleContent 提取的正文内容为空".into())
                            };
                            content_chapter_index = Some(next_chapter_idx);
                            content_chapter_title = Some(resolved_title);
                            content_chapter_url = Some(c_url.clone());
                            content_text = Some(out.data.content);
                            content_char_count = char_count;
                            steps.push(SourceDebugStepDto {
                                stage: "content".into(),
                                label: "正文规则 (ruleContent)".into(),
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
                            content_chapter_index = Some(next_chapter_idx);
                            content_chapter_title = (!next_chapter_title.is_empty())
                                .then_some(next_chapter_title.clone());
                            content_chapter_url = Some(c_url.clone());
                            steps.push(SourceDebugStepDto {
                                stage: "content".into(),
                                label: "正文规则 (ruleContent)".into(),
                                ok: false,
                                elapsed_ms,
                                summary: "正文请求或解析失败".into(),
                                error: Some(err.message),
                                diagnostics: err.diagnostics,
                            });
                        }
                    }
                } else if mode == SourceTestMode::Full && toc_succeeded_without_chapter_url {
                    steps.push(SourceDebugStepDto {
                        stage: "content".into(),
                        label: "正文规则 (ruleContent)".into(),
                        ok: false,
                        elapsed_ms: 0,
                        summary: "目录未包含有效章节 URL".into(),
                        error: Some("目录仅返回无有效链接的卷名或空链接，无法继续抓取正文".into()),
                        diagnostics: ExecutionDiagnostics::default(),
                    });
                }

                let total_elapsed_ms = total_start.elapsed().as_millis() as u64;
                let (overall_ok, first_error) = summarize_steps_outcome(&steps);
                let sample_titles: Vec<String> = search_items
                    .iter()
                    .take(5)
                    .map(|b| b.name.clone())
                    .collect();

                let report = SourceTestReportDto {
                    ok: overall_ok,
                    mode,
                    keyword: kw_clone,
                    total_elapsed_ms,
                    result_count: search_total_count,
                    sample_titles,
                    error: first_error,
                    diagnostics: all_diag,
                    steps,
                    search_items,
                    book_detail,
                    toc_chapters,
                    toc_total_count,
                    content_chapter_index,
                    content_chapter_title,
                    content_chapter_url,
                    content_text,
                    content_char_count,
                };
                (state, report)
            }),
        )
        .await;

        let (final_state, report) = match timed_res {
            Ok(Ok(pair)) => pair,
            Ok(Err(e)) => {
                return Err(DomainError::Storage(format!("测试书源线程失败: {e}")));
            }
            Err(_) => {
                cancel_flag.store(true, Ordering::Relaxed);
                let timeout_msg = format!(
                    "书源调试总耗时超过 {} 秒，已强制中止",
                    SOURCE_TEST_TOTAL_TIMEOUT.as_secs()
                );
                let report = SourceTestReportDto {
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
                    search_items: Vec::new(),
                    book_detail: None,
                    toc_chapters: Vec::new(),
                    toc_total_count: 0,
                    content_chapter_index: None,
                    content_chapter_title: None,
                    content_chapter_url: None,
                    content_text: None,
                    content_char_count: 0,
                };
                (rec.runtime_state, report)
            }
        };

        let now = Timestamp::now();
        let last_msg = if report.ok {
            match report.mode {
                SourceTestMode::Full => {
                    if report.result_count > 0 {
                        format!(
                            "全链路通过（搜索 {} 本 / 目录 {} 章 / 正文 {} 字）",
                            report.result_count, report.toc_total_count, report.content_char_count
                        )
                    } else {
                        format!(
                            "详情→目录({}章)→正文({}字) 通过",
                            report.toc_total_count, report.content_char_count
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

        let _ = self
            .sources
            .update_test_result(id, final_state, report.ok, Some(last_msg), now)
            .await?;

        Ok(report)
    }
}

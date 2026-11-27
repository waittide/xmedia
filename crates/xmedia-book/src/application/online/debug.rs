use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::user::User;
use xmedia_source::crud::{
    format_test_failure_summary, format_timeout_step, merge_diagnostics, summarize_steps_outcome,
};

use crate::application::ports::BookSourceEnginePort;
use crate::domain::online::{
    BookJsContext, ChapterJsContext, ExecutionDiagnostics, LegadoBookSource, OnlineBookDetail,
    OnlineChapterItem, SearchBookResult, SourceRuntimeState,
};

use super::dto::{SourceDebugStepDto, SourceTestInput, SourceTestMode, SourceTestReportDto};
use super::helpers::CancelOnDrop;
use super::{BookOnlineService, SOURCE_TEST_TOTAL_TIMEOUT};

#[derive(Default)]
struct DebugRunAcc {
    state: SourceRuntimeState,
    steps: Vec<SourceDebugStepDto>,
    all_diag: ExecutionDiagnostics,
    search_items: Vec<SearchBookResult>,
    search_total_count: usize,
    book_detail: Option<OnlineBookDetail>,
    toc_chapters: Vec<OnlineChapterItem>,
    toc_total_count: usize,
    content_chapter_index: Option<usize>,
    content_chapter_title: Option<String>,
    content_chapter_url: Option<String>,
    content_text: Option<String>,
    content_char_count: usize,
    next_book_url: Option<String>,
    next_fallback_hint: Option<BookJsContext>,
    next_toc_url: Option<String>,
    next_chapter_url: Option<String>,
    next_chapter_title: String,
    next_chapter_idx: usize,
    chapter_index_override: Option<usize>,
    toc_succeeded_without_chapter_url: bool,
}

impl DebugRunAcc {
    #[allow(clippy::too_many_arguments)]
    fn execute(
        mode: SourceTestMode,
        raw_input: Option<String>,
        kw_clone: String,
        explicit_book_url: Option<String>,
        explicit_toc_url: Option<String>,
        explicit_chapter_url: Option<String>,
        chapter_title_override: Option<String>,
        chapter_index_override: Option<usize>,
        prev_state: SourceRuntimeState,
        engine: &dyn BookSourceEnginePort,
        parsed: &LegadoBookSource,
        cancel_worker: Arc<AtomicBool>,
    ) -> (SourceRuntimeState, SourceTestReportDto) {
        let is_direct_url = |s: &str| {
            s.starts_with("http://") || s.starts_with("https://") || s.starts_with("data:")
        };
        let total_start = Instant::now();
        let remaining_budget = || {
            SOURCE_TEST_TOTAL_TIMEOUT
                .saturating_sub(total_start.elapsed())
                .max(Duration::from_millis(1))
        };
        let mut acc = Self {
            state: prev_state,
            next_book_url: explicit_book_url,
            next_toc_url: explicit_toc_url,
            next_chapter_url: explicit_chapter_url,
            next_chapter_title: chapter_title_override.unwrap_or_default(),
            next_chapter_idx: chapter_index_override.unwrap_or(0),
            chapter_index_override,
            ..Default::default()
        };

        let skip_search_for_url = mode == SourceTestMode::Full
            && (acc.next_book_url.is_some() || raw_input.as_deref().is_some_and(is_direct_url));
        if skip_search_for_url && acc.next_book_url.is_none() {
            acc.next_book_url = raw_input.clone();
        }

        if mode == SourceTestMode::Search || (mode == SourceTestMode::Full && !skip_search_for_url)
        {
            acc.run_search_stage(
                engine,
                parsed,
                &kw_clone,
                remaining_budget(),
                cancel_worker.clone(),
            );
        }

        let run_book_info = match mode {
            SourceTestMode::BookInfo => {
                if acc.next_book_url.is_none() {
                    acc.next_book_url = raw_input.clone();
                }
                acc.next_book_url.is_some()
            }
            SourceTestMode::Full => acc.next_book_url.is_some(),
            _ => false,
        };
        if run_book_info && let Some(b_url) = acc.next_book_url.clone() {
            acc.run_book_info_stage(
                engine,
                parsed,
                &b_url,
                remaining_budget(),
                cancel_worker.clone(),
            );
        }

        let book_ctx = acc.build_book_ctx();
        let run_toc = match mode {
            SourceTestMode::Toc => {
                if acc.next_toc_url.is_none() {
                    acc.next_toc_url = acc.next_book_url.clone().or_else(|| raw_input.clone());
                }
                acc.next_toc_url.is_some()
            }
            SourceTestMode::Full => {
                acc.next_toc_url.is_some()
                    && acc
                        .steps
                        .last()
                        .is_none_or(|s| s.stage != "book_info" || s.ok)
            }
            _ => false,
        };
        if run_toc && let Some(t_url) = acc.next_toc_url.clone() {
            acc.run_toc_stage(
                engine,
                parsed,
                &t_url,
                &book_ctx,
                remaining_budget(),
                cancel_worker.clone(),
            );
        }

        let run_content = match mode {
            SourceTestMode::Content => {
                if acc.next_chapter_url.is_none() {
                    acc.next_chapter_url = raw_input.clone();
                }
                acc.next_chapter_url.is_some()
            }
            SourceTestMode::Full => {
                acc.next_chapter_url
                    .as_deref()
                    .is_some_and(|u| !u.trim().is_empty())
                    && acc.steps.last().is_none_or(|s| s.stage != "toc" || s.ok)
            }
            _ => false,
        };
        if run_content && let Some(c_url) = acc.next_chapter_url.clone() {
            acc.run_content_stage(
                engine,
                parsed,
                &c_url,
                &book_ctx,
                remaining_budget(),
                cancel_worker,
            );
        } else if mode == SourceTestMode::Full && acc.toc_succeeded_without_chapter_url {
            acc.steps.push(SourceDebugStepDto {
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
        let (overall_ok, first_error) = summarize_steps_outcome(&acc.steps);
        let sample_titles: Vec<String> = acc
            .search_items
            .iter()
            .take(5)
            .map(|b| b.name.clone())
            .collect();

        let report = SourceTestReportDto {
            ok: overall_ok,
            mode,
            keyword: kw_clone,
            total_elapsed_ms,
            result_count: acc.search_total_count,
            sample_titles,
            error: first_error,
            diagnostics: acc.all_diag,
            steps: acc.steps,
            search_items: acc.search_items,
            book_detail: acc.book_detail,
            toc_chapters: acc.toc_chapters,
            toc_total_count: acc.toc_total_count,
            content_chapter_index: acc.content_chapter_index,
            content_chapter_title: acc.content_chapter_title,
            content_chapter_url: acc.content_chapter_url,
            content_text: acc.content_text,
            content_char_count: acc.content_char_count,
        };
        (acc.state, report)
    }

    fn run_search_stage(
        &mut self,
        engine: &dyn BookSourceEnginePort,
        parsed: &LegadoBookSource,
        kw: &str,
        budget: Duration,
        cancel: Arc<AtomicBool>,
    ) {
        let t0 = Instant::now();
        match engine.search_sync_with_control(parsed, self.state.clone(), kw, 1, budget, cancel) {
            Ok(out) => {
                let elapsed_ms = t0.elapsed().as_millis() as u64;
                self.state = out.state;
                merge_diagnostics(&mut self.all_diag, &out.diagnostics);
                self.search_total_count = out.data.len();
                let ok = self.search_total_count > 0;
                let summary = if ok {
                    format!("命中 {} 本（关键词：{kw}）", self.search_total_count)
                } else {
                    format!("关键词「{kw}」未匹配到书籍")
                };
                let err = (!ok)
                    .then(|| "搜索未返回条目，请检查关键词、是否需要登录或书源规则".to_string());
                self.search_items = out.data.into_iter().take(30).collect();
                if let Some(first) = self.search_items.first() {
                    self.next_book_url = Some(first.book_url.clone());
                    self.next_fallback_hint = Some(BookJsContext {
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
                self.steps.push(SourceDebugStepDto {
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
                merge_diagnostics(&mut self.all_diag, &err.diagnostics);
                self.steps.push(SourceDebugStepDto {
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

    fn run_book_info_stage(
        &mut self,
        engine: &dyn BookSourceEnginePort,
        parsed: &LegadoBookSource,
        b_url: &str,
        budget: Duration,
        cancel: Arc<AtomicBool>,
    ) {
        let t0 = Instant::now();
        match engine.fetch_book_info_sync_with_control(
            parsed,
            self.state.clone(),
            b_url,
            self.next_fallback_hint.as_ref(),
            budget,
            cancel,
        ) {
            Ok(out) => {
                let elapsed_ms = t0.elapsed().as_millis() as u64;
                self.state = out.state;
                merge_diagnostics(&mut self.all_diag, &out.diagnostics);
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
                let err = (!ok).then(|| "ruleBookInfo 未能提取有效书名或目录 URL".to_string());
                if self.next_toc_url.is_none() {
                    self.next_toc_url = Some(if detail.toc_url.is_empty() {
                        b_url.to_string()
                    } else {
                        detail.toc_url.clone()
                    });
                }
                self.book_detail = Some(detail);
                self.steps.push(SourceDebugStepDto {
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
                merge_diagnostics(&mut self.all_diag, &err.diagnostics);
                self.steps.push(SourceDebugStepDto {
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

    fn build_book_ctx(&self) -> BookJsContext {
        if let Some(ref d) = self.book_detail {
            BookJsContext {
                name: d.name.clone(),
                author: d.author.clone(),
                book_url: d.book_url.clone(),
                toc_url: d.toc_url.clone(),
                cover_url: d.cover_url.clone(),
                intro: d.intro.clone(),
                dur_chapter_index: self.next_chapter_idx,
                dur_chapter_title: self.next_chapter_title.clone(),
                custom_variable: String::new(),
            }
        } else if let Some(ref hint) = self.next_fallback_hint {
            hint.clone()
        } else {
            BookJsContext {
                book_url: self.next_book_url.clone().unwrap_or_default(),
                toc_url: self.next_toc_url.clone().unwrap_or_default(),
                ..BookJsContext::default()
            }
        }
    }

    fn run_toc_stage(
        &mut self,
        engine: &dyn BookSourceEnginePort,
        parsed: &LegadoBookSource,
        t_url: &str,
        book_ctx: &BookJsContext,
        budget: Duration,
        cancel: Arc<AtomicBool>,
    ) {
        let t0 = Instant::now();
        match engine.fetch_toc_sync_with_control(
            parsed,
            self.state.clone(),
            t_url,
            book_ctx,
            budget,
            cancel,
        ) {
            Ok(out) => {
                let elapsed_ms = t0.elapsed().as_millis() as u64;
                self.state = out.state;
                merge_diagnostics(&mut self.all_diag, &out.diagnostics);
                let full_toc = out.data;
                self.toc_total_count = full_toc.len();
                let ok = self.toc_total_count > 0;
                let summary = if ok {
                    format!("解析到 {} 章", self.toc_total_count)
                } else {
                    "目录未解析到任何章节".into()
                };
                let err = (!ok).then(|| "ruleToc 未匹配到章节列表".to_string());

                if self.next_chapter_url.is_none() {
                    let chosen = self
                        .chapter_index_override
                        .and_then(|idx| full_toc.iter().find(|c| c.index == idx))
                        .or_else(|| {
                            full_toc
                                .iter()
                                .find(|c| !c.is_volume && !c.url.trim().is_empty())
                        })
                        .or_else(|| full_toc.iter().find(|c| !c.url.trim().is_empty()));
                    if let Some(ch) = chosen {
                        self.next_chapter_url = Some(ch.url.clone());
                        self.next_chapter_title = ch.title.clone();
                        self.next_chapter_idx = ch.index;
                    } else if ok {
                        self.toc_succeeded_without_chapter_url = true;
                    }
                }

                self.toc_chapters = full_toc.into_iter().take(500).collect();
                self.steps.push(SourceDebugStepDto {
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
                merge_diagnostics(&mut self.all_diag, &err.diagnostics);
                self.steps.push(SourceDebugStepDto {
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

    fn run_content_stage(
        &mut self,
        engine: &dyn BookSourceEnginePort,
        parsed: &LegadoBookSource,
        c_url: &str,
        book_ctx: &BookJsContext,
        budget: Duration,
        cancel: Arc<AtomicBool>,
    ) {
        let t0 = Instant::now();
        let ch_ctx = ChapterJsContext {
            index: self.next_chapter_idx,
            title: self.next_chapter_title.clone(),
            url: c_url.to_string(),
        };
        let mut content_book_ctx = book_ctx.clone();
        content_book_ctx.dur_chapter_index = self.next_chapter_idx;
        content_book_ctx.dur_chapter_title = self.next_chapter_title.clone();

        match engine.fetch_chapter_content_sync_with_control(
            parsed,
            self.state.clone(),
            c_url,
            &content_book_ctx,
            &ch_ctx,
            budget,
            cancel,
        ) {
            Ok(out) => {
                let elapsed_ms = t0.elapsed().as_millis() as u64;
                self.state = out.state;
                merge_diagnostics(&mut self.all_diag, &out.diagnostics);
                let char_count = out.data.content.chars().count();
                let resolved_title = if !out.data.title.trim().is_empty() {
                    out.data.title.clone()
                } else if !self.next_chapter_title.is_empty() {
                    self.next_chapter_title.clone()
                } else {
                    format!("第 {} 章", self.next_chapter_idx + 1)
                };
                let ok = char_count > 0;
                let summary = if ok {
                    format!("《{resolved_title}》解析正文 {char_count} 字")
                } else {
                    format!("《{resolved_title}》正文内容为空")
                };
                let err = (!ok).then(|| "ruleContent 提取的正文内容为空".to_string());
                self.content_chapter_index = Some(self.next_chapter_idx);
                self.content_chapter_title = Some(resolved_title);
                self.content_chapter_url = Some(c_url.to_string());
                self.content_text = Some(out.data.content);
                self.content_char_count = char_count;
                self.steps.push(SourceDebugStepDto {
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
                merge_diagnostics(&mut self.all_diag, &err.diagnostics);
                self.content_chapter_index = Some(self.next_chapter_idx);
                self.content_chapter_title =
                    (!self.next_chapter_title.is_empty()).then(|| self.next_chapter_title.clone());
                self.content_chapter_url = Some(c_url.to_string());
                self.steps.push(SourceDebugStepDto {
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
    }
}

fn non_empty_trimmed(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
}

impl BookOnlineService {
    pub async fn test_source(
        &self,
        actor: &User,
        id: Uuid,
        input: SourceTestInput,
    ) -> DomainResult<SourceTestReportDto> {
        let rec = self.sources.load_owned_source(actor, id).await?;
        let parsed: LegadoBookSource = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("书源 JSON 无效: {e}")))?;

        let raw_input = non_empty_trimmed(input.keyword.as_deref());
        let default_kw = parsed.default_test_keyword();

        let explicit_book_url = non_empty_trimmed(input.book_url.as_deref());
        let explicit_toc_url = non_empty_trimmed(input.toc_url.as_deref());
        let explicit_chapter_url = non_empty_trimmed(input.chapter_url.as_deref());

        match input.mode {
            SourceTestMode::BookInfo if explicit_book_url.is_none() && raw_input.is_none() => {
                return Err(DomainError::Validation(
                    "请输入书籍详情页 URL，或在搜索结果中点击「测此书详情」".into(),
                ));
            }
            SourceTestMode::Toc
                if explicit_toc_url.is_none()
                    && explicit_book_url.is_none()
                    && raw_input.is_none() =>
            {
                return Err(DomainError::Validation(
                    "请输入目录页 URL，或先运行详情测试".into(),
                ));
            }
            SourceTestMode::Content if explicit_chapter_url.is_none() && raw_input.is_none() => {
                return Err(DomainError::Validation(
                    "请输入章节正文页 URL，或在目录列表中点击「测本章正文」".into(),
                ));
            }
            _ => {}
        }

        let effective_keyword = match input.mode {
            SourceTestMode::BookInfo | SourceTestMode::Full => explicit_book_url
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
            SourceTestMode::Search => raw_input.clone().unwrap_or_else(|| default_kw.clone()),
        };

        let mode = input.mode;
        let chapter_index_override = input.chapter_index;
        let chapter_title_override = non_empty_trimmed(input.chapter_title.as_deref());
        let engine = Arc::clone(&self.engine);
        let prev_state = rec.runtime_state.clone();
        let base_state = prev_state.clone();
        let kw_clone = effective_keyword.clone();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_worker = cancel_flag.clone();
        let mut cancel_guard = CancelOnDrop::new(cancel_flag.clone());

        let timed_res = tokio::time::timeout(
            SOURCE_TEST_TOTAL_TIMEOUT,
            tokio::task::spawn_blocking(move || {
                DebugRunAcc::execute(
                    mode,
                    raw_input,
                    kw_clone,
                    explicit_book_url,
                    explicit_toc_url,
                    explicit_chapter_url,
                    chapter_title_override,
                    chapter_index_override,
                    prev_state,
                    engine.as_ref(),
                    &parsed,
                    cancel_worker,
                )
            }),
        )
        .await;

        let (maybe_final_state, report) = match timed_res {
            Ok(Ok((final_state, report))) => {
                cancel_guard.disarm();
                (Some(final_state), report)
            }
            Ok(Err(e)) => {
                cancel_guard.disarm();
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
                (None, report)
            }
        };

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

        if let Some(final_state) = maybe_final_state {
            self.sources
                .persist_runtime_state_with_base(id, &base_state, final_state, false)
                .await?;
        }
        self.sources
            .record_test_outcome(id, report.ok, last_msg)
            .await?;

        Ok(report)
    }
}

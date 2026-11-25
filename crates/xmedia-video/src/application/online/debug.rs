use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::user::{User, require_admin};
use xmedia_source::crud::{
    format_test_failure_summary, format_timeout_step, merge_diagnostics, summarize_steps_outcome,
};

use crate::application::ports::{VideoProxyUrlBuilder, VideoSourceEnginePort};
use crate::domain::online::{
    ExecutionDiagnostics, OnlineVideoCategory, OnlineVideoDetail, OnlineVideoPlayResult,
    OnlineVideoSummary, SourceRuntimeState, VideoSourceSpec,
};

use super::dto::{
    VideoSourceTestInput, VideoSourceTestMode, VideoSourceTestPayload, VideoSourceTestReportDto,
    VideoSourceTestStepDto,
};
use super::{CancelOnDrop, SOURCE_TEST_TOTAL_TIMEOUT, VideoOnlineService};

struct TestStageContext {
    state: SourceRuntimeState,
    steps: Vec<VideoSourceTestStepDto>,
    all_diag: ExecutionDiagnostics,
    categories: Vec<OnlineVideoCategory>,
    search_items: Vec<OnlineVideoSummary>,
    video_detail: Option<OnlineVideoDetail>,
    play_result: Option<OnlineVideoPlayResult>,
    next_cat_id: Option<String>,
    next_video_id: Option<String>,
    next_detail_url: Option<String>,
    next_play_url: Option<String>,
    next_group_name: Option<String>,
}

fn run_categories_stage(
    engine: &dyn VideoSourceEnginePort,
    parsed: &VideoSourceSpec,
    mode: VideoSourceTestMode,
    budget: Duration,
    cancel: Arc<AtomicBool>,
    ctx: &mut TestStageContext,
) {
    let t0 = Instant::now();
    match engine.fetch_categories_sync_with_control(parsed, ctx.state.clone(), budget, cancel) {
        Ok(out) => {
            let elapsed_ms = t0.elapsed().as_millis() as u64;
            ctx.state = out.state;
            merge_diagnostics(&mut ctx.all_diag, &out.diagnostics);
            let count = out.data.len();
            if ctx.next_cat_id.is_none() {
                ctx.next_cat_id = out.data.first().map(|c| c.id.clone());
            }
            let ok = count > 0 || mode == VideoSourceTestMode::Full;
            let summary = if count > 0 {
                format!("获取 {count} 个分类")
            } else {
                "未配置或未返回分类".into()
            };
            let error = (!ok).then(|| "分类列表为空".into());
            ctx.categories = out.data.into_iter().take(100).collect();
            ctx.steps.push(VideoSourceTestStepDto {
                stage: "categories".into(),
                label: "分类解析".into(),
                ok,
                elapsed_ms,
                summary,
                error,
                diagnostics: out.diagnostics,
            });
        }
        Err(err) => {
            let elapsed_ms = t0.elapsed().as_millis() as u64;
            merge_diagnostics(&mut ctx.all_diag, &err.diagnostics);
            ctx.steps.push(VideoSourceTestStepDto {
                stage: "categories".into(),
                label: "分类解析".into(),
                ok: false,
                elapsed_ms,
                summary: "分类请求或解析失败".into(),
                error: Some(err.message),
                diagnostics: err.diagnostics,
            });
        }
    }
}

fn run_search_stage(
    engine: &dyn VideoSourceEnginePort,
    parsed: &VideoSourceSpec,
    kw: &str,
    budget: Duration,
    cancel: Arc<AtomicBool>,
    ctx: &mut TestStageContext,
) {
    let t0 = Instant::now();
    match engine.search_sync_with_control(parsed, ctx.state.clone(), kw, 1, budget, cancel) {
        Ok(out) => {
            let elapsed_ms = t0.elapsed().as_millis() as u64;
            ctx.state = out.state;
            merge_diagnostics(&mut ctx.all_diag, &out.diagnostics);
            let count = out.data.len();
            let ok = count > 0;
            let summary = if ok {
                format!("命中 {count} 条影视（关键词：{kw}）")
            } else {
                format!("关键词「{kw}」未匹配到影视")
            };
            let error = (!ok).then(|| "搜索未返回条目，请检查关键词或源规则".into());
            ctx.search_items = out.data.into_iter().take(30).collect();
            if let Some(first) = ctx.search_items.first() {
                if ctx.next_video_id.is_none() {
                    ctx.next_video_id = Some(first.vod_id.clone());
                }
                if ctx.next_detail_url.is_none() && !first.detail_url.is_empty() {
                    ctx.next_detail_url = Some(first.detail_url.clone());
                }
            }
            ctx.steps.push(VideoSourceTestStepDto {
                stage: "search".into(),
                label: "关键词搜索".into(),
                ok,
                elapsed_ms,
                summary,
                error,
                diagnostics: out.diagnostics,
            });
        }
        Err(err) => {
            let elapsed_ms = t0.elapsed().as_millis() as u64;
            merge_diagnostics(&mut ctx.all_diag, &err.diagnostics);
            ctx.steps.push(VideoSourceTestStepDto {
                stage: "search".into(),
                label: "关键词搜索".into(),
                ok: false,
                elapsed_ms,
                summary: "搜索请求或解析失败".into(),
                error: Some(err.message),
                diagnostics: err.diagnostics,
            });
        }
    }
}

fn run_browse_stage(
    engine: &dyn VideoSourceEnginePort,
    parsed: &VideoSourceSpec,
    budget: Duration,
    cancel: Arc<AtomicBool>,
    ctx: &mut TestStageContext,
) {
    let t0 = Instant::now();
    match engine.browse_sync_with_control(
        parsed,
        ctx.state.clone(),
        ctx.next_cat_id.as_deref(),
        1,
        budget,
        cancel,
    ) {
        Ok(out) => {
            let elapsed_ms = t0.elapsed().as_millis() as u64;
            ctx.state = out.state;
            merge_diagnostics(&mut ctx.all_diag, &out.diagnostics);
            let count = out.data.items.len();
            let ok = count > 0;
            let summary = if ok {
                format!("列表返回 {count} 条影视")
            } else {
                "分类/列表浏览未返回条目".into()
            };
            let error = (!ok).then(|| "分类浏览未匹配到影视".into());
            ctx.search_items = out.data.items.into_iter().take(30).collect();
            if let Some(first) = ctx.search_items.first() {
                if ctx.next_video_id.is_none() {
                    ctx.next_video_id = Some(first.vod_id.clone());
                }
                if ctx.next_detail_url.is_none() && !first.detail_url.is_empty() {
                    ctx.next_detail_url = Some(first.detail_url.clone());
                }
            }
            ctx.steps.push(VideoSourceTestStepDto {
                stage: "browse".into(),
                label: "分类浏览".into(),
                ok,
                elapsed_ms,
                summary,
                error,
                diagnostics: out.diagnostics,
            });
        }
        Err(err) => {
            let elapsed_ms = t0.elapsed().as_millis() as u64;
            merge_diagnostics(&mut ctx.all_diag, &err.diagnostics);
            ctx.steps.push(VideoSourceTestStepDto {
                stage: "browse".into(),
                label: "分类浏览".into(),
                ok: false,
                elapsed_ms,
                summary: "分类浏览请求或解析失败".into(),
                error: Some(err.message),
                diagnostics: err.diagnostics,
            });
        }
    }
}

fn run_detail_stage(
    engine: &dyn VideoSourceEnginePort,
    parsed: &VideoSourceSpec,
    budget: Duration,
    cancel: Arc<AtomicBool>,
    ctx: &mut TestStageContext,
) {
    let t0 = Instant::now();
    match engine.fetch_detail_sync_with_control(
        parsed,
        ctx.state.clone(),
        ctx.next_video_id.as_deref(),
        ctx.next_detail_url.as_deref(),
        budget,
        cancel,
    ) {
        Ok(out) => {
            let elapsed_ms = t0.elapsed().as_millis() as u64;
            ctx.state = out.state;
            merge_diagnostics(&mut ctx.all_diag, &out.diagnostics);
            let detail = out.data;
            let ep_total: usize = detail.play_groups.iter().map(|g| g.episodes.len()).sum();
            let ok = !detail.title.is_empty() && ep_total > 0;
            let summary = if ok {
                format!(
                    "《{}》{} 条线路 / 共 {} 集",
                    detail.title,
                    detail.play_groups.len(),
                    ep_total
                )
            } else {
                format!("《{}》未解析出有效剧集或播放链接", detail.title)
            };
            let error = (!ok).then(|| "详情未提取到可播放剧集".into());
            if ctx.next_play_url.is_none()
                && let Some(g) = detail.play_groups.first()
                && let Some(ep) = g.episodes.first()
            {
                ctx.next_group_name = Some(g.name.clone());
                ctx.next_play_url = Some(ep.play_url.clone());
            }
            ctx.video_detail = Some(detail);
            ctx.steps.push(VideoSourceTestStepDto {
                stage: "detail".into(),
                label: "详情与选集解析".into(),
                ok,
                elapsed_ms,
                summary,
                error,
                diagnostics: out.diagnostics,
            });
        }
        Err(err) => {
            let elapsed_ms = t0.elapsed().as_millis() as u64;
            merge_diagnostics(&mut ctx.all_diag, &err.diagnostics);
            ctx.steps.push(VideoSourceTestStepDto {
                stage: "detail".into(),
                label: "详情与选集解析".into(),
                ok: false,
                elapsed_ms,
                summary: "详情请求或解析失败".into(),
                error: Some(err.message),
                diagnostics: err.diagnostics,
            });
        }
    }
}

fn run_play_stage(
    engine: &dyn VideoSourceEnginePort,
    parsed: &VideoSourceSpec,
    p_url: &str,
    budget: Duration,
    cancel: Arc<AtomicBool>,
    ctx: &mut TestStageContext,
) {
    let t0 = Instant::now();
    match engine.resolve_play_sync_with_control(
        parsed,
        ctx.state.clone(),
        p_url,
        ctx.next_group_name.as_deref(),
        budget,
        cancel,
    ) {
        Ok(out) => {
            let elapsed_ms = t0.elapsed().as_millis() as u64;
            ctx.state = out.state;
            merge_diagnostics(&mut ctx.all_diag, &out.diagnostics);
            let ok = !out.data.stream_url.trim().is_empty();
            let summary = if ok {
                format!(
                    "解析播放流成功 ({})：{}",
                    out.data.stream_type, out.data.stream_url
                )
            } else {
                "解析播放流结果为空".into()
            };
            let error = (!ok).then(|| "未能解析出有效视频流地址".into());
            ctx.play_result = Some(out.data);
            ctx.steps.push(VideoSourceTestStepDto {
                stage: "play".into(),
                label: "播放地址解析".into(),
                ok,
                elapsed_ms,
                summary,
                error,
                diagnostics: out.diagnostics,
            });
        }
        Err(err) => {
            let elapsed_ms = t0.elapsed().as_millis() as u64;
            merge_diagnostics(&mut ctx.all_diag, &err.diagnostics);
            ctx.steps.push(VideoSourceTestStepDto {
                stage: "play".into(),
                label: "播放地址解析".into(),
                ok: false,
                elapsed_ms,
                summary: "播放地址解析失败".into(),
                error: Some(err.message),
                diagnostics: err.diagnostics,
            });
        }
    }
}

fn populate_test_report_proxy_urls(
    id: Uuid,
    payload: &mut VideoSourceTestPayload,
    url_builder: &dyn VideoProxyUrlBuilder,
) {
    for item in &mut payload.search_items {
        if let Some(ref c) = item.cover_url
            && !c.trim().is_empty()
        {
            item.proxy_cover_url = Some(url_builder.poster_proxy_url(c, Some(id)));
        }
    }
    if let Some(ref mut detail) = payload.video_detail
        && let Some(ref c) = detail.cover_url
        && !c.trim().is_empty()
    {
        detail.proxy_cover_url = Some(url_builder.poster_proxy_url(c, Some(id)));
    }
    if let Some(ref mut play) = payload.play_result {
        play.proxy_stream_url = if play.stream_type == "hls" {
            url_builder.hls_proxy_url(&play.stream_url, id, None)
        } else {
            url_builder.stream_proxy_url(&play.stream_url, id, None)
        };
        for sub in &mut play.subtitles {
            let clean = sub.url.trim();
            if clean.starts_with("http://") || clean.starts_with("https://") {
                sub.proxy_url = Some(url_builder.subtitle_proxy_url(clean, id, None));
            }
        }
    }
}

impl VideoOnlineService {
    pub async fn test_source(
        &self,
        actor: &User,
        id: Uuid,
        input: VideoSourceTestInput,
        url_builder: &dyn VideoProxyUrlBuilder,
    ) -> DomainResult<VideoSourceTestReportDto> {
        require_admin(actor)?;
        let rec = self.load_source(id).await?;
        let parsed: VideoSourceSpec = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("影视源 JSON 无效: {e}")))?;

        let trim_non_empty = |opt: Option<&str>| {
            opt.map(str::trim)
                .filter(|s| !s.is_empty())
                .map(ToString::to_string)
        };
        let raw_input = trim_non_empty(input.keyword.as_deref());
        let default_kw = parsed
            .check_keyword
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("电影")
            .to_string();
        let is_direct_url = |s: &str| {
            s.starts_with("http://") || s.starts_with("https://") || s.starts_with("data:")
        };

        let explicit_target = trim_non_empty(input.target_url.as_deref());
        let explicit_category_id = trim_non_empty(input.category_id.as_deref());
        let explicit_video_id = trim_non_empty(input.video_id.as_deref());
        let explicit_detail_url = trim_non_empty(input.detail_url.as_deref()).or_else(|| {
            (input.mode == VideoSourceTestMode::Detail)
                .then(|| explicit_target.clone())
                .flatten()
        });
        let explicit_play_url = trim_non_empty(input.play_url.as_deref()).or_else(|| {
            (input.mode == VideoSourceTestMode::Play)
                .then(|| explicit_target.clone())
                .flatten()
        });
        let explicit_group_name = trim_non_empty(input.group_name.as_deref());

        if input.mode == VideoSourceTestMode::Detail
            && explicit_video_id.is_none()
            && explicit_detail_url.is_none()
            && raw_input.is_none()
        {
            return Err(DomainError::Validation(
                "请输入 video_id 或 detail_url 以测试详情解析".into(),
            ));
        }
        if input.mode == VideoSourceTestMode::Play
            && explicit_play_url.is_none()
            && raw_input.is_none()
        {
            return Err(DomainError::Validation(
                "请输入 play_url 以测试播放地址解析".into(),
            ));
        }

        let effective_keyword = match input.mode {
            VideoSourceTestMode::Detail => explicit_detail_url
                .clone()
                .or_else(|| explicit_video_id.clone())
                .or_else(|| raw_input.clone())
                .unwrap_or_else(|| default_kw.clone()),
            VideoSourceTestMode::Play => explicit_play_url
                .clone()
                .or_else(|| raw_input.clone())
                .unwrap_or_else(|| default_kw.clone()),
            _ => raw_input.clone().unwrap_or_else(|| default_kw.clone()),
        };

        let mode = input.mode;
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
                let mut ctx = TestStageContext {
                    state: prev_state,
                    steps: Vec::new(),
                    all_diag: ExecutionDiagnostics::default(),
                    categories: Vec::new(),
                    search_items: Vec::new(),
                    video_detail: None,
                    play_result: None,
                    next_cat_id: explicit_category_id,
                    next_video_id: explicit_video_id,
                    next_detail_url: explicit_detail_url,
                    next_play_url: explicit_play_url,
                    next_group_name: explicit_group_name,
                };

                let skip_search_for_url = mode == VideoSourceTestMode::Full
                    && (ctx.next_detail_url.is_some()
                        || raw_input.as_deref().is_some_and(is_direct_url));
                if skip_search_for_url && ctx.next_detail_url.is_none() {
                    ctx.next_detail_url = raw_input.clone();
                }

                if mode == VideoSourceTestMode::Categories
                    || (mode == VideoSourceTestMode::Full && !skip_search_for_url)
                {
                    run_categories_stage(
                        engine.as_ref(),
                        &parsed,
                        mode,
                        remaining_budget(),
                        cancel_worker.clone(),
                        &mut ctx,
                    );
                }

                let run_search = mode == VideoSourceTestMode::Search
                    || (mode == VideoSourceTestMode::Full
                        && !skip_search_for_url
                        && parsed.searchable);
                let run_browse = mode == VideoSourceTestMode::Browse
                    || (mode == VideoSourceTestMode::Full
                        && !skip_search_for_url
                        && !parsed.searchable);

                if run_search {
                    run_search_stage(
                        engine.as_ref(),
                        &parsed,
                        &kw_clone,
                        remaining_budget(),
                        cancel_worker.clone(),
                        &mut ctx,
                    );
                } else if run_browse {
                    run_browse_stage(
                        engine.as_ref(),
                        &parsed,
                        remaining_budget(),
                        cancel_worker.clone(),
                        &mut ctx,
                    );
                }

                let run_detail = match mode {
                    VideoSourceTestMode::Detail => {
                        if ctx.next_detail_url.is_none() && ctx.next_video_id.is_none() {
                            ctx.next_detail_url = raw_input.clone();
                        }
                        ctx.next_detail_url.is_some() || ctx.next_video_id.is_some()
                    }
                    VideoSourceTestMode::Full => {
                        (ctx.next_detail_url.is_some() || ctx.next_video_id.is_some())
                            && ctx
                                .steps
                                .iter()
                                .filter(|s| s.stage == "search" || s.stage == "browse")
                                .all(|s| s.ok)
                    }
                    _ => false,
                };

                if run_detail {
                    run_detail_stage(
                        engine.as_ref(),
                        &parsed,
                        remaining_budget(),
                        cancel_worker.clone(),
                        &mut ctx,
                    );
                }

                let run_play = match mode {
                    VideoSourceTestMode::Play => {
                        if ctx.next_play_url.is_none() {
                            ctx.next_play_url = raw_input.clone();
                        }
                        ctx.next_play_url.is_some()
                    }
                    VideoSourceTestMode::Full => {
                        ctx.next_play_url.is_some()
                            && ctx.steps.last().is_none_or(|s| s.stage != "detail" || s.ok)
                    }
                    _ => false,
                };

                if run_play && let Some(p_url) = ctx.next_play_url.clone() {
                    run_play_stage(
                        engine.as_ref(),
                        &parsed,
                        &p_url,
                        remaining_budget(),
                        cancel_worker,
                        &mut ctx,
                    );
                }

                let total_elapsed_ms = total_start.elapsed().as_millis() as u64;
                let (overall_ok, first_error) = summarize_steps_outcome(&ctx.steps);
                let result_count = if !ctx.search_items.is_empty() {
                    ctx.search_items.len()
                } else {
                    ctx.categories.len()
                };
                let sample_titles: Vec<String> = if !ctx.search_items.is_empty() {
                    ctx.search_items
                        .iter()
                        .take(5)
                        .map(|v| v.title.clone())
                        .collect()
                } else {
                    ctx.categories
                        .iter()
                        .take(5)
                        .map(|c| c.name.clone())
                        .collect()
                };

                let report = VideoSourceTestReportDto {
                    ok: overall_ok,
                    mode,
                    keyword: kw_clone,
                    total_elapsed_ms,
                    result_count,
                    sample_titles,
                    error: first_error,
                    diagnostics: ctx.all_diag,
                    steps: ctx.steps,
                    payload: VideoSourceTestPayload {
                        categories: ctx.categories,
                        search_items: ctx.search_items,
                        video_detail: ctx.video_detail,
                        play_result: ctx.play_result,
                    },
                };
                (ctx.state, report)
            }),
        )
        .await;

        let (maybe_final_state, mut report) = match timed_res {
            Ok(Ok((state, report))) => {
                cancel_guard.disarm();
                (Some(state), report)
            }
            Ok(Err(e)) => {
                cancel_guard.disarm();
                return Err(DomainError::Storage(format!("测试影视源线程失败: {e}")));
            }
            Err(_) => {
                cancel_flag.store(true, Ordering::Relaxed);
                let timeout_msg = format!(
                    "影视源调试总耗时超过 {} 秒，已强制中止",
                    SOURCE_TEST_TOTAL_TIMEOUT.as_secs()
                );
                let report = VideoSourceTestReportDto {
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
                    payload: VideoSourceTestPayload::default(),
                };
                (None, report)
            }
        };

        populate_test_report_proxy_urls(id, &mut report.payload, url_builder);

        let last_msg = if report.ok {
            report
                .steps
                .last()
                .map(|s| s.summary.clone())
                .unwrap_or_else(|| "测试通过".into())
        } else {
            format_test_failure_summary(&report.steps)
        };

        if let Some(final_state) = maybe_final_state {
            self.persist_runtime_state_with_base(id, &base_state, final_state, false)
                .await?;
        }
        self.source_crud
            .record_test_outcome(id, report.ok, last_msg)
            .await?;

        Ok(report)
    }
}

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

use crate::source::hls::{
    build_hls_proxy_url, build_poster_proxy_url, build_stream_proxy_url, build_subtitle_proxy_url,
};
use crate::source::model::{
    ExecutionDiagnostics, OnlineVideoCategory, OnlineVideoDetail, OnlineVideoPlayResult,
    OnlineVideoSummary, VideoSourceSpec,
};

use super::dto::{
    VideoSourceTestInput, VideoSourceTestMode, VideoSourceTestPayload, VideoSourceTestReportDto,
    VideoSourceTestStepDto,
};
use super::sources::merge_runtime_state_with_base;
use super::{SOURCE_TEST_TOTAL_TIMEOUT, VideoOnlineService};

impl VideoOnlineService {
    pub async fn test_source(
        &self,
        actor: &User,
        id: Uuid,
        input: VideoSourceTestInput,
    ) -> DomainResult<VideoSourceTestReportDto> {
        require_admin(actor)?;
        let rec = self.load_source(id).await?;
        let parsed: VideoSourceSpec = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("影视源 JSON 无效: {e}")))?;

        let raw_input = input
            .keyword
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);

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

        let explicit_target = input
            .target_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);
        let explicit_category_id = input
            .category_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);
        let explicit_video_id = input
            .video_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);
        let explicit_detail_url = input
            .detail_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string)
            .or_else(|| {
                (input.mode == VideoSourceTestMode::Detail)
                    .then(|| explicit_target.clone())
                    .flatten()
            });
        let explicit_play_url = input
            .play_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string)
            .or_else(|| {
                (input.mode == VideoSourceTestMode::Play)
                    .then(|| explicit_target.clone())
                    .flatten()
            });
        let explicit_group_name = input
            .group_name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);

        match input.mode {
            VideoSourceTestMode::Detail => {
                if explicit_video_id.is_none()
                    && explicit_detail_url.is_none()
                    && raw_input.is_none()
                {
                    return Err(DomainError::Validation(
                        "请输入 video_id 或 detail_url 以测试详情解析".into(),
                    ));
                }
            }
            VideoSourceTestMode::Play => {
                if explicit_play_url.is_none() && raw_input.is_none() {
                    return Err(DomainError::Validation(
                        "请输入 play_url 以测试播放地址解析".into(),
                    ));
                }
            }
            VideoSourceTestMode::Full
            | VideoSourceTestMode::Categories
            | VideoSourceTestMode::Search
            | VideoSourceTestMode::Browse => {}
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
                let mut steps: Vec<VideoSourceTestStepDto> = Vec::new();
                let mut all_diag = ExecutionDiagnostics::default();

                let mut categories: Vec<OnlineVideoCategory> = Vec::new();
                let mut search_items: Vec<OnlineVideoSummary> = Vec::new();
                let mut video_detail: Option<OnlineVideoDetail> = None;
                let mut play_result: Option<OnlineVideoPlayResult> = None;

                let mut next_cat_id = explicit_category_id.clone();
                let mut next_video_id = explicit_video_id.clone();
                let mut next_detail_url = explicit_detail_url.clone();
                let mut next_play_url = explicit_play_url.clone();
                let mut next_group_name = explicit_group_name.clone();

                let skip_search_for_url = mode == VideoSourceTestMode::Full
                    && (next_detail_url.is_some()
                        || raw_input.as_deref().is_some_and(is_direct_url));
                if skip_search_for_url && next_detail_url.is_none() {
                    next_detail_url = raw_input.clone();
                }

                // Stage 1: Categories (in Categories or Full mode)
                if mode == VideoSourceTestMode::Categories
                    || (mode == VideoSourceTestMode::Full && !skip_search_for_url)
                {
                    let t0 = Instant::now();
                    match engine.fetch_categories_sync_with_control(
                        &parsed,
                        state.clone(),
                        remaining_budget(),
                        cancel_worker.clone(),
                    ) {
                        Ok(out) => {
                            let elapsed_ms = t0.elapsed().as_millis() as u64;
                            state = out.state;
                            merge_diagnostics(&mut all_diag, &out.diagnostics);
                            let count = out.data.len();
                            if next_cat_id.is_none() {
                                next_cat_id = out.data.first().map(|c| c.id.clone());
                            }
                            let ok = count > 0 || mode == VideoSourceTestMode::Full;
                            let summary = if count > 0 {
                                format!("获取 {count} 个分类")
                            } else {
                                "未配置或未返回分类".into()
                            };
                            let err = if ok {
                                None
                            } else {
                                Some("分类列表为空".into())
                            };
                            categories = out.data.into_iter().take(100).collect();
                            steps.push(VideoSourceTestStepDto {
                                stage: "categories".into(),
                                label: "分类解析".into(),
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
                            steps.push(VideoSourceTestStepDto {
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

                // Stage 2: Search or Browse fallback
                let run_search = mode == VideoSourceTestMode::Search
                    || (mode == VideoSourceTestMode::Full
                        && !skip_search_for_url
                        && parsed.searchable);
                let run_browse = mode == VideoSourceTestMode::Browse
                    || (mode == VideoSourceTestMode::Full
                        && !skip_search_for_url
                        && !parsed.searchable);

                if run_search {
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
                            let count = out.data.len();
                            let ok = count > 0;
                            let summary = if ok {
                                format!("命中 {count} 条影视（关键词：{kw_clone}）")
                            } else {
                                format!("关键词「{kw_clone}」未匹配到影视")
                            };
                            let err = if ok {
                                None
                            } else {
                                Some("搜索未返回条目，请检查关键词或源规则".into())
                            };
                            search_items = out.data.into_iter().take(30).collect();
                            for item in &mut search_items {
                                if let Some(ref c) = item.cover_url
                                    && !c.trim().is_empty()
                                {
                                    item.proxy_cover_url =
                                        Some(build_poster_proxy_url(c, Some(id)));
                                }
                            }
                            if let Some(first) = search_items.first() {
                                if next_video_id.is_none() {
                                    next_video_id = Some(first.vod_id.clone());
                                }
                                if next_detail_url.is_none() && !first.detail_url.is_empty() {
                                    next_detail_url = Some(first.detail_url.clone());
                                }
                            }
                            steps.push(VideoSourceTestStepDto {
                                stage: "search".into(),
                                label: "关键词搜索".into(),
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
                            steps.push(VideoSourceTestStepDto {
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
                } else if run_browse {
                    let t0 = Instant::now();
                    match engine.browse_sync_with_control(
                        &parsed,
                        state.clone(),
                        next_cat_id.as_deref(),
                        1,
                        remaining_budget(),
                        cancel_worker.clone(),
                    ) {
                        Ok(out) => {
                            let elapsed_ms = t0.elapsed().as_millis() as u64;
                            state = out.state;
                            merge_diagnostics(&mut all_diag, &out.diagnostics);
                            let count = out.data.items.len();
                            let ok = count > 0;
                            let summary = if ok {
                                format!("列表返回 {count} 条影视")
                            } else {
                                "分类/列表浏览未返回条目".into()
                            };
                            let err = if ok {
                                None
                            } else {
                                Some("分类浏览未匹配到影视".into())
                            };
                            search_items = out.data.items.into_iter().take(30).collect();
                            for item in &mut search_items {
                                if let Some(ref c) = item.cover_url
                                    && !c.trim().is_empty()
                                {
                                    item.proxy_cover_url =
                                        Some(build_poster_proxy_url(c, Some(id)));
                                }
                            }
                            if let Some(first) = search_items.first() {
                                if next_video_id.is_none() {
                                    next_video_id = Some(first.vod_id.clone());
                                }
                                if next_detail_url.is_none() && !first.detail_url.is_empty() {
                                    next_detail_url = Some(first.detail_url.clone());
                                }
                            }
                            steps.push(VideoSourceTestStepDto {
                                stage: "browse".into(),
                                label: "分类浏览".into(),
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
                            steps.push(VideoSourceTestStepDto {
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

                // Stage 3: Detail
                let run_detail = match mode {
                    VideoSourceTestMode::Detail => {
                        if next_detail_url.is_none() && next_video_id.is_none() {
                            next_detail_url = raw_input.clone();
                        }
                        next_detail_url.is_some() || next_video_id.is_some()
                    }
                    VideoSourceTestMode::Full => {
                        (next_detail_url.is_some() || next_video_id.is_some())
                            && steps
                                .iter()
                                .filter(|s| s.stage == "search" || s.stage == "browse")
                                .all(|s| s.ok)
                    }
                    _ => false,
                };

                if run_detail {
                    let t0 = Instant::now();
                    match engine.fetch_detail_sync_with_control(
                        &parsed,
                        state.clone(),
                        next_video_id.as_deref(),
                        next_detail_url.as_deref(),
                        remaining_budget(),
                        cancel_worker.clone(),
                    ) {
                        Ok(out) => {
                            let elapsed_ms = t0.elapsed().as_millis() as u64;
                            state = out.state;
                            merge_diagnostics(&mut all_diag, &out.diagnostics);
                            let mut detail = out.data;
                            if let Some(ref c) = detail.cover_url
                                && !c.trim().is_empty()
                            {
                                detail.proxy_cover_url = Some(build_poster_proxy_url(c, Some(id)));
                            }
                            let ep_total: usize =
                                detail.play_groups.iter().map(|g| g.episodes.len()).sum();
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
                            let err = if ok {
                                None
                            } else {
                                Some("详情未提取到可播放剧集".into())
                            };
                            if next_play_url.is_none()
                                && let Some(g) = detail.play_groups.first()
                                && let Some(ep) = g.episodes.first()
                            {
                                next_group_name = Some(g.name.clone());
                                next_play_url = Some(ep.play_url.clone());
                            }
                            video_detail = Some(detail);
                            steps.push(VideoSourceTestStepDto {
                                stage: "detail".into(),
                                label: "详情与选集解析".into(),
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
                            steps.push(VideoSourceTestStepDto {
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

                // Stage 4: Play
                let run_play = match mode {
                    VideoSourceTestMode::Play => {
                        if next_play_url.is_none() {
                            next_play_url = raw_input.clone();
                        }
                        next_play_url.is_some()
                    }
                    VideoSourceTestMode::Full => {
                        next_play_url.is_some()
                            && steps.last().is_none_or(|s| s.stage != "detail" || s.ok)
                    }
                    _ => false,
                };

                if run_play && let Some(ref p_url) = next_play_url {
                    let t0 = Instant::now();
                    match engine.resolve_play_sync_with_control(
                        &parsed,
                        state.clone(),
                        p_url,
                        next_group_name.as_deref(),
                        remaining_budget(),
                        cancel_worker.clone(),
                    ) {
                        Ok(mut out) => {
                            let elapsed_ms = t0.elapsed().as_millis() as u64;
                            state = out.state;
                            merge_diagnostics(&mut all_diag, &out.diagnostics);
                            out.data.proxy_stream_url = if out.data.stream_type == "hls" {
                                build_hls_proxy_url(&out.data.stream_url, Some(id), None)
                            } else {
                                build_stream_proxy_url(&out.data.stream_url, Some(id), None)
                            };
                            for sub in &mut out.data.subtitles {
                                let clean = sub.url.trim();
                                if clean.starts_with("http://") || clean.starts_with("https://") {
                                    sub.proxy_url =
                                        Some(build_subtitle_proxy_url(clean, Some(id), None));
                                }
                            }
                            let ok = !out.data.stream_url.trim().is_empty();
                            let summary = if ok {
                                format!(
                                    "解析播放流成功 ({})：{}",
                                    out.data.stream_type, out.data.stream_url
                                )
                            } else {
                                "解析播放流结果为空".into()
                            };
                            let err = if ok {
                                None
                            } else {
                                Some("未能解析出有效视频流地址".into())
                            };
                            play_result = Some(out.data);
                            steps.push(VideoSourceTestStepDto {
                                stage: "play".into(),
                                label: "播放地址解析".into(),
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
                            steps.push(VideoSourceTestStepDto {
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

                let total_elapsed_ms = total_start.elapsed().as_millis() as u64;
                let (overall_ok, first_error) = summarize_steps_outcome(&steps);
                let result_count = if !search_items.is_empty() {
                    search_items.len()
                } else {
                    categories.len()
                };
                let sample_titles: Vec<String> = if !search_items.is_empty() {
                    search_items
                        .iter()
                        .take(5)
                        .map(|v| v.title.clone())
                        .collect()
                } else {
                    categories.iter().take(5).map(|c| c.name.clone()).collect()
                };

                let report = VideoSourceTestReportDto {
                    ok: overall_ok,
                    mode,
                    keyword: kw_clone,
                    total_elapsed_ms,
                    result_count,
                    sample_titles,
                    error: first_error,
                    diagnostics: all_diag,
                    steps,
                    payload: VideoSourceTestPayload {
                        categories,
                        search_items,
                        video_detail,
                        play_result,
                    },
                };
                (state, report)
            }),
        )
        .await;

        let (maybe_final_state, report) = match timed_res {
            Ok(Ok((state, report))) => (Some(state), report),
            Ok(Err(e)) => {
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

        let now = Timestamp::now();
        let last_msg = if report.ok {
            report
                .steps
                .last()
                .map(|s| s.summary.clone())
                .unwrap_or_else(|| "测试通过".into())
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

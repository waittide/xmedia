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

use crate::domain::catalog::StructuredLyrics;
use crate::source::engine::is_http_url;
use crate::source::model::{
    ExecutionDiagnostics, MusicSourceSpec, MusicSourceTestMode, MusicTrackContext,
    OnlineMusicTrackItem,
};

use super::browse::{build_proxy_cover_url, build_proxy_stream_url, track_item_to_dto};
use super::dto::{
    MusicExploreCategoryDto, MusicSourceTestInput, MusicSourceTestPayload,
    MusicSourceTestReportDto, OnlineMusicResolvedTrackDto, OnlineMusicStreamResponseDto,
    OnlineMusicTrackDto, SourceDebugStepDto,
};
use super::{CancelOnDrop, MusicOnlineService, SOURCE_TEST_TOTAL_TIMEOUT};

impl MusicOnlineService {
    pub async fn test_source(
        &self,
        actor: &User,
        id: Uuid,
        input: MusicSourceTestInput,
    ) -> DomainResult<MusicSourceTestReportDto> {
        require_admin(actor)?;
        let rec = self.load_source(id).await?;
        let parsed: MusicSourceSpec = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("音源 JSON 无效: {e}")))?;

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
            .unwrap_or("Mojito")
            .to_string();

        let effective_keyword = raw_input.clone().unwrap_or_else(|| default_kw.clone());
        let mode = input.mode;
        let engine = self.engine.clone();
        let headers_applied = Self::has_effective_proxy_headers(&parsed);
        let prev_state = rec.runtime_state.clone();
        let prev_state_cmp = prev_state.clone();
        let kw_clone = effective_keyword.clone();
        let source_name = rec.name.clone();
        let qualities = parsed.supported_qualities.clone();
        let has_cover_rule = parsed.rule_cover.is_some();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_worker = cancel_flag.clone();
        let mut cancel_guard = CancelOnDrop::new(cancel_flag.clone());

        let explicit_ctx = MusicTrackContext {
            track_id: input.track_id.unwrap_or_default(),
            title: input.title.unwrap_or_default(),
            artist: input.artist.unwrap_or_default(),
            album: None,
            duration_secs: None,
            cover_url: None,
            cover_id: input.cover_id,
            url_id: input.url_id,
            lyric_id: input.lyric_id,
            play_url: input.play_url,
            detail_url: input.detail_url.or(input.target_url),
            quality: input.quality.clone(),
        };
        let explicit_explore_url = input.explore_url;
        let requested_quality = input.quality;

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

                let mut search_items: Vec<OnlineMusicTrackDto> = Vec::new();
                let mut search_total_count: usize = 0;
                let mut explore_categories: Vec<MusicExploreCategoryDto> = Vec::new();
                let mut active_item: Option<OnlineMusicTrackItem> = None;
                let mut stream_probe: Option<OnlineMusicStreamResponseDto> = None;
                let mut cover_resolved: Option<String> = None;
                let mut lyrics_resolved: Option<StructuredLyrics> = None;
                let mut translated_lyrics_resolved: Option<StructuredLyrics> = None;

                // Stage 1: Search (or Explore when mode == Explore or Full without searchUrl)
                let run_explore = mode == MusicSourceTestMode::Explore
                    || (mode == MusicSourceTestMode::Full
                        && parsed
                            .search_url
                            .as_deref()
                            .is_none_or(|s| s.trim().is_empty())
                        && parsed
                            .explore_url
                            .as_deref()
                            .is_some_and(|s| !s.trim().is_empty()));
                let run_search = mode == MusicSourceTestMode::Search
                    || (mode == MusicSourceTestMode::Full && !run_explore);

                if run_search {
                    let t0 = Instant::now();
                    match engine.search_sync_with_control(
                        &parsed,
                        state.clone(),
                        &kw_clone,
                        1,
                        10,
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
                                format!("命中 {search_total_count} 首（关键词：{kw_clone}）")
                            } else {
                                format!("关键词「{kw_clone}」未匹配到曲目")
                            };
                            let err = (!ok).then(|| {
                                "搜索未返回曲目，请检查关键词或 ruleSearch 规则".to_string()
                            });
                            if let Some(first) = out.data.first() {
                                active_item = Some(first.clone());
                            }
                            search_items = out
                                .data
                                .into_iter()
                                .take(20)
                                .map(|item| {
                                    track_item_to_dto(
                                        id,
                                        &source_name,
                                        &qualities,
                                        has_cover_rule,
                                        item,
                                    )
                                })
                                .collect();
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

                if run_explore {
                    let t0 = Instant::now();
                    match engine.explore_sync_with_control(
                        &parsed,
                        state.clone(),
                        explicit_explore_url.as_deref(),
                        1,
                        10,
                        remaining_budget(),
                        cancel_worker.clone(),
                    ) {
                        Ok(out) => {
                            let elapsed_ms = t0.elapsed().as_millis() as u64;
                            state = out.state;
                            merge_diagnostics(&mut all_diag, &out.diagnostics);
                            search_total_count = out.data.tracks.len();
                            let ok = search_total_count > 0;
                            let summary = if ok {
                                format!("浏览榜单返回 {search_total_count} 首曲目")
                            } else {
                                "榜单未返回曲目".into()
                            };
                            let err = (!ok).then(|| "explore 未匹配到曲目".to_string());
                            if active_item.is_none()
                                && let Some(first) = out.data.tracks.first()
                            {
                                active_item = Some(first.clone());
                            }
                            explore_categories = out
                                .data
                                .categories
                                .into_iter()
                                .map(|c| MusicExploreCategoryDto {
                                    title: c.title,
                                    url: c.url,
                                })
                                .collect();
                            search_items = out
                                .data
                                .tracks
                                .into_iter()
                                .take(20)
                                .map(|item| {
                                    track_item_to_dto(
                                        id,
                                        &source_name,
                                        &qualities,
                                        has_cover_rule,
                                        item,
                                    )
                                })
                                .collect();
                            steps.push(SourceDebugStepDto {
                                stage: "explore".into(),
                                label: "榜单/浏览规则 (exploreUrl)".into(),
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
                                label: "榜单/浏览规则 (exploreUrl)".into(),
                                ok: false,
                                elapsed_ms,
                                summary: "榜单请求或解析失败".into(),
                                error: Some(err.message),
                                diagnostics: err.diagnostics,
                            });
                        }
                    }
                }

                let mut ctx = if let Some(ref item) = active_item {
                    item.to_context(requested_quality.clone())
                } else {
                    explicit_ctx
                };

                // Stage 2: Detail (if requested or configured in Full mode)
                let run_detail = match mode {
                    MusicSourceTestMode::Detail => true,
                    MusicSourceTestMode::Full => {
                        parsed.rule_track_detail.is_some()
                            && (!ctx.track_id.trim().is_empty()
                                || ctx
                                    .detail_url
                                    .as_deref()
                                    .is_some_and(|s| !s.trim().is_empty()))
                            && steps.last().is_none_or(|s| s.ok)
                    }
                    _ => false,
                };
                if run_detail {
                    let t0 = Instant::now();
                    match engine.fetch_track_detail_sync_with_control(
                        &parsed,
                        state.clone(),
                        &ctx,
                        remaining_budget(),
                        cancel_worker.clone(),
                    ) {
                        Ok(out) => {
                            let elapsed_ms = t0.elapsed().as_millis() as u64;
                            state = out.state;
                            merge_diagnostics(&mut all_diag, &out.diagnostics);
                            let ok = !out.data.title.trim().is_empty()
                                || !out.data.track_id.trim().is_empty();
                            let summary = if ok {
                                format!("{} - {}", out.data.title, out.data.artist)
                            } else {
                                "曲目详情未解析出标题或 ID".into()
                            };
                            let err = (!ok).then(|| "ruleTrackDetail 未返回有效曲目信息".into());
                            ctx = out.data.to_context(requested_quality.clone());
                            active_item = Some(out.data);
                            steps.push(SourceDebugStepDto {
                                stage: "detail".into(),
                                label: "详情规则 (ruleTrackDetail)".into(),
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
                                label: "详情规则 (ruleTrackDetail)".into(),
                                ok: false,
                                elapsed_ms,
                                summary: "曲目详情请求或解析失败".into(),
                                error: Some(err.message),
                                diagnostics: err.diagnostics,
                            });
                        }
                    }
                }

                // Stage 3: Stream
                let run_stream = match mode {
                    MusicSourceTestMode::Stream => true,
                    MusicSourceTestMode::Full => {
                        (!ctx.track_id.trim().is_empty()
                            || ctx.url_id.as_deref().is_some_and(|s| !s.trim().is_empty())
                            || ctx
                                .play_url
                                .as_deref()
                                .is_some_and(|s| !s.trim().is_empty()))
                            && steps.iter().all(|s| s.ok)
                    }
                    _ => false,
                };
                if run_stream {
                    let t0 = Instant::now();
                    match engine.resolve_stream_sync_with_control(
                        &parsed,
                        state.clone(),
                        &ctx,
                        requested_quality.as_deref(),
                        remaining_budget(),
                        cancel_worker.clone(),
                    ) {
                        Ok(out) => {
                            let elapsed_ms = t0.elapsed().as_millis() as u64;
                            state = out.state;
                            merge_diagnostics(&mut all_diag, &out.diagnostics);
                            let valid_url = is_http_url(&out.data.stream_url);
                            let summary = if valid_url {
                                format!(
                                    "流解析成功 ({}{})",
                                    out.data.format,
                                    out.data
                                        .bitrate
                                        .map(|b| format!(" / {b}kbps"))
                                        .unwrap_or_default()
                                )
                            } else {
                                "未解析出有效的 HTTP(S) 音频流地址".into()
                            };
                            let err = (!valid_url).then(|| "音频流地址无效".to_string());
                            if valid_url {
                                let proxy_stream_url =
                                    build_proxy_stream_url(id, &out.data.stream_url);
                                stream_probe = Some(OnlineMusicStreamResponseDto {
                                    source_id: id,
                                    stream_url: out.data.stream_url,
                                    proxy_stream_url,
                                    format: Some(out.data.format),
                                    bitrate: out.data.bitrate,
                                    expires_in_secs: out.data.expires_in_secs,
                                    headers_applied,
                                });
                            }
                            steps.push(SourceDebugStepDto {
                                stage: "stream".into(),
                                label: "流解析规则 (ruleStream)".into(),
                                ok: valid_url,
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
                                stage: "stream".into(),
                                label: "流解析规则 (ruleStream)".into(),
                                ok: false,
                                elapsed_ms,
                                summary: "音频流解析失败".into(),
                                error: Some(err.message),
                                diagnostics: err.diagnostics,
                            });
                        }
                    }
                }

                // Stage 4: Cover (Full mode if ruleCover configured and prior steps ok)
                if mode == MusicSourceTestMode::Full
                    && parsed.rule_cover.is_some()
                    && steps.iter().all(|s| s.ok)
                {
                    let t0 = Instant::now();
                    match engine.resolve_cover_sync_with_control(
                        &parsed,
                        state.clone(),
                        &ctx,
                        remaining_budget(),
                        cancel_worker.clone(),
                    ) {
                        Ok(out) => {
                            let elapsed_ms = t0.elapsed().as_millis() as u64;
                            state = out.state;
                            merge_diagnostics(&mut all_diag, &out.diagnostics);
                            cover_resolved = out.data.clone();
                            let ok = cover_resolved.as_deref().is_some_and(is_http_url);
                            let summary = if ok {
                                "封面 URL 解析成功".into()
                            } else {
                                "未解析出封面 URL".into()
                            };
                            steps.push(SourceDebugStepDto {
                                stage: "cover".into(),
                                label: "封面规则 (ruleCover)".into(),
                                ok,
                                elapsed_ms,
                                summary,
                                error: (!ok).then(|| "ruleCover 未返回有效封面 URL".into()),
                                diagnostics: out.diagnostics,
                            });
                        }
                        Err(err) => {
                            let elapsed_ms = t0.elapsed().as_millis() as u64;
                            merge_diagnostics(&mut all_diag, &err.diagnostics);
                            steps.push(SourceDebugStepDto {
                                stage: "cover".into(),
                                label: "封面规则 (ruleCover)".into(),
                                ok: false,
                                elapsed_ms,
                                summary: "封面解析失败".into(),
                                error: Some(err.message),
                                diagnostics: err.diagnostics,
                            });
                        }
                    }
                }

                // Stage 5: Lyric
                let run_lyric = match mode {
                    MusicSourceTestMode::Lyric => true,
                    MusicSourceTestMode::Full => {
                        parsed.rule_lyric.is_some() && steps.iter().all(|s| s.ok)
                    }
                    _ => false,
                };
                if run_lyric {
                    let t0 = Instant::now();
                    match engine.resolve_lyrics_sync_with_control(
                        &parsed,
                        state.clone(),
                        &ctx,
                        remaining_budget(),
                        cancel_worker.clone(),
                    ) {
                        Ok(out) => {
                            let elapsed_ms = t0.elapsed().as_millis() as u64;
                            state = out.state;
                            merge_diagnostics(&mut all_diag, &out.diagnostics);
                            let ok = out.data.as_ref().is_some_and(|l| !l.lyrics.is_empty());
                            let summary = if let Some(ref res) = out.data {
                                if res.lyrics.synced {
                                    format!("解析到 {} 行同步歌词", res.lyrics.lines.len())
                                } else {
                                    format!(
                                        "解析到纯文本歌词 ({} 字符)",
                                        res.lyrics.plain_text.chars().count()
                                    )
                                }
                            } else {
                                "未获取到歌词".into()
                            };
                            if let Some(res) = out.data {
                                lyrics_resolved = Some(res.lyrics);
                                translated_lyrics_resolved = res.translated_lyrics;
                            }
                            steps.push(SourceDebugStepDto {
                                stage: "lyric".into(),
                                label: "歌词规则 (ruleLyric)".into(),
                                ok,
                                elapsed_ms,
                                summary,
                                error: (!ok).then(|| "未解析到有效歌词内容".into()),
                                diagnostics: out.diagnostics,
                            });
                        }
                        Err(err) => {
                            let elapsed_ms = t0.elapsed().as_millis() as u64;
                            merge_diagnostics(&mut all_diag, &err.diagnostics);
                            steps.push(SourceDebugStepDto {
                                stage: "lyric".into(),
                                label: "歌词规则 (ruleLyric)".into(),
                                ok: false,
                                elapsed_ms,
                                summary: "歌词请求或解析失败".into(),
                                error: Some(err.message),
                                diagnostics: err.diagnostics,
                            });
                        }
                    }
                }

                let resolved_track = active_item.map(|item| {
                    let mut dto =
                        track_item_to_dto(id, &source_name, &qualities, has_cover_rule, item);
                    let final_cover = cover_resolved.clone().or_else(|| dto.cover_url.clone());
                    if let Some(ref c) = final_cover {
                        dto.cover_url = Some(c.clone());
                        dto.proxy_cover_url = build_proxy_cover_url(id, c);
                    }
                    let proxy_cover_url = final_cover
                        .as_deref()
                        .and_then(|c| build_proxy_cover_url(id, c));
                    OnlineMusicResolvedTrackDto {
                        source_id: id,
                        source_name: source_name.clone(),
                        track: dto,
                        stream_url: stream_probe.as_ref().map(|s| s.stream_url.clone()),
                        proxy_stream_url: stream_probe.as_ref().map(|s| s.proxy_stream_url.clone()),
                        cover_url: final_cover,
                        proxy_cover_url,
                        lyrics: lyrics_resolved.clone(),
                        translated_lyrics: translated_lyrics_resolved.clone(),
                        format: stream_probe.as_ref().and_then(|s| s.format.clone()),
                        bitrate: stream_probe.as_ref().and_then(|s| s.bitrate),
                        expires_in_secs: stream_probe.as_ref().and_then(|s| s.expires_in_secs),
                        diagnostics: ExecutionDiagnostics::default(),
                    }
                });

                let total_elapsed_ms = total_start.elapsed().as_millis() as u64;
                let (overall_ok, first_error) = summarize_steps_outcome(&steps);
                let sample_titles: Vec<String> = search_items
                    .iter()
                    .take(5)
                    .map(|t| t.title.clone())
                    .collect();

                let report = MusicSourceTestReportDto {
                    ok: overall_ok,
                    mode,
                    keyword: kw_clone,
                    total_elapsed_ms,
                    result_count: search_total_count,
                    sample_titles,
                    error: first_error,
                    diagnostics: all_diag,
                    steps,
                    payload: MusicSourceTestPayload {
                        search_items,
                        resolved_track,
                        stream_probe,
                        lyrics: lyrics_resolved,
                        explore_categories,
                    },
                };
                (state, report)
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
                return Err(DomainError::Storage(format!("测试音源线程失败: {e}")));
            }
            Err(_) => {
                cancel_flag.store(true, Ordering::Relaxed);
                let timeout_msg = format!(
                    "音源调试总耗时超过 {} 秒，已强制中止",
                    SOURCE_TEST_TOTAL_TIMEOUT.as_secs()
                );
                let report = MusicSourceTestReportDto {
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
                    payload: MusicSourceTestPayload::default(),
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
                merge_runtime_state_with_base(&prev_state_cmp, current, final_state, false)
            }
            None => current.unwrap_or(prev_state_cmp),
        };

        let _ = self
            .sources
            .update_test_result(id, merged_state, report.ok, Some(last_msg), now)
            .await?;

        Ok(report)
    }
}

mod stages;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::user::User;
use xmedia_source::crud::{format_test_failure_summary, format_timeout_step};

use crate::domain::online::{ComicSourceSpec, ExecutionDiagnostics};

use self::stages::{DebugRunParams, run_test_stages};
use super::dto::{
    ComicSourceTestInput, ComicSourceTestMode, ComicSourceTestPayload, ComicSourceTestReportDto,
};
use super::helpers::CancelOnDrop;
use super::{ComicOnlineService, SOURCE_TEST_TOTAL_TIMEOUT};

impl ComicOnlineService {
    pub async fn test_source(
        &self,
        actor: &User,
        id: Uuid,
        input: ComicSourceTestInput,
    ) -> DomainResult<ComicSourceTestReportDto> {
        let rec = self.source_crud.load_owned_source(actor, id).await?;
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

        let default_kw = parsed.default_test_keyword();

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
        let chapter_title_override = input
            .chapter_title
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);
        let base_state = rec.runtime_state.clone();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let mut cancel_guard = CancelOnDrop::new(cancel_flag.clone());

        let params = DebugRunParams {
            source_id: id,
            engine: self.engine.clone(),
            parsed,
            initial_state: rec.runtime_state,
            mode,
            keyword: effective_keyword.clone(),
            raw_input,
            explicit_explore_url,
            explicit_comic_url,
            explicit_toc_url,
            explicit_chapter_url,
            chapter_index_override: input.chapter_index,
            chapter_title_override,
            cancel_worker: cancel_flag.clone(),
        };

        let timed_res = tokio::time::timeout(
            SOURCE_TEST_TOTAL_TIMEOUT,
            tokio::task::spawn_blocking(move || run_test_stages(params)),
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

        if let Some(final_state) = maybe_final_state {
            self.source_crud
                .persist_runtime_state_with_base(id, &base_state, final_state, false)
                .await?;
        }
        self.source_crud
            .record_test_outcome(id, report.ok, last_msg)
            .await?;

        Ok(report)
    }
}

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::Semaphore;
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::user::User;
use xmedia_source::rate_limit::get_or_insert_rate_limiter;

use crate::application::ports::VideoProxyUrlBuilder;
use crate::domain::online::{VideoSourceFormat, VideoSourceSpec};

use super::dto::{
    OnlineVideoBrowseResponseDto, OnlineVideoCategoriesResponseDto, OnlineVideoDetailResponseDto,
    OnlineVideoSearchErrorDto, OnlineVideoSearchItemDto, OnlineVideoSearchResponseDto,
};
use super::{CancelOnDrop, CancelTasksOnDrop, SEARCH_MAX_CONCURRENCY, VideoOnlineService};

impl VideoOnlineService {
    pub async fn categories(
        &self,
        actor: &User,
        source_id: Uuid,
    ) -> DomainResult<OnlineVideoCategoriesResponseDto> {
        let rec = self.load_source_for_actor(actor, source_id).await?;
        let parsed: VideoSourceSpec = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("影视源 JSON 无效: {e}")))?;
        let op_timeout = self.op_timeout;
        self.acquire_rate_slot(source_id, parsed.parse_concurrent_rate(), op_timeout)
            .await
            .map_err(DomainError::Validation)?;
        let has_explore = match parsed.effective_format() {
            VideoSourceFormat::ApplecmsV10 => true,
            VideoSourceFormat::Rule => {
                parsed.enabled_explore
                    && parsed
                        .explore_url
                        .as_deref()
                        .is_some_and(|s| !s.trim().is_empty())
            }
        };

        let engine = self.engine.clone();
        let prev_state = rec.runtime_state.clone();
        let prev_state_cmp = prev_state.clone();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_worker = cancel_flag.clone();
        let mut cancel_guard = CancelOnDrop::new(cancel_flag.clone());

        let timed = tokio::time::timeout(
            op_timeout,
            tokio::task::spawn_blocking(move || {
                engine
                    .fetch_categories_sync_with_control(
                        &parsed,
                        prev_state,
                        op_timeout,
                        cancel_worker,
                    )
                    .map_err(|e| e.message)
            }),
        )
        .await;

        let mut out = match timed {
            Ok(Ok(Ok(res))) => {
                cancel_guard.disarm();
                res
            }
            Ok(Ok(Err(err))) => {
                cancel_guard.disarm();
                return Err(DomainError::Validation(err));
            }
            Ok(Err(e)) => {
                cancel_guard.disarm();
                return Err(DomainError::Storage(format!("获取影视分类失败: {e}")));
            }
            Err(_) => {
                cancel_flag.store(true, Ordering::Relaxed);
                return Err(DomainError::Validation(format!(
                    "获取影视分类超时（超过 {} 秒）",
                    op_timeout.as_secs()
                )));
            }
        };

        if out.state != prev_state_cmp
            && let Err(err) = self
                .persist_runtime_state_with_base(source_id, &prev_state_cmp, out.state, false)
                .await
        {
            tracing::warn!(source_id = %source_id, error = %err, "failed to persist runtime state after categories");
        }
        if !actor.role.is_admin() {
            out.diagnostics.logs.clear();
        }

        Ok(OnlineVideoCategoriesResponseDto {
            source_id,
            source_name: rec.name,
            categories: out.data,
            has_explore,
            diagnostics: out.diagnostics,
        })
    }

    pub async fn search(
        &self,
        actor: &User,
        keyword: &str,
        source_ids: Option<Vec<Uuid>>,
        page: u32,
        url_builder: &dyn VideoProxyUrlBuilder,
    ) -> DomainResult<OnlineVideoSearchResponseDto> {
        let kw = keyword.trim();
        if kw.is_empty() {
            return Err(DomainError::Validation("请输入搜索关键词".into()));
        }

        let mut sources = self.source_crud.repository().list_all().await?;
        let is_admin = actor.role.is_admin();

        let explicit_single_or_filtered = source_ids.as_ref().is_some_and(|v| !v.is_empty());
        if let Some(filter_ids) = source_ids.filter(|v| !v.is_empty()) {
            sources.retain(|s| filter_ids.contains(&s.id) && (is_admin || s.enabled));
        } else {
            sources.retain(|s| s.enabled);
        }

        if sources.is_empty() {
            return Err(DomainError::Validation("没有可用的已启用影视源".into()));
        }

        sources.sort_by(|a, b| b.weight.cmp(&a.weight).then_with(|| a.name.cmp(&b.name)));
        let page_num = page.max(1);
        let sem = Arc::new(Semaphore::new(SEARCH_MAX_CONCURRENCY));
        let op_timeout = self.op_timeout;

        let mut handles = Vec::with_capacity(sources.len());
        let mut cancel_all_guard = CancelTasksOnDrop::new();
        let multi_source_mode = !explicit_single_or_filtered || sources.len() > 1;

        for rec in sources {
            let parsed_res = serde_json::from_str::<VideoSourceSpec>(&rec.raw_json);
            if multi_source_mode
                && let Ok(ref parsed) = parsed_res
                && !parsed.searchable
            {
                continue;
            }

            let engine = self.engine.clone();
            let kw_owned = kw.to_string();
            let sid = rec.id;
            let sname = rec.name.clone();
            let state = rec.runtime_state.clone();
            let prev_state_cmp = state.clone();
            let sem_clone = Arc::clone(&sem);
            let rate_limiter = get_or_insert_rate_limiter(&self.rate_limiters, sid);
            let cancel_flag = Arc::new(AtomicBool::new(false));
            let cancel_for_task = cancel_flag.clone();

            let handle = tokio::spawn(async move {
                let parsed = match parsed_res {
                    Ok(p) => p,
                    Err(e) => {
                        return (
                            sid,
                            sname,
                            prev_state_cmp,
                            Err(format!("影视源解析失败: {e}")),
                        );
                    }
                };
                if let Some(rate) = parsed.parse_concurrent_rate()
                    && let Err(e) = rate_limiter.acquire(rate, op_timeout).await
                {
                    return (sid, sname, prev_state_cmp, Err(e));
                }
                let Ok(_permit) = sem_clone.acquire_owned().await else {
                    return (sid, sname, prev_state_cmp, Err("搜索并发调度已关闭".into()));
                };
                let cancel_worker = cancel_for_task.clone();
                let mut cancel_guard = CancelOnDrop::new(cancel_for_task.clone());
                let timed = tokio::time::timeout(
                    op_timeout,
                    tokio::task::spawn_blocking(move || {
                        engine
                            .search_sync_with_control(
                                &parsed,
                                state,
                                &kw_owned,
                                page_num,
                                op_timeout,
                                cancel_worker,
                            )
                            .map_err(Into::into)
                    }),
                )
                .await;
                match timed {
                    Ok(Ok(res)) => {
                        cancel_guard.disarm();
                        (sid, sname, prev_state_cmp, res)
                    }
                    Ok(Err(e)) => {
                        cancel_guard.disarm();
                        (
                            sid,
                            sname,
                            prev_state_cmp,
                            Err(format!("搜索任务异常: {e}")),
                        )
                    }
                    Err(_) => {
                        cancel_for_task.store(true, Ordering::Relaxed);
                        (
                            sid,
                            sname,
                            prev_state_cmp,
                            Err(format!("搜索超时（超过 {} 秒）", op_timeout.as_secs())),
                        )
                    }
                }
            });
            cancel_all_guard.push(cancel_flag, handle.abort_handle());
            handles.push(handle);
        }

        let mut results = Vec::new();
        let mut errors = Vec::new();
        let mut toasts = Vec::new();

        for h in handles {
            if let Ok((sid, sname, prev_state, res)) = h.await {
                match res {
                    Ok(out) => {
                        if out.state != prev_state
                            && let Err(err) = self
                                .persist_runtime_state_with_base(sid, &prev_state, out.state, false)
                                .await
                        {
                            tracing::warn!(source_id = %sid, error = %err, "failed to persist video source runtime state after search");
                        }
                        toasts.extend(out.diagnostics.toasts);
                        for mut video in out.data {
                            if let Some(ref cover) = video.cover_url
                                && !cover.trim().is_empty()
                            {
                                video.proxy_cover_url =
                                    Some(url_builder.poster_proxy_url(cover, Some(sid)));
                            }
                            results.push(OnlineVideoSearchItemDto {
                                source_id: sid,
                                source_name: sname.clone(),
                                video,
                            });
                        }
                    }
                    Err(err) => {
                        errors.push(OnlineVideoSearchErrorDto {
                            source_id: sid,
                            source_name: sname,
                            error: err,
                        });
                    }
                }
            }
        }
        cancel_all_guard.disarm();

        Ok(OnlineVideoSearchResponseDto {
            keyword: kw.to_string(),
            page: page_num,
            results,
            errors,
            toasts,
        })
    }

    pub async fn browse(
        &self,
        actor: &User,
        source_id: Uuid,
        category_id: Option<&str>,
        page: u32,
        url_builder: &dyn VideoProxyUrlBuilder,
    ) -> DomainResult<OnlineVideoBrowseResponseDto> {
        let rec = self.load_source_for_actor(actor, source_id).await?;
        let parsed: VideoSourceSpec = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("影视源 JSON 无效: {e}")))?;
        let op_timeout = self.op_timeout;
        self.acquire_rate_slot(source_id, parsed.parse_concurrent_rate(), op_timeout)
            .await
            .map_err(DomainError::Validation)?;

        let engine = self.engine.clone();
        let prev_state = rec.runtime_state.clone();
        let prev_state_cmp = prev_state.clone();
        let cat_owned = category_id
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);
        let cat_for_worker = cat_owned.clone();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_worker = cancel_flag.clone();
        let mut cancel_guard = CancelOnDrop::new(cancel_flag.clone());
        let page_num = page.max(1);

        let timed = tokio::time::timeout(
            op_timeout,
            tokio::task::spawn_blocking(move || {
                engine
                    .browse_sync_with_control(
                        &parsed,
                        prev_state,
                        cat_for_worker.as_deref(),
                        page_num,
                        op_timeout,
                        cancel_worker,
                    )
                    .map_err(|e| e.message)
            }),
        )
        .await;

        let mut out = match timed {
            Ok(Ok(Ok(res))) => {
                cancel_guard.disarm();
                res
            }
            Ok(Ok(Err(err))) => {
                cancel_guard.disarm();
                return Err(DomainError::Validation(err));
            }
            Ok(Err(e)) => {
                cancel_guard.disarm();
                return Err(DomainError::Storage(format!("分类浏览失败: {e}")));
            }
            Err(_) => {
                cancel_flag.store(true, Ordering::Relaxed);
                return Err(DomainError::Validation(format!(
                    "分类浏览超时（超过 {} 秒）",
                    op_timeout.as_secs()
                )));
            }
        };

        if out.state != prev_state_cmp
            && let Err(err) = self
                .persist_runtime_state_with_base(source_id, &prev_state_cmp, out.state, false)
                .await
        {
            tracing::warn!(source_id = %source_id, error = %err, "failed to persist runtime state after browse");
        }
        if !actor.role.is_admin() {
            out.diagnostics.logs.clear();
        }

        for item in &mut out.data.items {
            if let Some(ref cover) = item.cover_url
                && !cover.trim().is_empty()
            {
                item.proxy_cover_url = Some(url_builder.poster_proxy_url(cover, Some(source_id)));
            }
        }

        Ok(OnlineVideoBrowseResponseDto {
            source_id,
            source_name: rec.name,
            category_id: cat_owned,
            results: out.data.items,
            total: out.data.total,
            page: out.data.page,
            page_count: out.data.page_count,
            diagnostics: out.diagnostics,
        })
    }

    pub async fn detail(
        &self,
        actor: &User,
        source_id: Uuid,
        video_id: Option<&str>,
        detail_url: Option<&str>,
        url_builder: Option<&dyn VideoProxyUrlBuilder>,
    ) -> DomainResult<OnlineVideoDetailResponseDto> {
        let rec = self.load_source_for_actor(actor, source_id).await?;
        let parsed: VideoSourceSpec = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("影视源 JSON 无效: {e}")))?;
        let op_timeout = self.op_timeout;
        let detail_timeout = self.detail_timeout;
        self.acquire_rate_slot(source_id, parsed.parse_concurrent_rate(), op_timeout)
            .await
            .map_err(DomainError::Validation)?;
        let clean_detail_url =
            if let Some(raw_detail) = detail_url.map(str::trim).filter(|s| !s.is_empty()) {
                Some(xmedia_source::http::resolve_literal_http_url(
                    raw_detail,
                    Some(parsed.effective_source_url()),
                )?)
            } else {
                None
            };

        let engine = self.engine.clone();
        let prev_state = rec.runtime_state.clone();
        let prev_state_cmp = prev_state.clone();
        let vid_owned = video_id.map(ToString::to_string);
        let durl_owned = clean_detail_url;
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_worker = cancel_flag.clone();
        let mut cancel_guard = CancelOnDrop::new(cancel_flag.clone());

        let timed = tokio::time::timeout(
            detail_timeout,
            tokio::task::spawn_blocking(move || {
                engine
                    .fetch_detail_sync_with_control(
                        &parsed,
                        prev_state,
                        vid_owned.as_deref(),
                        durl_owned.as_deref(),
                        detail_timeout,
                        cancel_worker,
                    )
                    .map_err(|e| e.message)
            }),
        )
        .await;

        let mut out = match timed {
            Ok(Ok(Ok(res))) => {
                cancel_guard.disarm();
                res
            }
            Ok(Ok(Err(err))) => {
                cancel_guard.disarm();
                return Err(DomainError::Validation(err));
            }
            Ok(Err(e)) => {
                cancel_guard.disarm();
                return Err(DomainError::Storage(format!("获取影视详情失败: {e}")));
            }
            Err(_) => {
                cancel_flag.store(true, Ordering::Relaxed);
                return Err(DomainError::Validation(format!(
                    "获取影视详情超时（超过 {} 秒）",
                    detail_timeout.as_secs()
                )));
            }
        };

        if out.state != prev_state_cmp
            && let Err(err) = self
                .persist_runtime_state_with_base(source_id, &prev_state_cmp, out.state, false)
                .await
        {
            tracing::warn!(source_id = %source_id, error = %err, "failed to persist runtime state after detail");
        }
        if !actor.role.is_admin() {
            out.diagnostics.logs.clear();
        }

        if let Some(builder) = url_builder
            && let Some(ref cover) = out.data.cover_url
            && !cover.trim().is_empty()
        {
            out.data.proxy_cover_url = Some(builder.poster_proxy_url(cover, Some(source_id)));
        }

        Ok(OnlineVideoDetailResponseDto {
            source_id,
            source_name: rec.name,
            detail: out.data,
            diagnostics: out.diagnostics,
        })
    }
}

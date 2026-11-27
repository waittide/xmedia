use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::Semaphore;
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::user::User;
use xmedia_source::crud::SourceRecord;
use xmedia_source::http::HttpClientConfig;

use crate::domain::online::{
    BookJsContext, ChapterJsContext, ExecutionDiagnostics, LegadoBookSource,
};

use super::dto::{
    AggregatedSearchItemDto, AggregatedSearchResponseDto, OnlineBookDetailResponseDto,
    OnlineContentResponseDto, OnlineTocResponseDto, SourceSearchErrorDto,
};
use super::helpers::{CancelOnDrop, CancelTasksOnDrop, ChapterFetchOptions};
use super::{BookOnlineService, SEARCH_MAX_CONCURRENCY};

impl BookOnlineService {
    pub async fn search(
        &self,
        actor: &User,
        keyword: &str,
        source_ids: Option<Vec<Uuid>>,
        page: u32,
    ) -> DomainResult<AggregatedSearchResponseDto> {
        let kw = keyword.trim();
        if kw.is_empty() {
            return Err(DomainError::Validation("请输入搜索关键词".into()));
        }

        let sources = self
            .sources
            .select_sources(actor, source_ids.as_deref())
            .await?;
        let page_num = page.max(1);
        let sem = Arc::new(Semaphore::new(SEARCH_MAX_CONCURRENCY));
        let op_timeout = self.op_timeout;

        let mut handles = Vec::with_capacity(sources.len());
        let mut cancel_all_guard = CancelTasksOnDrop::new();
        for rec in sources {
            let engine = Arc::clone(&self.engine);
            let kw_owned = kw.to_string();
            let parsed_res =
                <LegadoBookSource as xmedia_source::crud::SourceSpecAdapter>::parse_single(
                    &rec.raw_json,
                );
            let sid = rec.id;
            let sname = rec.name.clone();
            let state = rec.runtime_state.clone();
            let prev_state_cmp = state.clone();
            let sem_clone = Arc::clone(&sem);
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
                            Err(format!("书源解析失败: {e}")),
                        );
                    }
                };
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

        let mut items = Vec::new();
        let mut errors = Vec::new();
        let mut toasts = Vec::new();

        for h in handles {
            if let Ok((sid, sname, prev_state, res)) = h.await {
                match res {
                    Ok(out) => {
                        if out.state != prev_state
                            && let Err(err) = self
                                .sources
                                .persist_runtime_state_with_base(sid, &prev_state, out.state, false)
                                .await
                        {
                            tracing::warn!(source_id = %sid, error = %err, "failed to persist source runtime state after search");
                        }
                        toasts.extend(out.diagnostics.toasts);
                        for book in out.data {
                            items.push(AggregatedSearchItemDto {
                                source_id: sid,
                                source_name: sname.clone(),
                                book,
                            });
                        }
                    }
                    Err(err) => {
                        errors.push(SourceSearchErrorDto {
                            source_id: sid,
                            source_name: sname,
                            error: err,
                        });
                    }
                }
            }
        }
        cancel_all_guard.disarm();

        Ok(AggregatedSearchResponseDto {
            keyword: kw.to_string(),
            page: page_num,
            items,
            errors,
            toasts,
        })
    }

    pub(super) async fn fetch_book_info_internal(
        &self,
        rec: SourceRecord,
        book_url: &str,
        fallback_hint: Option<BookJsContext>,
        include_logs: bool,
    ) -> DomainResult<OnlineBookDetailResponseDto> {
        let source_id = rec.id;
        let parsed = <LegadoBookSource as xmedia_source::crud::SourceSpecAdapter>::parse_single(
            &rec.raw_json,
        )
        .map_err(DomainError::Validation)?;

        let detail_timeout = self.detail_timeout;
        let engine = Arc::clone(&self.engine);
        let url_owned = book_url.to_string();
        let prev_state = rec.runtime_state.clone();
        let prev_state_cmp = prev_state.clone();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_worker = cancel_flag.clone();
        let mut cancel_guard = CancelOnDrop::new(cancel_flag.clone());

        let timed = tokio::time::timeout(
            detail_timeout,
            tokio::task::spawn_blocking(move || {
                engine
                    .fetch_book_info_sync_with_control(
                        &parsed,
                        prev_state,
                        &url_owned,
                        fallback_hint.as_ref(),
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
                return Err(DomainError::Storage(format!("获取书籍详情失败: {e}")));
            }
            Err(_) => {
                cancel_flag.store(true, Ordering::Relaxed);
                return Err(DomainError::Validation(format!(
                    "获取书籍详情超时（超过 {} 秒）",
                    detail_timeout.as_secs()
                )));
            }
        };

        if out.state != prev_state_cmp
            && let Err(err) = self
                .sources
                .persist_runtime_state_with_base(source_id, &prev_state_cmp, out.state, false)
                .await
        {
            tracing::warn!(source_id = %source_id, error = %err, "failed to persist runtime state after fetch_book_info");
        }
        if !include_logs {
            out.diagnostics.logs.clear();
        }

        Ok(OnlineBookDetailResponseDto {
            source_id,
            source_name: rec.name,
            detail: out.data,
            diagnostics: out.diagnostics,
        })
    }

    pub(super) fn validate_caller_http_url(raw: &str, raw_json: &str) -> DomainResult<String> {
        let base =
            <LegadoBookSource as xmedia_source::crud::SourceSpecAdapter>::parse_single(raw_json)
                .ok()
                .map(|s| s.book_source_url);
        xmedia_source::http::resolve_literal_http_url(raw, base.as_deref())
    }

    pub(super) fn validate_caller_source_url(raw: &str, raw_json: &str) -> DomainResult<String> {
        const MAX_DATA_URL_BYTES: usize = 256 * 1024;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(DomainError::Validation("URL 不能为空".to_string()));
        }

        if trimmed
            .get(..5)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("data:"))
        {
            let parsed_src =
                <LegadoBookSource as xmedia_source::crud::SourceSpecAdapter>::parse_single(
                    raw_json,
                )
                .ok();
            if !parsed_src
                .as_ref()
                .is_some_and(LegadoBookSource::supports_data_pseudo_urls)
            {
                return Err(DomainError::Validation(
                    "该接口不接受 data: URL".to_string(),
                ));
            }
            if trimmed.len() > MAX_DATA_URL_BYTES {
                return Err(DomainError::Validation(
                    "data: URL 长度超出限制".to_string(),
                ));
            }
            let parsed_url = xmedia_source::http::parse_legado_url(trimmed);
            if parsed_url.options.method.is_some()
                || parsed_url.options.headers.is_some()
                || parsed_url.options.body.is_some()
                || parsed_url.options.web_view.is_some()
            {
                return Err(DomainError::Validation(
                    "data: URL 不允许携带 HTTP 请求配置选项".to_string(),
                ));
            }
            match xmedia_source::http::try_resolve_data_url(&parsed_url) {
                Some(Ok(_)) => return Ok(trimmed.to_string()),
                Some(Err(err)) => return Err(DomainError::Validation(err)),
                None => {
                    return Err(DomainError::Validation("无效的 data: URL".to_string()));
                }
            }
        }

        Self::validate_caller_http_url(trimmed, raw_json)
    }

    pub async fn fetch_book_info(
        &self,
        actor: &User,
        source_id: Uuid,
        book_url: &str,
        fallback_hint: Option<BookJsContext>,
    ) -> DomainResult<OnlineBookDetailResponseDto> {
        let rec = self.sources.load_source_for_actor(actor, source_id).await?;
        let clean_book_url = Self::validate_caller_source_url(book_url, &rec.raw_json)?;
        if let Some(ref hint) = fallback_hint {
            if !hint.book_url.trim().is_empty() {
                Self::validate_caller_source_url(&hint.book_url, &rec.raw_json)?;
            }
            if !hint.toc_url.trim().is_empty() {
                Self::validate_caller_source_url(&hint.toc_url, &rec.raw_json)?;
            }
        }
        self.fetch_book_info_internal(rec, &clean_book_url, fallback_hint, actor.role.is_admin())
            .await
    }

    pub(super) async fn fetch_toc_internal(
        &self,
        rec: SourceRecord,
        toc_url: &str,
        book_ctx: BookJsContext,
        include_logs: bool,
    ) -> DomainResult<OnlineTocResponseDto> {
        let source_id = rec.id;
        let parsed = <LegadoBookSource as xmedia_source::crud::SourceSpecAdapter>::parse_single(
            &rec.raw_json,
        )
        .map_err(DomainError::Validation)?;

        let detail_timeout = self.detail_timeout;
        let engine = Arc::clone(&self.engine);
        let url_owned = toc_url.to_string();
        let prev_state = rec.runtime_state.clone();
        let prev_state_cmp = prev_state.clone();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_worker = cancel_flag.clone();
        let mut cancel_guard = CancelOnDrop::new(cancel_flag.clone());

        let timed = tokio::time::timeout(
            detail_timeout,
            tokio::task::spawn_blocking(move || {
                engine
                    .fetch_toc_sync_with_control(
                        &parsed,
                        prev_state,
                        &url_owned,
                        &book_ctx,
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
                return Err(DomainError::Storage(format!("获取目录失败: {e}")));
            }
            Err(_) => {
                cancel_flag.store(true, Ordering::Relaxed);
                return Err(DomainError::Validation(format!(
                    "获取目录超时（超过 {} 秒）",
                    detail_timeout.as_secs()
                )));
            }
        };

        if out.state != prev_state_cmp
            && let Err(err) = self
                .sources
                .persist_runtime_state_with_base(source_id, &prev_state_cmp, out.state, false)
                .await
        {
            tracing::warn!(source_id = %source_id, error = %err, "failed to persist runtime state after fetch_toc");
        }
        if !include_logs {
            out.diagnostics.logs.clear();
        }

        Ok(OnlineTocResponseDto {
            source_id,
            chapters: out.data,
            diagnostics: out.diagnostics,
        })
    }

    pub async fn fetch_toc(
        &self,
        actor: &User,
        source_id: Uuid,
        toc_url: &str,
        book_ctx: BookJsContext,
    ) -> DomainResult<OnlineTocResponseDto> {
        let rec = self.sources.load_source_for_actor(actor, source_id).await?;
        let clean_toc_url = Self::validate_caller_source_url(toc_url, &rec.raw_json)?;
        if !book_ctx.book_url.trim().is_empty() {
            Self::validate_caller_source_url(&book_ctx.book_url, &rec.raw_json)?;
        }
        if !book_ctx.toc_url.trim().is_empty() {
            Self::validate_caller_source_url(&book_ctx.toc_url, &rec.raw_json)?;
        }
        self.fetch_toc_internal(rec, &clean_toc_url, book_ctx, actor.role.is_admin())
            .await
    }

    pub(super) async fn fetch_chapter_content_internal(
        &self,
        rec: SourceRecord,
        chapter_url: &str,
        book_ctx: BookJsContext,
        chapter_ctx: ChapterJsContext,
        opts: ChapterFetchOptions,
    ) -> DomainResult<OnlineContentResponseDto> {
        let source_id = rec.id;
        if !opts.force_refresh {
            let store = Arc::clone(&self.tracked_store);
            let ch_url = chapter_url.to_string();
            let cached = tokio::task::spawn_blocking(move || {
                store.read_cached_online_chapter(source_id, &ch_url)
            })
            .await
            .ok()
            .flatten();
            if let Some(cached) = cached {
                return Ok(OnlineContentResponseDto {
                    source_id,
                    chapter_index: chapter_ctx.index,
                    content: cached,
                    cached: true,
                    diagnostics: ExecutionDiagnostics::default(),
                });
            }
        }

        let parsed = <LegadoBookSource as xmedia_source::crud::SourceSpecAdapter>::parse_single(
            &rec.raw_json,
        )
        .map_err(DomainError::Validation)?;

        let detail_timeout = self.detail_timeout;
        let engine = Arc::clone(&self.engine);
        let url_owned = chapter_url.to_string();
        let prev_state = rec.runtime_state.clone();
        let prev_state_cmp = prev_state.clone();
        let ch_idx = chapter_ctx.index;
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_worker = cancel_flag.clone();
        let mut cancel_guard = CancelOnDrop::new(cancel_flag.clone());

        let timed = tokio::time::timeout(
            detail_timeout,
            tokio::task::spawn_blocking(move || {
                engine
                    .fetch_chapter_content_sync_with_control(
                        &parsed,
                        prev_state,
                        &url_owned,
                        &book_ctx,
                        &chapter_ctx,
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
                return Err(DomainError::Storage(format!("获取章节正文失败: {e}")));
            }
            Err(_) => {
                cancel_flag.store(true, Ordering::Relaxed);
                return Err(DomainError::Validation(format!(
                    "获取章节正文超时（超过 {} 秒）",
                    detail_timeout.as_secs()
                )));
            }
        };

        if out.state != prev_state_cmp
            && let Err(err) = self
                .sources
                .persist_runtime_state_with_base(source_id, &prev_state_cmp, out.state, false)
                .await
        {
            tracing::warn!(source_id = %source_id, error = %err, "failed to persist runtime state after fetch_chapter_content");
        }
        if opts.allow_shared_cache_write {
            let store = Arc::clone(&self.tracked_store);
            let ch_url = chapter_url.to_string();
            let content_clone = out.data.clone();
            let _ = tokio::task::spawn_blocking(move || {
                store.write_cached_online_chapter(source_id, &ch_url, &content_clone);
            })
            .await;
        }
        if !opts.include_logs {
            out.diagnostics.logs.clear();
        }

        Ok(OnlineContentResponseDto {
            source_id,
            chapter_index: ch_idx,
            content: out.data,
            cached: false,
            diagnostics: out.diagnostics,
        })
    }

    pub async fn fetch_chapter_content(
        &self,
        actor: &User,
        source_id: Uuid,
        chapter_url: &str,
        book_ctx: BookJsContext,
        chapter_ctx: ChapterJsContext,
        force_refresh: bool,
    ) -> DomainResult<OnlineContentResponseDto> {
        let rec = self.sources.load_source_for_actor(actor, source_id).await?;
        let clean_chapter_url = Self::validate_caller_source_url(chapter_url, &rec.raw_json)?;
        if !book_ctx.book_url.trim().is_empty() {
            Self::validate_caller_source_url(&book_ctx.book_url, &rec.raw_json)?;
        }
        if !book_ctx.toc_url.trim().is_empty() {
            Self::validate_caller_source_url(&book_ctx.toc_url, &rec.raw_json)?;
        }
        if !chapter_ctx.url.trim().is_empty() {
            Self::validate_caller_source_url(&chapter_ctx.url, &rec.raw_json)?;
        }
        self.fetch_chapter_content_internal(
            rec,
            &clean_chapter_url,
            book_ctx,
            chapter_ctx,
            ChapterFetchOptions {
                force_refresh,
                include_logs: actor.role.is_admin(),
                allow_shared_cache_write: actor.role.is_admin(),
            },
        )
        .await
    }

    pub async fn fetch_cover_proxy(&self, raw_url: &str) -> DomainResult<(Vec<u8>, String)> {
        self.fetch_cover_proxy_with_source(None, raw_url, None)
            .await
    }

    pub async fn fetch_cover_proxy_with_source(
        &self,
        actor: Option<&User>,
        raw_url: &str,
        source_id: Option<Uuid>,
    ) -> DomainResult<(Vec<u8>, String)> {
        let trimmed = raw_url.trim();
        let mut source_url: Option<String> = None;
        let mut headers: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();

        if let Some(sid) = source_id {
            let rec = match actor {
                Some(u) => self.sources.load_source_for_actor(u, sid).await?,
                None => self.sources.load_source(sid).await?,
            };
            if let Ok(spec) =
                <LegadoBookSource as xmedia_source::crud::SourceSpecAdapter>::parse_single(
                    &rec.raw_json,
                )
            {
                headers =
                    spec.build_image_headers_with_cookies(&rec.runtime_state.cookies, trimmed);
                source_url = Some(spec.book_source_url);
            }
        }

        let cfg = HttpClientConfig {
            timeout_secs: xmedia_source::proxy::DEFAULT_IMAGE_PROXY_TIMEOUT_SECS,
            allow_private_ips: self.allow_private_ips,
            proxy_url: None,
        };
        let header_opt = (!headers.is_empty()).then_some(&headers);
        let (bytes, mime) = xmedia_source::proxy::fetch_source_image(
            source_url.as_deref(),
            trimmed,
            header_opt,
            xmedia_source::proxy::DEFAULT_IMAGE_PROXY_MAX_BYTES,
            &cfg,
        )
        .await?;
        Ok((bytes, mime.to_string()))
    }
}

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::Semaphore;
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::user::User;

use crate::domain::online::BookSourceRecord;
use crate::source::http::{get_or_build_async_client, validate_target_url_static};
use crate::source::model::{
    BookJsContext, ChapterJsContext, ExecutionDiagnostics, LegadoBookSource,
};

use super::dto::{
    AggregatedSearchItemDto, AggregatedSearchResponseDto, OnlineBookDetailResponseDto,
    OnlineContentResponseDto, OnlineTocResponseDto, SourceSearchErrorDto,
};
use super::helpers::{
    ChapterFetchOptions, read_cached_chapter_at, validate_cover_header_mime,
    validate_safe_cover_image, write_cached_chapter_at,
};
use super::{BookOnlineService, SEARCH_MAX_CONCURRENCY, SEARCH_PER_SOURCE_TIMEOUT};

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

        let mut sources = self.sources.list_all().await?;
        let is_admin = actor.role.is_admin();

        if let Some(filter_ids) = source_ids.filter(|v| !v.is_empty()) {
            sources.retain(|s| filter_ids.contains(&s.id) && (is_admin || s.enabled));
        } else {
            sources.retain(|s| s.enabled);
        }

        if sources.is_empty() {
            return Err(DomainError::Validation("没有可用的已启用书源".into()));
        }

        sources.sort_by(|a, b| b.weight.cmp(&a.weight).then_with(|| a.name.cmp(&b.name)));
        let page_num = page.max(1);
        let sem = Arc::new(Semaphore::new(SEARCH_MAX_CONCURRENCY));

        let mut handles = Vec::with_capacity(sources.len());
        for rec in sources {
            let engine = self.engine.clone();
            let kw_owned = kw.to_string();
            let parsed_res = serde_json::from_str::<LegadoBookSource>(&rec.raw_json);
            let sid = rec.id;
            let sname = rec.name.clone();
            let state = rec.runtime_state.clone();
            let prev_state_cmp = state.clone();
            let sem_clone = Arc::clone(&sem);

            handles.push(tokio::spawn(async move {
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
                let cancel_flag = Arc::new(AtomicBool::new(false));
                let cancel_worker = cancel_flag.clone();
                let timed = tokio::time::timeout(
                    SEARCH_PER_SOURCE_TIMEOUT,
                    tokio::task::spawn_blocking(move || {
                        engine
                            .search_sync_with_control(
                                &parsed,
                                state,
                                &kw_owned,
                                page_num,
                                SEARCH_PER_SOURCE_TIMEOUT,
                                cancel_worker,
                            )
                            .map_err(Into::into)
                    }),
                )
                .await;
                match timed {
                    Ok(Ok(res)) => (sid, sname, prev_state_cmp, res),
                    Ok(Err(e)) => (
                        sid,
                        sname,
                        prev_state_cmp,
                        Err(format!("搜索任务异常: {e}")),
                    ),
                    Err(_) => {
                        cancel_flag.store(true, Ordering::Relaxed);
                        (
                            sid,
                            sname,
                            prev_state_cmp,
                            Err(format!(
                                "搜索超时（超过 {} 秒）",
                                SEARCH_PER_SOURCE_TIMEOUT.as_secs()
                            )),
                        )
                    }
                }
            }));
        }

        let mut items = Vec::new();
        let mut errors = Vec::new();
        let mut toasts = Vec::new();

        for h in handles {
            if let Ok((sid, sname, prev_state, res)) = h.await {
                match res {
                    Ok(out) => {
                        if out.state != prev_state
                            && let Err(err) = self.persist_runtime_state(sid, out.state).await
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
        rec: BookSourceRecord,
        book_url: &str,
        fallback_hint: Option<BookJsContext>,
        include_logs: bool,
    ) -> DomainResult<OnlineBookDetailResponseDto> {
        let source_id = rec.id;
        let parsed: LegadoBookSource = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("书源 JSON 无效: {e}")))?;

        let engine = self.engine.clone();
        let url_owned = book_url.to_string();
        let prev_state = rec.runtime_state.clone();
        let prev_state_cmp = prev_state.clone();

        let mut out = tokio::task::spawn_blocking(move || {
            engine.fetch_book_info_sync(&parsed, prev_state, &url_owned, fallback_hint.as_ref())
        })
        .await
        .map_err(|e| DomainError::Storage(format!("获取书籍详情失败: {e}")))?
        .map_err(DomainError::Validation)?;

        if out.state != prev_state_cmp
            && let Err(err) = self.persist_runtime_state(source_id, out.state).await
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

    pub async fn fetch_book_info(
        &self,
        actor: &User,
        source_id: Uuid,
        book_url: &str,
        fallback_hint: Option<BookJsContext>,
    ) -> DomainResult<OnlineBookDetailResponseDto> {
        let rec = self.load_source_for_actor(actor, source_id).await?;
        self.fetch_book_info_internal(rec, book_url, fallback_hint, actor.role.is_admin())
            .await
    }

    pub(super) async fn fetch_toc_internal(
        &self,
        rec: BookSourceRecord,
        toc_url: &str,
        book_ctx: BookJsContext,
        include_logs: bool,
    ) -> DomainResult<OnlineTocResponseDto> {
        let source_id = rec.id;
        let parsed: LegadoBookSource = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("书源 JSON 无效: {e}")))?;

        let engine = self.engine.clone();
        let url_owned = toc_url.to_string();
        let prev_state = rec.runtime_state.clone();
        let prev_state_cmp = prev_state.clone();

        let mut out = tokio::task::spawn_blocking(move || {
            engine.fetch_toc_sync(&parsed, prev_state, &url_owned, &book_ctx)
        })
        .await
        .map_err(|e| DomainError::Storage(format!("获取目录失败: {e}")))?
        .map_err(DomainError::Validation)?;

        if out.state != prev_state_cmp
            && let Err(err) = self.persist_runtime_state(source_id, out.state).await
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
        let rec = self.load_source_for_actor(actor, source_id).await?;
        self.fetch_toc_internal(rec, toc_url, book_ctx, actor.role.is_admin())
            .await
    }

    pub(super) async fn fetch_chapter_content_internal(
        &self,
        rec: BookSourceRecord,
        chapter_url: &str,
        book_ctx: BookJsContext,
        chapter_ctx: ChapterJsContext,
        opts: ChapterFetchOptions,
    ) -> DomainResult<OnlineContentResponseDto> {
        let source_id = rec.id;
        if !opts.force_refresh {
            let cache_dir = self.cache_dir.clone();
            let ch_url = chapter_url.to_string();
            let cached = tokio::task::spawn_blocking(move || {
                read_cached_chapter_at(&cache_dir, source_id, &ch_url)
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

        let parsed: LegadoBookSource = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("书源 JSON 无效: {e}")))?;

        let engine = self.engine.clone();
        let url_owned = chapter_url.to_string();
        let prev_state = rec.runtime_state.clone();
        let prev_state_cmp = prev_state.clone();
        let ch_idx = chapter_ctx.index;

        let mut out = tokio::task::spawn_blocking(move || {
            engine.fetch_chapter_content_sync(
                &parsed,
                prev_state,
                &url_owned,
                &book_ctx,
                &chapter_ctx,
            )
        })
        .await
        .map_err(|e| DomainError::Storage(format!("获取章节正文失败: {e}")))?
        .map_err(DomainError::Validation)?;

        if out.state != prev_state_cmp
            && let Err(err) = self.persist_runtime_state(source_id, out.state).await
        {
            tracing::warn!(source_id = %source_id, error = %err, "failed to persist runtime state after fetch_chapter_content");
        }
        if opts.allow_shared_cache_write {
            let cache_dir = self.cache_dir.clone();
            let ch_url = chapter_url.to_string();
            let content_clone = out.data.clone();
            let _ = tokio::task::spawn_blocking(move || {
                write_cached_chapter_at(&cache_dir, source_id, &ch_url, &content_clone);
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
        let rec = self.load_source_for_actor(actor, source_id).await?;
        self.fetch_chapter_content_internal(
            rec,
            chapter_url,
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
        const MAX_COVER_BYTES: usize = 8 * 1024 * 1024;
        let validated = validate_target_url_static(raw_url.trim(), self.allow_private_ips)
            .map_err(DomainError::Validation)?;
        let client = get_or_build_async_client(12, self.allow_private_ips, None)
            .map_err(DomainError::Storage)?;

        let mut resp = client
            .get(validated)
            .send()
            .await
            .map_err(|e| DomainError::Validation(format!("拉取封面失败: {e}")))?;

        if !resp.status().is_success() {
            return Err(DomainError::Validation(format!(
                "封面响应状态异常: {}",
                resp.status()
            )));
        }

        let header_ct = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.split(';').next().unwrap_or(s).trim().to_ascii_lowercase());

        if let Some(ref ct) = header_ct {
            validate_cover_header_mime(ct)?;
        }

        if let Some(content_len) = resp.content_length()
            && content_len > MAX_COVER_BYTES as u64
        {
            return Err(DomainError::Validation("封面图片体积超过 8MB 限制".into()));
        }

        let mut bytes = Vec::new();
        while let Some(chunk) = resp
            .chunk()
            .await
            .map_err(|e| DomainError::Validation(format!("读取封面失败: {e}")))?
        {
            if bytes.len().saturating_add(chunk.len()) > MAX_COVER_BYTES {
                return Err(DomainError::Validation("封面图片体积超过 8MB 限制".into()));
            }
            bytes.extend_from_slice(&chunk);
        }

        let safe_mime = validate_safe_cover_image(header_ct.as_deref(), &bytes)?;
        Ok((bytes, safe_mime))
    }
}

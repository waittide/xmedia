use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::user::User;
use xmedia_source::crud::merge_diagnostics;
use xmedia_source::http::resolve_literal_http_url;

use crate::application::online::dto::{
    OnlineComicChaptersResponseDto, OnlineComicDetailDto, OnlineComicDetailResponseDto,
    OnlineComicPageDto, OnlineComicPagesResponseDto,
};
use crate::application::online::helpers::{CancelOnDrop, build_image_proxy_url};
use crate::application::online::{
    ComicOnlineService, READER_OP_TIMEOUT, SEARCH_PER_SOURCE_TIMEOUT,
};
use crate::domain::online::{
    ComicChapterJsContext, ComicJsContext, ComicSourceSpec, parse_reading_direction_opt,
};

impl ComicOnlineService {
    pub async fn fetch_detail(
        &self,
        actor: &User,
        source_id: Uuid,
        comic_url: &str,
        fallback_hint: Option<ComicJsContext>,
    ) -> DomainResult<OnlineComicDetailResponseDto> {
        let rec = self
            .source_crud
            .load_source_for_actor(actor, source_id)
            .await?;
        let parsed: ComicSourceSpec = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("漫画源 JSON 无效: {e}")))?;
        let clean_comic_url = resolve_literal_http_url(comic_url, Some(&parsed.source_url))?;
        if let Some(ref hint) = fallback_hint {
            if !hint.book_url.trim().is_empty() {
                resolve_literal_http_url(&hint.book_url, Some(&parsed.source_url))?;
            }
            if !hint.toc_url.trim().is_empty() {
                resolve_literal_http_url(&hint.toc_url, Some(&parsed.source_url))?;
            }
        }
        self.acquire_rate_slot(
            source_id,
            parsed.parse_concurrent_rate(),
            SEARCH_PER_SOURCE_TIMEOUT,
        )
        .await
        .map_err(DomainError::Validation)?;

        let engine = self.engine.clone();
        let url_owned = clean_comic_url;
        let prev_state = rec.runtime_state.clone();
        let prev_state_cmp = prev_state.clone();
        let parsed_for_detail = parsed.clone();
        let has_toc_rule = parsed
            .rule_toc
            .as_ref()
            .and_then(|t| t.chapter_list.as_deref())
            .is_some_and(|s| !s.trim().is_empty());
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_worker = cancel_flag.clone();
        let mut cancel_guard = CancelOnDrop::new(cancel_flag.clone());

        let timed = tokio::time::timeout(
            READER_OP_TIMEOUT,
            tokio::task::spawn_blocking(move || {
                let total_start = Instant::now();
                let remaining_budget = || {
                    READER_OP_TIMEOUT
                        .saturating_sub(total_start.elapsed())
                        .max(Duration::from_millis(1))
                };
                let mut out = engine
                    .fetch_detail_sync_with_control(
                        &parsed_for_detail,
                        prev_state,
                        &url_owned,
                        fallback_hint.as_ref(),
                        remaining_budget(),
                        cancel_worker.clone(),
                    )
                    .map_err(String::from)?;

                let mut chapters = Vec::new();
                if has_toc_rule && !out.data.toc_url.trim().is_empty() {
                    let toc_url = out.data.toc_url.clone();
                    let comic_ctx = ComicJsContext {
                        name: out.data.title.clone(),
                        author: out.data.author.clone(),
                        book_url: out.data.comic_url.clone(),
                        toc_url: toc_url.clone(),
                        cover_url: out.data.cover_url.clone(),
                        intro: out.data.intro.clone(),
                        reading_direction: out
                            .data
                            .reading_direction
                            .map(|d| d.as_str().to_string()),
                        dur_chapter_index: 0,
                        dur_chapter_title: String::new(),
                        custom_variable: String::new(),
                    };
                    let st_toc = out.state.clone();
                    match engine.fetch_chapters_sync_with_control(
                        &parsed_for_detail,
                        st_toc,
                        &toc_url,
                        &comic_ctx,
                        remaining_budget(),
                        cancel_worker.clone(),
                    ) {
                        Ok(ch_out) => {
                            out.state = ch_out.state;
                            merge_diagnostics(&mut out.diagnostics, &ch_out.diagnostics);
                            chapters = ch_out.data;
                        }
                        Err(err) => {
                            if cancel_worker.load(Ordering::Relaxed)
                                || total_start.elapsed() >= READER_OP_TIMEOUT
                            {
                                return Err(format!(
                                    "获取漫画详情超时（超过 {} 秒）",
                                    READER_OP_TIMEOUT.as_secs()
                                ));
                            }
                            out.diagnostics
                                .logs
                                .push(format!("fetch_detail chapters skipped: {}", err.message));
                        }
                    }
                }
                Ok::<_, String>((out, chapters))
            }),
        )
        .await;

        let (mut out, chapters) = match timed {
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
                return Err(DomainError::Storage(format!("获取漫画详情失败: {e}")));
            }
            Err(_) => {
                cancel_flag.store(true, Ordering::Relaxed);
                return Err(DomainError::Validation(format!(
                    "获取漫画详情超时（超过 {} 秒）",
                    READER_OP_TIMEOUT.as_secs()
                )));
            }
        };

        if out.state != prev_state_cmp
            && let Err(err) = self
                .source_crud
                .persist_runtime_state_with_base(source_id, &prev_state_cmp, out.state, false)
                .await
        {
            tracing::warn!(
                source_id = %source_id,
                error = %err,
                "failed to persist runtime state after fetch_detail"
            );
        }
        if !actor.role.is_admin() {
            out.diagnostics.logs.clear();
        }

        let proxy_cover_url = out
            .data
            .cover_url
            .as_deref()
            .map(|u| build_image_proxy_url(source_id, u));
        let reading_direction = out
            .data
            .reading_direction
            .unwrap_or_else(|| parsed.effective_reading_direction());

        Ok(OnlineComicDetailResponseDto {
            source_id,
            source_name: rec.name,
            detail: OnlineComicDetailDto {
                title: out.data.title,
                author: out.data.author,
                comic_url: out.data.comic_url,
                toc_url: out.data.toc_url,
                cover_url: out.data.cover_url,
                proxy_cover_url,
                intro: out.data.intro,
                kind: out.data.kind,
                status: out.data.status,
                last_chapter: out.data.last_chapter,
                word_count: out.data.word_count,
                reading_direction,
            },
            chapters,
            diagnostics: out.diagnostics,
        })
    }

    pub async fn fetch_chapters(
        &self,
        actor: &User,
        source_id: Uuid,
        toc_url: &str,
        comic_ctx: ComicJsContext,
    ) -> DomainResult<OnlineComicChaptersResponseDto> {
        let rec = self
            .source_crud
            .load_source_for_actor(actor, source_id)
            .await?;
        let parsed: ComicSourceSpec = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("漫画源 JSON 无效: {e}")))?;
        let clean_toc_url = resolve_literal_http_url(toc_url, Some(&parsed.source_url))?;
        if !comic_ctx.book_url.trim().is_empty() {
            resolve_literal_http_url(&comic_ctx.book_url, Some(&parsed.source_url))?;
        }
        if !comic_ctx.toc_url.trim().is_empty() {
            resolve_literal_http_url(&comic_ctx.toc_url, Some(&parsed.source_url))?;
        }
        self.acquire_rate_slot(
            source_id,
            parsed.parse_concurrent_rate(),
            SEARCH_PER_SOURCE_TIMEOUT,
        )
        .await
        .map_err(DomainError::Validation)?;

        let engine = self.engine.clone();
        let url_owned = clean_toc_url;
        let prev_state = rec.runtime_state.clone();
        let prev_state_cmp = prev_state.clone();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_worker = cancel_flag.clone();
        let mut cancel_guard = CancelOnDrop::new(cancel_flag.clone());

        let timed = tokio::time::timeout(
            READER_OP_TIMEOUT,
            tokio::task::spawn_blocking(move || {
                engine
                    .fetch_chapters_sync_with_control(
                        &parsed,
                        prev_state,
                        &url_owned,
                        &comic_ctx,
                        READER_OP_TIMEOUT,
                        cancel_worker,
                    )
                    .map_err(String::from)
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
                return Err(DomainError::Storage(format!("获取漫画章节列表失败: {e}")));
            }
            Err(_) => {
                cancel_flag.store(true, Ordering::Relaxed);
                return Err(DomainError::Validation(format!(
                    "获取漫画章节列表超时（超过 {} 秒）",
                    READER_OP_TIMEOUT.as_secs()
                )));
            }
        };

        if out.state != prev_state_cmp
            && let Err(err) = self
                .source_crud
                .persist_runtime_state_with_base(source_id, &prev_state_cmp, out.state, false)
                .await
        {
            tracing::warn!(
                source_id = %source_id,
                error = %err,
                "failed to persist runtime state after fetch_chapters"
            );
        }
        if !actor.role.is_admin() {
            out.diagnostics.logs.clear();
        }

        Ok(OnlineComicChaptersResponseDto {
            source_id,
            source_name: rec.name,
            toc_url: toc_url.to_string(),
            chapters: out.data,
            diagnostics: out.diagnostics,
        })
    }

    pub async fn fetch_pages(
        &self,
        actor: &User,
        source_id: Uuid,
        chapter_url: &str,
        comic_ctx: ComicJsContext,
        chapter_ctx: ComicChapterJsContext,
    ) -> DomainResult<OnlineComicPagesResponseDto> {
        let rec = self
            .source_crud
            .load_source_for_actor(actor, source_id)
            .await?;
        let parsed: ComicSourceSpec = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("漫画源 JSON 无效: {e}")))?;
        let clean_chapter_url = resolve_literal_http_url(chapter_url, Some(&parsed.source_url))?;
        if !comic_ctx.book_url.trim().is_empty() {
            resolve_literal_http_url(&comic_ctx.book_url, Some(&parsed.source_url))?;
        }
        if !comic_ctx.toc_url.trim().is_empty() {
            resolve_literal_http_url(&comic_ctx.toc_url, Some(&parsed.source_url))?;
        }
        if !chapter_ctx.url.trim().is_empty() {
            resolve_literal_http_url(&chapter_ctx.url, Some(&parsed.source_url))?;
        }
        self.acquire_rate_slot(
            source_id,
            parsed.parse_concurrent_rate(),
            SEARCH_PER_SOURCE_TIMEOUT,
        )
        .await
        .map_err(DomainError::Validation)?;

        let reading_direction = comic_ctx
            .reading_direction
            .as_deref()
            .and_then(parse_reading_direction_opt)
            .unwrap_or_else(|| parsed.effective_reading_direction());

        let engine = self.engine.clone();
        let url_owned = clean_chapter_url;
        let prev_state = rec.runtime_state.clone();
        let prev_state_cmp = prev_state.clone();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_worker = cancel_flag.clone();
        let mut cancel_guard = CancelOnDrop::new(cancel_flag.clone());

        let timed = tokio::time::timeout(
            READER_OP_TIMEOUT,
            tokio::task::spawn_blocking(move || {
                engine
                    .fetch_pages_sync_with_control(
                        &parsed,
                        prev_state,
                        &url_owned,
                        &comic_ctx,
                        &chapter_ctx,
                        READER_OP_TIMEOUT,
                        cancel_worker,
                    )
                    .map_err(String::from)
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
                return Err(DomainError::Storage(format!("获取漫画页面列表失败: {e}")));
            }
            Err(_) => {
                cancel_flag.store(true, Ordering::Relaxed);
                return Err(DomainError::Validation(format!(
                    "获取漫画页面列表超时（超过 {} 秒）",
                    READER_OP_TIMEOUT.as_secs()
                )));
            }
        };

        if out.state != prev_state_cmp
            && let Err(err) = self
                .source_crud
                .persist_runtime_state_with_base(source_id, &prev_state_cmp, out.state, false)
                .await
        {
            tracing::warn!(
                source_id = %source_id,
                error = %err,
                "failed to persist runtime state after fetch_pages"
            );
        }
        if !actor.role.is_admin() {
            out.diagnostics.logs.clear();
        }

        let pages = out
            .data
            .into_iter()
            .enumerate()
            .map(|(index, image_url)| OnlineComicPageDto {
                index,
                proxy_url: build_image_proxy_url(source_id, &image_url),
                image_url,
            })
            .collect();

        Ok(OnlineComicPagesResponseDto {
            source_id,
            source_name: rec.name,
            chapter_url: chapter_url.to_string(),
            reading_direction,
            pages,
            diagnostics: out.diagnostics,
        })
    }
}

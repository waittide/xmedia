use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use tokio::sync::Semaphore;
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::user::User;
use xmedia_source::crud::merge_diagnostics;
use xmedia_source::http::HttpClientConfig;
use xmedia_source::proxy::{
    DEFAULT_IMAGE_PROXY_TIMEOUT_SECS, fetch_validated_image_bytes_with_config,
};

use crate::source::model::{
    ComicChapterJsContext, ComicJsContext, ComicSearchResult, ComicSourceSpec,
};

use super::dto::{
    ComicSearchResultItem, OnlineComicChaptersResponseDto, OnlineComicDetailDto,
    OnlineComicDetailResponseDto, OnlineComicExploreResponseDto, OnlineComicPageDto,
    OnlineComicPagesResponseDto, OnlineComicSearchResponseDto, SourceSearchErrorDto,
};
use super::helpers::{
    CancelOnDrop, CancelTasksOnDrop, build_image_proxy_url, get_or_insert_rate_limiter,
};
use super::{
    COMIC_IMAGE_PROXY_MAX_BYTES, ComicOnlineService, READER_OP_TIMEOUT, SEARCH_MAX_CONCURRENCY,
    SEARCH_PER_SOURCE_TIMEOUT,
};

fn to_search_item(
    source_id: Uuid,
    source_name: String,
    comic: ComicSearchResult,
) -> ComicSearchResultItem {
    let proxy_cover_url = comic
        .cover_url
        .as_deref()
        .map(|u| build_image_proxy_url(source_id, u));
    ComicSearchResultItem {
        source_id,
        source_name,
        title: comic.title,
        author: comic.author,
        comic_url: comic.comic_url,
        cover_url: comic.cover_url,
        proxy_cover_url,
        intro: comic.intro,
        kind: comic.kind,
        last_chapter: comic.last_chapter,
        word_count: comic.word_count,
    }
}

impl ComicOnlineService {
    pub async fn search(
        &self,
        actor: &User,
        keyword: &str,
        source_ids: Option<Vec<Uuid>>,
        page: u32,
    ) -> DomainResult<OnlineComicSearchResponseDto> {
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
            return Err(DomainError::Validation("没有可用的已启用漫画源".into()));
        }

        sources.sort_by(|a, b| b.weight.cmp(&a.weight).then_with(|| a.name.cmp(&b.name)));
        let page_num = page.max(1);
        let sem = Arc::new(Semaphore::new(SEARCH_MAX_CONCURRENCY));

        let mut handles = Vec::with_capacity(sources.len());
        let mut cancel_all_guard = CancelTasksOnDrop::new();
        for rec in sources {
            let engine = self.engine.clone();
            let kw_owned = kw.to_string();
            let parsed_res = serde_json::from_str::<ComicSourceSpec>(&rec.raw_json);
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
                            Err(format!("漫画源解析失败: {e}")),
                        );
                    }
                };
                if let Some(rate) = parsed.parse_concurrent_rate()
                    && let Err(e) = rate_limiter.acquire(rate, SEARCH_PER_SOURCE_TIMEOUT).await
                {
                    return (sid, sname, prev_state_cmp, Err(e));
                }
                let Ok(_permit) = sem_clone.acquire_owned().await else {
                    return (sid, sname, prev_state_cmp, Err("搜索并发调度已关闭".into()));
                };
                let cancel_worker = cancel_for_task.clone();
                let mut cancel_guard = CancelOnDrop::new(cancel_for_task.clone());
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
                            Err(format!(
                                "搜索超时（超过 {} 秒）",
                                SEARCH_PER_SOURCE_TIMEOUT.as_secs()
                            )),
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
                            tracing::warn!(source_id = %sid, error = %err, "failed to persist comic source runtime state after search");
                        }
                        toasts.extend(out.diagnostics.toasts);
                        for comic in out.data {
                            results.push(to_search_item(sid, sname.clone(), comic));
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

        Ok(OnlineComicSearchResponseDto {
            keyword: kw.to_string(),
            page: page_num,
            results,
            errors,
            toasts,
        })
    }

    pub async fn explore(
        &self,
        actor: &User,
        source_id: Uuid,
        explore_url: Option<&str>,
        page: u32,
    ) -> DomainResult<OnlineComicExploreResponseDto> {
        let rec = self.load_source_for_actor(actor, source_id).await?;
        let parsed: ComicSourceSpec = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("漫画源 JSON 无效: {e}")))?;
        self.acquire_rate_slot(
            source_id,
            parsed.parse_concurrent_rate(),
            SEARCH_PER_SOURCE_TIMEOUT,
        )
        .await
        .map_err(DomainError::Validation)?;

        let page_num = page.max(1);
        let engine = self.engine.clone();
        let sname = rec.name.clone();
        let prev_state = rec.runtime_state.clone();
        let prev_state_cmp = prev_state.clone();
        let explicit_url = explore_url
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);

        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_worker = cancel_flag.clone();
        let mut cancel_guard = CancelOnDrop::new(cancel_flag.clone());
        let spec = parsed;
        let explicit_url_owned = explicit_url;
        let sname_c = sname.clone();

        let timed = tokio::time::timeout(
            SEARCH_PER_SOURCE_TIMEOUT,
            tokio::task::spawn_blocking(move || {
                let total_start = Instant::now();
                let remaining_budget = || {
                    SEARCH_PER_SOURCE_TIMEOUT
                        .saturating_sub(total_start.elapsed())
                        .max(Duration::from_millis(1))
                };
                let cat_out = engine
                    .parse_explore_categories_with_control(
                        &spec,
                        prev_state,
                        remaining_budget(),
                        cancel_worker.clone(),
                    )
                    .map_err(String::from)?;
                let categories = cat_out.data;
                let mut state = cat_out.state;
                let mut diag = cat_out.diagnostics;
                let target_url =
                    explicit_url_owned.or_else(|| categories.first().map(|c| c.url.clone()));

                let items = if let Some(ref url_to_fetch) = target_url {
                    let out = engine
                        .explore_sync_with_categories_and_control(
                            &spec,
                            state,
                            url_to_fetch,
                            page_num,
                            Some(&categories),
                            remaining_budget(),
                            cancel_worker,
                        )
                        .map_err(String::from)?;
                    state = out.state;
                    merge_diagnostics(&mut diag, &out.diagnostics);
                    out.data
                        .into_iter()
                        .map(|c| to_search_item(source_id, sname_c.clone(), c))
                        .collect()
                } else {
                    Vec::new()
                };

                Ok::<_, String>((target_url, categories, items, state, diag))
            }),
        )
        .await;

        let (target_url, categories, items, state, mut diag) = match timed {
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
                return Err(DomainError::Storage(format!("发现列表任务异常: {e}")));
            }
            Err(_) => {
                cancel_flag.store(true, Ordering::Relaxed);
                return Err(DomainError::Validation(format!(
                    "发现列表请求超时（超过 {} 秒）",
                    SEARCH_PER_SOURCE_TIMEOUT.as_secs()
                )));
            }
        };

        if state != prev_state_cmp
            && let Err(err) = self
                .persist_runtime_state_with_base(source_id, &prev_state_cmp, state, false)
                .await
        {
            tracing::warn!(source_id = %source_id, error = %err, "failed to persist runtime state after explore");
        }
        if !actor.role.is_admin() {
            diag.logs.clear();
        }

        Ok(OnlineComicExploreResponseDto {
            source_id,
            explore_url: target_url.unwrap_or_default(),
            page: page_num,
            items,
            categories,
            diagnostics: diag,
        })
    }

    pub async fn fetch_detail(
        &self,
        actor: &User,
        source_id: Uuid,
        comic_url: &str,
        fallback_hint: Option<ComicJsContext>,
    ) -> DomainResult<OnlineComicDetailResponseDto> {
        let rec = self.load_source_for_actor(actor, source_id).await?;
        let parsed: ComicSourceSpec = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("漫画源 JSON 无效: {e}")))?;
        let clean_comic_url =
            xmedia_source::http::resolve_literal_http_url(comic_url, Some(&parsed.source_url))?;
        if let Some(ref hint) = fallback_hint {
            if !hint.book_url.trim().is_empty() {
                xmedia_source::http::resolve_literal_http_url(
                    &hint.book_url,
                    Some(&parsed.source_url),
                )?;
            }
            if !hint.toc_url.trim().is_empty() {
                xmedia_source::http::resolve_literal_http_url(
                    &hint.toc_url,
                    Some(&parsed.source_url),
                )?;
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
                .persist_runtime_state_with_base(source_id, &prev_state_cmp, out.state, false)
                .await
        {
            tracing::warn!(source_id = %source_id, error = %err, "failed to persist runtime state after fetch_detail");
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
        let rec = self.load_source_for_actor(actor, source_id).await?;
        let parsed: ComicSourceSpec = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("漫画源 JSON 无效: {e}")))?;
        let clean_toc_url =
            xmedia_source::http::resolve_literal_http_url(toc_url, Some(&parsed.source_url))?;
        if !comic_ctx.book_url.trim().is_empty() {
            xmedia_source::http::resolve_literal_http_url(
                &comic_ctx.book_url,
                Some(&parsed.source_url),
            )?;
        }
        if !comic_ctx.toc_url.trim().is_empty() {
            xmedia_source::http::resolve_literal_http_url(
                &comic_ctx.toc_url,
                Some(&parsed.source_url),
            )?;
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
                .persist_runtime_state_with_base(source_id, &prev_state_cmp, out.state, false)
                .await
        {
            tracing::warn!(source_id = %source_id, error = %err, "failed to persist runtime state after fetch_chapters");
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
        let rec = self.load_source_for_actor(actor, source_id).await?;
        let parsed: ComicSourceSpec = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("漫画源 JSON 无效: {e}")))?;
        let clean_chapter_url =
            xmedia_source::http::resolve_literal_http_url(chapter_url, Some(&parsed.source_url))?;
        if !comic_ctx.book_url.trim().is_empty() {
            xmedia_source::http::resolve_literal_http_url(
                &comic_ctx.book_url,
                Some(&parsed.source_url),
            )?;
        }
        if !comic_ctx.toc_url.trim().is_empty() {
            xmedia_source::http::resolve_literal_http_url(
                &comic_ctx.toc_url,
                Some(&parsed.source_url),
            )?;
        }
        if !chapter_ctx.url.trim().is_empty() {
            xmedia_source::http::resolve_literal_http_url(
                &chapter_ctx.url,
                Some(&parsed.source_url),
            )?;
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
            .and_then(crate::source::model::parse_reading_direction_opt)
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
                .persist_runtime_state_with_base(source_id, &prev_state_cmp, out.state, false)
                .await
        {
            tracing::warn!(source_id = %source_id, error = %err, "failed to persist runtime state after fetch_pages");
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

    pub async fn fetch_image_proxy(
        &self,
        actor: &User,
        source_id: Option<Uuid>,
        raw_url: &str,
    ) -> DomainResult<(Vec<u8>, String)> {
        let custom_headers = if let Some(sid) = source_id {
            let rec = self.load_source_for_actor(actor, sid).await?;
            let parsed: ComicSourceSpec = serde_json::from_str(&rec.raw_json)
                .map_err(|e| DomainError::Validation(format!("漫画源 JSON 无效: {e}")))?;
            Some(parsed.merged_image_headers())
        } else {
            None
        };

        let cfg = HttpClientConfig {
            timeout_secs: DEFAULT_IMAGE_PROXY_TIMEOUT_SECS,
            allow_private_ips: self.allow_private_ips,
            proxy_url: None,
        };
        let (bytes, mime) = fetch_validated_image_bytes_with_config(
            raw_url,
            custom_headers.as_ref(),
            COMIC_IMAGE_PROXY_MAX_BYTES,
            &cfg,
        )
        .await?;
        Ok((bytes, mime.to_string()))
    }
}

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

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
use super::helpers::build_image_proxy_url;
use super::{
    COMIC_IMAGE_PROXY_MAX_BYTES, ComicOnlineService, SEARCH_MAX_CONCURRENCY,
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
        for rec in sources {
            let engine = self.engine.clone();
            let kw_owned = kw.to_string();
            let parsed_res = serde_json::from_str::<ComicSourceSpec>(&rec.raw_json);
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
                            Err(format!("漫画源解析失败: {e}")),
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

        let mut results = Vec::new();
        let mut errors = Vec::new();
        let mut toasts = Vec::new();

        for h in handles {
            if let Ok((sid, sname, prev_state, res)) = h.await {
                match res {
                    Ok(out) => {
                        if out.state != prev_state
                            && let Err(err) = self.persist_runtime_state(sid, out.state).await
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

        let page_num = page.max(1);
        let engine = self.engine.clone();
        let sname = rec.name.clone();
        let prev_state = rec.runtime_state.clone();
        let prev_state_cmp = prev_state.clone();
        let explicit_url = explore_url
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);

        let (categories, mut state, mut diag) = {
            let eng = engine.clone();
            let spec = parsed.clone();
            let st = prev_state;
            let out = tokio::task::spawn_blocking(move || {
                eng.parse_explore_categories(&spec, st)
                    .map_err(String::from)
            })
            .await
            .map_err(|e| DomainError::Storage(format!("解析发现分类失败: {e}")))?
            .map_err(DomainError::Validation)?;
            (out.data, out.state, out.diagnostics)
        };

        let target_url = explicit_url.or_else(|| categories.first().map(|c| c.url.clone()));

        let items = if let Some(ref url_to_fetch) = target_url {
            let cancel_flag = Arc::new(AtomicBool::new(false));
            let cancel_worker = cancel_flag.clone();
            let spec = parsed;
            let url_owned = url_to_fetch.clone();
            let timed = tokio::time::timeout(
                SEARCH_PER_SOURCE_TIMEOUT,
                tokio::task::spawn_blocking(move || {
                    engine
                        .explore_sync_with_control(
                            &spec,
                            state,
                            &url_owned,
                            page_num,
                            SEARCH_PER_SOURCE_TIMEOUT,
                            cancel_worker,
                        )
                        .map_err(String::from)
                }),
            )
            .await;

            match timed {
                Ok(Ok(Ok(out))) => {
                    state = out.state;
                    merge_diagnostics(&mut diag, &out.diagnostics);
                    out.data
                        .into_iter()
                        .map(|c| to_search_item(source_id, sname.clone(), c))
                        .collect()
                }
                Ok(Ok(Err(err))) => return Err(DomainError::Validation(err)),
                Ok(Err(e)) => {
                    return Err(DomainError::Storage(format!("发现列表任务异常: {e}")));
                }
                Err(_) => {
                    cancel_flag.store(true, Ordering::Relaxed);
                    return Err(DomainError::Validation(format!(
                        "发现列表请求超时（超过 {} 秒）",
                        SEARCH_PER_SOURCE_TIMEOUT.as_secs()
                    )));
                }
            }
        } else {
            Vec::new()
        };

        if state != prev_state_cmp
            && let Err(err) = self.persist_runtime_state(source_id, state).await
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

        let engine = self.engine.clone();
        let url_owned = comic_url.to_string();
        let prev_state = rec.runtime_state.clone();
        let prev_state_cmp = prev_state.clone();
        let parsed_for_detail = parsed.clone();

        let mut out = tokio::task::spawn_blocking(move || {
            engine.fetch_detail_sync(
                &parsed_for_detail,
                prev_state,
                &url_owned,
                fallback_hint.as_ref(),
            )
        })
        .await
        .map_err(|e| DomainError::Storage(format!("获取漫画详情失败: {e}")))?
        .map_err(DomainError::Validation)?;

        let mut chapters = Vec::new();
        let has_toc_rule = parsed
            .rule_toc
            .as_ref()
            .and_then(|t| t.chapter_list.as_deref())
            .is_some_and(|s| !s.trim().is_empty());

        if has_toc_rule && !out.data.toc_url.trim().is_empty() {
            let toc_url = out.data.toc_url.clone();
            let comic_ctx = ComicJsContext {
                name: out.data.title.clone(),
                author: out.data.author.clone(),
                book_url: out.data.comic_url.clone(),
                toc_url: toc_url.clone(),
                cover_url: out.data.cover_url.clone(),
                intro: out.data.intro.clone(),
                reading_direction: out.data.reading_direction.map(|d| d.as_str().to_string()),
                dur_chapter_index: 0,
                dur_chapter_title: String::new(),
                custom_variable: String::new(),
            };
            let engine_toc = self.engine.clone();
            let parsed_toc = parsed.clone();
            let st_toc = out.state.clone();
            match tokio::task::spawn_blocking(move || {
                engine_toc.fetch_chapters_sync(&parsed_toc, st_toc, &toc_url, &comic_ctx)
            })
            .await
            {
                Ok(Ok(ch_out)) => {
                    out.state = ch_out.state;
                    merge_diagnostics(&mut out.diagnostics, &ch_out.diagnostics);
                    chapters = ch_out.data;
                }
                Ok(Err(err)) => {
                    out.diagnostics
                        .logs
                        .push(format!("fetch_detail chapters skipped: {err}"));
                }
                Err(e) => {
                    out.diagnostics
                        .logs
                        .push(format!("fetch_detail chapters task error: {e}"));
                }
            }
        }

        if out.state != prev_state_cmp
            && let Err(err) = self.persist_runtime_state(source_id, out.state).await
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

        let engine = self.engine.clone();
        let url_owned = toc_url.to_string();
        let prev_state = rec.runtime_state.clone();
        let prev_state_cmp = prev_state.clone();

        let mut out = tokio::task::spawn_blocking(move || {
            engine.fetch_chapters_sync(&parsed, prev_state, &url_owned, &comic_ctx)
        })
        .await
        .map_err(|e| DomainError::Storage(format!("获取漫画章节列表失败: {e}")))?
        .map_err(DomainError::Validation)?;

        if out.state != prev_state_cmp
            && let Err(err) = self.persist_runtime_state(source_id, out.state).await
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

        let reading_direction = comic_ctx
            .reading_direction
            .as_deref()
            .and_then(crate::source::model::parse_reading_direction_opt)
            .unwrap_or_else(|| parsed.effective_reading_direction());

        let engine = self.engine.clone();
        let url_owned = chapter_url.to_string();
        let prev_state = rec.runtime_state.clone();
        let prev_state_cmp = prev_state.clone();

        let mut out = tokio::task::spawn_blocking(move || {
            engine.fetch_pages_sync(&parsed, prev_state, &url_owned, &comic_ctx, &chapter_ctx)
        })
        .await
        .map_err(|e| DomainError::Storage(format!("获取漫画页面列表失败: {e}")))?
        .map_err(DomainError::Validation)?;

        if out.state != prev_state_cmp
            && let Err(err) = self.persist_runtime_state(source_id, out.state).await
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

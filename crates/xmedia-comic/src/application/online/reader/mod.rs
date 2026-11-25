mod content;

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

use crate::domain::online::{ComicSearchResult, ComicSourceSpec};

use super::dto::{
    ComicSearchResultItem, OnlineComicExploreResponseDto, OnlineComicSearchResponseDto,
    SourceSearchErrorDto,
};
use super::helpers::{
    CancelOnDrop, CancelTasksOnDrop, build_image_proxy_url, get_or_insert_rate_limiter,
};
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

        let mut sources = self.source_crud.repository().list_all().await?;
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
                                .source_crud
                                .persist_runtime_state_with_base(sid, &prev_state, out.state, false)
                                .await
                        {
                            tracing::warn!(
                                source_id = %sid,
                                error = %err,
                                "failed to persist comic source runtime state after search"
                            );
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
        let rec = self
            .source_crud
            .load_source_for_actor(actor, source_id)
            .await?;
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
        let sname_c = sname;

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
                .source_crud
                .persist_runtime_state_with_base(source_id, &prev_state_cmp, state, false)
                .await
        {
            tracing::warn!(
                source_id = %source_id,
                error = %err,
                "failed to persist runtime state after explore"
            );
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

    pub async fn fetch_image_proxy(
        &self,
        actor: &User,
        source_id: Option<Uuid>,
        raw_url: &str,
    ) -> DomainResult<(Vec<u8>, String)> {
        let custom_headers = if let Some(sid) = source_id {
            let rec = self.source_crud.load_source_for_actor(actor, sid).await?;
            let parsed: ComicSourceSpec = serde_json::from_str(&rec.raw_json)
                .map_err(|e| DomainError::Validation(format!("漫画源 JSON 无效: {e}")))?;
            let mut headers = parsed.merged_image_headers_for_target(raw_url);
            headers.retain(|k, _| !k.eq_ignore_ascii_case("range"));
            Some(headers)
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

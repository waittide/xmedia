use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::Semaphore;
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::user::User;
use xmedia_source::rate_limit::get_or_insert_rate_limiter;

use crate::domain::online::{
    MusicSourceSpec, OnlineMusicTrackItem, is_http_url, url_encode_component,
};

use super::dto::{
    MusicExploreCategoryDto, OnlineMusicExploreResponseDto, OnlineMusicSearchResponseDto,
    OnlineMusicTrackDto, SourceSearchErrorDto,
};
use super::{CancelOnDrop, CancelTasksOnDrop, MusicOnlineService, SEARCH_MAX_CONCURRENCY};

pub(super) fn build_proxy_cover_url(source_id: Uuid, cover_url: &str) -> Option<String> {
    let trimmed = cover_url.trim();
    if is_http_url(trimmed) {
        Some(format!(
            "/api/v1/music/online/cover-proxy?url={}&source_id={source_id}",
            url_encode_component(trimmed)
        ))
    } else {
        None
    }
}

pub(super) fn build_proxy_cover_url_by_id(
    source_id: Uuid,
    cover_id: &str,
    track_id: &str,
) -> Option<String> {
    let cid = cover_id.trim();
    let tid = track_id.trim();
    if cid.is_empty() && tid.is_empty() {
        return None;
    }
    let mut qs = format!("source_id={source_id}");
    if !cid.is_empty() {
        qs.push_str("&cover_id=");
        qs.push_str(&url_encode_component(cid));
    }
    if !tid.is_empty() && tid != cid {
        qs.push_str("&track_id=");
        qs.push_str(&url_encode_component(tid));
    }
    Some(format!("/api/v1/music/online/cover-proxy?{qs}"))
}

pub(super) fn build_proxy_stream_url(source_id: Uuid, stream_url: &str) -> String {
    format!(
        "/api/v1/music/online/audio-proxy?url={}&source_id={source_id}",
        url_encode_component(stream_url.trim())
    )
}

pub(super) fn track_item_to_dto(
    source_id: Uuid,
    source_name: &str,
    qualities: &[String],
    has_cover_rule: bool,
    item: OnlineMusicTrackItem,
) -> OnlineMusicTrackDto {
    let proxy_cover_url = item
        .cover_url
        .as_deref()
        .and_then(|u| build_proxy_cover_url(source_id, u))
        .or_else(|| {
            if has_cover_rule {
                item.cover_id
                    .as_deref()
                    .and_then(|cid| build_proxy_cover_url_by_id(source_id, cid, &item.track_id))
            } else {
                None
            }
        });
    OnlineMusicTrackDto {
        source_id,
        source_name: source_name.to_string(),
        track_id: item.track_id,
        title: item.title,
        artist: item.artist,
        album: item.album,
        duration_secs: item.duration_secs,
        cover_url: item.cover_url,
        proxy_cover_url,
        detail_url: item.detail_url,
        play_url: item.play_url,
        url_id: item.url_id,
        lyric_id: item.lyric_id,
        cover_id: item.cover_id,
        qualities: qualities.to_vec(),
        format: item.format,
    }
}

impl MusicOnlineService {
    pub async fn search(
        &self,
        actor: &User,
        keyword: &str,
        source_ids: Option<Vec<Uuid>>,
        page: u32,
        limit: u32,
    ) -> DomainResult<OnlineMusicSearchResponseDto> {
        let kw = keyword.trim();
        if kw.is_empty() {
            return Err(DomainError::Validation("请输入搜索关键词".into()));
        }

        let mut sources = self.sources.repository().list_all().await?;
        let is_admin = actor.role.is_admin();

        if let Some(filter_ids) = source_ids.filter(|v| !v.is_empty()) {
            sources.retain(|s| filter_ids.contains(&s.id) && (is_admin || s.enabled));
        } else {
            sources.retain(|s| s.enabled);
        }

        if sources.is_empty() {
            return Err(DomainError::Validation("没有可用的已启用音源".into()));
        }

        sources.sort_by(|a, b| b.weight.cmp(&a.weight).then_with(|| a.name.cmp(&b.name)));
        let page_num = page.max(1);
        let limit_num = limit.clamp(1, 50);
        let sem = Arc::new(Semaphore::new(SEARCH_MAX_CONCURRENCY));
        let op_timeout = self.op_timeout;

        let mut handles = Vec::with_capacity(sources.len());
        let mut cancel_all_guard = CancelTasksOnDrop::new();
        for rec in sources {
            let engine = self.engine.clone();
            let kw_owned = kw.to_string();
            let parsed_res = serde_json::from_str::<MusicSourceSpec>(&rec.raw_json);
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
                            Vec::new(),
                            false,
                            prev_state_cmp,
                            Err(format!("音源解析失败: {e}")),
                        );
                    }
                };
                let qualities = parsed.supported_qualities.clone();
                let has_cover_rule = parsed.rule_cover.is_some();
                if let Some(rate) = parsed.parse_concurrent_rate()
                    && let Err(e) = rate_limiter.acquire(rate, op_timeout).await
                {
                    return (
                        sid,
                        sname,
                        qualities,
                        has_cover_rule,
                        prev_state_cmp,
                        Err(e),
                    );
                }
                let Ok(_permit) = sem_clone.acquire_owned().await else {
                    return (
                        sid,
                        sname,
                        qualities,
                        has_cover_rule,
                        prev_state_cmp,
                        Err("搜索并发调度已关闭".into()),
                    );
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
                                limit_num,
                                op_timeout,
                                cancel_worker,
                            )
                            .map_err(|e| e.message)
                    }),
                )
                .await;
                match timed {
                    Ok(Ok(res)) => {
                        cancel_guard.disarm();
                        (sid, sname, qualities, has_cover_rule, prev_state_cmp, res)
                    }
                    Ok(Err(e)) => {
                        cancel_guard.disarm();
                        (
                            sid,
                            sname,
                            qualities,
                            has_cover_rule,
                            prev_state_cmp,
                            Err(format!("搜索任务异常: {e}")),
                        )
                    }
                    Err(_) => {
                        cancel_for_task.store(true, Ordering::Relaxed);
                        (
                            sid,
                            sname,
                            qualities,
                            has_cover_rule,
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
            let Ok((sid, sname, qualities, has_cover_rule, prev_state, res)) = h.await else {
                continue;
            };
            match res {
                Ok(out) => {
                    if out.state != prev_state {
                        let _ = self
                            .sources
                            .persist_runtime_state_with_base(sid, &prev_state, out.state, false)
                            .await;
                    }
                    for t in out.diagnostics.toasts {
                        if !toasts.contains(&t) {
                            toasts.push(t);
                        }
                    }
                    for item in out.data {
                        results.push(track_item_to_dto(
                            sid,
                            &sname,
                            &qualities,
                            has_cover_rule,
                            item,
                        ));
                    }
                }
                Err(message) => {
                    errors.push(SourceSearchErrorDto {
                        source_id: sid,
                        source_name: sname,
                        message,
                    });
                }
            }
        }
        cancel_all_guard.disarm();

        Ok(OnlineMusicSearchResponseDto {
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
        limit: u32,
    ) -> DomainResult<OnlineMusicExploreResponseDto> {
        let rec = self.sources.load_source_for_actor(actor, source_id).await?;
        let parsed: MusicSourceSpec = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("音源 JSON 无效: {e}")))?;

        let op_timeout = self.op_timeout;
        self.acquire_rate_slot(source_id, parsed.parse_concurrent_rate(), op_timeout)
            .await
            .map_err(DomainError::Validation)?;

        let qualities = parsed.supported_qualities.clone();
        let has_cover_rule = parsed.rule_cover.is_some();
        let engine = self.engine.clone();
        let prev_state = rec.runtime_state.clone();
        let prev_state_cmp = prev_state.clone();
        let explore_owned = explore_url.map(ToString::to_string);
        let page_num = page.max(1);
        let limit_num = limit.clamp(1, 50);

        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_worker = cancel_flag.clone();
        let mut cancel_guard = CancelOnDrop::new(cancel_flag.clone());

        let timed = tokio::time::timeout(
            op_timeout,
            tokio::task::spawn_blocking(move || {
                engine
                    .explore_sync_with_control(
                        &parsed,
                        prev_state,
                        explore_owned.as_deref(),
                        page_num,
                        limit_num,
                        op_timeout,
                        cancel_worker,
                    )
                    .map_err(|e| e.message)
            }),
        )
        .await;

        let out = match timed {
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
                return Err(DomainError::Storage(format!("浏览音源线程失败: {e}")));
            }
            Err(_) => {
                cancel_flag.store(true, Ordering::Relaxed);
                return Err(DomainError::Validation(format!(
                    "浏览音源超时（超过 {} 秒）",
                    op_timeout.as_secs()
                )));
            }
        };

        if out.state != prev_state_cmp {
            let _ = self
                .sources
                .persist_runtime_state_with_base(source_id, &prev_state_cmp, out.state, false)
                .await;
        }

        let mut diagnostics = out.diagnostics;
        if !actor.role.is_admin() {
            diagnostics.logs.clear();
        }

        let results = out
            .data
            .tracks
            .into_iter()
            .map(|item| track_item_to_dto(source_id, &rec.name, &qualities, has_cover_rule, item))
            .collect();
        let categories = out
            .data
            .categories
            .into_iter()
            .map(|c| MusicExploreCategoryDto {
                title: c.title,
                url: c.url,
            })
            .collect();

        Ok(OnlineMusicExploreResponseDto {
            source_id,
            results,
            categories,
            diagnostics,
        })
    }
}

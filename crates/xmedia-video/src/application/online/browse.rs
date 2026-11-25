use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use reqwest::header::{ACCEPT_ENCODING, CONTENT_TYPE, HeaderValue, RANGE, USER_AGENT};
use tokio::sync::Semaphore;
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::user::User;
use xmedia_source::http::{
    DEFAULT_WEBVIEW_UA, MAX_REDIRECT_HOPS, build_sanitized_headers, decode_response_bytes,
    get_or_build_async_client, normalize_cookie_key,
};
use xmedia_source::proxy::{
    DEFAULT_IMAGE_PROXY_MAX_BYTES, fetch_validated_image_bytes_with_config,
};
use xmedia_source::rate_limit::get_or_insert_rate_limiter;

use crate::source::hls::{
    MAX_M3U8_BYTES, MAX_SUBTITLE_BYTES, SNIFF_PREFIX_MAX_BYTES, build_hls_proxy_url,
    build_poster_proxy_url, build_stream_proxy_url, build_subtitle_proxy_url,
    infer_segment_content_type, is_forbidden_media_ct, is_forbidden_subtitle_ct,
    is_hls_playlist_response, is_partial_hls_playlist_range_candidate, looks_like_html_bytes,
    read_sniff_prefix_with_context, send_streaming_get, spawn_chunk_stream,
    validate_and_normalize_subtitle_to_vtt, validate_streaming_target_url,
};
use crate::source::model::VideoSourceSpec;

use super::dto::{
    HlsProxyOutcome, OnlineVideoBrowseResponseDto, OnlineVideoCategoriesResponseDto,
    OnlineVideoDetailResponseDto, OnlineVideoPlayResponseDto, OnlineVideoSearchErrorDto,
    OnlineVideoSearchItemDto, OnlineVideoSearchResponseDto, ProxyStreamOutcome, ProxyStreamPayload,
};
use super::{CancelOnDrop, CancelTasksOnDrop, SEARCH_MAX_CONCURRENCY, VideoOnlineService};

fn validate_range_header(raw: Option<&str>) -> DomainResult<Option<String>> {
    let Some(val) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    if !val.to_ascii_lowercase().starts_with("bytes=") || val.bytes().any(|b| b.is_ascii_control())
    {
        return Err(DomainError::Validation("无效的 Range 请求头".into()));
    }
    Ok(Some(val.to_string()))
}

pub(super) fn lookup_cookie_for_url(
    cookies: &HashMap<String, String>,
    target_url: &str,
) -> Option<String> {
    let key = normalize_cookie_key(target_url);
    if key.is_empty() {
        return None;
    }
    if let Some(v) = cookies.get(&key)
        && !v.trim().is_empty()
    {
        return Some(v.clone());
    }
    let mut best: Option<(&str, &str)> = None;
    for (stored_host, val) in cookies {
        let clean_stored = stored_host.trim_start_matches('.');
        if clean_stored.is_empty() || val.trim().is_empty() {
            continue;
        }
        if key.len() > clean_stored.len()
            && key.ends_with(clean_stored)
            && key.as_bytes()[key.len() - clean_stored.len() - 1] == b'.'
        {
            let replace = match best {
                None => true,
                Some((prev_host, _)) => {
                    clean_stored.len() > prev_host.len()
                        || (clean_stored.len() == prev_host.len() && clean_stored < prev_host)
                }
            };
            if replace {
                best = Some((clean_stored, val.as_str()));
            }
        }
    }
    best.map(|(_, v)| v.to_string())
}

impl VideoOnlineService {
    async fn resolve_source_media_headers(
        &self,
        actor: &User,
        source_id: Option<Uuid>,
        target_url: &str,
    ) -> DomainResult<HashMap<String, String>> {
        let Some(sid) = source_id else {
            return Ok(HashMap::new());
        };
        let rec = self.load_source_for_actor(actor, sid).await?;
        let spec: VideoSourceSpec = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("影视源 JSON 无效: {e}")))?;
        let mut merged = spec.parse_media_header_map_for_target(target_url);
        // Never allow static source config to override client Range requests or truncate playlists/streams.
        merged.retain(|k, _| !k.eq_ignore_ascii_case("range"));
        Ok(merged)
    }

    async fn resolve_source_media_headers_with_cookies(
        &self,
        actor: &User,
        source_id: Option<Uuid>,
        target_url: &str,
    ) -> DomainResult<HashMap<String, String>> {
        let Some(sid) = source_id else {
            return Ok(HashMap::new());
        };
        let rec = self.load_source_for_actor(actor, sid).await?;
        let spec: VideoSourceSpec = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("影视源 JSON 无效: {e}")))?;
        let mut merged = spec.parse_media_header_map_for_target(target_url);
        merged.retain(|k, _| !k.eq_ignore_ascii_case("range"));
        if !merged.keys().any(|k| k.eq_ignore_ascii_case("cookie"))
            && spec.enabled_cookie_jar
            && let Some(cookie) = lookup_cookie_for_url(&rec.runtime_state.cookies, target_url)
        {
            merged.insert("Cookie".to_string(), cookie);
        }
        Ok(merged)
    }

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
            crate::source::model::VideoSourceFormat::ApplecmsV10 => true,
            crate::source::model::VideoSourceFormat::Rule => {
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
    ) -> DomainResult<OnlineVideoSearchResponseDto> {
        let kw = keyword.trim();
        if kw.is_empty() {
            return Err(DomainError::Validation("请输入搜索关键词".into()));
        }

        let mut sources = self.sources.list_all().await?;
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
                                    Some(build_poster_proxy_url(cover, Some(sid)));
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
                item.proxy_cover_url = Some(build_poster_proxy_url(cover, Some(source_id)));
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

        if let Some(ref cover) = out.data.cover_url
            && !cover.trim().is_empty()
        {
            out.data.proxy_cover_url = Some(build_poster_proxy_url(cover, Some(source_id)));
        }

        Ok(OnlineVideoDetailResponseDto {
            source_id,
            source_name: rec.name,
            detail: out.data,
            diagnostics: out.diagnostics,
        })
    }

    pub async fn play(
        &self,
        actor: &User,
        source_id: Uuid,
        play_url: &str,
        group_name: Option<&str>,
    ) -> DomainResult<OnlineVideoPlayResponseDto> {
        let rec = self.load_source_for_actor(actor, source_id).await?;
        let parsed: VideoSourceSpec = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("影视源 JSON 无效: {e}")))?;
        let op_timeout = self.op_timeout;
        self.acquire_rate_slot(source_id, parsed.parse_concurrent_rate(), op_timeout)
            .await
            .map_err(DomainError::Validation)?;
        let clean_play_url = xmedia_source::http::resolve_literal_http_url(
            play_url,
            Some(parsed.effective_source_url()),
        )?;

        let engine = self.engine.clone();
        let prev_state = rec.runtime_state.clone();
        let prev_state_cmp = prev_state.clone();
        let play_owned = clean_play_url;
        let group_owned = group_name.map(ToString::to_string);
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_worker = cancel_flag.clone();
        let mut cancel_guard = CancelOnDrop::new(cancel_flag.clone());

        let timed = tokio::time::timeout(
            op_timeout,
            tokio::task::spawn_blocking(move || {
                engine
                    .resolve_play_sync_with_control(
                        &parsed,
                        prev_state,
                        &play_owned,
                        group_owned.as_deref(),
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
                return Err(DomainError::Storage(format!("解析播放地址失败: {e}")));
            }
            Err(_) => {
                cancel_flag.store(true, Ordering::Relaxed);
                return Err(DomainError::Validation(format!(
                    "解析播放地址超时（超过 {} 秒）",
                    op_timeout.as_secs()
                )));
            }
        };

        if out.state != prev_state_cmp
            && let Err(err) = self
                .persist_runtime_state_with_base(source_id, &prev_state_cmp, out.state, false)
                .await
        {
            tracing::warn!(source_id = %source_id, error = %err, "failed to persist runtime state after play");
        }
        if !actor.role.is_admin() {
            out.diagnostics.logs.clear();
        }

        out.data.proxy_stream_url = if out.data.stream_type == "hls" {
            build_hls_proxy_url(&out.data.stream_url, Some(source_id), None)
        } else {
            build_stream_proxy_url(&out.data.stream_url, Some(source_id), None)
        };
        for sub in &mut out.data.subtitles {
            let clean = sub.url.trim();
            if clean.starts_with("http://") || clean.starts_with("https://") {
                sub.proxy_url = Some(build_subtitle_proxy_url(clean, Some(source_id), None));
            }
        }

        Ok(OnlineVideoPlayResponseDto {
            source_id,
            source_name: rec.name,
            play: out.data,
            diagnostics: out.diagnostics,
        })
    }

    pub async fn fetch_poster_proxy(
        &self,
        actor: &User,
        raw_url: &str,
        source_id: Option<Uuid>,
    ) -> DomainResult<(Vec<u8>, &'static str)> {
        let headers = self
            .resolve_source_media_headers(actor, source_id, raw_url)
            .await?;
        fetch_validated_image_bytes_with_config(
            raw_url,
            (!headers.is_empty()).then_some(&headers),
            DEFAULT_IMAGE_PROXY_MAX_BYTES,
            &self.http_config,
        )
        .await
    }

    pub async fn fetch_subtitle_proxy(
        &self,
        actor: &User,
        raw_url: &str,
        source_id: Option<Uuid>,
    ) -> DomainResult<String> {
        let validated =
            validate_streaming_target_url(raw_url.trim(), self.allow_private_ips).await?;
        let source_headers = self
            .resolve_source_media_headers_with_cookies(actor, source_id, validated.as_str())
            .await?;

        let mut header_map = build_sanitized_headers(&source_headers);
        if !header_map.contains_key(USER_AGENT) {
            header_map.insert(USER_AGENT, HeaderValue::from_static(DEFAULT_WEBVIEW_UA));
        }

        let client = get_or_build_async_client(
            15,
            self.allow_private_ips,
            self.http_config.proxy_url.as_deref(),
        )
        .map_err(DomainError::Storage)?;

        let _ = MAX_REDIRECT_HOPS;
        let mut resp = client
            .get(validated)
            .headers(header_map)
            .send()
            .await
            .map_err(|e| DomainError::Validation(format!("拉取字幕失败: {e}")))?;

        if !resp.status().is_success() {
            return Err(DomainError::Validation(format!(
                "字幕响应状态异常: {}",
                resp.status()
            )));
        }

        let header_ct = resp
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(ToString::to_string);
        if let Some(ref ct) = header_ct
            && is_forbidden_subtitle_ct(&ct.to_ascii_lowercase())
        {
            return Err(DomainError::Validation(format!(
                "拒绝转发不安全的字幕内容类型: {ct}"
            )));
        }
        if let Some(len) = resp.content_length()
            && len > MAX_SUBTITLE_BYTES as u64
        {
            return Err(DomainError::Validation(format!(
                "字幕文件体积超过 {} 字节限制",
                MAX_SUBTITLE_BYTES
            )));
        }

        let mut buf = Vec::new();
        while let Some(chunk) = resp
            .chunk()
            .await
            .map_err(|e| DomainError::Validation(format!("读取字幕失败: {e}")))?
        {
            if buf.len().saturating_add(chunk.len()) > MAX_SUBTITLE_BYTES {
                return Err(DomainError::Validation(format!(
                    "字幕文件体积超过 {} 字节限制",
                    MAX_SUBTITLE_BYTES
                )));
            }
            buf.extend_from_slice(&chunk);
        }

        validate_and_normalize_subtitle_to_vtt(&buf, header_ct.as_deref())
    }

    pub async fn open_stream_proxy(
        &self,
        actor: &User,
        raw_url: &str,
        source_id: Option<Uuid>,
        range_header: Option<&str>,
    ) -> DomainResult<ProxyStreamOutcome> {
        let source_headers = self
            .resolve_source_media_headers(actor, source_id, raw_url)
            .await?;
        self.open_stream_proxy_with_headers(raw_url, source_headers, range_header)
            .await
    }

    pub async fn open_stream_proxy_for_tracked(
        &self,
        raw_url: &str,
        source_id: Option<Uuid>,
        range_header: Option<&str>,
    ) -> DomainResult<ProxyStreamOutcome> {
        let source_headers = self
            .resolve_internal_media_headers(source_id, raw_url)
            .await;
        self.open_stream_proxy_with_headers(raw_url, source_headers, range_header)
            .await
    }

    pub(super) async fn resolve_internal_media_headers(
        &self,
        source_id: Option<Uuid>,
        target_url: &str,
    ) -> HashMap<String, String> {
        let Some(sid) = source_id else {
            return HashMap::new();
        };
        let Ok(rec) = self.load_source(sid).await else {
            return HashMap::new();
        };
        let Ok(spec) = serde_json::from_str::<VideoSourceSpec>(&rec.raw_json) else {
            return HashMap::new();
        };
        let mut merged = spec.parse_media_header_map_for_target(target_url);
        merged.retain(|k, _| !k.eq_ignore_ascii_case("range"));
        if !merged.keys().any(|k| k.eq_ignore_ascii_case("cookie"))
            && spec.enabled_cookie_jar
            && let Some(cookie) = lookup_cookie_for_url(&rec.runtime_state.cookies, target_url)
        {
            merged.insert("Cookie".to_string(), cookie);
        }
        merged
    }

    async fn open_stream_proxy_with_headers(
        &self,
        raw_url: &str,
        source_headers: HashMap<String, String>,
        range_header: Option<&str>,
    ) -> DomainResult<ProxyStreamOutcome> {
        let sanitized_range = validate_range_header(range_header)?;

        let mut header_map = build_sanitized_headers(&source_headers);
        if !header_map.contains_key(USER_AGENT) {
            header_map.insert(USER_AGENT, HeaderValue::from_static(DEFAULT_WEBVIEW_UA));
        }
        header_map.insert(ACCEPT_ENCODING, HeaderValue::from_static("identity"));
        if let Some(ref rng) = sanitized_range
            && let Ok(hv) = HeaderValue::from_str(rng)
        {
            header_map.insert(RANGE, hv);
        }

        let mut resp = send_streaming_get(
            raw_url.trim(),
            header_map,
            self.allow_private_ips,
            self.http_config.proxy_url.as_deref(),
            "拉取视频流失败",
        )
        .await?;

        let status = resp.status();
        if status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
            let content_range = resp
                .headers()
                .get(reqwest::header::CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .map(ToString::to_string);
            return Ok(ProxyStreamOutcome::RangeNotSatisfiable { content_range });
        }
        if !status.is_success() {
            return Err(DomainError::Validation(format!(
                "视频流响应状态异常: {status}"
            )));
        }

        let header_ct = resp
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(ToString::to_string);
        if let Some(ref ct) = header_ct
            && is_forbidden_media_ct(&ct.to_ascii_lowercase())
        {
            return Err(DomainError::Validation(format!(
                "拒绝转发不安全的媒体内容类型: {ct}"
            )));
        }

        let content_type = infer_segment_content_type(header_ct.as_deref(), resp.url());
        let content_length = resp.content_length();
        let content_range = resp
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .map(ToString::to_string);
        let accept_ranges = resp
            .headers()
            .get(reqwest::header::ACCEPT_RANGES)
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("bytes")
            .to_string();

        let final_url = resp.url().clone();
        let ct_lower = header_ct
            .as_deref()
            .map(|s| s.split(';').next().unwrap_or(s).trim().to_ascii_lowercase())
            .unwrap_or_default();
        let prefix = read_sniff_prefix_with_context(
            &mut resp,
            SNIFF_PREFIX_MAX_BYTES,
            status == reqwest::StatusCode::PARTIAL_CONTENT,
            &ct_lower,
            &final_url,
        )
        .await?;
        if looks_like_html_bytes(&prefix) {
            return Err(DomainError::Validation(
                "拒绝转发伪装为媒体的 HTML/SVG 内容".into(),
            ));
        }
        if is_hls_playlist_response(&ct_lower, &final_url, &prefix)
            || (status == reqwest::StatusCode::PARTIAL_CONTENT
                && is_partial_hls_playlist_range_candidate(&ct_lower, &final_url, &prefix))
        {
            return Err(DomainError::Validation(
                "拒绝通过直链媒体代理转发 HLS 播放列表，请改用 hls-proxy".into(),
            ));
        }
        let initial = (!prefix.is_empty()).then_some(prefix);

        Ok(ProxyStreamOutcome::Stream(ProxyStreamPayload {
            status: status.as_u16(),
            content_type,
            content_length,
            content_range,
            accept_ranges,
            stream: spawn_chunk_stream(resp, initial),
        }))
    }

    pub async fn open_hls_proxy(
        &self,
        actor: &User,
        raw_url: &str,
        source_id: Option<Uuid>,
        range_header: Option<&str>,
        token: Option<&str>,
    ) -> DomainResult<HlsProxyOutcome> {
        let source_headers = self
            .resolve_source_media_headers(actor, source_id, raw_url)
            .await?;
        self.open_hls_proxy_with_rewriter(raw_url, source_headers, range_header, |resolved| {
            build_hls_proxy_url(resolved, source_id, token)
        })
        .await
    }

    pub async fn open_hls_proxy_for_tracked<F>(
        &self,
        actor: &User,
        raw_url: &str,
        source_id: Option<Uuid>,
        range_header: Option<&str>,
        rewrite_uri: F,
    ) -> DomainResult<HlsProxyOutcome>
    where
        F: FnMut(&str) -> String,
    {
        let source_headers = self
            .resolve_tracked_actor_media_headers(actor, source_id, raw_url)
            .await?;
        self.open_hls_proxy_with_rewriter(raw_url, source_headers, range_header, rewrite_uri)
            .await
    }

    async fn resolve_tracked_actor_media_headers(
        &self,
        actor: &User,
        source_id: Option<Uuid>,
        target_url: &str,
    ) -> DomainResult<HashMap<String, String>> {
        let Some(sid) = source_id else {
            return Ok(HashMap::new());
        };
        let rec = self.load_source_for_actor(actor, sid).await?;
        let Ok(spec) = serde_json::from_str::<VideoSourceSpec>(&rec.raw_json) else {
            return Ok(HashMap::new());
        };
        let mut merged = spec.parse_media_header_map_for_target(target_url);
        merged.retain(|k, _| !k.eq_ignore_ascii_case("range"));
        if !merged.keys().any(|k| k.eq_ignore_ascii_case("cookie"))
            && spec.enabled_cookie_jar
            && let Some(cookie) = lookup_cookie_for_url(&rec.runtime_state.cookies, target_url)
        {
            merged.insert("Cookie".to_string(), cookie);
        }
        Ok(merged)
    }

    async fn open_hls_proxy_with_rewriter<F>(
        &self,
        raw_url: &str,
        source_headers: HashMap<String, String>,
        range_header: Option<&str>,
        rewrite_uri: F,
    ) -> DomainResult<HlsProxyOutcome>
    where
        F: FnMut(&str) -> String,
    {
        let sanitized_range = validate_range_header(range_header)?;

        let mut header_map = build_sanitized_headers(&source_headers);
        if !header_map.contains_key(USER_AGENT) {
            header_map.insert(USER_AGENT, HeaderValue::from_static(DEFAULT_WEBVIEW_UA));
        }
        header_map.insert(ACCEPT_ENCODING, HeaderValue::from_static("identity"));

        let mut include_range = sanitized_range.is_some();
        let (
            mut resp,
            status,
            header_ct,
            ct_lower,
            final_url,
            content_length,
            content_range,
            accept_ranges,
            prefix,
        ) = loop {
            let mut req_headers = header_map.clone();
            if include_range
                && let Some(ref rng) = sanitized_range
                && let Ok(hv) = HeaderValue::from_str(rng)
            {
                req_headers.insert(RANGE, hv);
            }

            let mut resp = send_streaming_get(
                raw_url.trim(),
                req_headers,
                self.allow_private_ips,
                self.http_config.proxy_url.as_deref(),
                "拉取 HLS 资源失败",
            )
            .await?;

            let status = resp.status();
            if status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
                let content_range = resp
                    .headers()
                    .get(reqwest::header::CONTENT_RANGE)
                    .and_then(|v| v.to_str().ok())
                    .map(ToString::to_string);
                return Ok(HlsProxyOutcome::RangeNotSatisfiable { content_range });
            }
            if !status.is_success() {
                return Err(DomainError::Validation(format!(
                    "HLS 上游响应状态异常: {status}"
                )));
            }

            let header_ct = resp
                .headers()
                .get(CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .map(ToString::to_string);
            let ct_lower = header_ct
                .as_deref()
                .map(|s| s.split(';').next().unwrap_or(s).trim().to_ascii_lowercase())
                .unwrap_or_default();
            if is_forbidden_media_ct(&ct_lower) {
                return Err(DomainError::Validation(format!(
                    "拒绝转发不安全的媒体内容类型: {ct_lower}"
                )));
            }

            let final_url = resp.url().clone();
            let content_length = resp.content_length();
            let content_range = resp
                .headers()
                .get(reqwest::header::CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .map(ToString::to_string);
            let accept_ranges = resp
                .headers()
                .get(reqwest::header::ACCEPT_RANGES)
                .and_then(|v| v.to_str().ok())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or("bytes")
                .to_string();

            // Read bounded multi-chunk prefix so BOM/whitespace or opening tags split across
            // HTTP chunks still reliably trigger HTML/SVG rejection and `#EXTM3U` / partial `#EXTINF:` sniffing.
            let prefix = read_sniff_prefix_with_context(
                &mut resp,
                SNIFF_PREFIX_MAX_BYTES,
                status == reqwest::StatusCode::PARTIAL_CONTENT,
                &ct_lower,
                &final_url,
            )
            .await?;

            if looks_like_html_bytes(&prefix) {
                return Err(DomainError::Validation(
                    "拒绝转发伪装为媒体的 HTML/SVG 内容".into(),
                ));
            }

            if include_range
                && status == reqwest::StatusCode::PARTIAL_CONTENT
                && is_partial_hls_playlist_range_candidate(&ct_lower, &final_url, &prefix)
            {
                include_range = false;
                continue;
            }

            break (
                resp,
                status,
                header_ct,
                ct_lower,
                final_url,
                content_length,
                content_range,
                accept_ranges,
                prefix,
            );
        };

        if is_hls_playlist_response(&ct_lower, &final_url, &prefix) {
            if let Some(len) = content_length
                && len > MAX_M3U8_BYTES as u64
            {
                return Err(DomainError::Validation(format!(
                    "HLS 播放列表体积超过 {} 字节限制",
                    MAX_M3U8_BYTES
                )));
            }
            if prefix.len() > MAX_M3U8_BYTES {
                return Err(DomainError::Validation(format!(
                    "HLS 播放列表体积超过 {} 字节限制",
                    MAX_M3U8_BYTES
                )));
            }

            let mut buf = Vec::with_capacity(prefix.len().min(64 * 1024));
            buf.extend_from_slice(&prefix);

            while let Some(chunk) = resp
                .chunk()
                .await
                .map_err(|e| DomainError::Validation(format!("读取 HLS 播放列表失败: {e}")))?
            {
                if buf.len().saturating_add(chunk.len()) > MAX_M3U8_BYTES {
                    return Err(DomainError::Validation(format!(
                        "HLS 播放列表体积超过 {} 字节限制",
                        MAX_M3U8_BYTES
                    )));
                }
                buf.extend_from_slice(&chunk);
            }

            if looks_like_html_bytes(&buf) {
                return Err(DomainError::Validation(
                    "拒绝转发伪装为播放列表的 HTML/SVG 内容".into(),
                ));
            }

            let text = decode_response_bytes(&buf, None, header_ct.as_deref());
            let trimmed_m3u8 =
                text.trim_start_matches(|c: char| c == '\u{feff}' || c.is_ascii_whitespace());
            let valid_extm3u_header = trimmed_m3u8
                .strip_prefix("#EXTM3U")
                .is_some_and(|rest| rest.is_empty() || rest.starts_with([' ', '\t', '\r', '\n']));
            if !valid_extm3u_header {
                return Err(DomainError::Validation(
                    "无效的 HLS 播放列表：缺少 #EXTM3U 头".into(),
                ));
            }
            let rewritten =
                crate::source::hls::rewrite_m3u8_playlist_with(&text, &final_url, rewrite_uri);
            return Ok(HlsProxyOutcome::Playlist(rewritten));
        }

        let content_type = infer_segment_content_type(header_ct.as_deref(), &final_url);
        let initial = (!prefix.is_empty()).then_some(prefix);

        Ok(HlsProxyOutcome::Segment(ProxyStreamPayload {
            status: status.as_u16(),
            content_type,
            content_length,
            content_range,
            accept_ranges,
            stream: spawn_chunk_stream(resp, initial),
        }))
    }
}

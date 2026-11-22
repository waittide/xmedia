use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use reqwest::header::{ACCEPT_ENCODING, CONTENT_TYPE, HeaderValue, RANGE, USER_AGENT};
use tokio::sync::Semaphore;
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::user::User;
use xmedia_source::http::{
    DEFAULT_WEBVIEW_UA, build_sanitized_headers, decode_response_bytes, validate_target_url_static,
};
use xmedia_source::proxy::{
    DEFAULT_IMAGE_PROXY_MAX_BYTES, fetch_validated_image_bytes_with_config,
};

use crate::source::hls::{
    MAX_M3U8_BYTES, SNIFF_PREFIX_MAX_BYTES, build_hls_proxy_url, build_poster_proxy_url,
    build_stream_proxy_url, get_or_build_streaming_async_client, infer_segment_content_type,
    is_forbidden_media_ct, is_hls_playlist_response, looks_like_html_bytes, read_sniff_prefix,
    rewrite_m3u8_playlist, spawn_chunk_stream,
};
use crate::source::model::VideoSourceSpec;

use super::dto::{
    HlsProxyOutcome, OnlineVideoBrowseResponseDto, OnlineVideoCategoriesResponseDto,
    OnlineVideoDetailResponseDto, OnlineVideoPlayResponseDto, OnlineVideoSearchErrorDto,
    OnlineVideoSearchItemDto, OnlineVideoSearchResponseDto, ProxyStreamOutcome, ProxyStreamPayload,
};
use super::{SEARCH_MAX_CONCURRENCY, SEARCH_PER_SOURCE_TIMEOUT, VideoOnlineService};

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

impl VideoOnlineService {
    async fn resolve_source_media_headers(
        &self,
        actor: &User,
        source_id: Option<Uuid>,
    ) -> DomainResult<HashMap<String, String>> {
        let Some(sid) = source_id else {
            return Ok(HashMap::new());
        };
        let rec = self.load_source_for_actor(actor, sid).await?;
        let spec: VideoSourceSpec = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("影视源 JSON 无效: {e}")))?;
        Ok(spec.parse_media_header_map())
    }

    pub async fn categories(
        &self,
        actor: &User,
        source_id: Uuid,
    ) -> DomainResult<OnlineVideoCategoriesResponseDto> {
        let rec = self.load_source_for_actor(actor, source_id).await?;
        let parsed: VideoSourceSpec = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("影视源 JSON 无效: {e}")))?;

        let engine = self.engine.clone();
        let prev_state = rec.runtime_state.clone();
        let prev_state_cmp = prev_state.clone();
        let cancel_flag = Arc::new(AtomicBool::new(false));

        let mut out = tokio::task::spawn_blocking(move || {
            engine.fetch_categories_sync_with_control(
                &parsed,
                prev_state,
                Duration::from_secs(20),
                cancel_flag,
            )
        })
        .await
        .map_err(|e| DomainError::Storage(format!("获取影视分类失败: {e}")))?
        .map_err(|e| DomainError::Validation(e.message))?;

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

        let mut handles = Vec::with_capacity(sources.len());
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

            handles.push(tokio::spawn(async move {
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

        let engine = self.engine.clone();
        let prev_state = rec.runtime_state.clone();
        let prev_state_cmp = prev_state.clone();
        let cat_owned = category_id
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);
        let cat_for_worker = cat_owned.clone();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let page_num = page.max(1);

        let mut out = tokio::task::spawn_blocking(move || {
            engine.browse_sync_with_control(
                &parsed,
                prev_state,
                cat_for_worker.as_deref(),
                page_num,
                Duration::from_secs(20),
                cancel_flag,
            )
        })
        .await
        .map_err(|e| DomainError::Storage(format!("分类浏览失败: {e}")))?
        .map_err(|e| DomainError::Validation(e.message))?;

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

        let engine = self.engine.clone();
        let prev_state = rec.runtime_state.clone();
        let prev_state_cmp = prev_state.clone();
        let vid_owned = video_id.map(ToString::to_string);
        let durl_owned = detail_url.map(ToString::to_string);
        let cancel_flag = Arc::new(AtomicBool::new(false));

        let mut out = tokio::task::spawn_blocking(move || {
            engine.fetch_detail_sync_with_control(
                &parsed,
                prev_state,
                vid_owned.as_deref(),
                durl_owned.as_deref(),
                Duration::from_secs(25),
                cancel_flag,
            )
        })
        .await
        .map_err(|e| DomainError::Storage(format!("获取影视详情失败: {e}")))?
        .map_err(|e| DomainError::Validation(e.message))?;

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

        let engine = self.engine.clone();
        let prev_state = rec.runtime_state.clone();
        let prev_state_cmp = prev_state.clone();
        let play_owned = play_url.to_string();
        let group_owned = group_name.map(ToString::to_string);
        let cancel_flag = Arc::new(AtomicBool::new(false));

        let mut out = tokio::task::spawn_blocking(move || {
            engine.resolve_play_sync_with_control(
                &parsed,
                prev_state,
                &play_owned,
                group_owned.as_deref(),
                Duration::from_secs(20),
                cancel_flag,
            )
        })
        .await
        .map_err(|e| DomainError::Storage(format!("解析播放地址失败: {e}")))?
        .map_err(|e| DomainError::Validation(e.message))?;

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
        let headers = self.resolve_source_media_headers(actor, source_id).await?;
        fetch_validated_image_bytes_with_config(
            raw_url,
            (!headers.is_empty()).then_some(&headers),
            DEFAULT_IMAGE_PROXY_MAX_BYTES,
            &self.http_config,
        )
        .await
    }

    pub async fn open_stream_proxy(
        &self,
        actor: &User,
        raw_url: &str,
        source_id: Option<Uuid>,
        range_header: Option<&str>,
    ) -> DomainResult<ProxyStreamOutcome> {
        let validated = validate_target_url_static(raw_url.trim(), self.allow_private_ips)
            .map_err(DomainError::Validation)?;
        let sanitized_range = validate_range_header(range_header)?;
        let source_headers = self.resolve_source_media_headers(actor, source_id).await?;

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

        let client = get_or_build_streaming_async_client(
            self.allow_private_ips,
            self.http_config.proxy_url.as_deref(),
        )?;
        let mut resp = client
            .get(validated)
            .headers(header_map)
            .send()
            .await
            .map_err(|e| DomainError::Validation(format!("拉取视频流失败: {e}")))?;

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

        let prefix = read_sniff_prefix(&mut resp, SNIFF_PREFIX_MAX_BYTES).await?;
        if looks_like_html_bytes(&prefix) {
            return Err(DomainError::Validation(
                "拒绝转发伪装为媒体的 HTML/SVG 内容".into(),
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
        let validated = validate_target_url_static(raw_url.trim(), self.allow_private_ips)
            .map_err(DomainError::Validation)?;
        let sanitized_range = validate_range_header(range_header)?;
        let source_headers = self.resolve_source_media_headers(actor, source_id).await?;

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

        let client = get_or_build_streaming_async_client(
            self.allow_private_ips,
            self.http_config.proxy_url.as_deref(),
        )?;
        let mut resp = client
            .get(validated)
            .headers(header_map)
            .send()
            .await
            .map_err(|e| DomainError::Validation(format!("拉取 HLS 资源失败: {e}")))?;

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
        // HTTP chunks still reliably trigger HTML/SVG rejection and `#EXTM3U` sniffing.
        let prefix = read_sniff_prefix(&mut resp, SNIFF_PREFIX_MAX_BYTES).await?;

        if looks_like_html_bytes(&prefix) {
            return Err(DomainError::Validation(
                "拒绝转发伪装为媒体的 HTML/SVG 内容".into(),
            ));
        }

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
            let rewritten = rewrite_m3u8_playlist(&text, &final_url, source_id, token);
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

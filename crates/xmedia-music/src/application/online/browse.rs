use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::Semaphore;
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::user::User;

use crate::domain::catalog::StructuredLyrics;
use crate::source::engine::url_encode_component;
use crate::source::model::{
    ExecutionDiagnostics, MusicSourceSpec, MusicTrackContext, OnlineMusicTrackItem,
};

use super::dto::{
    MusicExploreCategoryDto, OnlineMusicExploreResponseDto, OnlineMusicResolvedTrackDto,
    OnlineMusicSearchResponseDto, OnlineMusicStreamResponseDto, OnlineMusicTrackDto,
    SourceSearchErrorDto,
};
use super::{MusicOnlineService, SEARCH_MAX_CONCURRENCY, SEARCH_PER_SOURCE_TIMEOUT};

pub(super) fn build_proxy_cover_url(source_id: Uuid, cover_url: &str) -> Option<String> {
    let trimmed = cover_url.trim();
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        Some(format!(
            "/api/v1/music/online/cover-proxy?url={}&source_id={source_id}",
            url_encode_component(trimmed)
        ))
    } else {
        None
    }
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
    item: OnlineMusicTrackItem,
) -> OnlineMusicTrackDto {
    let proxy_cover_url = item
        .cover_url
        .as_deref()
        .and_then(|u| build_proxy_cover_url(source_id, u));
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

        let mut sources = self.sources.list_all().await?;
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

        let mut handles = Vec::with_capacity(sources.len());
        for rec in sources {
            let engine = self.engine.clone();
            let kw_owned = kw.to_string();
            let parsed_res = serde_json::from_str::<MusicSourceSpec>(&rec.raw_json);
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
                            Vec::new(),
                            prev_state_cmp,
                            Err(format!("音源解析失败: {e}")),
                        );
                    }
                };
                let qualities = parsed.supported_qualities.clone();
                let Ok(_permit) = sem_clone.acquire_owned().await else {
                    return (
                        sid,
                        sname,
                        qualities,
                        prev_state_cmp,
                        Err("搜索并发调度已关闭".into()),
                    );
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
                                limit_num,
                                SEARCH_PER_SOURCE_TIMEOUT,
                                cancel_worker,
                            )
                            .map_err(|e| e.message)
                    }),
                )
                .await;
                match timed {
                    Ok(Ok(res)) => (sid, sname, qualities, prev_state_cmp, res),
                    Ok(Err(e)) => (
                        sid,
                        sname,
                        qualities,
                        prev_state_cmp,
                        Err(format!("搜索任务异常: {e}")),
                    ),
                    Err(_) => {
                        cancel_flag.store(true, Ordering::Relaxed);
                        (
                            sid,
                            sname,
                            qualities,
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
            let Ok((sid, sname, qualities, prev_state, res)) = h.await else {
                continue;
            };
            match res {
                Ok(out) => {
                    if out.state != prev_state {
                        let _ = self
                            .persist_runtime_state_with_base(sid, &prev_state, out.state, false)
                            .await;
                    }
                    for t in out.diagnostics.toasts {
                        if !toasts.contains(&t) {
                            toasts.push(t);
                        }
                    }
                    for item in out.data {
                        results.push(track_item_to_dto(sid, &sname, &qualities, item));
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
        let rec = self.load_source_for_actor(actor, source_id).await?;
        let parsed: MusicSourceSpec = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("音源 JSON 无效: {e}")))?;

        let qualities = parsed.supported_qualities.clone();
        let engine = self.engine.clone();
        let prev_state = rec.runtime_state.clone();
        let prev_state_cmp = prev_state.clone();
        let explore_owned = explore_url.map(ToString::to_string);
        let page_num = page.max(1);
        let limit_num = limit.clamp(1, 50);

        let out = tokio::task::spawn_blocking(move || {
            engine.explore_sync(
                &parsed,
                prev_state,
                explore_owned.as_deref(),
                page_num,
                limit_num,
            )
        })
        .await
        .map_err(|e| DomainError::Storage(format!("浏览音源线程失败: {e}")))?
        .map_err(|e| DomainError::Validation(e.message))?;

        if out.state != prev_state_cmp {
            let _ = self
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
            .map(|item| track_item_to_dto(source_id, &rec.name, &qualities, item))
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

    pub async fn fetch_track_info(
        &self,
        actor: &User,
        source_id: Uuid,
        mut ctx: MusicTrackContext,
        quality: Option<String>,
    ) -> DomainResult<OnlineMusicResolvedTrackDto> {
        let rec = self.load_source_for_actor(actor, source_id).await?;
        let parsed: MusicSourceSpec = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("音源 JSON 无效: {e}")))?;

        if let Some(ref q) = quality {
            ctx.quality = Some(q.clone());
        }
        let qualities = parsed.supported_qualities.clone();
        let engine = self.engine.clone();
        let prev_state = rec.runtime_state.clone();
        let prev_state_cmp = prev_state.clone();

        let (
            final_state,
            resolved_track,
            resolved_stream,
            resolved_cover,
            resolved_lyrics,
            mut diag,
        ) = tokio::task::spawn_blocking(move || -> Result<_, DomainError> {
            let mut state = prev_state;
            let mut combined_diag = ExecutionDiagnostics::default();

            let track_item = if parsed.rule_track_detail.is_some()
                && (ctx
                    .detail_url
                    .as_deref()
                    .is_some_and(|s| !s.trim().is_empty())
                    || !ctx.track_id.trim().is_empty())
            {
                match engine.fetch_track_detail_sync(&parsed, state.clone(), &ctx) {
                    Ok(out) => {
                        state = out.state;
                        xmedia_source::crud::merge_diagnostics(
                            &mut combined_diag,
                            &out.diagnostics,
                        );
                        out.data
                    }
                    Err(_) => OnlineMusicTrackItem {
                        track_id: ctx.track_id.clone(),
                        title: ctx.title.clone(),
                        artist: ctx.artist.clone(),
                        album: ctx.album.clone(),
                        duration_secs: ctx.duration_secs,
                        cover_url: ctx.cover_url.clone(),
                        cover_id: ctx.cover_id.clone(),
                        url_id: ctx.url_id.clone(),
                        lyric_id: ctx.lyric_id.clone(),
                        play_url: ctx.play_url.clone(),
                        detail_url: ctx.detail_url.clone(),
                        format: None,
                    },
                }
            } else {
                OnlineMusicTrackItem {
                    track_id: ctx.track_id.clone(),
                    title: ctx.title.clone(),
                    artist: ctx.artist.clone(),
                    album: ctx.album.clone(),
                    duration_secs: ctx.duration_secs,
                    cover_url: ctx.cover_url.clone(),
                    cover_id: ctx.cover_id.clone(),
                    url_id: ctx.url_id.clone(),
                    lyric_id: ctx.lyric_id.clone(),
                    play_url: ctx.play_url.clone(),
                    detail_url: ctx.detail_url.clone(),
                    format: None,
                }
            };

            let next_ctx = track_item.to_context(ctx.quality.clone());

            let stream_res = engine
                .resolve_stream_sync(&parsed, state.clone(), &next_ctx, quality.as_deref())
                .ok()
                .map(|out| {
                    state = out.state;
                    xmedia_source::crud::merge_diagnostics(&mut combined_diag, &out.diagnostics);
                    out.data
                });

            let cover_res = engine
                .resolve_cover_sync(&parsed, state.clone(), &next_ctx)
                .ok()
                .and_then(|out| {
                    state = out.state;
                    xmedia_source::crud::merge_diagnostics(&mut combined_diag, &out.diagnostics);
                    out.data
                })
                .or_else(|| track_item.cover_url.clone());

            let lyrics_res = engine
                .resolve_lyrics_sync(&parsed, state.clone(), &next_ctx)
                .ok()
                .and_then(|out| {
                    state = out.state;
                    xmedia_source::crud::merge_diagnostics(&mut combined_diag, &out.diagnostics);
                    out.data
                });

            Ok((
                state,
                track_item,
                stream_res,
                cover_res,
                lyrics_res,
                combined_diag,
            ))
        })
        .await
        .map_err(|e| DomainError::Storage(format!("解析曲目详情线程失败: {e}")))??;

        if final_state != prev_state_cmp {
            let _ = self
                .persist_runtime_state_with_base(source_id, &prev_state_cmp, final_state, false)
                .await;
        }
        if !actor.role.is_admin() {
            diag.logs.clear();
        }

        let mut track_dto = track_item_to_dto(source_id, &rec.name, &qualities, resolved_track);
        if let Some(ref cover) = resolved_cover {
            track_dto.cover_url = Some(cover.clone());
            track_dto.proxy_cover_url = build_proxy_cover_url(source_id, cover);
        }

        let (stream_url, proxy_stream_url, format, bitrate, expires_in_secs) =
            if let Some(s) = resolved_stream {
                let proxy = build_proxy_stream_url(source_id, &s.stream_url);
                (
                    Some(s.stream_url),
                    Some(proxy),
                    Some(s.format),
                    s.bitrate,
                    s.expires_in_secs,
                )
            } else {
                (None, None, track_dto.format.clone(), None, None)
            };

        let proxy_cover_url = resolved_cover
            .as_deref()
            .and_then(|u| build_proxy_cover_url(source_id, u));

        Ok(OnlineMusicResolvedTrackDto {
            source_id,
            source_name: rec.name,
            track: track_dto,
            stream_url,
            proxy_stream_url,
            cover_url: resolved_cover,
            proxy_cover_url,
            lyrics: resolved_lyrics.as_ref().map(|l| l.lyrics.clone()),
            translated_lyrics: resolved_lyrics.and_then(|l| l.translated_lyrics),
            format,
            bitrate,
            expires_in_secs,
            diagnostics: diag,
        })
    }

    pub async fn fetch_stream(
        &self,
        actor: &User,
        source_id: Uuid,
        ctx: MusicTrackContext,
        quality: Option<String>,
    ) -> DomainResult<OnlineMusicStreamResponseDto> {
        let rec = self.load_source_for_actor(actor, source_id).await?;
        let parsed: MusicSourceSpec = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("音源 JSON 无效: {e}")))?;

        let engine = self.engine.clone();
        let prev_state = rec.runtime_state.clone();
        let prev_state_cmp = prev_state.clone();

        let out = tokio::task::spawn_blocking(move || {
            engine.resolve_stream_sync(&parsed, prev_state, &ctx, quality.as_deref())
        })
        .await
        .map_err(|e| DomainError::Storage(format!("解析音频流线程失败: {e}")))?
        .map_err(|e| DomainError::Validation(e.message))?;

        if out.state != prev_state_cmp {
            let _ = self
                .persist_runtime_state_with_base(source_id, &prev_state_cmp, out.state, false)
                .await;
        }

        let stream_url = out.data.stream_url.trim().to_string();
        if !stream_url.starts_with("http://") && !stream_url.starts_with("https://") {
            return Err(DomainError::Validation(format!(
                "解析到的音频流地址不是有效的 HTTP(S) URL: {stream_url}"
            )));
        }

        let proxy_stream_url = build_proxy_stream_url(source_id, &stream_url);
        let headers_applied = !out.data.headers.is_empty();

        Ok(OnlineMusicStreamResponseDto {
            source_id,
            stream_url,
            proxy_stream_url,
            format: Some(out.data.format),
            bitrate: out.data.bitrate,
            expires_in_secs: out.data.expires_in_secs,
            headers_applied,
        })
    }

    pub async fn fetch_lyrics(
        &self,
        actor: &User,
        source_id: Uuid,
        ctx: MusicTrackContext,
    ) -> DomainResult<StructuredLyrics> {
        let rec = self.load_source_for_actor(actor, source_id).await?;
        let parsed: MusicSourceSpec = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("音源 JSON 无效: {e}")))?;

        let engine = self.engine.clone();
        let prev_state = rec.runtime_state.clone();
        let prev_state_cmp = prev_state.clone();

        let out = tokio::task::spawn_blocking(move || {
            engine.resolve_lyrics_sync(&parsed, prev_state, &ctx)
        })
        .await
        .map_err(|e| DomainError::Storage(format!("解析歌词线程失败: {e}")))?
        .map_err(|e| DomainError::Validation(e.message))?;

        if out.state != prev_state_cmp {
            let _ = self
                .persist_runtime_state_with_base(source_id, &prev_state_cmp, out.state, false)
                .await;
        }

        Ok(out
            .data
            .map(|r| r.lyrics)
            .unwrap_or_else(StructuredLyrics::empty))
    }
}

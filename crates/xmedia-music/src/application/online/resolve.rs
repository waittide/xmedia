use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::user::User;

use crate::domain::catalog::StructuredLyrics;
use crate::domain::online::{
    ExecutionDiagnostics, MusicSourceSpec, MusicTrackContext, OnlineMusicTrackItem, is_http_url,
};

use super::browse::{build_proxy_cover_url, build_proxy_stream_url, track_item_to_dto};
use super::dto::{OnlineMusicResolvedTrackDto, OnlineMusicStreamResponseDto};
use super::{CancelOnDrop, MusicOnlineService};

impl MusicOnlineService {
    pub async fn fetch_track_info(
        &self,
        actor: &User,
        source_id: Uuid,
        mut ctx: MusicTrackContext,
        quality: Option<String>,
    ) -> DomainResult<OnlineMusicResolvedTrackDto> {
        let rec = self.sources.load_source_for_actor(actor, source_id).await?;
        let parsed: MusicSourceSpec = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("音源 JSON 无效: {e}")))?;

        let op_timeout = self.op_timeout;
        let track_info_timeout = self.track_info_timeout;
        self.acquire_rate_slot(source_id, parsed.parse_concurrent_rate(), op_timeout)
            .await
            .map_err(DomainError::Validation)?;

        if let Some(ref q) = quality {
            ctx.quality = Some(q.clone());
        }
        let qualities = parsed.supported_qualities.clone();
        let has_cover_rule = parsed.rule_cover.is_some();
        let engine = self.engine.clone();
        let prev_state = rec.runtime_state.clone();
        let prev_state_cmp = prev_state.clone();

        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_worker = cancel_flag.clone();
        let mut cancel_guard = CancelOnDrop::new(cancel_flag.clone());

        let timed = tokio::time::timeout(
            track_info_timeout,
            tokio::task::spawn_blocking(move || -> Result<_, DomainError> {
                let total_start = Instant::now();
                let remaining_budget = || {
                    track_info_timeout
                        .saturating_sub(total_start.elapsed())
                        .max(Duration::from_millis(1))
                };
                let check_not_cancelled = || -> Result<(), DomainError> {
                    if cancel_worker.load(Ordering::Relaxed)
                        || total_start.elapsed() >= track_info_timeout
                    {
                        Err(DomainError::Validation(format!(
                            "获取曲目详情超时（超过 {} 秒）",
                            track_info_timeout.as_secs()
                        )))
                    } else {
                        Ok(())
                    }
                };

                let mut state = prev_state;
                let mut combined_diag = ExecutionDiagnostics::default();

                let fallback_item_from_ctx = |ctx: &MusicTrackContext| OnlineMusicTrackItem {
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
                };

                let track_item = if parsed.rule_track_detail.is_some()
                    && (ctx
                        .detail_url
                        .as_deref()
                        .is_some_and(|s| !s.trim().is_empty())
                        || !ctx.track_id.trim().is_empty())
                {
                    match engine.fetch_track_detail_sync_with_control(
                        &parsed,
                        state.clone(),
                        &ctx,
                        remaining_budget(),
                        cancel_worker.clone(),
                    ) {
                        Ok(out) => {
                            state = out.state;
                            xmedia_source::crud::merge_diagnostics(
                                &mut combined_diag,
                                &out.diagnostics,
                            );
                            out.data
                        }
                        Err(_) => {
                            check_not_cancelled()?;
                            fallback_item_from_ctx(&ctx)
                        }
                    }
                } else {
                    fallback_item_from_ctx(&ctx)
                };

                check_not_cancelled()?;
                let next_ctx = track_item.to_context(ctx.quality.clone());

                let stream_res = match engine.resolve_stream_sync_with_control(
                    &parsed,
                    state.clone(),
                    &next_ctx,
                    quality.as_deref(),
                    remaining_budget(),
                    cancel_worker.clone(),
                ) {
                    Ok(out) => {
                        state = out.state;
                        xmedia_source::crud::merge_diagnostics(
                            &mut combined_diag,
                            &out.diagnostics,
                        );
                        Some(out.data)
                    }
                    Err(_) => {
                        check_not_cancelled()?;
                        None
                    }
                };

                check_not_cancelled()?;
                let cover_res = match engine.resolve_cover_sync_with_control(
                    &parsed,
                    state.clone(),
                    &next_ctx,
                    remaining_budget(),
                    cancel_worker.clone(),
                ) {
                    Ok(out) => {
                        state = out.state;
                        xmedia_source::crud::merge_diagnostics(
                            &mut combined_diag,
                            &out.diagnostics,
                        );
                        out.data
                    }
                    Err(_) => {
                        check_not_cancelled()?;
                        None
                    }
                }
                .or_else(|| track_item.cover_url.clone());

                check_not_cancelled()?;
                let lyrics_res = match engine.resolve_lyrics_sync_with_control(
                    &parsed,
                    state.clone(),
                    &next_ctx,
                    remaining_budget(),
                    cancel_worker.clone(),
                ) {
                    Ok(out) => {
                        state = out.state;
                        xmedia_source::crud::merge_diagnostics(
                            &mut combined_diag,
                            &out.diagnostics,
                        );
                        out.data
                    }
                    Err(_) => {
                        check_not_cancelled()?;
                        None
                    }
                };

                Ok((
                    state,
                    track_item,
                    stream_res,
                    cover_res,
                    lyrics_res,
                    combined_diag,
                ))
            }),
        )
        .await;

        let (
            final_state,
            resolved_track,
            resolved_stream,
            resolved_cover,
            resolved_lyrics,
            mut diag,
        ) = match timed {
            Ok(Ok(Ok(res))) => {
                cancel_guard.disarm();
                res
            }
            Ok(Ok(Err(err))) => {
                cancel_guard.disarm();
                return Err(err);
            }
            Ok(Err(e)) => {
                cancel_guard.disarm();
                return Err(DomainError::Storage(format!("解析曲目详情线程失败: {e}")));
            }
            Err(_) => {
                cancel_flag.store(true, Ordering::Relaxed);
                return Err(DomainError::Validation(format!(
                    "获取曲目详情超时（超过 {} 秒）",
                    track_info_timeout.as_secs()
                )));
            }
        };

        if final_state != prev_state_cmp {
            let _ = self
                .sources
                .persist_runtime_state_with_base(source_id, &prev_state_cmp, final_state, false)
                .await;
        }
        if !actor.role.is_admin() {
            diag.logs.clear();
        }

        let mut track_dto = track_item_to_dto(
            source_id,
            &rec.name,
            &qualities,
            has_cover_rule,
            resolved_track,
        );
        if let Some(ref cover) = resolved_cover {
            track_dto.cover_url = Some(cover.clone());
            track_dto.proxy_cover_url = build_proxy_cover_url(source_id, cover);
        }

        let (stream_url, proxy_stream_url, format, bitrate, expires_in_secs) =
            if let Some(s) = resolved_stream.filter(|s| is_http_url(&s.stream_url)) {
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
        let rec = self.sources.load_source_for_actor(actor, source_id).await?;
        let parsed: MusicSourceSpec = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("音源 JSON 无效: {e}")))?;

        let op_timeout = self.op_timeout;
        self.acquire_rate_slot(source_id, parsed.parse_concurrent_rate(), op_timeout)
            .await
            .map_err(DomainError::Validation)?;

        let engine = self.engine.clone();
        let headers_applied = self.media_proxy.has_effective_proxy_headers(&parsed);
        let prev_state = rec.runtime_state.clone();
        let prev_state_cmp = prev_state.clone();

        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_worker = cancel_flag.clone();
        let mut cancel_guard = CancelOnDrop::new(cancel_flag.clone());

        let timed = tokio::time::timeout(
            op_timeout,
            tokio::task::spawn_blocking(move || {
                engine
                    .resolve_stream_sync_with_control(
                        &parsed,
                        prev_state,
                        &ctx,
                        quality.as_deref(),
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
                return Err(DomainError::Storage(format!("解析音频流线程失败: {e}")));
            }
            Err(_) => {
                cancel_flag.store(true, Ordering::Relaxed);
                return Err(DomainError::Validation(format!(
                    "解析音频流超时（超过 {} 秒）",
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

        let stream_url = out.data.stream_url.trim().to_string();
        if !is_http_url(&stream_url) {
            return Err(DomainError::Validation(format!(
                "解析到的音频流地址不是有效的 HTTP(S) URL: {stream_url}"
            )));
        }

        let proxy_stream_url = build_proxy_stream_url(source_id, &stream_url);

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
        let rec = self.sources.load_source_for_actor(actor, source_id).await?;
        let parsed: MusicSourceSpec = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("音源 JSON 无效: {e}")))?;

        let op_timeout = self.op_timeout;
        self.acquire_rate_slot(source_id, parsed.parse_concurrent_rate(), op_timeout)
            .await
            .map_err(DomainError::Validation)?;

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
                    .resolve_lyrics_sync_with_control(
                        &parsed,
                        prev_state,
                        &ctx,
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
                return Err(DomainError::Storage(format!("解析歌词线程失败: {e}")));
            }
            Err(_) => {
                cancel_flag.store(true, Ordering::Relaxed);
                return Err(DomainError::Validation(format!(
                    "解析歌词超时（超过 {} 秒）",
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

        Ok(out
            .data
            .map(|r| r.lyrics)
            .unwrap_or_else(StructuredLyrics::empty))
    }
}

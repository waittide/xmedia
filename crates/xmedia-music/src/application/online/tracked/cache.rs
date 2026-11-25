use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use bytes::Bytes;
use jiff::Timestamp;
use uuid::Uuid;
use xmedia_core::application::ports::{BinaryPayload, SystemEvent};
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;

use super::super::{CancelOnDrop, MusicOnlineService, MusicTrackingDeps, get_or_insert_weak_lock};
use super::MAX_TRACKED_AUDIO_BYTES;
use crate::domain::catalog::{
    AudioFormat, OnlineTrackedTrackSource, StructuredLyrics, TrackId, TrackSource,
};
use crate::domain::online::{MusicSourceSpec, MusicTrackContext, infer_audio_format, is_http_url};

fn infer_audio_ext_from_content_type(ct: &str) -> Option<&'static str> {
    let base = ct
        .split(';')
        .next()
        .unwrap_or(ct)
        .trim()
        .to_ascii_lowercase();
    match base.as_str() {
        "audio/flac" | "audio/x-flac" => Some("flac"),
        "audio/mpeg" | "audio/mp3" => Some("mp3"),
        "audio/aac" | "audio/x-aac" | "audio/aacp" => Some("aac"),
        "audio/mp4" | "audio/x-m4a" | "audio/m4a" | "video/mp4" | "application/mp4" => Some("m4a"),
        "audio/ogg" | "application/ogg" => Some("ogg"),
        "audio/opus" => Some("opus"),
        "audio/wav" | "audio/x-wav" | "audio/wave" => Some("wav"),
        "audio/aiff" | "audio/x-aiff" => Some("aiff"),
        "audio/x-ape" | "audio/ape" => Some("ape"),
        "audio/x-wavpack" | "audio/wavpack" => Some("wv"),
        _ => None,
    }
}

fn format_lyrics_as_lrc(raw_lyric: &str, structured: &StructuredLyrics) -> Option<String> {
    if !raw_lyric.trim().is_empty() {
        return Some(raw_lyric.to_string());
    }
    if structured.synced && !structured.lines.is_empty() {
        let mut out = String::new();
        for line in &structured.lines {
            let total_secs = line.start_ms / 1000;
            let mins = total_secs / 60;
            let secs = total_secs % 60;
            let cs = (line.start_ms % 1000) / 10;
            out.push_str(&format!("[{mins:02}:{secs:02}.{cs:02}]{}\n", line.text));
        }
        if !out.trim().is_empty() {
            return Some(out);
        }
    }
    if !structured.plain_text.trim().is_empty() {
        return Some(structured.plain_text.clone());
    }
    None
}

impl MusicOnlineService {
    async fn build_internal_source_headers(
        &self,
        source_id: Uuid,
        target_url: &str,
        extra_headers: Option<&HashMap<String, String>>,
    ) -> HashMap<String, String> {
        let mut merged = HashMap::new();
        if let Ok(rec) = self.sources.load_source(source_id).await
            && let Ok(spec) = serde_json::from_str::<MusicSourceSpec>(&rec.raw_json)
        {
            merged = spec.build_media_headers_with_cookies(&rec.runtime_state.cookies, target_url);
        }
        if let Some(extra) = extra_headers {
            for (k, v) in extra {
                let kt = k.trim();
                if kt.is_empty() {
                    continue;
                }
                merged.retain(|ek, _| !ek.eq_ignore_ascii_case(kt));
                merged.insert(kt.to_string(), v.clone());
            }
        }
        merged.retain(|k, _| !k.eq_ignore_ascii_case("range"));
        merged
    }

    pub(super) async fn fetch_and_save_cover_best_effort(
        &self,
        deps: &MusicTrackingDeps,
        source_id: Uuid,
        cover_id_key: &str,
        cover_url: Option<&str>,
    ) -> Option<String> {
        let url = cover_url.map(str::trim).filter(|u| is_http_url(u))?;
        let headers = self
            .build_internal_source_headers(source_id, url, None)
            .await;
        let header_opt = (!headers.is_empty()).then_some(&headers);
        let (bytes, mime) = self
            .media_proxy
            .fetch_cover_image(url, header_opt)
            .await
            .ok()?;
        let payload = BinaryPayload {
            bytes,
            mime_type: mime.to_string(),
        };
        deps.storage.save_cover(cover_id_key, &payload).await.ok()
    }

    pub(super) async fn resolve_cover_url_internal(
        &self,
        source_id: Uuid,
        ctx: MusicTrackContext,
    ) -> Option<String> {
        let rec = self.sources.load_source(source_id).await.ok()?;
        let parsed: MusicSourceSpec = serde_json::from_str(&rec.raw_json).ok()?;
        parsed.rule_cover.as_ref()?;
        let op_timeout = self.op_timeout.min(Duration::from_secs(15));
        let engine = self.engine.clone();
        let prev_state = rec.runtime_state.clone();
        let prev_state_cmp = prev_state.clone();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_worker = cancel_flag.clone();
        let mut cancel_guard = CancelOnDrop::new(cancel_flag.clone());

        let res = tokio::time::timeout(
            op_timeout,
            tokio::task::spawn_blocking(move || {
                engine.resolve_cover_sync_with_control(
                    &parsed,
                    prev_state,
                    &ctx,
                    op_timeout,
                    cancel_worker,
                )
            }),
        )
        .await;

        match res {
            Ok(Ok(Ok(out))) => {
                cancel_guard.disarm();
                if out.state != prev_state_cmp {
                    let _ = self
                        .sources
                        .persist_runtime_state_with_base(
                            source_id,
                            &prev_state_cmp,
                            out.state,
                            false,
                        )
                        .await;
                }
                out.data
            }
            _ => {
                cancel_flag.store(true, Ordering::Relaxed);
                None
            }
        }
    }

    async fn resolve_lyrics_internal(
        &self,
        source_id: Uuid,
        ctx: MusicTrackContext,
    ) -> Option<String> {
        let rec = self.sources.load_source(source_id).await.ok()?;
        let parsed: MusicSourceSpec = serde_json::from_str(&rec.raw_json).ok()?;
        parsed.rule_lyric.as_ref()?;
        let op_timeout = self.op_timeout.min(Duration::from_secs(20));
        let engine = self.engine.clone();
        let prev_state = rec.runtime_state.clone();
        let prev_state_cmp = prev_state.clone();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_worker = cancel_flag.clone();
        let mut cancel_guard = CancelOnDrop::new(cancel_flag.clone());

        let res = tokio::time::timeout(
            op_timeout,
            tokio::task::spawn_blocking(move || {
                engine.resolve_lyrics_sync_with_control(
                    &parsed,
                    prev_state,
                    &ctx,
                    op_timeout,
                    cancel_worker,
                )
            }),
        )
        .await;

        match res {
            Ok(Ok(Ok(out))) => {
                cancel_guard.disarm();
                if out.state != prev_state_cmp {
                    let _ = self
                        .sources
                        .persist_runtime_state_with_base(
                            source_id,
                            &prev_state_cmp,
                            out.state,
                            false,
                        )
                        .await;
                }
                out.data
                    .and_then(|r| format_lyrics_as_lrc(&r.raw_lyric, &r.lyrics))
            }
            _ => {
                cancel_flag.store(true, Ordering::Relaxed);
                None
            }
        }
    }

    pub async fn ensure_tracked_lyrics_cached(&self, track_id: TrackId) -> DomainResult<()> {
        let Some(deps) = self.tracking.as_ref() else {
            return Ok(());
        };
        let Some(mut track) = deps.catalog_repo.find_track_by_id(track_id).await? else {
            return Ok(());
        };
        if track.has_lyrics() || !track.source.is_online_tracked() {
            return Ok(());
        }
        let TrackSource::OnlineTracked(ref info) = track.source else {
            return Ok(());
        };
        if info.sidecar_lrc_key.is_some() {
            return Ok(());
        }

        let source_id = info.source_id;
        let ctx = MusicTrackContext {
            track_id: info.item_id.clone(),
            title: track.title.clone(),
            artist: track.artist_name.clone(),
            album: Some(track.album_title.clone()),
            duration_secs: (track.duration_secs > 0).then_some(track.duration_secs),
            cover_url: info.cover_url.clone(),
            cover_id: info.cover_id.clone(),
            url_id: info.url_id.clone(),
            lyric_id: info.lyric_id.clone(),
            play_url: info.play_url.clone(),
            detail_url: info.detail_url.clone(),
            quality: info.quality.clone(),
        };

        let Some(lrc_text) = self.resolve_lyrics_internal(source_id, ctx).await else {
            return Ok(());
        };

        let lock = get_or_insert_weak_lock(&self.track_cache_locks, track_id);
        let _guard = lock.lock().await;
        let Some(latest) = deps.catalog_repo.find_track_by_id(track_id).await? else {
            return Ok(());
        };
        track = latest;
        if track.has_sidecar_lyrics {
            return Ok(());
        }
        let saved_lrc_key = deps
            .storage
            .save_managed_sidecar_lrc(track_id, Bytes::from(lrc_text.into_bytes()))
            .await?;
        track.attach_online_sidecar_lrc(saved_lrc_key, Timestamp::now());
        deps.catalog_repo.save_track(&track).await?;
        Ok(())
    }

    pub async fn ensure_tracked_track_cached(&self, track_id: TrackId) -> DomainResult<()> {
        let Some(deps) = self.tracking.as_ref() else {
            return Ok(());
        };
        let Some(track) = deps.catalog_repo.find_track_by_id(track_id).await? else {
            return Ok(());
        };
        if !track.source.is_online_tracked() {
            return Ok(());
        }
        if track.source.is_cached()
            && let Some(library) = deps.libraries.find_by_id(track.library_id).await?
            && deps
                .storage
                .resolve_audio_fs_path(&library, &track)
                .await
                .is_ok()
        {
            return Ok(());
        }

        let lock = get_or_insert_weak_lock(&self.track_cache_locks, track_id);
        let _guard = lock.lock().await;

        let mut track = deps
            .catalog_repo
            .find_track_by_id(track_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("track not found".into()))?;
        if !track.source.is_online_tracked() {
            return Ok(());
        }
        let library = deps
            .libraries
            .find_by_id(track.library_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("library not found".into()))?;
        if track.source.is_cached()
            && deps
                .storage
                .resolve_audio_fs_path(&library, &track)
                .await
                .is_ok()
        {
            return Ok(());
        }

        let info = match &track.source {
            TrackSource::OnlineTracked(info) => (**info).clone(),
            _ => return Ok(()),
        };
        let source_id = info.source_id;
        let item_id = info.item_id;
        let detail_url = info.detail_url;
        let play_url = info.play_url;
        let url_id = info.url_id;
        let cover_url = info.cover_url;
        let cover_id = info.cover_id;
        let lyric_id = info.lyric_id;
        let quality = info.quality;
        let existing_lrc_key = info.sidecar_lrc_key;

        let rec = self.sources.load_source(source_id).await?;
        let parsed: MusicSourceSpec = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("音源 JSON 无效: {e}")))?;

        let op_timeout = self.op_timeout;
        self.acquire_rate_slot(source_id, parsed.parse_concurrent_rate(), op_timeout)
            .await
            .map_err(DomainError::Validation)?;

        let ctx = MusicTrackContext {
            track_id: item_id.clone(),
            title: track.title.clone(),
            artist: track.artist_name.clone(),
            album: Some(track.album_title.clone()),
            duration_secs: (track.duration_secs > 0).then_some(track.duration_secs),
            cover_url: cover_url.clone(),
            cover_id: cover_id.clone(),
            url_id: url_id.clone(),
            lyric_id: lyric_id.clone(),
            play_url: play_url.clone(),
            detail_url: detail_url.clone(),
            quality: quality.clone(),
        };

        let engine = self.engine.clone();
        let prev_state = rec.runtime_state.clone();
        let prev_state_cmp = prev_state.clone();
        let ctx_for_stream = ctx.clone();
        let quality_for_stream = quality.clone();
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
                        &ctx_for_stream,
                        quality_for_stream.as_deref(),
                        op_timeout,
                        cancel_worker,
                    )
                    .map_err(|e| e.message)
            }),
        )
        .await;

        let stream_out = match timed {
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

        if stream_out.state != prev_state_cmp {
            let _ = self
                .sources
                .persist_runtime_state_with_base(
                    source_id,
                    &prev_state_cmp,
                    stream_out.state,
                    false,
                )
                .await;
        }

        let resolved_url = stream_out.data.stream_url.trim().to_string();
        if !is_http_url(&resolved_url) {
            return Err(DomainError::Validation(format!(
                "解析到的音频流地址不是有效的 HTTP(S) URL: {resolved_url}"
            )));
        }

        let initial_ext = (!stream_out.data.format.trim().is_empty())
            .then_some(stream_out.data.format.trim().to_string())
            .or_else(|| infer_audio_format(&resolved_url))
            .unwrap_or_else(|| track.format.extension().to_string());

        let merged_headers = self
            .build_internal_source_headers(source_id, &resolved_url, Some(&stream_out.data.headers))
            .await;
        let downloaded = self
            .media_proxy
            .download_audio_to_temp_file(
                &resolved_url,
                &merged_headers,
                &initial_ext,
                MAX_TRACKED_AUDIO_BYTES,
            )
            .await?;

        let probe_ext = downloaded
            .content_type
            .as_deref()
            .and_then(infer_audio_ext_from_content_type)
            .map(ToString::to_string)
            .unwrap_or(initial_ext);

        let probe = deps.probe.clone();
        let probe_path = downloaded.temp_file.path().to_string_lossy().into_owned();
        let probe_ext_clone = probe_ext.clone();
        let fallback_title = track.title.clone();
        let probed_opt = tokio::task::spawn_blocking(move || {
            probe.probe_audio_file(&probe_path, &probe_ext_clone, &fallback_title)
        })
        .await
        .map_err(|e| DomainError::Storage(format!("音频元数据探测任务失败: {e}")))?
        .ok();

        let final_format = probed_opt
            .as_ref()
            .map(|p| p.format)
            .or_else(|| AudioFormat::parse(&probe_ext).ok())
            .unwrap_or(track.format);

        let stored = deps
            .storage
            .save_managed_audio_from_path(track.id, final_format, downloaded.temp_file.path())
            .await?;

        if let Some(ref probed) = probed_opt {
            track.apply_probed_metadata(
                probed.duration_secs,
                probed.bit_rate_kbps,
                probed.sample_rate_hz,
                probed.bit_depth,
                probed.channels,
                probed.year,
                probed.genre.clone(),
                probed.replay_gain.clone(),
                probed.has_embedded_lyrics,
            );
            if track.cover_key.is_none()
                && let Some(ref embedded_cover) = probed.cover
                && let Ok(key) = deps
                    .storage
                    .save_cover(&track.id.0.to_string(), embedded_cover)
                    .await
            {
                track.set_cover_if_absent(key);
            }
        } else if let Some(br) = stream_out.data.bitrate {
            track.set_bit_rate_kbps(br);
        }

        if track.cover_key.is_none() {
            let resolved_cover = match cover_url.clone() {
                Some(u) => Some(u),
                None => {
                    self.resolve_cover_url_internal(source_id, ctx.clone())
                        .await
                }
            };
            if let Some(key) = self
                .fetch_and_save_cover_best_effort(
                    deps,
                    source_id,
                    &track.id.0.to_string(),
                    resolved_cover.as_deref(),
                )
                .await
            {
                track.set_cover_if_absent(key);
            }
        }

        let mut final_lrc_key = existing_lrc_key;
        let mut added_sidecar_lrc = false;
        if final_lrc_key.is_none()
            && !track.has_embedded_lyrics
            && let Some(lrc_text) = self.resolve_lyrics_internal(source_id, ctx).await
            && let Ok(key) = deps
                .storage
                .save_managed_sidecar_lrc(track.id, Bytes::from(lrc_text.into_bytes()))
                .await
        {
            final_lrc_key = Some(key);
            added_sidecar_lrc = true;
        }

        let now = Timestamp::now();
        track.finalize_online_cached(
            final_format,
            stored.file_size,
            stored.file_hash,
            OnlineTrackedTrackSource {
                storage_key: Some(stored.storage_key),
                source_id,
                item_id,
                detail_url,
                play_url,
                url_id,
                cover_url,
                cover_id,
                lyric_id,
                quality,
                cached: true,
                sidecar_lrc_key: final_lrc_key,
            },
            added_sidecar_lrc,
            now,
        );

        deps.catalog_repo.save_track(&track).await?;

        if let Some(ref cov) = track.cover_key {
            if let Ok(Some(mut album)) = deps.catalog_repo.find_album_by_id(track.album_id).await
                && album.set_cover_if_absent(cov, now)
            {
                let _ = deps.catalog_repo.save_album(&album).await;
            }
            if let Ok(Some(mut artist)) = deps.catalog_repo.find_artist_by_id(track.artist_id).await
                && artist.set_cover_if_absent(cov, now)
            {
                let _ = deps.catalog_repo.save_artist(&artist).await;
            }
        }

        deps.catalog_repo.recompute_catalog_aggregates().await?;
        deps.events.publish(SystemEvent::MediaChanged {
            media_kind: MediaKind::Music,
        });
        Ok(())
    }
}

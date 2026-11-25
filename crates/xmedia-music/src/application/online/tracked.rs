use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use axum::http::header::{CONTENT_TYPE, USER_AGENT};
use axum::http::{HeaderValue, StatusCode};
use bytes::Bytes;
use jiff::Timestamp;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;
use xmedia_core::application::ports::{BinaryPayload, SystemEvent};
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;
use xmedia_core::domain::user::User;
use xmedia_source::http::{
    DEFAULT_WEBVIEW_UA, HttpClientConfig, build_sanitized_headers, validate_target_url_static,
};
use xmedia_source::proxy::{
    DEFAULT_IMAGE_PROXY_MAX_BYTES, DEFAULT_IMAGE_PROXY_TIMEOUT_SECS,
    fetch_validated_image_bytes_with_config, looks_like_html_or_svg_bytes,
};

use crate::application::dto::TrackDto;
use crate::domain::catalog::{
    AudioFormat, OnlineTrackedTrackSource, ReplayGain, StructuredLyrics, Track, TrackId,
    TrackSource,
};
use crate::source::engine::{infer_audio_format, is_http_url};
use crate::source::model::{MusicSourceSpec, MusicTrackContext};

use super::dto::TrackOnlineMusicInput;
use super::proxy::{
    AUDIO_PROXY_READ_TIMEOUT, AUDIO_SNIFF_PREFIX_MAX_BYTES, get_or_build_audio_streaming_client,
    is_allowed_audio_content_type, lookup_cookie_for_url, read_audio_sniff_prefix,
};
use super::{CancelOnDrop, MusicOnlineService, MusicTrackingDeps, get_or_insert_weak_lock};

const MAX_TRACKED_AUDIO_BYTES: u64 = 256 * 1024 * 1024;
const MAX_BATCH_TRACK_COUNT: usize = 200;

async fn open_exclusive_temp_file(path: &Path) -> std::io::Result<tokio::fs::File> {
    let mut opts = tokio::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    opts.mode(0o600);
    opts.open(path).await
}

struct TempFileGuard {
    path: PathBuf,
}

impl TempFileGuard {
    async fn create_exclusive(ext: &str) -> DomainResult<(Self, tokio::fs::File)> {
        let clean_ext: String = ext
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .take(8)
            .collect();
        let suffix = if clean_ext.is_empty() {
            "tmp".to_string()
        } else {
            clean_ext
        };
        for _ in 0..8 {
            let path =
                std::env::temp_dir().join(format!("xmedia-music-{}.{suffix}", Uuid::now_v7()));
            match open_exclusive_temp_file(&path).await {
                Ok(file) => return Ok((Self { path }, file)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => {
                    return Err(DomainError::Storage(format!(
                        "创建临时音频缓存文件失败: {e}"
                    )));
                }
            }
        }
        Err(DomainError::Storage(
            "创建独占临时音频缓存文件失败: 重试次数耗尽".into(),
        ))
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn clean_opt(v: Option<String>) -> Option<String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn canonical_item_id(input: &TrackOnlineMusicInput) -> String {
    let tid = input.track_id.trim();
    if !tid.is_empty() {
        return tid.to_string();
    }
    if let Some(u) = input
        .detail_url
        .as_deref()
        .or(input.play_url.as_deref())
        .or(input.url_id.as_deref())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return u.to_string();
    }
    format!(
        "{}::{}",
        input.artist.trim().to_lowercase(),
        input.title.trim().to_lowercase()
    )
}

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
    fn require_tracking(&self) -> DomainResult<&MusicTrackingDeps> {
        self.tracking
            .as_ref()
            .ok_or_else(|| DomainError::Storage("音乐追更存储依赖未初始化".into()))
    }

    async fn build_internal_source_headers(
        &self,
        source_id: Uuid,
        target_url: &str,
        extra_headers: Option<&HashMap<String, String>>,
    ) -> HashMap<String, String> {
        let mut merged = HashMap::new();
        if let Ok(rec) = self.load_source(source_id).await
            && let Ok(spec) = serde_json::from_str::<MusicSourceSpec>(&rec.raw_json)
        {
            merged = spec.parse_media_headers_for_target(target_url);
            if !merged.keys().any(|k| k.eq_ignore_ascii_case("cookie"))
                && spec.enabled_cookie_jar
                && let Some(cookie) = lookup_cookie_for_url(&rec.runtime_state.cookies, target_url)
            {
                merged.insert("Cookie".to_string(), cookie);
            }
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

    async fn fetch_and_save_cover_best_effort(
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
        let cfg = HttpClientConfig {
            timeout_secs: DEFAULT_IMAGE_PROXY_TIMEOUT_SECS,
            allow_private_ips: self.allow_private_ips,
            proxy_url: None,
        };
        let (bytes, mime) = fetch_validated_image_bytes_with_config(
            url,
            header_opt,
            DEFAULT_IMAGE_PROXY_MAX_BYTES,
            &cfg,
        )
        .await
        .ok()?;
        let payload = BinaryPayload {
            bytes,
            mime_type: mime.to_string(),
        };
        deps.storage.save_cover(cover_id_key, &payload).await.ok()
    }

    async fn resolve_cover_url_internal(
        &self,
        source_id: Uuid,
        ctx: MusicTrackContext,
    ) -> Option<String> {
        let rec = self.load_source(source_id).await.ok()?;
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
        let rec = self.load_source(source_id).await.ok()?;
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

    async fn download_audio_to_temp_file(
        &self,
        source_id: Uuid,
        stream_url: &str,
        extra_headers: &HashMap<String, String>,
        ext_hint: &str,
    ) -> DomainResult<(TempFileGuard, Option<String>, u64)> {
        let validated_url = validate_target_url_static(stream_url.trim(), self.allow_private_ips)
            .map_err(DomainError::Validation)?;
        let merged = self
            .build_internal_source_headers(source_id, validated_url.as_str(), Some(extra_headers))
            .await;
        let mut out_headers = build_sanitized_headers(&merged);
        if !out_headers.contains_key(USER_AGENT) {
            out_headers.insert(USER_AGENT, HeaderValue::from_static(DEFAULT_WEBVIEW_UA));
        }

        let client = get_or_build_audio_streaming_client(self.allow_private_ips)?;
        let mut resp = tokio::time::timeout(
            AUDIO_PROXY_READ_TIMEOUT,
            client.get(validated_url).headers(out_headers).send(),
        )
        .await
        .map_err(|_| DomainError::Validation("上游音频下载连接超时".into()))?
        .map_err(|e| DomainError::Validation(format!("拉取在线音频失败: {e}")))?;

        let status = resp.status();
        if status != StatusCode::OK && status != StatusCode::PARTIAL_CONTENT {
            return Err(DomainError::Validation(format!(
                "上游音频响应状态异常: {status}"
            )));
        }

        let raw_ct = resp
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
            .unwrap_or("")
            .to_string();
        let base_ct = raw_ct
            .split(';')
            .next()
            .unwrap_or(&raw_ct)
            .trim()
            .to_ascii_lowercase();
        if !is_allowed_audio_content_type(&base_ct) {
            return Err(DomainError::UnsupportedFormat(format!(
                "不支持的音频内容类型: {base_ct}"
            )));
        }

        let prefix = read_audio_sniff_prefix(&mut resp, AUDIO_SNIFF_PREFIX_MAX_BYTES).await?;
        if looks_like_html_or_svg_bytes(&prefix) {
            return Err(DomainError::Validation(
                "上游返回了伪装为音频的 HTML/SVG 内容".into(),
            ));
        }

        let (temp, mut file) = TempFileGuard::create_exclusive(ext_hint).await?;

        let mut total_bytes: u64 = 0;
        if !prefix.is_empty() {
            total_bytes = total_bytes.saturating_add(prefix.len() as u64);
            file.write_all(&prefix)
                .await
                .map_err(|e| DomainError::Storage(format!("写入临时音频缓存失败: {e}")))?;
        }

        while let Some(chunk) = resp
            .chunk()
            .await
            .map_err(|e| DomainError::Validation(format!("读取上游音频流失败: {e}")))?
        {
            if chunk.is_empty() {
                continue;
            }
            total_bytes = total_bytes.saturating_add(chunk.len() as u64);
            if total_bytes > MAX_TRACKED_AUDIO_BYTES {
                return Err(DomainError::Validation(format!(
                    "在线音频体积超过上限 ({} MiB)",
                    MAX_TRACKED_AUDIO_BYTES / (1024 * 1024)
                )));
            }
            file.write_all(&chunk)
                .await
                .map_err(|e| DomainError::Storage(format!("写入临时音频缓存失败: {e}")))?;
        }

        file.flush()
            .await
            .map_err(|e| DomainError::Storage(format!("刷新临时音频缓存失败: {e}")))?;
        drop(file);

        if total_bytes == 0 {
            return Err(DomainError::Validation("上游返回的音频内容为空".into()));
        }

        let ct_opt = (!raw_ct.is_empty()).then_some(raw_ct);
        Ok((temp, ct_opt, total_bytes))
    }

    async fn track_single_inner(
        &self,
        actor: &User,
        deps: &MusicTrackingDeps,
        managed_lib_id: xmedia_core::domain::library::LibraryId,
        existing_tracks: &mut Vec<Track>,
        input: TrackOnlineMusicInput,
    ) -> DomainResult<(Track, bool)> {
        let title = input.title.trim().to_string();
        if title.is_empty() {
            return Err(DomainError::Validation("曲目标题不能为空".into()));
        }
        let _src_rec = self.load_source_for_actor(actor, input.source_id).await?;
        let item_id = canonical_item_id(&input);

        let track_lock =
            get_or_insert_weak_lock(&self.track_locks, (input.source_id, item_id.clone()));
        let _track_guard = track_lock.lock().await;

        let fresh_existing = deps
            .catalog_repo
            .list_tracks_by_library(managed_lib_id)
            .await?;
        *existing_tracks = fresh_existing;

        if let Some(existing) = existing_tracks.iter().find(|tr| {
            matches!(
                &tr.source,
                TrackSource::OnlineTracked(info)
                    if info.source_id == input.source_id && info.item_id == item_id
            )
        }) {
            return Ok((existing.clone(), false));
        }

        let artist_name = {
            let a = input.artist.trim();
            if a.is_empty() {
                "Unknown Artist".to_string()
            } else {
                a.to_string()
            }
        };
        let album_title = input
            .album
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("Unknown Album")
            .to_string();

        let track_id = TrackId::new();
        let mut cover_url = clean_opt(input.cover_url.clone());
        if cover_url.is_none()
            && input
                .cover_id
                .as_deref()
                .is_some_and(|s| !s.trim().is_empty())
        {
            let ctx = MusicTrackContext {
                track_id: item_id.clone(),
                title: title.clone(),
                artist: artist_name.clone(),
                album: Some(album_title.clone()),
                duration_secs: input.duration_secs,
                cover_id: clean_opt(input.cover_id.clone()),
                detail_url: clean_opt(input.detail_url.clone()),
                ..Default::default()
            };
            cover_url = self.resolve_cover_url_internal(input.source_id, ctx).await;
        }

        let cover_key = self
            .fetch_and_save_cover_best_effort(
                deps,
                input.source_id,
                &track_id.0.to_string(),
                cover_url.as_deref(),
            )
            .await;

        let artist = deps
            .ingest_service
            .ensure_artist(&artist_name, true, true, cover_key.as_deref())
            .await?;
        let album = deps
            .ingest_service
            .ensure_album(
                managed_lib_id,
                &album_title,
                artist.id,
                &artist.name,
                None,
                None,
                false,
                cover_key.as_deref(),
            )
            .await?;

        let final_cover = cover_key.or_else(|| album.cover_key.clone());
        let format = input
            .format
            .as_deref()
            .and_then(|f| AudioFormat::parse(f).ok())
            .or_else(|| {
                input
                    .play_url
                    .as_deref()
                    .and_then(infer_audio_format)
                    .and_then(|ext| AudioFormat::parse(&ext).ok())
            })
            .unwrap_or(AudioFormat::Mp3);

        let now = Timestamp::now();
        let track = Track {
            id: track_id,
            library_id: managed_lib_id,
            album_id: album.id,
            artist_id: artist.id,
            album_artist_id: artist.id,
            title,
            artist_name: artist.name.clone(),
            album_artist_name: artist.name,
            album_title: album.title,
            track_number: None,
            disc_number: None,
            year: None,
            genre: None,
            duration_secs: input.duration_secs.unwrap_or(0),
            bit_rate_kbps: None,
            sample_rate_hz: None,
            bit_depth: None,
            channels: None,
            format,
            file_size: 0,
            file_hash: format!("online:{}:{item_id}", input.source_id),
            replay_gain: ReplayGain::default(),
            has_embedded_lyrics: false,
            has_sidecar_lyrics: false,
            is_compilation: false,
            cover_key: final_cover,
            source: TrackSource::OnlineTracked(Box::new(OnlineTrackedTrackSource {
                storage_key: None,
                source_id: input.source_id,
                item_id,
                detail_url: clean_opt(input.detail_url),
                play_url: clean_opt(input.play_url),
                url_id: clean_opt(input.url_id),
                cover_url,
                cover_id: clean_opt(input.cover_id),
                lyric_id: clean_opt(input.lyric_id),
                quality: clean_opt(input.quality),
                cached: false,
                sidecar_lrc_key: None,
            })),
            is_missing: false,
            created_at: now,
            updated_at: now,
        };

        deps.catalog_repo.save_track(&track).await?;
        existing_tracks.push(track.clone());
        Ok((track, true))
    }

    pub async fn track_online_track(
        &self,
        actor: &User,
        input: TrackOnlineMusicInput,
    ) -> DomainResult<TrackDto> {
        let deps = self.require_tracking()?;
        let managed_lib = deps.ingest_service.ensure_default_managed_library().await?;
        let mut existing = deps
            .catalog_repo
            .list_tracks_by_library(managed_lib.id)
            .await?;

        let (track, created) = self
            .track_single_inner(actor, deps, managed_lib.id, &mut existing, input)
            .await?;

        if created {
            deps.catalog_repo.recompute_catalog_aggregates().await?;
            deps.events.publish(SystemEvent::MediaChanged {
                media_kind: MediaKind::Music,
            });
        }
        deps.catalog_service
            .bind_track_for_user(actor.id, track.id)
            .await?;

        let mut list = deps
            .catalog_service
            .enrich_tracks(actor.id, vec![track])
            .await?;
        list.pop()
            .ok_or_else(|| DomainError::NotFound("track not found".into()))
    }

    pub async fn track_online_tracks_batch(
        &self,
        actor: &User,
        inputs: Vec<TrackOnlineMusicInput>,
    ) -> DomainResult<Vec<TrackDto>> {
        if inputs.is_empty() {
            return Err(DomainError::Validation("请至少选择一首曲目".into()));
        }
        if inputs.len() > MAX_BATCH_TRACK_COUNT {
            return Err(DomainError::Validation(format!(
                "单次最多批量加入 {MAX_BATCH_TRACK_COUNT} 首曲目"
            )));
        }

        let deps = self.require_tracking()?;
        let managed_lib = deps.ingest_service.ensure_default_managed_library().await?;
        let mut existing = deps
            .catalog_repo
            .list_tracks_by_library(managed_lib.id)
            .await?;

        let mut tracks = Vec::with_capacity(inputs.len());
        let mut any_created = false;
        for item in inputs {
            let (track, created) = self
                .track_single_inner(actor, deps, managed_lib.id, &mut existing, item)
                .await?;
            any_created |= created;
            tracks.push(track);
        }

        if any_created {
            deps.catalog_repo.recompute_catalog_aggregates().await?;
            deps.events.publish(SystemEvent::MediaChanged {
                media_kind: MediaKind::Music,
            });
        }
        let track_ids: Vec<TrackId> = tracks.iter().map(|t| t.id).collect();
        deps.catalog_service
            .bind_tracks_for_user(actor.id, &track_ids)
            .await?;

        deps.catalog_service.enrich_tracks(actor.id, tracks).await
    }

    pub async fn cache_tracked_track(
        &self,
        actor: &User,
        track_id: TrackId,
    ) -> DomainResult<TrackDto> {
        let deps = self.require_tracking()?;
        deps.catalog_service
            .require_track_bound(actor.id, track_id)
            .await?;
        self.ensure_tracked_track_cached(track_id).await?;
        deps.catalog_service.get_track(actor.id, track_id).await
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
        if let TrackSource::OnlineTracked(ref mut info) = track.source {
            info.sidecar_lrc_key = Some(saved_lrc_key);
        }
        track.has_sidecar_lyrics = true;
        track.updated_at = Timestamp::now();
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

        let rec = self.load_source(source_id).await?;
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

        let (temp_file, resp_ct, _downloaded_bytes) = self
            .download_audio_to_temp_file(
                source_id,
                &resolved_url,
                &stream_out.data.headers,
                &initial_ext,
            )
            .await?;

        let probe_ext = resp_ct
            .as_deref()
            .and_then(infer_audio_ext_from_content_type)
            .map(ToString::to_string)
            .unwrap_or(initial_ext);

        let probe = deps.probe.clone();
        let probe_path = temp_file.path().to_string_lossy().into_owned();
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
            .save_managed_audio_from_path(track.id, final_format, temp_file.path())
            .await?;

        if let Some(ref probed) = probed_opt {
            if probed.duration_secs > 0 {
                track.duration_secs = probed.duration_secs;
            }
            if probed.bit_rate_kbps.is_some() {
                track.bit_rate_kbps = probed.bit_rate_kbps;
            }
            if probed.sample_rate_hz.is_some() {
                track.sample_rate_hz = probed.sample_rate_hz;
            }
            if probed.bit_depth.is_some() {
                track.bit_depth = probed.bit_depth;
            }
            if probed.channels.is_some() {
                track.channels = probed.channels;
            }
            if probed.year.is_some() && track.year.is_none() {
                track.year = probed.year;
            }
            if probed.genre.is_some() && track.genre.is_none() {
                track.genre = probed.genre.clone();
            }
            track.replay_gain = probed.replay_gain.clone();
            track.has_embedded_lyrics = probed.has_embedded_lyrics;
            if track.cover_key.is_none()
                && let Some(ref embedded_cover) = probed.cover
                && let Ok(key) = deps
                    .storage
                    .save_cover(&track.id.0.to_string(), embedded_cover)
                    .await
            {
                track.cover_key = Some(key);
            }
        } else if let Some(br) = stream_out.data.bitrate {
            track.bit_rate_kbps = Some(br);
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
                track.cover_key = Some(key);
            }
        }

        let mut final_lrc_key = existing_lrc_key;
        if final_lrc_key.is_none()
            && !track.has_embedded_lyrics
            && let Some(lrc_text) = self.resolve_lyrics_internal(source_id, ctx).await
            && let Ok(key) = deps
                .storage
                .save_managed_sidecar_lrc(track.id, Bytes::from(lrc_text.into_bytes()))
                .await
        {
            final_lrc_key = Some(key);
            track.has_sidecar_lyrics = true;
        }

        track.format = final_format;
        track.file_size = stored.file_size;
        track.file_hash = stored.file_hash;
        track.source = TrackSource::OnlineTracked(Box::new(OnlineTrackedTrackSource {
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
        }));
        track.updated_at = Timestamp::now();

        deps.catalog_repo.save_track(&track).await?;

        if let Some(ref cov) = track.cover_key {
            if let Ok(Some(mut album)) = deps.catalog_repo.find_album_by_id(track.album_id).await
                && album.cover_key.is_none()
            {
                album.cover_key = Some(cov.clone());
                album.updated_at = track.updated_at;
                let _ = deps.catalog_repo.save_album(&album).await;
            }
            if let Ok(Some(mut artist)) = deps.catalog_repo.find_artist_by_id(track.artist_id).await
                && artist.cover_key.is_none()
            {
                artist.cover_key = Some(cov.clone());
                artist.updated_at = track.updated_at;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn temp_file_guard_creates_exclusive_owner_only_temp_file_and_cleans_up() {
        let (guard, mut file) = TempFileGuard::create_exclusive("mp3").await.unwrap();
        let path = guard.path().to_path_buf();
        file.write_all(b"audio-bytes").await.unwrap();
        drop(file);
        assert!(path.is_file());

        // Exclusive open on an existing path must fail with AlreadyExists
        let err = open_exclusive_temp_file(&path).await.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }

        drop(guard);
        assert!(!path.exists());
    }
}

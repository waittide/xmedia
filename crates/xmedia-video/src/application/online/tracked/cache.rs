use std::collections::HashSet;
use std::path::Path;

use bytes::Bytes;
use jiff::Timestamp;
use tokio::io::AsyncWriteExt;
use url::Url;
use uuid::Uuid;
use xmedia_core::application::ports::SystemEvent;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;
use xmedia_core::domain::user::User;

use crate::application::dto::VideoSummaryDto;
use crate::domain::id::{ShowId, VideoId};
use crate::domain::video::{SidecarSubtitle, VideoContainer};

use super::super::dto::VideoCacheReportDto;
use super::super::{VideoOnlineService, VideoTrackingDeps, get_or_insert_weak_lock};
use super::hls_stage::{
    TempDirGuard, TempFileGuard, open_exclusive_temp_file, stage_local_hls_playlist_with,
};

const MAX_TRACKED_VIDEO_BYTES: u64 = 4 * 1024 * 1024 * 1024;

fn pick_best_hls_variant(playlist: &str, base_url: &Url) -> Option<String> {
    let mut best: Option<(u64, String)> = None;
    let mut pending_bw: Option<u64> = None;

    for raw_line in playlist.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("#EXT-X-STREAM-INF:") {
            let bw = rest
                .split(',')
                .find_map(|part| {
                    let (k, v) = part.split_once('=')?;
                    k.trim()
                        .eq_ignore_ascii_case("BANDWIDTH")
                        .then(|| v.trim().parse::<u64>().ok())
                        .flatten()
                })
                .unwrap_or(0);
            pending_bw = Some(bw);
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        if let Some(bw) = pending_bw.take()
            && let Ok(joined) = base_url.join(line)
            && matches!(joined.scheme(), "http" | "https")
        {
            let replace = best.as_ref().is_none_or(|(prev_bw, _)| bw >= *prev_bw);
            if replace {
                best = Some((bw, joined.to_string()));
            }
        }
    }
    best.map(|(_, url)| url)
}

impl VideoOnlineService {
    async fn download_direct_video_to_temp(
        &self,
        source_id: Uuid,
        stream_url: &str,
    ) -> DomainResult<(TempFileGuard, VideoContainer)> {
        let source_headers = self
            .resolve_internal_media_headers(Some(source_id), stream_url.trim())
            .await;
        let (temp, precreated) = TempFileGuard::create_exclusive("vid").await?;
        drop(precreated);
        let container = self
            .transport
            .download_direct_video_to_path(
                stream_url,
                source_headers,
                temp.path(),
                MAX_TRACKED_VIDEO_BYTES,
            )
            .await?;
        Ok((temp, container))
    }

    async fn fetch_hls_text_for_source(
        &self,
        source_id: Uuid,
        raw_url: &str,
    ) -> DomainResult<(String, Url)> {
        let headers = self
            .resolve_internal_media_headers(Some(source_id), raw_url.trim())
            .await;
        self.transport
            .fetch_hls_playlist_text(raw_url, headers)
            .await
    }

    async fn download_hls_asset_to_file(
        &self,
        source_id: Uuid,
        raw_url: &str,
        dest_path: &Path,
        total_bytes: u64,
    ) -> DomainResult<u64> {
        let headers = self
            .resolve_internal_media_headers(Some(source_id), raw_url.trim())
            .await;
        self.transport
            .download_hls_asset_to_path(
                raw_url,
                headers,
                dest_path,
                total_bytes,
                MAX_TRACKED_VIDEO_BYTES,
            )
            .await
    }

    async fn download_and_remux_hls_to_temp(
        &self,
        deps: &VideoTrackingDeps,
        source_id: Uuid,
        hls_url: &str,
    ) -> DomainResult<(TempFileGuard, VideoContainer)> {
        let ff_status = deps.media_engine.check_status().await;
        if !ff_status.ffmpeg_available {
            return Err(DomainError::Validation(
                "缓存 HLS 在线视频需要系统安装 FFmpeg".into(),
            ));
        }

        let (mut playlist_text, mut base_url) =
            self.fetch_hls_text_for_source(source_id, hls_url).await?;
        if playlist_text.contains("#EXT-X-STREAM-INF:")
            && let Some(variant_url) = pick_best_hls_variant(&playlist_text, &base_url)
        {
            let (variant_text, variant_base) = self
                .fetch_hls_text_for_source(source_id, &variant_url)
                .await?;
            playlist_text = variant_text;
            base_url = variant_base;
        }

        let work_dir = TempDirGuard::new()?;
        let local_lines = stage_local_hls_playlist_with(
            &playlist_text,
            &base_url,
            |resolved_url, local_fname, bytes_so_far| {
                let dest = work_dir.path().join(&local_fname);
                let url_owned = resolved_url.to_string();
                async move {
                    self.download_hls_asset_to_file(source_id, &url_owned, &dest, bytes_so_far)
                        .await
                }
            },
        )
        .await?;

        let local_m3u8 = work_dir.path().join("index.m3u8");
        {
            let mut m3u8_file = open_exclusive_temp_file(&local_m3u8)
                .await
                .map_err(|e| DomainError::Storage(format!("创建本地 HLS 索引失败: {e}")))?;
            m3u8_file
                .write_all(local_lines.join("\n").as_bytes())
                .await
                .map_err(|e| DomainError::Storage(format!("写入本地 HLS 索引失败: {e}")))?;
            m3u8_file
                .flush()
                .await
                .map_err(|e| DomainError::Storage(format!("刷新本地 HLS 索引失败: {e}")))?;
        }

        let (out_mp4, precreated) = TempFileGuard::create_exclusive("mp4").await?;
        drop(precreated);
        deps.media_engine
            .remux_local_hls_to_mp4(&local_m3u8, out_mp4.path())
            .await?;

        Ok((out_mp4, VideoContainer::Mp4))
    }

    async fn download_subtitle_vtt_best_effort(
        &self,
        source_id: Uuid,
        raw_url: &str,
    ) -> Option<String> {
        let headers = self
            .resolve_internal_media_headers(Some(source_id), raw_url.trim())
            .await;
        self.transport
            .fetch_subtitle_vtt(raw_url, &headers)
            .await
            .ok()
    }

    pub async fn cache_tracked_video(
        &self,
        actor: &User,
        video_id: VideoId,
    ) -> DomainResult<VideoSummaryDto> {
        let deps = self.require_tracking()?;
        deps.catalog_service
            .require_video_bound(actor.id, video_id)
            .await?;

        let lock = get_or_insert_weak_lock(&self.video_cache_locks, video_id);
        let _guard = lock.lock().await;

        let mut video = deps
            .catalog_repo
            .find_video_by_id(video_id)
            .await?
            .filter(|v| !v.is_missing)
            .ok_or_else(|| DomainError::NotFound("video not found".into()))?;
        let library = deps
            .libraries
            .find_by_id(video.library_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("library not found".into()))?;

        if video.source.is_cached() {
            let file_exists = deps
                .storage
                .resolve_video_fs_path(&library, &video)
                .await
                .is_ok_and(|p| Path::new(&p).is_file());
            if file_exists {
                let mut enriched = deps
                    .catalog_service
                    .enrich_videos(actor.id, vec![video])
                    .await?;
                return enriched
                    .pop()
                    .ok_or_else(|| DomainError::NotFound("video not found".into()));
            }
        }

        let tracking_info = video
            .source
            .online_tracking_info()
            .ok_or_else(|| DomainError::Validation("该视频不是在线追更资源".into()))?
            .clone();

        let (source_id, play_res) = self.resolve_tracked_video_upstream(video_id).await?;
        let is_hls = play_res.stream_type.eq_ignore_ascii_case("hls")
            || play_res.stream_url.to_ascii_lowercase().contains(".m3u8");

        let (temp_file, container) = if is_hls {
            self.download_and_remux_hls_to_temp(deps, source_id, &play_res.stream_url)
                .await?
        } else {
            self.download_direct_video_to_temp(source_id, &play_res.stream_url)
                .await?
        };

        let stored = deps
            .storage
            .save_managed_video_from_path(video.id, container, temp_file.path())
            .await?;

        let probed = deps
            .media_engine
            .probe_video(Path::new(&stored.absolute_path))
            .await;

        let mut sidecar_subtitles = tracking_info.sidecar_subtitles.clone();
        if sidecar_subtitles.is_empty() && !play_res.subtitles.is_empty() {
            for (idx, sub) in play_res.subtitles.iter().enumerate() {
                if let Some(vtt) = self
                    .download_subtitle_vtt_best_effort(source_id, &sub.url)
                    .await
                {
                    let sub_id = format!("s{idx}");
                    if let Ok(key) = deps
                        .storage
                        .save_managed_subtitle(
                            video.id,
                            &sub_id,
                            "vtt",
                            Bytes::from(vtt.into_bytes()),
                        )
                        .await
                    {
                        sidecar_subtitles.push(SidecarSubtitle {
                            id: sub_id,
                            format: "vtt".into(),
                            language: sub.lang.clone(),
                            label: sub.label.clone(),
                            path_or_key: key,
                        });
                    }
                }
            }
        }

        let fallback_poster_key = if video.poster_key.is_none() {
            match deps
                .media_engine
                .generate_poster_bytes(
                    Path::new(&stored.absolute_path),
                    probed.duration_secs,
                    &video.title,
                )
                .await
            {
                Ok(frame_bytes) => deps
                    .storage
                    .save_generated_poster_bytes(&frame_bytes)
                    .await
                    .ok(),
                Err(_) => None,
            }
        } else {
            None
        };

        video.mark_online_cached(
            container,
            stored.storage_key,
            stored.file_size,
            stored.file_hash,
            probed.duration_secs,
            probed.bit_rate_kbps,
            probed.width,
            probed.height,
            probed.video_codec,
            probed.audio_streams,
            probed.subtitle_streams,
            sidecar_subtitles,
            fallback_poster_key,
            Timestamp::now(),
        )?;

        deps.catalog_repo.save_video(&video).await?;
        if video.show_id.is_some() {
            deps.catalog_repo.recompute_show_aggregates().await?;
        }
        deps.events.publish(SystemEvent::MediaChanged {
            media_kind: MediaKind::Video,
        });

        let mut enriched = deps
            .catalog_service
            .enrich_videos(actor.id, vec![video])
            .await?;
        enriched
            .pop()
            .ok_or_else(|| DomainError::NotFound("video not found".into()))
    }

    pub async fn cache_tracked_show(
        &self,
        actor: &User,
        show_id: ShowId,
        video_ids: Option<Vec<VideoId>>,
    ) -> DomainResult<VideoCacheReportDto> {
        let deps = self.require_tracking()?;
        deps.catalog_service
            .require_show_bound(actor.id, show_id)
            .await?;

        let _show = deps
            .catalog_repo
            .find_show_by_id(show_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("剧集不存在".into()))?;
        let episodes = deps.catalog_repo.list_episodes_by_show(show_id).await?;
        let filter_set: Option<HashSet<VideoId>> = video_ids
            .filter(|v| !v.is_empty())
            .map(|v| v.into_iter().collect());

        let mut cached_count = 0usize;
        let mut skipped_count = 0usize;
        let mut failed_count = 0usize;
        let mut errors = Vec::new();

        for ep in episodes {
            if let Some(ref set) = filter_set
                && !set.contains(&ep.id)
            {
                continue;
            }
            if !ep.source.is_online_tracked() || ep.source.is_cached() {
                skipped_count += 1;
                continue;
            }
            match self.cache_tracked_video(actor, ep.id).await {
                Ok(_) => cached_count += 1,
                Err(e) => {
                    failed_count += 1;
                    errors.push(format!("{}：{}", ep.title, e));
                }
            }
        }

        Ok(VideoCacheReportDto {
            cached_count,
            skipped_count,
            failed_count,
            errors,
        })
    }
}

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use bytes::Bytes;
use jiff::Timestamp;
use reqwest::header::{ACCEPT_ENCODING, CONTENT_TYPE, HeaderValue, USER_AGENT};
use tokio::io::AsyncWriteExt;
use url::Url;
use uuid::Uuid;
use xmedia_core::application::ports::SystemEvent;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;
use xmedia_core::domain::user::User;
use xmedia_source::http::{DEFAULT_WEBVIEW_UA, build_sanitized_headers, decode_response_bytes};
use xmedia_source::proxy::{
    DEFAULT_IMAGE_PROXY_MAX_BYTES, fetch_validated_image_bytes_with_config,
};

use crate::application::dto::{ShowDetailDto, VideoSummaryDto};
use crate::domain::id::{ShowId, VideoId};
use crate::domain::video::{
    OnlineTrackedVideoSource, SidecarSubtitle, VideoContainer, VideoItem, VideoShow, VideoSource,
    normalize_string_list,
};
use crate::source::hls::{
    MAX_M3U8_BYTES, MAX_SUBTITLE_BYTES, SNIFF_PREFIX_MAX_BYTES, build_hls_proxy_url,
    build_stream_proxy_url, build_subtitle_proxy_url, is_forbidden_media_ct,
    is_hls_playlist_response, looks_like_html_bytes, read_sniff_prefix_with_context,
    send_streaming_get, validate_and_normalize_subtitle_to_vtt, validate_streaming_target_url,
};
use crate::source::model::{OnlineVideoPlayGroup, OnlineVideoPlayResult, VideoSourceSpec};

use super::dto::{
    TrackOnlineVideoInput, TrackOnlineVideoResultDto, TrackedVideoPlayResponseDto,
    VideoCacheReportDto,
};
use super::{CancelOnDrop, VideoOnlineService, VideoTrackingDeps, get_or_insert_weak_lock};

const MAX_TRACKED_VIDEO_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAX_HLS_SEGMENTS: usize = 10_000;

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
        let clean: String = ext
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .take(8)
            .collect();
        let suffix = if clean.is_empty() {
            "mp4".to_string()
        } else {
            clean
        };
        for _ in 0..8 {
            let path =
                std::env::temp_dir().join(format!("xmedia-video-{}.{suffix}", Uuid::now_v7()));
            match open_exclusive_temp_file(&path).await {
                Ok(file) => return Ok((Self { path }, file)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => {
                    return Err(DomainError::Storage(format!(
                        "创建临时视频缓存文件失败: {e}"
                    )));
                }
            }
        }
        Err(DomainError::Storage(
            "创建独占临时视频缓存文件失败: 重试次数耗尽".into(),
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

struct TempDirGuard {
    path: PathBuf,
}

impl TempDirGuard {
    fn new() -> DomainResult<Self> {
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        for _ in 0..8 {
            let path = std::env::temp_dir().join(format!("xmedia-hls-{}", Uuid::now_v7()));
            match builder.create(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => {
                    return Err(DomainError::Storage(format!("创建 HLS 临时目录失败: {e}")));
                }
            }
        }
        Err(DomainError::Storage(
            "创建独占 HLS 临时目录失败: 重试次数耗尽".into(),
        ))
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn parse_year_opt(raw: Option<&str>) -> Option<i32> {
    let s = raw?.trim();
    let digits: String = s.chars().filter(|c| c.is_ascii_digit()).take(4).collect();
    if digits.len() == 4 {
        digits
            .parse::<i32>()
            .ok()
            .filter(|&y| (1880..=2150).contains(&y))
    } else {
        None
    }
}

fn select_play_group<'a>(
    groups: &'a [OnlineVideoPlayGroup],
    preferred_name: Option<&str>,
) -> Option<&'a OnlineVideoPlayGroup> {
    if let Some(pref) = preferred_name.map(str::trim).filter(|s| !s.is_empty())
        && let Some(found) = groups
            .iter()
            .find(|g| !g.episodes.is_empty() && g.name.trim().eq_ignore_ascii_case(pref))
    {
        return Some(found);
    }
    groups.iter().find(|g| !g.episodes.is_empty())
}

fn infer_container_from_url_and_ct(url: &Url, content_type: Option<&str>) -> VideoContainer {
    if let Some(ct) = content_type {
        let base = ct
            .split(';')
            .next()
            .unwrap_or(ct)
            .trim()
            .to_ascii_lowercase();
        match base.as_str() {
            "video/mp4" | "application/mp4" => return VideoContainer::Mp4,
            "video/x-matroska" => return VideoContainer::Mkv,
            "video/webm" => return VideoContainer::Webm,
            "video/quicktime" => return VideoContainer::Mov,
            "video/x-msvideo" => return VideoContainer::Avi,
            "video/mp2t" => return VideoContainer::Ts,
            "video/x-flv" => return VideoContainer::Flv,
            _ => {}
        }
    }
    if let Some(ext) = Path::new(url.path()).extension().and_then(|s| s.to_str())
        && let Some(c) = VideoContainer::from_extension(ext)
    {
        return c;
    }
    VideoContainer::Mp4
}

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

fn is_detail_url_like(raw: &str) -> bool {
    let t = raw.trim();
    t.starts_with("http://") || t.starts_with("https://") || t.starts_with('/')
}

impl VideoOnlineService {
    fn require_tracking(&self) -> DomainResult<&VideoTrackingDeps> {
        self.tracking
            .as_ref()
            .ok_or_else(|| DomainError::Storage("影视追更存储依赖未初始化".into()))
    }

    async fn fetch_and_save_poster_best_effort(
        &self,
        deps: &VideoTrackingDeps,
        source_id: Uuid,
        cover_url: Option<&str>,
    ) -> Option<String> {
        let url = cover_url
            .map(str::trim)
            .filter(|u| u.starts_with("http://") || u.starts_with("https://"))?;
        let headers = self
            .resolve_internal_media_headers(Some(source_id), url)
            .await;
        let (bytes, _mime) = fetch_validated_image_bytes_with_config(
            url,
            (!headers.is_empty()).then_some(&headers),
            DEFAULT_IMAGE_PROXY_MAX_BYTES,
            &self.http_config,
        )
        .await
        .ok()?;
        deps.storage.save_poster_bytes(&bytes).await.ok()
    }

    pub async fn track_online_video(
        &self,
        actor: &User,
        input: TrackOnlineVideoInput,
    ) -> DomainResult<TrackOnlineVideoResultDto> {
        let deps = self.require_tracking()?;

        let detail_resp = self
            .detail(
                actor,
                input.source_id,
                input.video_id.as_deref(),
                input.detail_url.as_deref(),
            )
            .await?;
        let detail = detail_resp.detail;
        let selected_group = select_play_group(&detail.play_groups, input.group_name.as_deref())
            .ok_or_else(|| DomainError::Validation("该在线影视没有可播放的剧集列表".into()))?;

        let managed_lib = deps.lifecycle.ensure_managed_library().await?;
        let title = detail.title.trim().to_string();
        if title.is_empty() {
            return Err(DomainError::Validation("影视标题不能为空".into()));
        }

        let canonical_video_id = {
            let d_id = detail.id.trim();
            if !d_id.is_empty() {
                d_id.to_string()
            } else if let Some(v) = input
                .video_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                v.to_string()
            } else if let Some(u) = input
                .detail_url
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                u.to_string()
            } else {
                title.clone()
            }
        };
        let canonical_detail_url = input
            .detail_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string)
            .unwrap_or_else(|| canonical_video_id.clone());

        let year = parse_year_opt(detail.year.as_deref());
        let summary = detail.intro.clone().unwrap_or_default().trim().to_string();
        let genres = detail
            .area
            .as_deref()
            .map(|a| {
                normalize_string_list(a.split([',', '，', '/', ' ']).map(str::to_string).collect())
            })
            .unwrap_or_default();

        let poster_key = self
            .fetch_and_save_poster_best_effort(deps, input.source_id, detail.cover_url.as_deref())
            .await;

        let track_lock = get_or_insert_weak_lock(
            &self.track_locks,
            (input.source_id, canonical_video_id.clone()),
        );
        let _track_guard = track_lock.lock().await;

        let now = Timestamp::now();

        if selected_group.episodes.len() == 1 {
            let ep = &selected_group.episodes[0];
            let existing_videos = deps
                .catalog_repo
                .list_videos_by_library(managed_lib.id)
                .await?;
            if let Some(existing) = existing_videos.into_iter().find(|v| {
                v.show_id.is_none()
                    && v.source.online_tracking_info().is_some_and(|info| {
                        info.source_id == input.source_id
                            && (info.video_id == canonical_video_id || info.play_url == ep.play_url)
                    })
            }) {
                deps.catalog_service
                    .bind_entity_for_user(actor.id, existing.id.0)
                    .await?;
                let mut enriched = deps
                    .catalog_service
                    .enrich_videos(actor.id, vec![existing])
                    .await?;
                return Ok(TrackOnlineVideoResultDto {
                    kind: "movie".into(),
                    video: enriched.pop(),
                    show: None,
                    added_episodes: 0,
                });
            }

            let video_id = VideoId::new();
            let item = VideoItem {
                id: video_id,
                library_id: managed_lib.id,
                show_id: None,
                show_title: None,
                season_number: None,
                episode_number: None,
                title,
                summary,
                year,
                genres,
                tags: Vec::new(),
                container: VideoContainer::Mp4,
                file_size: 0,
                file_hash: format!("online:{}:{}", input.source_id, canonical_video_id),
                duration_secs: 0,
                bit_rate_kbps: None,
                width: None,
                height: None,
                video_codec: None,
                audio_streams: Vec::new(),
                subtitle_streams: Vec::new(),
                poster_key,
                source: VideoSource::OnlineTracked(Box::new(OnlineTrackedVideoSource {
                    storage_key: None,
                    source_id: input.source_id,
                    video_id: canonical_video_id,
                    detail_url: canonical_detail_url,
                    play_url: ep.play_url.clone(),
                    group_name: Some(selected_group.name.clone()),
                    episode_name: Some(ep.name.clone()),
                    cached: false,
                    sidecar_subtitles: Vec::new(),
                })),
                is_missing: false,
                created_at: now,
                updated_at: now,
            };
            deps.catalog_repo.save_video(&item).await?;
            deps.catalog_service
                .bind_entity_for_user(actor.id, item.id.0)
                .await?;
            deps.events.publish(SystemEvent::MediaChanged {
                media_kind: MediaKind::Video,
            });
            let mut enriched = deps
                .catalog_service
                .enrich_videos(actor.id, vec![item])
                .await?;
            return Ok(TrackOnlineVideoResultDto {
                kind: "movie".into(),
                video: enriched.pop(),
                show: None,
                added_episodes: 1,
            });
        }

        let show_key = VideoShow::compute_online_tracked_key(input.source_id, &canonical_video_id);
        let show =
            if let Some(mut existing_show) = deps.catalog_repo.find_show_by_key(&show_key).await? {
                let mut dirty = false;
                if existing_show.poster_key.is_none() && poster_key.is_some() {
                    existing_show.poster_key = poster_key.clone();
                    dirty = true;
                }
                if existing_show.summary.is_empty() && !summary.is_empty() {
                    existing_show.summary = summary.clone();
                    dirty = true;
                }
                if existing_show.year.is_none() && year.is_some() {
                    existing_show.year = year;
                    dirty = true;
                }
                if dirty {
                    existing_show.updated_at = now;
                    deps.catalog_repo.save_show(&existing_show).await?;
                }
                existing_show
            } else {
                let created = VideoShow {
                    id: ShowId::new(),
                    library_id: managed_lib.id,
                    show_key,
                    title: title.clone(),
                    summary: summary.clone(),
                    year,
                    genres: genres.clone(),
                    tags: Vec::new(),
                    poster_key: poster_key.clone(),
                    season_count: 1,
                    episode_count: 0,
                    created_at: now,
                    updated_at: now,
                };
                deps.catalog_repo.save_show(&created).await?;
                created
            };

        let existing_eps = deps.catalog_repo.list_episodes_by_show(show.id).await?;
        let existing_play_urls: HashSet<String> = existing_eps
            .iter()
            .filter_map(|e| e.source.online_tracking_info().map(|i| i.play_url.clone()))
            .collect();
        let mut next_ep_num = existing_eps
            .iter()
            .filter_map(|e| e.episode_number)
            .max()
            .unwrap_or(0);

        let mut added_episodes = 0usize;
        for (idx, ep) in selected_group.episodes.iter().enumerate() {
            if existing_play_urls.contains(&ep.play_url) {
                continue;
            }
            next_ep_num = if existing_eps.is_empty() {
                (idx + 1) as u32
            } else {
                next_ep_num + 1
            };
            let ep_title = {
                let t = ep.name.trim();
                if t.is_empty() {
                    format!("第 {next_ep_num} 集")
                } else {
                    t.to_string()
                }
            };
            let item = VideoItem {
                id: VideoId::new(),
                library_id: managed_lib.id,
                show_id: Some(show.id),
                show_title: Some(show.title.clone()),
                season_number: Some(1),
                episode_number: Some(next_ep_num),
                title: ep_title.clone(),
                summary: String::new(),
                year,
                genres: genres.clone(),
                tags: Vec::new(),
                container: VideoContainer::Mp4,
                file_size: 0,
                file_hash: format!(
                    "online:{}:{}:{next_ep_num}",
                    input.source_id, canonical_video_id
                ),
                duration_secs: 0,
                bit_rate_kbps: None,
                width: None,
                height: None,
                video_codec: None,
                audio_streams: Vec::new(),
                subtitle_streams: Vec::new(),
                poster_key: show.poster_key.clone().or_else(|| poster_key.clone()),
                source: VideoSource::OnlineTracked(Box::new(OnlineTrackedVideoSource {
                    storage_key: None,
                    source_id: input.source_id,
                    video_id: canonical_video_id.clone(),
                    detail_url: canonical_detail_url.clone(),
                    play_url: ep.play_url.clone(),
                    group_name: Some(selected_group.name.clone()),
                    episode_name: Some(ep_title),
                    cached: false,
                    sidecar_subtitles: Vec::new(),
                })),
                is_missing: false,
                created_at: now,
                updated_at: now,
            };
            deps.catalog_repo.save_video(&item).await?;
            added_episodes += 1;
        }

        if added_episodes > 0 {
            deps.catalog_repo.recompute_show_aggregates().await?;
            deps.events.publish(SystemEvent::MediaChanged {
                media_kind: MediaKind::Video,
            });
        }

        deps.catalog_service
            .bind_entity_for_user(actor.id, show.id.0)
            .await?;
        let show_detail = deps.catalog_service.get_show_detail(actor, show.id).await?;
        Ok(TrackOnlineVideoResultDto {
            kind: "show".into(),
            video: None,
            show: Some(show_detail),
            added_episodes,
        })
    }

    pub async fn refresh_tracked_show(
        &self,
        actor: &User,
        show_id: ShowId,
    ) -> DomainResult<ShowDetailDto> {
        let deps = self.require_tracking()?;
        deps.catalog_service
            .require_show_bound(actor.id, show_id)
            .await?;

        let mut show = deps
            .catalog_repo
            .find_show_by_id(show_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("剧集不存在".into()))?;
        let (source_id, item_key) = show
            .online_tracked_ref()
            .ok_or_else(|| DomainError::Validation("该剧集不是在线追更资源".into()))?;
        let item_key = item_key.to_string();

        let existing_eps = deps.catalog_repo.list_episodes_by_show(show.id).await?;
        let first_info = existing_eps
            .iter()
            .find_map(|e| e.source.online_tracking_info());
        let video_id_str = first_info
            .map(|i| i.video_id.clone())
            .unwrap_or_else(|| item_key.clone());
        let stored_detail_url = first_info.map(|i| i.detail_url.clone());
        let preferred_group = first_info.and_then(|i| i.group_name.clone());

        let detail_url_opt = stored_detail_url
            .as_deref()
            .filter(|u| is_detail_url_like(u));

        let detail_resp = self
            .detail(actor, source_id, Some(&video_id_str), detail_url_opt)
            .await?;
        let detail = detail_resp.detail;
        let selected_group = select_play_group(&detail.play_groups, preferred_group.as_deref())
            .ok_or_else(|| DomainError::Validation("刷新失败：上游未返回剧集列表".into()))?;

        let now = Timestamp::now();
        let mut show_dirty = false;
        if show.poster_key.is_none()
            && let Some(pk) = self
                .fetch_and_save_poster_best_effort(deps, source_id, detail.cover_url.as_deref())
                .await
        {
            show.poster_key = Some(pk);
            show_dirty = true;
        }
        if show.summary.is_empty()
            && let Some(ref intro) = detail.intro
            && !intro.trim().is_empty()
        {
            show.summary = intro.trim().to_string();
            show_dirty = true;
        }
        if show_dirty {
            show.updated_at = now;
            deps.catalog_repo.save_show(&show).await?;
        }

        let existing_play_urls: HashSet<String> = existing_eps
            .iter()
            .filter_map(|e| e.source.online_tracking_info().map(|i| i.play_url.clone()))
            .collect();
        let mut next_ep_num = existing_eps
            .iter()
            .filter_map(|e| e.episode_number)
            .max()
            .unwrap_or(0);
        let canonical_detail_url = stored_detail_url.unwrap_or_else(|| video_id_str.clone());

        let mut added = 0usize;
        for ep in &selected_group.episodes {
            if existing_play_urls.contains(&ep.play_url) {
                continue;
            }
            next_ep_num += 1;
            let ep_title = {
                let t = ep.name.trim();
                if t.is_empty() {
                    format!("第 {next_ep_num} 集")
                } else {
                    t.to_string()
                }
            };
            let item = VideoItem {
                id: VideoId::new(),
                library_id: show.library_id,
                show_id: Some(show.id),
                show_title: Some(show.title.clone()),
                season_number: Some(1),
                episode_number: Some(next_ep_num),
                title: ep_title.clone(),
                summary: String::new(),
                year: show.year,
                genres: show.genres.clone(),
                tags: Vec::new(),
                container: VideoContainer::Mp4,
                file_size: 0,
                file_hash: format!("online:{source_id}:{video_id_str}:{next_ep_num}"),
                duration_secs: 0,
                bit_rate_kbps: None,
                width: None,
                height: None,
                video_codec: None,
                audio_streams: Vec::new(),
                subtitle_streams: Vec::new(),
                poster_key: show.poster_key.clone(),
                source: VideoSource::OnlineTracked(Box::new(OnlineTrackedVideoSource {
                    storage_key: None,
                    source_id,
                    video_id: video_id_str.clone(),
                    detail_url: canonical_detail_url.clone(),
                    play_url: ep.play_url.clone(),
                    group_name: Some(selected_group.name.clone()),
                    episode_name: Some(ep_title),
                    cached: false,
                    sidecar_subtitles: Vec::new(),
                })),
                is_missing: false,
                created_at: now,
                updated_at: now,
            };
            deps.catalog_repo.save_video(&item).await?;
            added += 1;
        }

        if added > 0 {
            deps.catalog_repo.recompute_show_aggregates().await?;
            deps.events.publish(SystemEvent::MediaChanged {
                media_kind: MediaKind::Video,
            });
        }

        deps.catalog_service.get_show_detail(actor, show.id).await
    }

    pub async fn resolve_tracked_video_upstream(
        &self,
        video_id: VideoId,
    ) -> DomainResult<(Uuid, OnlineVideoPlayResult)> {
        let deps = self.require_tracking()?;
        let video = deps
            .catalog_repo
            .find_video_by_id(video_id)
            .await?
            .filter(|v| !v.is_missing)
            .ok_or_else(|| DomainError::NotFound("video not found".into()))?;
        let info = video
            .source
            .online_tracking_info()
            .ok_or_else(|| DomainError::Validation("该视频不是在线追更资源".into()))?
            .clone();

        let rec = self.load_source(info.source_id).await?;
        let parsed: VideoSourceSpec = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("影视源 JSON 无效: {e}")))?;

        let op_timeout = self.op_timeout;
        self.acquire_rate_slot(info.source_id, parsed.parse_concurrent_rate(), op_timeout)
            .await
            .map_err(DomainError::Validation)?;

        let clean_play_url = xmedia_source::http::resolve_literal_http_url(
            &info.play_url,
            Some(parsed.effective_source_url()),
        )?;

        let engine = self.engine.clone();
        let prev_state = rec.runtime_state.clone();
        let prev_state_cmp = prev_state.clone();
        let play_owned = clean_play_url;
        let group_owned = info.group_name.clone();
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

        if out.state != prev_state_cmp {
            let _ = self
                .persist_runtime_state_with_base(info.source_id, &prev_state_cmp, out.state, false)
                .await;
        }

        Ok((info.source_id, out.data))
    }

    pub async fn resolve_tracked_video_play(
        &self,
        user: &User,
        video_id: VideoId,
        query_token: Option<&str>,
    ) -> DomainResult<TrackedVideoPlayResponseDto> {
        let deps = self.require_tracking()?;
        let video = deps
            .catalog_service
            .require_video_bound(user.id, video_id)
            .await?;

        if video.source.is_cached() {
            return Ok(TrackedVideoPlayResponseDto {
                video_id,
                cached: true,
                source_id: video.source.online_tracking_info().map(|i| i.source_id),
                stream_url: None,
                proxy_stream_url: None,
                stream_type: None,
                subtitles: Vec::new(),
            });
        }

        let (source_id, mut play) = self.resolve_tracked_video_upstream(video_id).await?;
        let proxy_stream_url = if play.stream_type == "hls" {
            build_hls_proxy_url(&play.stream_url, Some(source_id), query_token)
        } else {
            build_stream_proxy_url(&play.stream_url, Some(source_id), query_token)
        };
        for sub in &mut play.subtitles {
            let clean = sub.url.trim();
            if clean.starts_with("http://") || clean.starts_with("https://") {
                sub.proxy_url = Some(build_subtitle_proxy_url(
                    clean,
                    Some(source_id),
                    query_token,
                ));
            }
        }

        Ok(TrackedVideoPlayResponseDto {
            video_id,
            cached: false,
            source_id: Some(source_id),
            stream_url: Some(play.stream_url),
            proxy_stream_url: Some(proxy_stream_url),
            stream_type: Some(play.stream_type),
            subtitles: play.subtitles,
        })
    }

    async fn download_direct_video_to_temp(
        &self,
        source_id: Uuid,
        stream_url: &str,
    ) -> DomainResult<(TempFileGuard, VideoContainer)> {
        let validated =
            validate_streaming_target_url(stream_url.trim(), self.allow_private_ips).await?;
        let source_headers = self
            .resolve_internal_media_headers(Some(source_id), validated.as_str())
            .await;
        let mut header_map = build_sanitized_headers(&source_headers);
        if !header_map.contains_key(USER_AGENT) {
            header_map.insert(USER_AGENT, HeaderValue::from_static(DEFAULT_WEBVIEW_UA));
        }
        header_map.insert(ACCEPT_ENCODING, HeaderValue::from_static("identity"));

        let mut resp = send_streaming_get(
            validated.as_str(),
            header_map,
            self.allow_private_ips,
            self.http_config.proxy_url.as_deref(),
            "拉取在线视频失败",
        )
        .await?;

        if !resp.status().is_success() {
            return Err(DomainError::Validation(format!(
                "在线视频响应状态异常: {}",
                resp.status()
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
                "拒绝缓存不安全的媒体类型: {ct_lower}"
            )));
        }

        let final_url = resp.url().clone();
        let prefix = read_sniff_prefix_with_context(
            &mut resp,
            SNIFF_PREFIX_MAX_BYTES,
            false,
            &ct_lower,
            &final_url,
        )
        .await?;
        if looks_like_html_bytes(&prefix) {
            return Err(DomainError::Validation(
                "上游返回了伪装为视频的 HTML/SVG 内容".into(),
            ));
        }
        if is_hls_playlist_response(&ct_lower, &final_url, &prefix) {
            return Err(DomainError::Validation(
                "上游直链实际返回了 HLS 播放列表，请按 HLS 模式缓存".into(),
            ));
        }

        let container = infer_container_from_url_and_ct(&final_url, header_ct.as_deref());
        let (temp, mut file) = TempFileGuard::create_exclusive(container.extension()).await?;

        let mut total_bytes: u64 = 0;
        if !prefix.is_empty() {
            total_bytes = total_bytes.saturating_add(prefix.len() as u64);
            file.write_all(&prefix)
                .await
                .map_err(|e| DomainError::Storage(format!("写入临时视频缓存失败: {e}")))?;
        }
        while let Some(chunk) = resp
            .chunk()
            .await
            .map_err(|e| DomainError::Validation(format!("读取上游视频流失败: {e}")))?
        {
            if chunk.is_empty() {
                continue;
            }
            total_bytes = total_bytes.saturating_add(chunk.len() as u64);
            if total_bytes > MAX_TRACKED_VIDEO_BYTES {
                return Err(DomainError::Validation(
                    "在线视频体积超过 4 GiB 缓存上限".into(),
                ));
            }
            file.write_all(&chunk)
                .await
                .map_err(|e| DomainError::Storage(format!("写入临时视频缓存失败: {e}")))?;
        }
        file.flush()
            .await
            .map_err(|e| DomainError::Storage(format!("刷新临时视频缓存失败: {e}")))?;
        drop(file);

        if total_bytes == 0 {
            return Err(DomainError::Validation("上游返回的在线视频内容为空".into()));
        }
        Ok((temp, container))
    }

    async fn fetch_hls_text_ssrf_safe(
        &self,
        source_id: Uuid,
        raw_url: &str,
    ) -> DomainResult<(String, Url)> {
        let validated =
            validate_streaming_target_url(raw_url.trim(), self.allow_private_ips).await?;
        let source_headers = self
            .resolve_internal_media_headers(Some(source_id), validated.as_str())
            .await;
        let mut header_map = build_sanitized_headers(&source_headers);
        if !header_map.contains_key(USER_AGENT) {
            header_map.insert(USER_AGENT, HeaderValue::from_static(DEFAULT_WEBVIEW_UA));
        }
        header_map.insert(ACCEPT_ENCODING, HeaderValue::from_static("identity"));

        let mut resp = send_streaming_get(
            validated.as_str(),
            header_map,
            self.allow_private_ips,
            self.http_config.proxy_url.as_deref(),
            "拉取 HLS 播放列表失败",
        )
        .await?;
        if !resp.status().is_success() {
            return Err(DomainError::Validation(format!(
                "HLS 播放列表响应状态异常: {}",
                resp.status()
            )));
        }
        let header_ct = resp
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(ToString::to_string);
        let final_url = resp.url().clone();
        let mut buf = Vec::new();
        while let Some(chunk) = resp
            .chunk()
            .await
            .map_err(|e| DomainError::Validation(format!("读取 HLS 播放列表失败: {e}")))?
        {
            if buf.len().saturating_add(chunk.len()) > MAX_M3U8_BYTES {
                return Err(DomainError::Validation("HLS 播放列表体积超限".into()));
            }
            buf.extend_from_slice(&chunk);
        }
        if looks_like_html_bytes(&buf) {
            return Err(DomainError::Validation(
                "拒绝缓存伪装为 HLS 播放列表的 HTML/SVG 内容".into(),
            ));
        }
        let text = decode_response_bytes(&buf, None, header_ct.as_deref());
        let trimmed = text.trim_start_matches(|c: char| c == '\u{feff}' || c.is_ascii_whitespace());
        if !trimmed.starts_with("#EXTM3U") {
            return Err(DomainError::Validation("无效的 HLS 播放列表".into()));
        }
        Ok((text, final_url))
    }

    async fn download_hls_asset_to_file(
        &self,
        source_id: Uuid,
        raw_url: &str,
        dest_path: &Path,
        total_bytes: &mut u64,
    ) -> DomainResult<()> {
        let validated =
            validate_streaming_target_url(raw_url.trim(), self.allow_private_ips).await?;
        let source_headers = self
            .resolve_internal_media_headers(Some(source_id), validated.as_str())
            .await;
        let mut header_map = build_sanitized_headers(&source_headers);
        if !header_map.contains_key(USER_AGENT) {
            header_map.insert(USER_AGENT, HeaderValue::from_static(DEFAULT_WEBVIEW_UA));
        }
        header_map.insert(ACCEPT_ENCODING, HeaderValue::from_static("identity"));

        let mut resp = send_streaming_get(
            validated.as_str(),
            header_map,
            self.allow_private_ips,
            self.http_config.proxy_url.as_deref(),
            "拉取 HLS 分片失败",
        )
        .await?;
        if !resp.status().is_success() {
            return Err(DomainError::Validation(format!(
                "HLS 分片响应状态异常: {}",
                resp.status()
            )));
        }
        let mut file = open_exclusive_temp_file(dest_path)
            .await
            .map_err(|e| DomainError::Storage(format!("创建 HLS 分片缓存失败: {e}")))?;
        while let Some(chunk) = resp
            .chunk()
            .await
            .map_err(|e| DomainError::Validation(format!("读取 HLS 分片失败: {e}")))?
        {
            if chunk.is_empty() {
                continue;
            }
            *total_bytes = total_bytes.saturating_add(chunk.len() as u64);
            if *total_bytes > MAX_TRACKED_VIDEO_BYTES {
                return Err(DomainError::Validation(
                    "HLS 缓存总大小超过 4 GiB 上限".into(),
                ));
            }
            file.write_all(&chunk)
                .await
                .map_err(|e| DomainError::Storage(format!("写入 HLS 分片失败: {e}")))?;
        }
        file.flush()
            .await
            .map_err(|e| DomainError::Storage(format!("刷新 HLS 分片失败: {e}")))?;
        Ok(())
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
            self.fetch_hls_text_ssrf_safe(source_id, hls_url).await?;
        if playlist_text.contains("#EXT-X-STREAM-INF:")
            && let Some(variant_url) = pick_best_hls_variant(&playlist_text, &base_url)
        {
            let (variant_text, variant_base) = self
                .fetch_hls_text_ssrf_safe(source_id, &variant_url)
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
                    let mut running = bytes_so_far;
                    self.download_hls_asset_to_file(source_id, &url_owned, &dest, &mut running)
                        .await?;
                    Ok(running)
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
        let validated = validate_streaming_target_url(raw_url.trim(), self.allow_private_ips)
            .await
            .ok()?;
        let source_headers = self
            .resolve_internal_media_headers(Some(source_id), validated.as_str())
            .await;
        let mut header_map = build_sanitized_headers(&source_headers);
        if !header_map.contains_key(USER_AGENT) {
            header_map.insert(USER_AGENT, HeaderValue::from_static(DEFAULT_WEBVIEW_UA));
        }
        let mut resp = send_streaming_get(
            validated.as_str(),
            header_map,
            self.allow_private_ips,
            self.http_config.proxy_url.as_deref(),
            "拉取字幕失败",
        )
        .await
        .ok()?;
        if !resp.status().is_success() {
            return None;
        }
        let ct = resp
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(ToString::to_string);
        let mut buf = Vec::new();
        while let Ok(Some(chunk)) = resp.chunk().await {
            if buf.len().saturating_add(chunk.len()) > MAX_SUBTITLE_BYTES {
                return None;
            }
            buf.extend_from_slice(&chunk);
        }
        validate_and_normalize_subtitle_to_vtt(&buf, ct.as_deref()).ok()
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

        let mut tracking_info = video
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

        if video.poster_key.is_none()
            && let Ok(frame_bytes) = deps
                .media_engine
                .generate_poster_bytes(
                    Path::new(&stored.absolute_path),
                    probed.duration_secs,
                    &video.title,
                )
                .await
            && let Ok(pk) = deps.storage.save_generated_poster_bytes(&frame_bytes).await
        {
            video.poster_key = Some(pk);
        }

        video.container = container;
        video.file_size = stored.file_size;
        video.file_hash = stored.file_hash;
        if probed.duration_secs > 0 {
            video.duration_secs = probed.duration_secs;
        }
        if probed.bit_rate_kbps.is_some() {
            video.bit_rate_kbps = probed.bit_rate_kbps;
        }
        if probed.width.is_some() {
            video.width = probed.width;
        }
        if probed.height.is_some() {
            video.height = probed.height;
        }
        if probed.video_codec.is_some() {
            video.video_codec = probed.video_codec;
        }
        if !probed.audio_streams.is_empty() {
            video.audio_streams = probed.audio_streams;
        }
        if !probed.subtitle_streams.is_empty() {
            video.subtitle_streams = probed.subtitle_streams;
        }

        tracking_info.storage_key = Some(stored.storage_key);
        tracking_info.cached = true;
        tracking_info.sidecar_subtitles = sidecar_subtitles;
        video.source = VideoSource::OnlineTracked(Box::new(tracking_info));
        video.updated_at = Timestamp::now();

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

fn resolve_strict_http_hls_uri(raw_uri: &str, base_url: &Url) -> DomainResult<Url> {
    let clean = raw_uri.trim();
    if clean.is_empty() || clean.bytes().any(|b| b.is_ascii_control() || b == b'\\') {
        return Err(DomainError::Validation("无效的 HLS URI".into()));
    }
    let lower = clean.to_ascii_lowercase();
    if lower.starts_with("file:")
        || lower.starts_with("data:")
        || lower.starts_with("crypto:")
        || lower.starts_with("subfile:")
        || lower.starts_with("concat:")
        || lower.starts_with("pipe:")
        || lower.starts_with("fd:")
    {
        return Err(DomainError::Validation(
            "安全策略拒绝非 HTTP(S) 的 HLS URI 协议".into(),
        ));
    }
    let joined = base_url
        .join(clean)
        .map_err(|e| DomainError::Validation(format!("无效的 HLS URI: {e}")))?;
    if !matches!(joined.scheme(), "http" | "https")
        || joined.host_str().is_none_or(|h| h.is_empty())
    {
        return Err(DomainError::Validation(format!(
            "安全策略仅允许 HTTP/HTTPS HLS URI，已拒绝协议: {}",
            joined.scheme()
        )));
    }
    Ok(joined)
}

async fn stage_local_hls_playlist_with<F, Fut>(
    playlist_text: &str,
    base_url: &Url,
    mut fetch_to_local: F,
) -> DomainResult<Vec<String>>
where
    F: FnMut(&Url, String, u64) -> Fut,
    Fut: std::future::Future<Output = DomainResult<u64>>,
{
    let mut total_bytes: u64 = 0;
    let mut seg_idx: usize = 0;
    let mut asset_idx: usize = 0;
    let mut local_lines = Vec::new();

    for raw_line in playlist_text.lines() {
        let line = raw_line.trim_end_matches('\r');
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with('#') {
            let Some(prefix4) = trimmed.get(..4) else {
                continue;
            };
            if !prefix4.eq_ignore_ascii_case("#EXT") {
                // Drop pure comments so untrusted text never reaches local FFmpeg playlist.
                continue;
            }
            if trimmed
                .get(..13)
                .is_some_and(|p| p.eq_ignore_ascii_case("#EXT-X-DEFINE"))
            {
                return Err(DomainError::Validation(
                    "安全策略拒绝包含 #EXT-X-DEFINE 变量替换的 HLS 播放列表".into(),
                ));
            }
            if trimmed
                .get(..8)
                .is_some_and(|p| p.eq_ignore_ascii_case("#EXTINF:"))
            {
                let rest = &trimmed[8..];
                let dur = rest.split(',').next().unwrap_or(rest).trim();
                local_lines.push(format!("#EXTINF:{dur},"));
                continue;
            }

            let Some((tag_prefix, attr_str)) = trimmed.split_once(':') else {
                if trimmed.to_ascii_lowercase().contains("uri") {
                    return Err(DomainError::Validation(
                        "HLS 标签包含无法解析的 URI 属性".into(),
                    ));
                }
                local_lines.push(trimmed.to_string());
                continue;
            };

            if !attr_str.contains('=') && !attr_str.to_ascii_lowercase().contains("uri") {
                local_lines.push(trimmed.to_string());
                continue;
            }

            let bytes = attr_str.as_bytes();
            let len = bytes.len();
            let mut idx = 0usize;
            let mut out_attrs: Vec<String> = Vec::new();

            while idx < len {
                while idx < len && (bytes[idx].is_ascii_whitespace() || bytes[idx] == b',') {
                    idx += 1;
                }
                if idx >= len {
                    break;
                }
                let key_start = idx;
                while idx < len
                    && (bytes[idx].is_ascii_alphanumeric()
                        || bytes[idx] == b'-'
                        || bytes[idx] == b'_')
                {
                    idx += 1;
                }
                if idx == key_start {
                    return Err(DomainError::Validation("HLS 标签属性名格式无效".into()));
                }
                let key = &attr_str[key_start..idx];
                while idx < len && bytes[idx].is_ascii_whitespace() {
                    idx += 1;
                }
                if idx >= len || bytes[idx] != b'=' {
                    return Err(DomainError::Validation(format!(
                        "HLS 标签属性 {key} 缺少 '=' 号"
                    )));
                }
                idx += 1;
                while idx < len && bytes[idx].is_ascii_whitespace() {
                    idx += 1;
                }
                if idx >= len {
                    return Err(DomainError::Validation(format!(
                        "HLS 标签属性 {key} 值为空"
                    )));
                }

                let (raw_val, was_quoted) = if bytes[idx] == b'"' || bytes[idx] == b'\'' {
                    let quote = bytes[idx];
                    let val_start = idx + 1;
                    let Some(close_rel) = attr_str[val_start..].find(quote as char) else {
                        return Err(DomainError::Validation(format!(
                            "HLS 标签属性 {key} 引号未闭合"
                        )));
                    };
                    let val = &attr_str[val_start..val_start + close_rel];
                    idx = val_start + close_rel + 1;
                    while idx < len && bytes[idx].is_ascii_whitespace() {
                        idx += 1;
                    }
                    if idx < len && bytes[idx] != b',' {
                        return Err(DomainError::Validation(format!(
                            "HLS 标签属性 {key} 分隔符无效"
                        )));
                    }
                    (val, true)
                } else {
                    let val_start = idx;
                    while idx < len && bytes[idx] != b',' && !bytes[idx].is_ascii_whitespace() {
                        idx += 1;
                    }
                    let val = &attr_str[val_start..idx];
                    while idx < len && bytes[idx].is_ascii_whitespace() {
                        idx += 1;
                    }
                    if idx < len && bytes[idx] != b',' {
                        return Err(DomainError::Validation(format!(
                            "HLS 标签属性 {key} 未加引号的值格式无效"
                        )));
                    }
                    (val, false)
                };

                if key.eq_ignore_ascii_case("URI") {
                    let joined = resolve_strict_http_hls_uri(raw_val, base_url)?;
                    asset_idx += 1;
                    let fname = format!("asset_{asset_idx:05}.bin");
                    total_bytes = fetch_to_local(&joined, fname.clone(), total_bytes).await?;
                    out_attrs.push(format!("URI=\"{fname}\""));
                } else if was_quoted {
                    if raw_val.contains('"') || raw_val.bytes().any(|b| b.is_ascii_control()) {
                        return Err(DomainError::Validation(format!(
                            "HLS 标签属性 {key} 包含非法字符"
                        )));
                    }
                    out_attrs.push(format!("{key}=\"{raw_val}\""));
                } else {
                    if raw_val.is_empty()
                        || raw_val
                            .bytes()
                            .any(|b| b.is_ascii_control() || b == b'"' || b == b'\'')
                    {
                        return Err(DomainError::Validation(format!(
                            "HLS 标签属性 {key} 值无效"
                        )));
                    }
                    out_attrs.push(format!("{key}={raw_val}"));
                }
            }

            if out_attrs.is_empty() {
                return Err(DomainError::Validation("HLS 标签属性列表为空".into()));
            }
            local_lines.push(format!("{}:{}", tag_prefix.trim(), out_attrs.join(",")));
            continue;
        }

        let joined = resolve_strict_http_hls_uri(trimmed, base_url)?;
        seg_idx += 1;
        if seg_idx > MAX_HLS_SEGMENTS {
            return Err(DomainError::Validation("HLS 分片数量超过缓存上限".into()));
        }
        let ext = Path::new(joined.path())
            .extension()
            .and_then(|s| s.to_str())
            .map(|e| e.to_ascii_lowercase())
            .filter(|e| {
                matches!(
                    e.as_str(),
                    "ts" | "m4s" | "mp4" | "m4a" | "aac" | "mp3" | "ac3" | "eac3" | "vtt" | "bin"
                )
            })
            .unwrap_or_else(|| "ts".to_string());
        let fname = format!("seg_{seg_idx:06}.{ext}");
        total_bytes = fetch_to_local(&joined, fname.clone(), total_bytes).await?;
        local_lines.push(fname);
    }

    if seg_idx == 0 {
        return Err(DomainError::Validation(
            "HLS 播放列表未包含可缓存分片".into(),
        ));
    }

    Ok(local_lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stage_local_hls_playlist_rejects_file_data_and_malformed_uris() {
        let base = Url::parse("https://cdn.example.com/hls/index.m3u8").unwrap();
        let malicious_cases = [
            "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"file:///etc/passwd\"\n#EXTINF:4.0,\nseg0.ts\n",
            "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI='file:///etc/passwd'\n#EXTINF:4.0,\nseg0.ts\n",
            "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=file:///etc/passwd\n#EXTINF:4.0,\nseg0.ts\n",
            "#EXTM3U\n#EXT-X-MAP:URI=\"data:application/octet-stream;base64,AAAA\"\n#EXTINF:4.0,\nseg0.ts\n",
            "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"crypto:/etc/passwd\"\n#EXTINF:4.0,\nseg0.ts\n",
            "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"https://cdn.example.com/k.key\n#EXTINF:4.0,\nseg0.ts\n",
            "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\n#EXTINF:4.0,\nseg0.ts\n",
            "#EXTM3U\n#EXT-X-DEFINE:NAME=\"x\",VALUE=\"file:///etc/passwd\"\n#EXTINF:4.0,\nseg0.ts\n",
            "#EXTM3U\n#EXTINF:4.0,\nfile:///etc/passwd\n",
            "#EXTM3U\n#EXTINF:4.0,\ndata:video/mp2t;base64,AAAA\n",
        ];

        for playlist in malicious_cases {
            let res =
                stage_local_hls_playlist_with(playlist, &base, |_url, _fname, cur| async move {
                    Ok(cur + 16)
                })
                .await;
            assert!(
                matches!(res, Err(DomainError::Validation(_))),
                "expected Validation error for malicious playlist: {playlist}"
            );
        }
    }

    #[tokio::test]
    async fn stage_local_hls_playlist_rewrites_all_http_uri_forms() {
        let base = Url::parse("https://cdn.example.com/hls/index.m3u8").unwrap();
        let playlist = concat!(
            "#EXTM3U\n",
            "#EXT-X-VERSION:6\n",
            "#EXT-X-TARGETDURATION:6\n",
            "# arbitrary comment URI=file:///etc/passwd\n",
            "#EXT-X-KEY:METHOD=AES-128,URI='keys/enc.key',IV=0x00000000000000000000000000000001\n",
            "#EXT-X-MAP:URI=init.mp4\n",
            "#EXTINF:5.500,Episode Title\n",
            "https://cdn.example.com/hls/disguised_seg.png\n",
            "#EXT-X-ENDLIST\n",
        );

        let mut downloaded = Vec::new();
        let lines = stage_local_hls_playlist_with(playlist, &base, |url, fname, cur| {
            downloaded.push((url.to_string(), fname));
            async move { Ok(cur + 128) }
        })
        .await
        .expect("valid HTTP HLS playlist should succeed");

        assert_eq!(
            downloaded,
            vec![
                (
                    "https://cdn.example.com/hls/keys/enc.key".to_string(),
                    "asset_00001.bin".to_string()
                ),
                (
                    "https://cdn.example.com/hls/init.mp4".to_string(),
                    "asset_00002.bin".to_string()
                ),
                (
                    "https://cdn.example.com/hls/disguised_seg.png".to_string(),
                    "seg_000001.ts".to_string()
                ),
            ]
        );
        let joined = lines.join("\n");
        assert!(joined.contains("URI=\"asset_00001.bin\""));
        assert!(joined.contains("URI=\"asset_00002.bin\""));
        assert!(joined.contains("seg_000001.ts"));
        assert!(!joined.contains("/etc/passwd"));
    }

    #[tokio::test]
    async fn temp_file_and_dir_guards_use_exclusive_owner_only_permissions() {
        let (file_guard, mut file) = TempFileGuard::create_exclusive("mp4").await.unwrap();
        let file_path = file_guard.path().to_path_buf();
        file.write_all(b"video-data").await.unwrap();
        drop(file);
        assert!(file_path.is_file());

        let dup_err = open_exclusive_temp_file(&file_path).await.unwrap_err();
        assert_eq!(dup_err.kind(), std::io::ErrorKind::AlreadyExists);

        let dir_guard = TempDirGuard::new().unwrap();
        let dir_path = dir_guard.path().to_path_buf();
        assert!(dir_path.is_dir());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let f_mode = std::fs::metadata(&file_path).unwrap().permissions().mode() & 0o777;
            assert_eq!(f_mode, 0o600);
            let d_mode = std::fs::metadata(&dir_path).unwrap().permissions().mode() & 0o777;
            assert_eq!(d_mode, 0o700);
        }

        drop(file_guard);
        drop(dir_guard);
        assert!(!file_path.exists());
        assert!(!dir_path.exists());
    }
}

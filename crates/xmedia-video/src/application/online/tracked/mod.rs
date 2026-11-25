mod cache;
mod hls_stage;

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use jiff::Timestamp;
use uuid::Uuid;
use xmedia_core::application::ports::SystemEvent;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;
use xmedia_core::domain::user::User;

use crate::application::dto::ShowDetailDto;
use crate::application::ports::VideoProxyUrlBuilder;
use crate::domain::id::{ShowId, VideoId};
use crate::domain::online::{OnlineVideoPlayGroup, OnlineVideoPlayResult, VideoSourceSpec};
use crate::domain::video::{
    OnlineTrackedVideoSource, VideoContainer, VideoItem, VideoShow, VideoSource,
    normalize_string_list,
};

use super::dto::{TrackOnlineVideoInput, TrackOnlineVideoResultDto, TrackedVideoPlayResponseDto};
use super::{CancelOnDrop, VideoOnlineService, VideoTrackingDeps, get_or_insert_weak_lock};

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

fn is_detail_url_like(raw: &str) -> bool {
    let t = raw.trim();
    t.starts_with("http://") || t.starts_with("https://") || t.starts_with('/')
}

impl VideoOnlineService {
    pub(super) fn require_tracking(&self) -> DomainResult<&VideoTrackingDeps> {
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
        let (bytes, _mime) = self
            .transport
            .fetch_poster_bytes(url, &headers)
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
                None,
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

        let show = if let Some(mut existing_show) = deps
            .catalog_repo
            .find_online_tracked_show(input.source_id, &canonical_video_id)
            .await?
        {
            if existing_show.fill_missing_online_metadata(
                Some(&summary),
                year,
                poster_key.clone(),
                Some(canonical_detail_url.clone()),
                now,
            ) {
                deps.catalog_repo.save_show(&existing_show).await?;
            }
            existing_show
        } else {
            let created = VideoShow::new_online_tracked(
                managed_lib.id,
                input.source_id,
                canonical_video_id.clone(),
                Some(canonical_detail_url.clone()),
                title.clone(),
                summary.clone(),
                year,
                genres.clone(),
                poster_key.clone(),
                now,
            );
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
        let (source_id, item_key_ref, show_detail_url_ref) = show
            .online_tracked()
            .ok_or_else(|| DomainError::Validation("该剧集不是在线追更资源".into()))?;
        let item_key = item_key_ref.to_string();
        let show_detail_url = show_detail_url_ref.map(str::to_string);

        let existing_eps = deps.catalog_repo.list_episodes_by_show(show.id).await?;
        let first_info = existing_eps
            .iter()
            .find_map(|e| e.source.online_tracking_info());
        let video_id_str = first_info
            .map(|i| i.video_id.clone())
            .unwrap_or_else(|| item_key.clone());
        let stored_detail_url =
            show_detail_url.or_else(|| first_info.map(|i| i.detail_url.clone()));
        let preferred_group = first_info.and_then(|i| i.group_name.clone());

        let detail_url_opt = stored_detail_url
            .as_deref()
            .filter(|u| is_detail_url_like(u));

        let detail_resp = self
            .detail(actor, source_id, Some(&video_id_str), detail_url_opt, None)
            .await?;
        let detail = detail_resp.detail;
        let selected_group = select_play_group(&detail.play_groups, preferred_group.as_deref())
            .ok_or_else(|| DomainError::Validation("刷新失败：上游未返回剧集列表".into()))?;

        let now = Timestamp::now();
        let fetched_poster = if show.poster_key.is_none() {
            self.fetch_and_save_poster_best_effort(deps, source_id, detail.cover_url.as_deref())
                .await
        } else {
            None
        };
        if show.fill_missing_online_metadata(
            detail.intro.as_deref(),
            parse_year_opt(detail.year.as_deref()),
            fetched_poster,
            stored_detail_url.clone(),
            now,
        ) {
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
        let group_owned = info.group_name;
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
        url_builder: &dyn VideoProxyUrlBuilder,
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
            url_builder.hls_proxy_url(&play.stream_url, source_id, query_token)
        } else {
            url_builder.stream_proxy_url(&play.stream_url, source_id, query_token)
        };
        for sub in &mut play.subtitles {
            let clean = sub.url.trim();
            if clean.starts_with("http://") || clean.starts_with("https://") {
                sub.proxy_url = Some(url_builder.subtitle_proxy_url(clean, source_id, query_token));
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
}

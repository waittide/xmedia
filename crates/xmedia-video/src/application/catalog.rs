use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use jiff::Timestamp;
use uuid::Uuid;
use xmedia_core::application::ports::BinaryPayload;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;
use xmedia_core::domain::repository::LibraryRepository;
use xmedia_core::domain::user::{User, UserId};

use crate::application::dto::{
    SeasonGroupDto, ShowDetailDto, ShowSummaryDto, SubtitleTrackDto, VideoDetailDto,
    VideoHomeOverviewDto, VideoSummaryDto, VideoSystemStatusDto,
};
use crate::application::ports::{VideoMediaEnginePort, VideoStoragePort};
use crate::domain::id::{ShowId, VideoId};
use crate::domain::repository::{VideoCatalogRepository, VideoWatchRepository};
use crate::domain::video::{
    ShowMetadataPatch, ShowQueryFilter, VideoItem, VideoMetadataPatch, VideoQueryFilter, VideoShow,
    VideoSource,
};
use crate::domain::watch::{VideoFavoriteKind, WatchProgress};

#[derive(Clone)]
pub struct VideoCatalogService {
    libraries: Arc<dyn LibraryRepository>,
    catalog: Arc<dyn VideoCatalogRepository>,
    watch: Arc<dyn VideoWatchRepository>,
    storage: Arc<dyn VideoStoragePort>,
    engine: Arc<dyn VideoMediaEnginePort>,
}

impl VideoCatalogService {
    pub fn new(
        libraries: Arc<dyn LibraryRepository>,
        catalog: Arc<dyn VideoCatalogRepository>,
        watch: Arc<dyn VideoWatchRepository>,
        storage: Arc<dyn VideoStoragePort>,
        engine: Arc<dyn VideoMediaEnginePort>,
    ) -> Self {
        Self {
            libraries,
            catalog,
            watch,
            storage,
            engine,
        }
    }

    pub async fn system_status(&self) -> DomainResult<VideoSystemStatusDto> {
        let ffmpeg = self.engine.check_status().await;
        let libs = self.libraries.list_by_media(MediaKind::Video).await?;
        let scanning = libs.iter().any(|l| l.scan_status.is_scanning());
        let video_count = self.catalog.count_all_active_videos().await?;
        Ok(VideoSystemStatusDto {
            ffmpeg,
            scanning,
            video_count,
        })
    }

    pub async fn enrich_videos(
        &self,
        user_id: UserId,
        videos: Vec<VideoItem>,
    ) -> DomainResult<Vec<VideoSummaryDto>> {
        if videos.is_empty() {
            return Ok(Vec::new());
        }
        let progress_list = self.watch.list_user_progress(user_id).await?;
        let favs = self
            .watch
            .list_favorites(user_id, Some(VideoFavoriteKind::Video))
            .await?;

        let prog_map: HashMap<VideoId, WatchProgress> =
            progress_list.into_iter().map(|p| (p.video_id, p)).collect();
        let fav_set: HashSet<Uuid> = favs.into_iter().map(|f| f.target_id).collect();

        Ok(videos
            .into_iter()
            .map(|v| {
                let resolution = v.resolution_label();
                let can_direct_play = v.can_direct_play_in_browser();
                VideoSummaryDto {
                    id: v.id,
                    library_id: v.library_id,
                    show_id: v.show_id,
                    show_title: v.show_title,
                    season_number: v.season_number,
                    episode_number: v.episode_number,
                    title: v.title,
                    summary: v.summary,
                    year: v.year,
                    genres: v.genres,
                    tags: v.tags,
                    container: v.container,
                    mime_type: v.container.mime_type().to_string(),
                    can_direct_play,
                    file_size: v.file_size,
                    duration_secs: v.duration_secs,
                    width: v.width,
                    height: v.height,
                    resolution,
                    video_codec: v.video_codec,
                    poster_key: v.poster_key,
                    is_favorite: fav_set.contains(&v.id.0),
                    progress: prog_map.get(&v.id).cloned(),
                    created_at: v.created_at,
                }
            })
            .collect())
    }

    pub async fn enrich_shows(
        &self,
        user_id: UserId,
        shows: Vec<VideoShow>,
    ) -> DomainResult<Vec<ShowSummaryDto>> {
        if shows.is_empty() {
            return Ok(Vec::new());
        }
        let favs = self
            .watch
            .list_favorites(user_id, Some(VideoFavoriteKind::Show))
            .await?;
        let fav_set: HashSet<Uuid> = favs.into_iter().map(|f| f.target_id).collect();

        let progress_list = self.watch.list_user_progress(user_id).await?;
        let completed_video_ids: HashSet<VideoId> = progress_list
            .into_iter()
            .filter(|p| p.completed)
            .map(|p| p.video_id)
            .collect();

        let all_episodes = self
            .catalog
            .list_videos(&VideoQueryFilter {
                kind: crate::domain::video::VideoKindFilter::Episodes,
                ..Default::default()
            })
            .await?;
        let mut watched_by_show: HashMap<ShowId, u32> = HashMap::new();
        for ep in all_episodes {
            if let Some(sid) = ep.show_id
                && completed_video_ids.contains(&ep.id)
            {
                *watched_by_show.entry(sid).or_default() += 1;
            }
        }

        Ok(shows
            .into_iter()
            .map(|s| ShowSummaryDto {
                id: s.id,
                library_id: s.library_id,
                title: s.title,
                summary: s.summary,
                year: s.year,
                genres: s.genres,
                tags: s.tags,
                poster_key: s.poster_key,
                season_count: s.season_count,
                episode_count: s.episode_count,
                watched_episode_count: watched_by_show.get(&s.id).copied().unwrap_or(0),
                is_favorite: fav_set.contains(&s.id.0),
                created_at: s.created_at,
            })
            .collect())
    }

    pub async fn home_overview(&self, user: &User) -> DomainResult<VideoHomeOverviewDto> {
        let mut all_prog = self.watch.list_user_progress(user.id).await?;
        all_prog.retain(|p| !p.completed && p.position_secs > 1.0);
        all_prog.sort_by_key(|p| std::cmp::Reverse(p.updated_at));
        let all_prog_ids: Vec<VideoId> = all_prog.iter().map(|p| p.video_id).collect();
        let fetched_all_cw = self.catalog.find_videos_by_ids(&all_prog_ids).await?;
        let cw_lookup: HashMap<VideoId, VideoItem> = fetched_all_cw
            .into_iter()
            .filter(|v| !v.is_missing)
            .map(|v| (v.id, v))
            .collect();
        let mut active_cw: Vec<VideoItem> = all_prog_ids
            .iter()
            .filter_map(|id| cw_lookup.get(id).cloned())
            .collect();
        let watching_count = active_cw.len() as u64;
        active_cw.truncate(12);
        let continue_watching = self.enrich_videos(user.id, active_cw).await?;

        let mut all_videos = self
            .catalog
            .list_videos(&VideoQueryFilter {
                sort_desc: Some(true),
                ..Default::default()
            })
            .await?;
        let total_videos = all_videos.len() as u64;
        let movies_count = all_videos.iter().filter(|v| v.show_id.is_none()).count() as u64;

        let mut genre_set = HashSet::new();
        let mut tag_set = HashSet::new();
        for v in &all_videos {
            for g in &v.genres {
                let clean = g.trim().to_lowercase();
                if !clean.is_empty() {
                    genre_set.insert(clean);
                }
            }
            for t in &v.tags {
                let clean = t.trim().to_lowercase();
                if !clean.is_empty() {
                    tag_set.insert(clean);
                }
            }
        }

        all_videos.truncate(16);
        let recent_videos = self.enrich_videos(user.id, all_videos).await?;

        let mut all_shows = self.catalog.list_shows(&ShowQueryFilter::default()).await?;
        let total_shows = all_shows.len() as u64;
        for s in &all_shows {
            for g in &s.genres {
                let clean = g.trim().to_lowercase();
                if !clean.is_empty() {
                    genre_set.insert(clean);
                }
            }
            for t in &s.tags {
                let clean = t.trim().to_lowercase();
                if !clean.is_empty() {
                    tag_set.insert(clean);
                }
            }
        }
        all_shows.sort_by_key(|s| std::cmp::Reverse(s.created_at));
        all_shows.truncate(12);
        let recent_shows = self.enrich_shows(user.id, all_shows).await?;

        let favorites_count = self.watch.list_favorites(user.id, None).await?.len() as u64;
        let shelves_count = self.watch.list_shelves(user.id).await?.len() as u64;
        let libraries_count = self.libraries.list_by_media(MediaKind::Video).await?.len() as u64;

        Ok(VideoHomeOverviewDto {
            continue_watching,
            recent_videos,
            recent_shows,
            total_videos,
            movies_count,
            total_shows,
            watching_count,
            favorites_count,
            shelves_count,
            genres_count: genre_set.len() as u64,
            tags_count: tag_set.len() as u64,
            libraries_count,
        })
    }

    pub async fn list_videos(
        &self,
        user: &User,
        filter: VideoQueryFilter,
    ) -> DomainResult<Vec<VideoSummaryDto>> {
        let items = self.catalog.list_videos(&filter).await?;
        self.enrich_videos(user.id, items).await
    }

    pub async fn get_video_detail(
        &self,
        user: &User,
        video_id: VideoId,
    ) -> DomainResult<VideoDetailDto> {
        let item = self
            .catalog
            .find_video_by_id(video_id)
            .await?
            .filter(|v| !v.is_missing)
            .ok_or_else(|| DomainError::NotFound("video not found".into()))?;

        let bit_rate_kbps = item.bit_rate_kbps;
        let path = item.source.display_path().to_string();
        let audio_streams = item.audio_streams.clone();
        let subtitles = Self::collect_subtitle_dtos(&item);

        let (prev_episode_id, next_episode_id) = if let Some(show_id) = item.show_id {
            let episodes = self.catalog.list_episodes_by_show(show_id).await?;
            let pos = episodes.iter().position(|e| e.id == item.id);
            let prev = pos
                .and_then(|idx| idx.checked_sub(1))
                .and_then(|i| episodes.get(i))
                .map(|e| e.id);
            let next = pos.and_then(|idx| episodes.get(idx + 1)).map(|e| e.id);
            (prev, next)
        } else {
            (None, None)
        };

        let summary = self
            .enrich_videos(user.id, vec![item])
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound("video not found".into()))?;

        Ok(VideoDetailDto {
            video: summary,
            bit_rate_kbps,
            path,
            audio_streams,
            subtitles,
            prev_episode_id,
            next_episode_id,
        })
    }

    pub fn collect_subtitle_dtos(video: &VideoItem) -> Vec<SubtitleTrackDto> {
        let mut out = Vec::new();
        for sc in video.source.sidecar_subtitles() {
            out.push(SubtitleTrackDto {
                id: format!("ext-{}", sc.id),
                source_kind: "external".into(),
                codec_or_format: sc.format.clone(),
                language: sc.language.clone(),
                label: sc.label.clone(),
                is_default: out.is_empty(),
            });
        }
        for sub in &video.subtitle_streams {
            if sub.is_text_based {
                let label = sub
                    .title
                    .clone()
                    .or_else(|| sub.language.clone())
                    .unwrap_or_else(|| format!("Track #{}", sub.stream_index));
                out.push(SubtitleTrackDto {
                    id: format!("emb-{}", sub.stream_index),
                    source_kind: "embedded".into(),
                    codec_or_format: sub.codec.clone(),
                    language: sub.language.clone(),
                    label,
                    is_default: out.is_empty() && sub.is_default,
                });
            }
        }
        out
    }

    pub async fn get_subtitle_webvtt(
        &self,
        video_id: VideoId,
        sub_id: &str,
    ) -> DomainResult<String> {
        let video = self
            .catalog
            .find_video_by_id(video_id)
            .await?
            .filter(|v| !v.is_missing)
            .ok_or_else(|| DomainError::NotFound("video not found".into()))?;
        let library = self
            .libraries
            .find_by_id(video.library_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("library not found".into()))?;

        let trimmed = sub_id.trim().trim_end_matches(".vtt");
        if let Some(raw_idx) = trimmed.strip_prefix("emb-") {
            let stream_index = raw_idx
                .parse::<u32>()
                .map_err(|_| DomainError::Validation("invalid embedded subtitle id".into()))?;
            let fs_path = self.storage.resolve_video_fs_path(&library, &video).await?;
            return self
                .engine
                .extract_embedded_subtitle_vtt(Path::new(&fs_path), stream_index)
                .await;
        }

        let ext_id = trimmed.strip_prefix("ext-").unwrap_or(trimmed);
        let (fmt, bytes) = self
            .storage
            .read_sidecar_subtitle_bytes(&library, &video, ext_id)
            .await?;
        Ok(self.engine.convert_sidecar_subtitle_to_vtt(&fmt, &bytes))
    }

    pub async fn update_video_metadata(
        &self,
        actor: &User,
        video_id: VideoId,
        patch: VideoMetadataPatch,
    ) -> DomainResult<VideoSummaryDto> {
        Self::require_admin(actor)?;
        let mut item = self
            .catalog
            .find_video_by_id(video_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("video not found".into()))?;

        if let Some(title) = patch.title {
            let trimmed = title.trim();
            if trimmed.is_empty() {
                return Err(DomainError::Validation("title cannot be empty".into()));
            }
            item.title = trimmed.to_string();
        }
        if let Some(summary) = patch.summary {
            item.summary = summary.trim().to_string();
        }
        if let Some(year) = patch.year {
            item.year = year;
        }
        if let Some(genres) = patch.genres {
            item.genres = genres;
        }
        if let Some(tags) = patch.tags {
            item.tags = tags;
        }
        if let Some(season) = patch.season_number {
            item.season_number = season;
        }
        if let Some(ep) = patch.episode_number {
            item.episode_number = ep;
        }
        item.updated_at = Timestamp::now();
        self.catalog.save_video(&item).await?;
        if item.show_id.is_some() {
            self.catalog.recompute_show_aggregates().await?;
        }
        self.enrich_videos(actor.id, vec![item])
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound("video not found".into()))
    }

    pub async fn upload_video_poster(
        &self,
        actor: &User,
        video_id: VideoId,
        bytes: &[u8],
    ) -> DomainResult<VideoSummaryDto> {
        Self::require_admin(actor)?;
        if bytes.is_empty() {
            return Err(DomainError::Validation(
                "poster image cannot be empty".into(),
            ));
        }
        let mut item = self
            .catalog
            .find_video_by_id(video_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("video not found".into()))?;
        let key = self.storage.save_poster_bytes(bytes).await?;
        item.poster_key = Some(key);
        item.updated_at = Timestamp::now();
        self.catalog.save_video(&item).await?;
        self.enrich_videos(actor.id, vec![item])
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound("video not found".into()))
    }

    pub async fn delete_video(&self, actor: &User, video_id: VideoId) -> DomainResult<()> {
        Self::require_admin(actor)?;
        let item = self
            .catalog
            .find_video_by_id(video_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("video not found".into()))?;
        let old_show_id = item.show_id;
        if let VideoSource::Managed {
            ref storage_key,
            ref sidecar_subtitles,
        } = item.source
        {
            let _ = self.storage.delete_managed_file(storage_key).await;
            for sc in sidecar_subtitles {
                let _ = self.storage.delete_managed_file(&sc.path_or_key).await;
            }
        }
        self.watch.delete_progress_by_video(video_id).await?;
        self.catalog.delete_video(video_id).await?;
        self.catalog.recompute_show_aggregates().await?;
        let deleted_show_id = if let Some(sid) = old_show_id {
            if self.catalog.find_show_by_id(sid).await?.is_none() {
                Some(sid)
            } else {
                None
            }
        } else {
            None
        };
        self.watch
            .delete_video_references(video_id, deleted_show_id)
            .await
    }

    pub async fn list_shows(
        &self,
        user: &User,
        filter: ShowQueryFilter,
    ) -> DomainResult<Vec<ShowSummaryDto>> {
        let shows = self.catalog.list_shows(&filter).await?;
        self.enrich_shows(user.id, shows).await
    }

    pub async fn get_show_detail(
        &self,
        user: &User,
        show_id: ShowId,
    ) -> DomainResult<ShowDetailDto> {
        let show = self
            .catalog
            .find_show_by_id(show_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("show not found".into()))?;
        let show_dto = self
            .enrich_shows(user.id, vec![show])
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound("show not found".into()))?;

        let episodes = self.catalog.list_episodes_by_show(show_id).await?;
        let ep_dtos = self.enrich_videos(user.id, episodes).await?;

        let continue_episode = ep_dtos
            .iter()
            .filter(|e| {
                e.progress
                    .as_ref()
                    .is_some_and(|p| !p.completed && p.position_secs > 1.0)
            })
            .max_by_key(|e| e.progress.as_ref().map(|p| p.updated_at))
            .cloned()
            .or_else(|| {
                ep_dtos
                    .iter()
                    .find(|e| e.progress.as_ref().is_none_or(|p| !p.completed))
                    .cloned()
            })
            .or_else(|| ep_dtos.first().cloned());

        let mut season_buckets: BTreeMap<u32, Vec<VideoSummaryDto>> = BTreeMap::new();
        for ep in ep_dtos {
            let s_num = ep.season_number.unwrap_or(1);
            season_buckets.entry(s_num).or_default().push(ep);
        }
        let seasons = season_buckets
            .into_iter()
            .map(|(season_number, episodes)| SeasonGroupDto {
                season_number,
                episodes,
            })
            .collect();

        Ok(ShowDetailDto {
            show: show_dto,
            seasons,
            continue_episode,
        })
    }

    pub async fn update_show_metadata(
        &self,
        actor: &User,
        show_id: ShowId,
        patch: ShowMetadataPatch,
    ) -> DomainResult<ShowSummaryDto> {
        Self::require_admin(actor)?;
        let mut show = self
            .catalog
            .find_show_by_id(show_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("show not found".into()))?;
        let mut title_changed = false;
        if let Some(title) = patch.title {
            let trimmed = title.trim();
            if trimmed.is_empty() {
                return Err(DomainError::Validation("show title cannot be empty".into()));
            }
            if show.title != trimmed {
                show.title = trimmed.to_string();
                title_changed = true;
            }
        }
        if let Some(summary) = patch.summary {
            show.summary = summary.trim().to_string();
        }
        if let Some(year) = patch.year {
            show.year = year;
        }
        if let Some(genres) = patch.genres {
            show.genres = genres;
        }
        if let Some(tags) = patch.tags {
            show.tags = tags;
        }
        show.updated_at = Timestamp::now();
        self.catalog.save_show(&show).await?;
        if title_changed {
            let episodes = self.catalog.list_episodes_by_show(show_id).await?;
            for mut ep in episodes {
                ep.show_title = Some(show.title.clone());
                ep.updated_at = Timestamp::now();
                self.catalog.save_video(&ep).await?;
            }
        }
        self.enrich_shows(actor.id, vec![show])
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound("show not found".into()))
    }

    pub async fn upload_show_poster(
        &self,
        actor: &User,
        show_id: ShowId,
        bytes: &[u8],
    ) -> DomainResult<ShowSummaryDto> {
        Self::require_admin(actor)?;
        if bytes.is_empty() {
            return Err(DomainError::Validation(
                "poster image cannot be empty".into(),
            ));
        }
        let mut show = self
            .catalog
            .find_show_by_id(show_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("show not found".into()))?;
        let key = self.storage.save_poster_bytes(bytes).await?;
        show.poster_key = Some(key);
        show.updated_at = Timestamp::now();
        self.catalog.save_show(&show).await?;
        self.enrich_shows(actor.id, vec![show])
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound("show not found".into()))
    }

    pub async fn get_poster(
        &self,
        key: &str,
        max_size: Option<u32>,
    ) -> DomainResult<BinaryPayload> {
        self.storage.read_poster(key, max_size).await
    }

    fn require_admin(actor: &User) -> DomainResult<()> {
        if !actor.role.is_admin() {
            return Err(DomainError::Forbidden(
                "administrator privileges required".into(),
            ));
        }
        Ok(())
    }
}

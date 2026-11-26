use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use jiff::Timestamp;
use uuid::Uuid;
use xmedia_core::application::bindings::MediaBindingService;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::{LibraryId, MediaKind};
use xmedia_core::domain::repository::LibraryRepository;
use xmedia_core::domain::user::{User, UserId};

use crate::application::dto::{
    SubtitleTrackDto, VideoDetailDto, VideoHomeOverviewDto, VideoSummaryDto, VideoSystemStatusDto,
    VideoTaxonomyDto, VideoTaxonomyItemDto,
};
use crate::application::ports::VideoMediaEnginePort;
use crate::application::projector::{
    VideoProjector, enrich_shows_with_maps, enrich_videos_with_maps,
};
use crate::domain::id::{ShowId, VideoId};
use crate::domain::repository::{
    ProgressScope, ShowScope, VideoCatalogRepository, VideoScope, VideoShelfScope,
    VideoWatchRepository,
};
use crate::domain::video::{ShowQueryFilter, VideoItem, VideoQueryFilter, VideoShow};
use crate::domain::watch::{VideoFavoriteKind, WatchProgress};

#[derive(Clone)]
pub struct VideoCatalogService {
    libraries: Arc<dyn LibraryRepository>,
    catalog: Arc<dyn VideoCatalogRepository>,
    watch: Arc<dyn VideoWatchRepository>,
    bindings: Arc<MediaBindingService>,
    engine: Arc<dyn VideoMediaEnginePort>,
    projector: VideoProjector,
}

impl VideoCatalogService {
    pub(super) fn new(
        libraries: Arc<dyn LibraryRepository>,
        catalog: Arc<dyn VideoCatalogRepository>,
        watch: Arc<dyn VideoWatchRepository>,
        bindings: Arc<MediaBindingService>,
        engine: Arc<dyn VideoMediaEnginePort>,
        projector: VideoProjector,
    ) -> Self {
        Self {
            libraries,
            catalog,
            watch,
            bindings,
            engine,
            projector,
        }
    }

    pub async fn list_all_binding_ids_for_backfill(&self) -> DomainResult<Vec<Uuid>> {
        let mut ids = HashSet::new();
        for v in self
            .catalog
            .query_videos(VideoScope::Filter(&VideoQueryFilter::default()))
            .await?
        {
            ids.insert(v.binding_id());
        }
        for s in self
            .catalog
            .query_shows(ShowScope::Filter(&ShowQueryFilter::default()))
            .await?
        {
            ids.insert(s.id.0);
        }
        Ok(ids.into_iter().collect())
    }

    pub async fn require_bound_video(
        &self,
        user_id: UserId,
        video_id: VideoId,
    ) -> DomainResult<VideoItem> {
        self.projector.require_video_bound(user_id, video_id).await
    }

    pub async fn system_status(&self, user_id: UserId) -> DomainResult<VideoSystemStatusDto> {
        let ffmpeg = self.engine.check_status().await;
        let libs = self.libraries.list(Some(MediaKind::Video)).await?;
        let scanning = libs.iter().any(|l| l.scan_status.is_scanning());
        let bound_ids = self
            .bindings
            .list_bound_ids(user_id, MediaKind::Video)
            .await?;
        let video_count = if bound_ids.is_empty() {
            0
        } else {
            self.catalog
                .query_videos(VideoScope::Filter(&VideoQueryFilter::default()))
                .await?
                .into_iter()
                .filter(|v| bound_ids.contains(&v.binding_id()))
                .count() as u64
        };
        Ok(VideoSystemStatusDto {
            ffmpeg,
            scanning,
            video_count,
        })
    }

    pub async fn home_overview(&self, user: &User) -> DomainResult<VideoHomeOverviewDto> {
        let bound_ids = self
            .bindings
            .list_bound_ids(user.id, MediaKind::Video)
            .await?;

        let mut all_videos: Vec<VideoItem> = if bound_ids.is_empty() {
            Vec::new()
        } else {
            self.catalog
                .query_videos(VideoScope::Filter(&VideoQueryFilter {
                    sort_desc: Some(true),
                    ..Default::default()
                }))
                .await?
                .into_iter()
                .filter(|v| bound_ids.contains(&v.binding_id()))
                .collect()
        };
        let mut all_shows: Vec<VideoShow> = if bound_ids.is_empty() {
            Vec::new()
        } else {
            self.catalog
                .query_shows(ShowScope::Filter(&ShowQueryFilter::default()))
                .await?
                .into_iter()
                .filter(|s| bound_ids.contains(&s.id.0))
                .collect()
        };
        let bound_video_id_set: HashSet<VideoId> = all_videos.iter().map(|v| v.id).collect();
        let bound_show_id_set: HashSet<ShowId> = all_shows.iter().map(|s| s.id).collect();

        let all_progress_list: Vec<_> = self
            .watch
            .query_progress(ProgressScope::User(user.id))
            .await?
            .into_iter()
            .filter(|p| bound_video_id_set.contains(&p.video_id))
            .collect();
        let all_favorites: Vec<_> = self
            .watch
            .list_favorites(user.id, None)
            .await?
            .into_iter()
            .filter(|fav| match fav.kind {
                VideoFavoriteKind::Video => bound_video_id_set.contains(&VideoId(fav.target_id)),
                VideoFavoriteKind::Show => bound_show_id_set.contains(&ShowId(fav.target_id)),
            })
            .collect();
        let favorites_count = all_favorites.len() as u64;

        let mut video_fav_set: HashSet<Uuid> = HashSet::new();
        let mut show_fav_set: HashSet<Uuid> = HashSet::new();
        for fav in all_favorites {
            match fav.kind {
                VideoFavoriteKind::Video => {
                    video_fav_set.insert(fav.target_id);
                }
                VideoFavoriteKind::Show => {
                    show_fav_set.insert(fav.target_id);
                }
            }
        }

        let mut prog_map: HashMap<VideoId, WatchProgress> =
            HashMap::with_capacity(all_progress_list.len());
        let mut completed_video_ids: HashSet<VideoId> = HashSet::new();
        let mut active_prog: Vec<(VideoId, Timestamp)> = Vec::new();
        for p in all_progress_list {
            if p.completed {
                completed_video_ids.insert(p.video_id);
            } else if p.position_secs > 1.0 {
                active_prog.push((p.video_id, p.updated_at));
            }
            prog_map.insert(p.video_id, p);
        }
        active_prog.sort_by_key(|(_, ts)| std::cmp::Reverse(*ts));

        let total_videos = all_videos.len() as u64;
        let mut movies_count = 0u64;
        let mut genre_set = HashSet::new();
        let mut tag_set = HashSet::new();
        let mut lib_set: HashSet<LibraryId> = HashSet::new();
        let mut watched_by_show: HashMap<ShowId, u32> = HashMap::new();
        let mut video_index: HashMap<VideoId, usize> = HashMap::with_capacity(all_videos.len());

        for (idx, v) in all_videos.iter().enumerate() {
            video_index.insert(v.id, idx);
            lib_set.insert(v.library_id);
            if let Some(sid) = v.show_id {
                if completed_video_ids.contains(&v.id) {
                    *watched_by_show.entry(sid).or_default() += 1;
                }
            } else {
                movies_count += 1;
            }
            collect_lower_terms(&v.genres, &mut genre_set);
            collect_lower_terms(&v.tags, &mut tag_set);
        }

        let mut active_cw: Vec<VideoItem> = active_prog
            .iter()
            .filter_map(|(id, _)| video_index.get(id).map(|&idx| all_videos[idx].clone()))
            .collect();
        let watching_count = active_cw.len() as u64;
        active_cw.truncate(12);
        let continue_watching = enrich_videos_with_maps(active_cw, &prog_map, &video_fav_set);

        all_videos.truncate(16);
        let recent_videos = enrich_videos_with_maps(all_videos, &prog_map, &video_fav_set);

        let total_shows = all_shows.len() as u64;
        for s in &all_shows {
            lib_set.insert(s.library_id);
            collect_lower_terms(&s.genres, &mut genre_set);
            collect_lower_terms(&s.tags, &mut tag_set);
        }
        all_shows.sort_by_key(|s| std::cmp::Reverse(s.created_at));
        all_shows.truncate(12);
        let recent_shows = enrich_shows_with_maps(all_shows, &show_fav_set, &watched_by_show);

        let shelves_count = self
            .watch
            .query_shelves(VideoShelfScope::User(user.id))
            .await?
            .len() as u64;
        let libraries_count = lib_set.len() as u64;

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

    pub async fn get_taxonomy(&self, user_id: UserId) -> DomainResult<VideoTaxonomyDto> {
        let bound_ids = self
            .bindings
            .list_bound_ids(user_id, MediaKind::Video)
            .await?;
        if bound_ids.is_empty() {
            return Ok(VideoTaxonomyDto {
                genres: Vec::new(),
                tags: Vec::new(),
            });
        }
        let videos: Vec<_> = self
            .catalog
            .query_videos(VideoScope::Filter(&VideoQueryFilter::default()))
            .await?
            .into_iter()
            .filter(|v| bound_ids.contains(&v.binding_id()))
            .collect();
        let shows: Vec<_> = self
            .catalog
            .query_shows(ShowScope::Filter(&ShowQueryFilter::default()))
            .await?
            .into_iter()
            .filter(|s| bound_ids.contains(&s.id.0))
            .collect();

        let mut genre_counts: HashMap<String, usize> = HashMap::new();
        let mut tag_counts: HashMap<String, usize> = HashMap::new();

        for v in videos {
            increment_term_counts(&v.genres, &mut genre_counts);
            increment_term_counts(&v.tags, &mut tag_counts);
        }
        for s in shows {
            increment_term_counts(&s.genres, &mut genre_counts);
            increment_term_counts(&s.tags, &mut tag_counts);
        }

        Ok(VideoTaxonomyDto {
            genres: into_sorted_taxonomy_items(genre_counts),
            tags: into_sorted_taxonomy_items(tag_counts),
        })
    }

    pub async fn list_videos(
        &self,
        user: &User,
        filter: VideoQueryFilter,
    ) -> DomainResult<Vec<VideoSummaryDto>> {
        let bound_ids = self
            .bindings
            .list_bound_ids(user.id, MediaKind::Video)
            .await?;
        if bound_ids.is_empty() {
            return Ok(Vec::new());
        }
        let items: Vec<VideoItem> = self
            .catalog
            .query_videos(VideoScope::Filter(&filter))
            .await?
            .into_iter()
            .filter(|v| bound_ids.contains(&v.binding_id()))
            .collect();
        self.projector.enrich_videos(user.id, items).await
    }

    pub async fn get_video_detail(
        &self,
        user: &User,
        video_id: VideoId,
    ) -> DomainResult<VideoDetailDto> {
        let item = self
            .projector
            .require_video_bound(user.id, video_id)
            .await?;

        let bit_rate_kbps = item.bit_rate_kbps;
        let path = item.source.display_path().to_string();
        let audio_streams = item.audio_streams.clone();
        let subtitles = SubtitleTrackDto::collect_from_video(&item);

        let (prev_episode_id, next_episode_id) = if let Some(show_id) = item.show_id {
            let episodes = self
                .catalog
                .query_videos(VideoScope::ShowEpisodes(show_id))
                .await?;
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
            .projector
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
}

fn collect_lower_terms(terms: &[String], target: &mut HashSet<String>) {
    for term in terms {
        let clean = term.trim().to_lowercase();
        if !clean.is_empty() {
            target.insert(clean);
        }
    }
}

fn increment_term_counts(terms: &[String], counts: &mut HashMap<String, usize>) {
    for term in terms {
        let clean = term.trim();
        if !clean.is_empty() {
            *counts.entry(clean.to_string()).or_insert(0) += 1;
        }
    }
}

fn into_sorted_taxonomy_items(counts: HashMap<String, usize>) -> Vec<VideoTaxonomyItemDto> {
    let mut items: Vec<VideoTaxonomyItemDto> = counts
        .into_iter()
        .map(|(name, count)| VideoTaxonomyItemDto { name, count })
        .collect();
    items.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
    items
}

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use jiff::Timestamp;
use uuid::Uuid;
use xmedia_core::application::ports::BinaryPayload;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;
use xmedia_core::domain::user::{User, UserId, require_admin};

use crate::application::dto::{
    SeasonGroupDto, ShowDetailDto, ShowSummaryDto, SubtitleTrackDto, VideoSummaryDto,
    VideoTaxonomyDto, VideoTaxonomyItemDto,
};
use crate::domain::id::{ShowId, VideoId};
use crate::domain::video::{ShowMetadataPatch, ShowQueryFilter, VideoItem, VideoQueryFilter};
use crate::domain::watch::{VideoFavoriteKind, WatchProgress};

use super::{VideoCatalogService, video_binding_id};

impl VideoCatalogService {
    pub async fn list_video_subtitles(
        &self,
        user_id: UserId,
        video_id: VideoId,
    ) -> DomainResult<Vec<SubtitleTrackDto>> {
        let item = self.require_video_bound(user_id, video_id).await?;
        Ok(Self::collect_subtitle_dtos(&item))
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
        user_id: UserId,
        video_id: VideoId,
        sub_id: &str,
    ) -> DomainResult<String> {
        self.get_subtitle_webvtt_with_offset(user_id, video_id, sub_id, None)
            .await
    }

    pub async fn get_subtitle_webvtt_with_offset(
        &self,
        user_id: UserId,
        video_id: VideoId,
        sub_id: &str,
        offset_secs: Option<f64>,
    ) -> DomainResult<String> {
        let video = self.require_video_bound(user_id, video_id).await?;
        let library = self
            .libraries
            .find_by_id(video.library_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("library not found".into()))?;

        let trimmed = sub_id.trim().trim_end_matches(".vtt");
        let vtt = if let Some(raw_idx) = trimmed.strip_prefix("emb-") {
            let stream_index = raw_idx
                .parse::<u32>()
                .map_err(|_| DomainError::Validation("invalid embedded subtitle id".into()))?;
            let sub_stream = video
                .subtitle_streams
                .iter()
                .find(|s| s.stream_index == stream_index)
                .ok_or_else(|| {
                    DomainError::NotFound(format!(
                        "embedded subtitle stream {stream_index} not found"
                    ))
                })?;
            if !sub_stream.is_text_based {
                return Err(DomainError::Validation(format!(
                    "embedded subtitle stream {stream_index} ({}) is not text-based",
                    sub_stream.codec
                )));
            }
            let fs_path = self.storage.resolve_video_fs_path(&library, &video).await?;
            self.engine
                .extract_embedded_subtitle_vtt(Path::new(&fs_path), stream_index)
                .await?
        } else {
            let ext_id = trimmed.strip_prefix("ext-").unwrap_or(trimmed);
            let (fmt, bytes) = self
                .storage
                .read_sidecar_subtitle_bytes(&library, &video, ext_id)
                .await?;
            self.engine.convert_sidecar_subtitle_to_vtt(&fmt, &bytes)
        };

        if let Some(offset) = offset_secs.filter(|&v| v.is_finite() && v > 0.0) {
            Ok(self.engine.shift_webvtt_timestamps(&vtt, offset))
        } else {
            Ok(vtt)
        }
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
            .list_videos(&VideoQueryFilter::default())
            .await?
            .into_iter()
            .filter(|v| bound_ids.contains(&video_binding_id(v)))
            .collect();
        let shows: Vec<_> = self
            .catalog
            .list_shows(&ShowQueryFilter::default())
            .await?
            .into_iter()
            .filter(|s| bound_ids.contains(&s.id.0))
            .collect();

        let mut genre_counts: HashMap<String, usize> = HashMap::new();
        let mut tag_counts: HashMap<String, usize> = HashMap::new();

        for v in videos {
            for g in v.genres {
                let clean = g.trim();
                if !clean.is_empty() {
                    *genre_counts.entry(clean.to_string()).or_insert(0) += 1;
                }
            }
            for t in v.tags {
                let clean = t.trim();
                if !clean.is_empty() {
                    *tag_counts.entry(clean.to_string()).or_insert(0) += 1;
                }
            }
        }
        for s in shows {
            for g in s.genres {
                let clean = g.trim();
                if !clean.is_empty() {
                    *genre_counts.entry(clean.to_string()).or_insert(0) += 1;
                }
            }
            for t in s.tags {
                let clean = t.trim();
                if !clean.is_empty() {
                    *tag_counts.entry(clean.to_string()).or_insert(0) += 1;
                }
            }
        }

        let mut genres: Vec<VideoTaxonomyItemDto> = genre_counts
            .into_iter()
            .map(|(name, count)| VideoTaxonomyItemDto { name, count })
            .collect();
        genres.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));

        let mut tags: Vec<VideoTaxonomyItemDto> = tag_counts
            .into_iter()
            .map(|(name, count)| VideoTaxonomyItemDto { name, count })
            .collect();
        tags.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));

        Ok(VideoTaxonomyDto { genres, tags })
    }

    pub async fn list_shows(
        &self,
        user: &User,
        filter: ShowQueryFilter,
    ) -> DomainResult<Vec<ShowSummaryDto>> {
        let bound_ids = self
            .bindings
            .list_bound_ids(user.id, MediaKind::Video)
            .await?;
        if bound_ids.is_empty() {
            return Ok(Vec::new());
        }
        let shows: Vec<_> = self
            .catalog
            .list_shows(&filter)
            .await?
            .into_iter()
            .filter(|s| bound_ids.contains(&s.id.0))
            .collect();
        self.enrich_shows(user.id, shows).await
    }

    pub async fn get_show_detail(
        &self,
        user: &User,
        show_id: ShowId,
    ) -> DomainResult<ShowDetailDto> {
        self.require_show_bound(user.id, show_id).await?;
        let show = self
            .catalog
            .find_show_by_id(show_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("show not found".into()))?;
        let episodes = self.catalog.list_episodes_by_show(show_id).await?;
        let progress_list = self.watch.list_user_progress(user.id).await?;
        let all_favs = self.watch.list_favorites(user.id, None).await?;

        let prog_map: HashMap<VideoId, WatchProgress> =
            progress_list.into_iter().map(|p| (p.video_id, p)).collect();
        let mut video_fav_set: HashSet<Uuid> = HashSet::new();
        let mut show_fav_set: HashSet<Uuid> = HashSet::new();
        for fav in all_favs {
            match fav.kind {
                VideoFavoriteKind::Video => {
                    video_fav_set.insert(fav.target_id);
                }
                VideoFavoriteKind::Show => {
                    show_fav_set.insert(fav.target_id);
                }
            }
        }

        let watched_count = episodes
            .iter()
            .filter(|ep| prog_map.get(&ep.id).is_some_and(|p| p.completed))
            .count() as u32;
        let mut watched_by_show = HashMap::with_capacity(1);
        watched_by_show.insert(show.id, watched_count);

        let show_dto = Self::enrich_shows_with_maps(vec![show], &show_fav_set, &watched_by_show)
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound("show not found".into()))?;
        let ep_dtos = Self::enrich_videos_with_maps(episodes, &prog_map, &video_fav_set);

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
        require_admin(actor)?;
        let mut show = self
            .catalog
            .find_show_by_id(show_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("show not found".into()))?;
        let old_show_key = show.show_key.clone();
        let title_changed = show.apply_patch(patch)?;
        if show.show_key != old_show_key
            && let Some(conflict) = self.catalog.find_show_by_key(&show.show_key).await?
            && conflict.id != show.id
        {
            return Err(DomainError::Conflict(format!(
                "a show named '{}' already exists in this library",
                show.title
            )));
        }
        self.catalog.save_show(&show).await?;
        if title_changed {
            let now = Timestamp::now();
            let episodes = self.catalog.list_episodes_by_show(show_id).await?;
            for mut ep in episodes {
                ep.set_show_title(show.title.clone(), now);
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
        require_admin(actor)?;
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
        show.set_poster_key(key, Timestamp::now());
        self.catalog.save_show(&show).await?;
        self.enrich_shows(actor.id, vec![show])
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound("show not found".into()))
    }

    pub async fn get_poster(
        &self,
        actor: Option<&User>,
        key: &str,
        max_size: Option<u32>,
    ) -> DomainResult<BinaryPayload> {
        if let Some(user) = actor {
            let videos = self.catalog.find_videos_by_poster_key(key).await?;
            let mut matched_pool = false;
            let mut matched_tracked_binding_ids: Vec<Uuid> = Vec::new();
            for v in &videos {
                if v.source.is_online_tracked() {
                    matched_tracked_binding_ids.push(video_binding_id(v));
                } else {
                    matched_pool = true;
                    break;
                }
            }
            if !matched_pool {
                let shows = self.catalog.find_shows_by_poster_key(key).await?;
                for s in &shows {
                    if s.is_online_tracked() {
                        matched_tracked_binding_ids.push(s.id.0);
                    } else {
                        matched_pool = true;
                        break;
                    }
                }
            }
            if !matched_pool && !matched_tracked_binding_ids.is_empty() {
                let bound = self
                    .bindings
                    .list_bound_ids(user.id, MediaKind::Video)
                    .await?;
                if !matched_tracked_binding_ids
                    .iter()
                    .any(|id| bound.contains(id))
                {
                    return Err(DomainError::NotFound("poster not found".into()));
                }
            }
        }
        self.storage.read_poster(key, max_size).await
    }
}

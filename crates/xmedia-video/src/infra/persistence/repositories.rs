use std::collections::{HashMap, HashSet};

use async_trait::async_trait;
use jiff::Timestamp;
use toasty::Db;
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::LibraryId;
use xmedia_core::domain::user::UserId;

use crate::domain::id::{ShowId, VideoId, VideoShelfId};
use crate::domain::repository::{VideoCatalogRepository, VideoWatchRepository};
use crate::domain::video::{
    ShowQueryFilter, VideoContainer, VideoItem, VideoKindFilter, VideoQueryFilter, VideoShow,
    VideoSortBy,
};
use crate::domain::watch::{VideoFavoriteEntry, VideoFavoriteKind, VideoShelf, WatchProgress};

use super::models::{
    VideoFavoriteModel, VideoItemModel, VideoProgressModel, VideoShelfModel, VideoShowModel,
};

fn db_err(err: impl std::fmt::Display) -> DomainError {
    DomainError::Persistence(err.to_string())
}

#[derive(Clone)]
pub struct ToastyVideoCatalogRepository {
    db: Db,
}

impl ToastyVideoCatalogRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    fn item_to_domain(m: VideoItemModel) -> VideoItem {
        let container = VideoContainer::parse(&m.container).unwrap_or(VideoContainer::Mp4);
        VideoItem {
            id: VideoId(m.id),
            library_id: LibraryId(m.library_id),
            show_id: m.show_id.map(ShowId),
            show_title: m.show_title,
            season_number: m.season_number.and_then(|v| u32::try_from(v).ok()),
            episode_number: m.episode_number.and_then(|v| u32::try_from(v).ok()),
            title: m.title,
            summary: m.summary,
            year: m.year.and_then(|y| i32::try_from(y).ok()),
            genres: m.genres.0,
            tags: m.tags.0,
            container,
            file_size: u64::try_from(m.file_size.max(0)).unwrap_or(0),
            file_hash: m.file_hash,
            duration_secs: u32::try_from(m.duration_secs.max(0)).unwrap_or(0),
            bit_rate_kbps: m.bit_rate_kbps.and_then(|b| u32::try_from(b).ok()),
            width: m.width.and_then(|w| u32::try_from(w).ok()),
            height: m.height.and_then(|h| u32::try_from(h).ok()),
            video_codec: m.video_codec,
            audio_streams: m.audio_streams.0,
            subtitle_streams: m.subtitle_streams.0,
            poster_key: m.poster_key,
            source: m.source.0,
            is_missing: m.is_missing,
            created_at: m.created_at,
            updated_at: m.updated_at,
        }
    }

    fn show_to_domain(m: VideoShowModel) -> VideoShow {
        VideoShow {
            id: ShowId(m.id),
            library_id: LibraryId(m.library_id),
            show_key: m.show_key,
            title: m.title,
            summary: m.summary,
            year: m.year.and_then(|y| i32::try_from(y).ok()),
            genres: m.genres.0,
            tags: m.tags.0,
            poster_key: m.poster_key,
            season_count: u32::try_from(m.season_count.max(0)).unwrap_or(0),
            episode_count: u32::try_from(m.episode_count.max(0)).unwrap_or(0),
            created_at: m.created_at,
            updated_at: m.updated_at,
        }
    }
}

#[async_trait]
impl VideoCatalogRepository for ToastyVideoCatalogRepository {
    async fn find_video_by_id(&self, id: VideoId) -> DomainResult<Option<VideoItem>> {
        let mut db = self.db.clone();
        let row = VideoItemModel::filter_by_id(id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(row.map(Self::item_to_domain))
    }

    async fn find_videos_by_ids(&self, ids: &[VideoId]) -> DomainResult<Vec<VideoItem>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut db = self.db.clone();
        let raw_ids: Vec<Uuid> = ids.iter().map(|i| i.0).collect();
        let rows = VideoItemModel::filter(VideoItemModel::fields().id().in_list(raw_ids))
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(rows.into_iter().map(Self::item_to_domain).collect())
    }

    async fn list_videos(&self, filter: &VideoQueryFilter) -> DomainResult<Vec<VideoItem>> {
        let mut db = self.db.clone();
        let rows = if let Some(lib_id) = filter.library_id {
            VideoItemModel::filter_by_library_id(lib_id.0)
                .exec(&mut db)
                .await
                .map_err(db_err)?
        } else {
            VideoItemModel::all().exec(&mut db).await.map_err(db_err)?
        };

        let search_lower = filter
            .search
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_lowercase);
        let genre_lower = filter
            .genre
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_lowercase);
        let tag_lower = filter
            .tag
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_lowercase);

        let mut items: Vec<VideoItem> = rows
            .into_iter()
            .map(Self::item_to_domain)
            .filter(|v| !v.is_missing)
            .filter(|v| filter.show_id.is_none_or(|sid| v.show_id == Some(sid)))
            .filter(|v| match filter.kind {
                VideoKindFilter::All => true,
                VideoKindFilter::Standalone => v.show_id.is_none(),
                VideoKindFilter::Episodes => v.show_id.is_some(),
            })
            .filter(|v| filter.year.is_none_or(|y| v.year == Some(y)))
            .filter(|v| {
                genre_lower
                    .as_ref()
                    .is_none_or(|g| v.genres.iter().any(|vg| vg.to_lowercase() == *g))
            })
            .filter(|v| {
                tag_lower
                    .as_ref()
                    .is_none_or(|t| v.tags.iter().any(|vt| vt.to_lowercase() == *t))
            })
            .filter(|v| {
                search_lower.as_ref().is_none_or(|q| {
                    v.title.to_lowercase().contains(q)
                        || v.summary.to_lowercase().contains(q)
                        || v.show_title
                            .as_ref()
                            .is_some_and(|st| st.to_lowercase().contains(q))
                })
            })
            .collect();

        let desc = filter.sort_desc.unwrap_or(matches!(
            filter.sort_by,
            VideoSortBy::CreatedAt | VideoSortBy::Year | VideoSortBy::Duration
        ));

        match filter.sort_by {
            VideoSortBy::CreatedAt => items.sort_by_key(|v| v.created_at),
            VideoSortBy::Title => items.sort_by_key(|v| v.title.to_lowercase()),
            VideoSortBy::Year => items.sort_by_key(|v| v.year.unwrap_or(0)),
            VideoSortBy::Duration => items.sort_by_key(|v| v.duration_secs),
        }
        if desc {
            items.reverse();
        }
        Ok(items)
    }

    async fn list_videos_by_library(&self, library_id: LibraryId) -> DomainResult<Vec<VideoItem>> {
        let mut db = self.db.clone();
        let rows = VideoItemModel::filter_by_library_id(library_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(rows.into_iter().map(Self::item_to_domain).collect())
    }

    async fn list_episodes_by_show(&self, show_id: ShowId) -> DomainResult<Vec<VideoItem>> {
        let mut db = self.db.clone();
        let rows = VideoItemModel::filter_by_show_id(show_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        let mut items: Vec<VideoItem> = rows
            .into_iter()
            .map(Self::item_to_domain)
            .filter(|v| !v.is_missing)
            .collect();
        items.sort_by(|a, b| {
            a.season_number
                .unwrap_or(1)
                .cmp(&b.season_number.unwrap_or(1))
                .then_with(|| {
                    a.episode_number
                        .unwrap_or(u32::MAX)
                        .cmp(&b.episode_number.unwrap_or(u32::MAX))
                })
                .then_with(|| a.title.cmp(&b.title))
        });
        Ok(items)
    }

    async fn count_active_by_library(&self, library_id: LibraryId) -> DomainResult<u64> {
        let mut db = self.db.clone();
        let rows = VideoItemModel::filter_by_library_id(library_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(rows.into_iter().filter(|r| !r.is_missing).count() as u64)
    }

    async fn count_all_active_videos(&self) -> DomainResult<u64> {
        let mut db = self.db.clone();
        let rows = VideoItemModel::all().exec(&mut db).await.map_err(db_err)?;
        Ok(rows.into_iter().filter(|r| !r.is_missing).count() as u64)
    }

    async fn save_video(&self, v: &VideoItem) -> DomainResult<()> {
        let mut db = self.db.clone();
        let existing = VideoItemModel::filter_by_id(v.id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;

        let show_uuid = v.show_id.map(|s| s.0);
        let season_i64 = v.season_number.map(i64::from);
        let ep_i64 = v.episode_number.map(i64::from);
        let year_i64 = v.year.map(i64::from);
        let size_i64 = i64::try_from(v.file_size).unwrap_or(i64::MAX);
        let dur_i64 = i64::from(v.duration_secs);
        let br_i64 = v.bit_rate_kbps.map(i64::from);
        let w_i64 = v.width.map(i64::from);
        let h_i64 = v.height.map(i64::from);

        if let Some(mut rec) = existing {
            rec.update()
                .library_id(v.library_id.0)
                .show_id(show_uuid)
                .show_title(v.show_title.clone())
                .season_number(season_i64)
                .episode_number(ep_i64)
                .title(&v.title)
                .summary(&v.summary)
                .year(year_i64)
                .genres(toasty::Json(v.genres.clone()))
                .tags(toasty::Json(v.tags.clone()))
                .container(v.container.as_str())
                .file_size(size_i64)
                .file_hash(&v.file_hash)
                .duration_secs(dur_i64)
                .bit_rate_kbps(br_i64)
                .width(w_i64)
                .height(h_i64)
                .video_codec(v.video_codec.clone())
                .audio_streams(toasty::Json(v.audio_streams.clone()))
                .subtitle_streams(toasty::Json(v.subtitle_streams.clone()))
                .poster_key(v.poster_key.clone())
                .source(toasty::Json(v.source.clone()))
                .is_missing(v.is_missing)
                .updated_at(v.updated_at)
                .exec(&mut db)
                .await
                .map_err(db_err)?;
        } else {
            toasty::create!(VideoItemModel {
                id: v.id.0,
                library_id: v.library_id.0,
                show_id: show_uuid,
                show_title: v.show_title.clone(),
                season_number: season_i64,
                episode_number: ep_i64,
                title: v.title.clone(),
                summary: v.summary.clone(),
                year: year_i64,
                genres: toasty::Json(v.genres.clone()),
                tags: toasty::Json(v.tags.clone()),
                container: v.container.as_str().to_string(),
                file_size: size_i64,
                file_hash: v.file_hash.clone(),
                duration_secs: dur_i64,
                bit_rate_kbps: br_i64,
                width: w_i64,
                height: h_i64,
                video_codec: v.video_codec.clone(),
                audio_streams: toasty::Json(v.audio_streams.clone()),
                subtitle_streams: toasty::Json(v.subtitle_streams.clone()),
                poster_key: v.poster_key.clone(),
                source: toasty::Json(v.source.clone()),
                is_missing: v.is_missing,
                created_at: v.created_at,
                updated_at: v.updated_at,
            })
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        }
        Ok(())
    }

    async fn delete_video(&self, id: VideoId) -> DomainResult<()> {
        let mut db = self.db.clone();
        VideoItemModel::filter_by_id(id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    async fn delete_videos_by_library(&self, library_id: LibraryId) -> DomainResult<()> {
        let mut db = self.db.clone();
        for row in VideoItemModel::filter_by_library_id(library_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?
        {
            row.delete().exec(&mut db).await.map_err(db_err)?;
        }
        Ok(())
    }

    async fn find_show_by_id(&self, id: ShowId) -> DomainResult<Option<VideoShow>> {
        let mut db = self.db.clone();
        let row = VideoShowModel::filter_by_id(id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(row.map(Self::show_to_domain))
    }

    async fn find_show_by_key(&self, show_key: &str) -> DomainResult<Option<VideoShow>> {
        let mut db = self.db.clone();
        let row = VideoShowModel::filter_by_show_key(show_key)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(row.map(Self::show_to_domain))
    }

    async fn list_shows(&self, filter: &ShowQueryFilter) -> DomainResult<Vec<VideoShow>> {
        let mut db = self.db.clone();
        let rows = if let Some(lib_id) = filter.library_id {
            VideoShowModel::filter_by_library_id(lib_id.0)
                .exec(&mut db)
                .await
                .map_err(db_err)?
        } else {
            VideoShowModel::all().exec(&mut db).await.map_err(db_err)?
        };

        let search_lower = filter
            .search
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_lowercase);
        let genre_lower = filter
            .genre
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_lowercase);

        let mut shows: Vec<VideoShow> = rows
            .into_iter()
            .map(Self::show_to_domain)
            .filter(|s| s.episode_count > 0)
            .filter(|s| {
                genre_lower
                    .as_ref()
                    .is_none_or(|g| s.genres.iter().any(|sg| sg.to_lowercase() == *g))
            })
            .filter(|s| {
                search_lower.as_ref().is_none_or(|q| {
                    s.title.to_lowercase().contains(q) || s.summary.to_lowercase().contains(q)
                })
            })
            .collect();
        shows.sort_by_key(|s| s.title.to_lowercase());
        Ok(shows)
    }

    async fn save_show(&self, s: &VideoShow) -> DomainResult<()> {
        let mut db = self.db.clone();
        let existing = VideoShowModel::filter_by_id(s.id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        let year_i64 = s.year.map(i64::from);
        let seasons_i64 = i64::from(s.season_count);
        let eps_i64 = i64::from(s.episode_count);

        if let Some(mut rec) = existing {
            rec.update()
                .library_id(s.library_id.0)
                .show_key(&s.show_key)
                .title(&s.title)
                .summary(&s.summary)
                .year(year_i64)
                .genres(toasty::Json(s.genres.clone()))
                .tags(toasty::Json(s.tags.clone()))
                .poster_key(s.poster_key.clone())
                .season_count(seasons_i64)
                .episode_count(eps_i64)
                .updated_at(s.updated_at)
                .exec(&mut db)
                .await
                .map_err(db_err)?;
        } else {
            toasty::create!(VideoShowModel {
                id: s.id.0,
                library_id: s.library_id.0,
                show_key: s.show_key.clone(),
                title: s.title.clone(),
                summary: s.summary.clone(),
                year: year_i64,
                genres: toasty::Json(s.genres.clone()),
                tags: toasty::Json(s.tags.clone()),
                poster_key: s.poster_key.clone(),
                season_count: seasons_i64,
                episode_count: eps_i64,
                created_at: s.created_at,
                updated_at: s.updated_at,
            })
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        }
        Ok(())
    }

    async fn delete_show(&self, id: ShowId) -> DomainResult<()> {
        let mut db = self.db.clone();
        VideoShowModel::filter_by_id(id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    async fn recompute_show_aggregates(&self) -> DomainResult<()> {
        let mut db = self.db.clone();
        let all_videos = VideoItemModel::all().exec(&mut db).await.map_err(db_err)?;
        let all_shows = VideoShowModel::all().exec(&mut db).await.map_err(db_err)?;

        let mut ep_counts: HashMap<Uuid, i64> = HashMap::new();
        let mut seasons_by_show: HashMap<Uuid, HashSet<i64>> = HashMap::new();
        let mut fallback_poster: HashMap<Uuid, String> = HashMap::new();

        for v in all_videos {
            if v.is_missing {
                continue;
            }
            if let Some(sid) = v.show_id {
                *ep_counts.entry(sid).or_default() += 1;
                seasons_by_show
                    .entry(sid)
                    .or_default()
                    .insert(v.season_number.unwrap_or(1));
                if let Some(ref pk) = v.poster_key {
                    fallback_poster.entry(sid).or_insert_with(|| pk.clone());
                }
            }
        }

        for mut show in all_shows {
            let ep_count = ep_counts.get(&show.id).copied().unwrap_or(0);
            if ep_count == 0 {
                show.delete().exec(&mut db).await.map_err(db_err)?;
                continue;
            }
            let season_count = seasons_by_show
                .get(&show.id)
                .map(|set| set.len() as i64)
                .unwrap_or(1)
                .max(1);
            let poster = show
                .poster_key
                .clone()
                .or_else(|| fallback_poster.get(&show.id).cloned());
            show.update()
                .episode_count(ep_count)
                .season_count(season_count)
                .poster_key(poster)
                .exec(&mut db)
                .await
                .map_err(db_err)?;
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct ToastyVideoWatchRepository {
    db: Db,
}

impl ToastyVideoWatchRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    fn progress_to_domain(m: VideoProgressModel) -> WatchProgress {
        WatchProgress {
            user_id: UserId(m.user_id),
            video_id: VideoId(m.video_id),
            position_secs: (m.position_ms.max(0) as f64) / 1000.0,
            duration_secs: u32::try_from(m.duration_secs.max(0)).unwrap_or(0),
            completed: m.completed,
            selected_audio_stream: m.selected_audio_stream.and_then(|v| u32::try_from(v).ok()),
            selected_subtitle: m.selected_subtitle,
            updated_at: m.updated_at,
        }
    }
}

#[async_trait]
impl VideoWatchRepository for ToastyVideoWatchRepository {
    async fn get_progress(
        &self,
        user_id: UserId,
        video_id: VideoId,
    ) -> DomainResult<Option<WatchProgress>> {
        let mut db = self.db.clone();
        let key = format!("{}:{}", user_id.0, video_id.0);
        let row = VideoProgressModel::filter_by_progress_key(&key)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(row.map(Self::progress_to_domain))
    }

    async fn list_user_progress(&self, user_id: UserId) -> DomainResult<Vec<WatchProgress>> {
        let mut db = self.db.clone();
        let rows = VideoProgressModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(rows.into_iter().map(Self::progress_to_domain).collect())
    }

    async fn save_progress(&self, p: &WatchProgress) -> DomainResult<()> {
        let mut db = self.db.clone();
        let key = format!("{}:{}", p.user_id.0, p.video_id.0);
        let existing = VideoProgressModel::filter_by_progress_key(&key)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;

        let pos_ms = (p.position_secs.max(0.0) * 1000.0).round() as i64;
        let dur = i64::from(p.duration_secs);
        let audio = p.selected_audio_stream.map(i64::from);

        if let Some(mut rec) = existing {
            rec.update()
                .position_ms(pos_ms)
                .duration_secs(dur)
                .completed(p.completed)
                .selected_audio_stream(audio)
                .selected_subtitle(p.selected_subtitle.clone())
                .updated_at(p.updated_at)
                .exec(&mut db)
                .await
                .map_err(db_err)?;
        } else {
            toasty::create!(VideoProgressModel {
                progress_key: key,
                user_id: p.user_id.0,
                video_id: p.video_id.0,
                position_ms: pos_ms,
                duration_secs: dur,
                completed: p.completed,
                selected_audio_stream: audio,
                selected_subtitle: p.selected_subtitle.clone(),
                updated_at: p.updated_at,
            })
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        }
        Ok(())
    }

    async fn delete_progress_by_video(&self, video_id: VideoId) -> DomainResult<()> {
        let mut db = self.db.clone();
        for row in VideoProgressModel::filter_by_video_id(video_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?
        {
            row.delete().exec(&mut db).await.map_err(db_err)?;
        }
        Ok(())
    }

    async fn set_favorite(
        &self,
        user_id: UserId,
        kind: VideoFavoriteKind,
        target_id: Uuid,
    ) -> DomainResult<()> {
        let mut db = self.db.clone();
        let key = format!("{}:{}:{target_id}", user_id.0, kind.as_str());
        let existing = VideoFavoriteModel::filter_by_fav_key(&key)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        if existing.is_none() {
            toasty::create!(VideoFavoriteModel {
                fav_key: key,
                user_id: user_id.0,
                kind: kind.as_str().to_string(),
                target_id,
                created_at: Timestamp::now(),
            })
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        }
        Ok(())
    }

    async fn unset_favorite(
        &self,
        user_id: UserId,
        kind: VideoFavoriteKind,
        target_id: Uuid,
    ) -> DomainResult<()> {
        let mut db = self.db.clone();
        let key = format!("{}:{}:{target_id}", user_id.0, kind.as_str());
        VideoFavoriteModel::filter_by_fav_key(&key)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    async fn list_favorites(
        &self,
        user_id: UserId,
        kind: Option<VideoFavoriteKind>,
    ) -> DomainResult<Vec<VideoFavoriteEntry>> {
        let mut db = self.db.clone();
        let rows = VideoFavoriteModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        let mut out = Vec::new();
        for r in rows {
            let parsed = VideoFavoriteKind::parse(&r.kind)?;
            if kind.is_none_or(|k| k == parsed) {
                out.push(VideoFavoriteEntry {
                    user_id: UserId(r.user_id),
                    kind: parsed,
                    target_id: r.target_id,
                    created_at: r.created_at,
                });
            }
        }
        out.sort_by_key(|e| std::cmp::Reverse(e.created_at));
        Ok(out)
    }

    async fn list_shelves(&self, user_id: UserId) -> DomainResult<Vec<VideoShelf>> {
        let mut db = self.db.clone();
        let rows = VideoShelfModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        let mut out: Vec<VideoShelf> = rows
            .into_iter()
            .map(|r| VideoShelf {
                id: VideoShelfId(r.id),
                user_id: UserId(r.user_id),
                name: r.name,
                description: r.description,
                video_ids: r.video_ids.0,
                created_at: r.created_at,
                updated_at: r.updated_at,
            })
            .collect();
        out.sort_by_key(|s| std::cmp::Reverse(s.updated_at));
        Ok(out)
    }

    async fn find_shelf_by_id(&self, id: VideoShelfId) -> DomainResult<Option<VideoShelf>> {
        let mut db = self.db.clone();
        let row = VideoShelfModel::filter_by_id(id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(row.map(|r| VideoShelf {
            id: VideoShelfId(r.id),
            user_id: UserId(r.user_id),
            name: r.name,
            description: r.description,
            video_ids: r.video_ids.0,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }))
    }

    async fn save_shelf(&self, s: &VideoShelf) -> DomainResult<()> {
        let mut db = self.db.clone();
        let existing = VideoShelfModel::filter_by_id(s.id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        if let Some(mut rec) = existing {
            rec.update()
                .name(&s.name)
                .description(&s.description)
                .video_ids(toasty::Json(s.video_ids.clone()))
                .updated_at(s.updated_at)
                .exec(&mut db)
                .await
                .map_err(db_err)?;
        } else {
            toasty::create!(VideoShelfModel {
                id: s.id.0,
                user_id: s.user_id.0,
                name: s.name.clone(),
                description: s.description.clone(),
                video_ids: toasty::Json(s.video_ids.clone()),
                created_at: s.created_at,
                updated_at: s.updated_at,
            })
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        }
        Ok(())
    }

    async fn delete_shelf(&self, id: VideoShelfId) -> DomainResult<()> {
        let mut db = self.db.clone();
        VideoShelfModel::filter_by_id(id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    async fn delete_by_user(&self, user_id: UserId) -> DomainResult<()> {
        let mut db = self.db.clone();
        for r in VideoProgressModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?
        {
            r.delete().exec(&mut db).await.map_err(db_err)?;
        }
        for r in VideoFavoriteModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?
        {
            r.delete().exec(&mut db).await.map_err(db_err)?;
        }
        for r in VideoShelfModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?
        {
            r.delete().exec(&mut db).await.map_err(db_err)?;
        }
        Ok(())
    }
}

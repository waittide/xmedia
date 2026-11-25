use std::collections::{HashMap, HashSet};

use async_trait::async_trait;
use uuid::Uuid;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::domain::library::LibraryId;

use crate::domain::id::{ShowId, VideoId};
use crate::domain::repository::VideoCatalogRepository;
use crate::domain::video::{ShowQueryFilter, ShowSource, VideoItem, VideoQueryFilter, VideoShow};

use super::super::models::{VideoFavoriteModel, VideoItemModel, VideoShowModel};
use super::{ToastyVideoCatalogRepository, db_err};

impl ToastyVideoCatalogRepository {
    pub(super) fn show_to_domain(m: VideoShowModel) -> VideoShow {
        let source = if m.source_kind == "online_tracked"
            && let (Some(source_id), Some(item_id)) = (m.online_source_id, m.online_item_id)
            && !item_id.trim().is_empty()
        {
            ShowSource::OnlineTracked {
                source_id,
                item_id,
                detail_url: m.online_detail_url,
            }
        } else {
            ShowSource::Local
        };
        VideoShow {
            id: ShowId(m.id),
            library_id: LibraryId(m.library_id),
            show_key: m.show_key,
            source,
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
        self.find_video_by_id_impl(id).await
    }

    async fn find_videos_by_ids(&self, ids: &[VideoId]) -> DomainResult<Vec<VideoItem>> {
        self.find_videos_by_ids_impl(ids).await
    }

    async fn find_videos_by_poster_key(&self, poster_key: &str) -> DomainResult<Vec<VideoItem>> {
        self.find_videos_by_poster_key_impl(poster_key).await
    }

    async fn list_videos(&self, filter: &VideoQueryFilter) -> DomainResult<Vec<VideoItem>> {
        self.list_videos_impl(filter).await
    }

    async fn list_videos_by_library(&self, library_id: LibraryId) -> DomainResult<Vec<VideoItem>> {
        self.list_videos_by_library_impl(library_id).await
    }

    async fn list_episodes_by_show(&self, show_id: ShowId) -> DomainResult<Vec<VideoItem>> {
        self.list_episodes_by_show_impl(show_id).await
    }

    async fn save_video(&self, v: &VideoItem) -> DomainResult<()> {
        self.save_video_impl(v).await
    }

    async fn delete_video(&self, id: VideoId) -> DomainResult<()> {
        self.delete_video_impl(id).await
    }

    async fn delete_videos_by_library(&self, library_id: LibraryId) -> DomainResult<()> {
        self.delete_videos_by_library_impl(library_id).await
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

    async fn find_online_tracked_show(
        &self,
        source_id: Uuid,
        item_id: &str,
    ) -> DomainResult<Option<VideoShow>> {
        let clean_item = item_id.trim();
        if clean_item.is_empty() {
            return Ok(None);
        }
        let mut db = self.db.clone();
        let rows = VideoShowModel::filter_by_online_source_id(source_id)
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(rows.into_iter().map(Self::show_to_domain).find(|s| {
            matches!(
                &s.source,
                ShowSource::OnlineTracked {
                    source_id: sid,
                    item_id: iid,
                    ..
                } if *sid == source_id && iid == clean_item
            )
        }))
    }

    async fn find_shows_by_poster_key(&self, poster_key: &str) -> DomainResult<Vec<VideoShow>> {
        let clean = poster_key.trim();
        if clean.is_empty() {
            return Ok(Vec::new());
        }
        let mut db = self.db.clone();
        let rows = VideoShowModel::filter_by_poster_key(clean.to_string())
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(rows.into_iter().map(Self::show_to_domain).collect())
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
        let tag_lower = filter
            .tag
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
                tag_lower
                    .as_ref()
                    .is_none_or(|t| s.tags.iter().any(|st| st.to_lowercase() == *t))
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
        let (source_kind, online_source_id, online_item_id, online_detail_url) = match &s.source {
            ShowSource::Local => ("local".to_string(), None, None, None),
            ShowSource::OnlineTracked {
                source_id,
                item_id,
                detail_url,
            } => (
                "online_tracked".to_string(),
                Some(*source_id),
                Some(item_id.clone()),
                detail_url.clone(),
            ),
        };

        if let Some(mut rec) = existing {
            rec.update()
                .library_id(s.library_id.0)
                .show_key(&s.show_key)
                .source_kind(source_kind)
                .online_source_id(online_source_id)
                .online_item_id(online_item_id)
                .online_detail_url(online_detail_url)
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
                source_kind,
                online_source_id,
                online_item_id,
                online_detail_url,
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
        let mut total_refs: HashMap<Uuid, i64> = HashMap::new();
        let mut seasons_by_show: HashMap<Uuid, HashSet<i64>> = HashMap::new();
        let mut fallback_poster: HashMap<Uuid, String> = HashMap::new();

        for v in all_videos {
            if let Some(sid) = v.show_id {
                *total_refs.entry(sid).or_default() += 1;
                if v.is_missing {
                    continue;
                }
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

        let mut orphan_shows: Vec<VideoShowModel> = Vec::new();
        for mut show in all_shows {
            let total_ref_count = total_refs.get(&show.id).copied().unwrap_or(0);
            if total_ref_count == 0 {
                orphan_shows.push(show);
                continue;
            }
            let ep_count = ep_counts.get(&show.id).copied().unwrap_or(0);
            let season_count = seasons_by_show
                .get(&show.id)
                .map(|set| set.len() as i64)
                .unwrap_or(0);
            let poster = show
                .poster_key
                .clone()
                .or_else(|| fallback_poster.get(&show.id).cloned());
            let changed = show.episode_count != ep_count
                || show.season_count != season_count
                || show.poster_key != poster;
            if changed {
                show.update()
                    .episode_count(ep_count)
                    .season_count(season_count)
                    .poster_key(poster)
                    .exec(&mut db)
                    .await
                    .map_err(db_err)?;
            }
        }
        if !orphan_shows.is_empty() {
            let orphan_ids: HashSet<Uuid> = orphan_shows.iter().map(|s| s.id).collect();
            let favs = VideoFavoriteModel::all()
                .exec(&mut db)
                .await
                .map_err(db_err)?;
            for fav in favs {
                if fav.kind == "show" && orphan_ids.contains(&fav.target_id) {
                    fav.delete().exec(&mut db).await.map_err(db_err)?;
                }
            }
            for show in orphan_shows {
                show.delete().exec(&mut db).await.map_err(db_err)?;
            }
        }
        Ok(())
    }
}

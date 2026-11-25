use uuid::Uuid;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::domain::library::LibraryId;

use crate::domain::id::{ShowId, VideoId};
use crate::domain::video::{
    VideoContainer, VideoItem, VideoKindFilter, VideoQueryFilter, VideoSortBy,
};

use super::super::models::VideoItemModel;
use super::{ToastyVideoCatalogRepository, db_err};

impl ToastyVideoCatalogRepository {
    pub(super) fn item_to_domain(m: VideoItemModel) -> VideoItem {
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

    pub(super) async fn find_video_by_id_impl(
        &self,
        id: VideoId,
    ) -> DomainResult<Option<VideoItem>> {
        let mut db = self.db.clone();
        let row = VideoItemModel::filter_by_id(id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(row.map(Self::item_to_domain))
    }

    pub(super) async fn find_videos_by_ids_impl(
        &self,
        ids: &[VideoId],
    ) -> DomainResult<Vec<VideoItem>> {
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

    pub(super) async fn find_videos_by_poster_key_impl(
        &self,
        poster_key: &str,
    ) -> DomainResult<Vec<VideoItem>> {
        let clean = poster_key.trim();
        if clean.is_empty() {
            return Ok(Vec::new());
        }
        let mut db = self.db.clone();
        let rows = VideoItemModel::filter_by_poster_key(clean.to_string())
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(rows
            .into_iter()
            .filter(|r| !r.is_missing)
            .map(Self::item_to_domain)
            .collect())
    }

    pub(super) async fn list_videos_impl(
        &self,
        filter: &VideoQueryFilter,
    ) -> DomainResult<Vec<VideoItem>> {
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

    pub(super) async fn list_videos_by_library_impl(
        &self,
        library_id: LibraryId,
    ) -> DomainResult<Vec<VideoItem>> {
        let mut db = self.db.clone();
        let rows = VideoItemModel::filter_by_library_id(library_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(rows.into_iter().map(Self::item_to_domain).collect())
    }

    pub(super) async fn list_episodes_by_show_impl(
        &self,
        show_id: ShowId,
    ) -> DomainResult<Vec<VideoItem>> {
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

    pub(super) async fn save_video_impl(&self, v: &VideoItem) -> DomainResult<()> {
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

    pub(super) async fn delete_video_impl(&self, id: VideoId) -> DomainResult<()> {
        let mut db = self.db.clone();
        VideoItemModel::filter_by_id(id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    pub(super) async fn delete_videos_by_library_impl(
        &self,
        library_id: LibraryId,
    ) -> DomainResult<()> {
        let mut db = self.db.clone();
        VideoItemModel::filter_by_library_id(library_id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(())
    }
}

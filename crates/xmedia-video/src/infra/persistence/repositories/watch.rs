use std::collections::HashSet;

use async_trait::async_trait;
use jiff::Timestamp;
use toasty::Db;
use uuid::Uuid;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::domain::user::UserId;

use crate::domain::id::{ShowId, VideoId, VideoShelfId};
use crate::domain::repository::VideoWatchRepository;
use crate::domain::watch::{VideoFavoriteEntry, VideoFavoriteKind, VideoShelf, WatchProgress};

use super::super::models::{VideoFavoriteModel, VideoProgressModel, VideoShelfModel};
use super::db_err;

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
        VideoProgressModel::filter_by_video_id(video_id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    async fn delete_video_references(
        &self,
        video_id: VideoId,
        deleted_show_id: Option<ShowId>,
    ) -> DomainResult<()> {
        let mut db = self.db.clone();
        for row in VideoFavoriteModel::all()
            .exec(&mut db)
            .await
            .map_err(db_err)?
        {
            let match_video = row.kind == "video" && row.target_id == video_id.0;
            let match_show =
                row.kind == "show" && deleted_show_id.is_some_and(|sid| sid.0 == row.target_id);
            if match_video || match_show {
                row.delete().exec(&mut db).await.map_err(db_err)?;
            }
        }
        for mut shelf in VideoShelfModel::all().exec(&mut db).await.map_err(db_err)? {
            if shelf.video_ids.0.contains(&video_id) {
                let filtered: Vec<VideoId> = shelf
                    .video_ids
                    .0
                    .iter()
                    .copied()
                    .filter(|&id| id != video_id)
                    .collect();
                shelf
                    .update()
                    .video_ids(toasty::Json(filtered))
                    .updated_at(Timestamp::now())
                    .exec(&mut db)
                    .await
                    .map_err(db_err)?;
            }
        }
        Ok(())
    }

    async fn delete_library_references(
        &self,
        video_ids: &[VideoId],
        show_ids: &[ShowId],
    ) -> DomainResult<()> {
        if video_ids.is_empty() && show_ids.is_empty() {
            return Ok(());
        }
        let mut db = self.db.clone();
        let video_id_set: HashSet<VideoId> = video_ids.iter().copied().collect();
        let video_uuid_set: HashSet<Uuid> = video_ids.iter().map(|id| id.0).collect();
        let show_uuid_set: HashSet<Uuid> = show_ids.iter().map(|id| id.0).collect();

        if !video_uuid_set.is_empty() {
            let raw_video_ids: Vec<Uuid> = video_uuid_set.iter().copied().collect();
            for chunk in raw_video_ids.chunks(200) {
                for row in VideoProgressModel::filter(
                    VideoProgressModel::fields()
                        .video_id()
                        .in_list(chunk.to_vec()),
                )
                .exec(&mut db)
                .await
                .map_err(db_err)?
                {
                    row.delete().exec(&mut db).await.map_err(db_err)?;
                }
            }
        }

        for row in VideoFavoriteModel::all()
            .exec(&mut db)
            .await
            .map_err(db_err)?
        {
            let match_video = row.kind == "video" && video_uuid_set.contains(&row.target_id);
            let match_show = row.kind == "show" && show_uuid_set.contains(&row.target_id);
            if match_video || match_show {
                row.delete().exec(&mut db).await.map_err(db_err)?;
            }
        }

        if !video_id_set.is_empty() {
            for mut shelf in VideoShelfModel::all().exec(&mut db).await.map_err(db_err)? {
                if shelf.video_ids.0.iter().any(|id| video_id_set.contains(id)) {
                    let filtered: Vec<VideoId> = shelf
                        .video_ids
                        .0
                        .iter()
                        .copied()
                        .filter(|id| !video_id_set.contains(id))
                        .collect();
                    shelf
                        .update()
                        .video_ids(toasty::Json(filtered))
                        .updated_at(Timestamp::now())
                        .exec(&mut db)
                        .await
                        .map_err(db_err)?;
                }
            }
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
        VideoProgressModel::filter_by_user_id(user_id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        VideoFavoriteModel::filter_by_user_id(user_id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        VideoShelfModel::filter_by_user_id(user_id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    async fn delete_by_user_and_targets(
        &self,
        user_id: UserId,
        video_ids: &[VideoId],
        show_id: Option<ShowId>,
    ) -> DomainResult<()> {
        let mut db = self.db.clone();
        let video_set: HashSet<VideoId> = video_ids.iter().copied().collect();

        for &vid in video_ids {
            let p_key = format!("{}:{}", user_id.0, vid.0);
            VideoProgressModel::filter_by_progress_key(&p_key)
                .delete()
                .exec(&mut db)
                .await
                .map_err(db_err)?;
            let f_key = format!(
                "{}:{}:{}",
                user_id.0,
                VideoFavoriteKind::Video.as_str(),
                vid.0
            );
            VideoFavoriteModel::filter_by_fav_key(&f_key)
                .delete()
                .exec(&mut db)
                .await
                .map_err(db_err)?;
        }

        if let Some(sid) = show_id {
            let f_key = format!(
                "{}:{}:{}",
                user_id.0,
                VideoFavoriteKind::Show.as_str(),
                sid.0
            );
            VideoFavoriteModel::filter_by_fav_key(&f_key)
                .delete()
                .exec(&mut db)
                .await
                .map_err(db_err)?;
        }

        if !video_set.is_empty() {
            for mut shelf in VideoShelfModel::filter_by_user_id(user_id.0)
                .exec(&mut db)
                .await
                .map_err(db_err)?
            {
                if shelf.video_ids.0.iter().any(|id| video_set.contains(id)) {
                    let filtered: Vec<VideoId> = shelf
                        .video_ids
                        .0
                        .iter()
                        .copied()
                        .filter(|id| !video_set.contains(id))
                        .collect();
                    shelf
                        .update()
                        .video_ids(toasty::Json(filtered))
                        .updated_at(Timestamp::now())
                        .exec(&mut db)
                        .await
                        .map_err(db_err)?;
                }
            }
        }
        Ok(())
    }
}

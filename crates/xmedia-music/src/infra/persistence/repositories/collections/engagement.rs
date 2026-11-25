use jiff::Timestamp;
use uuid::Uuid;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::domain::user::UserId;

use super::super::db_err;
use super::ToastyMusicCollectionRepository;
use crate::domain::catalog::{AlbumId, TrackId};
use crate::domain::collection::{
    AudioBookmark, PlayQueue, RatingEntry, ScrobbleEntry, StarTargetKind, StarredEntry,
    TrackPlayStat,
};
use crate::infra::persistence::models::{
    MusicBookmarkModel, MusicPlayQueueModel, MusicPlayStatModel, MusicPlaylistModel,
    MusicRatingModel, MusicScrobbleModel, MusicStarredModel,
};

impl ToastyMusicCollectionRepository {
    pub(super) async fn set_starred_impl(
        &self,
        user_id: UserId,
        kind: StarTargetKind,
        target_id: Uuid,
        starred_at: Timestamp,
    ) -> DomainResult<()> {
        let mut db = self.db.clone();
        let key = format!("{}:{}:{target_id}", user_id.0, kind.as_str());
        if MusicStarredModel::filter_by_star_key(&key)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?
            .is_none()
        {
            toasty::create!(MusicStarredModel {
                star_key: key,
                user_id: user_id.0,
                kind: kind.as_str().to_string(),
                target_id: target_id,
                starred_at: starred_at,
            })
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        }
        Ok(())
    }

    pub(super) async fn unset_starred_impl(
        &self,
        user_id: UserId,
        kind: StarTargetKind,
        target_id: Uuid,
    ) -> DomainResult<()> {
        let mut db = self.db.clone();
        let key = format!("{}:{}:{target_id}", user_id.0, kind.as_str());
        MusicStarredModel::filter_by_star_key(&key)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    pub(super) async fn list_starred_impl(
        &self,
        user_id: UserId,
        kind: Option<StarTargetKind>,
    ) -> DomainResult<Vec<StarredEntry>> {
        let mut db = self.db.clone();
        let rows = MusicStarredModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?;

        let mut out = Vec::new();
        for r in rows {
            let parsed = StarTargetKind::parse(&r.kind)?;
            if kind.is_none_or(|k| k == parsed) {
                out.push(StarredEntry {
                    user_id: UserId(r.user_id),
                    kind: parsed,
                    target_id: r.target_id,
                    starred_at: r.starred_at,
                });
            }
        }
        Ok(out)
    }

    pub(super) async fn set_rating_impl(
        &self,
        user_id: UserId,
        kind: StarTargetKind,
        target_id: Uuid,
        rating: u8,
    ) -> DomainResult<()> {
        let mut db = self.db.clone();
        let key = format!("{}:{}:{target_id}", user_id.0, kind.as_str());
        let now = Timestamp::now();
        let existing = MusicRatingModel::filter_by_rating_key(&key)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;

        if let Some(mut rec) = existing {
            rec.update()
                .rating(i64::from(rating))
                .updated_at(now)
                .exec(&mut db)
                .await
                .map_err(db_err)?;
        } else {
            toasty::create!(MusicRatingModel {
                rating_key: key,
                user_id: user_id.0,
                kind: kind.as_str().to_string(),
                target_id: target_id,
                rating: i64::from(rating),
                updated_at: now,
            })
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        }
        Ok(())
    }

    pub(super) async fn remove_rating_impl(
        &self,
        user_id: UserId,
        kind: StarTargetKind,
        target_id: Uuid,
    ) -> DomainResult<()> {
        let mut db = self.db.clone();
        let key = format!("{}:{}:{target_id}", user_id.0, kind.as_str());
        MusicRatingModel::filter_by_rating_key(&key)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    pub(super) async fn list_user_ratings_impl(
        &self,
        user_id: UserId,
    ) -> DomainResult<Vec<RatingEntry>> {
        let mut db = self.db.clone();
        let rows = MusicRatingModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(RatingEntry {
                user_id: UserId(r.user_id),
                kind: StarTargetKind::parse(&r.kind)?,
                target_id: r.target_id,
                rating: u8::try_from(r.rating.clamp(1, 5)).unwrap_or(5),
                updated_at: r.updated_at,
            });
        }
        Ok(out)
    }

    pub(super) async fn record_play_impl(
        &self,
        user_id: UserId,
        track_id: TrackId,
        album_id: AlbumId,
        played_at: Timestamp,
    ) -> DomainResult<TrackPlayStat> {
        let mut db = self.db.clone();
        let stat_key = format!("{}:{}", user_id.0, track_id.0);
        let existing = MusicPlayStatModel::filter_by_stat_key(&stat_key)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;

        let (new_count, effective_last_played) = if let Some(mut rec) = existing {
            let next = rec.play_count.saturating_add(1);
            let latest = played_at.max(rec.last_played_at);
            rec.update()
                .album_id(album_id.0)
                .play_count(next)
                .last_played_at(latest)
                .exec(&mut db)
                .await
                .map_err(db_err)?;
            (u64::try_from(next.max(1)).unwrap_or(1), latest)
        } else {
            toasty::create!(MusicPlayStatModel {
                stat_key: stat_key,
                user_id: user_id.0,
                track_id: track_id.0,
                album_id: album_id.0,
                play_count: 1,
                last_played_at: played_at,
            })
            .exec(&mut db)
            .await
            .map_err(db_err)?;
            (1, played_at)
        };

        toasty::create!(MusicScrobbleModel {
            id: Uuid::now_v7(),
            user_id: user_id.0,
            track_id: track_id.0,
            played_at: played_at,
        })
        .exec(&mut db)
        .await
        .map_err(db_err)?;

        Ok(TrackPlayStat {
            user_id,
            track_id,
            album_id,
            play_count: new_count,
            last_played_at: effective_last_played,
        })
    }

    pub(super) async fn list_user_play_stats_impl(
        &self,
        user_id: UserId,
    ) -> DomainResult<Vec<TrackPlayStat>> {
        let mut db = self.db.clone();
        let rows = MusicPlayStatModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(rows
            .into_iter()
            .map(|r| TrackPlayStat {
                user_id: UserId(r.user_id),
                track_id: TrackId(r.track_id),
                album_id: AlbumId(r.album_id),
                play_count: u64::try_from(r.play_count.max(0)).unwrap_or(0),
                last_played_at: r.last_played_at,
            })
            .collect())
    }

    pub(super) async fn list_recent_scrobbles_impl(
        &self,
        user_id: UserId,
        limit: u32,
    ) -> DomainResult<Vec<ScrobbleEntry>> {
        let mut db = self.db.clone();
        let mut rows = MusicScrobbleModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        rows.sort_by_key(|r| std::cmp::Reverse(r.played_at));
        rows.truncate(limit as usize);
        Ok(rows
            .into_iter()
            .map(|r| ScrobbleEntry {
                id: r.id,
                user_id: UserId(r.user_id),
                track_id: TrackId(r.track_id),
                played_at: r.played_at,
            })
            .collect())
    }

    pub(super) async fn get_play_queue_impl(
        &self,
        user_id: UserId,
    ) -> DomainResult<Option<PlayQueue>> {
        let mut db = self.db.clone();
        let row = MusicPlayQueueModel::filter_by_user_id(user_id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(row.map(|m| PlayQueue {
            user_id: UserId(m.user_id),
            track_ids: m.track_ids.0,
            current_track_id: m.current_track_id.map(TrackId),
            position_ms: u64::try_from(m.position_ms.max(0)).unwrap_or(0),
            changed_by: m.changed_by,
            updated_at: m.updated_at,
        }))
    }

    pub(super) async fn save_play_queue_impl(&self, queue: &PlayQueue) -> DomainResult<()> {
        let mut db = self.db.clone();
        let existing = MusicPlayQueueModel::filter_by_user_id(queue.user_id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        let pos = i64::try_from(queue.position_ms).unwrap_or(i64::MAX);
        let cur = queue.current_track_id.map(|t| t.0);

        if let Some(mut rec) = existing {
            rec.update()
                .track_ids(toasty::Json(queue.track_ids.clone()))
                .current_track_id(cur)
                .position_ms(pos)
                .changed_by(queue.changed_by.clone())
                .updated_at(queue.updated_at)
                .exec(&mut db)
                .await
                .map_err(db_err)?;
        } else {
            toasty::create!(MusicPlayQueueModel {
                user_id: queue.user_id.0,
                track_ids: toasty::Json(queue.track_ids.clone()),
                current_track_id: cur,
                position_ms: pos,
                changed_by: queue.changed_by.clone(),
                updated_at: queue.updated_at,
            })
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        }
        Ok(())
    }

    pub(super) async fn list_bookmarks_impl(
        &self,
        user_id: UserId,
    ) -> DomainResult<Vec<AudioBookmark>> {
        let mut db = self.db.clone();
        let mut rows = MusicBookmarkModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        rows.sort_by_key(|r| std::cmp::Reverse(r.updated_at));
        Ok(rows
            .into_iter()
            .map(|r| AudioBookmark {
                user_id: UserId(r.user_id),
                track_id: TrackId(r.track_id),
                position_ms: u64::try_from(r.position_ms.max(0)).unwrap_or(0),
                comment: r.comment,
                created_at: r.created_at,
                updated_at: r.updated_at,
            })
            .collect())
    }

    pub(super) async fn get_bookmark_impl(
        &self,
        user_id: UserId,
        track_id: TrackId,
    ) -> DomainResult<Option<AudioBookmark>> {
        let mut db = self.db.clone();
        let key = format!("{}:{}", user_id.0, track_id.0);
        let row = MusicBookmarkModel::filter_by_bookmark_key(&key)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(row.map(|r| AudioBookmark {
            user_id: UserId(r.user_id),
            track_id: TrackId(r.track_id),
            position_ms: u64::try_from(r.position_ms.max(0)).unwrap_or(0),
            comment: r.comment,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }))
    }

    pub(super) async fn save_bookmark_impl(&self, bookmark: &AudioBookmark) -> DomainResult<()> {
        let mut db = self.db.clone();
        let key = format!("{}:{}", bookmark.user_id.0, bookmark.track_id.0);
        let existing = MusicBookmarkModel::filter_by_bookmark_key(&key)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        let pos = i64::try_from(bookmark.position_ms).unwrap_or(i64::MAX);

        if let Some(mut rec) = existing {
            rec.update()
                .position_ms(pos)
                .comment(bookmark.comment.clone())
                .updated_at(bookmark.updated_at)
                .exec(&mut db)
                .await
                .map_err(db_err)?;
        } else {
            toasty::create!(MusicBookmarkModel {
                bookmark_key: key,
                user_id: bookmark.user_id.0,
                track_id: bookmark.track_id.0,
                position_ms: pos,
                comment: bookmark.comment.clone(),
                created_at: bookmark.created_at,
                updated_at: bookmark.updated_at,
            })
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        }
        Ok(())
    }

    pub(super) async fn delete_bookmark_impl(
        &self,
        user_id: UserId,
        track_id: TrackId,
    ) -> DomainResult<()> {
        let mut db = self.db.clone();
        let key = format!("{}:{}", user_id.0, track_id.0);
        MusicBookmarkModel::filter_by_bookmark_key(&key)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    pub(super) async fn delete_by_user_impl(&self, user_id: UserId) -> DomainResult<()> {
        let mut db = self.db.clone();
        MusicPlaylistModel::filter_by_owner_id(user_id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        MusicStarredModel::filter_by_user_id(user_id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        MusicRatingModel::filter_by_user_id(user_id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        MusicPlayStatModel::filter_by_user_id(user_id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        MusicScrobbleModel::filter_by_user_id(user_id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        MusicPlayQueueModel::filter_by_user_id(user_id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        MusicBookmarkModel::filter_by_user_id(user_id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    pub(super) async fn delete_by_user_and_track_impl(
        &self,
        user_id: UserId,
        track_id: TrackId,
    ) -> DomainResult<()> {
        let mut db = self.db.clone();
        for mut pl in MusicPlaylistModel::filter_by_owner_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?
        {
            if pl.track_ids.0.contains(&track_id) {
                let filtered: Vec<TrackId> = pl
                    .track_ids
                    .0
                    .iter()
                    .copied()
                    .filter(|&id| id != track_id)
                    .collect();
                pl.update()
                    .track_ids(toasty::Json(filtered))
                    .updated_at(Timestamp::now())
                    .exec(&mut db)
                    .await
                    .map_err(db_err)?;
            }
        }
        if let Some(mut q) = MusicPlayQueueModel::filter_by_user_id(user_id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?
        {
            let has_in_list = q.track_ids.0.contains(&track_id);
            let is_current = q.current_track_id == Some(track_id.0);
            if has_in_list || is_current {
                let filtered: Vec<TrackId> = q
                    .track_ids
                    .0
                    .iter()
                    .copied()
                    .filter(|&id| id != track_id)
                    .collect();
                let next_current = if is_current {
                    filtered.first().map(|id| id.0)
                } else {
                    q.current_track_id
                };
                let next_position = if is_current { 0 } else { q.position_ms };
                q.update()
                    .track_ids(toasty::Json(filtered))
                    .current_track_id(next_current)
                    .position_ms(next_position)
                    .updated_at(Timestamp::now())
                    .exec(&mut db)
                    .await
                    .map_err(db_err)?;
            }
        }
        let bm_key = format!("{}:{}", user_id.0, track_id.0);
        MusicBookmarkModel::filter_by_bookmark_key(&bm_key)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        let stat_key = format!("{}:{}", user_id.0, track_id.0);
        MusicPlayStatModel::filter_by_stat_key(&stat_key)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        for sc in MusicScrobbleModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?
        {
            if sc.track_id == track_id.0 {
                sc.delete().exec(&mut db).await.map_err(db_err)?;
            }
        }
        let star_key = format!(
            "{}:{}:{}",
            user_id.0,
            StarTargetKind::Track.as_str(),
            track_id.0
        );
        MusicStarredModel::filter_by_star_key(&star_key)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        let rating_key = format!(
            "{}:{}:{}",
            user_id.0,
            StarTargetKind::Track.as_str(),
            track_id.0
        );
        MusicRatingModel::filter_by_rating_key(&rating_key)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(())
    }
}

use std::collections::HashSet;

use async_trait::async_trait;
use jiff::Timestamp;
use toasty::Db;
use uuid::Uuid;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::domain::user::UserId;

use super::db_err;
use crate::domain::catalog::{AlbumId, ArtistId, TrackId};
use crate::domain::collection::{
    AudioBookmark, PlayQueue, Playlist, PlaylistId, RatingEntry, ScrobbleEntry, StarTargetKind,
    StarredEntry, TrackPlayStat,
};
use crate::domain::repository::MusicCollectionRepository;
use crate::infra::persistence::models::{
    MusicBookmarkModel, MusicPlayQueueModel, MusicPlayStatModel, MusicPlaylistModel,
    MusicRatingModel, MusicScrobbleModel, MusicStarredModel,
};

#[derive(Clone)]
pub struct ToastyMusicCollectionRepository {
    db: Db,
}

impl ToastyMusicCollectionRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    fn playlist_to_domain(m: MusicPlaylistModel) -> Playlist {
        Playlist {
            id: PlaylistId(m.id),
            owner_id: UserId(m.owner_id),
            owner_username: m.owner_username,
            name: m.name,
            comment: m.comment,
            is_public: m.is_public,
            track_ids: m.track_ids.0,
            created_at: m.created_at,
            updated_at: m.updated_at,
        }
    }
}

#[async_trait]
impl MusicCollectionRepository for ToastyMusicCollectionRepository {
    async fn find_playlist_by_id(&self, id: PlaylistId) -> DomainResult<Option<Playlist>> {
        let mut db = self.db.clone();
        let row = MusicPlaylistModel::filter_by_id(id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(row.map(Self::playlist_to_domain))
    }

    async fn list_visible_playlists(&self, user_id: UserId) -> DomainResult<Vec<Playlist>> {
        let mut db = self.db.clone();
        let rows = MusicPlaylistModel::all()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        let mut out: Vec<Playlist> = rows
            .into_iter()
            .filter(|m| m.owner_id == user_id.0 || m.is_public)
            .map(Self::playlist_to_domain)
            .collect();
        out.sort_by_key(|p| std::cmp::Reverse(p.updated_at));
        Ok(out)
    }

    async fn save_playlist(&self, playlist: &Playlist) -> DomainResult<()> {
        let mut db = self.db.clone();
        let existing = MusicPlaylistModel::filter_by_id(playlist.id.0)
            .first()
            .exec(&mut db)
            .await
            .map_err(db_err)?;

        if let Some(mut rec) = existing {
            rec.update()
                .owner_username(playlist.owner_username.clone())
                .name(playlist.name.clone())
                .comment(playlist.comment.clone())
                .is_public(playlist.is_public)
                .track_ids(toasty::Json(playlist.track_ids.clone()))
                .updated_at(playlist.updated_at)
                .exec(&mut db)
                .await
                .map_err(db_err)?;
        } else {
            toasty::create!(MusicPlaylistModel {
                id: playlist.id.0,
                owner_id: playlist.owner_id.0,
                owner_username: playlist.owner_username.clone(),
                name: playlist.name.clone(),
                comment: playlist.comment.clone(),
                is_public: playlist.is_public,
                track_ids: toasty::Json(playlist.track_ids.clone()),
                created_at: playlist.created_at,
                updated_at: playlist.updated_at,
            })
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        }
        Ok(())
    }

    async fn delete_playlist(&self, id: PlaylistId) -> DomainResult<()> {
        let mut db = self.db.clone();
        MusicPlaylistModel::filter_by_id(id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    async fn delete_tracks_references(
        &self,
        track_ids: &[TrackId],
        deleted_album_ids: &[AlbumId],
        deleted_artist_ids: &[ArtistId],
    ) -> DomainResult<()> {
        if track_ids.is_empty() && deleted_album_ids.is_empty() && deleted_artist_ids.is_empty() {
            return Ok(());
        }

        let mut db = self.db.clone();
        let track_id_set: HashSet<TrackId> = track_ids.iter().copied().collect();

        if !track_id_set.is_empty() {
            for mut pl in MusicPlaylistModel::all()
                .exec(&mut db)
                .await
                .map_err(db_err)?
            {
                if pl.track_ids.0.iter().any(|id| track_id_set.contains(id)) {
                    let filtered: Vec<TrackId> = pl
                        .track_ids
                        .0
                        .iter()
                        .copied()
                        .filter(|id| !track_id_set.contains(id))
                        .collect();
                    pl.update()
                        .track_ids(toasty::Json(filtered))
                        .updated_at(Timestamp::now())
                        .exec(&mut db)
                        .await
                        .map_err(db_err)?;
                }
            }

            for mut q in MusicPlayQueueModel::all()
                .exec(&mut db)
                .await
                .map_err(db_err)?
            {
                let has_in_list = q.track_ids.0.iter().any(|id| track_id_set.contains(id));
                let is_current = q
                    .current_track_id
                    .is_some_and(|cid| track_id_set.contains(&TrackId(cid)));
                if has_in_list || is_current {
                    let filtered: Vec<TrackId> = q
                        .track_ids
                        .0
                        .iter()
                        .copied()
                        .filter(|id| !track_id_set.contains(id))
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

            for chunk in track_ids.chunks(500) {
                let raw_ids: Vec<Uuid> = chunk.iter().map(|id| id.0).collect();
                for row in MusicBookmarkModel::filter(
                    MusicBookmarkModel::fields()
                        .track_id()
                        .in_list(raw_ids.clone()),
                )
                .exec(&mut db)
                .await
                .map_err(db_err)?
                {
                    row.delete().exec(&mut db).await.map_err(db_err)?;
                }
                for row in MusicPlayStatModel::filter(
                    MusicPlayStatModel::fields()
                        .track_id()
                        .in_list(raw_ids.clone()),
                )
                .exec(&mut db)
                .await
                .map_err(db_err)?
                {
                    row.delete().exec(&mut db).await.map_err(db_err)?;
                }
                for row in MusicScrobbleModel::filter(
                    MusicScrobbleModel::fields().track_id().in_list(raw_ids),
                )
                .exec(&mut db)
                .await
                .map_err(db_err)?
                {
                    row.delete().exec(&mut db).await.map_err(db_err)?;
                }
            }
        }

        if !deleted_album_ids.is_empty() {
            for chunk in deleted_album_ids.chunks(500) {
                let raw_album_ids: Vec<Uuid> = chunk.iter().map(|id| id.0).collect();
                for row in MusicPlayStatModel::filter(
                    MusicPlayStatModel::fields()
                        .album_id()
                        .in_list(raw_album_ids),
                )
                .exec(&mut db)
                .await
                .map_err(db_err)?
                {
                    row.delete().exec(&mut db).await.map_err(db_err)?;
                }
            }
        }

        let mut target_keys: HashSet<(&'static str, Uuid)> = HashSet::new();
        let mut unique_uuids: Vec<Uuid> = Vec::new();
        let mut seen_uuids: HashSet<Uuid> = HashSet::new();
        for &tid in track_ids {
            target_keys.insert((StarTargetKind::Track.as_str(), tid.0));
            if seen_uuids.insert(tid.0) {
                unique_uuids.push(tid.0);
            }
        }
        for &aid in deleted_album_ids {
            target_keys.insert((StarTargetKind::Album.as_str(), aid.0));
            if seen_uuids.insert(aid.0) {
                unique_uuids.push(aid.0);
            }
        }
        for &arid in deleted_artist_ids {
            target_keys.insert((StarTargetKind::Artist.as_str(), arid.0));
            if seen_uuids.insert(arid.0) {
                unique_uuids.push(arid.0);
            }
        }

        for chunk in unique_uuids.chunks(500) {
            let chunk_vec = chunk.to_vec();
            for row in MusicStarredModel::filter(
                MusicStarredModel::fields()
                    .target_id()
                    .in_list(chunk_vec.clone()),
            )
            .exec(&mut db)
            .await
            .map_err(db_err)?
            {
                if target_keys.contains(&(row.kind.as_str(), row.target_id)) {
                    row.delete().exec(&mut db).await.map_err(db_err)?;
                }
            }
            for row in
                MusicRatingModel::filter(MusicRatingModel::fields().target_id().in_list(chunk_vec))
                    .exec(&mut db)
                    .await
                    .map_err(db_err)?
            {
                if target_keys.contains(&(row.kind.as_str(), row.target_id)) {
                    row.delete().exec(&mut db).await.map_err(db_err)?;
                }
            }
        }
        Ok(())
    }

    async fn set_starred(
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

    async fn unset_starred(
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

    async fn list_starred(
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

    async fn set_rating(
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

    async fn remove_rating(
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

    async fn list_user_ratings(&self, user_id: UserId) -> DomainResult<Vec<RatingEntry>> {
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

    async fn record_play(
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

    async fn list_user_play_stats(&self, user_id: UserId) -> DomainResult<Vec<TrackPlayStat>> {
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

    async fn list_recent_scrobbles(
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

    async fn get_play_queue(&self, user_id: UserId) -> DomainResult<Option<PlayQueue>> {
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

    async fn save_play_queue(&self, queue: &PlayQueue) -> DomainResult<()> {
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

    async fn list_bookmarks(&self, user_id: UserId) -> DomainResult<Vec<AudioBookmark>> {
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

    async fn get_bookmark(
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

    async fn save_bookmark(&self, bookmark: &AudioBookmark) -> DomainResult<()> {
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

    async fn delete_bookmark(&self, user_id: UserId, track_id: TrackId) -> DomainResult<()> {
        let mut db = self.db.clone();
        let key = format!("{}:{}", user_id.0, track_id.0);
        MusicBookmarkModel::filter_by_bookmark_key(&key)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    async fn delete_by_user(&self, user_id: UserId) -> DomainResult<()> {
        let mut db = self.db.clone();
        for row in MusicPlaylistModel::filter_by_owner_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?
        {
            row.delete().exec(&mut db).await.map_err(db_err)?;
        }
        for row in MusicStarredModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?
        {
            row.delete().exec(&mut db).await.map_err(db_err)?;
        }
        for row in MusicRatingModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?
        {
            row.delete().exec(&mut db).await.map_err(db_err)?;
        }
        for row in MusicPlayStatModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?
        {
            row.delete().exec(&mut db).await.map_err(db_err)?;
        }
        for row in MusicScrobbleModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?
        {
            row.delete().exec(&mut db).await.map_err(db_err)?;
        }
        MusicPlayQueueModel::filter_by_user_id(user_id.0)
            .delete()
            .exec(&mut db)
            .await
            .map_err(db_err)?;
        for row in MusicBookmarkModel::filter_by_user_id(user_id.0)
            .exec(&mut db)
            .await
            .map_err(db_err)?
        {
            row.delete().exec(&mut db).await.map_err(db_err)?;
        }
        Ok(())
    }
}

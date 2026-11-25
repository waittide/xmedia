mod engagement;

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
    pub(super) db: Db,
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
        self.set_starred_impl(user_id, kind, target_id, starred_at)
            .await
    }

    async fn unset_starred(
        &self,
        user_id: UserId,
        kind: StarTargetKind,
        target_id: Uuid,
    ) -> DomainResult<()> {
        self.unset_starred_impl(user_id, kind, target_id).await
    }

    async fn list_starred(
        &self,
        user_id: UserId,
        kind: Option<StarTargetKind>,
    ) -> DomainResult<Vec<StarredEntry>> {
        self.list_starred_impl(user_id, kind).await
    }

    async fn set_rating(
        &self,
        user_id: UserId,
        kind: StarTargetKind,
        target_id: Uuid,
        rating: u8,
    ) -> DomainResult<()> {
        self.set_rating_impl(user_id, kind, target_id, rating).await
    }

    async fn remove_rating(
        &self,
        user_id: UserId,
        kind: StarTargetKind,
        target_id: Uuid,
    ) -> DomainResult<()> {
        self.remove_rating_impl(user_id, kind, target_id).await
    }

    async fn list_user_ratings(&self, user_id: UserId) -> DomainResult<Vec<RatingEntry>> {
        self.list_user_ratings_impl(user_id).await
    }

    async fn record_play(
        &self,
        user_id: UserId,
        track_id: TrackId,
        album_id: AlbumId,
        played_at: Timestamp,
    ) -> DomainResult<TrackPlayStat> {
        self.record_play_impl(user_id, track_id, album_id, played_at)
            .await
    }

    async fn list_user_play_stats(&self, user_id: UserId) -> DomainResult<Vec<TrackPlayStat>> {
        self.list_user_play_stats_impl(user_id).await
    }

    async fn list_recent_scrobbles(
        &self,
        user_id: UserId,
        limit: u32,
    ) -> DomainResult<Vec<ScrobbleEntry>> {
        self.list_recent_scrobbles_impl(user_id, limit).await
    }

    async fn get_play_queue(&self, user_id: UserId) -> DomainResult<Option<PlayQueue>> {
        self.get_play_queue_impl(user_id).await
    }

    async fn save_play_queue(&self, queue: &PlayQueue) -> DomainResult<()> {
        self.save_play_queue_impl(queue).await
    }

    async fn list_bookmarks(&self, user_id: UserId) -> DomainResult<Vec<AudioBookmark>> {
        self.list_bookmarks_impl(user_id).await
    }

    async fn get_bookmark(
        &self,
        user_id: UserId,
        track_id: TrackId,
    ) -> DomainResult<Option<AudioBookmark>> {
        self.get_bookmark_impl(user_id, track_id).await
    }

    async fn save_bookmark(&self, bookmark: &AudioBookmark) -> DomainResult<()> {
        self.save_bookmark_impl(bookmark).await
    }

    async fn delete_bookmark(&self, user_id: UserId, track_id: TrackId) -> DomainResult<()> {
        self.delete_bookmark_impl(user_id, track_id).await
    }

    async fn delete_by_user(&self, user_id: UserId) -> DomainResult<()> {
        self.delete_by_user_impl(user_id).await
    }

    async fn delete_by_user_and_track(
        &self,
        user_id: UserId,
        track_id: TrackId,
    ) -> DomainResult<()> {
        self.delete_by_user_and_track_impl(user_id, track_id).await
    }
}

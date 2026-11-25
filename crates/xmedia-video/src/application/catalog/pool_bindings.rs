use uuid::Uuid;
use xmedia_core::application::bindings::{
    BindMediaItemsInput, BindMediaItemsResultDto, MediaBindingService, UnbindMediaItemResultDto,
};
use xmedia_core::application::ports::SystemEvent;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::{LibraryId, MediaKind};
use xmedia_core::domain::user::{User, require_admin};

use crate::application::dto::{PoolShowSummaryDto, PoolVideoSummaryDto, VideoPoolDto};
use crate::domain::id::{ShowId, VideoId};
use crate::domain::video::{
    ShowQueryFilter, VideoItem, VideoKindFilter, VideoQueryFilter, VideoShow,
};

use super::{VideoCatalogService, video_binding_id};

impl VideoCatalogService {
    pub async fn list_pool(
        &self,
        user: &User,
        library_id: Option<LibraryId>,
        search: Option<String>,
    ) -> DomainResult<VideoPoolDto> {
        let bound_ids = self
            .bindings
            .list_bound_ids(user.id, MediaKind::Video)
            .await?;
        let raw_movies: Vec<VideoItem> = self
            .catalog
            .list_videos(&VideoQueryFilter {
                library_id,
                kind: VideoKindFilter::Standalone,
                search: search.clone(),
                sort_desc: Some(true),
                ..Default::default()
            })
            .await?
            .into_iter()
            .filter(|v| !v.source.is_online_tracked())
            .collect();
        let raw_shows: Vec<VideoShow> = self
            .catalog
            .list_shows(&ShowQueryFilter {
                library_id,
                search,
                ..Default::default()
            })
            .await?
            .into_iter()
            .filter(|s| !s.is_online_tracked())
            .collect();

        let movie_dtos = self.enrich_videos(user.id, raw_movies).await?;
        let show_dtos = self.enrich_shows(user.id, raw_shows).await?;

        let movies = movie_dtos
            .into_iter()
            .map(|video| {
                let bound = bound_ids.contains(&video.id.0);
                PoolVideoSummaryDto { video, bound }
            })
            .collect();
        let shows = show_dtos
            .into_iter()
            .map(|show| {
                let bound = bound_ids.contains(&show.id.0);
                PoolShowSummaryDto { show, bound }
            })
            .collect();

        Ok(VideoPoolDto { movies, shows })
    }

    pub async fn bind_items(
        &self,
        actor: &User,
        input: BindMediaItemsInput,
    ) -> DomainResult<BindMediaItemsResultDto> {
        let target_user_id = MediaBindingService::resolve_target_user_id(actor, input.user_id)?;
        let mut to_bind: Vec<Uuid> = Vec::new();

        if let Some(lib_uuid) = input.library_id {
            let lid = LibraryId(lib_uuid);
            for v in self.catalog.list_videos_by_library(lid).await? {
                if !v.is_missing && v.show_id.is_none() && !v.source.is_online_tracked() {
                    to_bind.push(v.id.0);
                }
            }
            for s in self
                .catalog
                .list_shows(&ShowQueryFilter {
                    library_id: Some(lid),
                    ..Default::default()
                })
                .await?
            {
                if !s.is_online_tracked() {
                    to_bind.push(s.id.0);
                }
            }
        }

        for raw_id in input.item_ids {
            if let Some(show) = self.catalog.find_show_by_id(ShowId(raw_id)).await? {
                if show.is_online_tracked()
                    && !actor.role.is_admin()
                    && !self
                        .bindings
                        .is_bound(actor.id, MediaKind::Video, show.id.0)
                        .await?
                {
                    continue;
                }
                to_bind.push(show.id.0);
            } else if let Some(video) = self.catalog.find_video_by_id(VideoId(raw_id)).await?
                && !video.is_missing
            {
                let bid = video_binding_id(&video);
                if video.source.is_online_tracked()
                    && !actor.role.is_admin()
                    && !self
                        .bindings
                        .is_bound(actor.id, MediaKind::Video, bid)
                        .await?
                {
                    continue;
                }
                to_bind.push(bid);
            }
        }

        let total_requested = to_bind.len();
        let bound_count = self
            .bindings
            .bind_batch(target_user_id, MediaKind::Video, &to_bind)
            .await?;
        Ok(BindMediaItemsResultDto {
            bound_count,
            total_requested,
        })
    }

    pub async fn unbind_item(
        &self,
        actor: &User,
        raw_id: Uuid,
        requested_user_id: Option<Uuid>,
    ) -> DomainResult<UnbindMediaItemResultDto> {
        let target_user_id = MediaBindingService::resolve_target_user_id(actor, requested_user_id)?;

        if let Some(show) = self.catalog.find_show_by_id(ShowId(raw_id)).await? {
            let (unbound, remaining_bindings, deleted_from_pool) = self
                .bindings
                .unbind_with_gc(target_user_id, MediaKind::Video, show.id.0, || async {
                    if let Some(fresh_show) = self.catalog.find_show_by_id(show.id).await?
                        && fresh_show.is_online_tracked()
                    {
                        self.delete_show_without_bindings(fresh_show.id).await?;
                        return Ok(true);
                    }
                    Ok(false)
                })
                .await?;
            if unbound && !deleted_from_pool {
                let eps = self.catalog.list_episodes_by_show(show.id).await?;
                let ep_ids: Vec<VideoId> = eps.into_iter().map(|e| e.id).collect();
                self.watch
                    .delete_by_user_and_targets(target_user_id, &ep_ids, Some(show.id))
                    .await?;
            }
            return Ok(UnbindMediaItemResultDto {
                unbound,
                remaining_bindings,
                deleted_from_pool,
            });
        }

        let video_opt = self.catalog.find_video_by_id(VideoId(raw_id)).await?;
        let canonical_id = video_opt.as_ref().map(video_binding_id).unwrap_or(raw_id);
        let (unbound, remaining_bindings, deleted_from_pool) = self
            .bindings
            .unbind_with_gc(target_user_id, MediaKind::Video, canonical_id, || async {
                if let Some(ref v) = video_opt {
                    if let Some(sid) = v.show_id {
                        if let Some(show) = self.catalog.find_show_by_id(sid).await?
                            && show.is_online_tracked()
                        {
                            self.delete_show_without_bindings(sid).await?;
                            return Ok(true);
                        }
                    } else if let Some(fresh_v) = self.catalog.find_video_by_id(v.id).await?
                        && fresh_v.source.is_online_tracked()
                    {
                        self.delete_video_without_bindings(fresh_v).await?;
                        return Ok(true);
                    }
                }
                Ok(false)
            })
            .await?;
        if unbound && !deleted_from_pool {
            if let Some(ref v) = video_opt
                && let Some(sid) = v.show_id
            {
                let eps = self.catalog.list_episodes_by_show(sid).await?;
                let ep_ids: Vec<VideoId> = eps.into_iter().map(|e| e.id).collect();
                self.watch
                    .delete_by_user_and_targets(target_user_id, &ep_ids, Some(sid))
                    .await?;
            } else {
                self.watch
                    .delete_by_user_and_targets(target_user_id, &[VideoId(canonical_id)], None)
                    .await?;
            }
        }
        Ok(UnbindMediaItemResultDto {
            unbound,
            remaining_bindings,
            deleted_from_pool,
        })
    }

    pub(crate) async fn delete_video_internal(&self, item: VideoItem) -> DomainResult<()> {
        let _ = self.delete_video_inner(item, true).await?;
        Ok(())
    }

    pub(crate) async fn delete_video_without_bindings(&self, item: VideoItem) -> DomainResult<()> {
        self.delete_video_inner(item, false).await?;
        Ok(())
    }

    async fn delete_video_inner(
        &self,
        item: VideoItem,
        delete_bindings: bool,
    ) -> DomainResult<Option<ShowId>> {
        let video_id = item.id;
        let old_show_id = item.show_id;
        for key in item.source.managed_file_keys() {
            let _ = self.storage.delete_managed_file(key).await;
        }
        self.watch.delete_progress_by_video(video_id).await?;
        self.catalog.delete_video(video_id).await?;
        self.catalog.recompute_show_aggregates().await?;
        let deleted_show_id = if let Some(sid) = old_show_id {
            if self.catalog.find_show_by_id(sid).await?.is_none() {
                if delete_bindings {
                    self.bindings
                        .delete_item_bindings(MediaKind::Video, sid.0)
                        .await?;
                }
                Some(sid)
            } else {
                None
            }
        } else {
            if delete_bindings {
                self.bindings
                    .delete_item_bindings(MediaKind::Video, video_id.0)
                    .await?;
            }
            None
        };
        self.watch
            .delete_video_references(video_id, deleted_show_id)
            .await?;
        self.events.publish(SystemEvent::MediaChanged {
            media_kind: MediaKind::Video,
        });
        Ok(deleted_show_id)
    }

    pub(crate) async fn delete_show_without_bindings(&self, show_id: ShowId) -> DomainResult<()> {
        let episodes = self.catalog.list_episodes_by_show(show_id).await?;
        let mut ep_ids = Vec::with_capacity(episodes.len());
        for ep in episodes {
            for key in ep.source.managed_file_keys() {
                let _ = self.storage.delete_managed_file(key).await;
            }
            ep_ids.push(ep.id);
            self.catalog.delete_video(ep.id).await?;
        }
        self.watch
            .delete_library_references(&ep_ids, &[show_id])
            .await?;
        self.catalog.delete_show(show_id).await?;
        self.catalog.recompute_show_aggregates().await?;
        self.events.publish(SystemEvent::MediaChanged {
            media_kind: MediaKind::Video,
        });
        Ok(())
    }

    pub async fn delete_video(&self, actor: &User, video_id: VideoId) -> DomainResult<()> {
        require_admin(actor)?;
        let item = self
            .catalog
            .find_video_by_id(video_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("video not found".into()))?;
        self.delete_video_internal(item).await
    }
}

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use jiff::Timestamp;
use uuid::Uuid;
use xmedia_core::application::bindings::MediaBindingService;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;
use xmedia_core::domain::user::{User, UserId};

use crate::application::dto::{
    ResolvedCatalogEntity, SeasonGroupDto, ShowDetailDto, ShowSummaryDto, VideoSummaryDto,
    season_uuid,
};
use crate::application::ports::VideoStoragePort;
use crate::application::projector::{
    VideoProjector, enrich_shows_with_maps, enrich_videos_with_maps,
};
use crate::domain::id::{ShowId, VideoId};
use crate::domain::repository::{
    ProgressScope, ShowScope, VideoCatalogMutation, VideoCatalogRepository, VideoScope,
    VideoWatchRepository,
};
use crate::domain::video::{ShowMetadataPatch, ShowQueryFilter};
use crate::domain::watch::{VideoFavoriteKind, WatchProgress};

#[derive(Clone)]
pub struct VideoShowService {
    catalog: Arc<dyn VideoCatalogRepository>,
    watch: Arc<dyn VideoWatchRepository>,
    bindings: Arc<MediaBindingService>,
    storage: Arc<dyn VideoStoragePort>,
    projector: VideoProjector,
}

impl VideoShowService {
    pub(super) fn new(
        catalog: Arc<dyn VideoCatalogRepository>,
        watch: Arc<dyn VideoWatchRepository>,
        bindings: Arc<MediaBindingService>,
        storage: Arc<dyn VideoStoragePort>,
        projector: VideoProjector,
    ) -> Self {
        Self {
            catalog,
            watch,
            bindings,
            storage,
            projector,
        }
    }

    pub async fn require_bound_show(&self, user_id: UserId, show_id: ShowId) -> DomainResult<()> {
        self.projector.require_show_bound(user_id, show_id).await
    }

    pub async fn resolve_item_kind(&self, id: Uuid) -> DomainResult<Option<ResolvedCatalogEntity>> {
        if let Some(video) = self
            .catalog
            .query_videos(VideoScope::Id(VideoId(id)))
            .await?
            .into_iter()
            .next()
            .filter(|v| !v.is_missing)
        {
            return Ok(Some(ResolvedCatalogEntity::Video(Box::new(video))));
        }
        if let Some(show) = self
            .catalog
            .query_shows(ShowScope::Id(ShowId(id)))
            .await?
            .into_iter()
            .next()
        {
            return Ok(Some(ResolvedCatalogEntity::Show(show)));
        }
        let all_shows = self
            .catalog
            .query_shows(ShowScope::Filter(&ShowQueryFilter::default()))
            .await?;
        for show in all_shows {
            let max_s = show.season_count.max(1) + 2;
            for season_number in 0..=max_s {
                if season_uuid(show.id, season_number) == id {
                    return Ok(Some(ResolvedCatalogEntity::Season {
                        show,
                        season_number,
                    }));
                }
            }
        }
        Ok(None)
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
            .query_shows(ShowScope::Filter(&filter))
            .await?
            .into_iter()
            .filter(|s| bound_ids.contains(&s.id.0))
            .collect();
        self.projector.enrich_shows(user.id, shows).await
    }

    pub async fn get_show_detail(
        &self,
        user: &User,
        show_id: ShowId,
    ) -> DomainResult<ShowDetailDto> {
        self.projector.require_show_bound(user.id, show_id).await?;
        let show = self
            .catalog
            .query_shows(ShowScope::Id(show_id))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound("show not found".into()))?;
        let episodes = self
            .catalog
            .query_videos(VideoScope::ShowEpisodes(show_id))
            .await?;
        let progress_list = self
            .watch
            .query_progress(ProgressScope::User(user.id))
            .await?;
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

        let show_dto = enrich_shows_with_maps(vec![show], &show_fav_set, &watched_by_show)
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound("show not found".into()))?;
        let ep_dtos = enrich_videos_with_maps(episodes, &prog_map, &video_fav_set);

        let continue_episode = select_continue_episode(&ep_dtos);
        let seasons = group_episodes_by_season(ep_dtos);

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
        actor.ensure_admin()?;
        let mut show = self
            .catalog
            .query_shows(ShowScope::Id(show_id))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound("show not found".into()))?;
        let old_show_key = show.show_key.clone();
        let title_changed = show.apply_patch(patch)?;
        if show.show_key != old_show_key
            && let Some(conflict) = self
                .catalog
                .query_shows(ShowScope::Key(&show.show_key))
                .await?
                .into_iter()
                .next()
            && conflict.id != show.id
        {
            return Err(DomainError::Conflict(format!(
                "a show named '{}' already exists in this library",
                show.title
            )));
        }
        self.catalog.save(VideoCatalogMutation::Show(&show)).await?;
        if title_changed {
            let now = Timestamp::now();
            let episodes = self
                .catalog
                .query_videos(VideoScope::ShowEpisodes(show_id))
                .await?;
            for mut ep in episodes {
                ep.show_title = Some(show.title.clone());
                ep.updated_at = now;
                self.catalog.save(VideoCatalogMutation::Video(&ep)).await?;
            }
        }
        self.projector
            .enrich_shows(actor.id, vec![show])
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
        actor.ensure_admin()?;
        if bytes.is_empty() {
            return Err(DomainError::Validation(
                "poster image cannot be empty".into(),
            ));
        }
        let mut show = self
            .catalog
            .query_shows(ShowScope::Id(show_id))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound("show not found".into()))?;
        let key = self.storage.save_poster_bytes(bytes).await?;
        show.poster_key = Some(key);
        show.updated_at = Timestamp::now();
        self.catalog.save(VideoCatalogMutation::Show(&show)).await?;
        self.projector
            .enrich_shows(actor.id, vec![show])
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound("show not found".into()))
    }
}

fn select_continue_episode(episodes: &[VideoSummaryDto]) -> Option<VideoSummaryDto> {
    episodes
        .iter()
        .filter(|e| {
            e.progress
                .as_ref()
                .is_some_and(|p| !p.completed && p.position_secs > 1.0)
        })
        .max_by_key(|e| e.progress.as_ref().map(|p| p.updated_at))
        .cloned()
        .or_else(|| {
            episodes
                .iter()
                .find(|e| e.progress.as_ref().is_none_or(|p| !p.completed))
                .cloned()
        })
        .or_else(|| episodes.first().cloned())
}

fn group_episodes_by_season(episodes: Vec<VideoSummaryDto>) -> Vec<SeasonGroupDto> {
    let mut season_buckets: BTreeMap<u32, Vec<VideoSummaryDto>> = BTreeMap::new();
    for ep in episodes {
        let s_num = ep.season_number.unwrap_or(1);
        season_buckets.entry(s_num).or_default().push(ep);
    }
    season_buckets
        .into_iter()
        .map(|(season_number, episodes)| SeasonGroupDto {
            season_number,
            episodes,
        })
        .collect()
}

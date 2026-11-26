mod browse;
mod debug;
mod dto;
mod proxy;
mod resolve;
mod tracked;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use uuid::Uuid;
use xmedia_core::application::bindings::MediaBindingService;
use xmedia_core::application::ports::RealtimeEventPort;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::domain::repository::LibraryRepository;
use xmedia_core::domain::user::User;
pub(super) use xmedia_source::concurrency::{
    AsyncLockMap, CancelOnDrop, CancelTasksOnDrop, weak_lock,
};
use xmedia_source::crud::SourceCrudService;
use xmedia_source::model::ConcurrentRateSpec;
use xmedia_source::rate_limit::{RateLimiterMap, acquire_source_rate_slot};

pub use crate::application::ports::AudioProxyResponse;
pub use dto::*;
pub use proxy::MAX_AUDIO_PROXY_BYTES;

use crate::application::ingest::MusicIngestService;
use crate::application::ports::{
    AudioMetadataProbePort, MusicMediaProxyPort, MusicSourceEnginePort, MusicStoragePort,
};
use crate::application::projector::MusicProjector;
use crate::application::tracks::MusicTrackService;
use crate::domain::catalog::TrackId;
use crate::domain::online::MusicSourceSpec;
use crate::domain::repository::MusicCatalogRepository;

pub(super) const SEARCH_MAX_CONCURRENCY: usize = 16;
pub(super) const COVER_RESOLVE_MAX_CONCURRENCY: usize = 6;
pub(super) const COVER_URL_CACHE_MAX_ENTRIES: usize = 2048;
pub(super) const SEARCH_PER_SOURCE_TIMEOUT: Duration = Duration::from_secs(20);
pub(super) const TRACK_INFO_OP_TIMEOUT: Duration = Duration::from_secs(45);
pub(super) const SOURCE_TEST_TOTAL_TIMEOUT: Duration = Duration::from_secs(90);

#[derive(Clone)]
pub(crate) struct MusicTrackingDeps {
    pub(crate) libraries: Arc<dyn LibraryRepository>,
    pub(crate) catalog_repo: Arc<dyn MusicCatalogRepository>,
    pub(crate) bindings: Arc<MediaBindingService>,
    pub(crate) projector: MusicProjector,
    pub(crate) tracks_service: Arc<MusicTrackService>,
    pub(crate) ingest_service: Arc<MusicIngestService>,
    pub(crate) storage: Arc<dyn MusicStoragePort>,
    pub(crate) probe: Arc<dyn AudioMetadataProbePort>,
    pub(crate) events: Arc<dyn RealtimeEventPort>,
}

pub(super) type CoverUrlCacheKey = (Uuid, u64, String);

pub struct MusicOnlineService {
    pub(super) sources: Arc<SourceCrudService<MusicSourceSpec>>,
    pub(super) engine: Arc<dyn MusicSourceEnginePort>,
    pub(super) media_proxy: Arc<dyn MusicMediaProxyPort>,
    pub(super) track_locks: AsyncLockMap<(Uuid, String)>,
    pub(super) track_cache_locks: AsyncLockMap<TrackId>,
    pub(super) rate_limiters: RateLimiterMap,
    pub(super) cover_resolve_sem: Arc<tokio::sync::Semaphore>,
    pub(super) cover_url_cache: Arc<Mutex<HashMap<CoverUrlCacheKey, String>>>,
    pub(super) tracking: Option<MusicTrackingDeps>,
    pub(super) op_timeout: Duration,
    pub(super) track_info_timeout: Duration,
}

impl MusicOnlineService {
    pub(crate) fn with_ports(
        sources: Arc<SourceCrudService<MusicSourceSpec>>,
        engine: Arc<dyn MusicSourceEnginePort>,
        media_proxy: Arc<dyn MusicMediaProxyPort>,
    ) -> Self {
        Self {
            sources,
            engine,
            media_proxy,
            track_locks: Arc::new(Mutex::new(HashMap::new())),
            track_cache_locks: Arc::new(Mutex::new(HashMap::new())),
            rate_limiters: Arc::new(Mutex::new(HashMap::new())),
            cover_resolve_sem: Arc::new(tokio::sync::Semaphore::new(COVER_RESOLVE_MAX_CONCURRENCY)),
            cover_url_cache: Arc::new(Mutex::new(HashMap::new())),
            tracking: None,
            op_timeout: SEARCH_PER_SOURCE_TIMEOUT,
            track_info_timeout: TRACK_INFO_OP_TIMEOUT,
        }
    }

    pub fn sources_crud(&self) -> &Arc<SourceCrudService<MusicSourceSpec>> {
        &self.sources
    }

    pub(crate) fn with_tracking(mut self, deps: MusicTrackingDeps) -> Self {
        self.tracking = Some(deps);
        self
    }

    #[cfg(test)]
    pub(crate) fn with_timeouts(
        mut self,
        op_timeout: Duration,
        track_info_timeout: Duration,
    ) -> Self {
        self.op_timeout = op_timeout;
        self.track_info_timeout = track_info_timeout;
        self
    }

    pub async fn run_login_action(
        &self,
        actor: &User,
        id: Uuid,
        login_info: HashMap<String, String>,
        action: Option<String>,
    ) -> DomainResult<LoginActionResponseDto> {
        let engine = self.engine.clone();
        self.sources
            .run_login_action_with(
                actor,
                id,
                login_info,
                action,
                move |spec, state, merged, act| {
                    engine.run_login_action_sync(spec, state, merged, act)
                },
            )
            .await
    }

    pub(super) async fn acquire_rate_slot(
        &self,
        source_id: Uuid,
        spec: Option<ConcurrentRateSpec>,
        timeout: Duration,
    ) -> Result<(), String> {
        acquire_source_rate_slot(&self.rate_limiters, source_id, spec, timeout).await
    }
}

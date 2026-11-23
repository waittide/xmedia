mod browse;
mod debug;
mod dto;
mod items;
mod proxy;
mod sources;
mod tracked;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use uuid::Uuid;
use xmedia_core::application::ports::RealtimeEventPort;
use xmedia_core::domain::repository::LibraryRepository;
pub(super) use xmedia_source::concurrency::{
    AsyncLockMap, CancelOnDrop, CancelTasksOnDrop, get_or_insert_weak_lock,
};
use xmedia_source::model::ConcurrentRateSpec;
use xmedia_source::rate_limit::{RateLimiterMap, acquire_source_rate_slot};

pub use dto::*;
pub use items::MusicOnlineSourceDirectory;
pub use proxy::AudioProxyResponse;

use crate::application::catalog::MusicCatalogService;
use crate::application::ingest::MusicIngestService;
use crate::application::ports::{AudioMetadataProbePort, MusicStoragePort};
use crate::domain::catalog::TrackId;
use crate::domain::repository::{MusicCatalogRepository, MusicSourceRepository};
use crate::source::MusicSourceEngine;

pub(super) const SEARCH_MAX_CONCURRENCY: usize = 16;
pub(super) const COVER_RESOLVE_MAX_CONCURRENCY: usize = 6;
pub(super) const COVER_URL_CACHE_MAX_ENTRIES: usize = 2048;
pub(super) const SEARCH_PER_SOURCE_TIMEOUT: Duration = Duration::from_secs(20);
pub(super) const TRACK_INFO_OP_TIMEOUT: Duration = Duration::from_secs(45);
pub(super) const SOURCE_TEST_TOTAL_TIMEOUT: Duration = Duration::from_secs(90);

#[derive(Clone)]
pub struct MusicTrackingDeps {
    pub libraries: Arc<dyn LibraryRepository>,
    pub catalog_repo: Arc<dyn MusicCatalogRepository>,
    pub catalog_service: Arc<MusicCatalogService>,
    pub ingest_service: Arc<MusicIngestService>,
    pub storage: Arc<dyn MusicStoragePort>,
    pub probe: Arc<dyn AudioMetadataProbePort>,
    pub events: Arc<dyn RealtimeEventPort>,
}

pub struct MusicOnlineService {
    pub(super) sources: Arc<dyn MusicSourceRepository>,
    pub(super) engine: MusicSourceEngine,
    pub(super) source_state_locks: AsyncLockMap<Uuid>,
    pub(super) track_cache_locks: AsyncLockMap<TrackId>,
    pub(super) rate_limiters: RateLimiterMap,
    pub(super) cover_resolve_sem: Arc<tokio::sync::Semaphore>,
    pub(super) cover_url_cache: Arc<Mutex<HashMap<(Uuid, String), String>>>,
    pub(super) seed_import_lock: tokio::sync::Mutex<()>,
    pub(super) tracking: Option<MusicTrackingDeps>,
    pub(super) allow_private_ips: bool,
    pub(super) op_timeout: Duration,
    pub(super) track_info_timeout: Duration,
}

impl MusicOnlineService {
    pub fn new(sources: Arc<dyn MusicSourceRepository>) -> Self {
        Self::with_engine(sources, MusicSourceEngine::default(), false)
    }

    pub fn with_engine(
        sources: Arc<dyn MusicSourceRepository>,
        engine: MusicSourceEngine,
        allow_private_ips: bool,
    ) -> Self {
        Self {
            sources,
            engine,
            source_state_locks: Arc::new(Mutex::new(HashMap::new())),
            track_cache_locks: Arc::new(Mutex::new(HashMap::new())),
            rate_limiters: Arc::new(Mutex::new(HashMap::new())),
            cover_resolve_sem: Arc::new(tokio::sync::Semaphore::new(COVER_RESOLVE_MAX_CONCURRENCY)),
            cover_url_cache: Arc::new(Mutex::new(HashMap::new())),
            seed_import_lock: tokio::sync::Mutex::new(()),
            tracking: None,
            allow_private_ips,
            op_timeout: SEARCH_PER_SOURCE_TIMEOUT,
            track_info_timeout: TRACK_INFO_OP_TIMEOUT,
        }
    }

    pub fn with_tracking(mut self, deps: MusicTrackingDeps) -> Self {
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

    pub(super) async fn acquire_rate_slot(
        &self,
        source_id: Uuid,
        spec: Option<ConcurrentRateSpec>,
        timeout: Duration,
    ) -> Result<(), String> {
        acquire_source_rate_slot(&self.rate_limiters, source_id, spec, timeout).await
    }

    pub(super) fn invalidate_cover_cache_for_source(&self, source_id: Uuid) {
        let mut guard = self
            .cover_url_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        guard.retain(|(sid, _), _| *sid != source_id);
    }
}

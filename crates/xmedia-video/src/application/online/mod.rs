mod browse;
mod cache;
mod debug;
mod dto;
mod hls_stage;
mod playback;
mod proxy;
mod tracked;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use uuid::Uuid;
use xmedia_core::application::bindings::MediaBindingService;
use xmedia_core::application::ports::RealtimeEventPort;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::repository::LibraryRepository;
use xmedia_core::domain::user::User;
pub(super) use xmedia_source::concurrency::{
    AsyncLockMap, CancelOnDrop, CancelTasksOnDrop, weak_lock,
};
use xmedia_source::crud::{SourceCrudService, SourceRecord};
use xmedia_source::model::{ConcurrentRateSpec, SourceCallError};
use xmedia_source::rate_limit::{RateLimiterMap, acquire_source_rate_slot};

use crate::application::ingest::VideoIngestService;
use crate::application::ports::{
    VideoMediaEnginePort, VideoSourceEnginePort, VideoStoragePort, VideoStreamTransportPort,
};
use crate::application::projector::VideoProjector;
use crate::application::shows::VideoShowService;
use crate::domain::id::VideoId;
use crate::domain::online::{ExecutionDiagnostics, SourceRuntimeState, VideoSourceSpec};
use crate::domain::repository::VideoCatalogRepository;

pub use dto::*;

const SEARCH_MAX_CONCURRENCY: usize = 16;
const SEARCH_PER_SOURCE_TIMEOUT: Duration = Duration::from_secs(20);
const DETAIL_OP_TIMEOUT: Duration = Duration::from_secs(25);
const SOURCE_TEST_TOTAL_TIMEOUT: Duration = Duration::from_secs(90);

#[derive(Clone)]
pub(in crate::application) struct VideoTrackingDeps {
    pub(super) libraries: Arc<dyn LibraryRepository>,
    pub(super) catalog_repo: Arc<dyn VideoCatalogRepository>,
    pub(super) bindings: Arc<MediaBindingService>,
    pub(super) shows: Arc<VideoShowService>,
    pub(super) ingest: Arc<VideoIngestService>,
    pub(super) projector: VideoProjector,
    pub(super) storage: Arc<dyn VideoStoragePort>,
    pub(super) media_engine: Arc<dyn VideoMediaEnginePort>,
    pub(super) events: Arc<dyn RealtimeEventPort>,
}

pub struct VideoOnlineService {
    source_crud: Arc<SourceCrudService<VideoSourceSpec>>,
    engine: Arc<dyn VideoSourceEnginePort>,
    transport: Arc<dyn VideoStreamTransportPort>,
    track_locks: AsyncLockMap<(Uuid, String)>,
    video_cache_locks: AsyncLockMap<VideoId>,
    rate_limiters: RateLimiterMap,
    tracking: Option<VideoTrackingDeps>,
    op_timeout: Duration,
    detail_timeout: Duration,
}

impl VideoOnlineService {
    pub fn new(
        source_crud: Arc<SourceCrudService<VideoSourceSpec>>,
        engine: Arc<dyn VideoSourceEnginePort>,
        transport: Arc<dyn VideoStreamTransportPort>,
    ) -> Self {
        Self {
            source_crud,
            engine,
            transport,
            track_locks: Arc::new(Mutex::new(HashMap::new())),
            video_cache_locks: Arc::new(Mutex::new(HashMap::new())),
            rate_limiters: Arc::new(Mutex::new(HashMap::new())),
            tracking: None,
            op_timeout: SEARCH_PER_SOURCE_TIMEOUT,
            detail_timeout: DETAIL_OP_TIMEOUT,
        }
    }

    pub fn source_crud(&self) -> &Arc<SourceCrudService<VideoSourceSpec>> {
        &self.source_crud
    }

    pub(in crate::application) fn with_tracking(mut self, deps: VideoTrackingDeps) -> Self {
        self.tracking = Some(deps);
        self
    }

    #[cfg(test)]
    pub(crate) fn with_timeouts(mut self, op_timeout: Duration, detail_timeout: Duration) -> Self {
        self.op_timeout = op_timeout;
        self.detail_timeout = detail_timeout;
        self
    }

    pub(super) async fn load_source(&self, id: Uuid) -> DomainResult<SourceRecord> {
        self.source_crud.load_source(id).await
    }

    pub(super) async fn load_source_for_actor(
        &self,
        actor: &User,
        id: Uuid,
    ) -> DomainResult<SourceRecord> {
        self.source_crud.load_source_for_actor(actor, id).await
    }

    pub(super) async fn resolve_source_media_headers(
        &self,
        actor: &User,
        source_id: Uuid,
        target_url: &str,
    ) -> DomainResult<HashMap<String, String>> {
        let rec = self.load_source_for_actor(actor, source_id).await?;
        let spec: VideoSourceSpec = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("影视源 JSON 无效: {e}")))?;
        Ok(spec.build_media_headers(target_url, None))
    }

    pub(super) async fn resolve_source_media_headers_with_cookies(
        &self,
        actor: &User,
        source_id: Uuid,
        target_url: &str,
    ) -> DomainResult<HashMap<String, String>> {
        let rec = self.load_source_for_actor(actor, source_id).await?;
        let spec: VideoSourceSpec = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("影视源 JSON 无效: {e}")))?;
        Ok(spec.build_media_headers(target_url, Some(&rec.runtime_state.cookies)))
    }

    pub(super) async fn resolve_internal_media_headers(
        &self,
        source_id: Option<Uuid>,
        target_url: &str,
    ) -> HashMap<String, String> {
        let Some(sid) = source_id else {
            return HashMap::new();
        };
        let Ok(rec) = self.load_source(sid).await else {
            return HashMap::new();
        };
        let Ok(spec) = serde_json::from_str::<VideoSourceSpec>(&rec.raw_json) else {
            return HashMap::new();
        };
        spec.build_media_headers(target_url, Some(&rec.runtime_state.cookies))
    }

    pub(super) async fn persist_runtime_state_with_base(
        &self,
        id: Uuid,
        base_state: &SourceRuntimeState,
        new_state: SourceRuntimeState,
        allow_login_info_override: bool,
    ) -> DomainResult<()> {
        self.source_crud
            .persist_runtime_state_with_base(id, base_state, new_state, allow_login_info_override)
            .await
    }

    pub async fn import_sources(
        &self,
        actor: &User,
        raw_json: &str,
    ) -> DomainResult<ImportVideoSourcesReportDto> {
        self.source_crud.import_sources(actor, raw_json).await
    }

    pub async fn run_login_action(
        &self,
        actor: &User,
        id: Uuid,
        login_info: HashMap<String, String>,
        action: Option<String>,
    ) -> DomainResult<LoginActionResponseDto> {
        let engine = self.engine.clone();
        self.source_crud
            .run_login_action_with(
                actor,
                id,
                login_info,
                action,
                move |spec, prev_state, merged_login, act| {
                    engine
                        .run_login_action_sync(spec, prev_state, merged_login, act)
                        .map_err(|msg| SourceCallError::new(msg, ExecutionDiagnostics::default()))
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

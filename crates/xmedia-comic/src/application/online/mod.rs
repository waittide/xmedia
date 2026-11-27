mod debug;
mod dto;
mod helpers;
mod reader;
mod tracked;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use uuid::Uuid;
use xmedia_core::application::ports::RealtimeEventPort;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::domain::repository::LibraryRepository;
use xmedia_core::domain::user::User;
use xmedia_source::crud::SourceCrudService;
use xmedia_source::model::ConcurrentRateSpec;

use crate::application::catalog::ComicCatalogService;
use crate::application::ports::{
    ComicFileStoragePort, ComicSourceEnginePort, ThumbnailGeneratorPort,
};
use crate::domain::id::ChapterId;
use crate::domain::online::ComicSourceSpec;
use crate::domain::repository::{ChapterRepository, ComicReadingRepository, SeriesRepository};

pub use dto::*;
use helpers::{AsyncLockMap, RateLimiterMap, get_or_insert_rate_limiter};
pub use tracked::{ComicCacheReportDto, TrackOnlineComicInput};

pub type ComicOnlineSourceDirectory = SourceCrudService<ComicSourceSpec>;

const SEARCH_MAX_CONCURRENCY: usize = 16;
const SEARCH_PER_SOURCE_TIMEOUT: Duration = Duration::from_secs(20);
const READER_OP_TIMEOUT: Duration = Duration::from_secs(45);
const SOURCE_TEST_TOTAL_TIMEOUT: Duration = Duration::from_secs(90);
const COMIC_IMAGE_PROXY_MAX_BYTES: usize = 16 * 1024 * 1024;

pub struct ComicOnlineService {
    pub(super) source_crud: Arc<SourceCrudService<ComicSourceSpec>>,
    pub(super) series: Arc<dyn SeriesRepository>,
    pub(super) chapters: Arc<dyn ChapterRepository>,
    pub(super) reading: Arc<dyn ComicReadingRepository>,
    pub(super) catalog: Arc<ComicCatalogService>,
    pub(super) thumbnails: Arc<dyn ThumbnailGeneratorPort>,
    pub(super) storage: Arc<dyn ComicFileStoragePort>,
    pub(super) events: Arc<dyn RealtimeEventPort>,
    pub(super) engine: Arc<dyn ComicSourceEnginePort>,
    pub(super) track_locks: AsyncLockMap<(Uuid, String)>,
    pub(super) chapter_cache_locks: AsyncLockMap<ChapterId>,
    pub(super) rate_limiters: RateLimiterMap,
    pub(super) allow_private_ips: bool,
}

impl ComicOnlineService {
    #[allow(clippy::too_many_arguments)]
    pub fn with_engine(
        source_crud: Arc<SourceCrudService<ComicSourceSpec>>,
        _libraries: Arc<dyn LibraryRepository>,
        series: Arc<dyn SeriesRepository>,
        chapters: Arc<dyn ChapterRepository>,
        reading: Arc<dyn ComicReadingRepository>,
        catalog: Arc<ComicCatalogService>,
        thumbnails: Arc<dyn ThumbnailGeneratorPort>,
        storage: Arc<dyn ComicFileStoragePort>,
        events: Arc<dyn RealtimeEventPort>,
        engine: Arc<dyn ComicSourceEnginePort>,
        allow_private_ips: bool,
    ) -> Self {
        Self {
            source_crud,
            series,
            chapters,
            reading,
            catalog,
            thumbnails,
            storage,
            events,
            engine,
            track_locks: Arc::new(Mutex::new(HashMap::new())),
            chapter_cache_locks: Arc::new(Mutex::new(HashMap::new())),
            rate_limiters: Arc::new(Mutex::new(HashMap::new())),
            allow_private_ips,
        }
    }

    pub fn source_crud(&self) -> &Arc<SourceCrudService<ComicSourceSpec>> {
        &self.source_crud
    }

    pub fn http_config(&self) -> xmedia_source::http::HttpClientConfig {
        self.engine.http_config()
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
                move |spec, state, merged_login, act| {
                    engine.run_login_action_sync(spec, state, merged_login, act)
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
        let Some(rate) = spec else {
            return Ok(());
        };
        let limiter = get_or_insert_rate_limiter(&self.rate_limiters, source_id);
        limiter.acquire(rate, timeout).await
    }
}

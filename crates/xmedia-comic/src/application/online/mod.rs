mod debug;
mod dto;
mod helpers;
mod items;
mod reader;
mod sources;
mod tracked;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use uuid::Uuid;
use xmedia_core::application::ports::RealtimeEventPort;
use xmedia_core::domain::repository::LibraryRepository;

use crate::application::catalog::ComicCatalogService;
use crate::application::ports::{ComicFileStoragePort, ThumbnailGeneratorPort};
use crate::domain::id::ChapterId;
use crate::domain::repository::{
    ChapterRepository, ComicReadingRepository, ComicSourceRepository, SeriesRepository,
};
use crate::source::ComicSourceEngine;

pub use dto::*;
use helpers::{AsyncLockMap, RateLimiterMap, get_or_insert_rate_limiter};
pub use items::ComicOnlineSourceDirectory;
pub use tracked::{ComicCacheReportDto, TrackOnlineComicInput};
use xmedia_source::model::ConcurrentRateSpec;

const SEARCH_MAX_CONCURRENCY: usize = 16;
const SEARCH_PER_SOURCE_TIMEOUT: Duration = Duration::from_secs(20);
const READER_OP_TIMEOUT: Duration = Duration::from_secs(45);
const SOURCE_TEST_TOTAL_TIMEOUT: Duration = Duration::from_secs(90);
const COMIC_IMAGE_PROXY_MAX_BYTES: usize = 16 * 1024 * 1024;

pub struct ComicOnlineService {
    sources: Arc<dyn ComicSourceRepository>,
    libraries: Arc<dyn LibraryRepository>,
    series: Arc<dyn SeriesRepository>,
    chapters: Arc<dyn ChapterRepository>,
    reading: Arc<dyn ComicReadingRepository>,
    catalog: Arc<ComicCatalogService>,
    thumbnails: Arc<dyn ThumbnailGeneratorPort>,
    storage: Arc<dyn ComicFileStoragePort>,
    events: Arc<dyn RealtimeEventPort>,
    engine: ComicSourceEngine,
    source_state_locks: AsyncLockMap<Uuid>,
    track_locks: AsyncLockMap<(Uuid, String)>,
    chapter_cache_locks: AsyncLockMap<ChapterId>,
    rate_limiters: RateLimiterMap,
    seed_import_lock: tokio::sync::Mutex<()>,
    allow_private_ips: bool,
}

impl ComicOnlineService {
    #[allow(clippy::too_many_arguments)]
    pub fn with_engine(
        sources: Arc<dyn ComicSourceRepository>,
        libraries: Arc<dyn LibraryRepository>,
        series: Arc<dyn SeriesRepository>,
        chapters: Arc<dyn ChapterRepository>,
        reading: Arc<dyn ComicReadingRepository>,
        catalog: Arc<ComicCatalogService>,
        thumbnails: Arc<dyn ThumbnailGeneratorPort>,
        storage: Arc<dyn ComicFileStoragePort>,
        events: Arc<dyn RealtimeEventPort>,
        engine: ComicSourceEngine,
        allow_private_ips: bool,
    ) -> Self {
        Self {
            sources,
            libraries,
            series,
            chapters,
            reading,
            catalog,
            thumbnails,
            storage,
            events,
            engine,
            source_state_locks: Arc::new(Mutex::new(HashMap::new())),
            track_locks: Arc::new(Mutex::new(HashMap::new())),
            chapter_cache_locks: Arc::new(Mutex::new(HashMap::new())),
            rate_limiters: Arc::new(Mutex::new(HashMap::new())),
            seed_import_lock: tokio::sync::Mutex::new(()),
            allow_private_ips,
        }
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

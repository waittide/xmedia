mod debug;
mod dto;
mod helpers;
mod reader;
mod sources;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use uuid::Uuid;

use crate::domain::repository::ComicSourceRepository;
use crate::source::ComicSourceEngine;

pub use dto::*;
use helpers::{AsyncLockMap, RateLimiterMap, get_or_insert_rate_limiter};
use xmedia_source::model::ConcurrentRateSpec;

const SEARCH_MAX_CONCURRENCY: usize = 16;
const SEARCH_PER_SOURCE_TIMEOUT: Duration = Duration::from_secs(20);
const READER_OP_TIMEOUT: Duration = Duration::from_secs(45);
const SOURCE_TEST_TOTAL_TIMEOUT: Duration = Duration::from_secs(90);
const COMIC_IMAGE_PROXY_MAX_BYTES: usize = 16 * 1024 * 1024;

pub struct ComicOnlineService {
    sources: Arc<dyn ComicSourceRepository>,
    engine: ComicSourceEngine,
    source_state_locks: AsyncLockMap<Uuid>,
    rate_limiters: RateLimiterMap,
    seed_import_lock: tokio::sync::Mutex<()>,
    allow_private_ips: bool,
}

impl ComicOnlineService {
    pub fn new(sources: Arc<dyn ComicSourceRepository>) -> Self {
        Self::with_engine(sources, ComicSourceEngine::default(), false)
    }

    pub fn with_engine(
        sources: Arc<dyn ComicSourceRepository>,
        engine: ComicSourceEngine,
        allow_private_ips: bool,
    ) -> Self {
        Self {
            sources,
            engine,
            source_state_locks: Arc::new(Mutex::new(HashMap::new())),
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

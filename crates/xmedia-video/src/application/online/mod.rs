mod browse;
mod debug;
mod dto;
mod sources;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use uuid::Uuid;
use xmedia_source::http::HttpClientConfig;
use xmedia_source::model::ConcurrentRateSpec;
use xmedia_source::rate_limit::{RateLimiterMap, acquire_source_rate_slot};

use crate::domain::repository::VideoSourceRepository;
use crate::source::VideoSourceEngine;

pub use dto::*;

const SEARCH_MAX_CONCURRENCY: usize = 16;
const SEARCH_PER_SOURCE_TIMEOUT: Duration = Duration::from_secs(20);
const SOURCE_TEST_TOTAL_TIMEOUT: Duration = Duration::from_secs(90);

pub(super) type AsyncLockMap<K> = Arc<Mutex<HashMap<K, Weak<tokio::sync::Mutex<()>>>>>;

pub(super) fn get_or_insert_weak_lock<K: Eq + std::hash::Hash>(
    map: &AsyncLockMap<K>,
    key: K,
) -> Arc<tokio::sync::Mutex<()>> {
    let mut guard = map.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(existing) = guard.get(&key).and_then(Weak::upgrade) {
        return existing;
    }
    if guard.len() > 256 {
        guard.retain(|_, w| w.strong_count() > 0);
    }
    let lock = Arc::new(tokio::sync::Mutex::new(()));
    guard.insert(key, Arc::downgrade(&lock));
    lock
}

pub struct VideoOnlineService {
    sources: Arc<dyn VideoSourceRepository>,
    engine: VideoSourceEngine,
    http_config: HttpClientConfig,
    source_state_locks: AsyncLockMap<Uuid>,
    rate_limiters: RateLimiterMap,
    seed_import_lock: tokio::sync::Mutex<()>,
    allow_private_ips: bool,
}

impl VideoOnlineService {
    pub fn new(sources: Arc<dyn VideoSourceRepository>) -> Self {
        Self::with_engine(sources, VideoSourceEngine::default(), false)
    }

    pub fn with_engine(
        sources: Arc<dyn VideoSourceRepository>,
        engine: VideoSourceEngine,
        allow_private_ips: bool,
    ) -> Self {
        let http_config = engine.http_config().clone();
        Self {
            sources,
            engine,
            http_config,
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
        acquire_source_rate_slot(&self.rate_limiters, source_id, spec, timeout).await
    }

    pub fn allow_private_ips(&self) -> bool {
        self.allow_private_ips
    }
}

mod browse;
mod debug;
mod dto;
mod items;
mod sources;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use uuid::Uuid;
use xmedia_source::http::HttpClientConfig;
use xmedia_source::model::ConcurrentRateSpec;
use xmedia_source::rate_limit::{RateLimiterMap, acquire_source_rate_slot};

use crate::domain::repository::VideoSourceRepository;
use crate::source::VideoSourceEngine;

pub use dto::*;
pub use items::VideoOnlineSourceDirectory;

const SEARCH_MAX_CONCURRENCY: usize = 16;
const SEARCH_PER_SOURCE_TIMEOUT: Duration = Duration::from_secs(20);
const DETAIL_OP_TIMEOUT: Duration = Duration::from_secs(25);
const SOURCE_TEST_TOTAL_TIMEOUT: Duration = Duration::from_secs(90);

pub(super) struct CancelOnDrop {
    flag: Arc<AtomicBool>,
    armed: bool,
}

impl CancelOnDrop {
    pub(super) fn new(flag: Arc<AtomicBool>) -> Self {
        Self { flag, armed: true }
    }

    pub(super) fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if self.armed {
            self.flag.store(true, Ordering::Relaxed);
        }
    }
}

pub(super) struct CancelTasksOnDrop {
    entries: Vec<(Arc<AtomicBool>, tokio::task::AbortHandle)>,
    armed: bool,
}

impl CancelTasksOnDrop {
    pub(super) fn new() -> Self {
        Self {
            entries: Vec::new(),
            armed: true,
        }
    }

    pub(super) fn push(&mut self, flag: Arc<AtomicBool>, abort: tokio::task::AbortHandle) {
        self.entries.push((flag, abort));
    }

    pub(super) fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for CancelTasksOnDrop {
    fn drop(&mut self) {
        if self.armed {
            for (flag, abort) in &self.entries {
                flag.store(true, Ordering::Relaxed);
                abort.abort();
            }
        }
    }
}

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
    op_timeout: Duration,
    detail_timeout: Duration,
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
            op_timeout: SEARCH_PER_SOURCE_TIMEOUT,
            detail_timeout: DETAIL_OP_TIMEOUT,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_timeouts(mut self, op_timeout: Duration, detail_timeout: Duration) -> Self {
        self.op_timeout = op_timeout;
        self.detail_timeout = detail_timeout;
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

    pub fn allow_private_ips(&self) -> bool {
        self.allow_private_ips
    }
}

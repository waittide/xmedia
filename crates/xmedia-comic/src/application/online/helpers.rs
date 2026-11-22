use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use uuid::Uuid;
use xmedia_source::model::ConcurrentRateSpec;

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

pub(super) fn build_image_proxy_url(source_id: Uuid, raw_url: &str) -> String {
    let encoded: String = form_urlencoded::byte_serialize(raw_url.as_bytes()).collect();
    format!("/api/v1/comic/online/image-proxy?source_id={source_id}&url={encoded}")
}

pub(super) struct SourceRateLimiter {
    history: tokio::sync::Mutex<VecDeque<Instant>>,
}

impl SourceRateLimiter {
    fn new() -> Self {
        Self {
            history: tokio::sync::Mutex::new(VecDeque::new()),
        }
    }

    pub(super) async fn acquire(
        &self,
        spec: ConcurrentRateSpec,
        timeout: Duration,
    ) -> Result<(), String> {
        let window_requests = spec.window_requests.max(1);
        let window = Duration::from_millis(spec.window_ms.max(1));
        let deadline = Instant::now() + timeout;
        let mut hist = self.history.lock().await;
        loop {
            let now = Instant::now();
            while let Some(&front) = hist.front() {
                if now.saturating_duration_since(front) >= window {
                    hist.pop_front();
                } else {
                    break;
                }
            }
            if hist.len() < window_requests {
                hist.push_back(now);
                return Ok(());
            }
            let oldest = *hist.front().expect("non-empty history");
            let wait_dur = window.saturating_sub(now.saturating_duration_since(oldest));
            let remaining = deadline.saturating_duration_since(now);
            if wait_dur > remaining || remaining.is_zero() {
                return Err("请求受限：超过 concurrentRate 等待超时".into());
            }
            tokio::time::sleep(wait_dur).await;
        }
    }
}

pub(super) type RateLimiterMap = Arc<Mutex<HashMap<Uuid, Arc<SourceRateLimiter>>>>;

pub(super) fn get_or_insert_rate_limiter(
    map: &RateLimiterMap,
    key: Uuid,
) -> Arc<SourceRateLimiter> {
    let mut guard = map.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(existing) = guard.get(&key) {
        return Arc::clone(existing);
    }
    if guard.len() > 256 {
        guard.retain(|_, v| Arc::strong_count(v) > 1);
    }
    let limiter = Arc::new(SourceRateLimiter::new());
    guard.insert(key, Arc::clone(&limiter));
    limiter
}

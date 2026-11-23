use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use uuid::Uuid;

use crate::model::ConcurrentRateSpec;

pub struct SourceRateLimiter {
    history: tokio::sync::Mutex<VecDeque<Instant>>,
}

impl Default for SourceRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl SourceRateLimiter {
    pub fn new() -> Self {
        Self {
            history: tokio::sync::Mutex::new(VecDeque::new()),
        }
    }

    pub async fn acquire(&self, spec: ConcurrentRateSpec, timeout: Duration) -> Result<(), String> {
        let window_requests = spec.window_requests.max(1);
        let window = Duration::from_millis(spec.window_ms.max(1));
        let deadline = Instant::now() + timeout;
        loop {
            let wait_dur = {
                let mut hist = self.history.lock().await;
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
                wait_dur
            };
            tokio::time::sleep(wait_dur).await;
        }
    }
}

pub type RateLimiterMap = Arc<Mutex<HashMap<Uuid, Arc<SourceRateLimiter>>>>;

pub fn get_or_insert_rate_limiter(map: &RateLimiterMap, key: Uuid) -> Arc<SourceRateLimiter> {
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

pub async fn acquire_source_rate_slot(
    map: &RateLimiterMap,
    source_id: Uuid,
    spec: Option<ConcurrentRateSpec>,
    timeout: Duration,
) -> Result<(), String> {
    let Some(rate) = spec else {
        return Ok(());
    };
    let limiter = get_or_insert_rate_limiter(map, source_id);
    limiter.acquire(rate, timeout).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn acquire_releases_lock_while_sleeping() {
        let limiter = Arc::new(SourceRateLimiter::new());
        let spec = ConcurrentRateSpec {
            max_concurrency: 1,
            window_requests: 1,
            window_ms: 400,
        };
        limiter.acquire(spec, Duration::from_secs(1)).await.unwrap();

        let l1 = Arc::clone(&limiter);
        let waiter = tokio::spawn(async move { l1.acquire(spec, Duration::from_secs(2)).await });
        tokio::time::sleep(Duration::from_millis(20)).await;

        let started = Instant::now();
        let fast_fail = limiter.acquire(spec, Duration::from_millis(30)).await;
        assert!(fast_fail.is_err());
        assert!(
            started.elapsed() < Duration::from_millis(200),
            "short-timeout caller must not block behind a sleeping waiter"
        );
        waiter.await.unwrap().unwrap();
    }
}

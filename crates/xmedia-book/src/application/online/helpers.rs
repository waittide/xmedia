use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::Notify;
#[cfg(test)]
use xmedia_core::domain::error::DomainResult;
pub(super) use xmedia_source::concurrency::{
    AsyncLockMap, CancelOnDrop, CancelTasksOnDrop, weak_lock,
};

use crate::domain::online::{ConcurrentRateSpec, PackChapterInput, sanitize_novel_stem};

use super::dto::{DownloadJobStatus, OnlineDownloadJobDto};
use super::{DEFAULT_DOWNLOAD_CONCURRENCY, DEFAULT_DOWNLOAD_MIN_INTERVAL_MS};

pub(super) struct StoredDownloadJob {
    pub(super) dto: OnlineDownloadJobDto,
    pub(super) output_path: Option<PathBuf>,
    pub(super) started_at: Option<Instant>,
    #[allow(dead_code)]
    pub(super) cancel_flag: Arc<AtomicBool>,
    pub(super) cancel_cond: Arc<(Mutex<bool>, Condvar)>,
    pub(super) cancel_notify: Arc<Notify>,
}

impl StoredDownloadJob {
    pub(super) fn snapshot_dto(&self) -> OnlineDownloadJobDto {
        let mut dto = self.dto.clone();
        if dto.status == DownloadJobStatus::Running
            && let Some(started) = self.started_at
        {
            let elapsed = started.elapsed().as_millis() as u64;
            dto.elapsed_ms = elapsed;
            dto.chapters_per_sec = compute_chapters_per_sec(dto.processed_chapters, elapsed);
        }
        dto
    }

    pub(super) fn signal_cancel(&self) {
        self.cancel_flag.store(true, Ordering::Release);
        let (lock, cvar) = &*self.cancel_cond;
        let mut guard = lock.lock().unwrap_or_else(|e| e.into_inner());
        *guard = true;
        cvar.notify_all();
        self.cancel_notify.notify_waiters();
    }
}

pub(super) fn compute_chapters_per_sec(processed: usize, elapsed_ms: u64) -> f64 {
    if processed == 0 || elapsed_ms == 0 {
        return 0.0;
    }
    let rate = processed as f64 / (elapsed_ms as f64 / 1000.0);
    (rate * 100.0).round() / 100.0
}

pub(crate) struct DownloadRateLimiter {
    window_requests: usize,
    window: Duration,
    min_interval: Duration,
    history: Mutex<VecDeque<Instant>>,
}

impl DownloadRateLimiter {
    pub(crate) fn from_spec(spec: Option<ConcurrentRateSpec>) -> (usize, Self) {
        match spec {
            Some(s) => (
                s.max_concurrency,
                Self {
                    window_requests: s.window_requests.max(1),
                    window: Duration::from_millis(s.window_ms.max(1)),
                    min_interval: Duration::ZERO,
                    history: Mutex::new(VecDeque::with_capacity(s.window_requests.min(64))),
                },
            ),
            None => (
                DEFAULT_DOWNLOAD_CONCURRENCY,
                Self {
                    window_requests: DEFAULT_DOWNLOAD_CONCURRENCY,
                    window: Duration::from_millis(
                        DEFAULT_DOWNLOAD_MIN_INTERVAL_MS * DEFAULT_DOWNLOAD_CONCURRENCY as u64,
                    ),
                    min_interval: Duration::from_millis(DEFAULT_DOWNLOAD_MIN_INTERVAL_MS),
                    history: Mutex::new(VecDeque::with_capacity(DEFAULT_DOWNLOAD_CONCURRENCY)),
                },
            ),
        }
    }

    /// Blocks until a request slot is available, or returns `false` immediately upon cancellation.
    pub(crate) fn wait_for_slot(
        &self,
        cancel_flag: &AtomicBool,
        cancel_cond: &(Mutex<bool>, Condvar),
    ) -> bool {
        loop {
            if cancel_flag.load(Ordering::Acquire) {
                return false;
            }
            let wait_dur = {
                let mut hist = self.history.lock().unwrap_or_else(|e| e.into_inner());
                let now = Instant::now();
                while let Some(&front) = hist.front() {
                    if now.saturating_duration_since(front) >= self.window {
                        hist.pop_front();
                    } else {
                        break;
                    }
                }
                let mut need_wait = Duration::ZERO;
                if hist.len() >= self.window_requests
                    && let Some(&oldest) = hist.front()
                {
                    let w = self
                        .window
                        .saturating_sub(now.saturating_duration_since(oldest));
                    need_wait = need_wait.max(w);
                }
                if !self.min_interval.is_zero()
                    && let Some(&last) = hist.back()
                {
                    let w = self
                        .min_interval
                        .saturating_sub(now.saturating_duration_since(last));
                    need_wait = need_wait.max(w);
                }
                if need_wait.is_zero() {
                    hist.push_back(now);
                    return true;
                }
                need_wait
            };

            let (lock, cvar) = cancel_cond;
            let guard = lock.lock().unwrap_or_else(|e| e.into_inner());
            if *guard || cancel_flag.load(Ordering::Acquire) {
                return false;
            }
            let _ = cvar.wait_timeout(guard, wait_dur);
        }
    }
}

pub(super) struct ChapterProgressEvent {
    pub(super) idx: usize,
    pub(super) chapter_title: String,
    pub(super) packed: PackChapterInput,
    pub(super) failed: bool,
    pub(super) from_cache: bool,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ChapterFetchOptions {
    pub(super) force_refresh: bool,
    pub(super) include_logs: bool,
    pub(super) allow_shared_cache_write: bool,
}

#[cfg(test)]
pub(crate) fn validate_safe_cover_image(
    header_ct: Option<&str>,
    bytes: &[u8],
) -> DomainResult<String> {
    xmedia_source::proxy::validate_safe_raster_image(header_ct, bytes).map(str::to_string)
}

pub(super) fn sanitize_file_stem(name: &str) -> String {
    sanitize_novel_stem(name)
}

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::time::{Duration, Instant, SystemTime};

use md5::{Digest, Md5};
use tokio::sync::Notify;
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};

use crate::source::model::{ConcurrentRateSpec, OnlineChapterContent};
use crate::source::packager::PackChapterInput;
use crate::source::tracked_store::write_atomic;

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

pub(super) struct DownloadRateLimiter {
    window_requests: usize,
    window: Duration,
    min_interval: Duration,
    history: Mutex<VecDeque<Instant>>,
}

impl DownloadRateLimiter {
    pub(super) fn from_spec(spec: Option<ConcurrentRateSpec>) -> (usize, Self) {
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
    pub(super) fn wait_for_slot(
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

pub(super) fn validate_cover_header_mime(ct_lower: &str) -> DomainResult<()> {
    if ct_lower.is_empty()
        || ct_lower == "application/octet-stream"
        || ct_lower == "binary/octet-stream"
    {
        return Ok(());
    }
    if !ct_lower.starts_with("image/") || ct_lower.contains("svg") {
        return Err(DomainError::Validation(format!(
            "不支持的封面内容类型: {ct_lower}"
        )));
    }
    Ok(())
}

fn detect_safe_raster_image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some("image/png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.len() >= 14 && bytes.starts_with(b"BM") {
        Some("image/bmp")
    } else if bytes.starts_with(&[0x00, 0x00, 0x01, 0x00]) {
        Some("image/x-icon")
    } else if bytes.len() >= 12
        && &bytes[4..8] == b"ftyp"
        && matches!(
            &bytes[8..12],
            b"avif" | b"avis" | b"heic" | b"heix" | b"mif1"
        )
    {
        Some("image/avif")
    } else {
        None
    }
}

pub(crate) fn validate_safe_cover_image(
    header_ct: Option<&str>,
    bytes: &[u8],
) -> DomainResult<String> {
    if let Some(ct) = header_ct {
        let normalized = ct
            .split(';')
            .next()
            .unwrap_or(ct)
            .trim()
            .to_ascii_lowercase();
        validate_cover_header_mime(&normalized)?;
    }
    let sniffed = detect_safe_raster_image_mime(bytes)
        .ok_or_else(|| DomainError::Validation("封面内容不是受支持的安全图片格式".into()))?;
    Ok(sniffed.to_string())
}

fn chapter_cache_path_at(cache_dir: &Path, source_id: Uuid, chapter_url: &str) -> PathBuf {
    let mut h = Md5::new();
    h.update(chapter_url.as_bytes());
    let key = hex::encode(h.finalize());
    cache_dir
        .join(source_id.to_string())
        .join(format!("{key}.json"))
}

pub(super) fn read_cached_chapter_at(
    cache_dir: &Path,
    source_id: Uuid,
    chapter_url: &str,
) -> Option<OnlineChapterContent> {
    let path = chapter_cache_path_at(cache_dir, source_id, chapter_url);
    let meta = std::fs::metadata(&path).ok()?;
    // Expire cached chapters after 14 days
    if let Ok(modified) = meta.modified()
        && let Ok(age) = SystemTime::now().duration_since(modified)
        && age > Duration::from_secs(14 * 24 * 3600)
    {
        let _ = std::fs::remove_file(&path);
        return None;
    }
    let raw = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str::<OnlineChapterContent>(&raw).ok()
}

pub(super) fn write_cached_chapter_at(
    cache_dir: &Path,
    source_id: Uuid,
    chapter_url: &str,
    content: &OnlineChapterContent,
) {
    if content.content.trim().is_empty() {
        return;
    }
    let path = chapter_cache_path_at(cache_dir, source_id, chapter_url);
    if let Some(parent) = path.parent()
        && let Err(err) = std::fs::create_dir_all(parent)
    {
        tracing::warn!(path = %parent.display(), error = %err, "failed to create chapter cache dir");
        return;
    }
    if let Ok(json) = serde_json::to_vec(content)
        && let Err(err) = write_atomic(&path, &json)
    {
        tracing::warn!(path = %path.display(), error = %err, "failed to atomically write cached chapter");
    }
}

fn is_windows_reserved_stem(stem: &str) -> bool {
    let upper = stem.to_ascii_uppercase();
    matches!(
        upper.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}

pub(super) fn sanitize_file_stem(name: &str) -> String {
    let cleaned: String = name
        .trim()
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('.').trim();
    if trimmed.is_empty() {
        return "novel".to_string();
    }
    let truncated: String = trimmed.chars().take(80).collect();
    if is_windows_reserved_stem(&truncated) {
        format!("_{truncated}")
    } else {
        truncated
    }
}

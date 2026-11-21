use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime};

use bytes::Bytes;
use jiff::Timestamp;
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use tokio::sync::{Notify, RwLock, Semaphore, mpsc};
use uuid::Uuid;
use xmedia_core::application::ports::{ByteRangeSpec, ByteRangeStream};
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::user::{User, UserId, require_admin};
use xmedia_core::infra::stream::open_file_range_stream_spec;

use crate::application::catalog::BookCatalogService;
use crate::application::dto::BookSummaryDto;
use crate::application::ports::CoverPayload;
use crate::domain::book::{Book, BookFormat, BookId, BookMetadata, BookSource};
use crate::domain::online::{BookSourceRecord, OnlineShelfBook};
use crate::domain::repository::{BookSourceRepository, OnlineShelfRepository};
use crate::source::BookSourceEngine;
use crate::source::http::{get_or_build_async_client, validate_target_url_static};
use crate::source::model::{
    BookJsContext, ChapterJsContext, ConcurrentRateSpec, ExecutionDiagnostics, LegadoBookSource,
    LoginUiItem, OnlineBookDetail, OnlineChapterContent, OnlineChapterItem, SearchBookResult,
    SourceRuntimeState,
};
use crate::source::packager::{
    PackBookMetadata, PackChapterInput, package_chapters_to_epub, package_chapters_to_txt,
};
use crate::source::tracked_store::{
    TrackedBookStore, TrackedChapterFile, TrackedChapterMeta, write_atomic,
};

const SEARCH_MAX_CONCURRENCY: usize = 16;
const SEARCH_PER_SOURCE_TIMEOUT: Duration = Duration::from_secs(20);
const SOURCE_TEST_TOTAL_TIMEOUT: Duration = Duration::from_secs(90);
const DEFAULT_DOWNLOAD_CONCURRENCY: usize = 4;
const DEFAULT_DOWNLOAD_MIN_INTERVAL_MS: u64 = 40;
const DOWNLOAD_STATE_PERSIST_BATCH: usize = 20;
const MAX_ACTIVE_DOWNLOADS_PER_USER: usize = 3;
const FINISHED_DOWNLOAD_TTL_SECS: i64 = 24 * 3600;
const TRACKED_EPUB_DEBOUNCE: Duration = Duration::from_secs(6);
const TRACKED_EPUB_IMMEDIATE_BATCH: u32 = 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookSourceSummaryDto {
    pub id: Uuid,
    pub name: String,
    pub source_url: String,
    pub group_name: String,
    pub comment: Option<String>,
    pub check_keyword: Option<String>,
    pub enabled: bool,
    pub weight: i64,
    pub source_type: i64,
    pub has_login_url: bool,
    pub login_ui: Vec<LoginUiItem>,
    pub login_info: HashMap<String, String>,
    pub has_variable: bool,
    pub variable_preview: String,
    pub cookie_hosts: Vec<String>,
    pub last_test_ok: Option<bool>,
    pub last_test_msg: Option<String>,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportBookSourcesReportDto {
    pub imported_count: usize,
    pub updated_count: usize,
    pub sources: Vec<BookSourceSummaryDto>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct UpdateBookSourceInput {
    pub enabled: Option<bool>,
    pub weight: Option<i64>,
    pub group_name: Option<String>,
    pub raw_json: Option<String>,
    pub variable: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum BatchBookSourceAction {
    Enable,
    Disable,
    SetGroup { group_name: String },
    Delete,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BatchBookSourcesInput {
    pub ids: Vec<Uuid>,
    #[serde(flatten)]
    pub action: BatchBookSourceAction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchBookSourcesReportDto {
    pub affected_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginActionResponseDto {
    pub result: String,
    pub diagnostics: ExecutionDiagnostics,
    pub source: BookSourceSummaryDto,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceTestMode {
    #[default]
    Full,
    Search,
    BookInfo,
    Toc,
    Content,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SourceTestInput {
    pub keyword: Option<String>,
    pub mode: SourceTestMode,
    pub book_url: Option<String>,
    pub toc_url: Option<String>,
    pub chapter_url: Option<String>,
    pub chapter_index: Option<usize>,
    pub chapter_title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceDebugStepDto {
    pub stage: String,
    pub label: String,
    pub ok: bool,
    pub elapsed_ms: u64,
    pub summary: String,
    pub error: Option<String>,
    pub diagnostics: ExecutionDiagnostics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceTestReportDto {
    pub ok: bool,
    pub mode: SourceTestMode,
    pub keyword: String,
    pub total_elapsed_ms: u64,
    pub result_count: usize,
    pub sample_titles: Vec<String>,
    pub error: Option<String>,
    pub diagnostics: ExecutionDiagnostics,
    pub steps: Vec<SourceDebugStepDto>,
    pub search_items: Vec<SearchBookResult>,
    pub book_detail: Option<OnlineBookDetail>,
    pub toc_chapters: Vec<OnlineChapterItem>,
    pub toc_total_count: usize,
    pub content_chapter_index: Option<usize>,
    pub content_chapter_title: Option<String>,
    pub content_chapter_url: Option<String>,
    pub content_text: Option<String>,
    pub content_char_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregatedSearchItemDto {
    pub source_id: Uuid,
    pub source_name: String,
    pub book: SearchBookResult,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceSearchErrorDto {
    pub source_id: Uuid,
    pub source_name: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregatedSearchResponseDto {
    pub keyword: String,
    pub page: u32,
    pub items: Vec<AggregatedSearchItemDto>,
    pub errors: Vec<SourceSearchErrorDto>,
    pub toasts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineBookDetailResponseDto {
    pub source_id: Uuid,
    pub source_name: String,
    pub detail: OnlineBookDetail,
    pub diagnostics: ExecutionDiagnostics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineTocResponseDto {
    pub source_id: Uuid,
    pub chapters: Vec<OnlineChapterItem>,
    pub diagnostics: ExecutionDiagnostics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineContentResponseDto {
    pub source_id: Uuid,
    pub chapter_index: usize,
    pub content: OnlineChapterContent,
    pub cached: bool,
    pub diagnostics: ExecutionDiagnostics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineShelfBookDto {
    pub id: Uuid,
    pub source_id: Uuid,
    pub source_name: String,
    pub name: String,
    pub author: String,
    pub book_url: String,
    pub toc_url: String,
    pub cover_url: Option<String>,
    pub intro: Option<String>,
    pub kind: Option<String>,
    pub last_chapter: Option<String>,
    pub total_chapters: i64,
    pub current_chapter_index: i64,
    pub current_chapter_title: Option<String>,
    pub scroll_ratio: f64,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AddOnlineShelfInput {
    pub source_id: Uuid,
    pub name: String,
    #[serde(default)]
    pub author: String,
    pub book_url: String,
    #[serde(default)]
    pub toc_url: String,
    #[serde(default)]
    pub cover_url: Option<String>,
    #[serde(default)]
    pub intro: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub last_chapter: Option<String>,
    #[serde(default)]
    pub total_chapters: Option<i64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct UpdateOnlineShelfInput {
    pub current_chapter_index: Option<i64>,
    pub current_chapter_title: Option<String>,
    pub scroll_ratio: Option<f64>,
    pub total_chapters: Option<i64>,
    pub last_chapter: Option<String>,
    pub toc_url: Option<String>,
    // Optional source switch
    pub source_id: Option<Uuid>,
    pub book_url: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OnlineDownloadFormat {
    Epub,
    Txt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DownloadJobStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TrackOnlineBookInput {
    pub source_id: Uuid,
    pub name: String,
    #[serde(default)]
    pub author: String,
    pub book_url: String,
    #[serde(default)]
    pub toc_url: String,
    #[serde(default)]
    pub cover_url: Option<String>,
    #[serde(default)]
    pub intro: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub last_chapter: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TrackedTocResponseDto {
    pub book_id: BookId,
    pub source_id: Uuid,
    pub source_name: String,
    pub chapters: Vec<TrackedChapterMeta>,
    pub cached_chapters: u32,
    pub total_chapters: u32,
    pub last_chapter: Option<String>,
    pub epub_dirty: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct TrackedChapterResponseDto {
    pub book_id: BookId,
    pub source_id: Uuid,
    pub chapter_index: usize,
    pub title: String,
    pub content: String,
    pub cached: bool,
    pub cached_chapters: u32,
    pub total_chapters: u32,
    pub epub_dirty: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SwitchTrackedSourceInput {
    pub source_id: Uuid,
    pub book_url: String,
    pub toc_url: String,
    #[serde(default)]
    pub cover_url: Option<String>,
    #[serde(default)]
    pub intro: Option<String>,
    #[serde(default)]
    pub last_chapter: Option<String>,
    #[serde(default)]
    pub clear_cache: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StartDownloadInput {
    #[serde(default)]
    pub book_id: Option<Uuid>,
    pub source_id: Uuid,
    pub book_name: String,
    #[serde(default)]
    pub author: String,
    pub book_url: String,
    pub toc_url: String,
    #[serde(default)]
    pub cover_url: Option<String>,
    #[serde(default)]
    pub intro: Option<String>,
    pub format: OnlineDownloadFormat,
    #[serde(default)]
    pub start_chapter: Option<usize>,
    #[serde(default)]
    pub end_chapter: Option<usize>,
    #[serde(default)]
    pub import_to_library: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineDownloadJobDto {
    pub id: Uuid,
    pub user_id: Uuid,
    pub source_id: Uuid,
    pub source_name: String,
    pub book_name: String,
    pub author: String,
    pub format: OnlineDownloadFormat,
    pub status: DownloadJobStatus,
    pub total_chapters: usize,
    pub processed_chapters: usize,
    pub failed_chapters: usize,
    #[serde(default)]
    pub chapters_per_sec: f64,
    #[serde(default)]
    pub elapsed_ms: u64,
    pub current_chapter_title: Option<String>,
    pub output_file_name: Option<String>,
    pub file_size: Option<u64>,
    pub import_to_library: bool,
    pub imported_book_id: Option<Uuid>,
    pub error: Option<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

struct StoredDownloadJob {
    dto: OnlineDownloadJobDto,
    output_path: Option<PathBuf>,
    started_at: Option<Instant>,
    cancel_flag: Arc<AtomicBool>,
    cancel_cond: Arc<(Mutex<bool>, Condvar)>,
    cancel_notify: Arc<Notify>,
}

impl StoredDownloadJob {
    fn snapshot_dto(&self) -> OnlineDownloadJobDto {
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

    fn signal_cancel(&self) {
        self.cancel_flag.store(true, Ordering::Release);
        let (lock, cvar) = &*self.cancel_cond;
        let mut guard = lock.lock().unwrap_or_else(|e| e.into_inner());
        *guard = true;
        cvar.notify_all();
        self.cancel_notify.notify_waiters();
    }
}

fn compute_chapters_per_sec(processed: usize, elapsed_ms: u64) -> f64 {
    if processed == 0 || elapsed_ms == 0 {
        return 0.0;
    }
    let rate = processed as f64 / (elapsed_ms as f64 / 1000.0);
    (rate * 100.0).round() / 100.0
}

struct DownloadRateLimiter {
    window_requests: usize,
    window: Duration,
    min_interval: Duration,
    history: Mutex<VecDeque<Instant>>,
}

impl DownloadRateLimiter {
    fn from_spec(spec: Option<ConcurrentRateSpec>) -> (usize, Self) {
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
    fn wait_for_slot(
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

struct ChapterProgressEvent {
    idx: usize,
    chapter_title: String,
    packed: PackChapterInput,
    failed: bool,
    from_cache: bool,
}

#[derive(Debug, Clone, Copy)]
struct ChapterFetchOptions {
    force_refresh: bool,
    include_logs: bool,
    allow_shared_cache_write: bool,
}

type AsyncLockMap<K> = Arc<Mutex<HashMap<K, Arc<tokio::sync::Mutex<()>>>>>;

pub struct BookOnlineService {
    sources: Arc<dyn BookSourceRepository>,
    shelves: Arc<dyn OnlineShelfRepository>,
    catalog: Arc<BookCatalogService>,
    engine: BookSourceEngine,
    cache_dir: PathBuf,
    download_dir: PathBuf,
    tracked_store: TrackedBookStore,
    book_state_locks: AsyncLockMap<BookId>,
    epub_sync_locks: AsyncLockMap<BookId>,
    pending_sync_gens: Arc<Mutex<HashMap<BookId, (u64, u32)>>>,
    track_locks: AsyncLockMap<(Uuid, String)>,
    source_state_locks: AsyncLockMap<Uuid>,
    downloads: Arc<RwLock<HashMap<Uuid, StoredDownloadJob>>>,
    allow_private_ips: bool,
}

impl BookOnlineService {
    pub fn new(
        sources: Arc<dyn BookSourceRepository>,
        shelves: Arc<dyn OnlineShelfRepository>,
        data_dir: &Path,
        catalog: Arc<BookCatalogService>,
    ) -> DomainResult<Self> {
        Self::with_engine(
            sources,
            shelves,
            data_dir,
            catalog,
            BookSourceEngine::default(),
            false,
        )
    }

    pub fn with_engine(
        sources: Arc<dyn BookSourceRepository>,
        shelves: Arc<dyn OnlineShelfRepository>,
        data_dir: &Path,
        catalog: Arc<BookCatalogService>,
        engine: BookSourceEngine,
        allow_private_ips: bool,
    ) -> DomainResult<Self> {
        let cache_dir = data_dir.join("online_book_cache");
        let download_dir = data_dir.join("online_downloads");
        std::fs::create_dir_all(&cache_dir).map_err(|e| {
            DomainError::Storage(format!(
                "创建在线书源缓存目录 '{}' 失败: {e}",
                cache_dir.display()
            ))
        })?;
        std::fs::create_dir_all(&download_dir).map_err(|e| {
            DomainError::Storage(format!(
                "创建在线下载输出目录 '{}' 失败: {e}",
                download_dir.display()
            ))
        })?;
        let tracked_store = TrackedBookStore::new(data_dir).map_err(DomainError::Storage)?;
        let book_state_locks: Arc<Mutex<HashMap<BookId, Arc<tokio::sync::Mutex<()>>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let epub_sync_locks: Arc<Mutex<HashMap<BookId, Arc<tokio::sync::Mutex<()>>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let pending_sync_gens: Arc<Mutex<HashMap<BookId, (u64, u32)>>> =
            Arc::new(Mutex::new(HashMap::new()));

        {
            let state_locks = Arc::clone(&book_state_locks);
            let sync_locks = Arc::clone(&epub_sync_locks);
            let gens = Arc::clone(&pending_sync_gens);
            let store = tracked_store.clone();
            catalog.set_on_book_deleted_hook(Arc::new(move |book_id| {
                state_locks
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&book_id);
                sync_locks
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&book_id);
                gens.lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&book_id);
                store.remove_book_lock(book_id);
            }));
        }

        Ok(Self {
            sources,
            shelves,
            catalog,
            engine,
            cache_dir,
            download_dir,
            tracked_store,
            book_state_locks,
            epub_sync_locks,
            pending_sync_gens,
            track_locks: Arc::new(Mutex::new(HashMap::new())),
            source_state_locks: Arc::new(Mutex::new(HashMap::new())),
            downloads: Arc::new(RwLock::new(HashMap::new())),
            allow_private_ips,
        })
    }

    /// Removes in-memory synchronization locks and generation counters for a deleted book.
    pub fn cleanup_book_state(&self, book_id: BookId) {
        self.book_state_locks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&book_id);
        self.epub_sync_locks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&book_id);
        self.pending_sync_gens
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&book_id);
        self.tracked_store.remove_book_lock(book_id);
    }

    fn is_sensitive_login_field(key: &str, login_ui: &[LoginUiItem]) -> bool {
        if login_ui
            .iter()
            .any(|item| item.name == key && item.item_type.eq_ignore_ascii_case("password"))
        {
            return true;
        }
        let lower = key.to_ascii_lowercase();
        lower.contains("password")
            || lower.contains("passwd")
            || lower.contains("pwd")
            || lower.contains("token")
            || lower.contains("secret")
            || lower.contains("cookie")
            || lower.contains("key")
            || key.contains("密码")
            || key.contains("口令")
            || key.contains("密钥")
            || key.contains("授权码")
            || key.contains("凭证")
    }

    fn record_to_summary(m: &BookSourceRecord, is_admin: bool) -> BookSourceSummaryDto {
        let parsed = serde_json::from_str::<LegadoBookSource>(&m.raw_json).ok();
        let login_ui = parsed
            .as_ref()
            .map(LegadoBookSource::parse_login_ui)
            .unwrap_or_default();
        let has_login_url = parsed
            .as_ref()
            .and_then(|p| p.login_url.as_deref())
            .is_some_and(|s| !s.trim().is_empty())
            || !login_ui.is_empty();
        let comment = parsed.as_ref().and_then(|p| p.book_source_comment.clone());
        let check_keyword = parsed
            .as_ref()
            .and_then(|p| p.rule_search.as_ref())
            .and_then(|r| r.check_key_word.as_deref())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);

        let (login_info, variable_preview, cookie_hosts) = if is_admin {
            let mut masked = HashMap::new();
            for (k, v) in &m.runtime_state.login_info {
                let is_sensitive = Self::is_sensitive_login_field(k, &login_ui);
                masked.insert(
                    k.clone(),
                    if is_sensitive && !v.is_empty() {
                        "••••••••".to_string()
                    } else {
                        v.clone()
                    },
                );
            }
            let preview: String = m.runtime_state.variable.chars().take(240).collect();
            let mut hosts: Vec<String> = m.runtime_state.cookies.keys().cloned().collect();
            hosts.sort();
            (masked, preview, hosts)
        } else {
            (HashMap::new(), String::new(), Vec::new())
        };

        BookSourceSummaryDto {
            id: m.id,
            name: m.name.clone(),
            source_url: m.source_url.clone(),
            group_name: m.group_name.clone(),
            comment,
            check_keyword,
            enabled: m.enabled,
            weight: m.weight,
            source_type: m.source_type,
            has_login_url,
            login_ui: if is_admin { login_ui } else { Vec::new() },
            login_info,
            has_variable: !m.runtime_state.variable.trim().is_empty(),
            variable_preview,
            cookie_hosts,
            last_test_ok: m.last_test_ok,
            last_test_msg: m.last_test_msg.clone(),
            updated_at: m.updated_at,
        }
    }

    async fn load_source(&self, id: Uuid) -> DomainResult<BookSourceRecord> {
        self.sources
            .find_by_id(id)
            .await?
            .ok_or_else(|| DomainError::NotFound(format!("未找到书源 {id}")))
    }

    async fn load_source_for_actor(
        &self,
        actor: &User,
        id: Uuid,
    ) -> DomainResult<BookSourceRecord> {
        let rec = self.load_source(id).await?;
        if !actor.role.is_admin() && !rec.enabled {
            return Err(DomainError::Forbidden("该书源已被禁用".into()));
        }
        Ok(rec)
    }

    /// Returns the full untruncated runtime variable for a source (admin only).
    pub async fn get_source_variable(&self, actor: &User, id: Uuid) -> DomainResult<String> {
        require_admin(actor)?;
        let rec = self.load_source(id).await?;
        Ok(rec.runtime_state.variable)
    }

    fn source_state_lock_for(&self, id: Uuid) -> Arc<tokio::sync::Mutex<()>> {
        let mut guard = self
            .source_state_locks
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        Arc::clone(
            guard
                .entry(id)
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(()))),
        )
    }

    async fn persist_runtime_state(
        &self,
        id: Uuid,
        new_state: SourceRuntimeState,
    ) -> DomainResult<()> {
        let lock = self.source_state_lock_for(id);
        let _guard = lock.lock().await;
        let merged = if let Some(current_rec) = self.sources.find_by_id(id).await? {
            let mut merged = current_rec.runtime_state;
            merged.cookies.extend(new_state.cookies);
            merged.cache.extend(new_state.cache);
            merged.login_info.extend(new_state.login_info);
            if !new_state.variable.is_empty() {
                merged.variable = new_state.variable;
            }
            if !new_state.android_id.is_empty() {
                merged.android_id = new_state.android_id;
            }
            merged
        } else {
            new_state
        };
        self.sources
            .update_runtime_state(id, merged, Timestamp::now())
            .await
    }

    pub async fn list_sources(&self, actor: &User) -> DomainResult<Vec<BookSourceSummaryDto>> {
        let mut rows = self.sources.list_all().await?;
        if !actor.role.is_admin() {
            rows.retain(|r| r.enabled);
        }
        rows.sort_by(|a, b| b.weight.cmp(&a.weight).then_with(|| a.name.cmp(&b.name)));
        Ok(rows
            .iter()
            .map(|r| Self::record_to_summary(r, actor.role.is_admin()))
            .collect())
    }

    pub async fn reorder_sources(
        &self,
        actor: &User,
        ordered_ids: Vec<Uuid>,
    ) -> DomainResult<Vec<BookSourceSummaryDto>> {
        require_admin(actor)?;
        let mut rows = self.sources.list_all().await?;
        rows.sort_by(|a, b| b.weight.cmp(&a.weight).then_with(|| a.name.cmp(&b.name)));

        let mut by_id: HashMap<Uuid, BookSourceRecord> =
            rows.into_iter().map(|r| (r.id, r)).collect();

        let mut final_order: Vec<BookSourceRecord> = Vec::with_capacity(by_id.len());
        for id in ordered_ids {
            if let Some(rec) = by_id.remove(&id) {
                final_order.push(rec);
            }
        }
        // Preserve relative order of any sources not included in `ordered_ids`
        if !by_id.is_empty() {
            let mut rest: Vec<BookSourceRecord> = by_id.into_values().collect();
            rest.sort_by(|a, b| b.weight.cmp(&a.weight).then_with(|| a.name.cmp(&b.name)));
            final_order.extend(rest);
        }

        let total = final_order.len() as i64;
        let now = Timestamp::now();
        let mut persisted = Vec::with_capacity(final_order.len());
        for (idx, rec) in final_order.into_iter().enumerate() {
            let target_weight = (total - idx as i64) * 10;
            if rec.weight != target_weight {
                if let Some(updated) = self
                    .sources
                    .update_weight_if_exists(rec.id, target_weight, now)
                    .await?
                {
                    persisted.push(updated);
                }
            } else {
                persisted.push(rec);
            }
        }

        Ok(persisted
            .iter()
            .map(|r| Self::record_to_summary(r, true))
            .collect())
    }

    pub async fn import_sources(
        &self,
        actor: &User,
        raw_json: &str,
    ) -> DomainResult<ImportBookSourcesReportDto> {
        require_admin(actor)?;
        let parsed_batch =
            LegadoBookSource::parse_json_batch(raw_json).map_err(DomainError::Validation)?;

        let existing_all = self.sources.list_all().await?;
        let mut existing_by_key: HashMap<(String, String), BookSourceRecord> = existing_all
            .into_iter()
            .map(|m| ((m.source_url.clone(), m.name.clone()), m))
            .collect();

        let mut imported_count = 0usize;
        let mut updated_count = 0usize;
        let mut out_by_id: HashMap<Uuid, usize> = HashMap::new();
        let mut out: Vec<BookSourceSummaryDto> = Vec::with_capacity(parsed_batch.len());
        let now = Timestamp::now();

        for (src, item_raw) in parsed_batch {
            let group_name = src.book_source_group.clone().unwrap_or_default();
            let key = (src.book_source_url.clone(), src.book_source_name.clone());

            if let Some(mut rec) = existing_by_key.remove(&key) {
                rec.group_name = group_name;
                rec.enabled = src.enabled;
                rec.weight = src.weight;
                rec.source_type = src.book_source_type;
                rec.raw_json = item_raw;
                rec.updated_at = now;
                self.sources.save(&rec).await?;
                let summary = Self::record_to_summary(&rec, true);
                if let Some(&pos) = out_by_id.get(&rec.id) {
                    out[pos] = summary;
                } else {
                    updated_count += 1;
                    out_by_id.insert(rec.id, out.len());
                    out.push(summary);
                }
                // Re-insert so duplicate keys within the same batch update this record
                existing_by_key.insert(key, rec);
            } else {
                let mut rt = SourceRuntimeState::default();
                rt.ensure_android_id(&src.book_source_url);
                let rec = BookSourceRecord {
                    id: Uuid::now_v7(),
                    name: src.book_source_name.clone(),
                    source_url: src.book_source_url.clone(),
                    group_name,
                    enabled: src.enabled,
                    weight: src.weight,
                    source_type: src.book_source_type,
                    raw_json: item_raw,
                    runtime_state: rt,
                    last_test_ok: None,
                    last_test_msg: None,
                    created_at: now,
                    updated_at: now,
                };
                self.sources.save(&rec).await?;
                imported_count += 1;
                out_by_id.insert(rec.id, out.len());
                out.push(Self::record_to_summary(&rec, true));
                existing_by_key.insert(key, rec);
            }
        }

        Ok(ImportBookSourcesReportDto {
            imported_count,
            updated_count,
            sources: out,
        })
    }

    pub async fn update_source(
        &self,
        actor: &User,
        id: Uuid,
        input: UpdateBookSourceInput,
    ) -> DomainResult<BookSourceSummaryDto> {
        require_admin(actor)?;
        let mut rec = self.load_source(id).await?;
        let now = Timestamp::now();

        if let Some(raw) = input.raw_json {
            let mut batch =
                LegadoBookSource::parse_json_batch(&raw).map_err(DomainError::Validation)?;
            if batch.len() != 1 {
                return Err(DomainError::Validation(
                    "编辑单条书源时仅支持提交 1 条书源 JSON".into(),
                ));
            }
            let (parsed, clean_raw) = batch.remove(0);
            rec.name = parsed.book_source_name;
            rec.source_url = parsed.book_source_url;
            rec.group_name = parsed.book_source_group.unwrap_or_default();
            rec.enabled = parsed.enabled;
            rec.weight = parsed.weight;
            rec.source_type = parsed.book_source_type;
            rec.raw_json = clean_raw;
        }
        if let Some(en) = input.enabled {
            rec.enabled = en;
        }
        if let Some(w) = input.weight {
            rec.weight = w;
        }
        if let Some(ref g) = input.group_name {
            rec.group_name = g.trim().to_string();
        }
        if input.enabled.is_some() || input.weight.is_some() || input.group_name.is_some() {
            rec.raw_json = BookSourceRecord::sync_raw_json_fields(
                &rec.raw_json,
                input.enabled,
                input.weight,
                input.group_name.as_deref(),
            );
        }
        if let Some(v) = input.variable {
            rec.runtime_state.variable = v;
        }
        rec.updated_at = now;

        if !self.sources.update_existing(&rec).await? {
            return Err(DomainError::NotFound(format!("未找到书源 {id}")));
        }
        Ok(Self::record_to_summary(&rec, true))
    }

    pub async fn delete_source(&self, actor: &User, id: Uuid) -> DomainResult<()> {
        require_admin(actor)?;
        let _ = self.load_source(id).await?;
        self.sources.delete(id).await?;
        let src_cache = self.cache_dir.join(id.to_string());
        if let Err(err) = tokio::fs::remove_dir_all(&src_cache).await
            && err.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(source_id = %id, error = %err, "failed to remove source cache dir");
        }
        Ok(())
    }

    pub async fn export_source_json(&self, actor: &User, id: Uuid) -> DomainResult<String> {
        require_admin(actor)?;
        let rec = self.load_source(id).await?;
        Ok(rec.raw_json)
    }

    pub async fn export_sources_batch_json(
        &self,
        actor: &User,
        ids: Vec<Uuid>,
    ) -> DomainResult<String> {
        require_admin(actor)?;
        if ids.is_empty() {
            return Err(DomainError::Validation("请至少选择一条书源".into()));
        }
        let all = self.sources.list_all().await?;
        let mut by_id: HashMap<Uuid, BookSourceRecord> =
            all.into_iter().map(|r| (r.id, r)).collect();
        let mut arr: Vec<serde_json::Value> = Vec::new();
        for id in ids {
            if let Some(rec) = by_id.remove(&id)
                && let Ok(val) = serde_json::from_str::<serde_json::Value>(&rec.raw_json)
            {
                arr.push(val);
            }
        }
        if arr.is_empty() {
            return Err(DomainError::NotFound("未找到可导出的书源".into()));
        }
        serde_json::to_string_pretty(&arr)
            .map_err(|e| DomainError::Storage(format!("序列化书源导出 JSON 失败: {e}")))
    }

    pub async fn batch_sources(
        &self,
        actor: &User,
        input: BatchBookSourcesInput,
    ) -> DomainResult<BatchBookSourcesReportDto> {
        require_admin(actor)?;
        if input.ids.is_empty() {
            return Err(DomainError::Validation("请至少选择一条书源".into()));
        }

        let mut seen = HashSet::with_capacity(input.ids.len());
        let unique_ids: Vec<Uuid> = input
            .ids
            .into_iter()
            .filter(|id| seen.insert(*id))
            .collect();

        let now = Timestamp::now();
        let mut affected_count = 0usize;

        for id in unique_ids {
            match &input.action {
                BatchBookSourceAction::Enable => {
                    if self
                        .sources
                        .update_batch_fields_if_exists(id, Some(true), None, now)
                        .await?
                    {
                        affected_count += 1;
                    }
                }
                BatchBookSourceAction::Disable => {
                    if self
                        .sources
                        .update_batch_fields_if_exists(id, Some(false), None, now)
                        .await?
                    {
                        affected_count += 1;
                    }
                }
                BatchBookSourceAction::SetGroup { group_name } => {
                    if self
                        .sources
                        .update_batch_fields_if_exists(id, None, Some(group_name.clone()), now)
                        .await?
                    {
                        affected_count += 1;
                    }
                }
                BatchBookSourceAction::Delete => {
                    if self.sources.find_by_id(id).await?.is_some() {
                        self.sources.delete(id).await?;
                        let src_cache = self.cache_dir.join(id.to_string());
                        if let Err(err) = tokio::fs::remove_dir_all(&src_cache).await
                            && err.kind() != std::io::ErrorKind::NotFound
                        {
                            tracing::warn!(source_id = %id, error = %err, "failed to remove source cache dir");
                        }
                        affected_count += 1;
                    }
                }
            }
        }

        Ok(BatchBookSourcesReportDto { affected_count })
    }

    pub async fn run_login_action(
        &self,
        actor: &User,
        id: Uuid,
        login_info: HashMap<String, String>,
        action: Option<String>,
    ) -> DomainResult<LoginActionResponseDto> {
        require_admin(actor)?;
        let rec = self.load_source(id).await?;
        let parsed: LegadoBookSource = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("书源 JSON 无效: {e}")))?;

        // Merge submitted fields onto persisted login_info so omitted keys are never wiped,
        // and preserve real secrets when the frontend echoes back the masked placeholder.
        let mut merged_login = rec.runtime_state.login_info.clone();
        for (k, v) in login_info {
            if v == "••••••••" && merged_login.contains_key(&k) {
                continue;
            }
            merged_login.insert(k, v);
        }

        let engine = self.engine.clone();
        let prev_state = rec.runtime_state.clone();
        let action_clone = action.clone();

        let out = tokio::task::spawn_blocking(move || {
            engine.run_login_action_sync(&parsed, prev_state, merged_login, action_clone.as_deref())
        })
        .await
        .map_err(|e| DomainError::Storage(format!("执行登录脚本线程失败: {e}")))?
        .map_err(DomainError::Validation)?;

        self.persist_runtime_state(id, out.state).await?;
        let refreshed = self.load_source(id).await?;

        Ok(LoginActionResponseDto {
            result: out.data,
            diagnostics: out.diagnostics,
            source: Self::record_to_summary(&refreshed, true),
        })
    }

    pub async fn test_source(
        &self,
        actor: &User,
        id: Uuid,
        input: SourceTestInput,
    ) -> DomainResult<SourceTestReportDto> {
        require_admin(actor)?;
        let rec = self.load_source(id).await?;
        let parsed: LegadoBookSource = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("书源 JSON 无效: {e}")))?;

        let raw_input = input
            .keyword
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);

        let default_kw = parsed
            .rule_search
            .as_ref()
            .and_then(|r| r.check_key_word.as_deref())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("我的")
            .to_string();

        let is_direct_url = |s: &str| {
            s.starts_with("http://") || s.starts_with("https://") || s.starts_with("data:")
        };

        let explicit_book_url = input
            .book_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);
        let explicit_toc_url = input
            .toc_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);
        let explicit_chapter_url = input
            .chapter_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);

        match input.mode {
            SourceTestMode::BookInfo => {
                if explicit_book_url.is_none() && raw_input.is_none() {
                    return Err(DomainError::Validation(
                        "请输入书籍详情页 URL，或在搜索结果中点击「测此书详情」".into(),
                    ));
                }
            }
            SourceTestMode::Toc => {
                if explicit_toc_url.is_none() && explicit_book_url.is_none() && raw_input.is_none()
                {
                    return Err(DomainError::Validation(
                        "请输入目录页 URL，或先运行详情测试".into(),
                    ));
                }
            }
            SourceTestMode::Content => {
                if explicit_chapter_url.is_none() && raw_input.is_none() {
                    return Err(DomainError::Validation(
                        "请输入章节正文页 URL，或在目录列表中点击「测本章正文」".into(),
                    ));
                }
            }
            SourceTestMode::Full | SourceTestMode::Search => {}
        }

        let effective_keyword = match input.mode {
            SourceTestMode::BookInfo => explicit_book_url
                .clone()
                .or_else(|| raw_input.clone())
                .unwrap_or_else(|| default_kw.clone()),
            SourceTestMode::Toc => explicit_toc_url
                .clone()
                .or_else(|| explicit_book_url.clone())
                .or_else(|| raw_input.clone())
                .unwrap_or_else(|| default_kw.clone()),
            SourceTestMode::Content => explicit_chapter_url
                .clone()
                .or_else(|| raw_input.clone())
                .unwrap_or_else(|| default_kw.clone()),
            SourceTestMode::Full => explicit_book_url
                .clone()
                .or_else(|| raw_input.clone())
                .unwrap_or_else(|| default_kw.clone()),
            SourceTestMode::Search => raw_input.clone().unwrap_or_else(|| default_kw.clone()),
        };
        let mode = input.mode;
        let chapter_index_override = input.chapter_index;
        let chapter_title_override = input
            .chapter_title
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);
        let engine = self.engine.clone();
        let prev_state = rec.runtime_state.clone();
        let kw_clone = effective_keyword.clone();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_worker = cancel_flag.clone();

        let timed_res = tokio::time::timeout(
            SOURCE_TEST_TOTAL_TIMEOUT,
            tokio::task::spawn_blocking(move || {
                fn merge_diag(acc: &mut ExecutionDiagnostics, d: &ExecutionDiagnostics) {
                    acc.logs.extend(d.logs.iter().cloned());
                    acc.toasts.extend(d.toasts.iter().cloned());
                    acc.browser_urls.extend(d.browser_urls.iter().cloned());
                }

                let total_start = Instant::now();
                let remaining_budget = || {
                    SOURCE_TEST_TOTAL_TIMEOUT
                        .saturating_sub(total_start.elapsed())
                        .max(Duration::from_millis(1))
                };
                let mut state = prev_state;
                let mut steps: Vec<SourceDebugStepDto> = Vec::new();
                let mut all_diag = ExecutionDiagnostics::default();

                let mut search_items: Vec<SearchBookResult> = Vec::new();
                let mut search_total_count: usize = 0;
                let mut book_detail: Option<OnlineBookDetail> = None;
                let mut toc_chapters: Vec<OnlineChapterItem> = Vec::new();
                let mut toc_total_count: usize = 0;
                let mut content_chapter_index: Option<usize> = None;
                let mut content_chapter_title: Option<String> = None;
                let mut content_chapter_url: Option<String> = None;
                let mut content_text: Option<String> = None;
                let mut content_char_count: usize = 0;

                let mut next_book_url: Option<String> = explicit_book_url.clone();
                let mut next_fallback_hint: Option<BookJsContext> = None;
                let mut next_toc_url: Option<String> = explicit_toc_url.clone();
                let mut next_chapter_url: Option<String> = explicit_chapter_url.clone();
                let mut next_chapter_title = chapter_title_override.unwrap_or_default();
                let mut next_chapter_idx = chapter_index_override.unwrap_or(0);

                // In Full mode, pasting a direct URL (including data:) into the keyword box jumps straight to book_info
                let skip_search_for_url = mode == SourceTestMode::Full
                    && (next_book_url.is_some() || raw_input.as_deref().is_some_and(is_direct_url));
                if skip_search_for_url && next_book_url.is_none() {
                    next_book_url = raw_input.clone();
                }

                // Stage 1: ruleSearch
                if mode == SourceTestMode::Search
                    || (mode == SourceTestMode::Full && !skip_search_for_url)
                {
                    let t0 = Instant::now();
                    match engine.search_sync_with_control(
                        &parsed,
                        state.clone(),
                        &kw_clone,
                        1,
                        remaining_budget(),
                        cancel_worker.clone(),
                    ) {
                        Ok(out) => {
                            let elapsed_ms = t0.elapsed().as_millis() as u64;
                            state = out.state;
                            merge_diag(&mut all_diag, &out.diagnostics);
                            search_total_count = out.data.len();
                            let ok = search_total_count > 0;
                            let summary = if ok {
                                format!("命中 {search_total_count} 本（关键词：{kw_clone}）")
                            } else {
                                format!("关键词「{kw_clone}」未匹配到书籍")
                            };
                            let err = if ok {
                                None
                            } else {
                                Some("搜索未返回条目，请检查关键词、是否需要登录或书源规则".into())
                            };
                            search_items = out.data.into_iter().take(30).collect();
                            if let Some(first) = search_items.first() {
                                next_book_url = Some(first.book_url.clone());
                                next_fallback_hint = Some(BookJsContext {
                                    name: first.name.clone(),
                                    author: first.author.clone(),
                                    book_url: first.book_url.clone(),
                                    toc_url: String::new(),
                                    cover_url: first.cover_url.clone(),
                                    intro: first.intro.clone(),
                                    dur_chapter_index: 0,
                                    dur_chapter_title: String::new(),
                                    custom_variable: String::new(),
                                });
                            }
                            steps.push(SourceDebugStepDto {
                                stage: "search".into(),
                                label: "搜索规则 (ruleSearch)".into(),
                                ok,
                                elapsed_ms,
                                summary,
                                error: err,
                                diagnostics: out.diagnostics,
                            });
                        }
                        Err(err) => {
                            let elapsed_ms = t0.elapsed().as_millis() as u64;
                            merge_diag(&mut all_diag, &err.diagnostics);
                            steps.push(SourceDebugStepDto {
                                stage: "search".into(),
                                label: "搜索规则 (ruleSearch)".into(),
                                ok: false,
                                elapsed_ms,
                                summary: "搜索请求或解析失败".into(),
                                error: Some(err.message),
                                diagnostics: err.diagnostics,
                            });
                        }
                    }
                }

                // Stage 2: ruleBookInfo
                let run_book_info = match mode {
                    SourceTestMode::BookInfo => {
                        if next_book_url.is_none() {
                            next_book_url = raw_input.clone();
                        }
                        next_book_url.is_some()
                    }
                    SourceTestMode::Full => next_book_url.is_some(),
                    _ => false,
                };

                if run_book_info && let Some(ref b_url) = next_book_url {
                    let t0 = Instant::now();
                    match engine.fetch_book_info_sync_with_control(
                        &parsed,
                        state.clone(),
                        b_url,
                        next_fallback_hint.as_ref(),
                        remaining_budget(),
                        cancel_worker.clone(),
                    ) {
                        Ok(out) => {
                            let elapsed_ms = t0.elapsed().as_millis() as u64;
                            state = out.state;
                            merge_diag(&mut all_diag, &out.diagnostics);
                            let detail = out.data;
                            let ok = !detail.name.is_empty() || !detail.toc_url.is_empty();
                            let summary = if ok {
                                let author_part = if detail.author.is_empty() {
                                    String::new()
                                } else {
                                    format!(" / {}", detail.author)
                                };
                                format!("《{}》{}", detail.name, author_part)
                            } else {
                                "详情页未解析出书名与目录地址".into()
                            };
                            let err = if ok {
                                None
                            } else {
                                Some("ruleBookInfo 未能提取有效书名或目录 URL".into())
                            };
                            if next_toc_url.is_none() {
                                next_toc_url = Some(if detail.toc_url.is_empty() {
                                    b_url.clone()
                                } else {
                                    detail.toc_url.clone()
                                });
                            }
                            book_detail = Some(detail);
                            steps.push(SourceDebugStepDto {
                                stage: "book_info".into(),
                                label: "详情规则 (ruleBookInfo)".into(),
                                ok,
                                elapsed_ms,
                                summary,
                                error: err,
                                diagnostics: out.diagnostics,
                            });
                        }
                        Err(err) => {
                            let elapsed_ms = t0.elapsed().as_millis() as u64;
                            merge_diag(&mut all_diag, &err.diagnostics);
                            steps.push(SourceDebugStepDto {
                                stage: "book_info".into(),
                                label: "详情规则 (ruleBookInfo)".into(),
                                ok: false,
                                elapsed_ms,
                                summary: "详情请求或解析失败".into(),
                                error: Some(err.message),
                                diagnostics: err.diagnostics,
                            });
                        }
                    }
                }

                // Build BookJsContext for toc / content stages
                let book_ctx = if let Some(ref d) = book_detail {
                    BookJsContext {
                        name: d.name.clone(),
                        author: d.author.clone(),
                        book_url: d.book_url.clone(),
                        toc_url: d.toc_url.clone(),
                        cover_url: d.cover_url.clone(),
                        intro: d.intro.clone(),
                        dur_chapter_index: next_chapter_idx,
                        dur_chapter_title: next_chapter_title.clone(),
                        custom_variable: String::new(),
                    }
                } else if let Some(ref hint) = next_fallback_hint {
                    hint.clone()
                } else {
                    BookJsContext {
                        book_url: next_book_url.clone().unwrap_or_default(),
                        toc_url: next_toc_url.clone().unwrap_or_default(),
                        ..BookJsContext::default()
                    }
                };

                // Stage 3: ruleToc
                let run_toc = match mode {
                    SourceTestMode::Toc => {
                        if next_toc_url.is_none() {
                            next_toc_url = next_book_url.clone().or_else(|| raw_input.clone());
                        }
                        next_toc_url.is_some()
                    }
                    SourceTestMode::Full => {
                        next_toc_url.is_some()
                            && steps.last().is_none_or(|s| s.stage != "book_info" || s.ok)
                    }
                    _ => false,
                };

                let mut toc_succeeded_without_chapter_url = false;
                if run_toc && let Some(ref t_url) = next_toc_url {
                    let t0 = Instant::now();
                    match engine.fetch_toc_sync_with_control(
                        &parsed,
                        state.clone(),
                        t_url,
                        &book_ctx,
                        remaining_budget(),
                        cancel_worker.clone(),
                    ) {
                        Ok(out) => {
                            let elapsed_ms = t0.elapsed().as_millis() as u64;
                            state = out.state;
                            merge_diag(&mut all_diag, &out.diagnostics);
                            let full_toc = out.data;
                            toc_total_count = full_toc.len();
                            let ok = toc_total_count > 0;
                            let summary = if ok {
                                format!("解析到 {toc_total_count} 章")
                            } else {
                                "目录未解析到任何章节".into()
                            };
                            let err = if ok {
                                None
                            } else {
                                Some("ruleToc 未匹配到章节列表".into())
                            };

                            // Look up `chapter_index_override` from FULL TOC before truncating to 500
                            if next_chapter_url.is_none() {
                                let chosen = chapter_index_override
                                    .and_then(|idx| full_toc.iter().find(|c| c.index == idx))
                                    .or_else(|| {
                                        full_toc
                                            .iter()
                                            .find(|c| !c.is_volume && !c.url.trim().is_empty())
                                    })
                                    .or_else(|| full_toc.iter().find(|c| !c.url.trim().is_empty()));
                                if let Some(ch) = chosen {
                                    next_chapter_url = Some(ch.url.clone());
                                    next_chapter_title = ch.title.clone();
                                    next_chapter_idx = ch.index;
                                } else if ok {
                                    toc_succeeded_without_chapter_url = true;
                                }
                            }

                            toc_chapters = full_toc.into_iter().take(500).collect();

                            steps.push(SourceDebugStepDto {
                                stage: "toc".into(),
                                label: "目录规则 (ruleToc)".into(),
                                ok,
                                elapsed_ms,
                                summary,
                                error: err,
                                diagnostics: out.diagnostics,
                            });
                        }
                        Err(err) => {
                            let elapsed_ms = t0.elapsed().as_millis() as u64;
                            merge_diag(&mut all_diag, &err.diagnostics);
                            steps.push(SourceDebugStepDto {
                                stage: "toc".into(),
                                label: "目录规则 (ruleToc)".into(),
                                ok: false,
                                elapsed_ms,
                                summary: "目录请求或解析失败".into(),
                                error: Some(err.message),
                                diagnostics: err.diagnostics,
                            });
                        }
                    }
                }

                // Stage 4: ruleContent
                let run_content = match mode {
                    SourceTestMode::Content => {
                        if next_chapter_url.is_none() {
                            next_chapter_url = raw_input.clone();
                        }
                        next_chapter_url.is_some()
                    }
                    SourceTestMode::Full => {
                        next_chapter_url
                            .as_deref()
                            .is_some_and(|u| !u.trim().is_empty())
                            && steps.last().is_none_or(|s| s.stage != "toc" || s.ok)
                    }
                    _ => false,
                };

                if run_content && let Some(ref c_url) = next_chapter_url {
                    let t0 = Instant::now();
                    let ch_ctx = ChapterJsContext {
                        index: next_chapter_idx,
                        title: next_chapter_title.clone(),
                        url: c_url.clone(),
                    };
                    let mut content_book_ctx = book_ctx.clone();
                    content_book_ctx.dur_chapter_index = next_chapter_idx;
                    content_book_ctx.dur_chapter_title = next_chapter_title.clone();

                    match engine.fetch_chapter_content_sync_with_control(
                        &parsed,
                        state.clone(),
                        c_url,
                        &content_book_ctx,
                        &ch_ctx,
                        remaining_budget(),
                        cancel_worker.clone(),
                    ) {
                        Ok(out) => {
                            let elapsed_ms = t0.elapsed().as_millis() as u64;
                            state = out.state;
                            merge_diag(&mut all_diag, &out.diagnostics);
                            let char_count = out.data.content.chars().count();
                            let resolved_title = if !out.data.title.trim().is_empty() {
                                out.data.title.clone()
                            } else if !next_chapter_title.is_empty() {
                                next_chapter_title.clone()
                            } else {
                                format!("第 {} 章", next_chapter_idx + 1)
                            };
                            let ok = char_count > 0;
                            let summary = if ok {
                                format!("《{resolved_title}》解析正文 {char_count} 字")
                            } else {
                                format!("《{resolved_title}》正文内容为空")
                            };
                            let err = if ok {
                                None
                            } else {
                                Some("ruleContent 提取的正文内容为空".into())
                            };
                            content_chapter_index = Some(next_chapter_idx);
                            content_chapter_title = Some(resolved_title);
                            content_chapter_url = Some(c_url.clone());
                            content_text = Some(out.data.content);
                            content_char_count = char_count;
                            steps.push(SourceDebugStepDto {
                                stage: "content".into(),
                                label: "正文规则 (ruleContent)".into(),
                                ok,
                                elapsed_ms,
                                summary,
                                error: err,
                                diagnostics: out.diagnostics,
                            });
                        }
                        Err(err) => {
                            let elapsed_ms = t0.elapsed().as_millis() as u64;
                            merge_diag(&mut all_diag, &err.diagnostics);
                            content_chapter_index = Some(next_chapter_idx);
                            content_chapter_title = (!next_chapter_title.is_empty())
                                .then_some(next_chapter_title.clone());
                            content_chapter_url = Some(c_url.clone());
                            steps.push(SourceDebugStepDto {
                                stage: "content".into(),
                                label: "正文规则 (ruleContent)".into(),
                                ok: false,
                                elapsed_ms,
                                summary: "正文请求或解析失败".into(),
                                error: Some(err.message),
                                diagnostics: err.diagnostics,
                            });
                        }
                    }
                } else if mode == SourceTestMode::Full && toc_succeeded_without_chapter_url {
                    steps.push(SourceDebugStepDto {
                        stage: "content".into(),
                        label: "正文规则 (ruleContent)".into(),
                        ok: false,
                        elapsed_ms: 0,
                        summary: "目录未包含有效章节 URL".into(),
                        error: Some("目录仅返回无有效链接的卷名或空链接，无法继续抓取正文".into()),
                        diagnostics: ExecutionDiagnostics::default(),
                    });
                }

                let total_elapsed_ms = total_start.elapsed().as_millis() as u64;
                let overall_ok = !steps.is_empty() && steps.iter().all(|s| s.ok);
                let first_error = steps.iter().find_map(|s| s.error.clone());
                let sample_titles: Vec<String> = search_items
                    .iter()
                    .take(5)
                    .map(|b| b.name.clone())
                    .collect();

                let report = SourceTestReportDto {
                    ok: overall_ok,
                    mode,
                    keyword: kw_clone,
                    total_elapsed_ms,
                    result_count: search_total_count,
                    sample_titles,
                    error: first_error,
                    diagnostics: all_diag,
                    steps,
                    search_items,
                    book_detail,
                    toc_chapters,
                    toc_total_count,
                    content_chapter_index,
                    content_chapter_title,
                    content_chapter_url,
                    content_text,
                    content_char_count,
                };
                (state, report)
            }),
        )
        .await;

        let (final_state, report) = match timed_res {
            Ok(Ok(pair)) => pair,
            Ok(Err(e)) => {
                return Err(DomainError::Storage(format!("测试书源线程失败: {e}")));
            }
            Err(_) => {
                cancel_flag.store(true, Ordering::Relaxed);
                let timeout_msg = format!(
                    "书源调试总耗时超过 {} 秒，已强制中止",
                    SOURCE_TEST_TOTAL_TIMEOUT.as_secs()
                );
                let report = SourceTestReportDto {
                    ok: false,
                    mode,
                    keyword: effective_keyword,
                    total_elapsed_ms: SOURCE_TEST_TOTAL_TIMEOUT.as_millis() as u64,
                    result_count: 0,
                    sample_titles: Vec::new(),
                    error: Some(timeout_msg.clone()),
                    diagnostics: ExecutionDiagnostics::default(),
                    steps: vec![SourceDebugStepDto {
                        stage: "timeout".into(),
                        label: "总超时控制".into(),
                        ok: false,
                        elapsed_ms: SOURCE_TEST_TOTAL_TIMEOUT.as_millis() as u64,
                        summary: timeout_msg.clone(),
                        error: Some(timeout_msg),
                        diagnostics: ExecutionDiagnostics::default(),
                    }],
                    search_items: Vec::new(),
                    book_detail: None,
                    toc_chapters: Vec::new(),
                    toc_total_count: 0,
                    content_chapter_index: None,
                    content_chapter_title: None,
                    content_chapter_url: None,
                    content_text: None,
                    content_char_count: 0,
                };
                (rec.runtime_state, report)
            }
        };

        let now = Timestamp::now();
        let last_msg = if report.ok {
            match report.mode {
                SourceTestMode::Full => {
                    if report.result_count > 0 {
                        format!(
                            "全链路通过（搜索 {} 本 / 目录 {} 章 / 正文 {} 字）",
                            report.result_count, report.toc_total_count, report.content_char_count
                        )
                    } else {
                        format!(
                            "详情→目录({}章)→正文({}字) 通过",
                            report.toc_total_count, report.content_char_count
                        )
                    }
                }
                _ => report
                    .steps
                    .last()
                    .map(|s| s.summary.clone())
                    .unwrap_or_else(|| "测试通过".into()),
            }
        } else if let Some(failed) = report.steps.iter().find(|s| !s.ok) {
            let reason = failed
                .error
                .as_deref()
                .unwrap_or(&failed.summary)
                .chars()
                .take(120)
                .collect::<String>();
            format!("{} 失败：{reason}", failed.label)
        } else {
            "测试失败".into()
        };

        let _ = self
            .sources
            .update_test_result(id, final_state, report.ok, Some(last_msg), now)
            .await?;

        Ok(report)
    }

    pub async fn search(
        &self,
        actor: &User,
        keyword: &str,
        source_ids: Option<Vec<Uuid>>,
        page: u32,
    ) -> DomainResult<AggregatedSearchResponseDto> {
        let kw = keyword.trim();
        if kw.is_empty() {
            return Err(DomainError::Validation("请输入搜索关键词".into()));
        }

        let mut sources = self.sources.list_all().await?;
        let is_admin = actor.role.is_admin();

        if let Some(filter_ids) = source_ids.filter(|v| !v.is_empty()) {
            sources.retain(|s| filter_ids.contains(&s.id) && (is_admin || s.enabled));
        } else {
            sources.retain(|s| s.enabled);
        }

        if sources.is_empty() {
            return Err(DomainError::Validation("没有可用的已启用书源".into()));
        }

        sources.sort_by(|a, b| b.weight.cmp(&a.weight).then_with(|| a.name.cmp(&b.name)));
        let page_num = page.max(1);
        let sem = Arc::new(Semaphore::new(SEARCH_MAX_CONCURRENCY));

        let mut handles = Vec::with_capacity(sources.len());
        for rec in sources {
            let engine = self.engine.clone();
            let kw_owned = kw.to_string();
            let parsed_res = serde_json::from_str::<LegadoBookSource>(&rec.raw_json);
            let sid = rec.id;
            let sname = rec.name.clone();
            let state = rec.runtime_state.clone();
            let prev_state_cmp = state.clone();
            let sem_clone = Arc::clone(&sem);

            handles.push(tokio::spawn(async move {
                let parsed = match parsed_res {
                    Ok(p) => p,
                    Err(e) => {
                        return (
                            sid,
                            sname,
                            prev_state_cmp,
                            Err(format!("书源解析失败: {e}")),
                        );
                    }
                };
                let Ok(_permit) = sem_clone.acquire_owned().await else {
                    return (sid, sname, prev_state_cmp, Err("搜索并发调度已关闭".into()));
                };
                let cancel_flag = Arc::new(AtomicBool::new(false));
                let cancel_worker = cancel_flag.clone();
                let timed = tokio::time::timeout(
                    SEARCH_PER_SOURCE_TIMEOUT,
                    tokio::task::spawn_blocking(move || {
                        engine
                            .search_sync_with_control(
                                &parsed,
                                state,
                                &kw_owned,
                                page_num,
                                SEARCH_PER_SOURCE_TIMEOUT,
                                cancel_worker,
                            )
                            .map_err(Into::into)
                    }),
                )
                .await;
                match timed {
                    Ok(Ok(res)) => (sid, sname, prev_state_cmp, res),
                    Ok(Err(e)) => (
                        sid,
                        sname,
                        prev_state_cmp,
                        Err(format!("搜索任务异常: {e}")),
                    ),
                    Err(_) => {
                        cancel_flag.store(true, Ordering::Relaxed);
                        (
                            sid,
                            sname,
                            prev_state_cmp,
                            Err(format!(
                                "搜索超时（超过 {} 秒）",
                                SEARCH_PER_SOURCE_TIMEOUT.as_secs()
                            )),
                        )
                    }
                }
            }));
        }

        let mut items = Vec::new();
        let mut errors = Vec::new();
        let mut toasts = Vec::new();

        for h in handles {
            if let Ok((sid, sname, prev_state, res)) = h.await {
                match res {
                    Ok(out) => {
                        if out.state != prev_state
                            && let Err(err) = self.persist_runtime_state(sid, out.state).await
                        {
                            tracing::warn!(source_id = %sid, error = %err, "failed to persist source runtime state after search");
                        }
                        toasts.extend(out.diagnostics.toasts);
                        for book in out.data {
                            items.push(AggregatedSearchItemDto {
                                source_id: sid,
                                source_name: sname.clone(),
                                book,
                            });
                        }
                    }
                    Err(err) => {
                        errors.push(SourceSearchErrorDto {
                            source_id: sid,
                            source_name: sname,
                            error: err,
                        });
                    }
                }
            }
        }

        Ok(AggregatedSearchResponseDto {
            keyword: kw.to_string(),
            page: page_num,
            items,
            errors,
            toasts,
        })
    }

    async fn fetch_book_info_internal(
        &self,
        rec: BookSourceRecord,
        book_url: &str,
        fallback_hint: Option<BookJsContext>,
        include_logs: bool,
    ) -> DomainResult<OnlineBookDetailResponseDto> {
        let source_id = rec.id;
        let parsed: LegadoBookSource = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("书源 JSON 无效: {e}")))?;

        let engine = self.engine.clone();
        let url_owned = book_url.to_string();
        let prev_state = rec.runtime_state.clone();
        let prev_state_cmp = prev_state.clone();

        let mut out = tokio::task::spawn_blocking(move || {
            engine.fetch_book_info_sync(&parsed, prev_state, &url_owned, fallback_hint.as_ref())
        })
        .await
        .map_err(|e| DomainError::Storage(format!("获取书籍详情失败: {e}")))?
        .map_err(DomainError::Validation)?;

        if out.state != prev_state_cmp
            && let Err(err) = self.persist_runtime_state(source_id, out.state).await
        {
            tracing::warn!(source_id = %source_id, error = %err, "failed to persist runtime state after fetch_book_info");
        }
        if !include_logs {
            out.diagnostics.logs.clear();
        }

        Ok(OnlineBookDetailResponseDto {
            source_id,
            source_name: rec.name,
            detail: out.data,
            diagnostics: out.diagnostics,
        })
    }

    pub async fn fetch_book_info(
        &self,
        actor: &User,
        source_id: Uuid,
        book_url: &str,
        fallback_hint: Option<BookJsContext>,
    ) -> DomainResult<OnlineBookDetailResponseDto> {
        let rec = self.load_source_for_actor(actor, source_id).await?;
        self.fetch_book_info_internal(rec, book_url, fallback_hint, actor.role.is_admin())
            .await
    }

    async fn fetch_toc_internal(
        &self,
        rec: BookSourceRecord,
        toc_url: &str,
        book_ctx: BookJsContext,
        include_logs: bool,
    ) -> DomainResult<OnlineTocResponseDto> {
        let source_id = rec.id;
        let parsed: LegadoBookSource = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("书源 JSON 无效: {e}")))?;

        let engine = self.engine.clone();
        let url_owned = toc_url.to_string();
        let prev_state = rec.runtime_state.clone();
        let prev_state_cmp = prev_state.clone();

        let mut out = tokio::task::spawn_blocking(move || {
            engine.fetch_toc_sync(&parsed, prev_state, &url_owned, &book_ctx)
        })
        .await
        .map_err(|e| DomainError::Storage(format!("获取目录失败: {e}")))?
        .map_err(DomainError::Validation)?;

        if out.state != prev_state_cmp
            && let Err(err) = self.persist_runtime_state(source_id, out.state).await
        {
            tracing::warn!(source_id = %source_id, error = %err, "failed to persist runtime state after fetch_toc");
        }
        if !include_logs {
            out.diagnostics.logs.clear();
        }

        Ok(OnlineTocResponseDto {
            source_id,
            chapters: out.data,
            diagnostics: out.diagnostics,
        })
    }

    pub async fn fetch_toc(
        &self,
        actor: &User,
        source_id: Uuid,
        toc_url: &str,
        book_ctx: BookJsContext,
    ) -> DomainResult<OnlineTocResponseDto> {
        let rec = self.load_source_for_actor(actor, source_id).await?;
        self.fetch_toc_internal(rec, toc_url, book_ctx, actor.role.is_admin())
            .await
    }

    async fn fetch_chapter_content_internal(
        &self,
        rec: BookSourceRecord,
        chapter_url: &str,
        book_ctx: BookJsContext,
        chapter_ctx: ChapterJsContext,
        opts: ChapterFetchOptions,
    ) -> DomainResult<OnlineContentResponseDto> {
        let source_id = rec.id;
        if !opts.force_refresh {
            let cache_dir = self.cache_dir.clone();
            let ch_url = chapter_url.to_string();
            let cached = tokio::task::spawn_blocking(move || {
                read_cached_chapter_at(&cache_dir, source_id, &ch_url)
            })
            .await
            .ok()
            .flatten();
            if let Some(cached) = cached {
                return Ok(OnlineContentResponseDto {
                    source_id,
                    chapter_index: chapter_ctx.index,
                    content: cached,
                    cached: true,
                    diagnostics: ExecutionDiagnostics::default(),
                });
            }
        }

        let parsed: LegadoBookSource = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("书源 JSON 无效: {e}")))?;

        let engine = self.engine.clone();
        let url_owned = chapter_url.to_string();
        let prev_state = rec.runtime_state.clone();
        let prev_state_cmp = prev_state.clone();
        let ch_idx = chapter_ctx.index;

        let mut out = tokio::task::spawn_blocking(move || {
            engine.fetch_chapter_content_sync(
                &parsed,
                prev_state,
                &url_owned,
                &book_ctx,
                &chapter_ctx,
            )
        })
        .await
        .map_err(|e| DomainError::Storage(format!("获取章节正文失败: {e}")))?
        .map_err(DomainError::Validation)?;

        if out.state != prev_state_cmp
            && let Err(err) = self.persist_runtime_state(source_id, out.state).await
        {
            tracing::warn!(source_id = %source_id, error = %err, "failed to persist runtime state after fetch_chapter_content");
        }
        if opts.allow_shared_cache_write {
            let cache_dir = self.cache_dir.clone();
            let ch_url = chapter_url.to_string();
            let content_clone = out.data.clone();
            let _ = tokio::task::spawn_blocking(move || {
                write_cached_chapter_at(&cache_dir, source_id, &ch_url, &content_clone);
            })
            .await;
        }
        if !opts.include_logs {
            out.diagnostics.logs.clear();
        }

        Ok(OnlineContentResponseDto {
            source_id,
            chapter_index: ch_idx,
            content: out.data,
            cached: false,
            diagnostics: out.diagnostics,
        })
    }

    pub async fn fetch_chapter_content(
        &self,
        actor: &User,
        source_id: Uuid,
        chapter_url: &str,
        book_ctx: BookJsContext,
        chapter_ctx: ChapterJsContext,
        force_refresh: bool,
    ) -> DomainResult<OnlineContentResponseDto> {
        let rec = self.load_source_for_actor(actor, source_id).await?;
        self.fetch_chapter_content_internal(
            rec,
            chapter_url,
            book_ctx,
            chapter_ctx,
            ChapterFetchOptions {
                force_refresh,
                include_logs: actor.role.is_admin(),
                allow_shared_cache_write: actor.role.is_admin(),
            },
        )
        .await
    }

    // -----------------------------------------------------------------------
    // Per-User Online Bookshelf
    // -----------------------------------------------------------------------

    fn shelf_to_dto(r: OnlineShelfBook, source_name: String) -> OnlineShelfBookDto {
        OnlineShelfBookDto {
            id: r.id,
            source_id: r.source_id,
            source_name,
            name: r.name,
            author: r.author,
            book_url: r.book_url,
            toc_url: r.toc_url,
            cover_url: r.cover_url,
            intro: r.intro,
            kind: r.kind,
            last_chapter: r.last_chapter,
            total_chapters: r.total_chapters,
            current_chapter_index: r.current_chapter_index,
            current_chapter_title: r.current_chapter_title,
            scroll_ratio: r.scroll_ratio,
            updated_at: r.updated_at,
        }
    }

    pub async fn list_shelf(&self, user_id: UserId) -> DomainResult<Vec<OnlineShelfBookDto>> {
        let rows = self.shelves.list_by_user(user_id).await?;
        let sources = self.sources.list_all().await?;
        let src_names: HashMap<Uuid, String> =
            sources.into_iter().map(|s| (s.id, s.name)).collect();

        Ok(rows
            .into_iter()
            .map(|r| {
                let source_name = src_names
                    .get(&r.source_id)
                    .cloned()
                    .unwrap_or_else(|| "已删除书源".to_string());
                Self::shelf_to_dto(r, source_name)
            })
            .collect())
    }

    pub async fn add_to_shelf(
        &self,
        actor: &User,
        input: AddOnlineShelfInput,
    ) -> DomainResult<OnlineShelfBookDto> {
        let src = self.load_source_for_actor(actor, input.source_id).await?;
        let user_id = actor.id;
        let existing_list = self.shelves.list_by_user(user_id).await?;
        let now = Timestamp::now();

        if let Some(mut existing) = existing_list
            .into_iter()
            .find(|b| b.source_id == input.source_id && b.book_url == input.book_url)
        {
            existing.name = input.name;
            existing.author = input.author;
            if !input.toc_url.trim().is_empty() {
                existing.toc_url = input.toc_url;
            }
            if input.cover_url.is_some() {
                existing.cover_url = input.cover_url;
            }
            if input.intro.is_some() {
                existing.intro = input.intro;
            }
            if input.kind.is_some() {
                existing.kind = input.kind;
            }
            if input.last_chapter.is_some() {
                existing.last_chapter = input.last_chapter;
            }
            if let Some(total) = input.total_chapters {
                existing.total_chapters = total;
            }
            existing.updated_at = now;
            self.shelves.save(&existing).await?;
            return Ok(Self::shelf_to_dto(existing, src.name));
        }

        let item = OnlineShelfBook {
            id: Uuid::now_v7(),
            user_id,
            source_id: input.source_id,
            name: input.name,
            author: input.author,
            book_url: input.book_url,
            toc_url: input.toc_url,
            cover_url: input.cover_url,
            intro: input.intro,
            kind: input.kind,
            last_chapter: input.last_chapter,
            total_chapters: input.total_chapters.unwrap_or(0),
            current_chapter_index: 0,
            current_chapter_title: None,
            scroll_ratio: 0.0,
            created_at: now,
            updated_at: now,
        };
        self.shelves.save(&item).await?;
        Ok(Self::shelf_to_dto(item, src.name))
    }

    pub async fn update_shelf_book(
        &self,
        user_id: UserId,
        id: Uuid,
        input: UpdateOnlineShelfInput,
    ) -> DomainResult<OnlineShelfBookDto> {
        let mut rec = self
            .shelves
            .find_by_id(id)
            .await?
            .ok_or_else(|| DomainError::NotFound("未找到在线书架条目".into()))?;
        if rec.user_id != user_id {
            return Err(DomainError::Forbidden("无权操作他人在线书架".into()));
        }

        if let Some(source_id) = input.source_id {
            rec.source_id = source_id;
        }
        if let Some(book_url) = input.book_url {
            rec.book_url = book_url;
        }
        if let Some(toc_url) = input.toc_url {
            rec.toc_url = toc_url;
        }
        if let Some(idx) = input.current_chapter_index {
            rec.current_chapter_index = idx;
        }
        if input.current_chapter_title.is_some() {
            rec.current_chapter_title = input.current_chapter_title;
        }
        if let Some(scroll) = input.scroll_ratio {
            rec.scroll_ratio = scroll;
        }
        if let Some(total) = input.total_chapters {
            rec.total_chapters = total;
        }
        if input.last_chapter.is_some() {
            rec.last_chapter = input.last_chapter;
        }
        rec.updated_at = Timestamp::now();

        self.shelves.save(&rec).await?;

        let src_name = self
            .load_source(rec.source_id)
            .await
            .map(|s| s.name)
            .unwrap_or_else(|_| "未知书源".to_string());

        Ok(Self::shelf_to_dto(rec, src_name))
    }

    pub async fn remove_from_shelf(&self, user_id: UserId, id: Uuid) -> DomainResult<()> {
        let rec = self
            .shelves
            .find_by_id(id)
            .await?
            .ok_or_else(|| DomainError::NotFound("未找到在线书架条目".into()))?;
        if rec.user_id != user_id {
            return Err(DomainError::Forbidden("无权删除他人在线书架".into()));
        }
        self.shelves.delete(id).await
    }

    // -----------------------------------------------------------------------
    // Unified Online-Tracked Books (BookModel + Incremental Chapters + EPUB 3.0)
    // -----------------------------------------------------------------------

    fn book_state_lock_for(&self, book_id: BookId) -> Arc<tokio::sync::Mutex<()>> {
        let mut guard = self
            .book_state_locks
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        Arc::clone(
            guard
                .entry(book_id)
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(()))),
        )
    }

    fn epub_lock_for(&self, book_id: BookId) -> Arc<tokio::sync::Mutex<()>> {
        let mut guard = self
            .epub_sync_locks
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        Arc::clone(
            guard
                .entry(book_id)
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(()))),
        )
    }

    fn track_lock_for(&self, source_id: Uuid, book_url: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut guard = self.track_locks.lock().unwrap_or_else(|e| e.into_inner());
        Arc::clone(
            guard
                .entry((source_id, book_url.to_string()))
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(()))),
        )
    }

    fn bump_sync_gen(&self, book_id: BookId, delta_chapters: u32) -> (u64, bool) {
        let mut guard = self
            .pending_sync_gens
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let entry = guard.entry(book_id).or_insert((0, 0));
        entry.0 = entry.0.wrapping_add(1);
        entry.1 = entry.1.saturating_add(delta_chapters);
        let flush_now = entry.1 >= TRACKED_EPUB_IMMEDIATE_BATCH;
        if flush_now {
            entry.1 = 0;
        }
        (entry.0, flush_now)
    }

    fn current_sync_gen(&self, book_id: BookId) -> u64 {
        self.pending_sync_gens
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&book_id)
            .map(|(g, _)| *g)
            .unwrap_or(0)
    }

    fn parse_tags_from_kind(raw: Option<&str>) -> Vec<String> {
        raw.map(|s| {
            s.split([',', '，', '|', '/'])
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
    }

    /// Tracks an online book as a first-class unified `Book` in the managed library:
    /// creates (or returns existing) `BookModel` with `BookSource::OnlineTracked`,
    /// stores its cover, fetches initial TOC best-effort, and writes a valid initial EPUB 3.0.
    pub async fn track_online_book(
        self: &Arc<Self>,
        actor: &User,
        input: TrackOnlineBookInput,
    ) -> DomainResult<BookSummaryDto> {
        require_admin(actor)?;
        let src = self.load_source(input.source_id).await?;

        // Serialize concurrent track requests for the same (source_id, book_url) to prevent duplicate Books.
        let tlock = self.track_lock_for(input.source_id, &input.book_url);
        let _track_guard = tlock.lock().await;

        if let Some(existing) = self
            .catalog
            .find_online_tracked_book(input.source_id, &input.book_url)
            .await?
        {
            return self.catalog.get_book(Some(actor), existing.id).await;
        }

        let managed_lib = self.catalog.ensure_managed_library().await?;
        let book_id = BookId::new();

        // Best-effort cover download
        let cover_payload = if let Some(ref cover_url) = input.cover_url
            && !cover_url.trim().is_empty()
        {
            self.fetch_cover_proxy(cover_url)
                .await
                .ok()
                .map(|(bytes, mime_type)| CoverPayload {
                    bytes: Bytes::from(bytes),
                    mime_type,
                })
        } else {
            None
        };

        // Best-effort initial TOC fetch so manifest & chapter count are populated right away
        let mut toc_url = input.toc_url.trim().to_string();
        let mut last_chapter = input.last_chapter.clone();
        let mut total_chapters = 0u32;

        if toc_url.is_empty()
            && let Ok(info) = self
                .fetch_book_info_internal(
                    src.clone(),
                    &input.book_url,
                    Some(BookJsContext {
                        name: input.name.clone(),
                        author: input.author.clone(),
                        book_url: input.book_url.clone(),
                        toc_url: String::new(),
                        cover_url: input.cover_url.clone(),
                        intro: input.intro.clone(),
                        ..Default::default()
                    }),
                    false,
                )
                .await
        {
            toc_url = info.detail.toc_url;
            if last_chapter.is_none() {
                last_chapter = info.detail.last_chapter;
            }
        }

        if !toc_url.is_empty()
            && let Ok(toc_res) = self
                .fetch_toc_internal(
                    src,
                    &toc_url,
                    BookJsContext {
                        name: input.name.clone(),
                        author: input.author.clone(),
                        book_url: input.book_url.clone(),
                        toc_url: toc_url.clone(),
                        cover_url: input.cover_url.clone(),
                        intro: input.intro.clone(),
                        ..Default::default()
                    },
                    false,
                )
                .await
        {
            let clean_chapters: Vec<OnlineChapterItem> = toc_res
                .chapters
                .into_iter()
                .filter(|c| !c.is_volume && !c.url.trim().is_empty())
                .collect();
            if !clean_chapters.is_empty() {
                if let Some(last) = clean_chapters.last() {
                    last_chapter = Some(last.title.clone());
                }
                total_chapters = clean_chapters.len() as u32;
                let store = self.tracked_store.clone();
                if let Err(err) = tokio::task::spawn_blocking(move || {
                    store.update_manifest_from_toc(book_id, &clean_chapters, false)
                })
                .await
                .map_err(|e| e.to_string())
                .and_then(|r| r)
                {
                    tracing::warn!(book_id = %book_id, error = %err, "failed to save initial tracked manifest");
                }
            }
        }

        // Synthesize initial valid standard EPUB 3.0 archive (skeleton with cover + metadata)
        let pack_meta = PackBookMetadata {
            identifier: Some(book_id.0),
            title: input.name.clone(),
            author: input.author.clone(),
            description: input.intro.clone(),
            cover_bytes: cover_payload
                .as_ref()
                .map(|c| (c.bytes.to_vec(), c.mime_type.clone())),
        };
        let epub_bytes =
            tokio::task::spawn_blocking(move || package_chapters_to_epub(&pack_meta, &[]))
                .await
                .map_err(|e| DomainError::Storage(format!("生成初始 EPUB3 失败: {e}")))?
                .map_err(DomainError::Storage)?;

        let (stored, cover_key) = self
            .catalog
            .save_managed_epub_and_cover(book_id, Bytes::from(epub_bytes), cover_payload)
            .await?;

        let mut metadata = BookMetadata::new(input.name)?;
        if !input.author.trim().is_empty() {
            metadata.authors = vec![input.author.trim().to_string()];
        }
        metadata.description = BookMetadata::normalize_optional(input.intro);
        metadata.tags =
            BookMetadata::normalize_list(Self::parse_tags_from_kind(input.kind.as_deref()));
        metadata.language = Some("zh-CN".to_string());
        metadata.chapter_count = if total_chapters > 0 {
            Some(total_chapters)
        } else {
            None
        };

        let mut book = Book::new(
            managed_lib.id,
            BookFormat::Epub,
            BookSource::OnlineTracked {
                storage_key: stored.storage_key,
                source_id: input.source_id,
                book_url: input.book_url,
                toc_url,
                last_chapter,
                cached_chapters: 0,
                total_chapters,
                epub_dirty: false,
            },
            stored.file_size,
            stored.file_hash,
            cover_key,
            metadata,
        )?;
        book.id = book_id;

        self.catalog.save_book_raw(&book).await?;
        self.catalog.get_book(Some(actor), book_id).await
    }

    /// Returns the TOC for an online-tracked book (with per-chapter cache status).
    /// Fetches fresh TOC from the source when `force_refresh` is true or manifest is empty.
    pub async fn get_tracked_toc(
        self: &Arc<Self>,
        book_id: BookId,
        force_refresh: bool,
    ) -> DomainResult<TrackedTocResponseDto> {
        let book = self.catalog.find_book_raw(book_id).await?;
        let (
            source_id,
            book_url,
            toc_url,
            cached_chapters,
            total_chapters,
            last_chapter,
            epub_dirty,
        ) = match &book.source {
            BookSource::OnlineTracked {
                source_id,
                book_url,
                toc_url,
                cached_chapters,
                total_chapters,
                last_chapter,
                epub_dirty,
                ..
            } => (
                *source_id,
                book_url.clone(),
                toc_url.clone(),
                *cached_chapters,
                *total_chapters,
                last_chapter.clone(),
                *epub_dirty,
            ),
            _ => {
                return Err(DomainError::Validation("该书籍不是在线追更书籍".into()));
            }
        };

        let src_rec_opt = self.load_source(source_id).await.ok();
        let source_name = src_rec_opt
            .as_ref()
            .map(|s| s.name.clone())
            .unwrap_or_else(|| "已删除书源".to_string());

        let existing_manifest = {
            let store = self.tracked_store.clone();
            tokio::task::spawn_blocking(move || store.load_manifest(book_id))
                .await
                .map_err(|e| DomainError::Storage(format!("读取目录清单失败: {e}")))?
        };
        if !force_refresh && !existing_manifest.chapters.is_empty() {
            return Ok(TrackedTocResponseDto {
                book_id,
                source_id,
                source_name,
                chapters: existing_manifest.chapters,
                cached_chapters,
                total_chapters,
                last_chapter,
                epub_dirty,
            });
        }

        let src_rec =
            src_rec_opt.ok_or_else(|| DomainError::NotFound(format!("未找到书源 {source_id}")))?;

        let effective_toc_url = if toc_url.trim().is_empty() {
            let info = self
                .fetch_book_info_internal(
                    src_rec.clone(),
                    &book_url,
                    Some(BookJsContext {
                        name: book.metadata.title.clone(),
                        author: book.metadata.authors.join(", "),
                        book_url: book_url.clone(),
                        toc_url: String::new(),
                        intro: book.metadata.description.clone(),
                        ..Default::default()
                    }),
                    false,
                )
                .await?;
            info.detail.toc_url
        } else {
            toc_url.clone()
        };

        let toc_res = self
            .fetch_toc_internal(
                src_rec,
                &effective_toc_url,
                BookJsContext {
                    name: book.metadata.title.clone(),
                    author: book.metadata.authors.join(", "),
                    book_url: book_url.clone(),
                    toc_url: effective_toc_url.clone(),
                    intro: book.metadata.description.clone(),
                    ..Default::default()
                },
                false,
            )
            .await?;

        let clean_chapters: Vec<OnlineChapterItem> = toc_res
            .chapters
            .into_iter()
            .filter(|c| !c.is_volume && !c.url.trim().is_empty())
            .collect();

        // Serialize manifest + DB update under per-book state lock and re-read latest Book.
        let state_lock = self.book_state_lock_for(book_id);
        let _state_guard = state_lock.lock().await;
        let mut latest_book = self.catalog.find_book_raw(book_id).await?;
        let current_source_matches = matches!(
            &latest_book.source,
            BookSource::OnlineTracked { source_id: sid, .. } if *sid == source_id
        );
        if !current_source_matches {
            return Err(DomainError::Validation(
                "书籍来源已在拉取目录期间变更，请重新加载目录".into(),
            ));
        }

        let store = self.tracked_store.clone();
        let manifest = tokio::task::spawn_blocking(move || {
            store.update_manifest_from_toc(book_id, &clean_chapters, true)
        })
        .await
        .map_err(|e| DomainError::Storage(format!("更新目录清单线程失败: {e}")))?
        .map_err(DomainError::Storage)?;

        let new_total = manifest.chapters.len() as u32;
        let new_cached = manifest.chapters.iter().filter(|c| c.cached).count() as u32;
        let new_last = manifest.chapters.last().map(|c| c.title.clone());
        let mut latest_dirty = epub_dirty;

        if let BookSource::OnlineTracked {
            toc_url: ref mut t_url,
            last_chapter: ref mut l_ch,
            cached_chapters: ref mut c_cnt,
            total_chapters: ref mut t_cnt,
            epub_dirty: ref dirty,
            ..
        } = latest_book.source
        {
            *t_url = effective_toc_url;
            *l_ch = new_last.clone();
            *c_cnt = new_cached;
            *t_cnt = new_total;
            latest_dirty = *dirty;
        }
        latest_book.metadata.chapter_count = if new_total > 0 { Some(new_total) } else { None };
        latest_book.updated_at = Timestamp::now();
        self.catalog.save_book_raw(&latest_book).await?;

        Ok(TrackedTocResponseDto {
            book_id,
            source_id,
            source_name,
            chapters: manifest.chapters,
            cached_chapters: new_cached,
            total_chapters: new_total,
            last_chapter: new_last,
            epub_dirty: latest_dirty,
        })
    }

    /// Reads a single chapter of an online-tracked book:
    /// serves immediately from local chapter cache if present (with matching URL), otherwise
    /// fetches from the Legado source, saves O(1) chapter file atomically, marks `epub_dirty`,
    /// and schedules EPUB 3.0 sync.
    pub async fn get_tracked_chapter(
        self: &Arc<Self>,
        book_id: BookId,
        chapter_index: usize,
        force_refresh: bool,
    ) -> DomainResult<TrackedChapterResponseDto> {
        let book = self.catalog.find_book_raw(book_id).await?;
        let (source_id, book_url, toc_url, cached_chapters, total_chapters, epub_dirty) =
            match &book.source {
                BookSource::OnlineTracked {
                    source_id,
                    book_url,
                    toc_url,
                    cached_chapters,
                    total_chapters,
                    epub_dirty,
                    ..
                } => (
                    *source_id,
                    book_url.clone(),
                    toc_url.clone(),
                    *cached_chapters,
                    *total_chapters,
                    *epub_dirty,
                ),
                _ => {
                    return Err(DomainError::Validation("该书籍不是在线追更书籍".into()));
                }
            };

        let mut manifest = {
            let store = self.tracked_store.clone();
            tokio::task::spawn_blocking(move || store.load_manifest(book_id))
                .await
                .map_err(|e| DomainError::Storage(format!("读取目录清单失败: {e}")))?
        };

        if !force_refresh && let Some(meta) = manifest.chapters.get(chapter_index) {
            let expected_url = meta.url.clone();
            let store = self.tracked_store.clone();
            let cached_opt = tokio::task::spawn_blocking(move || {
                store.load_chapter_matching_url(book_id, chapter_index, &expected_url)
            })
            .await
            .ok()
            .flatten();
            if let Some(cached) = cached_opt {
                return Ok(TrackedChapterResponseDto {
                    book_id,
                    source_id,
                    chapter_index,
                    title: cached.title,
                    content: cached.content,
                    cached: true,
                    cached_chapters,
                    total_chapters,
                    epub_dirty,
                });
            }
        }

        if chapter_index >= manifest.chapters.len() {
            let refreshed = self.get_tracked_toc(book_id, true).await?;
            manifest.chapters = refreshed.chapters;
        }
        let chapter_meta = manifest
            .chapters
            .get(chapter_index)
            .cloned()
            .ok_or_else(|| {
                DomainError::NotFound(format!("未找到第 {} 章目录信息", chapter_index + 1))
            })?;
        let book_ctx = BookJsContext {
            name: book.metadata.title.clone(),
            author: book.metadata.authors.join(", "),
            book_url: book_url.clone(),
            toc_url: toc_url.clone(),
            intro: book.metadata.description.clone(),
            dur_chapter_index: chapter_index,
            dur_chapter_title: chapter_meta.title.clone(),
            ..Default::default()
        };
        let chapter_ctx = ChapterJsContext {
            title: chapter_meta.title.clone(),
            url: chapter_meta.url.clone(),
            index: chapter_index,
        };

        let src_rec = self.load_source(source_id).await?;
        let fetched = self
            .fetch_chapter_content_internal(
                src_rec,
                &chapter_meta.url,
                book_ctx,
                chapter_ctx,
                ChapterFetchOptions {
                    force_refresh,
                    include_logs: false,
                    allow_shared_cache_write: true,
                },
            )
            .await?;

        let resolved_title = if fetched.content.title.trim().is_empty() {
            chapter_meta.title.clone()
        } else {
            fetched.content.title.clone()
        };
        let chapter_file = TrackedChapterFile {
            index: chapter_index,
            title: resolved_title.clone(),
            url: chapter_meta.url.clone(),
            content: fetched.content.content.clone(),
        };

        // Coordinate manifest write + generation bump + latest Book DB update under `book_state_lock`.
        let (new_cached, new_total, sync_gen, should_flush_now) = {
            let state_lock = self.book_state_lock_for(book_id);
            let _state_guard = state_lock.lock().await;

            let mut latest_book = self.catalog.find_book_raw(book_id).await?;
            let source_still_matches = matches!(
                &latest_book.source,
                BookSource::OnlineTracked { source_id: sid, .. } if *sid == source_id
            );
            if !source_still_matches {
                return Err(DomainError::Validation(
                    "书籍来源已在章节抓取期间变更，请重新加载".into(),
                ));
            }

            let store = self.tracked_store.clone();
            let cf = chapter_file;
            let new_cached = tokio::task::spawn_blocking(move || store.save_chapter(book_id, &cf))
                .await
                .map_err(|e| DomainError::Storage(format!("写入章节缓存线程失败: {e}")))?
                .map_err(DomainError::Storage)?;
            let new_total = manifest.chapters.len() as u32;

            let (sync_gen, should_flush_now) = self.bump_sync_gen(book_id, 1);

            if let BookSource::OnlineTracked {
                cached_chapters: ref mut c_cnt,
                total_chapters: ref mut t_cnt,
                epub_dirty: ref mut dirty,
                ..
            } = latest_book.source
            {
                *c_cnt = new_cached;
                *t_cnt = new_total.max(*t_cnt);
                *dirty = true;
            }
            latest_book.updated_at = Timestamp::now();
            self.catalog.save_book_raw(&latest_book).await?;

            (new_cached, new_total, sync_gen, should_flush_now)
        };

        self.spawn_debounced_epub_sync(book_id, sync_gen, should_flush_now);

        Ok(TrackedChapterResponseDto {
            book_id,
            source_id,
            chapter_index,
            title: resolved_title,
            content: fetched.content.content,
            cached: false,
            cached_chapters: new_cached,
            total_chapters: new_total,
            epub_dirty: true,
        })
    }

    /// Switches the upstream book source for a unified online-tracked book while preserving
    /// its `BookId`, shelves, favorites, tags, and reading progress.
    /// Fetches the new source's TOC (and optional cover) BEFORE committing DB changes or clearing old cache.
    pub async fn switch_tracked_source(
        self: &Arc<Self>,
        actor: &User,
        book_id: BookId,
        input: SwitchTrackedSourceInput,
    ) -> DomainResult<TrackedTocResponseDto> {
        require_admin(actor)?;
        let new_src = self.load_source(input.source_id).await?;
        let book_snapshot = self.catalog.find_book_raw(book_id).await?;

        if !book_snapshot.source.is_online_tracked() {
            return Err(DomainError::Validation("该书籍不是在线追更书籍".into()));
        }

        // 1. Pre-fetch new TOC (and book_info if toc_url is empty) BEFORE touching DB or old cache
        let effective_toc_url = if input.toc_url.trim().is_empty() {
            let info = self
                .fetch_book_info_internal(
                    new_src.clone(),
                    &input.book_url,
                    Some(BookJsContext {
                        name: book_snapshot.metadata.title.clone(),
                        author: book_snapshot.metadata.authors.join(", "),
                        book_url: input.book_url.clone(),
                        toc_url: String::new(),
                        cover_url: input.cover_url.clone(),
                        intro: input
                            .intro
                            .clone()
                            .or_else(|| book_snapshot.metadata.description.clone()),
                        ..Default::default()
                    }),
                    false,
                )
                .await?;
            info.detail.toc_url
        } else {
            input.toc_url.trim().to_string()
        };

        let toc_res = self
            .fetch_toc_internal(
                new_src.clone(),
                &effective_toc_url,
                BookJsContext {
                    name: book_snapshot.metadata.title.clone(),
                    author: book_snapshot.metadata.authors.join(", "),
                    book_url: input.book_url.clone(),
                    toc_url: effective_toc_url.clone(),
                    cover_url: input.cover_url.clone(),
                    intro: input
                        .intro
                        .clone()
                        .or_else(|| book_snapshot.metadata.description.clone()),
                    ..Default::default()
                },
                false,
            )
            .await?;

        let clean_chapters: Vec<OnlineChapterItem> = toc_res
            .chapters
            .into_iter()
            .filter(|c| !c.is_volume && !c.url.trim().is_empty())
            .collect();
        if clean_chapters.is_empty() {
            return Err(DomainError::Validation(
                "新书源未解析到有效章节目录，已取消换源以保留原书数据".into(),
            ));
        }

        // Best-effort download & save new cover if `input.cover_url` was provided
        let new_cover_key = if let Some(ref cover_url) = input.cover_url
            && !cover_url.trim().is_empty()
        {
            if let Ok((bytes, mime_type)) = self.fetch_cover_proxy(cover_url).await {
                self.catalog
                    .save_cover_raw(
                        book_id,
                        &CoverPayload {
                            bytes: Bytes::from(bytes),
                            mime_type,
                        },
                    )
                    .await
                    .ok()
            } else {
                None
            }
        } else {
            None
        };

        // 2. Commit cache update + DB update atomically under `book_state_lock`
        let (manifest, new_cached, new_total, new_last, sync_gen) = {
            let state_lock = self.book_state_lock_for(book_id);
            let _state_guard = state_lock.lock().await;

            let mut latest_book = self.catalog.find_book_raw(book_id).await?;
            if !latest_book.source.is_online_tracked() {
                return Err(DomainError::Validation("该书籍不是在线追更书籍".into()));
            }

            let store = self.tracked_store.clone();
            let clear_cache = input.clear_cache;
            let manifest = tokio::task::spawn_blocking(move || {
                if clear_cache {
                    store.clear_chapters(book_id);
                }
                store.update_manifest_from_toc(book_id, &clean_chapters, !clear_cache)
            })
            .await
            .map_err(|e| DomainError::Storage(format!("切换书源更新清单线程失败: {e}")))?
            .map_err(DomainError::Storage)?;

            let new_total = manifest.chapters.len() as u32;
            let new_cached = manifest.chapters.iter().filter(|c| c.cached).count() as u32;
            let new_last = manifest
                .chapters
                .last()
                .map(|c| c.title.clone())
                .or(input.last_chapter);

            if let BookSource::OnlineTracked {
                source_id,
                book_url,
                toc_url,
                last_chapter,
                cached_chapters,
                total_chapters,
                epub_dirty,
                ..
            } = &mut latest_book.source
            {
                *source_id = input.source_id;
                *book_url = input.book_url;
                *toc_url = effective_toc_url;
                *last_chapter = new_last.clone();
                *cached_chapters = new_cached;
                *total_chapters = new_total;
                *epub_dirty = true;
            }
            if let Some(intro) = input.intro
                && !intro.trim().is_empty()
            {
                latest_book.metadata.description = Some(intro.trim().to_string());
            }
            if let Some(ck) = new_cover_key {
                latest_book.set_cover(Some(ck));
            }
            latest_book.metadata.chapter_count = if new_total > 0 { Some(new_total) } else { None };
            latest_book.updated_at = Timestamp::now();

            // Bump generation so any in-flight `sync_tracked_epub` will NOT clear `epub_dirty`
            let (sync_gen, _) = self.bump_sync_gen(book_id, 1);
            self.catalog.save_book_raw(&latest_book).await?;

            (manifest, new_cached, new_total, new_last, sync_gen)
        };

        self.spawn_debounced_epub_sync(book_id, sync_gen, false);

        Ok(TrackedTocResponseDto {
            book_id,
            source_id: input.source_id,
            source_name: new_src.name,
            chapters: manifest.chapters,
            cached_chapters: new_cached,
            total_chapters: new_total,
            last_chapter: new_last,
            epub_dirty: true,
        })
    }

    /// Spawns a debounced background standard EPUB 3.0 rebuild task for `sync_gen`.
    fn spawn_debounced_epub_sync(
        self: &Arc<Self>,
        book_id: BookId,
        sync_gen: u64,
        should_flush_now: bool,
    ) {
        let svc = Arc::clone(self);
        tokio::spawn(async move {
            if !should_flush_now {
                tokio::time::sleep(TRACKED_EPUB_DEBOUNCE).await;
                if svc.current_sync_gen(book_id) != sync_gen {
                    return;
                }
            }
            if let Err(err) = svc.ensure_tracked_epub_synced(book_id).await {
                tracing::warn!(book_id = %book_id, error = %err, "background EPUB 3.0 sync failed");
            }
        });
    }

    /// Ensures the standard EPUB 3.0 archive on disk reflects all cached chapters if `epub_dirty`.
    pub async fn ensure_tracked_epub_synced(self: &Arc<Self>, book_id: BookId) -> DomainResult<()> {
        let book = self.catalog.find_book_raw(book_id).await?;
        let is_dirty = matches!(
            &book.source,
            BookSource::OnlineTracked {
                epub_dirty: true,
                ..
            }
        );
        if !is_dirty {
            return Ok(());
        }
        let _ = self.sync_tracked_epub(book_id, false).await?;
        Ok(())
    }

    /// Atomically synthesizes the standard EPUB 3.0 file for an online-tracked book from its
    /// locally cached chapters + cover + metadata, updating `BookModel` in-place.
    /// Uses generation tracking (`start_gen` vs `current_gen`) and re-reads the latest `Book`
    /// under `book_state_lock` so concurrent chapter fetches or source switches are never lost.
    pub async fn sync_tracked_epub(
        self: &Arc<Self>,
        book_id: BookId,
        force: bool,
    ) -> DomainResult<BookSummaryDto> {
        let epub_lock = self.epub_lock_for(book_id);
        let _epub_guard = epub_lock.lock().await;

        let (start_gen, title, author, description, cover_key) = {
            let state_lock = self.book_state_lock_for(book_id);
            let _state_guard = state_lock.lock().await;

            let book = self.catalog.find_book_raw(book_id).await?;
            let is_dirty = match &book.source {
                BookSource::OnlineTracked { epub_dirty, .. } => *epub_dirty,
                _ => {
                    return Err(DomainError::Validation("该书籍不是在线追更书籍".into()));
                }
            };
            if !force && !is_dirty {
                return self.catalog.get_book(None, book_id).await;
            }
            (
                self.current_sync_gen(book_id),
                book.metadata.title.clone(),
                book.metadata.authors.join(", "),
                book.metadata.description.clone(),
                book.cover_key.clone(),
            )
        };

        let store = self.tracked_store.clone();
        let (chapters, cached_count, total_count) =
            tokio::task::spawn_blocking(move || store.collect_cached_chapters_for_epub(book_id))
                .await
                .map_err(|e| DomainError::Storage(format!("收集章节缓存线程失败: {e}")))?;

        let cover_bytes = match cover_key.as_deref() {
            Some(key) => self.catalog.read_cover_bytes(key).await,
            None => None,
        };

        let pack_meta = PackBookMetadata {
            identifier: Some(book_id.0),
            title,
            author,
            description,
            cover_bytes,
        };
        let epub_bytes =
            tokio::task::spawn_blocking(move || package_chapters_to_epub(&pack_meta, &chapters))
                .await
                .map_err(|e| DomainError::Storage(format!("合成 EPUB3 线程失败: {e}")))?
                .map_err(DomainError::Storage)?;

        let (stored, _) = self
            .catalog
            .save_managed_epub_and_cover(book_id, Bytes::from(epub_bytes), None)
            .await?;

        let reschedule_gen = {
            let state_lock = self.book_state_lock_for(book_id);
            let _state_guard = state_lock.lock().await;

            let mut latest_book = self.catalog.find_book_raw(book_id).await?;
            let current_gen = self.current_sync_gen(book_id);
            let is_clean = current_gen == start_gen;

            latest_book.file_size = stored.file_size;
            latest_book.file_hash = stored.file_hash;
            if let BookSource::OnlineTracked {
                storage_key,
                cached_chapters,
                total_chapters,
                epub_dirty,
                ..
            } = &mut latest_book.source
            {
                *storage_key = stored.storage_key;
                if is_clean {
                    *cached_chapters = cached_count;
                    if total_count > 0 {
                        *total_chapters = total_count;
                    }
                    *epub_dirty = false;
                } else {
                    *epub_dirty = true;
                }
            }
            if is_clean {
                latest_book.metadata.chapter_count = if total_count > 0 {
                    Some(total_count)
                } else if cached_count > 0 {
                    Some(cached_count)
                } else {
                    latest_book.metadata.chapter_count
                };
                if let Some(entry) = self
                    .pending_sync_gens
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get_mut(&book_id)
                {
                    entry.1 = 0;
                }
            }
            latest_book.updated_at = Timestamp::now();
            self.catalog.save_book_raw(&latest_book).await?;

            if is_clean { None } else { Some(current_gen) }
        };

        drop(_epub_guard);
        if let Some(next_gen) = reschedule_gen {
            self.spawn_debounced_epub_sync(book_id, next_gen, false);
        }

        self.catalog.get_book(None, book_id).await
    }

    // -----------------------------------------------------------------------
    // Background Download + EPUB 3.0 / TXT Packaging + Optional Library Import
    // -----------------------------------------------------------------------

    pub async fn start_download(
        self: &Arc<Self>,
        actor: &User,
        input: StartDownloadInput,
    ) -> DomainResult<OnlineDownloadJobDto> {
        if input.import_to_library || input.book_id.is_some() {
            require_admin(actor)?;
        }
        let src = self.load_source_for_actor(actor, input.source_id).await?;

        if let Some(bid) = input.book_id {
            let book = self.catalog.find_book_raw(BookId(bid)).await?;
            match &book.source {
                BookSource::OnlineTracked {
                    source_id, toc_url, ..
                } => {
                    if *source_id != input.source_id {
                        return Err(DomainError::Validation(
                            "下载任务的书源与目标追更书籍不一致".into(),
                        ));
                    }
                    if !toc_url.trim().is_empty() && input.toc_url.trim() != toc_url.trim() {
                        return Err(DomainError::Validation(
                            "下载任务的目录地址与目标追更书籍不一致".into(),
                        ));
                    }
                }
                _ => {
                    return Err(DomainError::Validation(
                        "目标书籍不是在线追更书籍，无法关联写入缓存".into(),
                    ));
                }
            }
        }

        // Evict stale finished jobs and enforce per-user active download quota
        let now = Timestamp::now();
        let mut stale_dirs = Vec::new();
        {
            let mut guard = self.downloads.write().await;
            let stale_ids: Vec<Uuid> = guard
                .iter()
                .filter(|(_, job)| {
                    matches!(
                        job.dto.status,
                        DownloadJobStatus::Completed
                            | DownloadJobStatus::Failed
                            | DownloadJobStatus::Cancelled
                    ) && now
                        .as_second()
                        .saturating_sub(job.dto.updated_at.as_second())
                        > FINISHED_DOWNLOAD_TTL_SECS
                })
                .map(|(id, _)| *id)
                .collect();
            for sid in stale_ids {
                guard.remove(&sid);
                stale_dirs.push(self.download_dir.join(sid.to_string()));
            }

            let active_count = guard
                .values()
                .filter(|j| {
                    j.dto.user_id == actor.id.0
                        && matches!(
                            j.dto.status,
                            DownloadJobStatus::Pending | DownloadJobStatus::Running
                        )
                })
                .count();
            if active_count >= MAX_ACTIVE_DOWNLOADS_PER_USER {
                return Err(DomainError::Validation(format!(
                    "同时进行的下载任务数已达上限 ({MAX_ACTIVE_DOWNLOADS_PER_USER})，请等待或取消部分任务"
                )));
            }
        }
        for dir in stale_dirs {
            let _ = tokio::fs::remove_dir_all(&dir).await;
        }

        let job_id = Uuid::now_v7();

        let dto = OnlineDownloadJobDto {
            id: job_id,
            user_id: actor.id.0,
            source_id: input.source_id,
            source_name: src.name,
            book_name: input.book_name.clone(),
            author: input.author.clone(),
            format: input.format,
            status: DownloadJobStatus::Pending,
            total_chapters: 0,
            processed_chapters: 0,
            failed_chapters: 0,
            chapters_per_sec: 0.0,
            elapsed_ms: 0,
            current_chapter_title: None,
            output_file_name: None,
            file_size: None,
            import_to_library: input.import_to_library,
            imported_book_id: None,
            error: None,
            created_at: now,
            updated_at: now,
        };

        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_cond = Arc::new((Mutex::new(false), Condvar::new()));
        let cancel_notify = Arc::new(Notify::new());

        {
            let mut guard = self.downloads.write().await;
            guard.insert(
                job_id,
                StoredDownloadJob {
                    dto: dto.clone(),
                    output_path: None,
                    started_at: None,
                    cancel_flag: cancel_flag.clone(),
                    cancel_cond: cancel_cond.clone(),
                    cancel_notify: cancel_notify.clone(),
                },
            );
        }

        let svc = Arc::clone(self);
        let actor_clone = actor.clone();
        tokio::spawn(async move {
            svc.execute_download_job(
                job_id,
                actor_clone,
                input,
                cancel_flag,
                cancel_cond,
                cancel_notify,
            )
            .await;
        });

        Ok(dto)
    }

    async fn execute_download_job(
        self: Arc<Self>,
        job_id: Uuid,
        actor: User,
        input: StartDownloadInput,
        cancel_flag: Arc<AtomicBool>,
        cancel_cond: Arc<(Mutex<bool>, Condvar)>,
        cancel_notify: Arc<Notify>,
    ) {
        let started_at = Instant::now();
        {
            let mut guard = self.downloads.write().await;
            let Some(slot) = guard.get_mut(&job_id) else {
                return;
            };
            if cancel_flag.load(Ordering::Acquire)
                || slot.dto.status == DownloadJobStatus::Cancelled
            {
                slot.dto.status = DownloadJobStatus::Cancelled;
                slot.dto.current_chapter_title = None;
                slot.dto.updated_at = Timestamp::now();
                return;
            }
            slot.started_at = Some(started_at);
            slot.dto.status = DownloadJobStatus::Running;
            slot.dto.current_chapter_title = Some("正在拉取章节目录…".into());
            slot.dto.updated_at = Timestamp::now();
        }

        // Load & parse book source ONCE for the entire download job
        let src_rec = match self.load_source_for_actor(&actor, input.source_id).await {
            Ok(r) => r,
            Err(e) => {
                self.fail_job(job_id, started_at, format!("加载书源失败: {e}"))
                    .await;
                return;
            }
        };
        let parsed_source = match serde_json::from_str::<LegadoBookSource>(&src_rec.raw_json) {
            Ok(p) => Arc::new(p),
            Err(e) => {
                self.fail_job(job_id, started_at, format!("书源 JSON 无效: {e}"))
                    .await;
                return;
            }
        };
        let mut last_persisted_state = src_rec.runtime_state.clone();
        let reusable_ctx = match self.engine.prepare_reusable_context_with_cancel(
            Arc::clone(&parsed_source),
            src_rec.runtime_state,
            cancel_flag.clone(),
        ) {
            Ok(c) => c,
            Err(e) => {
                self.fail_job(job_id, started_at, format!("初始化书源会话失败: {e}"))
                    .await;
                return;
            }
        };

        let book_ctx = BookJsContext {
            name: input.book_name.clone(),
            author: input.author.clone(),
            book_url: input.book_url.clone(),
            toc_url: input.toc_url.clone(),
            cover_url: input.cover_url.clone(),
            intro: input.intro.clone(),
            ..Default::default()
        };

        // Pin cancel_notify BEFORE checking cancel_flag so we never miss a wakeup.
        let toc_cancelled = cancel_notify.notified();
        tokio::pin!(toc_cancelled);
        if cancel_flag.load(Ordering::Acquire) {
            self.mark_job_cancelled(job_id, started_at).await;
            return;
        }

        let toc_task = {
            let ctx = reusable_ctx.clone();
            let toc_url = input.toc_url.clone();
            let bctx = book_ctx.clone();
            tokio::task::spawn_blocking(move || ctx.fetch_toc_sync(&toc_url, &bctx))
        };

        let toc_res = tokio::select! {
            biased;
            _ = &mut toc_cancelled => {
                self.mark_job_cancelled(job_id, started_at).await;
                return;
            }
            res = toc_task => match res {
                Ok(inner) => inner,
                Err(e) => Err(format!("获取目录线程失败: {e}")),
            },
        };

        if cancel_flag.load(Ordering::Acquire) {
            self.mark_job_cancelled(job_id, started_at).await;
            return;
        }

        let all_chapters = match toc_res {
            Ok(list) => list
                .into_iter()
                .filter(|c| !c.is_volume && !c.url.trim().is_empty())
                .collect::<Vec<_>>(),
            Err(e) => {
                self.fail_job(job_id, started_at, format!("获取目录失败: {e}"))
                    .await;
                return;
            }
        };

        if all_chapters.is_empty() {
            self.fail_job(job_id, started_at, "目录为空，无可下载章节".into())
                .await;
            return;
        }

        let start = input.start_chapter.unwrap_or(0).min(all_chapters.len());
        let end = input
            .end_chapter
            .map(|e| e.saturating_add(1).min(all_chapters.len()))
            .unwrap_or(all_chapters.len())
            .max(start);
        let selected: Vec<OnlineChapterItem> = all_chapters[start..end].to_vec();

        // Only admin imports may write to shared `OnlineTracked` cache, and source/toc must match.
        let target_tracked_book_id: Option<BookId> =
            if input.import_to_library && actor.role.is_admin() {
                if let Some(bid) = input.book_id {
                    self.catalog
                        .find_book_raw(BookId(bid))
                        .await
                        .ok()
                        .filter(|b| {
                            matches!(
                                &b.source,
                                BookSource::OnlineTracked {
                                    source_id,
                                    toc_url,
                                    ..
                                } if *source_id == input.source_id
                                    && (toc_url.trim().is_empty()
                                        || toc_url.trim() == input.toc_url.trim())
                            )
                        })
                        .map(|b| b.id)
                } else if input.format == OnlineDownloadFormat::Epub {
                    self.catalog
                        .find_online_tracked_book(input.source_id, &input.book_url)
                        .await
                        .ok()
                        .flatten()
                        .map(|b| b.id)
                } else {
                    None
                }
            } else {
                None
            };

        if let Some(bid) = target_tracked_book_id {
            let store = self.tracked_store.clone();
            let toc_for_manifest = all_chapters.clone();
            if let Err(err) = tokio::task::spawn_blocking(move || {
                store.update_manifest_from_toc(bid, &toc_for_manifest, true)
            })
            .await
            .map_err(|e| e.to_string())
            .and_then(|r| r)
            {
                tracing::warn!(book_id = %bid, error = %err, "failed to update tracked manifest before download");
            }
        }
        if selected.is_empty() {
            self.fail_job(job_id, started_at, "所选章节区间为空".into())
                .await;
            return;
        }

        let total_selected = selected.len();
        self.update_job(job_id, |j| {
            j.total_chapters = total_selected;
            j.elapsed_ms = started_at.elapsed().as_millis() as u64;
        })
        .await;

        let (concurrency, rate_limiter) =
            DownloadRateLimiter::from_spec(parsed_source.parse_concurrent_rate());
        let rate_limiter = Arc::new(rate_limiter);
        let selected_arc = Arc::new(selected);
        let cursor = Arc::new(AtomicUsize::new(0));
        let worker_count = concurrency.min(total_selected).max(1);
        let (tx, mut rx) = mpsc::unbounded_channel::<ChapterProgressEvent>();

        let mut worker_handles = Vec::with_capacity(worker_count);
        for _ in 0..worker_count {
            let selected_w = Arc::clone(&selected_arc);
            let cursor_w = Arc::clone(&cursor);
            let ctx_w = reusable_ctx.clone();
            let limiter_w = Arc::clone(&rate_limiter);
            let cancel_flag_w = Arc::clone(&cancel_flag);
            let cancel_cond_w = Arc::clone(&cancel_cond);
            let cache_dir_w = self.cache_dir.clone();
            let source_id = input.source_id;
            let book_ctx_w = book_ctx.clone();
            let tx_w = tx.clone();

            worker_handles.push(tokio::task::spawn_blocking(move || {
                let mut session = ctx_w.create_worker_session();
                loop {
                    if cancel_flag_w.load(Ordering::Acquire) {
                        break;
                    }
                    let idx = cursor_w.fetch_add(1, Ordering::Relaxed);
                    if idx >= selected_w.len() {
                        break;
                    }
                    let ch = &selected_w[idx];

                    // Fast path: cached chapters are read directly without consuming rate-limit quota
                    if let Some(cached) = read_cached_chapter_at(&cache_dir_w, source_id, &ch.url)
                        && !cached.content.trim().is_empty()
                    {
                        let packed = PackChapterInput {
                            title: if cached.title.trim().is_empty() {
                                ch.title.clone()
                            } else {
                                cached.title
                            },
                            content: cached.content,
                        };
                        if tx_w
                            .send(ChapterProgressEvent {
                                idx,
                                chapter_title: ch.title.clone(),
                                packed,
                                failed: false,
                                from_cache: true,
                            })
                            .is_err()
                        {
                            break;
                        }
                        continue;
                    }

                    if !limiter_w.wait_for_slot(&cancel_flag_w, &cancel_cond_w) {
                        break;
                    }

                    session.reset_chapter_budget();
                    let mut cur_book_ctx = book_ctx_w.clone();
                    cur_book_ctx.dur_chapter_index = ch.index;
                    cur_book_ctx.dur_chapter_title = ch.title.clone();
                    let ch_ctx = ChapterJsContext {
                        index: ch.index,
                        title: ch.title.clone(),
                        url: ch.url.clone(),
                    };

                    let (packed, failed) =
                        match session.fetch_chapter_content(&ch.url, &cur_book_ctx, &ch_ctx) {
                            Ok(content) if !content.content.trim().is_empty() => {
                                write_cached_chapter_at(&cache_dir_w, source_id, &ch.url, &content);
                                let title = if content.title.trim().is_empty() {
                                    ch.title.clone()
                                } else {
                                    content.title
                                };
                                (
                                    PackChapterInput {
                                        title,
                                        content: content.content,
                                    },
                                    false,
                                )
                            }
                            _ => (
                                PackChapterInput {
                                    title: ch.title.clone(),
                                    content: "[本章抓取失败，请稍后重试]".into(),
                                },
                                true,
                            ),
                        };

                    if tx_w
                        .send(ChapterProgressEvent {
                            idx,
                            chapter_title: ch.title.clone(),
                            packed,
                            failed,
                            from_cache: false,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            }));
        }
        drop(tx);

        let mut ordered_chapters: Vec<Option<PackChapterInput>> = vec![None; total_selected];
        let mut processed_count = 0usize;
        let mut failed_count = 0usize;
        let mut uncached_since_persist = 0usize;
        let mut was_cancelled = false;

        // Pin cancel_notify BEFORE checking cancel_flag so a signal between check and select is never lost.
        let loop_cancelled = cancel_notify.notified();
        tokio::pin!(loop_cancelled);

        loop {
            if cancel_flag.load(Ordering::Acquire) {
                was_cancelled = true;
                break;
            }
            tokio::select! {
                biased;
                _ = &mut loop_cancelled => {
                    was_cancelled = true;
                    break;
                }
                maybe_ev = rx.recv() => {
                    let Some(ev) = maybe_ev else {
                        break;
                    };
                    if let Some(slot) = ordered_chapters.get_mut(ev.idx) {
                        if !ev.failed && let Some(bid) = target_tracked_book_id {
                            let global_idx = start + ev.idx;
                            let chapter_url = selected_arc
                                .get(ev.idx)
                                .map(|c| c.url.clone())
                                .unwrap_or_default();
                            let store = self.tracked_store.clone();
                            let chapter_file = TrackedChapterFile {
                                index: global_idx,
                                title: ev.packed.title.clone(),
                                url: chapter_url,
                                content: ev.packed.content.clone(),
                            };
                            if let Err(err) = tokio::task::spawn_blocking(move || {
                                store.save_chapter(bid, &chapter_file)
                            })
                            .await
                            .map_err(|e| e.to_string())
                            .and_then(|r| r)
                            {
                                tracing::warn!(book_id = %bid, index = global_idx, error = %err, "failed to save downloaded chapter to tracked store");
                            } else {
                                self.bump_sync_gen(bid, 1);
                            }
                        }
                        *slot = Some(ev.packed);
                    }
                    processed_count += 1;
                    if ev.failed {
                        failed_count += 1;
                    }
                    if !ev.from_cache {
                        uncached_since_persist += 1;
                    }

                    let elapsed_ms = started_at.elapsed().as_millis() as u64;
                    let cps = compute_chapters_per_sec(processed_count, elapsed_ms);
                    self.update_job(job_id, |j| {
                        j.processed_chapters = processed_count;
                        j.failed_chapters = failed_count;
                        j.current_chapter_title = Some(ev.chapter_title);
                        j.elapsed_ms = elapsed_ms;
                        j.chapters_per_sec = cps;
                    })
                    .await;

                    if uncached_since_persist >= DOWNLOAD_STATE_PERSIST_BATCH {
                        uncached_since_persist = 0;
                        let snap = reusable_ctx.snapshot_state();
                        if snap != last_persisted_state
                            && self.persist_runtime_state(input.source_id, snap.clone()).await.is_ok()
                        {
                            last_persisted_state = snap;
                        }
                    }
                }
            }
        }

        // Always join all worker threads before cleaning up directories or persisting final state
        for h in worker_handles {
            let _ = h.await;
        }

        // Persist any final runtime_state changes once at end of chapter fetching
        let final_snap = reusable_ctx.snapshot_state();
        if final_snap != last_persisted_state
            && let Err(err) = self
                .persist_runtime_state(input.source_id, final_snap)
                .await
        {
            tracing::warn!(source_id = %input.source_id, error = %err, "failed to persist final download runtime state");
        }

        if was_cancelled || cancel_flag.load(Ordering::Acquire) {
            self.mark_job_cancelled(job_id, started_at).await;
            let job_dir = self.download_dir.join(job_id.to_string());
            let _ = tokio::fs::remove_dir_all(&job_dir).await;
            return;
        }

        let packed_chapters: Vec<PackChapterInput> = ordered_chapters
            .into_iter()
            .enumerate()
            .map(|(idx, opt)| {
                opt.unwrap_or_else(|| PackChapterInput {
                    title: selected_arc[idx].title.clone(),
                    content: "[本章抓取失败，请稍后重试]".into(),
                })
            })
            .collect();

        let ext = match input.format {
            OnlineDownloadFormat::Epub => "epub",
            OnlineDownloadFormat::Txt => "txt",
        };
        let safe_stem = sanitize_file_stem(&input.book_name);
        let file_name = format!("{safe_stem}.{ext}");

        let mut imported_book_id = None;
        let mut output_path = None;
        let file_size: u64;

        // If downloading EPUB for an existing unified online-tracked book, atomically update
        // its standard EPUB 3.0 in-place instead of creating a duplicate book entry.
        if input.import_to_library
            && input.format == OnlineDownloadFormat::Epub
            && let Some(bid) = target_tracked_book_id
        {
            self.update_job(job_id, |j| {
                j.current_chapter_title = Some("正在同步标准 EPUB 3.0 文件…".into());
                j.elapsed_ms = started_at.elapsed().as_millis() as u64;
                j.chapters_per_sec = compute_chapters_per_sec(j.processed_chapters, j.elapsed_ms);
            })
            .await;

            match self.sync_tracked_epub(bid, true).await {
                Ok(summary) => {
                    imported_book_id = Some(summary.id.0);
                    file_size = summary.file_size;
                }
                Err(e) => {
                    self.fail_job(job_id, started_at, format!("同步 EPUB 3.0 失败: {e}"))
                        .await;
                    return;
                }
            }
        } else {
            // Optional cover download for EPUB
            let cover_bytes = if input.format == OnlineDownloadFormat::Epub
                && let Some(ref cover_url) = input.cover_url
                && !cover_url.trim().is_empty()
            {
                self.fetch_cover_proxy(cover_url).await.ok()
            } else {
                None
            };

            if cancel_flag.load(Ordering::Acquire) {
                self.mark_job_cancelled(job_id, started_at).await;
                return;
            }

            self.update_job(job_id, |j| {
                j.current_chapter_title = Some("正在封装书籍文件…".into());
                j.elapsed_ms = started_at.elapsed().as_millis() as u64;
                j.chapters_per_sec = compute_chapters_per_sec(j.processed_chapters, j.elapsed_ms);
            })
            .await;

            let pack_meta = PackBookMetadata {
                identifier: None,
                title: input.book_name.clone(),
                author: input.author.clone(),
                description: input.intro.clone(),
                cover_bytes,
            };
            let fmt = input.format;
            let packaged = tokio::task::spawn_blocking(move || match fmt {
                OnlineDownloadFormat::Epub => {
                    package_chapters_to_epub(&pack_meta, &packed_chapters)
                }
                OnlineDownloadFormat::Txt => {
                    Ok(package_chapters_to_txt(&pack_meta, &packed_chapters))
                }
            })
            .await
            .map_err(|e| format!("封装线程失败: {e}"))
            .and_then(|r| r);

            let bytes = match packaged {
                Ok(b) => b,
                Err(e) => {
                    self.fail_job(job_id, started_at, e).await;
                    return;
                }
            };

            if cancel_flag.load(Ordering::Acquire) {
                self.mark_job_cancelled(job_id, started_at).await;
                return;
            }

            file_size = bytes.len() as u64;

            if input.import_to_library {
                self.update_job(job_id, |j| {
                    j.current_chapter_title = Some("正在导入本地图书库…".into());
                    j.elapsed_ms = started_at.elapsed().as_millis() as u64;
                })
                .await;
                match self
                    .catalog
                    .upload_book(&actor, &file_name, Bytes::from(bytes))
                    .await
                {
                    Ok(summary) => {
                        imported_book_id = Some(summary.id.0);
                    }
                    Err(e) => {
                        self.fail_job(job_id, started_at, format!("封装成功但导入图书库失败: {e}"))
                            .await;
                        return;
                    }
                }
            } else {
                let job_dir = self.download_dir.join(job_id.to_string());
                if let Err(e) = tokio::fs::create_dir_all(&job_dir).await {
                    self.fail_job(job_id, started_at, format!("创建输出目录失败: {e}"))
                        .await;
                    return;
                }
                let out_path = job_dir.join(&file_name);
                if let Err(e) = tokio::fs::write(&out_path, &bytes).await {
                    self.fail_job(job_id, started_at, format!("写入输出文件失败: {e}"))
                        .await;
                    return;
                }
                output_path = Some(out_path);
            }
        }

        let final_elapsed = started_at.elapsed().as_millis() as u64;
        let mut guard = self.downloads.write().await;
        if let Some(slot) = guard.get_mut(&job_id) {
            if cancel_flag.load(Ordering::Acquire)
                || slot.dto.status == DownloadJobStatus::Cancelled
            {
                slot.dto.status = DownloadJobStatus::Cancelled;
                slot.dto.current_chapter_title = None;
                slot.dto.elapsed_ms = final_elapsed;
                slot.dto.chapters_per_sec =
                    compute_chapters_per_sec(slot.dto.processed_chapters, final_elapsed);
                slot.dto.updated_at = Timestamp::now();
                drop(guard);
                let job_dir = self.download_dir.join(job_id.to_string());
                let _ = tokio::fs::remove_dir_all(&job_dir).await;
                return;
            }
            slot.output_path = output_path;
            slot.dto.status = DownloadJobStatus::Completed;
            slot.dto.output_file_name = Some(file_name);
            slot.dto.file_size = Some(file_size);
            slot.dto.imported_book_id = imported_book_id;
            slot.dto.current_chapter_title = None;
            slot.dto.elapsed_ms = final_elapsed;
            slot.dto.chapters_per_sec =
                compute_chapters_per_sec(slot.dto.processed_chapters, final_elapsed);
            slot.dto.updated_at = Timestamp::now();
        } else {
            // Job was deleted while finishing; clean up disk artifact if any
            drop(guard);
            let job_dir = self.download_dir.join(job_id.to_string());
            let _ = tokio::fs::remove_dir_all(&job_dir).await;
        }
    }

    async fn update_job(&self, job_id: Uuid, f: impl FnOnce(&mut OnlineDownloadJobDto)) {
        let mut guard = self.downloads.write().await;
        if let Some(slot) = guard.get_mut(&job_id)
            && slot.dto.status != DownloadJobStatus::Cancelled
        {
            f(&mut slot.dto);
            slot.dto.updated_at = Timestamp::now();
        }
    }

    async fn fail_job(&self, job_id: Uuid, started_at: Instant, msg: String) {
        let elapsed = started_at.elapsed().as_millis() as u64;
        self.update_job(job_id, |j| {
            j.status = DownloadJobStatus::Failed;
            j.error = Some(msg);
            j.current_chapter_title = None;
            j.elapsed_ms = elapsed;
            j.chapters_per_sec = compute_chapters_per_sec(j.processed_chapters, elapsed);
        })
        .await;
    }

    async fn mark_job_cancelled(&self, job_id: Uuid, started_at: Instant) {
        let elapsed = started_at.elapsed().as_millis() as u64;
        let mut guard = self.downloads.write().await;
        if let Some(slot) = guard.get_mut(&job_id) {
            slot.dto.status = DownloadJobStatus::Cancelled;
            slot.dto.current_chapter_title = None;
            slot.dto.error = None;
            slot.dto.elapsed_ms = elapsed;
            slot.dto.chapters_per_sec =
                compute_chapters_per_sec(slot.dto.processed_chapters, elapsed);
            slot.dto.updated_at = Timestamp::now();
        }
    }

    pub async fn list_downloads(&self, user_id: UserId) -> Vec<OnlineDownloadJobDto> {
        let guard = self.downloads.read().await;
        let mut list: Vec<OnlineDownloadJobDto> = guard
            .values()
            .filter(|j| j.dto.user_id == user_id.0)
            .map(StoredDownloadJob::snapshot_dto)
            .collect();
        list.sort_by_key(|j| std::cmp::Reverse(j.created_at));
        list
    }

    pub async fn get_download(
        &self,
        user_id: UserId,
        job_id: Uuid,
    ) -> DomainResult<OnlineDownloadJobDto> {
        let guard = self.downloads.read().await;
        let slot = guard
            .get(&job_id)
            .ok_or_else(|| DomainError::NotFound("未找到下载任务".into()))?;
        if slot.dto.user_id != user_id.0 {
            return Err(DomainError::Forbidden("无权查看他人下载任务".into()));
        }
        Ok(slot.snapshot_dto())
    }

    pub async fn cancel_download(
        &self,
        user_id: UserId,
        job_id: Uuid,
    ) -> DomainResult<OnlineDownloadJobDto> {
        let mut guard = self.downloads.write().await;
        let slot = guard
            .get_mut(&job_id)
            .ok_or_else(|| DomainError::NotFound("未找到下载任务".into()))?;
        if slot.dto.user_id != user_id.0 {
            return Err(DomainError::Forbidden("无权取消他人下载任务".into()));
        }
        if matches!(
            slot.dto.status,
            DownloadJobStatus::Pending | DownloadJobStatus::Running
        ) {
            slot.signal_cancel();
            if let Some(started) = slot.started_at {
                let elapsed = started.elapsed().as_millis() as u64;
                slot.dto.elapsed_ms = elapsed;
                slot.dto.chapters_per_sec =
                    compute_chapters_per_sec(slot.dto.processed_chapters, elapsed);
            }
            slot.dto.status = DownloadJobStatus::Cancelled;
            slot.dto.current_chapter_title = None;
            slot.dto.error = None;
            slot.dto.updated_at = Timestamp::now();
        }
        Ok(slot.snapshot_dto())
    }

    pub async fn delete_download(&self, user_id: UserId, job_id: Uuid) -> DomainResult<()> {
        {
            let mut guard = self.downloads.write().await;
            let slot = guard
                .get(&job_id)
                .ok_or_else(|| DomainError::NotFound("未找到下载任务".into()))?;
            if slot.dto.user_id != user_id.0 {
                return Err(DomainError::Forbidden("无权删除他人下载任务".into()));
            }
            if matches!(
                slot.dto.status,
                DownloadJobStatus::Pending | DownloadJobStatus::Running
            ) {
                slot.signal_cancel();
            }
            guard.remove(&job_id);
        }

        let job_dir = self.download_dir.join(job_id.to_string());
        if let Err(e) = tokio::fs::remove_dir_all(&job_dir).await
            && e.kind() != std::io::ErrorKind::NotFound
        {
            return Err(DomainError::Storage(format!("清理下载任务目录失败: {e}")));
        }
        Ok(())
    }

    pub async fn open_download_file_stream(
        &self,
        user_id: UserId,
        job_id: Uuid,
    ) -> DomainResult<(String, &'static str, ByteRangeStream)> {
        let (file_name, format, path, imported_book_id) = {
            let guard = self.downloads.read().await;
            let slot = guard
                .get(&job_id)
                .ok_or_else(|| DomainError::NotFound("未找到下载任务".into()))?;
            if slot.dto.user_id != user_id.0 {
                return Err(DomainError::Forbidden("无权下载他人任务产物".into()));
            }
            let fname = slot
                .dto
                .output_file_name
                .clone()
                .unwrap_or_else(|| "novel.epub".to_string());
            (
                fname,
                slot.dto.format,
                slot.output_path.clone(),
                slot.dto.imported_book_id,
            )
        };

        let mime = match format {
            OnlineDownloadFormat::Epub => "application/epub+zip",
            OnlineDownloadFormat::Txt => "text/plain; charset=utf-8",
        };

        if let Some(p) = path {
            let stream = open_file_range_stream_spec(&p, ByteRangeSpec::Full).await?;
            return Ok((file_name, mime, stream));
        }
        if let Some(book_id) = imported_book_id {
            let (_book, stream) = self
                .catalog
                .open_book_file_stream(BookId(book_id), ByteRangeSpec::Full)
                .await?;
            return Ok((file_name, mime, stream));
        }
        Err(DomainError::Validation("任务尚未完成或产物不存在".into()))
    }

    pub async fn read_download_file(
        &self,
        user_id: UserId,
        job_id: Uuid,
    ) -> DomainResult<(String, &'static str, Vec<u8>)> {
        let (file_name, mime, stream) = self.open_download_file_stream(user_id, job_id).await?;
        let bytes = axum::body::to_bytes(axum::body::Body::from_stream(stream.stream), usize::MAX)
            .await
            .map_err(|e| DomainError::Storage(format!("读取下载文件流失败: {e}")))?;
        Ok((file_name, mime, bytes.to_vec()))
    }

    pub async fn fetch_cover_proxy(&self, raw_url: &str) -> DomainResult<(Vec<u8>, String)> {
        const MAX_COVER_BYTES: usize = 8 * 1024 * 1024;
        let validated = validate_target_url_static(raw_url.trim(), self.allow_private_ips)
            .map_err(DomainError::Validation)?;
        let client = get_or_build_async_client(12, self.allow_private_ips, None)
            .map_err(DomainError::Storage)?;

        let mut resp = client
            .get(validated)
            .send()
            .await
            .map_err(|e| DomainError::Validation(format!("拉取封面失败: {e}")))?;

        if !resp.status().is_success() {
            return Err(DomainError::Validation(format!(
                "封面响应状态异常: {}",
                resp.status()
            )));
        }

        let header_ct = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.split(';').next().unwrap_or(s).trim().to_ascii_lowercase());

        if let Some(ref ct) = header_ct {
            validate_cover_header_mime(ct)?;
        }

        if let Some(content_len) = resp.content_length()
            && content_len > MAX_COVER_BYTES as u64
        {
            return Err(DomainError::Validation("封面图片体积超过 8MB 限制".into()));
        }

        let mut bytes = Vec::new();
        while let Some(chunk) = resp
            .chunk()
            .await
            .map_err(|e| DomainError::Validation(format!("读取封面失败: {e}")))?
        {
            if bytes.len().saturating_add(chunk.len()) > MAX_COVER_BYTES {
                return Err(DomainError::Validation("封面图片体积超过 8MB 限制".into()));
            }
            bytes.extend_from_slice(&chunk);
        }

        let safe_mime = validate_safe_cover_image(header_ct.as_deref(), &bytes)?;
        Ok((bytes, safe_mime))
    }
}

fn validate_cover_header_mime(ct_lower: &str) -> DomainResult<()> {
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

fn read_cached_chapter_at(
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

fn write_cached_chapter_at(
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

fn sanitize_file_stem(name: &str) -> String {
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
        "novel".to_string()
    } else {
        trimmed.chars().take(80).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::extract::Path as AxumPath;
    use axum::routing::get;
    use tempfile::TempDir;
    use toasty::Db;
    use toasty::schema::ModelSet;
    use xmedia_core::application::auth::AuthService;
    use xmedia_core::application::dto::SetupAdminInput;
    use xmedia_core::application::library::LibraryService;
    use xmedia_core::infra::events::BroadcastEventHub;
    use xmedia_core::infra::persistence::repositories::{
        ToastyLibraryRepository, ToastyUserRepository,
    };
    use xmedia_core::infra::security::{JwtTokenService, SecurityManager};
    use xmedia_core::{ensure_core_schema, register_core_models};

    use crate::infra::persistence::repositories::{
        ToastyBookSourceRepository, ToastyOnlineShelfRepository,
    };
    use crate::source::http::HttpClientConfig;
    use crate::{ensure_book_schema, init_book_module, register_book_models};

    async fn setup_test_online_service() -> (TempDir, User, Arc<BookOnlineService>) {
        let (tmp, admin, _member, online_svc) = setup_test_online_service_with_member().await;
        (tmp, admin, online_svc)
    }

    async fn setup_test_online_service_with_member() -> (TempDir, User, User, Arc<BookOnlineService>)
    {
        let tmp = TempDir::new().unwrap();
        let db_path = tmp.path().join("online_unit_test.db");
        let db_url = format!("sqlite:{}", db_path.display());

        let mut model_set = ModelSet::new();
        register_core_models(&mut model_set);
        register_book_models(&mut model_set);
        let mut db = Db::builder()
            .models(model_set)
            .connect(&db_url)
            .await
            .unwrap();
        ensure_core_schema(&mut db, &db_url).await.unwrap();
        ensure_book_schema(&mut db, &db_url).await.unwrap();

        let users_repo = Arc::new(ToastyUserRepository::new(db.clone()));
        let libs_repo = Arc::new(ToastyLibraryRepository::new(db.clone()));
        let lifecycles = Arc::new(std::sync::RwLock::new(HashMap::new()));
        let security = Arc::new(SecurityManager::new("book-unit-secret"));
        let tokens = Arc::new(JwtTokenService::new("book-unit-secret", 3600));
        let events = Arc::new(BroadcastEventHub::default());
        let auth_svc = Arc::new(AuthService::new(
            users_repo.clone(),
            security,
            tokens,
            lifecycles.clone(),
        ));
        let session = auth_svc
            .setup_initial_admin(SetupAdminInput {
                username: "admin".into(),
                display_name: Some("Admin".into()),
                password: "password123".into(),
            })
            .await
            .unwrap();
        let admin = auth_svc.authenticate_jwt(&session.token).await.unwrap();
        let member_dto = auth_svc
            .create_user(
                &admin,
                xmedia_core::application::dto::CreateUserInput {
                    username: "member".into(),
                    display_name: Some("Member".into()),
                    password: "password123".into(),
                    role: xmedia_core::domain::user::UserRole::Member,
                },
            )
            .await
            .unwrap();
        let member = xmedia_core::domain::repository::UserRepository::find_by_id(
            users_repo.as_ref(),
            member_dto.id,
        )
        .await
        .unwrap()
        .unwrap();
        let lib_svc = Arc::new(LibraryService::new(
            libs_repo.clone(),
            events.clone(),
            lifecycles,
        ));
        let book_services =
            init_book_module(db.clone(), tmp.path(), libs_repo, &lib_svc, events).unwrap();
        let sources_repo = Arc::new(ToastyBookSourceRepository::new(db.clone()));
        let shelves_repo = Arc::new(ToastyOnlineShelfRepository::new(db.clone()));
        let online_svc = Arc::new(
            BookOnlineService::with_engine(
                sources_repo,
                shelves_repo,
                tmp.path(),
                book_services.catalog.clone(),
                BookSourceEngine::new(
                    HttpClientConfig {
                        allow_private_ips: true,
                        ..Default::default()
                    },
                    Duration::from_secs(10),
                ),
                true,
            )
            .unwrap(),
        );
        (tmp, admin, member, online_svc)
    }

    #[test]
    fn cover_image_validation_rejects_html_and_svg_xss_payloads() {
        let html = b"<html><script>alert(1)</script></html>";
        assert!(validate_safe_cover_image(Some("text/html"), html).is_err());
        assert!(validate_safe_cover_image(Some("image/jpeg"), html).is_err());

        let svg = b"<svg xmlns=\"http://www.w3.org/2000/svg\" onload=\"alert(1)\"/>";
        assert!(validate_safe_cover_image(Some("image/svg+xml"), svg).is_err());
        assert!(validate_safe_cover_image(None, svg).is_err());

        // Valid PNG magic bytes with text/html header must still be rejected
        let png = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0];
        assert!(validate_safe_cover_image(Some("text/html"), &png).is_err());
        assert!(validate_safe_cover_image(Some("image/svg+xml"), &png).is_err());

        // Valid PNG / JPEG / WebP with safe or octet-stream header succeed
        assert_eq!(
            validate_safe_cover_image(Some("image/png"), &png).unwrap(),
            "image/png"
        );
        assert_eq!(
            validate_safe_cover_image(Some("application/octet-stream"), &png).unwrap(),
            "image/png"
        );
        let jpg = [0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        assert_eq!(validate_safe_cover_image(None, &jpg).unwrap(), "image/jpeg");
    }

    #[test]
    fn rate_limiter_wakes_immediately_on_cancel() {
        let (_conc, limiter) = DownloadRateLimiter::from_spec(Some(ConcurrentRateSpec {
            max_concurrency: 1,
            window_requests: 1,
            window_ms: 5_000,
        }));
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_cond = Arc::new((Mutex::new(false), Condvar::new()));

        // First slot succeeds immediately
        assert!(limiter.wait_for_slot(&cancel_flag, &cancel_cond));

        let flag_bg = cancel_flag.clone();
        let cond_bg = cancel_cond.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(30));
            flag_bg.store(true, Ordering::Release);
            let (lock, cvar) = &*cond_bg;
            *lock.lock().unwrap() = true;
            cvar.notify_all();
        });

        let t0 = Instant::now();
        assert!(!limiter.wait_for_slot(&cancel_flag, &cancel_cond));
        assert!(
            t0.elapsed() < Duration::from_millis(500),
            "expected prompt wakeup on cancel, took {:?}",
            t0.elapsed()
        );
    }

    #[tokio::test]
    async fn concurrent_download_reports_realtime_speed_preserves_order_and_deletes_disk_artifact()
    {
        let in_flight = Arc::new(AtomicUsize::new(0));
        let max_in_flight = Arc::new(AtomicUsize::new(0));
        let total_chapters = 10usize;

        let in_flight_c = in_flight.clone();
        let max_in_flight_c = max_in_flight.clone();
        let mock_app = Router::new()
            .route(
                "/toc",
                get(move || async move {
                    let mut items = String::from("<html><body><ul id=\"toc\">");
                    for i in 1..=total_chapters {
                        items.push_str(&format!("<li><a href=\"/ch/{i}\">第{i}章</a></li>"));
                    }
                    items.push_str("</ul></body></html>");
                    items
                }),
            )
            .route(
                "/ch/{num}",
                get(move |AxumPath(num): AxumPath<usize>| {
                    let in_flight = in_flight_c.clone();
                    let max_in_flight = max_in_flight_c.clone();
                    async move {
                        let cur = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                        max_in_flight.fetch_max(cur, Ordering::SeqCst);
                        tokio::time::sleep(Duration::from_millis(40)).await;
                        in_flight.fetch_sub(1, Ordering::SeqCst);
                        format!("<html><body><div id=\"body\"><p>正文内容第{num}章</p></div></body></html>")
                    }
                }),
            );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_handle = tokio::spawn(async move {
            axum::serve(listener, mock_app).await.unwrap();
        });

        let (tmp, admin, online_svc) = setup_test_online_service().await;
        let raw_source = serde_json::json!({
            "bookSourceName": "并发测试书源",
            "bookSourceUrl": format!("http://{addr}"),
            "concurrentRate": "4/100",
            "ruleToc": {
                "chapterList": "#toc li a",
                "chapterName": "text",
                "chapterUrl": "href"
            },
            "ruleContent": {
                "content": "id.body@html"
            }
        })
        .to_string();
        let imported = online_svc
            .import_sources(&admin, &raw_source)
            .await
            .unwrap();
        let source_id = imported.sources[0].id;

        let job = online_svc
            .start_download(
                &admin,
                StartDownloadInput {
                    book_id: None,
                    source_id,
                    book_name: "并发测试小说".into(),
                    author: "作者A".into(),
                    book_url: format!("http://{addr}/book"),
                    toc_url: format!("http://{addr}/toc"),
                    cover_url: None,
                    intro: None,
                    format: OnlineDownloadFormat::Txt,
                    start_chapter: None,
                    end_chapter: None,
                    import_to_library: false,
                },
            )
            .await
            .unwrap();

        let mut finished = job;
        for _ in 0..120 {
            finished = online_svc
                .get_download(admin.id, finished.id)
                .await
                .unwrap();
            if matches!(
                finished.status,
                DownloadJobStatus::Completed
                    | DownloadJobStatus::Failed
                    | DownloadJobStatus::Cancelled
            ) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        assert_eq!(
            finished.status,
            DownloadJobStatus::Completed,
            "{finished:?}"
        );
        assert_eq!(finished.processed_chapters, total_chapters);
        assert_eq!(finished.failed_chapters, 0);
        assert!(finished.elapsed_ms > 0, "expected elapsed_ms > 0");
        assert!(
            finished.chapters_per_sec > 0.0,
            "expected chapters_per_sec > 0.0, got {}",
            finished.chapters_per_sec
        );
        assert!(
            max_in_flight.load(Ordering::SeqCst) >= 2,
            "expected concurrent chapter fetching (max_in_flight={})",
            max_in_flight.load(Ordering::SeqCst)
        );

        // Verify chapter ordering in packaged TXT via streaming reader
        let (fname, mime, bytes) = online_svc
            .read_download_file(admin.id, finished.id)
            .await
            .unwrap();
        assert_eq!(fname, "并发测试小说.txt");
        assert_eq!(mime, "text/plain; charset=utf-8");
        let text = String::from_utf8(bytes).unwrap();
        let pos1 = text.find("正文内容第1章").unwrap();
        let pos5 = text.find("正文内容第5章").unwrap();
        let pos10 = text.find("正文内容第10章").unwrap();
        assert!(
            pos1 < pos5 && pos5 < pos10,
            "chapter order must be preserved"
        );

        // Verify disk artifact exists, then DELETE removes both record and disk directory
        let job_dir = tmp
            .path()
            .join("online_downloads")
            .join(finished.id.to_string());
        assert!(job_dir.is_dir());
        online_svc
            .delete_download(admin.id, finished.id)
            .await
            .unwrap();
        assert!(!job_dir.exists());
        assert!(
            online_svc
                .get_download(admin.id, finished.id)
                .await
                .is_err()
        );

        server_handle.abort();
    }

    #[tokio::test]
    async fn download_can_be_cancelled_mid_flight() {
        let fetched_count = Arc::new(AtomicUsize::new(0));
        let fetched_c = fetched_count.clone();
        let mock_app = Router::new()
            .route(
                "/toc",
                get(|| async {
                    let mut items = String::from("<html><body><ul id=\"toc\">");
                    for i in 1..=20 {
                        items.push_str(&format!("<li><a href=\"/ch/{i}\">第{i}章</a></li>"));
                    }
                    items.push_str("</ul></body></html>");
                    items
                }),
            )
            .route(
                "/ch/{num}",
                get(move |AxumPath(num): AxumPath<usize>| {
                    let fetched = fetched_c.clone();
                    async move {
                        fetched.fetch_add(1, Ordering::SeqCst);
                        tokio::time::sleep(Duration::from_millis(80)).await;
                        format!("<html><body><div id=\"body\"><p>慢章{num}</p></div></body></html>")
                    }
                }),
            );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_handle = tokio::spawn(async move {
            axum::serve(listener, mock_app).await.unwrap();
        });

        let (tmp, admin, online_svc) = setup_test_online_service().await;
        let raw_source = serde_json::json!({
            "bookSourceName": "慢速可取消书源",
            "bookSourceUrl": format!("http://{addr}"),
            "concurrentRate": "200",
            "ruleToc": {
                "chapterList": "#toc li a",
                "chapterName": "text",
                "chapterUrl": "href"
            },
            "ruleContent": {
                "content": "id.body@html"
            }
        })
        .to_string();
        let imported = online_svc
            .import_sources(&admin, &raw_source)
            .await
            .unwrap();
        let source_id = imported.sources[0].id;

        let job = online_svc
            .start_download(
                &admin,
                StartDownloadInput {
                    book_id: None,
                    source_id,
                    book_name: "可取消小说".into(),
                    author: "作者B".into(),
                    book_url: format!("http://{addr}/book"),
                    toc_url: format!("http://{addr}/toc"),
                    cover_url: None,
                    intro: None,
                    format: OnlineDownloadFormat::Epub,
                    start_chapter: None,
                    end_chapter: None,
                    import_to_library: false,
                },
            )
            .await
            .unwrap();

        // Wait until at least 1 chapter has been processed or job is running
        for _ in 0..40 {
            let cur = online_svc.get_download(admin.id, job.id).await.unwrap();
            if cur.processed_chapters >= 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        let cancelled = online_svc.cancel_download(admin.id, job.id).await.unwrap();
        assert_eq!(cancelled.status, DownloadJobStatus::Cancelled);

        // Give background workers a moment to observe cancellation
        tokio::time::sleep(Duration::from_millis(150)).await;
        let after = online_svc.get_download(admin.id, job.id).await.unwrap();
        assert_eq!(after.status, DownloadJobStatus::Cancelled);
        assert!(
            fetched_count.load(Ordering::SeqCst) < 10,
            "cancellation should stop remaining chapter fetches early (fetched={})",
            fetched_count.load(Ordering::SeqCst)
        );
        let job_dir = tmp.path().join("online_downloads").join(job.id.to_string());
        assert!(!job_dir.exists());

        server_handle.abort();
    }

    #[tokio::test]
    async fn tracked_online_book_creates_unified_book_and_syncs_standard_epub3() {
        let mock_app = Router::new()
            .route(
                "/toc",
                get(|| async {
                    r#"<html><body><ul id="toc">
                        <li><a href="/ch/1">第一章 初入江湖</a></li>
                        <li><a href="/ch/2">第二章 剑气初成</a></li>
                        <li><a href="/ch/3">第三章 远行问道</a></li>
                    </ul></body></html>"#
                }),
            )
            .route(
                "/ch/{num}",
                get(|AxumPath(num): AxumPath<usize>| async move {
                    format!(
                        "<html><body><div id=\"body\"><p>第{num}章正文第一段。</p><p>第{num}章正文第二段。</p></div></body></html>"
                    )
                }),
            );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_handle = tokio::spawn(async move {
            axum::serve(listener, mock_app).await.unwrap();
        });

        let (tmp, admin, online_svc) = setup_test_online_service().await;
        let raw_source = serde_json::json!({
            "bookSourceName": "统一书籍测试书源",
            "bookSourceUrl": format!("http://{addr}"),
            "ruleToc": {
                "chapterList": "#toc li a",
                "chapterName": "text",
                "chapterUrl": "href"
            },
            "ruleContent": {
                "content": "id.body@html"
            }
        })
        .to_string();
        let imported = online_svc
            .import_sources(&admin, &raw_source)
            .await
            .unwrap();
        let source_id = imported.sources[0].id;

        // 1. Track online book -> creates a real unified Book + initial valid EPUB 3.0 skeleton
        let tracked = online_svc
            .track_online_book(
                &admin,
                TrackOnlineBookInput {
                    source_id,
                    name: "青山问道".into(),
                    author: "测试作者".into(),
                    book_url: format!("http://{addr}/book/1"),
                    toc_url: format!("http://{addr}/toc"),
                    cover_url: None,
                    intro: Some("一本用来验证统一 EPUB3 同步的小说。".into()),
                    kind: Some("玄幻, 仙侠".into()),
                    last_chapter: None,
                },
            )
            .await
            .unwrap();

        let tracking = tracked.online_tracking.as_ref().expect("tracking metadata");
        assert_eq!(tracking.total_chapters, 3);
        assert_eq!(tracking.cached_chapters, 0);
        assert!(!tracking.epub_dirty);
        assert_eq!(tracked.format, BookFormat::Epub);
        assert!(tracked.file_size > 0);

        // Verify initial skeleton EPUB 3.0 is valid ZIP with intro.xhtml
        let epub_path = tmp
            .path()
            .join("book")
            .join("uploads")
            .join(format!("{}.epub", tracked.id.0));
        assert!(epub_path.is_file());
        {
            let raw = std::fs::read(&epub_path).unwrap();
            let mut archive = zip::ZipArchive::new(std::io::Cursor::new(raw)).unwrap();
            assert!(archive.by_name("mimetype").is_ok());
            assert!(archive.by_name("OEBPS/content.opf").is_ok());
            assert!(archive.by_name("OEBPS/nav.xhtml").is_ok());
            assert!(archive.by_name("OEBPS/intro.xhtml").is_ok());
        }

        // Duplicate track call must return the exact same BookId
        let tracked_again = online_svc
            .track_online_book(
                &admin,
                TrackOnlineBookInput {
                    source_id,
                    name: "青山问道".into(),
                    author: "测试作者".into(),
                    book_url: format!("http://{addr}/book/1"),
                    toc_url: format!("http://{addr}/toc"),
                    cover_url: None,
                    intro: None,
                    kind: None,
                    last_chapter: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(tracked_again.id, tracked.id);

        // 2. Fetch chapter 0 & chapter 1 -> incremental single-chapter cache + epub_dirty = true
        let ch0 = online_svc
            .get_tracked_chapter(tracked.id, 0, false)
            .await
            .unwrap();
        assert!(!ch0.cached);
        assert!(ch0.content.contains("第1章正文第一段"));
        assert_eq!(ch0.cached_chapters, 1);
        assert!(ch0.epub_dirty);

        // Second read of chapter 0 hits local chapter cache immediately
        let ch0_cached = online_svc
            .get_tracked_chapter(tracked.id, 0, false)
            .await
            .unwrap();
        assert!(ch0_cached.cached);

        let ch1 = online_svc
            .get_tracked_chapter(tracked.id, 1, false)
            .await
            .unwrap();
        assert_eq!(ch1.cached_chapters, 2);

        // 3. On-demand EPUB 3.0 sync -> rebuilds standard EPUB3 containing both cached chapters
        let synced = online_svc
            .sync_tracked_epub(tracked.id, false)
            .await
            .unwrap();
        let synced_tracking = synced.online_tracking.as_ref().unwrap();
        assert_eq!(synced_tracking.cached_chapters, 2);
        assert!(!synced_tracking.epub_dirty);
        {
            let raw = std::fs::read(&epub_path).unwrap();
            let mut archive = zip::ZipArchive::new(std::io::Cursor::new(raw)).unwrap();
            let mut ch1_html = String::new();
            std::io::Read::read_to_string(
                &mut archive.by_name("OEBPS/ch_00001.xhtml").unwrap(),
                &mut ch1_html,
            )
            .unwrap();
            assert!(ch1_html.contains("第1章正文第一段"));
            let mut ch2_html = String::new();
            std::io::Read::read_to_string(
                &mut archive.by_name("OEBPS/ch_00002.xhtml").unwrap(),
                &mut ch2_html,
            )
            .unwrap();
            assert!(ch2_html.contains("第2章正文第一段"));
        }

        // 4. Batch download with book_id updates the SAME Book in-place (no duplicate book!)
        let job = online_svc
            .start_download(
                &admin,
                StartDownloadInput {
                    book_id: Some(tracked.id.0),
                    source_id,
                    book_name: "青山问道".into(),
                    author: "测试作者".into(),
                    book_url: format!("http://{addr}/book/1"),
                    toc_url: format!("http://{addr}/toc"),
                    cover_url: None,
                    intro: Some("一本用来验证统一 EPUB3 同步的小说。".into()),
                    format: OnlineDownloadFormat::Epub,
                    start_chapter: None,
                    end_chapter: None,
                    import_to_library: true,
                },
            )
            .await
            .unwrap();

        let mut finished = job;
        for _ in 0..80 {
            finished = online_svc
                .get_download(admin.id, finished.id)
                .await
                .unwrap();
            if finished.status == DownloadJobStatus::Completed
                || finished.status == DownloadJobStatus::Failed
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(finished.status, DownloadJobStatus::Completed);
        assert_eq!(finished.imported_book_id, Some(tracked.id.0));

        // Verify all 3 chapters are now in the same EPUB3 file and only 1 Book exists
        {
            let raw = std::fs::read(&epub_path).unwrap();
            let mut archive = zip::ZipArchive::new(std::io::Cursor::new(raw)).unwrap();
            assert!(archive.by_name("OEBPS/ch_00003.xhtml").is_ok());
        }
        let search_page = online_svc
            .catalog
            .search_books(Some(&admin), Default::default())
            .await
            .unwrap();
        assert_eq!(search_page.total, 1);

        // 5. Deleting the Book cleans up both EPUB3 file and online_tracked chapter workspace
        let tracked_dir = tmp
            .path()
            .join("book")
            .join("online_tracked")
            .join(tracked.id.0.to_string());
        assert!(tracked_dir.is_dir());
        online_svc
            .catalog
            .delete_book(&admin, tracked.id)
            .await
            .unwrap();
        assert!(!epub_path.exists());
        assert!(!tracked_dir.exists());

        server_handle.abort();
    }

    #[tokio::test]
    async fn test_source_debugger_runs_full_chain_and_individual_rule_modes() {
        let mock_app = Router::new()
            .route(
                "/search",
                get(|| async {
                    r#"<html><body>
                        <div class="item">
                            <a class="title" href="/book/42">星辰剑道</a>
                            <span class="author">青云客</span>
                            <span class="kind">玄幻</span>
                            <span class="latest">第三章 破境</span>
                            <p class="intro">少年持剑踏星河。</p>
                        </div>
                    </body></html>"#
                }),
            )
            .route(
                "/book/42",
                get(|| async {
                    r#"<html><body>
                        <h1 id="name">星辰剑道</h1>
                        <div id="author">青云客</div>
                        <div id="intro">少年持剑踏星河，一剑开天门。</div>
                        <a id="toc-link" href="/book/42/toc">查看目录</a>
                    </body></html>"#
                }),
            )
            .route(
                "/book/42/toc",
                get(|| async {
                    r#"<html><body><ul id="toc">
                        <li><a href="/ch/1">第一章 初闻剑鸣</a></li>
                        <li><a href="/ch/2">第二章 星河初现</a></li>
                    </ul></body></html>"#
                }),
            )
            .route(
                "/ch/{num}",
                get(|AxumPath(num): AxumPath<usize>| async move {
                    format!(
                        "<html><body><div id=\"body\"><p>这是第{num}章的完整调试正文内容。</p></div></body></html>"
                    )
                }),
            );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_handle = tokio::spawn(async move {
            axum::serve(listener, mock_app).await.unwrap();
        });

        let (_tmp, admin, online_svc) = setup_test_online_service().await;
        let raw_source = serde_json::json!({
            "bookSourceName": "全链路调试书源",
            "bookSourceUrl": format!("http://{addr}"),
            "searchUrl": "/search?q={{key}}",
            "ruleSearch": {
                "checkKeyWord": "星辰",
                "bookList": ".item",
                "name": ".title@text",
                "bookUrl": ".title@href",
                "author": ".author@text",
                "kind": ".kind@text",
                "lastChapter": ".latest@text",
                "intro": ".intro@text"
            },
            "ruleBookInfo": {
                "name": "#name@text",
                "author": "#author@text",
                "intro": "#intro@text",
                "tocUrl": "#toc-link@href"
            },
            "ruleToc": {
                "chapterList": "#toc li a",
                "chapterName": "text",
                "chapterUrl": "href"
            },
            "ruleContent": {
                "content": "id.body@html"
            }
        })
        .to_string();

        let imported = online_svc
            .import_sources(&admin, &raw_source)
            .await
            .unwrap();
        let src = &imported.sources[0];
        assert_eq!(src.check_keyword.as_deref(), Some("星辰"));

        // 1. Full chain using default checkKeyWord
        let full = online_svc
            .test_source(&admin, src.id, SourceTestInput::default())
            .await
            .unwrap();
        assert!(full.ok);
        assert_eq!(full.keyword, "星辰");
        assert_eq!(full.steps.len(), 4);
        assert_eq!(full.search_items.len(), 1);
        assert_eq!(full.book_detail.as_ref().unwrap().name, "星辰剑道");
        assert_eq!(full.toc_total_count, 2);
        assert_eq!(full.toc_chapters.len(), 2);
        assert!(
            full.content_text
                .as_deref()
                .unwrap()
                .contains("第1章的完整调试正文内容")
        );

        // 2. Full chain starting directly from a book URL (skips search stage)
        let direct_book = online_svc
            .test_source(
                &admin,
                src.id,
                SourceTestInput {
                    keyword: Some(format!("http://{addr}/book/42")),
                    mode: SourceTestMode::Full,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert!(direct_book.ok);
        assert_eq!(direct_book.steps.len(), 3);
        assert_eq!(direct_book.steps[0].stage, "book_info");
        assert_eq!(direct_book.toc_total_count, 2);

        // 3. Targeted content-only mode on chapter 2
        let content_only = online_svc
            .test_source(
                &admin,
                src.id,
                SourceTestInput {
                    mode: SourceTestMode::Content,
                    chapter_url: Some(format!("http://{addr}/ch/2")),
                    chapter_index: Some(1),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert!(content_only.ok);
        assert_eq!(content_only.steps.len(), 1);
        assert_eq!(content_only.steps[0].stage, "content");
        assert!(
            content_only
                .content_text
                .as_deref()
                .unwrap()
                .contains("第2章的完整调试正文内容")
        );

        server_handle.abort();
    }

    #[tokio::test]
    async fn reorder_sources_persists_custom_order_across_enabled_and_disabled() {
        let (_tmp, admin, online_svc) = setup_test_online_service().await;
        let batch = serde_json::json!([
            { "bookSourceName": "书源A", "bookSourceUrl": "https://a.example", "enabled": true },
            { "bookSourceName": "书源B", "bookSourceUrl": "https://b.example", "enabled": false },
            { "bookSourceName": "书源C", "bookSourceUrl": "https://c.example", "enabled": true }
        ])
        .to_string();
        let imported = online_svc.import_sources(&admin, &batch).await.unwrap();
        let id_a = imported.sources[0].id;
        let id_b = imported.sources[1].id;
        let id_c = imported.sources[2].id;

        // Place disabled source B at the top, followed by C, then A
        let reordered = online_svc
            .reorder_sources(&admin, vec![id_b, id_c, id_a])
            .await
            .unwrap();
        let names: Vec<&str> = reordered.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["书源B", "书源C", "书源A"]);

        // Subsequent list_sources must return the exact persisted order
        let listed = online_svc.list_sources(&admin).await.unwrap();
        let listed_names: Vec<&str> = listed.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(listed_names, vec!["书源B", "书源C", "书源A"]);
    }

    #[tokio::test]
    async fn batch_source_operations_enable_disable_group_export_and_delete() {
        let (_tmp, admin, online_svc) = setup_test_online_service().await;
        let batch = serde_json::json!([
            { "bookSourceName": "源1", "bookSourceUrl": "https://s1.example", "enabled": true },
            { "bookSourceName": "源2", "bookSourceUrl": "https://s2.example", "enabled": false },
            { "bookSourceName": "源3", "bookSourceUrl": "https://s3.example", "enabled": true }
        ])
        .to_string();
        let imported = online_svc.import_sources(&admin, &batch).await.unwrap();
        let id1 = imported.sources[0].id;
        let id2 = imported.sources[1].id;
        let id3 = imported.sources[2].id;

        // 1. Batch disable [1, 3]
        let rep = online_svc
            .batch_sources(
                &admin,
                BatchBookSourcesInput {
                    ids: vec![id1, id3],
                    action: BatchBookSourceAction::Disable,
                },
            )
            .await
            .unwrap();
        assert_eq!(rep.affected_count, 2);

        // 2. Batch enable [2]
        online_svc
            .batch_sources(
                &admin,
                BatchBookSourcesInput {
                    ids: vec![id2],
                    action: BatchBookSourceAction::Enable,
                },
            )
            .await
            .unwrap();

        // 3. Batch set group on [1, 2]
        online_svc
            .batch_sources(
                &admin,
                BatchBookSourcesInput {
                    ids: vec![id1, id2],
                    action: BatchBookSourceAction::SetGroup {
                        group_name: "精选".into(),
                    },
                },
            )
            .await
            .unwrap();

        let listed = online_svc.list_sources(&admin).await.unwrap();
        let s1 = listed.iter().find(|s| s.id == id1).unwrap();
        let s2 = listed.iter().find(|s| s.id == id2).unwrap();
        let s3 = listed.iter().find(|s| s.id == id3).unwrap();
        assert!(!s1.enabled && s1.group_name == "精选");
        assert!(s2.enabled && s2.group_name == "精选");
        assert!(!s3.enabled && s3.group_name.is_empty());

        // 4. Batch export JSON array [1, 2]
        let exported = online_svc
            .export_sources_batch_json(&admin, vec![id1, id2])
            .await
            .unwrap();
        let arr: Vec<serde_json::Value> = serde_json::from_str(&exported).unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["bookSourceGroup"], "精选");

        // 5. Batch delete [1, 3]
        let del = online_svc
            .batch_sources(
                &admin,
                BatchBookSourcesInput {
                    ids: vec![id1, id3],
                    action: BatchBookSourceAction::Delete,
                },
            )
            .await
            .unwrap();
        assert_eq!(del.affected_count, 2);
        let remaining = online_svc.list_sources(&admin).await.unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, id2);
    }

    #[tokio::test]
    async fn start_download_enforces_admin_and_source_consistency_for_tracked_books() {
        let mock_app = Router::new()
            .route(
                "/toc",
                get(|| async {
                    r#"<html><body><ul id="toc">
                        <li><a href="/ch/1">第一章</a></li>
                    </ul></body></html>"#
                }),
            )
            .route(
                "/ch/1",
                get(|| async { r#"<div id="body"><p>正版正文</p></div>"# }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_handle = tokio::spawn(async move {
            axum::serve(listener, mock_app).await.unwrap();
        });

        let (_tmp, admin, member, online_svc) = setup_test_online_service_with_member().await;
        let raw_sources = serde_json::json!([
            {
                "bookSourceName": "源A",
                "bookSourceUrl": format!("http://{addr}"),
                "ruleToc": { "chapterList": "#toc li a", "chapterName": "text", "chapterUrl": "href" },
                "ruleContent": { "content": "id.body@html" }
            },
            {
                "bookSourceName": "源B",
                "bookSourceUrl": format!("http://{addr}#b"),
                "ruleToc": { "chapterList": "#toc li a", "chapterName": "text", "chapterUrl": "href" },
                "ruleContent": { "content": "id.body@html" }
            }
        ])
        .to_string();
        let imported = online_svc
            .import_sources(&admin, &raw_sources)
            .await
            .unwrap();
        let src_a = imported.sources[0].id;
        let src_b = imported.sources[1].id;

        let tracked = online_svc
            .track_online_book(
                &admin,
                TrackOnlineBookInput {
                    source_id: src_a,
                    name: "权限测试书".into(),
                    author: "作者".into(),
                    book_url: format!("http://{addr}/book"),
                    toc_url: format!("http://{addr}/toc"),
                    cover_url: None,
                    intro: None,
                    kind: None,
                    last_chapter: None,
                },
            )
            .await
            .unwrap();

        // 1. Non-admin passing book_id with import_to_library = false must be rejected (Forbidden)
        let err = online_svc
            .start_download(
                &member,
                StartDownloadInput {
                    book_id: Some(tracked.id.0),
                    source_id: src_a,
                    book_name: "权限测试书".into(),
                    author: "作者".into(),
                    book_url: format!("http://{addr}/book"),
                    toc_url: format!("http://{addr}/toc"),
                    cover_url: None,
                    intro: None,
                    format: OnlineDownloadFormat::Epub,
                    start_chapter: None,
                    end_chapter: None,
                    import_to_library: false,
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, DomainError::Forbidden(_)), "got {err:?}");

        // 2. Admin passing mismatched source_id must be rejected
        let err_src = online_svc
            .start_download(
                &admin,
                StartDownloadInput {
                    book_id: Some(tracked.id.0),
                    source_id: src_b,
                    book_name: "权限测试书".into(),
                    author: "作者".into(),
                    book_url: format!("http://{addr}/book"),
                    toc_url: format!("http://{addr}/toc"),
                    cover_url: None,
                    intro: None,
                    format: OnlineDownloadFormat::Epub,
                    start_chapter: None,
                    end_chapter: None,
                    import_to_library: true,
                },
            )
            .await
            .unwrap_err();
        assert!(
            matches!(err_src, DomainError::Validation(_)),
            "got {err_src:?}"
        );

        // 3. Admin passing mismatched toc_url must be rejected
        let err_toc = online_svc
            .start_download(
                &admin,
                StartDownloadInput {
                    book_id: Some(tracked.id.0),
                    source_id: src_a,
                    book_name: "权限测试书".into(),
                    author: "作者".into(),
                    book_url: format!("http://{addr}/book"),
                    toc_url: format!("http://{addr}/other-toc"),
                    cover_url: None,
                    intro: None,
                    format: OnlineDownloadFormat::Epub,
                    start_chapter: None,
                    end_chapter: None,
                    import_to_library: true,
                },
            )
            .await
            .unwrap_err();
        assert!(
            matches!(err_toc, DomainError::Validation(_)),
            "got {err_toc:?}"
        );

        // 4. Non-admin personal download (book_id: None, import_to_library: false) succeeds
        //    and MUST NOT mutate shared tracked_store chapters
        let job = online_svc
            .start_download(
                &member,
                StartDownloadInput {
                    book_id: None,
                    source_id: src_a,
                    book_name: "权限测试书".into(),
                    author: "作者".into(),
                    book_url: format!("http://{addr}/book"),
                    toc_url: format!("http://{addr}/toc"),
                    cover_url: None,
                    intro: None,
                    format: OnlineDownloadFormat::Txt,
                    start_chapter: None,
                    end_chapter: None,
                    import_to_library: false,
                },
            )
            .await
            .unwrap();
        for _ in 0..80 {
            let st = online_svc.get_download(member.id, job.id).await.unwrap();
            if st.status == DownloadJobStatus::Completed {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(
            online_svc
                .tracked_store
                .load_chapter(tracked.id, 0)
                .is_none(),
            "personal export must not write into shared tracked_store"
        );

        server_handle.abort();
    }

    #[tokio::test]
    async fn non_admin_cannot_call_disabled_source_and_logs_are_stripped() {
        let mock_app = Router::new()
            .route(
                "/search",
                get(|| async { r#"<div class="b"><a href="/book/1">书一</a></div>"# }),
            )
            .route(
                "/book/1",
                get(|| async { r#"<h1 id="n">书一</h1><a id="t" href="/toc">目录</a>"# }),
            )
            .route(
                "/toc",
                get(|| async { r#"<ul id="c"><li><a href="/ch/1">第一章</a></li></ul>"# }),
            )
            .route(
                "/ch/1",
                get(|| async { r#"<div id="body"><p>内容</p></div>"# }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_handle = tokio::spawn(async move {
            axum::serve(listener, mock_app).await.unwrap();
        });

        let (_tmp, admin, member, online_svc) = setup_test_online_service_with_member().await;
        let raw_enabled = serde_json::json!({
            "bookSourceName": "带日志启用源",
            "bookSourceUrl": format!("http://{addr}"),
            "enabled": true,
            "searchUrl": "/search?q={{key}}",
            "ruleSearch": { "bookList": ".b", "name": "a@text", "bookUrl": "a@href" },
            "ruleBookInfo": {
                "name": "<js>java.log('SECRET_TOKEN_123'); java.getString('#n@text');</js>",
                "tocUrl": "#t@href"
            },
            "ruleToc": { "chapterList": "#c li a", "chapterName": "text", "chapterUrl": "href" },
            "ruleContent": { "content": "id.body@html" }
        })
        .to_string();
        let raw_disabled = serde_json::json!({
            "bookSourceName": "已禁用源",
            "bookSourceUrl": format!("http://{addr}#disabled"),
            "enabled": false,
            "searchUrl": "/search?q={{key}}",
            "ruleSearch": { "bookList": ".b", "name": "a@text", "bookUrl": "a@href" },
            "ruleBookInfo": { "name": "#n@text", "tocUrl": "#t@href" },
            "ruleToc": { "chapterList": "#c li a", "chapterName": "text", "chapterUrl": "href" },
            "ruleContent": { "content": "id.body@html" }
        })
        .to_string();
        let en_id = online_svc
            .import_sources(&admin, &raw_enabled)
            .await
            .unwrap()
            .sources[0]
            .id;
        let dis_id = online_svc
            .import_sources(&admin, &raw_disabled)
            .await
            .unwrap()
            .sources[0]
            .id;

        // Non-admin searching explicitly with disabled source_id must fail
        assert!(
            online_svc
                .search(&member, "书", Some(vec![dis_id]), 1)
                .await
                .is_err()
        );
        // Non-admin calling fetch_book_info / fetch_toc / fetch_chapter_content / add_to_shelf / start_download on disabled source -> Forbidden
        assert!(matches!(
            online_svc
                .fetch_book_info(&member, dis_id, &format!("http://{addr}/book/1"), None)
                .await,
            Err(DomainError::Forbidden(_))
        ));
        assert!(matches!(
            online_svc
                .fetch_toc(
                    &member,
                    dis_id,
                    &format!("http://{addr}/toc"),
                    Default::default()
                )
                .await,
            Err(DomainError::Forbidden(_))
        ));
        assert!(matches!(
            online_svc
                .fetch_chapter_content(
                    &member,
                    dis_id,
                    &format!("http://{addr}/ch/1"),
                    Default::default(),
                    Default::default(),
                    false
                )
                .await,
            Err(DomainError::Forbidden(_))
        ));
        assert!(matches!(
            online_svc
                .add_to_shelf(
                    &member,
                    AddOnlineShelfInput {
                        source_id: dis_id,
                        name: "书一".into(),
                        author: String::new(),
                        book_url: format!("http://{addr}/book/1"),
                        toc_url: format!("http://{addr}/toc"),
                        cover_url: None,
                        intro: None,
                        kind: None,
                        last_chapter: None,
                        total_chapters: None,
                    }
                )
                .await,
            Err(DomainError::Forbidden(_))
        ));

        // On enabled source: admin sees diagnostics.logs, non-admin gets logs stripped!
        let admin_info = online_svc
            .fetch_book_info(&admin, en_id, &format!("http://{addr}/book/1"), None)
            .await
            .unwrap();
        assert!(
            admin_info
                .diagnostics
                .logs
                .iter()
                .any(|l| l == "SECRET_TOKEN_123"),
            "admin should see diagnostics.logs"
        );
        let member_info = online_svc
            .fetch_book_info(&member, en_id, &format!("http://{addr}/book/1"), None)
            .await
            .unwrap();
        assert!(
            member_info.diagnostics.logs.is_empty(),
            "non-admin must have diagnostics.logs stripped"
        );

        server_handle.abort();
    }

    #[tokio::test]
    async fn switch_tracked_source_preserves_old_state_on_failure_and_updates_cover_on_success() {
        const TINY_PNG: &[u8] = &[
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9C, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00,
            0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ];
        let mock_app = Router::new()
            .route(
                "/toc-old",
                get(|| async { r#"<ul id="toc"><li><a href="/ch-old/1">旧第一章</a></li></ul>"# }),
            )
            .route(
                "/ch-old/1",
                get(|| async { r#"<div id="body"><p>旧源正文</p></div>"# }),
            )
            .route(
                "/toc-bad",
                get(|| async { r#"<ul id="toc"></ul>"# }),
            )
            .route(
                "/toc-new",
                get(|| async {
                    r#"<ul id="toc"><li><a href="/ch-new/1">新第一章</a></li><li><a href="/ch-new/2">新第二章</a></li></ul>"#
                }),
            )
            .route(
                "/cover.png",
                get(|| async { ([(axum::http::header::CONTENT_TYPE, "image/png")], TINY_PNG) }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_handle = tokio::spawn(async move {
            axum::serve(listener, mock_app).await.unwrap();
        });

        let (_tmp, admin, online_svc) = setup_test_online_service().await;
        let raw_sources = serde_json::json!([
            {
                "bookSourceName": "旧源",
                "bookSourceUrl": format!("http://{addr}"),
                "ruleToc": { "chapterList": "#toc li a", "chapterName": "text", "chapterUrl": "href" },
                "ruleContent": { "content": "id.body@html" }
            },
            {
                "bookSourceName": "新源",
                "bookSourceUrl": format!("http://{addr}#new"),
                "ruleToc": { "chapterList": "#toc li a", "chapterName": "text", "chapterUrl": "href" },
                "ruleContent": { "content": "id.body@html" }
            }
        ])
        .to_string();
        let imported = online_svc
            .import_sources(&admin, &raw_sources)
            .await
            .unwrap();
        let old_src = imported.sources[0].id;
        let new_src = imported.sources[1].id;

        let tracked = online_svc
            .track_online_book(
                &admin,
                TrackOnlineBookInput {
                    source_id: old_src,
                    name: "换源测试书".into(),
                    author: "作者".into(),
                    book_url: format!("http://{addr}/book-old"),
                    toc_url: format!("http://{addr}/toc-old"),
                    cover_url: None,
                    intro: None,
                    kind: None,
                    last_chapter: None,
                },
            )
            .await
            .unwrap();
        let _ = online_svc
            .get_tracked_chapter(tracked.id, 0, false)
            .await
            .unwrap();

        // Failing switch (empty TOC) with clear_cache: true must preserve old source & old cached chapter!
        assert!(
            online_svc
                .switch_tracked_source(
                    &admin,
                    tracked.id,
                    SwitchTrackedSourceInput {
                        source_id: new_src,
                        book_url: format!("http://{addr}/book-bad"),
                        toc_url: format!("http://{addr}/toc-bad"),
                        cover_url: None,
                        intro: None,
                        last_chapter: None,
                        clear_cache: true,
                    },
                )
                .await
                .is_err()
        );
        let still_old = online_svc.catalog.find_book_raw(tracked.id).await.unwrap();
        assert!(matches!(
            still_old.source,
            BookSource::OnlineTracked { source_id, .. } if source_id == old_src
        ));
        assert!(
            online_svc
                .tracked_store
                .load_chapter(tracked.id, 0)
                .is_some(),
            "old chapter cache must not be destroyed when new source TOC fails"
        );

        // Successful switch updates source, cover_key, and clears old cache
        let switched = online_svc
            .switch_tracked_source(
                &admin,
                tracked.id,
                SwitchTrackedSourceInput {
                    source_id: new_src,
                    book_url: format!("http://{addr}/book-new"),
                    toc_url: format!("http://{addr}/toc-new"),
                    cover_url: Some(format!("http://{addr}/cover.png")),
                    intro: Some("新简介".into()),
                    last_chapter: None,
                    clear_cache: true,
                },
            )
            .await
            .unwrap();
        assert_eq!(switched.source_id, new_src);
        assert_eq!(switched.total_chapters, 2);
        let updated_book = online_svc.catalog.find_book_raw(tracked.id).await.unwrap();
        assert!(
            updated_book.cover_key.is_some(),
            "cover_url should be applied on switch"
        );
        assert!(
            online_svc
                .tracked_store
                .load_chapter(tracked.id, 0)
                .is_none()
        );

        server_handle.abort();
    }

    #[tokio::test]
    async fn source_consistency_dedup_raw_json_sync_variable_and_credential_masking() {
        let (_tmp, admin, member, online_svc) = setup_test_online_service_with_member().await;
        // Duplicate key inside same import batch must collapse to 1 record
        let dup_batch = serde_json::json!([
            {
                "bookSourceName": "同批重复源",
                "bookSourceUrl": "https://dup.example",
                "enabled": true,
                "weight": 10,
                "loginUi": r#"[{"name":"口令","type":"password"},{"name":"自定义密钥","type":"text"},{"name":"昵称","type":"text"}]"#,
                "loginUrl": "function save(){ return 'ok'; }"
            },
            {
                "bookSourceName": "同批重复源",
                "bookSourceUrl": "https://dup.example",
                "bookSourceGroup": "更新分组",
                "enabled": true,
                "weight": 20,
                "loginUi": r#"[{"name":"口令","type":"password"},{"name":"自定义密钥","type":"text"},{"name":"昵称","type":"text"}]"#,
                "loginUrl": "function save(){ return 'ok'; }"
            }
        ])
        .to_string();
        let report = online_svc.import_sources(&admin, &dup_batch).await.unwrap();
        assert_eq!(report.imported_count, 1);
        assert_eq!(online_svc.list_sources(&admin).await.unwrap().len(), 1);
        let sid = report.sources[0].id;

        // update_source rejects multi-entry JSON array
        assert!(
            online_svc
                .update_source(
                    &admin,
                    sid,
                    UpdateBookSourceInput {
                        raw_json: Some(dup_batch.clone()),
                        ..Default::default()
                    },
                )
                .await
                .is_err()
        );

        // update_source syncs enabled/weight/group_name into raw_json and stores long variable
        let long_var = "x".repeat(500);
        let updated = online_svc
            .update_source(
                &admin,
                sid,
                UpdateBookSourceInput {
                    enabled: Some(false),
                    weight: Some(77),
                    group_name: Some("精校组".into()),
                    variable: Some(long_var.clone()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(updated.variable_preview.chars().count(), 240);
        let full_var = online_svc.get_source_variable(&admin, sid).await.unwrap();
        assert_eq!(full_var, long_var);
        assert!(matches!(
            online_svc.get_source_variable(&member, sid).await,
            Err(DomainError::Forbidden(_))
        ));

        let exported_single: serde_json::Value =
            serde_json::from_str(&online_svc.export_source_json(&admin, sid).await.unwrap())
                .unwrap();
        assert_eq!(exported_single["enabled"], false);
        assert_eq!(exported_single["weight"], 77);
        assert_eq!(exported_single["bookSourceGroup"], "精校组");

        // export_sources_batch_json deduplicates duplicate IDs
        let exported_batch: Vec<serde_json::Value> = serde_json::from_str(
            &online_svc
                .export_sources_batch_json(&admin, vec![sid, sid, sid])
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(exported_batch.len(), 1);

        // Credential masking covers loginUi password type + sensitive keywords, and login action merges
        let login1 = online_svc
            .run_login_action(
                &admin,
                sid,
                HashMap::from([
                    ("口令".into(), "secret-pass".into()),
                    ("自定义密钥".into(), "api-key-999".into()),
                    ("昵称".into(), "小明".into()),
                ]),
                Some("save()".to_string()),
            )
            .await
            .unwrap();
        assert_eq!(
            login1.source.login_info.get("口令").map(String::as_str),
            Some("••••••••")
        );
        assert_eq!(
            login1
                .source
                .login_info
                .get("自定义密钥")
                .map(String::as_str),
            Some("••••••••")
        );
        assert_eq!(
            login1.source.login_info.get("昵称").map(String::as_str),
            Some("小明")
        );

        // Partial second action preserves omitted credentials and masked values
        let login2 = online_svc
            .run_login_action(
                &admin,
                sid,
                HashMap::from([("口令".into(), "••••••••".into())]),
                Some(
                    "source.getLoginInfoMap()['口令'] + ':' + source.getLoginInfoMap()['自定义密钥']"
                        .to_string(),
                ),
            )
            .await
            .unwrap();
        assert_eq!(login2.result, "secret-pass:api-key-999");

        // reorder_sources syncs weight to raw_json without clobbering runtime_state
        let _ = online_svc.reorder_sources(&admin, vec![sid]).await.unwrap();
        let after_reorder: serde_json::Value =
            serde_json::from_str(&online_svc.export_source_json(&admin, sid).await.unwrap())
                .unwrap();
        assert_eq!(after_reorder["weight"], 10);
        assert_eq!(
            online_svc.get_source_variable(&admin, sid).await.unwrap(),
            long_var
        );
    }

    #[tokio::test]
    async fn test_source_debugger_edge_cases_diagnostics_large_toc_empty_urls_and_data_url() {
        let mock_app = Router::new()
            .route(
                "/toc-510",
                get(|| async {
                    let mut html = String::from(r#"<ul id="toc">"#);
                    for i in 0..510 {
                        html.push_str(&format!(r#"<li><a href="/ch/{i}">第{i}章</a></li>"#));
                    }
                    html.push_str("</ul>");
                    html
                }),
            )
            .route(
                "/toc-vol-only",
                get(|| async {
                    r#"<ul id="toc"><li><a>第一卷 仅卷名无链接</a></li></ul>"#
                }),
            )
            .route(
                "/ch/{num}",
                get(|AxumPath(num): AxumPath<usize>| async move {
                    format!(r#"<div id="body"><p>章节正文编号{num}</p></div>"#)
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_handle = tokio::spawn(async move {
            axum::serve(listener, mock_app).await.unwrap();
        });

        let (_tmp, admin, online_svc) = setup_test_online_service().await;
        // 1. Large TOC (>500 chapters) with chapter_index_override = 505 & data: URL direct jump
        let raw_large = serde_json::json!({
            "bookSourceName": "大目录调试源",
            "bookSourceUrl": format!("http://{addr}"),
            "ruleBookInfo": {
                "name": "<js>result.name</js>",
                "tocUrl": format!("<js>'http://{addr}/toc-510'</js>")
            },
            "ruleToc": {
                "chapterList": "#toc li a",
                "chapterName": "text",
                "chapterUrl": "href"
            },
            "ruleContent": { "content": "id.body@html" }
        })
        .to_string();
        let large_id = online_svc
            .import_sources(&admin, &raw_large)
            .await
            .unwrap()
            .sources[0]
            .id;

        let b64 = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            r#"{"name":"DataUrl书"}"#,
        );
        let report_large = online_svc
            .test_source(
                &admin,
                large_id,
                SourceTestInput {
                    keyword: Some(format!("data:;base64,{b64}")),
                    mode: SourceTestMode::Full,
                    chapter_index: Some(505),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert!(report_large.ok, "{report_large:?}");
        assert_eq!(report_large.steps[0].stage, "book_info");
        assert_eq!(report_large.toc_total_count, 510);
        assert_eq!(report_large.toc_chapters.len(), 500);
        assert_eq!(report_large.content_chapter_index, Some(505));
        assert!(
            report_large
                .content_text
                .as_deref()
                .unwrap()
                .contains("章节正文编号505")
        );

        // 2. Full mode where TOC has only volume entries (no valid chapter URL) -> overall_ok == false
        let raw_vol = serde_json::json!({
            "bookSourceName": "空链接目录源",
            "bookSourceUrl": format!("http://{addr}#vol"),
            "ruleBookInfo": {
                "name": "<js>'测试书'</js>",
                "tocUrl": format!("<js>'http://{addr}/toc-vol-only'</js>")
            },
            "ruleToc": {
                "chapterList": "#toc li a",
                "chapterName": "text",
                "chapterUrl": "href"
            },
            "ruleContent": { "content": "id.body@html" }
        })
        .to_string();
        let vol_id = online_svc
            .import_sources(&admin, &raw_vol)
            .await
            .unwrap()
            .sources[0]
            .id;
        let report_vol = online_svc
            .test_source(
                &admin,
                vol_id,
                SourceTestInput {
                    book_url: Some(format!("data:;base64,{b64}")),
                    mode: SourceTestMode::Full,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert!(
            !report_vol.ok,
            "Full mode with no valid chapter URL must fail"
        );
        assert!(
            report_vol
                .steps
                .iter()
                .any(|s| s.stage == "content" && !s.ok)
        );

        // 3. Failed stage preserves diagnostics.logs collected before error
        let raw_fail = serde_json::json!({
            "bookSourceName": "失败保留日志源",
            "bookSourceUrl": format!("http://{addr}#fail"),
            "searchUrl": "<js>java.log('TRACE_BEFORE_BOOM'); throw new Error('boom');</js>",
            "ruleSearch": { "bookList": ".x" }
        })
        .to_string();
        let fail_id = online_svc
            .import_sources(&admin, &raw_fail)
            .await
            .unwrap()
            .sources[0]
            .id;
        let report_fail = online_svc
            .test_source(
                &admin,
                fail_id,
                SourceTestInput {
                    mode: SourceTestMode::Search,
                    keyword: Some("测试".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert!(!report_fail.ok);
        assert!(
            report_fail
                .diagnostics
                .logs
                .iter()
                .any(|l| l == "TRACE_BEFORE_BOOM"),
            "failed stage must preserve collected diagnostics: {:?}",
            report_fail.diagnostics
        );

        server_handle.abort();
    }

    #[tokio::test]
    async fn concurrent_track_online_book_creates_single_book_and_delete_cleans_locks() {
        let mock_app = Router::new().route(
            "/toc",
            get(|| async {
                tokio::time::sleep(Duration::from_millis(60)).await;
                r#"<ul id="toc"><li><a href="/ch/1">第一章</a></li></ul>"#
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_handle = tokio::spawn(async move {
            axum::serve(listener, mock_app).await.unwrap();
        });

        let (_tmp, admin, online_svc) = setup_test_online_service().await;
        let raw_src = serde_json::json!({
            "bookSourceName": "并发建书源",
            "bookSourceUrl": format!("http://{addr}"),
            "ruleToc": { "chapterList": "#toc li a", "chapterName": "text", "chapterUrl": "href" },
            "ruleContent": { "content": "id.body@html" }
        })
        .to_string();
        let sid = online_svc
            .import_sources(&admin, &raw_src)
            .await
            .unwrap()
            .sources[0]
            .id;

        let make_input = || TrackOnlineBookInput {
            source_id: sid,
            name: "并发追更书".into(),
            author: "作者".into(),
            book_url: format!("http://{addr}/book/dup"),
            toc_url: format!("http://{addr}/toc"),
            cover_url: None,
            intro: None,
            kind: None,
            last_chapter: None,
        };
        let (r1, r2) = tokio::join!(
            online_svc.track_online_book(&admin, make_input()),
            online_svc.track_online_book(&admin, make_input()),
        );
        let b1 = r1.unwrap();
        let b2 = r2.unwrap();
        assert_eq!(
            b1.id, b2.id,
            "concurrent track_online_book must not create duplicates"
        );

        // Trigger lock entry creation, then delete book and verify lock maps are cleaned
        let _ = online_svc.sync_tracked_epub(b1.id, true).await.unwrap();
        assert!(
            online_svc
                .epub_sync_locks
                .lock()
                .unwrap()
                .contains_key(&b1.id)
        );
        online_svc.catalog.delete_book(&admin, b1.id).await.unwrap();
        assert!(
            !online_svc
                .epub_sync_locks
                .lock()
                .unwrap()
                .contains_key(&b1.id),
            "delete_book must clean up epub_sync_locks"
        );
        assert!(
            !online_svc
                .pending_sync_gens
                .lock()
                .unwrap()
                .contains_key(&b1.id),
            "delete_book must clean up pending_sync_gens"
        );

        server_handle.abort();
    }

    #[tokio::test]
    async fn sync_tracked_epub_preserves_dirty_flag_when_chapter_arrives_concurrently() {
        let mock_app = Router::new()
            .route(
                "/toc",
                get(|| async {
                    r#"<ul id="toc">
                        <li><a href="/ch/1">第一章</a></li>
                        <li><a href="/ch/2">第二章</a></li>
                    </ul>"#
                }),
            )
            .route(
                "/ch/{num}",
                get(|AxumPath(num): AxumPath<usize>| async move {
                    format!(r#"<div id="body"><p>第{num}章正文</p></div>"#)
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_handle = tokio::spawn(async move {
            axum::serve(listener, mock_app).await.unwrap();
        });

        let (_tmp, admin, online_svc) = setup_test_online_service().await;
        let raw_src = serde_json::json!({
            "bookSourceName": "同步竞态源",
            "bookSourceUrl": format!("http://{addr}"),
            "ruleToc": { "chapterList": "#toc li a", "chapterName": "text", "chapterUrl": "href" },
            "ruleContent": { "content": "id.body@html" }
        })
        .to_string();
        let sid = online_svc
            .import_sources(&admin, &raw_src)
            .await
            .unwrap()
            .sources[0]
            .id;

        let tracked = online_svc
            .track_online_book(
                &admin,
                TrackOnlineBookInput {
                    source_id: sid,
                    name: "竞态测试书".into(),
                    author: "作者".into(),
                    book_url: format!("http://{addr}/book/race"),
                    toc_url: format!("http://{addr}/toc"),
                    cover_url: None,
                    intro: None,
                    kind: None,
                    last_chapter: None,
                },
            )
            .await
            .unwrap();

        // Fetch ch0 and ch1 concurrently while running sync_tracked_epub
        let (c0, c1, _sync) = tokio::join!(
            online_svc.get_tracked_chapter(tracked.id, 0, false),
            online_svc.get_tracked_chapter(tracked.id, 1, false),
            online_svc.sync_tracked_epub(tracked.id, true),
        );
        c0.unwrap();
        c1.unwrap();

        // Now run final ensure_tracked_epub_synced; both chapters MUST end up in the EPUB
        online_svc
            .ensure_tracked_epub_synced(tracked.id)
            .await
            .unwrap();
        let final_book = online_svc.catalog.find_book_raw(tracked.id).await.unwrap();
        match final_book.source {
            BookSource::OnlineTracked {
                cached_chapters,
                epub_dirty,
                ..
            } => {
                assert_eq!(cached_chapters, 2);
                assert!(!epub_dirty);
            }
            _ => panic!("expected OnlineTracked"),
        }

        server_handle.abort();
    }
}

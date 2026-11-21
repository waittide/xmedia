use std::collections::{HashMap, VecDeque};
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
use crate::source::http::{get_or_build_async_client, validate_target_url};
use crate::source::model::{
    BookJsContext, ChapterJsContext, ConcurrentRateSpec, ExecutionDiagnostics, LegadoBookSource,
    LoginUiItem, OnlineBookDetail, OnlineChapterContent, OnlineChapterItem, SearchBookResult,
    SourceRuntimeState,
};
use crate::source::packager::{
    PackBookMetadata, PackChapterInput, package_chapters_to_epub, package_chapters_to_txt,
};
use crate::source::tracked_store::{TrackedBookStore, TrackedChapterFile, TrackedChapterMeta};

const SEARCH_MAX_CONCURRENCY: usize = 16;
const SEARCH_PER_SOURCE_TIMEOUT: Duration = Duration::from_secs(20);
const DEFAULT_DOWNLOAD_CONCURRENCY: usize = 4;
const DEFAULT_DOWNLOAD_MIN_INTERVAL_MS: u64 = 40;
const DOWNLOAD_STATE_PERSIST_BATCH: usize = 20;
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

pub struct BookOnlineService {
    sources: Arc<dyn BookSourceRepository>,
    shelves: Arc<dyn OnlineShelfRepository>,
    catalog: Arc<BookCatalogService>,
    engine: BookSourceEngine,
    cache_dir: PathBuf,
    download_dir: PathBuf,
    tracked_store: TrackedBookStore,
    epub_sync_locks: Arc<RwLock<HashMap<BookId, Arc<tokio::sync::Mutex<()>>>>>,
    pending_sync_gens: Arc<RwLock<HashMap<BookId, (u64, u32)>>>,
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

        Ok(Self {
            sources,
            shelves,
            catalog,
            engine,
            cache_dir,
            download_dir,
            tracked_store,
            epub_sync_locks: Arc::new(RwLock::new(HashMap::new())),
            pending_sync_gens: Arc::new(RwLock::new(HashMap::new())),
            downloads: Arc::new(RwLock::new(HashMap::new())),
            allow_private_ips,
        })
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
                let is_pwd = k.contains("密码") || k.to_ascii_lowercase().contains("password");
                masked.insert(
                    k.clone(),
                    if is_pwd && !v.is_empty() {
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

    async fn persist_runtime_state(
        &self,
        id: Uuid,
        new_state: SourceRuntimeState,
    ) -> DomainResult<()> {
        self.sources
            .update_runtime_state(id, new_state, Timestamp::now())
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
        for (idx, rec) in final_order.iter_mut().enumerate() {
            let target_weight = (total - idx as i64) * 10;
            if rec.weight != target_weight {
                rec.weight = target_weight;
                rec.updated_at = now;
                self.sources.save(rec).await?;
            }
        }

        Ok(final_order
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
        let mut out = Vec::with_capacity(parsed_batch.len());
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
                updated_count += 1;
                out.push(Self::record_to_summary(&rec, true));
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
                out.push(Self::record_to_summary(&rec, true));
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
            let (parsed, clean_raw) = batch.remove(0);
            rec.name = parsed.book_source_name;
            rec.source_url = parsed.book_source_url;
            rec.group_name = parsed.book_source_group.unwrap_or_default();
            rec.source_type = parsed.book_source_type;
            rec.raw_json = clean_raw;
        }
        if let Some(en) = input.enabled {
            rec.enabled = en;
        }
        if let Some(w) = input.weight {
            rec.weight = w;
        }
        if let Some(g) = input.group_name {
            rec.group_name = g;
        }
        if let Some(v) = input.variable {
            rec.runtime_state.variable = v;
        }
        rec.updated_at = now;

        self.sources.save(&rec).await?;
        Ok(Self::record_to_summary(&rec, true))
    }

    pub async fn delete_source(&self, actor: &User, id: Uuid) -> DomainResult<()> {
        require_admin(actor)?;
        let _ = self.load_source(id).await?;
        self.sources.delete(id).await?;
        let src_cache = self.cache_dir.join(id.to_string());
        if src_cache.is_dir() {
            let _ = std::fs::remove_dir_all(src_cache);
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
        let by_id: HashMap<Uuid, BookSourceRecord> = all.into_iter().map(|r| (r.id, r)).collect();
        let mut arr: Vec<serde_json::Value> = Vec::new();
        for id in ids {
            if let Some(rec) = by_id.get(&id)
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
        let all = self.sources.list_all().await?;
        let mut by_id: HashMap<Uuid, BookSourceRecord> =
            all.into_iter().map(|r| (r.id, r)).collect();

        let now = Timestamp::now();
        let mut affected_count = 0usize;

        for id in input.ids {
            let Some(mut rec) = by_id.remove(&id) else {
                continue;
            };
            match &input.action {
                BatchBookSourceAction::Enable => {
                    rec.enabled = true;
                    if let Ok(mut val) = serde_json::from_str::<serde_json::Value>(&rec.raw_json)
                        && let Some(obj) = val.as_object_mut()
                    {
                        obj.insert("enabled".into(), serde_json::Value::Bool(true));
                        if let Ok(s) = serde_json::to_string(&val) {
                            rec.raw_json = s;
                        }
                    }
                    rec.updated_at = now;
                    self.sources.save(&rec).await?;
                    affected_count += 1;
                }
                BatchBookSourceAction::Disable => {
                    rec.enabled = false;
                    if let Ok(mut val) = serde_json::from_str::<serde_json::Value>(&rec.raw_json)
                        && let Some(obj) = val.as_object_mut()
                    {
                        obj.insert("enabled".into(), serde_json::Value::Bool(false));
                        if let Ok(s) = serde_json::to_string(&val) {
                            rec.raw_json = s;
                        }
                    }
                    rec.updated_at = now;
                    self.sources.save(&rec).await?;
                    affected_count += 1;
                }
                BatchBookSourceAction::SetGroup { group_name } => {
                    let trimmed = group_name.trim().to_string();
                    rec.group_name = trimmed.clone();
                    if let Ok(mut val) = serde_json::from_str::<serde_json::Value>(&rec.raw_json)
                        && let Some(obj) = val.as_object_mut()
                    {
                        if trimmed.is_empty() {
                            obj.remove("bookSourceGroup");
                        } else {
                            obj.insert(
                                "bookSourceGroup".into(),
                                serde_json::Value::String(trimmed),
                            );
                        }
                        if let Ok(s) = serde_json::to_string(&val) {
                            rec.raw_json = s;
                        }
                    }
                    rec.updated_at = now;
                    self.sources.save(&rec).await?;
                    affected_count += 1;
                }
                BatchBookSourceAction::Delete => {
                    self.sources.delete(id).await?;
                    let src_cache = self.cache_dir.join(id.to_string());
                    if src_cache.is_dir() {
                        let _ = std::fs::remove_dir_all(src_cache);
                    }
                    affected_count += 1;
                }
            }
        }

        Ok(BatchBookSourcesReportDto { affected_count })
    }

    pub async fn run_login_action(
        &self,
        actor: &User,
        id: Uuid,
        mut login_info: HashMap<String, String>,
        action: Option<String>,
    ) -> DomainResult<LoginActionResponseDto> {
        require_admin(actor)?;
        let rec = self.load_source(id).await?;
        let parsed: LegadoBookSource = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("书源 JSON 无效: {e}")))?;

        // Preserve existing real password if frontend echoed masked placeholder back
        for (k, v) in login_info.iter_mut() {
            if v == "••••••••"
                && let Some(orig) = rec.runtime_state.login_info.get(k)
            {
                *v = orig.clone();
            }
        }

        let engine = self.engine.clone();
        let prev_state = rec.runtime_state.clone();
        let action_clone = action.clone();

        let out = tokio::task::spawn_blocking(move || {
            engine.run_login_action_sync(&parsed, prev_state, login_info, action_clone.as_deref())
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
        let mut rec = self.load_source(id).await?;
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

        let is_http_url = |s: &str| s.starts_with("http://") || s.starts_with("https://");

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

        let (final_state, report) = tokio::task::spawn_blocking(move || {
            fn merge_diag(acc: &mut ExecutionDiagnostics, d: &ExecutionDiagnostics) {
                acc.logs.extend(d.logs.iter().cloned());
                acc.toasts.extend(d.toasts.iter().cloned());
                acc.browser_urls.extend(d.browser_urls.iter().cloned());
            }

            let total_start = Instant::now();
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

            // In Full mode, pasting a direct URL into the keyword box jumps straight to book_info (Legado behavior)
            let skip_search_for_url = mode == SourceTestMode::Full
                && (next_book_url.is_some() || raw_input.as_deref().is_some_and(is_http_url));
            if skip_search_for_url && next_book_url.is_none() {
                next_book_url = raw_input.clone();
            }

            // Stage 1: ruleSearch
            if mode == SourceTestMode::Search
                || (mode == SourceTestMode::Full && !skip_search_for_url)
            {
                let t0 = Instant::now();
                match engine.search_sync(&parsed, state.clone(), &kw_clone, 1) {
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
                        steps.push(SourceDebugStepDto {
                            stage: "search".into(),
                            label: "搜索规则 (ruleSearch)".into(),
                            ok: false,
                            elapsed_ms,
                            summary: "搜索请求或解析失败".into(),
                            error: Some(err),
                            diagnostics: ExecutionDiagnostics::default(),
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
                match engine.fetch_book_info_sync(
                    &parsed,
                    state.clone(),
                    b_url,
                    next_fallback_hint.as_ref(),
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
                        steps.push(SourceDebugStepDto {
                            stage: "book_info".into(),
                            label: "详情规则 (ruleBookInfo)".into(),
                            ok: false,
                            elapsed_ms,
                            summary: "详情请求或解析失败".into(),
                            error: Some(err),
                            diagnostics: ExecutionDiagnostics::default(),
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

            if run_toc && let Some(ref t_url) = next_toc_url {
                let t0 = Instant::now();
                match engine.fetch_toc_sync(&parsed, state.clone(), t_url, &book_ctx) {
                    Ok(out) => {
                        let elapsed_ms = t0.elapsed().as_millis() as u64;
                        state = out.state;
                        merge_diag(&mut all_diag, &out.diagnostics);
                        toc_total_count = out.data.len();
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
                        toc_chapters = out.data.into_iter().take(500).collect();

                        if next_chapter_url.is_none() {
                            let chosen = chapter_index_override
                                .and_then(|idx| toc_chapters.iter().find(|c| c.index == idx))
                                .or_else(|| {
                                    toc_chapters
                                        .iter()
                                        .find(|c| !c.is_volume && !c.url.is_empty())
                                })
                                .or_else(|| toc_chapters.first());
                            if let Some(ch) = chosen {
                                next_chapter_url = Some(ch.url.clone());
                                next_chapter_title = ch.title.clone();
                                next_chapter_idx = ch.index;
                            }
                        }

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
                        steps.push(SourceDebugStepDto {
                            stage: "toc".into(),
                            label: "目录规则 (ruleToc)".into(),
                            ok: false,
                            elapsed_ms,
                            summary: "目录请求或解析失败".into(),
                            error: Some(err),
                            diagnostics: ExecutionDiagnostics::default(),
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

                match engine.fetch_chapter_content_sync(
                    &parsed,
                    state.clone(),
                    c_url,
                    &content_book_ctx,
                    &ch_ctx,
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
                        content_chapter_index = Some(next_chapter_idx);
                        content_chapter_title =
                            (!next_chapter_title.is_empty()).then_some(next_chapter_title.clone());
                        content_chapter_url = Some(c_url.clone());
                        steps.push(SourceDebugStepDto {
                            stage: "content".into(),
                            label: "正文规则 (ruleContent)".into(),
                            ok: false,
                            elapsed_ms,
                            summary: "正文请求或解析失败".into(),
                            error: Some(err),
                            diagnostics: ExecutionDiagnostics::default(),
                        });
                    }
                }
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
        })
        .await
        .map_err(|e| DomainError::Storage(format!("测试书源线程失败: {e}")))?;

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

        rec.runtime_state = final_state;
        rec.last_test_ok = Some(report.ok);
        rec.last_test_msg = Some(last_msg);
        rec.updated_at = now;
        self.sources.save(&rec).await?;

        Ok(report)
    }

    pub async fn search(
        &self,
        keyword: &str,
        source_ids: Option<Vec<Uuid>>,
        page: u32,
    ) -> DomainResult<AggregatedSearchResponseDto> {
        let kw = keyword.trim();
        if kw.is_empty() {
            return Err(DomainError::Validation("请输入搜索关键词".into()));
        }

        let mut sources = self.sources.list_all().await?;

        if let Some(filter_ids) = source_ids.filter(|v| !v.is_empty()) {
            sources.retain(|s| filter_ids.contains(&s.id));
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
                let timed = tokio::time::timeout(
                    SEARCH_PER_SOURCE_TIMEOUT,
                    tokio::task::spawn_blocking(move || {
                        engine.search_sync(&parsed, state, &kw_owned, page_num)
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
                    Err(_) => (
                        sid,
                        sname,
                        prev_state_cmp,
                        Err(format!(
                            "搜索超时（超过 {} 秒）",
                            SEARCH_PER_SOURCE_TIMEOUT.as_secs()
                        )),
                    ),
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
                        if out.state != prev_state {
                            let _ = self.persist_runtime_state(sid, out.state).await;
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

    pub async fn fetch_book_info(
        &self,
        source_id: Uuid,
        book_url: &str,
        fallback_hint: Option<BookJsContext>,
    ) -> DomainResult<OnlineBookDetailResponseDto> {
        let rec = self.load_source(source_id).await?;
        let parsed: LegadoBookSource = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("书源 JSON 无效: {e}")))?;

        let engine = self.engine.clone();
        let url_owned = book_url.to_string();
        let prev_state = rec.runtime_state.clone();
        let prev_state_cmp = prev_state.clone();

        let out = tokio::task::spawn_blocking(move || {
            engine.fetch_book_info_sync(&parsed, prev_state, &url_owned, fallback_hint.as_ref())
        })
        .await
        .map_err(|e| DomainError::Storage(format!("获取书籍详情失败: {e}")))?
        .map_err(DomainError::Validation)?;

        if out.state != prev_state_cmp {
            let _ = self.persist_runtime_state(source_id, out.state).await;
        }

        Ok(OnlineBookDetailResponseDto {
            source_id,
            source_name: rec.name,
            detail: out.data,
            diagnostics: out.diagnostics,
        })
    }

    pub async fn fetch_toc(
        &self,
        source_id: Uuid,
        toc_url: &str,
        book_ctx: BookJsContext,
    ) -> DomainResult<OnlineTocResponseDto> {
        let rec = self.load_source(source_id).await?;
        let parsed: LegadoBookSource = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("书源 JSON 无效: {e}")))?;

        let engine = self.engine.clone();
        let url_owned = toc_url.to_string();
        let prev_state = rec.runtime_state.clone();
        let prev_state_cmp = prev_state.clone();

        let out = tokio::task::spawn_blocking(move || {
            engine.fetch_toc_sync(&parsed, prev_state, &url_owned, &book_ctx)
        })
        .await
        .map_err(|e| DomainError::Storage(format!("获取目录失败: {e}")))?
        .map_err(DomainError::Validation)?;

        if out.state != prev_state_cmp {
            let _ = self.persist_runtime_state(source_id, out.state).await;
        }

        Ok(OnlineTocResponseDto {
            source_id,
            chapters: out.data,
            diagnostics: out.diagnostics,
        })
    }

    fn read_cached_chapter(
        &self,
        source_id: Uuid,
        chapter_url: &str,
    ) -> Option<OnlineChapterContent> {
        read_cached_chapter_at(&self.cache_dir, source_id, chapter_url)
    }

    fn write_cached_chapter(
        &self,
        source_id: Uuid,
        chapter_url: &str,
        content: &OnlineChapterContent,
    ) {
        write_cached_chapter_at(&self.cache_dir, source_id, chapter_url, content);
    }

    pub async fn fetch_chapter_content(
        &self,
        source_id: Uuid,
        chapter_url: &str,
        book_ctx: BookJsContext,
        chapter_ctx: ChapterJsContext,
        force_refresh: bool,
    ) -> DomainResult<OnlineContentResponseDto> {
        if !force_refresh && let Some(cached) = self.read_cached_chapter(source_id, chapter_url) {
            return Ok(OnlineContentResponseDto {
                source_id,
                chapter_index: chapter_ctx.index,
                content: cached,
                cached: true,
                diagnostics: ExecutionDiagnostics::default(),
            });
        }

        let rec = self.load_source(source_id).await?;
        let parsed: LegadoBookSource = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("书源 JSON 无效: {e}")))?;

        let engine = self.engine.clone();
        let url_owned = chapter_url.to_string();
        let prev_state = rec.runtime_state.clone();
        let prev_state_cmp = prev_state.clone();
        let ch_idx = chapter_ctx.index;

        let out = tokio::task::spawn_blocking(move || {
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

        if out.state != prev_state_cmp {
            let _ = self.persist_runtime_state(source_id, out.state).await;
        }
        self.write_cached_chapter(source_id, chapter_url, &out.data);

        Ok(OnlineContentResponseDto {
            source_id,
            chapter_index: ch_idx,
            content: out.data,
            cached: false,
            diagnostics: out.diagnostics,
        })
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
        user_id: UserId,
        input: AddOnlineShelfInput,
    ) -> DomainResult<OnlineShelfBookDto> {
        let src = self.load_source(input.source_id).await?;
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

    async fn epub_lock_for(&self, book_id: BookId) -> Arc<tokio::sync::Mutex<()>> {
        {
            let guard = self.epub_sync_locks.read().await;
            if let Some(lock) = guard.get(&book_id) {
                return Arc::clone(lock);
            }
        }
        let mut guard = self.epub_sync_locks.write().await;
        Arc::clone(
            guard
                .entry(book_id)
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(()))),
        )
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
        let _src = self.load_source(input.source_id).await?;

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
                .fetch_book_info(
                    input.source_id,
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
                .fetch_toc(
                    input.source_id,
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
                let _ =
                    self.tracked_store
                        .update_manifest_from_toc(book_id, &clean_chapters, false);
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
        let mut book = self.catalog.find_book_raw(book_id).await?;
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

        let source_name = self
            .load_source(source_id)
            .await
            .map(|s| s.name)
            .unwrap_or_else(|_| "已删除书源".to_string());

        let existing_manifest = self.tracked_store.load_manifest(book_id);
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

        let effective_toc_url = if toc_url.trim().is_empty() {
            let info = self
                .fetch_book_info(
                    source_id,
                    &book_url,
                    Some(BookJsContext {
                        name: book.metadata.title.clone(),
                        author: book.metadata.authors.join(", "),
                        book_url: book_url.clone(),
                        toc_url: String::new(),
                        intro: book.metadata.description.clone(),
                        ..Default::default()
                    }),
                )
                .await?;
            info.detail.toc_url
        } else {
            toc_url.clone()
        };

        let toc_res = self
            .fetch_toc(
                source_id,
                &effective_toc_url,
                BookJsContext {
                    name: book.metadata.title.clone(),
                    author: book.metadata.authors.join(", "),
                    book_url: book_url.clone(),
                    toc_url: effective_toc_url.clone(),
                    intro: book.metadata.description.clone(),
                    ..Default::default()
                },
            )
            .await?;

        let clean_chapters: Vec<OnlineChapterItem> = toc_res
            .chapters
            .into_iter()
            .filter(|c| !c.is_volume && !c.url.trim().is_empty())
            .collect();

        let manifest = self
            .tracked_store
            .update_manifest_from_toc(book_id, &clean_chapters, true)
            .map_err(DomainError::Storage)?;

        let new_total = manifest.chapters.len() as u32;
        let new_cached = manifest.chapters.iter().filter(|c| c.cached).count() as u32;
        let new_last = manifest.chapters.last().map(|c| c.title.clone());

        if let BookSource::OnlineTracked {
            toc_url: ref mut t_url,
            last_chapter: ref mut l_ch,
            cached_chapters: ref mut c_cnt,
            total_chapters: ref mut t_cnt,
            ..
        } = book.source
        {
            *t_url = effective_toc_url;
            *l_ch = new_last.clone();
            *c_cnt = new_cached;
            *t_cnt = new_total;
        }
        book.metadata.chapter_count = if new_total > 0 { Some(new_total) } else { None };
        book.updated_at = Timestamp::now();
        self.catalog.save_book_raw(&book).await?;

        Ok(TrackedTocResponseDto {
            book_id,
            source_id,
            source_name,
            chapters: manifest.chapters,
            cached_chapters: new_cached,
            total_chapters: new_total,
            last_chapter: new_last,
            epub_dirty,
        })
    }

    /// Reads a single chapter of an online-tracked book:
    /// serves immediately from local chapter cache if present, otherwise fetches from the Legado
    /// source, saves O(1) chapter file atomically, marks `epub_dirty`, and schedules EPUB 3.0 sync.
    pub async fn get_tracked_chapter(
        self: &Arc<Self>,
        book_id: BookId,
        chapter_index: usize,
        force_refresh: bool,
    ) -> DomainResult<TrackedChapterResponseDto> {
        let mut book = self.catalog.find_book_raw(book_id).await?;
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

        if !force_refresh
            && let Some(cached) = self.tracked_store.load_chapter(book_id, chapter_index)
            && !cached.content.trim().is_empty()
        {
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

        let mut manifest = self.tracked_store.load_manifest(book_id);
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

        let fetched = self
            .fetch_chapter_content(
                source_id,
                &chapter_meta.url,
                book_ctx,
                chapter_ctx,
                force_refresh,
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
        let new_cached = self
            .tracked_store
            .save_chapter(book_id, &chapter_file)
            .map_err(DomainError::Storage)?;
        let new_total = manifest.chapters.len() as u32;

        if let BookSource::OnlineTracked {
            cached_chapters: ref mut c_cnt,
            total_chapters: ref mut t_cnt,
            epub_dirty: ref mut dirty,
            ..
        } = book.source
        {
            *c_cnt = new_cached;
            *t_cnt = new_total.max(*t_cnt);
            *dirty = true;
        }
        book.updated_at = Timestamp::now();
        self.catalog.save_book_raw(&book).await?;

        self.schedule_debounced_epub_sync(book_id).await;

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
    pub async fn switch_tracked_source(
        self: &Arc<Self>,
        actor: &User,
        book_id: BookId,
        input: SwitchTrackedSourceInput,
    ) -> DomainResult<TrackedTocResponseDto> {
        require_admin(actor)?;
        let _src = self.load_source(input.source_id).await?;
        let mut book = self.catalog.find_book_raw(book_id).await?;

        if !book.source.is_online_tracked() {
            return Err(DomainError::Validation("该书籍不是在线追更书籍".into()));
        }

        if input.clear_cache {
            self.tracked_store.clear_chapters(book_id);
        }

        if let BookSource::OnlineTracked {
            source_id,
            book_url,
            toc_url,
            last_chapter,
            epub_dirty,
            ..
        } = &mut book.source
        {
            *source_id = input.source_id;
            *book_url = input.book_url;
            *toc_url = input.toc_url;
            if input.last_chapter.is_some() {
                *last_chapter = input.last_chapter;
            }
            *epub_dirty = true;
        }
        if let Some(intro) = input.intro
            && !intro.trim().is_empty()
        {
            book.metadata.description = Some(intro.trim().to_string());
        }
        book.updated_at = Timestamp::now();
        self.catalog.save_book_raw(&book).await?;

        self.get_tracked_toc(book_id, true).await
    }

    /// Schedules a debounced background standard EPUB 3.0 rebuild so rapid page turns
    /// do not cause ZIP write amplification. Flushes immediately every 20 newly cached chapters.
    async fn schedule_debounced_epub_sync(self: &Arc<Self>, book_id: BookId) {
        let (sync_gen, should_flush_now) = {
            let mut guard = self.pending_sync_gens.write().await;
            let entry = guard.entry(book_id).or_insert((0, 0));
            entry.0 = entry.0.wrapping_add(1);
            entry.1 = entry.1.saturating_add(1);
            let flush_now = entry.1 >= TRACKED_EPUB_IMMEDIATE_BATCH;
            if flush_now {
                entry.1 = 0;
            }
            (entry.0, flush_now)
        };

        let svc = Arc::clone(self);
        tokio::spawn(async move {
            if !should_flush_now {
                tokio::time::sleep(TRACKED_EPUB_DEBOUNCE).await;
                let current_gen = svc
                    .pending_sync_gens
                    .read()
                    .await
                    .get(&book_id)
                    .map(|(g, _)| *g)
                    .unwrap_or(0);
                if current_gen != sync_gen {
                    return;
                }
            }
            if let Err(err) = svc.ensure_tracked_epub_synced(book_id).await {
                tracing::warn!(book_id = %book_id, error = %err, "background EPUB 3.0 sync failed");
            }
        });
    }

    /// Ensures the standard EPUB 3.0 archive on disk reflects all cached chapters if `epub_dirty`.
    pub async fn ensure_tracked_epub_synced(&self, book_id: BookId) -> DomainResult<()> {
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
    pub async fn sync_tracked_epub(
        &self,
        book_id: BookId,
        force: bool,
    ) -> DomainResult<BookSummaryDto> {
        let lock = self.epub_lock_for(book_id).await;
        let _guard = lock.lock().await;

        let mut book = self.catalog.find_book_raw(book_id).await?;
        let is_dirty = match &book.source {
            BookSource::OnlineTracked { epub_dirty, .. } => *epub_dirty,
            _ => {
                return Err(DomainError::Validation("该书籍不是在线追更书籍".into()));
            }
        };

        if !force && !is_dirty {
            return self.catalog.get_book(None, book_id).await;
        }

        let (chapters, cached_count, total_count) =
            self.tracked_store.collect_cached_chapters_for_epub(book_id);
        let cover_bytes = match book.cover_key.as_deref() {
            Some(key) => self.catalog.read_cover_bytes(key).await,
            None => None,
        };

        let pack_meta = PackBookMetadata {
            identifier: Some(book_id.0),
            title: book.metadata.title.clone(),
            author: book.metadata.authors.join(", "),
            description: book.metadata.description.clone(),
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

        book.file_size = stored.file_size;
        book.file_hash = stored.file_hash;
        if let BookSource::OnlineTracked {
            storage_key,
            cached_chapters,
            total_chapters,
            epub_dirty,
            ..
        } = &mut book.source
        {
            *storage_key = stored.storage_key;
            *cached_chapters = cached_count;
            if total_count > 0 {
                *total_chapters = total_count;
            }
            *epub_dirty = false;
        }
        book.metadata.chapter_count = if total_count > 0 {
            Some(total_count)
        } else if cached_count > 0 {
            Some(cached_count)
        } else {
            book.metadata.chapter_count
        };
        book.updated_at = Timestamp::now();
        self.catalog.save_book_raw(&book).await?;

        if let Some(entry) = self.pending_sync_gens.write().await.get_mut(&book_id) {
            entry.1 = 0;
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
        if input.import_to_library {
            require_admin(actor)?;
        }
        let src = self.load_source(input.source_id).await?;
        let job_id = Uuid::now_v7();
        let now = Timestamp::now();

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
        let src_rec = match self.load_source(input.source_id).await {
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
        let reusable_ctx = match self
            .engine
            .prepare_reusable_context(Arc::clone(&parsed_source), src_rec.runtime_state)
        {
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
            res = toc_task => match res {
                Ok(inner) => inner,
                Err(e) => Err(format!("获取目录线程失败: {e}")),
            },
            _ = cancel_notify.notified() => {
                self.mark_job_cancelled(job_id, started_at).await;
                return;
            }
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
            .map(|e| (e + 1).min(all_chapters.len()))
            .unwrap_or(all_chapters.len())
            .max(start);
        let selected: Vec<OnlineChapterItem> = all_chapters[start..end].to_vec();

        // Check whether this download targets an existing unified online-tracked Book
        let target_tracked_book_id: Option<BookId> = if let Some(bid) = input.book_id {
            self.catalog
                .find_book_raw(BookId(bid))
                .await
                .ok()
                .filter(|b| b.source.is_online_tracked())
                .map(|b| b.id)
        } else if input.import_to_library && input.format == OnlineDownloadFormat::Epub {
            self.catalog
                .find_online_tracked_book(input.source_id, &input.book_url)
                .await
                .ok()
                .flatten()
                .map(|b| b.id)
        } else {
            None
        };

        if let Some(bid) = target_tracked_book_id {
            let _ = self
                .tracked_store
                .update_manifest_from_toc(bid, &all_chapters, true);
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

        loop {
            if cancel_flag.load(Ordering::Acquire) {
                was_cancelled = true;
                break;
            }
            tokio::select! {
                biased;
                _ = cancel_notify.notified() => {
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
                            let _ = self.tracked_store.save_chapter(
                                bid,
                                &TrackedChapterFile {
                                    index: global_idx,
                                    title: ev.packed.title.clone(),
                                    url: chapter_url,
                                    content: ev.packed.content.clone(),
                                },
                            );
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
                        if snap != last_persisted_state {
                            if self.persist_runtime_state(input.source_id, snap.clone()).await.is_ok() {
                                last_persisted_state = snap;
                            }
                        }
                    }
                }
            }
        }

        // Persist any final runtime_state changes once at end of chapter fetching
        let final_snap = reusable_ctx.snapshot_state();
        if final_snap != last_persisted_state {
            let _ = self
                .persist_runtime_state(input.source_id, final_snap)
                .await;
        }

        if was_cancelled || cancel_flag.load(Ordering::Acquire) {
            self.mark_job_cancelled(job_id, started_at).await;
            let job_dir = self.download_dir.join(job_id.to_string());
            let _ = tokio::fs::remove_dir_all(&job_dir).await;
            return;
        }

        for h in worker_handles {
            let _ = h.await;
        }

        if cancel_flag.load(Ordering::Acquire) {
            self.mark_job_cancelled(job_id, started_at).await;
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
        let validated = validate_target_url(raw_url.trim(), self.allow_private_ips)
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
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string(content) {
        let _ = std::fs::write(path, json);
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
            users_repo,
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
        (tmp, admin, online_svc)
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
}

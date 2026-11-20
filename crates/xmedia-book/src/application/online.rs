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
use crate::domain::book::BookId;
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

const SEARCH_MAX_CONCURRENCY: usize = 16;
const SEARCH_PER_SOURCE_TIMEOUT: Duration = Duration::from_secs(20);
const DEFAULT_DOWNLOAD_CONCURRENCY: usize = 4;
const DEFAULT_DOWNLOAD_MIN_INTERVAL_MS: u64 = 40;
const DOWNLOAD_STATE_PERSIST_BATCH: usize = 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookSourceSummaryDto {
    pub id: Uuid,
    pub name: String,
    pub source_url: String,
    pub group_name: String,
    pub comment: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginActionResponseDto {
    pub result: String,
    pub diagnostics: ExecutionDiagnostics,
    pub source: BookSourceSummaryDto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceTestReportDto {
    pub ok: bool,
    pub keyword: String,
    pub result_count: usize,
    pub sample_titles: Vec<String>,
    pub error: Option<String>,
    pub diagnostics: ExecutionDiagnostics,
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
pub struct StartDownloadInput {
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

        Ok(Self {
            sources,
            shelves,
            catalog,
            engine,
            cache_dir,
            download_dir,
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
        rows.sort_by(|a, b| {
            b.enabled
                .cmp(&a.enabled)
                .then_with(|| b.weight.cmp(&a.weight))
                .then_with(|| a.name.cmp(&b.name))
        });
        Ok(rows
            .iter()
            .map(|r| Self::record_to_summary(r, actor.role.is_admin()))
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
        keyword_override: Option<String>,
    ) -> DomainResult<SourceTestReportDto> {
        require_admin(actor)?;
        let mut rec = self.load_source(id).await?;
        let parsed: LegadoBookSource = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("书源 JSON 无效: {e}")))?;

        let kw = keyword_override
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .or_else(|| {
                parsed
                    .rule_search
                    .as_ref()
                    .and_then(|r| r.check_key_word.clone())
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
            })
            .unwrap_or_else(|| "我的".to_string());

        let engine = self.engine.clone();
        let prev_state = rec.runtime_state.clone();
        let kw_clone = kw.clone();
        let res = tokio::task::spawn_blocking(move || {
            engine.search_sync(&parsed, prev_state, &kw_clone, 1)
        })
        .await
        .map_err(|e| DomainError::Storage(format!("测试书源线程失败: {e}")))?;

        let now = Timestamp::now();
        match res {
            Ok(out) => {
                let sample_titles: Vec<String> =
                    out.data.iter().take(5).map(|b| b.name.clone()).collect();
                let count = out.data.len();
                let msg = format!("命中 {count} 本（关键词：{kw}）");
                rec.runtime_state = out.state;
                rec.last_test_ok = Some(count > 0);
                rec.last_test_msg = Some(msg);
                rec.updated_at = now;
                self.sources.save(&rec).await?;
                Ok(SourceTestReportDto {
                    ok: count > 0,
                    keyword: kw,
                    result_count: count,
                    sample_titles,
                    error: if count == 0 {
                        Some("搜索未返回条目，请检查是否需要登录或更换线路".into())
                    } else {
                        None
                    },
                    diagnostics: out.diagnostics,
                })
            }
            Err(err) => {
                rec.last_test_ok = Some(false);
                rec.last_test_msg = Some(err.chars().take(180).collect());
                rec.updated_at = now;
                self.sources.save(&rec).await?;
                Ok(SourceTestReportDto {
                    ok: false,
                    keyword: kw,
                    result_count: 0,
                    sample_titles: Vec::new(),
                    error: Some(err),
                    diagnostics: ExecutionDiagnostics::default(),
                })
            }
        }
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
            j.current_chapter_title = Some("正在封装电子书文件…".into());
            j.elapsed_ms = started_at.elapsed().as_millis() as u64;
            j.chapters_per_sec = compute_chapters_per_sec(j.processed_chapters, j.elapsed_ms);
        })
        .await;

        let pack_meta = PackBookMetadata {
            title: input.book_name.clone(),
            author: input.author.clone(),
            description: input.intro.clone(),
            cover_bytes,
        };
        let fmt = input.format;
        let packaged = tokio::task::spawn_blocking(move || match fmt {
            OnlineDownloadFormat::Epub => package_chapters_to_epub(&pack_meta, &packed_chapters),
            OnlineDownloadFormat::Txt => Ok(package_chapters_to_txt(&pack_meta, &packed_chapters)),
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

        let ext = match input.format {
            OnlineDownloadFormat::Epub => "epub",
            OnlineDownloadFormat::Txt => "txt",
        };
        let safe_stem = sanitize_file_stem(&input.book_name);
        let file_name = format!("{safe_stem}.{ext}");
        let file_size = bytes.len() as u64;

        let mut imported_book_id = None;
        let mut output_path = None;

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
                    // Do not write a redundant second copy in `online_downloads/{job_id}`;
                    // downloads for imported books stream directly from the catalog storage.
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
    } else if bytes.starts_with(b"BM") {
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
}

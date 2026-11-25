mod debug;
mod downloads;
mod dto;
mod helpers;
mod reader;
mod shelf;
mod sources;
mod tracked;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::RwLock;
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};

use crate::application::catalog::BookCatalogService;
use crate::domain::book::BookId;
use crate::domain::repository::{BookSourceRepository, OnlineShelfRepository};
use crate::source::BookSourceEngine;
use crate::source::tracked_store::TrackedBookStore;

pub use dto::*;
pub(crate) use helpers::validate_safe_cover_image;
use helpers::{AsyncLockMap, StoredDownloadJob};

const SEARCH_MAX_CONCURRENCY: usize = 16;
const SEARCH_PER_SOURCE_TIMEOUT: Duration = Duration::from_secs(20);
const DETAIL_OP_TIMEOUT: Duration = Duration::from_secs(45);
const SOURCE_TEST_TOTAL_TIMEOUT: Duration = Duration::from_secs(90);
const DEFAULT_DOWNLOAD_CONCURRENCY: usize = 4;
const DEFAULT_DOWNLOAD_MIN_INTERVAL_MS: u64 = 40;
const DOWNLOAD_STATE_PERSIST_BATCH: usize = 20;
const MAX_ACTIVE_DOWNLOADS_PER_USER: usize = 3;
const FINISHED_DOWNLOAD_TTL_SECS: i64 = 24 * 3600;
const TRACKED_EPUB_DEBOUNCE: Duration = Duration::from_secs(6);
const TRACKED_EPUB_IMMEDIATE_BATCH: u32 = 20;

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
    seed_import_lock: tokio::sync::Mutex<()>,
    downloads: Arc<RwLock<HashMap<Uuid, StoredDownloadJob>>>,
    allow_private_ips: bool,
    op_timeout: Duration,
    detail_timeout: Duration,
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
        // Clean up any leftover per-job download directories from previous server runs.
        if let Ok(entries) = std::fs::read_dir(&download_dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    let _ = std::fs::remove_dir_all(&p);
                } else {
                    let _ = std::fs::remove_file(&p);
                }
            }
        }
        let tracked_store = TrackedBookStore::new(data_dir).map_err(DomainError::Storage)?;
        let book_state_locks: AsyncLockMap<BookId> = Arc::new(Mutex::new(HashMap::new()));
        let epub_sync_locks: AsyncLockMap<BookId> = Arc::new(Mutex::new(HashMap::new()));
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
            seed_import_lock: tokio::sync::Mutex::new(()),
            downloads: Arc::new(RwLock::new(HashMap::new())),
            allow_private_ips,
            op_timeout: SEARCH_PER_SOURCE_TIMEOUT,
            detail_timeout: DETAIL_OP_TIMEOUT,
        })
    }

    #[cfg(test)]
    pub(crate) fn with_timeouts(mut self, op_timeout: Duration, detail_timeout: Duration) -> Self {
        self.op_timeout = op_timeout;
        self.detail_timeout = detail_timeout;
        self
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

    #[cfg(test)]
    pub(crate) fn has_in_memory_book_state(&self, book_id: BookId) -> bool {
        let has_state = self
            .book_state_locks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains_key(&book_id);
        let has_sync = self
            .epub_sync_locks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains_key(&book_id);
        let has_gen = self
            .pending_sync_gens
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains_key(&book_id);
        has_state || has_sync || has_gen
    }
}

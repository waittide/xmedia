mod debug;
mod dto;
mod helpers;
mod reader;
mod sources;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use uuid::Uuid;

use crate::domain::repository::ComicSourceRepository;
use crate::source::ComicSourceEngine;

pub use dto::*;
use helpers::AsyncLockMap;

const SEARCH_MAX_CONCURRENCY: usize = 16;
const SEARCH_PER_SOURCE_TIMEOUT: Duration = Duration::from_secs(20);
const READER_OP_TIMEOUT: Duration = Duration::from_secs(45);
const SOURCE_TEST_TOTAL_TIMEOUT: Duration = Duration::from_secs(90);
const COMIC_IMAGE_PROXY_MAX_BYTES: usize = 16 * 1024 * 1024;

pub struct ComicOnlineService {
    sources: Arc<dyn ComicSourceRepository>,
    engine: ComicSourceEngine,
    source_state_locks: AsyncLockMap<Uuid>,
    seed_import_lock: tokio::sync::Mutex<()>,
    allow_private_ips: bool,
}

impl ComicOnlineService {
    pub fn new(sources: Arc<dyn ComicSourceRepository>) -> Self {
        Self::with_engine(sources, ComicSourceEngine::default(), false)
    }

    pub fn with_engine(
        sources: Arc<dyn ComicSourceRepository>,
        engine: ComicSourceEngine,
        allow_private_ips: bool,
    ) -> Self {
        Self {
            sources,
            engine,
            source_state_locks: Arc::new(Mutex::new(HashMap::new())),
            seed_import_lock: tokio::sync::Mutex::new(()),
            allow_private_ips,
        }
    }
}

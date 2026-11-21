mod browse;
mod debug;
mod dto;
mod proxy;
mod sources;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use uuid::Uuid;

pub use dto::*;
pub use proxy::AudioProxyResponse;

use crate::domain::repository::MusicSourceRepository;
use crate::source::MusicSourceEngine;
use crate::source::model::SourceRuntimeState;

pub(super) const SEARCH_MAX_CONCURRENCY: usize = 16;
pub(super) const SEARCH_PER_SOURCE_TIMEOUT: Duration = Duration::from_secs(20);
pub(super) const SOURCE_TEST_TOTAL_TIMEOUT: Duration = Duration::from_secs(90);

pub(super) fn runtime_state_delta(
    prev: &SourceRuntimeState,
    next: &SourceRuntimeState,
) -> SourceRuntimeState {
    SourceRuntimeState {
        variable: if next.variable != prev.variable {
            next.variable.clone()
        } else {
            String::new()
        },
        login_info: next
            .login_info
            .iter()
            .filter(|(k, v)| prev.login_info.get(*k) != Some(*v))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
        cookies: next
            .cookies
            .iter()
            .filter(|(k, v)| prev.cookies.get(*k) != Some(*v))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
        cache: next
            .cache
            .iter()
            .filter(|(k, v)| prev.cache.get(*k) != Some(*v))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
        android_id: if next.android_id != prev.android_id {
            next.android_id.clone()
        } else {
            String::new()
        },
    }
}

pub(super) type AsyncLockMap<K> = Arc<Mutex<HashMap<K, Weak<tokio::sync::Mutex<()>>>>>;

pub(super) fn get_or_insert_weak_lock<K: Eq + std::hash::Hash + Clone>(
    map: &AsyncLockMap<K>,
    key: K,
) -> Arc<tokio::sync::Mutex<()>> {
    let mut guard = map.lock().unwrap_or_else(|e| e.into_inner());
    if guard.len() > 256 {
        guard.retain(|_, weak| weak.strong_count() > 0);
    }
    if let Some(existing) = guard.get(&key).and_then(Weak::upgrade) {
        return existing;
    }
    let created = Arc::new(tokio::sync::Mutex::new(()));
    guard.insert(key, Arc::downgrade(&created));
    created
}

pub struct MusicOnlineService {
    pub(super) sources: Arc<dyn MusicSourceRepository>,
    pub(super) engine: MusicSourceEngine,
    pub(super) source_state_locks: AsyncLockMap<Uuid>,
    pub(super) allow_private_ips: bool,
}

impl MusicOnlineService {
    pub fn new(sources: Arc<dyn MusicSourceRepository>) -> Self {
        Self::with_engine(sources, MusicSourceEngine::default(), false)
    }

    pub fn with_engine(
        sources: Arc<dyn MusicSourceRepository>,
        engine: MusicSourceEngine,
        allow_private_ips: bool,
    ) -> Self {
        Self {
            sources,
            engine,
            source_state_locks: Arc::new(Mutex::new(HashMap::new())),
            allow_private_ips,
        }
    }
}

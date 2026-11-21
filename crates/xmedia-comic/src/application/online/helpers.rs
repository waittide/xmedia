use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};

use uuid::Uuid;

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

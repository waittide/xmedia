use std::collections::HashMap;
use std::hash::Hash;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};

/// Sets an atomic cancellation flag on drop unless explicitly disarmed.
pub struct CancelOnDrop {
    flag: Arc<AtomicBool>,
    armed: bool,
}

impl CancelOnDrop {
    pub fn new(flag: Arc<AtomicBool>) -> Self {
        Self { flag, armed: true }
    }

    pub fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if self.armed {
            self.flag.store(true, Ordering::Relaxed);
        }
    }
}

/// Cancels and aborts a batch of background tasks on drop unless explicitly disarmed.
pub struct CancelTasksOnDrop {
    entries: Vec<(Arc<AtomicBool>, tokio::task::AbortHandle)>,
    armed: bool,
}

impl Default for CancelTasksOnDrop {
    fn default() -> Self {
        Self::new()
    }
}

impl CancelTasksOnDrop {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            armed: true,
        }
    }

    pub fn push(&mut self, flag: Arc<AtomicBool>, abort: tokio::task::AbortHandle) {
        self.entries.push((flag, abort));
    }

    pub fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for CancelTasksOnDrop {
    fn drop(&mut self) {
        if self.armed {
            for (flag, abort) in &self.entries {
                flag.store(true, Ordering::Relaxed);
                abort.abort();
            }
        }
    }
}

pub type AsyncLockMap<K> = Arc<Mutex<HashMap<K, Weak<tokio::sync::Mutex<()>>>>>;

pub fn weak_lock<K: Eq + Hash>(map: &AsyncLockMap<K>, key: K) -> Arc<tokio::sync::Mutex<()>> {
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

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use xmedia_core::domain::error::{DomainError, DomainResult};

use crate::domain::scraper::{ScrapeCandidate, ScrapeQuery, ScrapeTargetKind, ScrapedMetadata};

const DEFAULT_MAX_MEMORY_ENTRIES: usize = 2048;

static TMP_FILE_SEQ: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScrapeCacheStatsDto {
    pub total_entries: usize,
    pub valid_entries: usize,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedEnvelope<T> {
    created_at_ms: u64,
    value: T,
}

fn current_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn hash_cache_key(raw_key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(raw_key.as_bytes());
    hex::encode(hasher.finalize())
}

#[derive(Clone)]
pub struct ScrapeMetadataCache {
    cache_dir: Option<PathBuf>,
    memory: Arc<RwLock<HashMap<String, (u64, serde_json::Value)>>>,
    max_memory_entries: usize,
}

impl ScrapeMetadataCache {
    pub fn new(cache_dir: Option<PathBuf>) -> Self {
        Self::with_capacity(cache_dir, DEFAULT_MAX_MEMORY_ENTRIES)
    }

    pub fn with_capacity(cache_dir: Option<PathBuf>, max_memory_entries: usize) -> Self {
        if let Some(ref dir) = cache_dir {
            let _ = std::fs::create_dir_all(dir);
        }
        Self {
            cache_dir,
            memory: Arc::new(RwLock::new(HashMap::new())),
            max_memory_entries: max_memory_entries.max(1),
        }
    }

    fn insert_memory(&self, digest: String, ts_ms: u64, val: serde_json::Value) {
        if let Ok(mut guard) = self.memory.write() {
            if !guard.contains_key(&digest) && guard.len() >= self.max_memory_entries {
                // Evict oldest entry by timestamp to keep memory bounded
                let oldest_key = guard
                    .iter()
                    .min_by_key(|(_, (ts, _))| *ts)
                    .map(|(k, _)| k.clone());
                if let Some(k) = oldest_key {
                    guard.remove(&k);
                }
            }
            guard.insert(digest, (ts_ms, val));
        }
    }

    pub fn build_search_key(provider_id: &str, query: &ScrapeQuery, locale: &str) -> String {
        let target_str = serde_json::to_string(&query.target).unwrap_or_default();
        format!(
            "search|v1|{provider_id}|{target_str}|kw={}|art={}|alb={}|isbn={}|yr={}|s={}|e={}|dur={}|loc={}",
            query.keyword.trim().to_lowercase(),
            query
                .artist_or_author
                .as_deref()
                .unwrap_or("")
                .trim()
                .to_lowercase(),
            query
                .album_or_series
                .as_deref()
                .unwrap_or("")
                .trim()
                .to_lowercase(),
            query.isbn.as_deref().unwrap_or("").trim().to_lowercase(),
            query.year.map(|v| v.to_string()).unwrap_or_default(),
            query.season.map(|v| v.to_string()).unwrap_or_default(),
            query.episode.map(|v| v.to_string()).unwrap_or_default(),
            query
                .duration_secs
                .map(|v| v.to_string())
                .unwrap_or_default(),
            locale.trim().to_lowercase(),
        )
    }

    pub fn build_detail_key(
        provider_id: &str,
        target: ScrapeTargetKind,
        external_id: &str,
        locale: &str,
    ) -> String {
        let target_str = serde_json::to_string(&target).unwrap_or_default();
        format!(
            "detail|v1|{provider_id}|{target_str}|ext={}|loc={}",
            external_id.trim(),
            locale.trim().to_lowercase(),
        )
    }

    pub fn get_search(
        &self,
        provider_id: &str,
        query: &ScrapeQuery,
        locale: &str,
        ttl_ms: u64,
    ) -> Option<Vec<ScrapeCandidate>> {
        let raw_key = Self::build_search_key(provider_id, query, locale);
        self.get_typed(&raw_key, ttl_ms)
    }

    pub fn put_search(
        &self,
        provider_id: &str,
        query: &ScrapeQuery,
        locale: &str,
        ttl_ms: u64,
        items: &[ScrapeCandidate],
    ) {
        if items.is_empty() {
            return;
        }
        let raw_key = Self::build_search_key(provider_id, query, locale);
        self.put_typed(&raw_key, ttl_ms, &items);
    }

    pub fn get_detail(
        &self,
        provider_id: &str,
        target: ScrapeTargetKind,
        external_id: &str,
        locale: &str,
        ttl_ms: u64,
    ) -> Option<ScrapedMetadata> {
        let raw_key = Self::build_detail_key(provider_id, target, external_id, locale);
        self.get_typed(&raw_key, ttl_ms)
    }

    pub fn put_detail(
        &self,
        provider_id: &str,
        target: ScrapeTargetKind,
        external_id: &str,
        locale: &str,
        ttl_ms: u64,
        detail: &ScrapedMetadata,
    ) {
        let raw_key = Self::build_detail_key(provider_id, target, external_id, locale);
        self.put_typed(&raw_key, ttl_ms, detail);
    }

    fn get_typed<T: DeserializeOwned>(&self, raw_key: &str, ttl_ms: u64) -> Option<T> {
        if ttl_ms == 0 {
            return None;
        }
        let digest = hash_cache_key(raw_key);
        let now = current_epoch_ms();

        if let Ok(guard) = self.memory.read()
            && let Some((created_at_ms, val)) = guard.get(&digest)
            && now.saturating_sub(*created_at_ms) < ttl_ms
            && let Ok(parsed) = serde_json::from_value::<T>(val.clone())
        {
            return Some(parsed);
        }

        let dir = self.cache_dir.as_ref()?;
        let path = dir.join(format!("{digest}.json"));
        let bytes = std::fs::read(&path).ok()?;
        let env: CachedEnvelope<serde_json::Value> = serde_json::from_slice(&bytes).ok()?;
        if now.saturating_sub(env.created_at_ms) >= ttl_ms {
            let _ = std::fs::remove_file(&path);
            if let Ok(mut guard) = self.memory.write() {
                guard.remove(&digest);
            }
            return None;
        }

        let parsed = serde_json::from_value::<T>(env.value.clone()).ok()?;
        self.insert_memory(digest, env.created_at_ms, env.value);
        Some(parsed)
    }

    fn put_typed<T: Serialize>(&self, raw_key: &str, ttl_ms: u64, value: &T) {
        if ttl_ms == 0 {
            return;
        }
        let Ok(json_val) = serde_json::to_value(value) else {
            return;
        };
        let digest = hash_cache_key(raw_key);
        let now = current_epoch_ms();

        self.insert_memory(digest.clone(), now, json_val.clone());

        if let Some(ref dir) = self.cache_dir {
            let _ = std::fs::create_dir_all(dir);
            let env = CachedEnvelope {
                created_at_ms: now,
                value: json_val,
            };
            if let Ok(bytes) = serde_json::to_vec(&env) {
                let seq = TMP_FILE_SEQ.fetch_add(1, Ordering::Relaxed);
                let pid = std::process::id();
                let tmp_path = dir.join(format!("{digest}.{pid}.{seq}.tmp"));
                let final_path = dir.join(format!("{digest}.json"));
                if std::fs::write(&tmp_path, &bytes).is_ok() {
                    if std::fs::rename(&tmp_path, &final_path).is_err() {
                        let _ = std::fs::remove_file(&tmp_path);
                    } else if seq.is_multiple_of(64) {
                        Self::prune_disk_cache(dir, 2048);
                    }
                } else {
                    let _ = std::fs::remove_file(&tmp_path);
                }
            }
        }
    }

    fn prune_disk_cache(dir: &std::path::Path, max_entries: usize) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        let mut files: Vec<(PathBuf, std::time::SystemTime)> = Vec::new();
        for entry in rd.flatten() {
            let p = entry.path();
            if p.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            if let Ok(meta) = entry.metadata()
                && meta.is_file()
            {
                let mtime = meta.modified().unwrap_or(UNIX_EPOCH);
                files.push((p, mtime));
            }
        }
        if files.len() <= max_entries {
            return;
        }
        files.sort_by_key(|(_, mtime)| *mtime);
        let remove_count = files.len().saturating_sub(max_entries);
        for (path, _) in files.into_iter().take(remove_count) {
            let _ = std::fs::remove_file(path);
        }
    }

    pub fn stats(&self, ttl_ms: u64) -> ScrapeCacheStatsDto {
        let now = current_epoch_ms();
        let Some(ref dir) = self.cache_dir else {
            let guard = self.memory.read().ok();
            let total_entries = guard.as_ref().map(|g| g.len()).unwrap_or(0);
            let valid_entries = guard
                .as_ref()
                .map(|g| {
                    g.values()
                        .filter(|(ts, _)| ttl_ms > 0 && now.saturating_sub(*ts) < ttl_ms)
                        .count()
                })
                .unwrap_or(0);
            return ScrapeCacheStatsDto {
                total_entries,
                valid_entries,
                total_bytes: 0,
            };
        };

        let Ok(rd) = std::fs::read_dir(dir) else {
            return ScrapeCacheStatsDto::default();
        };

        let mut total_entries = 0usize;
        let mut valid_entries = 0usize;
        let mut total_bytes = 0u64;

        for entry in rd.flatten() {
            let p = entry.path();
            if p.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if !meta.is_file() {
                continue;
            }
            total_entries += 1;
            total_bytes += meta.len();
            if ttl_ms > 0 {
                let created_ms = std::fs::read(&p)
                    .ok()
                    .and_then(|b| {
                        serde_json::from_slice::<CachedEnvelope<serde_json::Value>>(&b).ok()
                    })
                    .map(|e| e.created_at_ms)
                    .or_else(|| {
                        meta.modified()
                            .ok()
                            .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
                            .map(|d| d.as_millis() as u64)
                    });
                if created_ms.is_some_and(|ts| now.saturating_sub(ts) < ttl_ms) {
                    valid_entries += 1;
                }
            }
        }

        ScrapeCacheStatsDto {
            total_entries,
            valid_entries,
            total_bytes,
        }
    }

    pub fn clear(&self) -> DomainResult<usize> {
        let mut removed = 0usize;
        if let Ok(mut guard) = self.memory.write() {
            removed = guard.len();
            guard.clear();
        }
        if let Some(ref dir) = self.cache_dir
            && dir.is_dir()
        {
            let mut disk_removed = 0usize;
            let rd = std::fs::read_dir(dir).map_err(|e| {
                DomainError::Storage(format!("failed reading scrape cache dir: {e}"))
            })?;
            for entry in rd.flatten() {
                let p = entry.path();
                if p.extension().and_then(|s| s.to_str()) == Some("json")
                    && std::fs::remove_file(&p).is_ok()
                {
                    disk_removed += 1;
                }
            }
            removed = removed.max(disk_removed);
        }
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrape_cache_respects_ms_ttl_and_clear() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = ScrapeMetadataCache::new(Some(tmp.path().to_path_buf()));
        let meta = ScrapedMetadata {
            provider_id: "tmdb".into(),
            external_id: "123".into(),
            target: Some(ScrapeTargetKind::Movie),
            title: Some("Interstellar".into()),
            year: Some(2014),
            ..Default::default()
        };

        // ttl_ms = 0 bypasses cache
        cache.put_detail("tmdb", ScrapeTargetKind::Movie, "123", "zh-CN", 0, &meta);
        assert!(
            cache
                .get_detail("tmdb", ScrapeTargetKind::Movie, "123", "zh-CN", 60_000)
                .is_none()
        );

        // Store with positive TTL
        cache.put_detail(
            "tmdb",
            ScrapeTargetKind::Movie,
            "123",
            "zh-CN",
            60_000,
            &meta,
        );
        let hit = cache
            .get_detail("tmdb", ScrapeTargetKind::Movie, "123", "zh-CN", 60_000)
            .expect("should hit within 60_000 ms");
        assert_eq!(hit.title.as_deref(), Some("Interstellar"));

        // Dynamic ms precision check: wait 25ms, verify ttl_ms = 10ms treats it as expired
        std::thread::sleep(std::time::Duration::from_millis(25));
        assert!(
            cache
                .get_detail("tmdb", ScrapeTargetKind::Movie, "123", "zh-CN", 10)
                .is_none()
        );

        // Re-populate and clear
        cache.put_detail(
            "tmdb",
            ScrapeTargetKind::Movie,
            "456",
            "zh-CN",
            60_000,
            &meta,
        );
        assert_eq!(cache.stats(60_000).valid_entries, 1);
        let removed = cache.clear().unwrap();
        assert_eq!(removed, 1);
        assert_eq!(cache.stats(60_000).total_entries, 0);
    }

    #[test]
    fn scrape_cache_evicts_oldest_memory_entry_when_capacity_exceeded() {
        let cache = ScrapeMetadataCache::with_capacity(None, 2);
        let make_meta = |id: &str| ScrapedMetadata {
            provider_id: "tmdb".into(),
            external_id: id.into(),
            target: Some(ScrapeTargetKind::Movie),
            title: Some(format!("Title {id}")),
            ..Default::default()
        };

        cache.put_detail(
            "tmdb",
            ScrapeTargetKind::Movie,
            "old",
            "zh-CN",
            60_000,
            &make_meta("old"),
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
        cache.put_detail(
            "tmdb",
            ScrapeTargetKind::Movie,
            "mid",
            "zh-CN",
            60_000,
            &make_meta("mid"),
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
        cache.put_detail(
            "tmdb",
            ScrapeTargetKind::Movie,
            "new",
            "zh-CN",
            60_000,
            &make_meta("new"),
        );

        assert_eq!(cache.stats(60_000).total_entries, 2);
        assert!(
            cache
                .get_detail("tmdb", ScrapeTargetKind::Movie, "old", "zh-CN", 60_000)
                .is_none(),
            "oldest entry should be evicted"
        );
        assert!(
            cache
                .get_detail("tmdb", ScrapeTargetKind::Movie, "mid", "zh-CN", 60_000)
                .is_some()
        );
        assert!(
            cache
                .get_detail("tmdb", ScrapeTargetKind::Movie, "new", "zh-CN", 60_000)
                .is_some()
        );
    }
}

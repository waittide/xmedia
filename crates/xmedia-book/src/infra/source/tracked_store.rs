use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, SystemTime};

use md5::{Digest, Md5};
use uuid::Uuid;

use crate::application::ports::TrackedBookStorePort;
use crate::domain::book::BookId;
use crate::domain::online::{
    OnlineChapterContent, OnlineChapterItem, PackChapterInput, TrackedChapterFile,
    TrackedChapterMeta, TrackedManifest,
};

#[derive(Debug, Clone)]
pub struct TrackedBookStore {
    root_dir: PathBuf,
    cache_dir: PathBuf,
    book_locks: Arc<Mutex<HashMap<BookId, Weak<Mutex<()>>>>>,
}

impl TrackedBookStore {
    pub fn new(data_dir: &Path) -> Result<Self, String> {
        let root_dir = data_dir.join("book").join("online_tracked");
        let cache_dir = data_dir.join("online_book_cache");
        fs::create_dir_all(&root_dir)
            .map_err(|e| format!("创建在线书籍章节目录 '{}' 失败: {e}", root_dir.display()))?;
        fs::create_dir_all(&cache_dir)
            .map_err(|e| format!("创建在线书源缓存目录 '{}' 失败: {e}", cache_dir.display()))?;
        Ok(Self {
            root_dir,
            cache_dir,
            book_locks: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    fn lock_for(&self, book_id: BookId) -> Arc<Mutex<()>> {
        let mut guard = self.book_locks.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(existing) = guard.get(&book_id).and_then(Weak::upgrade) {
            return existing;
        }
        if guard.len() > 256 {
            guard.retain(|_, w| w.strong_count() > 0);
        }
        let lock = Arc::new(Mutex::new(()));
        guard.insert(book_id, Arc::downgrade(&lock));
        lock
    }

    pub fn remove_book_lock(&self, book_id: BookId) {
        let mut guard = self.book_locks.lock().unwrap_or_else(|e| e.into_inner());
        guard.remove(&book_id);
    }

    pub fn book_dir(&self, book_id: BookId) -> PathBuf {
        self.root_dir.join(book_id.0.to_string())
    }

    fn manifest_path(&self, book_id: BookId) -> PathBuf {
        self.book_dir(book_id).join("manifest.json")
    }

    fn chapters_dir(&self, book_id: BookId) -> PathBuf {
        self.book_dir(book_id).join("chapters")
    }

    fn chapter_path(&self, book_id: BookId, index: usize) -> PathBuf {
        self.chapters_dir(book_id).join(format!("{index:06}.json"))
    }

    fn online_chapter_cache_path(&self, source_id: Uuid, chapter_url: &str) -> PathBuf {
        let mut h = Md5::new();
        h.update(chapter_url.as_bytes());
        let key = hex::encode(h.finalize());
        self.cache_dir
            .join(source_id.to_string())
            .join(format!("{key}.json"))
    }

    fn load_manifest_unlocked(&self, book_id: BookId) -> TrackedManifest {
        let path = self.manifest_path(book_id);
        let bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return TrackedManifest::default();
            }
            Err(err) => {
                tracing::warn!(book_id = %book_id, path = %path.display(), error = %err, "failed to read tracked manifest");
                return TrackedManifest::default();
            }
        };
        match serde_json::from_slice(&bytes) {
            Ok(m) => m,
            Err(err) => {
                tracing::warn!(book_id = %book_id, path = %path.display(), error = %err, "corrupt tracked manifest JSON");
                TrackedManifest::default()
            }
        }
    }

    fn save_manifest_unlocked(
        &self,
        book_id: BookId,
        manifest: &TrackedManifest,
    ) -> Result<(), String> {
        let dir = self.book_dir(book_id);
        fs::create_dir_all(&dir).map_err(|e| format!("创建书籍目录失败: {e}"))?;
        let data = serde_json::to_vec(manifest).map_err(|e| format!("序列化目录清单失败: {e}"))?;
        write_atomic(&self.manifest_path(book_id), &data)
    }

    pub fn load_manifest(&self, book_id: BookId) -> TrackedManifest {
        let lock = self.lock_for(book_id);
        let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
        self.load_manifest_unlocked(book_id)
    }

    pub fn save_manifest(&self, book_id: BookId, manifest: &TrackedManifest) -> Result<(), String> {
        let lock = self.lock_for(book_id);
        let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
        self.save_manifest_unlocked(book_id, manifest)
    }

    pub fn update_manifest_from_toc(
        &self,
        book_id: BookId,
        toc: &[OnlineChapterItem],
        preserve_existing_cache: bool,
    ) -> Result<TrackedManifest, String> {
        let lock = self.lock_for(book_id);
        let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());

        let existing_cached_indices: HashSet<usize> = self.existing_chapter_indices(book_id);

        let mut valid_cached_indices = HashSet::new();
        let chapters = toc
            .iter()
            .enumerate()
            .map(|(idx, ch)| {
                let is_cached = preserve_existing_cache
                    && existing_cached_indices.contains(&idx)
                    && self.load_chapter(book_id, idx).is_some_and(|cached| {
                        !cached.content.trim().is_empty() && cached.url == ch.url
                    });
                if is_cached {
                    valid_cached_indices.insert(idx);
                }
                TrackedChapterMeta {
                    index: idx,
                    title: ch.title.clone(),
                    url: ch.url.clone(),
                    cached: is_cached,
                }
            })
            .collect();

        for old_idx in existing_cached_indices {
            if !valid_cached_indices.contains(&old_idx) {
                let _ = fs::remove_file(self.chapter_path(book_id, old_idx));
            }
        }

        let manifest = TrackedManifest { chapters };
        self.save_manifest_unlocked(book_id, &manifest)?;
        Ok(manifest)
    }

    pub fn load_chapter(&self, book_id: BookId, index: usize) -> Option<TrackedChapterFile> {
        let path = self.chapter_path(book_id, index);
        let bytes = fs::read(path).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    pub fn load_chapter_matching_url(
        &self,
        book_id: BookId,
        index: usize,
        expected_url: &str,
    ) -> Option<TrackedChapterFile> {
        let ch = self.load_chapter(book_id, index)?;
        if ch.content.trim().is_empty() {
            return None;
        }
        if !expected_url.is_empty() && ch.url != expected_url {
            return None;
        }
        Some(ch)
    }

    pub fn save_chapter(
        &self,
        book_id: BookId,
        chapter: &TrackedChapterFile,
    ) -> Result<u32, String> {
        let data = serde_json::to_vec(chapter).map_err(|e| format!("序列化章节失败: {e}"))?;

        let lock = self.lock_for(book_id);
        let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());

        let dir = self.chapters_dir(book_id);
        fs::create_dir_all(&dir).map_err(|e| format!("创建章节缓存目录失败: {e}"))?;

        write_atomic(&self.chapter_path(book_id, chapter.index), &data)?;

        let mut manifest = self.load_manifest_unlocked(book_id);
        if let Some(entry) = manifest.chapters.get_mut(chapter.index) {
            entry.cached = true;
            if !chapter.title.trim().is_empty() {
                entry.title = chapter.title.clone();
            }
            if !chapter.url.trim().is_empty() {
                entry.url = chapter.url.clone();
            }
            if let Err(err) = self.save_manifest_unlocked(book_id, &manifest) {
                tracing::warn!(book_id = %book_id, error = %err, "failed to persist tracked manifest after chapter save");
            }
            let count = manifest.chapters.iter().filter(|c| c.cached).count() as u32;
            return Ok(count);
        }

        Ok(self.existing_chapter_indices(book_id).len() as u32)
    }

    pub fn existing_chapter_indices(&self, book_id: BookId) -> HashSet<usize> {
        let mut set = HashSet::new();
        let Ok(entries) = fs::read_dir(self.chapters_dir(book_id)) else {
            return set;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(stem) = name.to_str().and_then(|s| s.strip_suffix(".json")) else {
                continue;
            };
            if let Ok(idx) = stem.parse::<usize>() {
                set.insert(idx);
            }
        }
        set
    }

    pub fn collect_cached_chapters_for_epub(
        &self,
        book_id: BookId,
    ) -> (Vec<PackChapterInput>, u32, u32) {
        let lock = self.lock_for(book_id);
        let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());

        let manifest_exists = self.manifest_path(book_id).is_file();
        let manifest = self.load_manifest_unlocked(book_id);
        let total = manifest.chapters.len() as u32;
        let mut out = Vec::new();

        if !manifest.chapters.is_empty() {
            for meta in &manifest.chapters {
                if let Some(ch) = self.load_chapter(book_id, meta.index)
                    && !ch.content.trim().is_empty()
                    && (meta.url.is_empty() || ch.url == meta.url)
                {
                    let title = if ch.title.trim().is_empty() {
                        meta.title.clone()
                    } else {
                        ch.title
                    };
                    out.push(PackChapterInput {
                        title,
                        content: ch.content,
                    });
                }
            }
            let cached = out.len() as u32;
            return (out, cached, total);
        }

        if manifest_exists {
            return (Vec::new(), 0, 0);
        }

        let mut indices: Vec<usize> = self.existing_chapter_indices(book_id).into_iter().collect();
        indices.sort_unstable();
        for idx in indices {
            if let Some(ch) = self.load_chapter(book_id, idx)
                && !ch.content.trim().is_empty()
            {
                out.push(PackChapterInput {
                    title: ch.title,
                    content: ch.content,
                });
            }
        }
        let cached = out.len() as u32;
        (out, cached, cached)
    }

    pub fn clear_chapters(&self, book_id: BookId) {
        let lock = self.lock_for(book_id);
        let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
        let dir = self.chapters_dir(book_id);
        if dir.is_dir()
            && let Err(err) = fs::remove_dir_all(&dir)
        {
            tracing::warn!(book_id = %book_id, path = %dir.display(), error = %err, "failed to clear tracked chapters");
        }
    }

    pub fn delete_book_dir(&self, book_id: BookId) {
        let lock = self.lock_for(book_id);
        let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
        let dir = self.book_dir(book_id);
        if dir.is_dir()
            && let Err(err) = fs::remove_dir_all(&dir)
        {
            tracing::warn!(book_id = %book_id, path = %dir.display(), error = %err, "failed to delete tracked book dir");
        }
        drop(_guard);
        self.remove_book_lock(book_id);
    }

    pub fn read_cached_online_chapter(
        &self,
        source_id: Uuid,
        chapter_url: &str,
    ) -> Option<OnlineChapterContent> {
        let path = self.online_chapter_cache_path(source_id, chapter_url);
        let meta = fs::metadata(&path).ok()?;
        if let Ok(modified) = meta.modified()
            && let Ok(age) = SystemTime::now().duration_since(modified)
            && age > Duration::from_secs(14 * 24 * 3600)
        {
            let _ = fs::remove_file(&path);
            return None;
        }
        let raw = fs::read_to_string(&path).ok()?;
        serde_json::from_str::<OnlineChapterContent>(&raw).ok()
    }

    pub fn write_cached_online_chapter(
        &self,
        source_id: Uuid,
        chapter_url: &str,
        content: &OnlineChapterContent,
    ) {
        if content.content.trim().is_empty() {
            return;
        }
        let path = self.online_chapter_cache_path(source_id, chapter_url);
        if let Some(parent) = path.parent()
            && let Err(err) = fs::create_dir_all(parent)
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
}

impl TrackedBookStorePort for TrackedBookStore {
    fn remove_book_lock(&self, book_id: BookId) {
        Self::remove_book_lock(self, book_id);
    }

    fn load_manifest(&self, book_id: BookId) -> TrackedManifest {
        Self::load_manifest(self, book_id)
    }

    fn save_manifest(&self, book_id: BookId, manifest: &TrackedManifest) -> Result<(), String> {
        Self::save_manifest(self, book_id, manifest)
    }

    fn update_manifest_from_toc(
        &self,
        book_id: BookId,
        toc: &[OnlineChapterItem],
        preserve_existing_cache: bool,
    ) -> Result<TrackedManifest, String> {
        Self::update_manifest_from_toc(self, book_id, toc, preserve_existing_cache)
    }

    fn load_chapter(&self, book_id: BookId, index: usize) -> Option<TrackedChapterFile> {
        Self::load_chapter(self, book_id, index)
    }

    fn load_chapter_matching_url(
        &self,
        book_id: BookId,
        index: usize,
        expected_url: &str,
    ) -> Option<TrackedChapterFile> {
        Self::load_chapter_matching_url(self, book_id, index, expected_url)
    }

    fn save_chapter(&self, book_id: BookId, chapter: &TrackedChapterFile) -> Result<u32, String> {
        Self::save_chapter(self, book_id, chapter)
    }

    fn collect_cached_chapters_for_epub(
        &self,
        book_id: BookId,
    ) -> (Vec<PackChapterInput>, u32, u32) {
        Self::collect_cached_chapters_for_epub(self, book_id)
    }

    fn clear_chapters(&self, book_id: BookId) {
        Self::clear_chapters(self, book_id);
    }

    fn delete_book_dir(&self, book_id: BookId) {
        Self::delete_book_dir(self, book_id);
    }

    fn read_cached_online_chapter(
        &self,
        source_id: Uuid,
        chapter_url: &str,
    ) -> Option<OnlineChapterContent> {
        Self::read_cached_online_chapter(self, source_id, chapter_url)
    }

    fn write_cached_online_chapter(
        &self,
        source_id: Uuid,
        chapter_url: &str,
        content: &OnlineChapterContent,
    ) {
        Self::write_cached_online_chapter(self, source_id, chapter_url, content);
    }
}

struct TempFileGuard {
    path: PathBuf,
    armed: bool,
}

impl TempFileGuard {
    fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

pub(crate) fn write_atomic(path: &Path, data: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("无效目标路径: {}", path.display()))?;
    let tmp_name = format!(
        ".{}.{}.tmp",
        path.file_name().and_then(|s| s.to_str()).unwrap_or("file"),
        Uuid::new_v4().simple()
    );
    let tmp_path = parent.join(tmp_name);
    let mut guard = TempFileGuard::new(tmp_path.clone());
    {
        let mut file = fs::File::create(&tmp_path)
            .map_err(|e| format!("创建临时文件 '{}' 失败: {e}", tmp_path.display()))?;
        file.write_all(data)
            .map_err(|e| format!("写入临时文件 '{}' 失败: {e}", tmp_path.display()))?;
        file.sync_all()
            .map_err(|e| format!("同步临时文件 '{}' 失败: {e}", tmp_path.display()))?;
    }
    fs::rename(&tmp_path, path)
        .map_err(|e| format!("原子替换文件 '{}' 失败: {e}", path.display()))?;
    guard.disarm();
    Ok(())
}

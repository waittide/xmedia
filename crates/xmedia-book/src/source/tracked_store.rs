use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::book::BookId;
use crate::source::model::OnlineChapterItem;
use crate::source::packager::PackChapterInput;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackedChapterMeta {
    pub index: usize,
    pub title: String,
    pub url: String,
    #[serde(default)]
    pub cached: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TrackedManifest {
    pub chapters: Vec<TrackedChapterMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackedChapterFile {
    pub index: usize,
    pub title: String,
    pub url: String,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct TrackedBookStore {
    root_dir: PathBuf,
    book_locks: Arc<Mutex<HashMap<BookId, Arc<Mutex<()>>>>>,
}

impl TrackedBookStore {
    pub fn new(data_dir: &Path) -> Result<Self, String> {
        let root_dir = data_dir.join("book").join("online_tracked");
        fs::create_dir_all(&root_dir)
            .map_err(|e| format!("创建在线书籍章节目录 '{}' 失败: {e}", root_dir.display()))?;
        Ok(Self {
            root_dir,
            book_locks: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    fn lock_for(&self, book_id: BookId) -> Arc<Mutex<()>> {
        let mut guard = self.book_locks.lock().unwrap_or_else(|e| e.into_inner());
        Arc::clone(
            guard
                .entry(book_id)
                .or_insert_with(|| Arc::new(Mutex::new(()))),
        )
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

    fn load_manifest_unlocked(&self, book_id: BookId) -> TrackedManifest {
        let path = self.manifest_path(book_id);
        let Ok(bytes) = fs::read(path) else {
            return TrackedManifest::default();
        };
        serde_json::from_slice(&bytes).unwrap_or_default()
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

    /// Merges a freshly fetched TOC with previously cached chapters, marking a slot `cached: true`
    /// only when a cached chapter file exists at that index with matching URL and non-empty body.
    pub fn update_manifest_from_toc(
        &self,
        book_id: BookId,
        toc: &[OnlineChapterItem],
        preserve_existing_cache: bool,
    ) -> Result<TrackedManifest, String> {
        let lock = self.lock_for(book_id);
        let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());

        let existing_cached_indices: HashSet<usize> = if preserve_existing_cache {
            self.existing_chapter_indices(book_id)
        } else {
            HashSet::new()
        };

        let chapters = toc
            .iter()
            .enumerate()
            .map(|(idx, ch)| {
                let is_cached = preserve_existing_cache
                    && existing_cached_indices.contains(&idx)
                    && self.load_chapter(book_id, idx).is_some_and(|cached| {
                        !cached.content.trim().is_empty() && cached.url == ch.url
                    });
                TrackedChapterMeta {
                    index: idx,
                    title: ch.title.clone(),
                    url: ch.url.clone(),
                    cached: is_cached,
                }
            })
            .collect();

        let manifest = TrackedManifest { chapters };
        self.save_manifest_unlocked(book_id, &manifest)?;
        Ok(manifest)
    }

    pub fn load_chapter(&self, book_id: BookId, index: usize) -> Option<TrackedChapterFile> {
        let path = self.chapter_path(book_id, index);
        let bytes = fs::read(path).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    /// Loads a cached chapter only if its stored URL matches `expected_url` (when non-empty).
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
        let manifest = self.load_manifest(book_id);
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

        // Fallback if chapter files exist without manifest
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

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn concurrent_save_chapter_preserves_all_manifest_cached_flags() {
        let tmp = TempDir::new().unwrap();
        let store = TrackedBookStore::new(tmp.path()).unwrap();
        let book_id = BookId::new();
        let toc: Vec<OnlineChapterItem> = (0..12)
            .map(|i| OnlineChapterItem {
                index: i,
                title: format!("第{}章", i + 1),
                url: format!("https://example.com/ch/{i}"),
                update_time: None,
                is_volume: false,
            })
            .collect();
        store
            .update_manifest_from_toc(book_id, &toc, false)
            .unwrap();

        std::thread::scope(|s| {
            for i in 0..12 {
                let st = store.clone();
                s.spawn(move || {
                    st.save_chapter(
                        book_id,
                        &TrackedChapterFile {
                            index: i,
                            title: format!("第{}章", i + 1),
                            url: format!("https://example.com/ch/{i}"),
                            content: format!("正文 {i}"),
                        },
                    )
                    .unwrap();
                });
            }
        });

        let manifest = store.load_manifest(book_id);
        let cached_count = manifest.chapters.iter().filter(|c| c.cached).count();
        assert_eq!(
            cached_count, 12,
            "concurrent chapter saves must not lose cached flags"
        );
    }

    #[test]
    fn update_manifest_invalidates_cache_flag_when_chapter_url_changes() {
        let tmp = TempDir::new().unwrap();
        let store = TrackedBookStore::new(tmp.path()).unwrap();
        let book_id = BookId::new();
        let toc1 = vec![
            OnlineChapterItem {
                index: 0,
                title: "第一章".into(),
                url: "https://src-a.test/1".into(),
                update_time: None,
                is_volume: false,
            },
            OnlineChapterItem {
                index: 1,
                title: "第二章".into(),
                url: "https://src-a.test/2".into(),
                update_time: None,
                is_volume: false,
            },
        ];
        store
            .update_manifest_from_toc(book_id, &toc1, false)
            .unwrap();
        store
            .save_chapter(
                book_id,
                &TrackedChapterFile {
                    index: 0,
                    title: "第一章".into(),
                    url: "https://src-a.test/1".into(),
                    content: "旧源第一章".into(),
                },
            )
            .unwrap();

        // Prepended prologue shifts URLs -> index 0 now has a different URL
        let toc2 = vec![
            OnlineChapterItem {
                index: 0,
                title: "楔子".into(),
                url: "https://src-a.test/0".into(),
                update_time: None,
                is_volume: false,
            },
            OnlineChapterItem {
                index: 1,
                title: "第一章".into(),
                url: "https://src-a.test/1".into(),
                update_time: None,
                is_volume: false,
            },
        ];
        let manifest = store
            .update_manifest_from_toc(book_id, &toc2, true)
            .unwrap();
        assert!(
            !manifest.chapters[0].cached,
            "mismatched URL at index 0 must not be marked cached"
        );
        assert!(
            store
                .load_chapter_matching_url(book_id, 0, "https://src-a.test/0")
                .is_none()
        );
        let (packed, cached, total) = store.collect_cached_chapters_for_epub(book_id);
        assert_eq!(total, 2);
        assert_eq!(cached, 0);
        assert!(packed.is_empty());
    }
}

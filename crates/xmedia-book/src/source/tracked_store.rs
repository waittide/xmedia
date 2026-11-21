use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

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
}

impl TrackedBookStore {
    pub fn new(data_dir: &Path) -> Result<Self, String> {
        let root_dir = data_dir.join("book").join("online_tracked");
        fs::create_dir_all(&root_dir)
            .map_err(|e| format!("创建在线书籍章节目录 '{}' 失败: {e}", root_dir.display()))?;
        Ok(Self { root_dir })
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

    pub fn load_manifest(&self, book_id: BookId) -> TrackedManifest {
        let path = self.manifest_path(book_id);
        let Ok(bytes) = fs::read(path) else {
            return TrackedManifest::default();
        };
        serde_json::from_slice(&bytes).unwrap_or_default()
    }

    pub fn save_manifest(&self, book_id: BookId, manifest: &TrackedManifest) -> Result<(), String> {
        let dir = self.book_dir(book_id);
        fs::create_dir_all(&dir).map_err(|e| format!("创建书籍目录失败: {e}"))?;
        let data = serde_json::to_vec(manifest).map_err(|e| format!("序列化目录清单失败: {e}"))?;
        write_atomic(&self.manifest_path(book_id), &data)
    }

    /// Merges a freshly fetched TOC with any previously cached chapters, preserving cached status
    /// when a chapter file already exists at that index with matching URL or same slot.
    pub fn update_manifest_from_toc(
        &self,
        book_id: BookId,
        toc: &[OnlineChapterItem],
        preserve_existing_cache: bool,
    ) -> Result<TrackedManifest, String> {
        let existing_cached_indices: HashSet<usize> = if preserve_existing_cache {
            self.existing_chapter_indices(book_id)
        } else {
            HashSet::new()
        };

        let chapters = toc
            .iter()
            .enumerate()
            .map(|(idx, ch)| TrackedChapterMeta {
                index: idx,
                title: ch.title.clone(),
                url: ch.url.clone(),
                cached: preserve_existing_cache && existing_cached_indices.contains(&idx),
            })
            .collect();

        let manifest = TrackedManifest { chapters };
        self.save_manifest(book_id, &manifest)?;
        Ok(manifest)
    }

    pub fn load_chapter(&self, book_id: BookId, index: usize) -> Option<TrackedChapterFile> {
        let path = self.chapter_path(book_id, index);
        let bytes = fs::read(path).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    pub fn save_chapter(
        &self,
        book_id: BookId,
        chapter: &TrackedChapterFile,
    ) -> Result<u32, String> {
        let dir = self.chapters_dir(book_id);
        fs::create_dir_all(&dir).map_err(|e| format!("创建章节缓存目录失败: {e}"))?;
        let data = serde_json::to_vec(chapter).map_err(|e| format!("序列化章节失败: {e}"))?;
        write_atomic(&self.chapter_path(book_id, chapter.index), &data)?;

        let mut manifest = self.load_manifest(book_id);
        if let Some(entry) = manifest.chapters.get_mut(chapter.index) {
            entry.cached = true;
            if !chapter.title.trim().is_empty() {
                entry.title = chapter.title.clone();
            }
            if !chapter.url.trim().is_empty() {
                entry.url = chapter.url.clone();
            }
            let _ = self.save_manifest(book_id, &manifest);
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
        let dir = self.chapters_dir(book_id);
        if dir.is_dir() {
            let _ = fs::remove_dir_all(dir);
        }
    }

    pub fn delete_book_dir(&self, book_id: BookId) {
        let dir = self.book_dir(book_id);
        if dir.is_dir() {
            let _ = fs::remove_dir_all(dir);
        }
    }
}

fn write_atomic(path: &Path, data: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("无效目标路径: {}", path.display()))?;
    let tmp_name = format!(
        ".{}.{}.tmp",
        path.file_name().and_then(|s| s.to_str()).unwrap_or("file"),
        Uuid::new_v4().simple()
    );
    let tmp_path = parent.join(tmp_name);
    {
        let mut file = fs::File::create(&tmp_path)
            .map_err(|e| format!("创建临时文件 '{}' 失败: {e}", tmp_path.display()))?;
        file.write_all(data)
            .map_err(|e| format!("写入临时文件 '{}' 失败: {e}", tmp_path.display()))?;
    }
    fs::rename(&tmp_path, path)
        .map_err(|e| format!("原子替换文件 '{}' 失败: {e}", path.display()))?;
    Ok(())
}

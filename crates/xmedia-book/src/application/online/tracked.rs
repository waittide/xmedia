use std::sync::Arc;

use bytes::Bytes;
use jiff::Timestamp;
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::user::{User, require_admin};

use crate::application::dto::BookSummaryDto;
use crate::application::ports::CoverPayload;
use crate::domain::book::{Book, BookFormat, BookId, BookMetadata, BookSource};
use crate::source::model::{BookJsContext, ChapterJsContext, OnlineChapterItem};
use crate::source::packager::{PackBookMetadata, package_chapters_to_epub};
use crate::source::tracked_store::TrackedChapterFile;

use super::dto::{
    SwitchTrackedSourceInput, TrackOnlineBookInput, TrackedChapterResponseDto,
    TrackedTocResponseDto,
};
use super::helpers::{ChapterFetchOptions, get_or_insert_weak_lock};
use super::{BookOnlineService, TRACKED_EPUB_DEBOUNCE, TRACKED_EPUB_IMMEDIATE_BATCH};

impl BookOnlineService {
    pub(super) fn book_state_lock_for(&self, book_id: BookId) -> Arc<tokio::sync::Mutex<()>> {
        get_or_insert_weak_lock(&self.book_state_locks, book_id)
    }

    fn epub_lock_for(&self, book_id: BookId) -> Arc<tokio::sync::Mutex<()>> {
        get_or_insert_weak_lock(&self.epub_sync_locks, book_id)
    }

    fn track_lock_for(&self, source_id: Uuid, book_url: &str) -> Arc<tokio::sync::Mutex<()>> {
        get_or_insert_weak_lock(&self.track_locks, (source_id, book_url.to_string()))
    }

    pub(super) fn bump_sync_gen(&self, book_id: BookId, delta_chapters: u32) -> (u64, bool) {
        let mut guard = self
            .pending_sync_gens
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let entry = guard.entry(book_id).or_insert((0, 0));
        entry.0 = entry.0.wrapping_add(1);
        entry.1 = entry.1.saturating_add(delta_chapters);
        let flush_now = entry.1 >= TRACKED_EPUB_IMMEDIATE_BATCH;
        if flush_now {
            entry.1 = 0;
        }
        (entry.0, flush_now)
    }

    fn current_sync_gen(&self, book_id: BookId) -> u64 {
        self.pending_sync_gens
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&book_id)
            .map(|(g, _)| *g)
            .unwrap_or(0)
    }

    fn parse_tags_from_kind(raw: Option<&str>) -> Vec<String> {
        raw.map(|s| {
            s.split([',', '，', '|', '/'])
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
    }

    /// Tracks an online book as a first-class unified `Book` in the managed library:
    /// creates (or returns existing) `BookModel` with `BookSource::OnlineTracked`,
    /// stores its cover, fetches initial TOC best-effort, and writes a valid initial EPUB 3.0.
    pub async fn track_online_book(
        self: &Arc<Self>,
        actor: &User,
        mut input: TrackOnlineBookInput,
    ) -> DomainResult<BookSummaryDto> {
        require_admin(actor)?;
        let src = self.load_source(input.source_id).await?;
        input.book_url = Self::validate_caller_source_url(&input.book_url, &src.raw_json)?;
        if !input.toc_url.trim().is_empty() {
            input.toc_url = Self::validate_caller_source_url(&input.toc_url, &src.raw_json)?;
        }
        if let Some(ref cover_url) = input.cover_url
            && !cover_url.trim().is_empty()
        {
            input.cover_url = Some(Self::validate_caller_http_url(cover_url, &src.raw_json)?);
        }

        // Serialize concurrent track requests for the same (source_id, book_url) to prevent duplicate Books.
        let tlock = self.track_lock_for(input.source_id, &input.book_url);
        let _track_guard = tlock.lock().await;

        if let Some(existing) = self
            .catalog
            .find_online_tracked_book(input.source_id, &input.book_url)
            .await?
        {
            return self.catalog.get_book(Some(actor), existing.id).await;
        }

        let managed_lib = self.catalog.ensure_managed_library().await?;
        let book_id = BookId::new();

        // Best-effort cover download
        let cover_payload = if let Some(ref cover_url) = input.cover_url
            && !cover_url.trim().is_empty()
        {
            self.fetch_cover_proxy(cover_url)
                .await
                .ok()
                .map(|(bytes, mime_type)| CoverPayload {
                    bytes: Bytes::from(bytes),
                    mime_type,
                })
        } else {
            None
        };

        // Best-effort initial TOC fetch so manifest & chapter count are populated right away
        let mut toc_url = input.toc_url.trim().to_string();
        let mut last_chapter = input.last_chapter.clone();
        let mut total_chapters = 0u32;

        if toc_url.is_empty()
            && let Ok(info) = self
                .fetch_book_info_internal(
                    src.clone(),
                    &input.book_url,
                    Some(BookJsContext {
                        name: input.name.clone(),
                        author: input.author.clone(),
                        book_url: input.book_url.clone(),
                        toc_url: String::new(),
                        cover_url: input.cover_url.clone(),
                        intro: input.intro.clone(),
                        ..Default::default()
                    }),
                    false,
                )
                .await
        {
            toc_url = info.detail.toc_url;
            if last_chapter.is_none() {
                last_chapter = info.detail.last_chapter;
            }
        }

        if !toc_url.is_empty()
            && let Ok(toc_res) = self
                .fetch_toc_internal(
                    src,
                    &toc_url,
                    BookJsContext {
                        name: input.name.clone(),
                        author: input.author.clone(),
                        book_url: input.book_url.clone(),
                        toc_url: toc_url.clone(),
                        cover_url: input.cover_url.clone(),
                        intro: input.intro.clone(),
                        ..Default::default()
                    },
                    false,
                )
                .await
        {
            let clean_chapters: Vec<OnlineChapterItem> = toc_res
                .chapters
                .into_iter()
                .filter(|c| !c.is_volume && !c.url.trim().is_empty())
                .collect();
            if !clean_chapters.is_empty() {
                if let Some(last) = clean_chapters.last() {
                    last_chapter = Some(last.title.clone());
                }
                total_chapters = clean_chapters.len() as u32;
                let store = self.tracked_store.clone();
                if let Err(err) = tokio::task::spawn_blocking(move || {
                    store.update_manifest_from_toc(book_id, &clean_chapters, false)
                })
                .await
                .map_err(|e| e.to_string())
                .and_then(|r| r)
                {
                    tracing::warn!(book_id = %book_id, error = %err, "failed to save initial tracked manifest");
                }
            }
        }

        // Synthesize initial valid standard EPUB 3.0 archive (skeleton with cover + metadata)
        let pack_meta = PackBookMetadata {
            identifier: Some(book_id.0),
            title: input.name.clone(),
            author: input.author.clone(),
            description: input.intro.clone(),
            cover_bytes: cover_payload
                .as_ref()
                .map(|c| (c.bytes.to_vec(), c.mime_type.clone())),
        };
        let epub_bytes =
            tokio::task::spawn_blocking(move || package_chapters_to_epub(&pack_meta, &[]))
                .await
                .map_err(|e| DomainError::Storage(format!("生成初始 EPUB3 失败: {e}")))?
                .map_err(DomainError::Storage)?;

        let (stored, cover_key) = self
            .catalog
            .save_managed_epub_and_cover(book_id, Bytes::from(epub_bytes), cover_payload)
            .await?;

        let mut metadata = BookMetadata::new(input.name)?;
        if !input.author.trim().is_empty() {
            metadata.authors = vec![input.author.trim().to_string()];
        }
        metadata.description = BookMetadata::normalize_optional(input.intro);
        metadata.tags =
            BookMetadata::normalize_list(Self::parse_tags_from_kind(input.kind.as_deref()));
        metadata.language = Some("zh-CN".to_string());
        metadata.chapter_count = if total_chapters > 0 {
            Some(total_chapters)
        } else {
            None
        };

        let mut book = Book::new(
            managed_lib.id,
            BookFormat::Epub,
            BookSource::OnlineTracked {
                storage_key: stored.storage_key,
                source_id: input.source_id,
                book_url: input.book_url,
                toc_url,
                last_chapter,
                cached_chapters: 0,
                total_chapters,
                epub_dirty: false,
            },
            stored.file_size,
            stored.file_hash,
            cover_key,
            metadata,
        )?;
        book.id = book_id;

        self.catalog.save_book_raw(&book).await?;
        self.catalog.get_book(Some(actor), book_id).await
    }

    /// Returns the TOC for an online-tracked book (with per-chapter cache status).
    /// Fetches fresh TOC from the source when `force_refresh` is true or manifest is empty.
    pub async fn get_tracked_toc(
        self: &Arc<Self>,
        actor: &User,
        book_id: BookId,
        force_refresh: bool,
    ) -> DomainResult<TrackedTocResponseDto> {
        if force_refresh {
            require_admin(actor)?;
        }
        let book = self.catalog.find_book_raw(book_id).await?;
        let (
            source_id,
            book_url,
            toc_url,
            cached_chapters,
            total_chapters,
            last_chapter,
            epub_dirty,
        ) = match &book.source {
            BookSource::OnlineTracked {
                source_id,
                book_url,
                toc_url,
                cached_chapters,
                total_chapters,
                last_chapter,
                epub_dirty,
                ..
            } => (
                *source_id,
                book_url.clone(),
                toc_url.clone(),
                *cached_chapters,
                *total_chapters,
                last_chapter.clone(),
                *epub_dirty,
            ),
            _ => {
                return Err(DomainError::Validation("该书籍不是在线追更书籍".into()));
            }
        };

        let src_rec_opt = self.load_source(source_id).await.ok();
        let source_name = src_rec_opt
            .as_ref()
            .map(|s| s.name.clone())
            .unwrap_or_else(|| "已删除书源".to_string());

        let existing_manifest = {
            let store = self.tracked_store.clone();
            tokio::task::spawn_blocking(move || store.load_manifest(book_id))
                .await
                .map_err(|e| DomainError::Storage(format!("读取目录清单失败: {e}")))?
        };
        if !force_refresh && !existing_manifest.chapters.is_empty() {
            return Ok(TrackedTocResponseDto {
                book_id,
                source_id,
                source_name,
                chapters: existing_manifest.chapters,
                cached_chapters,
                total_chapters,
                last_chapter,
                epub_dirty,
            });
        }

        let src_rec =
            src_rec_opt.ok_or_else(|| DomainError::NotFound(format!("未找到书源 {source_id}")))?;
        if !src_rec.enabled && !actor.role.is_admin() {
            return Err(DomainError::Forbidden(
                "书源已禁用，仅管理员可刷新目录".into(),
            ));
        }

        let effective_toc_url = if toc_url.trim().is_empty() {
            let info = self
                .fetch_book_info_internal(
                    src_rec.clone(),
                    &book_url,
                    Some(BookJsContext {
                        name: book.metadata.title.clone(),
                        author: book.metadata.authors.join(", "),
                        book_url: book_url.clone(),
                        toc_url: String::new(),
                        intro: book.metadata.description.clone(),
                        ..Default::default()
                    }),
                    false,
                )
                .await?;
            info.detail.toc_url
        } else {
            toc_url.clone()
        };

        let toc_res = self
            .fetch_toc_internal(
                src_rec,
                &effective_toc_url,
                BookJsContext {
                    name: book.metadata.title.clone(),
                    author: book.metadata.authors.join(", "),
                    book_url: book_url.clone(),
                    toc_url: effective_toc_url.clone(),
                    intro: book.metadata.description.clone(),
                    ..Default::default()
                },
                false,
            )
            .await?;

        let clean_chapters: Vec<OnlineChapterItem> = toc_res
            .chapters
            .into_iter()
            .filter(|c| !c.is_volume && !c.url.trim().is_empty())
            .collect();

        // Serialize manifest + DB update under per-book state lock and re-read latest Book.
        let state_lock = self.book_state_lock_for(book_id);
        let _state_guard = state_lock.lock().await;
        let mut latest_book = self.catalog.find_book_raw(book_id).await?;
        let current_source_matches = matches!(
            &latest_book.source,
            BookSource::OnlineTracked { source_id: sid, .. } if *sid == source_id
        );
        if !current_source_matches {
            return Err(DomainError::Validation(
                "书籍来源已在拉取目录期间变更，请重新加载目录".into(),
            ));
        }

        let store = self.tracked_store.clone();
        let manifest = tokio::task::spawn_blocking(move || {
            store.update_manifest_from_toc(book_id, &clean_chapters, true)
        })
        .await
        .map_err(|e| DomainError::Storage(format!("更新目录清单线程失败: {e}")))?
        .map_err(DomainError::Storage)?;

        let new_total = manifest.chapters.len() as u32;
        let new_cached = manifest.chapters.iter().filter(|c| c.cached).count() as u32;
        let new_last = manifest.chapters.last().map(|c| c.title.clone());
        let mut latest_dirty = epub_dirty;

        if let BookSource::OnlineTracked {
            toc_url: ref mut t_url,
            last_chapter: ref mut l_ch,
            cached_chapters: ref mut c_cnt,
            total_chapters: ref mut t_cnt,
            epub_dirty: ref dirty,
            ..
        } = latest_book.source
        {
            *t_url = effective_toc_url;
            *l_ch = new_last.clone();
            *c_cnt = new_cached;
            *t_cnt = new_total;
            latest_dirty = *dirty;
        }
        latest_book.metadata.chapter_count = if new_total > 0 { Some(new_total) } else { None };
        latest_book.updated_at = Timestamp::now();
        self.catalog.save_book_raw(&latest_book).await?;

        Ok(TrackedTocResponseDto {
            book_id,
            source_id,
            source_name,
            chapters: manifest.chapters,
            cached_chapters: new_cached,
            total_chapters: new_total,
            last_chapter: new_last,
            epub_dirty: latest_dirty,
        })
    }

    /// Reads a single chapter of an online-tracked book:
    /// serves immediately from local chapter cache if present (with matching URL), otherwise
    /// fetches from the Legado source, saves O(1) chapter file atomically, marks `epub_dirty`,
    /// and schedules EPUB 3.0 sync.
    pub async fn get_tracked_chapter(
        self: &Arc<Self>,
        actor: &User,
        book_id: BookId,
        chapter_index: usize,
        force_refresh: bool,
    ) -> DomainResult<TrackedChapterResponseDto> {
        if force_refresh {
            require_admin(actor)?;
        }
        let book = self.catalog.find_book_raw(book_id).await?;
        let (source_id, book_url, toc_url, cached_chapters, total_chapters, epub_dirty) =
            match &book.source {
                BookSource::OnlineTracked {
                    source_id,
                    book_url,
                    toc_url,
                    cached_chapters,
                    total_chapters,
                    epub_dirty,
                    ..
                } => (
                    *source_id,
                    book_url.clone(),
                    toc_url.clone(),
                    *cached_chapters,
                    *total_chapters,
                    *epub_dirty,
                ),
                _ => {
                    return Err(DomainError::Validation("该书籍不是在线追更书籍".into()));
                }
            };

        let mut manifest = {
            let store = self.tracked_store.clone();
            tokio::task::spawn_blocking(move || store.load_manifest(book_id))
                .await
                .map_err(|e| DomainError::Storage(format!("读取目录清单失败: {e}")))?
        };

        if !force_refresh && let Some(meta) = manifest.chapters.get(chapter_index) {
            let expected_url = meta.url.clone();
            let store = self.tracked_store.clone();
            let cached_opt = tokio::task::spawn_blocking(move || {
                store.load_chapter_matching_url(book_id, chapter_index, &expected_url)
            })
            .await
            .ok()
            .flatten();
            if let Some(cached) = cached_opt {
                return Ok(TrackedChapterResponseDto {
                    book_id,
                    source_id,
                    chapter_index,
                    title: cached.title,
                    content: cached.content,
                    cached: true,
                    cached_chapters,
                    total_chapters,
                    epub_dirty,
                });
            }
        }

        if manifest.chapters.is_empty()
            || (chapter_index >= manifest.chapters.len() && actor.role.is_admin())
        {
            let refreshed = self
                .get_tracked_toc(actor, book_id, !manifest.chapters.is_empty())
                .await?;
            manifest.chapters = refreshed.chapters;
        }
        let chapter_meta = manifest
            .chapters
            .get(chapter_index)
            .cloned()
            .ok_or_else(|| {
                DomainError::NotFound(format!("未找到第 {} 章目录信息", chapter_index + 1))
            })?;
        let book_ctx = BookJsContext {
            name: book.metadata.title.clone(),
            author: book.metadata.authors.join(", "),
            book_url: book_url.clone(),
            toc_url: toc_url.clone(),
            intro: book.metadata.description.clone(),
            dur_chapter_index: chapter_index,
            dur_chapter_title: chapter_meta.title.clone(),
            ..Default::default()
        };
        let chapter_ctx = ChapterJsContext {
            title: chapter_meta.title.clone(),
            url: chapter_meta.url.clone(),
            index: chapter_index,
        };

        let src_rec = self.load_source(source_id).await?;
        if !src_rec.enabled && !actor.role.is_admin() {
            return Err(DomainError::Forbidden(
                "书源已禁用，仅管理员可拉取未缓存章节".into(),
            ));
        }
        let fetched = self
            .fetch_chapter_content_internal(
                src_rec,
                &chapter_meta.url,
                book_ctx,
                chapter_ctx,
                ChapterFetchOptions {
                    force_refresh,
                    include_logs: false,
                    allow_shared_cache_write: true,
                },
            )
            .await?;

        let resolved_title = if fetched.content.title.trim().is_empty() {
            chapter_meta.title.clone()
        } else {
            fetched.content.title.clone()
        };
        let chapter_file = TrackedChapterFile {
            index: chapter_index,
            title: resolved_title.clone(),
            url: chapter_meta.url.clone(),
            content: fetched.content.content.clone(),
        };

        // Coordinate manifest write + generation bump + latest Book DB update under `book_state_lock`.
        let (new_cached, new_total, sync_gen, should_flush_now) = {
            let state_lock = self.book_state_lock_for(book_id);
            let _state_guard = state_lock.lock().await;

            let mut latest_book = self.catalog.find_book_raw(book_id).await?;
            let source_still_matches = matches!(
                &latest_book.source,
                BookSource::OnlineTracked { source_id: sid, .. } if *sid == source_id
            );
            if !source_still_matches {
                return Err(DomainError::Validation(
                    "书籍来源已在章节抓取期间变更，请重新加载".into(),
                ));
            }

            let store = self.tracked_store.clone();
            let cf = chapter_file;
            let new_cached = tokio::task::spawn_blocking(move || store.save_chapter(book_id, &cf))
                .await
                .map_err(|e| DomainError::Storage(format!("写入章节缓存线程失败: {e}")))?
                .map_err(DomainError::Storage)?;
            let new_total = manifest.chapters.len() as u32;

            let (sync_gen, should_flush_now) = self.bump_sync_gen(book_id, 1);

            if let BookSource::OnlineTracked {
                cached_chapters: ref mut c_cnt,
                total_chapters: ref mut t_cnt,
                epub_dirty: ref mut dirty,
                ..
            } = latest_book.source
            {
                *c_cnt = new_cached;
                *t_cnt = new_total.max(*t_cnt);
                *dirty = true;
            }
            latest_book.updated_at = Timestamp::now();
            self.catalog.save_book_raw(&latest_book).await?;

            (new_cached, new_total, sync_gen, should_flush_now)
        };

        self.spawn_debounced_epub_sync(book_id, sync_gen, should_flush_now);

        Ok(TrackedChapterResponseDto {
            book_id,
            source_id,
            chapter_index,
            title: resolved_title,
            content: fetched.content.content,
            cached: false,
            cached_chapters: new_cached,
            total_chapters: new_total,
            epub_dirty: true,
        })
    }

    /// Switches the upstream book source for a unified online-tracked book while preserving
    /// its `BookId`, shelves, favorites, tags, and reading progress.
    /// Fetches the new source's TOC (and optional cover) BEFORE committing DB changes or clearing old cache.
    pub async fn switch_tracked_source(
        self: &Arc<Self>,
        actor: &User,
        book_id: BookId,
        mut input: SwitchTrackedSourceInput,
    ) -> DomainResult<TrackedTocResponseDto> {
        require_admin(actor)?;
        let new_src = self.load_source(input.source_id).await?;
        input.book_url = Self::validate_caller_source_url(&input.book_url, &new_src.raw_json)?;
        if !input.toc_url.trim().is_empty() {
            input.toc_url = Self::validate_caller_source_url(&input.toc_url, &new_src.raw_json)?;
        }
        if let Some(ref cover_url) = input.cover_url
            && !cover_url.trim().is_empty()
        {
            input.cover_url = Some(Self::validate_caller_http_url(
                cover_url,
                &new_src.raw_json,
            )?);
        }
        let book_snapshot = self.catalog.find_book_raw(book_id).await?;

        if !book_snapshot.source.is_online_tracked() {
            return Err(DomainError::Validation("该书籍不是在线追更书籍".into()));
        }

        // 1. Pre-fetch new TOC (and book_info if toc_url is empty) BEFORE touching DB or old cache
        let effective_toc_url = if input.toc_url.trim().is_empty() {
            let info = self
                .fetch_book_info_internal(
                    new_src.clone(),
                    &input.book_url,
                    Some(BookJsContext {
                        name: book_snapshot.metadata.title.clone(),
                        author: book_snapshot.metadata.authors.join(", "),
                        book_url: input.book_url.clone(),
                        toc_url: String::new(),
                        cover_url: input.cover_url.clone(),
                        intro: input
                            .intro
                            .clone()
                            .or_else(|| book_snapshot.metadata.description.clone()),
                        ..Default::default()
                    }),
                    false,
                )
                .await?;
            info.detail.toc_url
        } else {
            input.toc_url.trim().to_string()
        };

        let toc_res = self
            .fetch_toc_internal(
                new_src.clone(),
                &effective_toc_url,
                BookJsContext {
                    name: book_snapshot.metadata.title.clone(),
                    author: book_snapshot.metadata.authors.join(", "),
                    book_url: input.book_url.clone(),
                    toc_url: effective_toc_url.clone(),
                    cover_url: input.cover_url.clone(),
                    intro: input
                        .intro
                        .clone()
                        .or_else(|| book_snapshot.metadata.description.clone()),
                    ..Default::default()
                },
                false,
            )
            .await?;

        let clean_chapters: Vec<OnlineChapterItem> = toc_res
            .chapters
            .into_iter()
            .filter(|c| !c.is_volume && !c.url.trim().is_empty())
            .collect();
        if clean_chapters.is_empty() {
            return Err(DomainError::Validation(
                "新书源未解析到有效章节目录，已取消换源以保留原书数据".into(),
            ));
        }

        // Best-effort download & save new cover if `input.cover_url` was provided
        let new_cover_key = if let Some(ref cover_url) = input.cover_url
            && !cover_url.trim().is_empty()
        {
            if let Ok((bytes, mime_type)) = self.fetch_cover_proxy(cover_url).await {
                self.catalog
                    .save_cover_raw(
                        book_id,
                        &CoverPayload {
                            bytes: Bytes::from(bytes),
                            mime_type,
                        },
                    )
                    .await
                    .ok()
            } else {
                None
            }
        } else {
            None
        };

        // 2. Commit cache update + DB update atomically under `book_state_lock`
        let (manifest, new_cached, new_total, new_last, sync_gen) = {
            let state_lock = self.book_state_lock_for(book_id);
            let _state_guard = state_lock.lock().await;

            let mut latest_book = self.catalog.find_book_raw(book_id).await?;
            if !latest_book.source.is_online_tracked() {
                return Err(DomainError::Validation("该书籍不是在线追更书籍".into()));
            }

            let store = self.tracked_store.clone();
            let clear_cache = input.clear_cache;
            let manifest = tokio::task::spawn_blocking(move || {
                if clear_cache {
                    store.clear_chapters(book_id);
                }
                store.update_manifest_from_toc(book_id, &clean_chapters, !clear_cache)
            })
            .await
            .map_err(|e| DomainError::Storage(format!("切换书源更新清单线程失败: {e}")))?
            .map_err(DomainError::Storage)?;

            let new_total = manifest.chapters.len() as u32;
            let new_cached = manifest.chapters.iter().filter(|c| c.cached).count() as u32;
            let new_last = manifest
                .chapters
                .last()
                .map(|c| c.title.clone())
                .or(input.last_chapter);

            if let BookSource::OnlineTracked {
                source_id,
                book_url,
                toc_url,
                last_chapter,
                cached_chapters,
                total_chapters,
                epub_dirty,
                ..
            } = &mut latest_book.source
            {
                *source_id = input.source_id;
                *book_url = input.book_url;
                *toc_url = effective_toc_url;
                *last_chapter = new_last.clone();
                *cached_chapters = new_cached;
                *total_chapters = new_total;
                *epub_dirty = true;
            }
            if let Some(intro) = input.intro
                && !intro.trim().is_empty()
            {
                latest_book.metadata.description = Some(intro.trim().to_string());
            }
            if let Some(ck) = new_cover_key {
                latest_book.set_cover(Some(ck));
            }
            latest_book.metadata.chapter_count = if new_total > 0 { Some(new_total) } else { None };
            latest_book.updated_at = Timestamp::now();

            // Bump generation so any in-flight `sync_tracked_epub` will NOT clear `epub_dirty`
            let (sync_gen, _) = self.bump_sync_gen(book_id, 1);
            self.catalog.save_book_raw(&latest_book).await?;

            (manifest, new_cached, new_total, new_last, sync_gen)
        };

        self.spawn_debounced_epub_sync(book_id, sync_gen, false);

        Ok(TrackedTocResponseDto {
            book_id,
            source_id: input.source_id,
            source_name: new_src.name,
            chapters: manifest.chapters,
            cached_chapters: new_cached,
            total_chapters: new_total,
            last_chapter: new_last,
            epub_dirty: true,
        })
    }

    /// Spawns a debounced background standard EPUB 3.0 rebuild task for `sync_gen`.
    fn spawn_debounced_epub_sync(
        self: &Arc<Self>,
        book_id: BookId,
        sync_gen: u64,
        should_flush_now: bool,
    ) {
        let svc = Arc::clone(self);
        tokio::spawn(async move {
            if !should_flush_now {
                tokio::time::sleep(TRACKED_EPUB_DEBOUNCE).await;
                if svc.current_sync_gen(book_id) != sync_gen {
                    return;
                }
            }
            if let Err(err) = svc.ensure_tracked_epub_synced(book_id).await {
                tracing::warn!(book_id = %book_id, error = %err, "background EPUB 3.0 sync failed");
            }
        });
    }

    /// Ensures the standard EPUB 3.0 archive on disk reflects all cached chapters if `epub_dirty`.
    pub async fn ensure_tracked_epub_synced(self: &Arc<Self>, book_id: BookId) -> DomainResult<()> {
        let book = self.catalog.find_book_raw(book_id).await?;
        let is_dirty = matches!(
            &book.source,
            BookSource::OnlineTracked {
                epub_dirty: true,
                ..
            }
        );
        if !is_dirty {
            return Ok(());
        }
        let _ = self.sync_tracked_epub(book_id, false).await?;
        Ok(())
    }

    /// Atomically synthesizes the standard EPUB 3.0 file for an online-tracked book from its
    /// locally cached chapters + cover + metadata, updating `BookModel` in-place.
    /// Uses generation tracking (`start_gen` vs `current_gen`) and re-reads the latest `Book`
    /// under `book_state_lock` so concurrent chapter fetches or source switches are never lost.
    pub async fn sync_tracked_epub(
        self: &Arc<Self>,
        book_id: BookId,
        force: bool,
    ) -> DomainResult<BookSummaryDto> {
        let epub_lock = self.epub_lock_for(book_id);
        let _epub_guard = epub_lock.lock().await;

        let (start_gen, title, author, description, cover_key) = {
            let state_lock = self.book_state_lock_for(book_id);
            let _state_guard = state_lock.lock().await;

            let book = self.catalog.find_book_raw(book_id).await?;
            let is_dirty = match &book.source {
                BookSource::OnlineTracked { epub_dirty, .. } => *epub_dirty,
                _ => {
                    return Err(DomainError::Validation("该书籍不是在线追更书籍".into()));
                }
            };
            if !force && !is_dirty {
                return self.catalog.get_book(None, book_id).await;
            }
            (
                self.current_sync_gen(book_id),
                book.metadata.title.clone(),
                book.metadata.authors.join(", "),
                book.metadata.description.clone(),
                book.cover_key.clone(),
            )
        };

        let store = self.tracked_store.clone();
        let (chapters, cached_count, total_count) =
            tokio::task::spawn_blocking(move || store.collect_cached_chapters_for_epub(book_id))
                .await
                .map_err(|e| DomainError::Storage(format!("收集章节缓存线程失败: {e}")))?;

        let cover_bytes = match cover_key.as_deref() {
            Some(key) => self.catalog.read_cover_bytes(key).await,
            None => None,
        };

        let pack_meta = PackBookMetadata {
            identifier: Some(book_id.0),
            title,
            author,
            description,
            cover_bytes,
        };
        let epub_bytes =
            tokio::task::spawn_blocking(move || package_chapters_to_epub(&pack_meta, &chapters))
                .await
                .map_err(|e| DomainError::Storage(format!("合成 EPUB3 线程失败: {e}")))?
                .map_err(DomainError::Storage)?;

        let (stored, _) = self
            .catalog
            .save_managed_epub_and_cover(book_id, Bytes::from(epub_bytes), None)
            .await?;

        let reschedule_gen = {
            let state_lock = self.book_state_lock_for(book_id);
            let _state_guard = state_lock.lock().await;

            let mut latest_book = match self.catalog.find_book_raw(book_id).await {
                Ok(b) => b,
                Err(err) => {
                    // Clean up the newly written EPUB if the book was deleted concurrently.
                    let _ = self
                        .catalog
                        .delete_managed_book_file(&stored.storage_key)
                        .await;
                    return Err(err);
                }
            };
            let current_gen = self.current_sync_gen(book_id);
            let is_clean = current_gen == start_gen;

            latest_book.file_size = stored.file_size;
            latest_book.file_hash = stored.file_hash;
            if let BookSource::OnlineTracked {
                storage_key,
                cached_chapters,
                total_chapters,
                epub_dirty,
                ..
            } = &mut latest_book.source
            {
                *storage_key = stored.storage_key;
                if is_clean {
                    *cached_chapters = cached_count;
                    if total_count > 0 {
                        *total_chapters = total_count;
                    }
                    *epub_dirty = false;
                } else {
                    *epub_dirty = true;
                }
            }
            if is_clean {
                latest_book.metadata.chapter_count = if total_count > 0 {
                    Some(total_count)
                } else if cached_count > 0 {
                    Some(cached_count)
                } else {
                    latest_book.metadata.chapter_count
                };
                if let Some(entry) = self
                    .pending_sync_gens
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get_mut(&book_id)
                {
                    entry.1 = 0;
                }
            }
            latest_book.updated_at = Timestamp::now();
            self.catalog.save_book_raw(&latest_book).await?;

            if is_clean { None } else { Some(current_gen) }
        };

        drop(_epub_guard);
        if let Some(next_gen) = reschedule_gen {
            self.spawn_debounced_epub_sync(book_id, next_gen, false);
        }

        self.catalog.get_book(None, book_id).await
    }
}

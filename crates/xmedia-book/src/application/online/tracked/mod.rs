mod sync;

use std::sync::Arc;

use bytes::Bytes;
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::user::User;

use crate::application::dto::BookSummaryDto;
use crate::application::ports::CoverPayload;
use crate::domain::book::{Book, BookFormat, BookId, BookMetadata, BookSource};
use crate::domain::online::{
    BookJsContext, ChapterJsContext, OnlineChapterItem, PackBookMetadata, TrackedChapterFile,
};

use super::dto::{TrackOnlineBookInput, TrackedChapterResponseDto, TrackedTocResponseDto};
use super::helpers::{ChapterFetchOptions, weak_lock};
use super::{BookOnlineService, TRACKED_EPUB_IMMEDIATE_BATCH};

impl BookOnlineService {
    pub(in crate::application::online) fn book_state_lock_for(
        &self,
        book_id: BookId,
    ) -> Arc<tokio::sync::Mutex<()>> {
        weak_lock(&self.book_state_locks, book_id)
    }

    pub(super) fn epub_lock_for(&self, book_id: BookId) -> Arc<tokio::sync::Mutex<()>> {
        weak_lock(&self.epub_sync_locks, book_id)
    }

    fn track_lock_for(&self, source_id: Uuid, book_url: &str) -> Arc<tokio::sync::Mutex<()>> {
        weak_lock(&self.track_locks, (source_id, book_url.to_string()))
    }

    pub(in crate::application::online) fn bump_sync_gen(
        &self,
        book_id: BookId,
        delta_chapters: u32,
    ) -> (u64, bool) {
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

    pub(super) fn current_sync_gen(&self, book_id: BookId) -> u64 {
        self.pending_sync_gens
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&book_id)
            .map(|(g, _)| *g)
            .unwrap_or(0)
    }

    pub async fn track_online_book(
        self: &Arc<Self>,
        actor: &User,
        mut input: TrackOnlineBookInput,
    ) -> DomainResult<BookSummaryDto> {
        let src = self
            .sources
            .load_source_for_actor(actor, input.source_id)
            .await?;
        input.book_url = Self::validate_caller_source_url(&input.book_url, &src.raw_json)?;
        if !input.toc_url.trim().is_empty() {
            input.toc_url = Self::validate_caller_source_url(&input.toc_url, &src.raw_json)?;
        }
        if let Some(ref cover_url) = input.cover_url
            && !cover_url.trim().is_empty()
        {
            input.cover_url = Some(Self::validate_caller_http_url(cover_url, &src.raw_json)?);
        }

        let tlock = self.track_lock_for(input.source_id, &input.book_url);
        let _track_guard = tlock.lock().await;

        if let Some(existing) = self
            .catalog
            .find_online_tracked_book(input.source_id, &input.book_url)
            .await?
        {
            self.catalog
                .bind_book_for_user(actor.id, existing.id)
                .await?;
            return self.catalog.get_book(Some(actor), existing.id).await;
        }

        let managed_lib = self.catalog.ensure_managed_library().await?;
        let book_id = BookId::new();

        let cover_payload = if let Some(ref cover_url) = input.cover_url
            && !cover_url.trim().is_empty()
        {
            self.fetch_cover_proxy(cover_url)
                .await
                .ok()
                .map(|(bytes, mime_type)| CoverPayload::new(Bytes::from(bytes), mime_type))
        } else {
            None
        };

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
                let store = Arc::clone(&self.tracked_store);
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

        let pack_meta = PackBookMetadata {
            identifier: Some(book_id.0),
            title: input.name.clone(),
            author: input.author.clone(),
            description: input.intro.clone(),
            cover_bytes: cover_payload
                .as_ref()
                .map(|c| (c.bytes.to_vec(), c.mime_type.clone())),
        };
        let packager = Arc::clone(&self.packager);
        let epub_bytes =
            tokio::task::spawn_blocking(move || packager.package_epub(&pack_meta, &[]))
                .await
                .map_err(|e| DomainError::Storage(format!("生成初始 EPUB3 失败: {e}")))?
                .map_err(DomainError::Storage)?;

        let (stored, cover_key) = self
            .catalog
            .save_managed_epub_and_cover(book_id, Bytes::from(epub_bytes), cover_payload)
            .await?;

        let metadata = BookMetadata::from_online_tracked(
            input.name,
            &input.author,
            input.intro,
            input.kind.as_deref(),
            total_chapters,
        )?;

        let book = Book::new(
            book_id,
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

        self.catalog.save_book_raw(&book).await?;
        self.catalog.bind_book_for_user(actor.id, book_id).await?;
        self.catalog.get_book(Some(actor), book_id).await
    }

    pub async fn get_tracked_toc(
        self: &Arc<Self>,
        actor: &User,
        book_id: BookId,
        force_refresh: bool,
    ) -> DomainResult<TrackedTocResponseDto> {
        self.catalog.require_book_bound(actor, book_id).await?;
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

        let src_rec_opt = self.sources.load_source(source_id).await.ok();
        let source_name = src_rec_opt
            .as_ref()
            .map(|s| s.name.clone())
            .unwrap_or_else(|| "已删除书源".to_string());

        let existing_manifest = {
            let store = Arc::clone(&self.tracked_store);
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

        let store = Arc::clone(&self.tracked_store);
        let manifest = tokio::task::spawn_blocking(move || {
            store.update_manifest_from_toc(book_id, &clean_chapters, true)
        })
        .await
        .map_err(|e| DomainError::Storage(format!("更新目录清单线程失败: {e}")))?
        .map_err(DomainError::Storage)?;

        let new_total = manifest.chapters.len() as u32;
        let new_cached = manifest.chapters.iter().filter(|c| c.cached).count() as u32;
        let new_last = manifest.chapters.last().map(|c| c.title.clone());
        let latest_dirty = latest_book.sync_tracked(crate::domain::book::TrackedSync::TocRefresh {
            effective_toc_url,
            new_last_chapter: new_last.clone(),
            new_cached,
            new_total,
        });
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

    pub async fn get_tracked_chapter(
        self: &Arc<Self>,
        actor: &User,
        book_id: BookId,
        chapter_index: usize,
        force_refresh: bool,
    ) -> DomainResult<TrackedChapterResponseDto> {
        self.catalog.require_book_bound(actor, book_id).await?;
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
            let store = Arc::clone(&self.tracked_store);
            tokio::task::spawn_blocking(move || store.load_manifest(book_id))
                .await
                .map_err(|e| DomainError::Storage(format!("读取目录清单失败: {e}")))?
        };

        if !force_refresh && let Some(meta) = manifest.chapters.get(chapter_index) {
            let expected_url = meta.url.clone();
            let store = Arc::clone(&self.tracked_store);
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

        let src_rec = self.sources.load_source(source_id).await?;
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

            let store = Arc::clone(&self.tracked_store);
            let cf = chapter_file;
            let new_cached = tokio::task::spawn_blocking(move || store.save_chapter(book_id, &cf))
                .await
                .map_err(|e| DomainError::Storage(format!("写入章节缓存线程失败: {e}")))?
                .map_err(DomainError::Storage)?;
            let new_total = manifest.chapters.len() as u32;

            let (sync_gen, should_flush_now) = self.bump_sync_gen(book_id, 1);

            latest_book.sync_tracked(crate::domain::book::TrackedSync::ChapterCached {
                new_cached,
                new_total,
            });
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
}

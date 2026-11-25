use std::sync::Arc;

use bytes::Bytes;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::user::{User, require_admin};

use crate::application::dto::BookSummaryDto;
use crate::application::online::dto::{SwitchTrackedSourceInput, TrackedTocResponseDto};
use crate::application::online::{BookOnlineService, TRACKED_EPUB_DEBOUNCE};
use crate::application::ports::CoverPayload;
use crate::domain::book::{BookId, BookSource};
use crate::domain::online::{BookJsContext, OnlineChapterItem, PackBookMetadata};

impl BookOnlineService {
    pub async fn switch_tracked_source(
        self: &Arc<Self>,
        actor: &User,
        book_id: BookId,
        mut input: SwitchTrackedSourceInput,
    ) -> DomainResult<TrackedTocResponseDto> {
        require_admin(actor)?;
        let new_src = self.sources.load_source(input.source_id).await?;
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

        let new_cover_key = if let Some(ref cover_url) = input.cover_url
            && !cover_url.trim().is_empty()
        {
            if let Ok((bytes, mime_type)) = self.fetch_cover_proxy(cover_url).await {
                self.catalog
                    .save_cover_raw(book_id, &CoverPayload::new(Bytes::from(bytes), mime_type))
                    .await
                    .ok()
            } else {
                None
            }
        } else {
            None
        };

        let (manifest, new_cached, new_total, new_last, sync_gen) = {
            let state_lock = self.book_state_lock_for(book_id);
            let _state_guard = state_lock.lock().await;

            let mut latest_book = self.catalog.find_book_raw(book_id).await?;
            if !latest_book.source.is_online_tracked() {
                return Err(DomainError::Validation("该书籍不是在线追更书籍".into()));
            }

            let store = Arc::clone(&self.tracked_store);
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

            latest_book.switch_online_tracking(
                input.source_id,
                input.book_url,
                effective_toc_url,
                new_last.clone(),
                new_cached,
                new_total,
                input.intro,
                new_cover_key,
            );

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

    pub(super) fn spawn_debounced_epub_sync(
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

        let store = Arc::clone(&self.tracked_store);
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
        let packager = Arc::clone(&self.packager);
        let epub_bytes =
            tokio::task::spawn_blocking(move || packager.package_epub(&pack_meta, &chapters))
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
                    let _ = self
                        .catalog
                        .delete_managed_book_file(&stored.storage_key)
                        .await;
                    return Err(err);
                }
            };
            let current_gen = self.current_sync_gen(book_id);
            let is_clean = current_gen == start_gen;

            latest_book.commit_synced_tracked_epub(
                stored.storage_key,
                stored.file_size,
                stored.file_hash,
                cached_count,
                total_count,
                is_clean,
            );
            if is_clean
                && let Some(entry) = self
                    .pending_sync_gens
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get_mut(&book_id)
            {
                entry.1 = 0;
            }
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

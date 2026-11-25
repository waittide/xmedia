use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

use bytes::Bytes;
use jiff::Timestamp;
use tokio::sync::{Notify, mpsc};
use uuid::Uuid;
use xmedia_core::domain::user::User;

use crate::application::online::dto::{
    DownloadJobStatus, OnlineDownloadFormat, OnlineDownloadJobDto, StartDownloadInput,
};
use crate::application::online::helpers::{
    ChapterProgressEvent, DownloadRateLimiter, compute_chapters_per_sec, sanitize_file_stem,
};
use crate::application::online::{BookOnlineService, DOWNLOAD_STATE_PERSIST_BATCH};
use crate::domain::book::{BookId, BookSource};
use crate::domain::online::{
    BookJsContext, ChapterJsContext, LegadoBookSource, OnlineChapterItem, PackBookMetadata,
    PackChapterInput, TrackedChapterFile,
};

impl BookOnlineService {
    pub(super) async fn execute_download_job(
        self: Arc<Self>,
        job_id: Uuid,
        actor: User,
        input: StartDownloadInput,
        cancel_flag: Arc<AtomicBool>,
        cancel_cond: Arc<(Mutex<bool>, Condvar)>,
        cancel_notify: Arc<Notify>,
    ) {
        let started_at = Instant::now();
        {
            let mut guard = self.downloads.write().await;
            let Some(slot) = guard.get_mut(&job_id) else {
                return;
            };
            if cancel_flag.load(Ordering::Acquire)
                || slot.dto.status == DownloadJobStatus::Cancelled
            {
                slot.dto.status = DownloadJobStatus::Cancelled;
                slot.dto.current_chapter_title = None;
                slot.dto.updated_at = Timestamp::now();
                return;
            }
            slot.started_at = Some(started_at);
            slot.dto.status = DownloadJobStatus::Running;
            slot.dto.current_chapter_title = Some("正在拉取章节目录…".into());
            slot.dto.updated_at = Timestamp::now();
        }

        let src_rec = match self
            .sources
            .load_source_for_actor(&actor, input.source_id)
            .await
        {
            Ok(r) => r,
            Err(e) => {
                self.fail_job(job_id, started_at, format!("加载书源失败: {e}"))
                    .await;
                return;
            }
        };
        let parsed_source = match serde_json::from_str::<LegadoBookSource>(&src_rec.raw_json) {
            Ok(p) => Arc::new(p),
            Err(e) => {
                self.fail_job(job_id, started_at, format!("书源 JSON 无效: {e}"))
                    .await;
                return;
            }
        };
        let mut last_persisted_state = src_rec.runtime_state.clone();
        let reusable_ctx = match self.engine.prepare_batch_context(
            Arc::clone(&parsed_source),
            src_rec.runtime_state,
            cancel_flag.clone(),
        ) {
            Ok(c) => c,
            Err(e) => {
                self.fail_job(job_id, started_at, format!("初始化书源会话失败: {e}"))
                    .await;
                return;
            }
        };

        let book_ctx = BookJsContext {
            name: input.book_name.clone(),
            author: input.author.clone(),
            book_url: input.book_url.clone(),
            toc_url: input.toc_url.clone(),
            cover_url: input.cover_url.clone(),
            intro: input.intro.clone(),
            ..Default::default()
        };

        let toc_cancelled = cancel_notify.notified();
        tokio::pin!(toc_cancelled);
        if cancel_flag.load(Ordering::Acquire) {
            self.mark_job_cancelled(job_id, started_at).await;
            return;
        }

        let toc_task = {
            let ctx = Arc::clone(&reusable_ctx);
            let toc_url = input.toc_url.clone();
            let bctx = book_ctx.clone();
            tokio::task::spawn_blocking(move || ctx.fetch_toc_sync(&toc_url, &bctx))
        };

        let toc_res = tokio::select! {
            biased;
            _ = &mut toc_cancelled => {
                self.mark_job_cancelled(job_id, started_at).await;
                return;
            }
            res = toc_task => match res {
                Ok(inner) => inner,
                Err(e) => Err(format!("获取目录线程失败: {e}")),
            },
        };

        if cancel_flag.load(Ordering::Acquire) {
            self.mark_job_cancelled(job_id, started_at).await;
            return;
        }

        let all_chapters = match toc_res {
            Ok(list) => list
                .into_iter()
                .filter(|c| !c.is_volume && !c.url.trim().is_empty())
                .collect::<Vec<_>>(),
            Err(e) => {
                self.fail_job(job_id, started_at, format!("获取目录失败: {e}"))
                    .await;
                return;
            }
        };

        if all_chapters.is_empty() {
            self.fail_job(job_id, started_at, "目录为空，无可下载章节".into())
                .await;
            return;
        }

        let start = input.start_chapter.unwrap_or(0).min(all_chapters.len());
        let end = input
            .end_chapter
            .map(|e| e.saturating_add(1).min(all_chapters.len()))
            .unwrap_or(all_chapters.len())
            .max(start);
        let selected: Vec<OnlineChapterItem> = all_chapters[start..end].to_vec();

        let target_tracked_book_id: Option<BookId> =
            if input.import_to_library && actor.role.is_admin() {
                if let Some(bid) = input.book_id {
                    self.catalog
                        .find_book_raw(BookId(bid))
                        .await
                        .ok()
                        .filter(|b| {
                            matches!(
                                &b.source,
                                BookSource::OnlineTracked {
                                    source_id,
                                    toc_url,
                                    ..
                                } if *source_id == input.source_id
                                    && (toc_url.trim().is_empty()
                                        || toc_url.trim() == input.toc_url.trim())
                            )
                        })
                        .map(|b| b.id)
                } else if input.format == OnlineDownloadFormat::Epub {
                    self.catalog
                        .find_online_tracked_book(input.source_id, &input.book_url)
                        .await
                        .ok()
                        .flatten()
                        .map(|b| b.id)
                } else {
                    None
                }
            } else {
                None
            };

        if let Some(bid) = target_tracked_book_id {
            let state_lock = self.book_state_lock_for(bid);
            let _state_guard = state_lock.lock().await;
            let source_matches = self
                .catalog
                .find_book_raw(bid)
                .await
                .is_ok_and(|b| matches!(&b.source, BookSource::OnlineTracked { source_id, .. } if *source_id == input.source_id));
            if source_matches {
                let store = Arc::clone(&self.tracked_store);
                let toc_for_manifest = all_chapters.clone();
                if let Err(err) = tokio::task::spawn_blocking(move || {
                    store.update_manifest_from_toc(bid, &toc_for_manifest, true)
                })
                .await
                .map_err(|e| e.to_string())
                .and_then(|r| r)
                {
                    tracing::warn!(book_id = %bid, error = %err, "failed to update tracked manifest before download");
                }
            }
        }
        if selected.is_empty() {
            self.fail_job(job_id, started_at, "所选章节区间为空".into())
                .await;
            return;
        }

        let total_selected = selected.len();
        self.update_job(job_id, |j| {
            j.total_chapters = total_selected;
            j.elapsed_ms = started_at.elapsed().as_millis() as u64;
        })
        .await;

        let (concurrency, rate_limiter) =
            DownloadRateLimiter::from_spec(parsed_source.parse_concurrent_rate());
        let rate_limiter = Arc::new(rate_limiter);
        let selected_arc = Arc::new(selected);
        let cursor = Arc::new(AtomicUsize::new(0));
        let worker_count = concurrency.min(total_selected).max(1);
        let (tx, mut rx) = mpsc::unbounded_channel::<ChapterProgressEvent>();

        let mut worker_handles = Vec::with_capacity(worker_count);
        for _ in 0..worker_count {
            let selected_w = Arc::clone(&selected_arc);
            let cursor_w = Arc::clone(&cursor);
            let ctx_w = Arc::clone(&reusable_ctx);
            let limiter_w = Arc::clone(&rate_limiter);
            let cancel_flag_w = Arc::clone(&cancel_flag);
            let cancel_cond_w = Arc::clone(&cancel_cond);
            let store_w = Arc::clone(&self.tracked_store);
            let source_id = input.source_id;
            let book_ctx_w = book_ctx.clone();
            let tx_w = tx.clone();

            worker_handles.push(tokio::task::spawn_blocking(move || {
                let mut session = ctx_w.create_worker_session();
                loop {
                    if cancel_flag_w.load(Ordering::Acquire) {
                        break;
                    }
                    let idx = cursor_w.fetch_add(1, Ordering::Relaxed);
                    if idx >= selected_w.len() {
                        break;
                    }
                    let ch = &selected_w[idx];

                    if let Some(cached) = store_w.read_cached_online_chapter(source_id, &ch.url)
                        && !cached.content.trim().is_empty()
                    {
                        let packed = PackChapterInput {
                            title: if cached.title.trim().is_empty() {
                                ch.title.clone()
                            } else {
                                cached.title
                            },
                            content: cached.content,
                        };
                        if tx_w
                            .send(ChapterProgressEvent {
                                idx,
                                chapter_title: ch.title.clone(),
                                packed,
                                failed: false,
                                from_cache: true,
                            })
                            .is_err()
                        {
                            break;
                        }
                        continue;
                    }

                    if !limiter_w.wait_for_slot(&cancel_flag_w, &cancel_cond_w) {
                        break;
                    }

                    session.reset_chapter_budget();
                    let mut cur_book_ctx = book_ctx_w.clone();
                    cur_book_ctx.dur_chapter_index = ch.index;
                    cur_book_ctx.dur_chapter_title = ch.title.clone();
                    let ch_ctx = ChapterJsContext {
                        index: ch.index,
                        title: ch.title.clone(),
                        url: ch.url.clone(),
                    };

                    let (packed, failed) =
                        match session.fetch_chapter_content(&ch.url, &cur_book_ctx, &ch_ctx) {
                            Ok(content) if !content.content.trim().is_empty() => {
                                store_w.write_cached_online_chapter(source_id, &ch.url, &content);
                                let title = if content.title.trim().is_empty() {
                                    ch.title.clone()
                                } else {
                                    content.title
                                };
                                (
                                    PackChapterInput {
                                        title,
                                        content: content.content,
                                    },
                                    false,
                                )
                            }
                            _ => (
                                PackChapterInput {
                                    title: ch.title.clone(),
                                    content: "[本章抓取失败，请稍后重试]".into(),
                                },
                                true,
                            ),
                        };

                    if tx_w
                        .send(ChapterProgressEvent {
                            idx,
                            chapter_title: ch.title.clone(),
                            packed,
                            failed,
                            from_cache: false,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            }));
        }
        drop(tx);

        let mut ordered_chapters: Vec<Option<PackChapterInput>> = vec![None; total_selected];
        let mut processed_count = 0usize;
        let mut failed_count = 0usize;
        let mut uncached_since_persist = 0usize;
        let mut tracked_chapters_written = 0u32;
        let mut was_cancelled = false;

        let loop_cancelled = cancel_notify.notified();
        tokio::pin!(loop_cancelled);

        loop {
            if cancel_flag.load(Ordering::Acquire) {
                was_cancelled = true;
                break;
            }
            tokio::select! {
                biased;
                _ = &mut loop_cancelled => {
                    was_cancelled = true;
                    break;
                }
                maybe_ev = rx.recv() => {
                    let Some(ev) = maybe_ev else {
                        break;
                    };
                    if let Some(slot) = ordered_chapters.get_mut(ev.idx) {
                        if !ev.failed && let Some(bid) = target_tracked_book_id {
                            let state_lock = self.book_state_lock_for(bid);
                            let _state_guard = state_lock.lock().await;
                            let source_matches = self.catalog.find_book_raw(bid).await.is_ok_and(
                                |b| matches!(&b.source, BookSource::OnlineTracked { source_id, .. } if *source_id == input.source_id),
                            );
                            if source_matches {
                                let global_idx = start + ev.idx;
                                let chapter_url = selected_arc
                                    .get(ev.idx)
                                    .map(|c| c.url.clone())
                                    .unwrap_or_default();
                                let store = Arc::clone(&self.tracked_store);
                                let chapter_file = TrackedChapterFile {
                                    index: global_idx,
                                    title: ev.packed.title.clone(),
                                    url: chapter_url,
                                    content: ev.packed.content.clone(),
                                };
                                if let Err(err) = tokio::task::spawn_blocking(move || {
                                    store.save_chapter(bid, &chapter_file)
                                })
                                .await
                                .map_err(|e| e.to_string())
                                .and_then(|r| r)
                                {
                                    tracing::warn!(book_id = %bid, index = global_idx, error = %err, "failed to save downloaded chapter to tracked store");
                                } else {
                                    tracked_chapters_written = tracked_chapters_written.saturating_add(1);
                                    self.bump_sync_gen(bid, 1);
                                }
                            }
                        }
                        *slot = Some(ev.packed);
                    }
                    processed_count += 1;
                    if ev.failed {
                        failed_count += 1;
                    }
                    if !ev.from_cache {
                        uncached_since_persist += 1;
                    }

                    let elapsed_ms = started_at.elapsed().as_millis() as u64;
                    let cps = compute_chapters_per_sec(processed_count, elapsed_ms);
                    self.update_job(job_id, |j| {
                        j.processed_chapters = processed_count;
                        j.failed_chapters = failed_count;
                        j.current_chapter_title = Some(ev.chapter_title);
                        j.elapsed_ms = elapsed_ms;
                        j.chapters_per_sec = cps;
                    })
                    .await;

                    if uncached_since_persist >= DOWNLOAD_STATE_PERSIST_BATCH {
                        uncached_since_persist = 0;
                        let snap = reusable_ctx.snapshot_state();
                        if snap != last_persisted_state
                            && self
                                .sources
                                .persist_runtime_state_with_base(
                                    input.source_id,
                                    &last_persisted_state,
                                    snap.clone(),
                                    false,
                                )
                                .await
                                .is_ok()
                        {
                            last_persisted_state = snap;
                        }
                    }
                }
            }
        }

        for h in worker_handles {
            let _ = h.await;
        }

        let final_snap = reusable_ctx.snapshot_state();
        if final_snap != last_persisted_state
            && let Err(err) = self
                .sources
                .persist_runtime_state_with_base(
                    input.source_id,
                    &last_persisted_state,
                    final_snap,
                    false,
                )
                .await
        {
            tracing::warn!(source_id = %input.source_id, error = %err, "failed to persist final download runtime state");
        }

        if was_cancelled || cancel_flag.load(Ordering::Acquire) {
            if tracked_chapters_written > 0
                && let Some(bid) = target_tracked_book_id
            {
                let state_lock = self.book_state_lock_for(bid);
                let _state_guard = state_lock.lock().await;
                if let Ok(mut latest_book) = self.catalog.find_book_raw(bid).await
                    && matches!(
                        &latest_book.source,
                        BookSource::OnlineTracked { source_id, .. } if *source_id == input.source_id
                    )
                {
                    latest_book.mark_tracked_epub_dirty();
                    let _ = self.catalog.save_book_raw(&latest_book).await;
                }
            }
            self.mark_job_cancelled(job_id, started_at).await;
            let job_dir = self.download_dir.join(job_id.to_string());
            let _ = tokio::fs::remove_dir_all(&job_dir).await;
            return;
        }

        let packed_chapters: Vec<PackChapterInput> = ordered_chapters
            .into_iter()
            .enumerate()
            .map(|(idx, opt)| {
                opt.unwrap_or_else(|| PackChapterInput {
                    title: selected_arc[idx].title.clone(),
                    content: "[本章抓取失败，请稍后重试]".into(),
                })
            })
            .collect();

        let ext = match input.format {
            OnlineDownloadFormat::Epub => "epub",
            OnlineDownloadFormat::Txt => "txt",
        };
        let safe_stem = sanitize_file_stem(&input.book_name);
        let file_name = format!("{safe_stem}.{ext}");

        let mut imported_book_id = None;
        let mut output_path = None;
        let file_size: u64;

        if input.import_to_library
            && input.format == OnlineDownloadFormat::Epub
            && let Some(bid) = target_tracked_book_id
        {
            self.update_job(job_id, |j| {
                j.current_chapter_title = Some("正在同步标准 EPUB 3.0 文件…".into());
                j.elapsed_ms = started_at.elapsed().as_millis() as u64;
                j.chapters_per_sec = compute_chapters_per_sec(j.processed_chapters, j.elapsed_ms);
            })
            .await;

            match self.sync_tracked_epub(bid, true).await {
                Ok(summary) => {
                    if let Err(e) = self.catalog.bind_book_for_user(actor.id, summary.id).await {
                        self.fail_job(job_id, started_at, format!("绑定书籍到个人库失败: {e}"))
                            .await;
                        return;
                    }
                    imported_book_id = Some(summary.id.0);
                    file_size = summary.file_size;
                }
                Err(e) => {
                    self.fail_job(job_id, started_at, format!("同步 EPUB 3.0 失败: {e}"))
                        .await;
                    return;
                }
            }
        } else {
            let cover_bytes = if input.format == OnlineDownloadFormat::Epub
                && let Some(ref cover_url) = input.cover_url
                && !cover_url.trim().is_empty()
            {
                self.fetch_cover_proxy(cover_url).await.ok()
            } else {
                None
            };

            if cancel_flag.load(Ordering::Acquire) {
                self.mark_job_cancelled(job_id, started_at).await;
                return;
            }

            self.update_job(job_id, |j| {
                j.current_chapter_title = Some("正在封装书籍文件…".into());
                j.elapsed_ms = started_at.elapsed().as_millis() as u64;
                j.chapters_per_sec = compute_chapters_per_sec(j.processed_chapters, j.elapsed_ms);
            })
            .await;

            let pack_meta = PackBookMetadata {
                identifier: None,
                title: input.book_name.clone(),
                author: input.author.clone(),
                description: input.intro.clone(),
                cover_bytes,
            };
            let fmt = input.format;
            let packager = Arc::clone(&self.packager);
            let packaged = tokio::task::spawn_blocking(move || match fmt {
                OnlineDownloadFormat::Epub => packager.package_epub(&pack_meta, &packed_chapters),
                OnlineDownloadFormat::Txt => Ok(packager.package_txt(&pack_meta, &packed_chapters)),
            })
            .await
            .map_err(|e| format!("封装线程失败: {e}"))
            .and_then(|r| r);

            let bytes = match packaged {
                Ok(b) => b,
                Err(e) => {
                    self.fail_job(job_id, started_at, e).await;
                    return;
                }
            };

            if cancel_flag.load(Ordering::Acquire) {
                self.mark_job_cancelled(job_id, started_at).await;
                return;
            }

            file_size = bytes.len() as u64;

            if input.import_to_library {
                self.update_job(job_id, |j| {
                    j.current_chapter_title = Some("正在导入本地图书库…".into());
                    j.elapsed_ms = started_at.elapsed().as_millis() as u64;
                })
                .await;
                match self
                    .catalog
                    .upload_book(&actor, &file_name, Bytes::from(bytes))
                    .await
                {
                    Ok(summary) => {
                        imported_book_id = Some(summary.id.0);
                    }
                    Err(e) => {
                        self.fail_job(job_id, started_at, format!("封装成功但导入图书库失败: {e}"))
                            .await;
                        return;
                    }
                }
            } else {
                let job_dir = self.download_dir.join(job_id.to_string());
                if let Err(e) = tokio::fs::create_dir_all(&job_dir).await {
                    self.fail_job(job_id, started_at, format!("创建输出目录失败: {e}"))
                        .await;
                    return;
                }
                let out_path = job_dir.join(&file_name);
                if let Err(e) = tokio::fs::write(&out_path, &bytes).await {
                    self.fail_job(job_id, started_at, format!("写入输出文件失败: {e}"))
                        .await;
                    return;
                }
                output_path = Some(out_path);
            }
        }

        let final_elapsed = started_at.elapsed().as_millis() as u64;
        let mut guard = self.downloads.write().await;
        if let Some(slot) = guard.get_mut(&job_id) {
            if cancel_flag.load(Ordering::Acquire)
                || slot.dto.status == DownloadJobStatus::Cancelled
            {
                slot.dto.status = DownloadJobStatus::Cancelled;
                slot.dto.current_chapter_title = None;
                slot.dto.elapsed_ms = final_elapsed;
                slot.dto.chapters_per_sec =
                    compute_chapters_per_sec(slot.dto.processed_chapters, final_elapsed);
                slot.dto.updated_at = Timestamp::now();
                drop(guard);
                let job_dir = self.download_dir.join(job_id.to_string());
                let _ = tokio::fs::remove_dir_all(&job_dir).await;
                return;
            }
            slot.output_path = output_path;
            slot.dto.status = DownloadJobStatus::Completed;
            slot.dto.output_file_name = Some(file_name);
            slot.dto.file_size = Some(file_size);
            slot.dto.imported_book_id = imported_book_id;
            slot.dto.current_chapter_title = None;
            slot.dto.elapsed_ms = final_elapsed;
            slot.dto.chapters_per_sec =
                compute_chapters_per_sec(slot.dto.processed_chapters, final_elapsed);
            slot.dto.updated_at = Timestamp::now();
        } else {
            drop(guard);
            let job_dir = self.download_dir.join(job_id.to_string());
            let _ = tokio::fs::remove_dir_all(&job_dir).await;
        }
    }

    async fn update_job(&self, job_id: Uuid, f: impl FnOnce(&mut OnlineDownloadJobDto)) {
        let mut guard = self.downloads.write().await;
        if let Some(slot) = guard.get_mut(&job_id)
            && slot.dto.status != DownloadJobStatus::Cancelled
        {
            f(&mut slot.dto);
            slot.dto.updated_at = Timestamp::now();
        }
    }

    async fn fail_job(&self, job_id: Uuid, started_at: Instant, msg: String) {
        let elapsed = started_at.elapsed().as_millis() as u64;
        self.update_job(job_id, |j| {
            j.status = DownloadJobStatus::Failed;
            j.error = Some(msg);
            j.current_chapter_title = None;
            j.elapsed_ms = elapsed;
            j.chapters_per_sec = compute_chapters_per_sec(j.processed_chapters, elapsed);
        })
        .await;
    }

    async fn mark_job_cancelled(&self, job_id: Uuid, started_at: Instant) {
        let elapsed = started_at.elapsed().as_millis() as u64;
        let mut guard = self.downloads.write().await;
        if let Some(slot) = guard.get_mut(&job_id) {
            slot.dto.status = DownloadJobStatus::Cancelled;
            slot.dto.current_chapter_title = None;
            slot.dto.error = None;
            slot.dto.elapsed_ms = elapsed;
            slot.dto.chapters_per_sec =
                compute_chapters_per_sec(slot.dto.processed_chapters, elapsed);
            slot.dto.updated_at = Timestamp::now();
        }
    }
}

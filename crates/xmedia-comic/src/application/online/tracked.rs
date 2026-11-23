use std::collections::HashSet;
use std::io::{Cursor, Write};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;
use uuid::Uuid;
use xmedia_core::application::ports::SystemEvent;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::{Library, MediaKind};
use xmedia_core::domain::user::User;
use xmedia_core::domain::xml::xml_escape;
use xmedia_source::http::{HttpClientConfig, resolve_literal_http_url};
use xmedia_source::proxy::{
    DEFAULT_IMAGE_PROXY_TIMEOUT_SECS, fetch_validated_image_bytes_with_config,
};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::application::dto::{ChapterItemDto, SeriesDetailDto};
use crate::domain::chapter::{Chapter, ChapterKind, ChapterSourceKind, OnlineTrackedChapterRef};
use crate::domain::id::{ChapterId, SeriesId};
use crate::domain::series::{OnlineTrackedSeriesRef, ReadingDirection, Series, SeriesStatus};
use crate::source::model::{
    ComicChapterJsContext, ComicJsContext, ComicSourceSpec, OnlineComicChapter,
};

use super::helpers::{CancelOnDrop, get_or_insert_weak_lock};
use super::{
    COMIC_IMAGE_PROXY_MAX_BYTES, ComicOnlineService, READER_OP_TIMEOUT, SEARCH_PER_SOURCE_TIMEOUT,
};

const MAX_TRACKED_PAGES_PER_CHAPTER: usize = 500;
const PAGE_DOWNLOAD_CONCURRENCY: usize = 4;

#[derive(Debug, Clone, Deserialize)]
pub struct TrackOnlineComicInput {
    pub source_id: Uuid,
    #[serde(alias = "book_url")]
    pub comic_url: String,
    #[serde(default)]
    pub toc_url: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub cover_url: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub reading_direction: Option<String>,
    #[serde(default)]
    pub genres: Option<Vec<String>>,
    #[serde(default)]
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ComicCacheReportDto {
    pub series_id: SeriesId,
    pub total_requested: u32,
    pub cached_now: u32,
    pub already_cached: u32,
    pub failed: Vec<String>,
}

impl ComicOnlineService {
    async fn ensure_managed_comic_library(&self) -> DomainResult<Library> {
        if let Some(existing) = self
            .libraries
            .find_managed_by_media(MediaKind::Comic)
            .await?
        {
            return Ok(existing);
        }
        let _ = self.storage.ensure_managed_root().await?;
        let lib = Library::new_managed(MediaKind::Comic, MediaKind::Comic.default_managed_name())?;
        self.libraries.save(&lib).await?;
        Ok(lib)
    }

    async fn fetch_pages_internal(
        &self,
        source_id: Uuid,
        chapter_url: &str,
        comic_ctx: ComicJsContext,
        chapter_ctx: ComicChapterJsContext,
    ) -> DomainResult<(Vec<String>, ComicSourceSpec)> {
        let rec = self
            .sources
            .find_by_id(source_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("漫画源不存在".into()))?;
        if !rec.enabled {
            return Err(DomainError::Validation(
                "该追更章节所属漫画源已被停用，无法按需拉取".into(),
            ));
        }
        let parsed: ComicSourceSpec = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("漫画源 JSON 无效: {e}")))?;
        let clean_chapter_url = resolve_literal_http_url(chapter_url, Some(&parsed.source_url))?;
        self.acquire_rate_slot(
            source_id,
            parsed.parse_concurrent_rate(),
            SEARCH_PER_SOURCE_TIMEOUT,
        )
        .await
        .map_err(DomainError::Validation)?;

        let engine = self.engine.clone();
        let parsed_for_run = parsed.clone();
        let prev_state = rec.runtime_state.clone();
        let prev_state_cmp = prev_state.clone();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_worker = cancel_flag.clone();
        let mut cancel_guard = CancelOnDrop::new(cancel_flag.clone());

        let timed = tokio::time::timeout(
            READER_OP_TIMEOUT,
            tokio::task::spawn_blocking(move || {
                engine
                    .fetch_pages_sync_with_control(
                        &parsed_for_run,
                        prev_state,
                        &clean_chapter_url,
                        &comic_ctx,
                        &chapter_ctx,
                        READER_OP_TIMEOUT,
                        cancel_worker,
                    )
                    .map_err(String::from)
            }),
        )
        .await;

        let out = match timed {
            Ok(Ok(Ok(res))) => {
                cancel_guard.disarm();
                res
            }
            Ok(Ok(Err(err))) => {
                cancel_guard.disarm();
                return Err(DomainError::Validation(err));
            }
            Ok(Err(e)) => {
                cancel_guard.disarm();
                return Err(DomainError::Storage(format!("获取漫画页面列表失败: {e}")));
            }
            Err(_) => {
                cancel_flag.store(true, Ordering::Relaxed);
                return Err(DomainError::Validation(format!(
                    "获取漫画页面列表超时（超过 {} 秒）",
                    READER_OP_TIMEOUT.as_secs()
                )));
            }
        };

        if out.state != prev_state_cmp
            && let Err(err) = self
                .persist_runtime_state_with_base(source_id, &prev_state_cmp, out.state, false)
                .await
        {
            tracing::warn!(
                source_id = %source_id,
                error = %err,
                "failed to persist runtime state after tracked fetch_pages"
            );
        }

        Ok((out.data, parsed))
    }

    async fn fetch_image_with_spec(
        &self,
        parsed: &ComicSourceSpec,
        raw_url: &str,
    ) -> DomainResult<(Vec<u8>, String)> {
        let mut headers = parsed.merged_image_headers();
        headers.retain(|k, _| !k.eq_ignore_ascii_case("range"));
        let cfg = HttpClientConfig {
            timeout_secs: DEFAULT_IMAGE_PROXY_TIMEOUT_SECS,
            allow_private_ips: self.allow_private_ips,
            proxy_url: None,
        };
        let (bytes, mime) = fetch_validated_image_bytes_with_config(
            raw_url,
            Some(&headers),
            COMIC_IMAGE_PROXY_MAX_BYTES,
            &cfg,
        )
        .await?;
        Ok((bytes, mime.to_string()))
    }

    /// Tracks an online comic into the unified managed comic library (hybrid mode):
    /// creates the series + TOC chapter entries immediately without downloading all pages;
    /// pages are fetched & packaged as CBZ transparently on first read / OPDS access or
    /// via explicit offline caching.
    pub async fn track_online_comic(
        &self,
        actor: &User,
        input: TrackOnlineComicInput,
    ) -> DomainResult<SeriesDetailDto> {
        let rec = self.load_source_for_actor(actor, input.source_id).await?;
        let parsed: ComicSourceSpec = serde_json::from_str(&rec.raw_json)
            .map_err(|e| DomainError::Validation(format!("漫画源 JSON 无效: {e}")))?;

        let clean_comic_url = resolve_literal_http_url(&input.comic_url, Some(&parsed.source_url))?;
        let clean_input_toc = input
            .toc_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|u| resolve_literal_http_url(u, Some(&parsed.source_url)))
            .transpose()?;
        let clean_input_cover = input
            .cover_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|u| resolve_literal_http_url(u, Some(&parsed.source_url)))
            .transpose()?;

        let lock = get_or_insert_weak_lock(
            &self.track_locks,
            (input.source_id, clean_comic_url.clone()),
        );
        let _guard = lock.lock().await;

        let managed_lib = self.ensure_managed_comic_library().await?;
        let existing_series = self.series.list_by_library(managed_lib.id).await?;
        if let Some(found) = existing_series.into_iter().find(|s| {
            s.online_tracking()
                .is_some_and(|t| t.source_id == input.source_id && t.comic_url == clean_comic_url)
        }) {
            self.catalog
                .bind_series_for_user(actor.id, found.id)
                .await?;
            return self.catalog.get_series_detail(actor, found.id).await;
        }

        let fallback_hint = ComicJsContext {
            name: input.title.clone().unwrap_or_default(),
            author: input.author.clone().unwrap_or_default(),
            book_url: clean_comic_url.clone(),
            toc_url: clean_input_toc.clone().unwrap_or_default(),
            cover_url: clean_input_cover.clone(),
            intro: input.summary.clone(),
            reading_direction: input.reading_direction.clone(),
            ..Default::default()
        };

        let detail_res = self
            .fetch_detail(
                actor,
                input.source_id,
                &clean_comic_url,
                Some(fallback_hint),
            )
            .await;

        let (title, author, summary, cover_url, toc_url, status_str, direction_str, mut toc_items) =
            match detail_res {
                Ok(resp) => {
                    let d = resp.detail;
                    let resolved_title = if !d.title.trim().is_empty() {
                        d.title
                    } else {
                        input
                            .title
                            .clone()
                            .unwrap_or_else(|| "Untitled Comic".into())
                    };
                    let resolved_toc = if !d.toc_url.trim().is_empty() {
                        d.toc_url
                    } else {
                        clean_input_toc
                            .clone()
                            .unwrap_or_else(|| clean_comic_url.clone())
                    };
                    (
                        resolved_title,
                        if !d.author.trim().is_empty() {
                            Some(d.author)
                        } else {
                            input.author.clone()
                        },
                        d.intro.or_else(|| input.summary.clone()),
                        d.cover_url.or(clean_input_cover),
                        resolved_toc,
                        d.status.or_else(|| input.status.clone()),
                        Some(d.reading_direction.as_str().to_string()),
                        resp.chapters,
                    )
                }
                Err(_) => (
                    input
                        .title
                        .clone()
                        .filter(|s| !s.trim().is_empty())
                        .ok_or_else(|| {
                            DomainError::Validation("无法获取漫画详情且未提供标题".into())
                        })?,
                    input.author.clone(),
                    input.summary.clone(),
                    clean_input_cover,
                    clean_input_toc.unwrap_or_else(|| clean_comic_url.clone()),
                    input.status.clone(),
                    input.reading_direction.clone(),
                    Vec::new(),
                ),
            };

        let clean_toc_url = resolve_literal_http_url(&toc_url, Some(&parsed.source_url))
            .unwrap_or_else(|_| clean_comic_url.clone());

        if toc_items.is_empty() {
            let comic_ctx = ComicJsContext {
                name: title.clone(),
                author: author.clone().unwrap_or_default(),
                book_url: clean_comic_url.clone(),
                toc_url: clean_toc_url.clone(),
                cover_url: cover_url.clone(),
                intro: summary.clone(),
                reading_direction: direction_str.clone(),
                ..Default::default()
            };
            if let Ok(ch_resp) = self
                .fetch_chapters(actor, input.source_id, &clean_toc_url, comic_ctx)
                .await
            {
                toc_items = ch_resp.chapters;
            }
        }

        let series_ref = OnlineTrackedSeriesRef {
            source_id: input.source_id,
            comic_url: clean_comic_url.clone(),
            toc_url: clean_toc_url.clone(),
        };
        let encoded_dir = Series::encode_online_tracking(&series_ref)?;
        let mut series = Series::new(managed_lib.id, title.clone(), encoded_dir)?;
        if let Some(ref au) = author {
            let writers: Vec<String> = au
                .split(['/', ',', '、', ';'])
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(ToOwned::to_owned)
                .collect();
            if !writers.is_empty() {
                series.writers = writers;
            }
        }
        if let Some(ref intro) = summary {
            series.summary = intro.trim().to_owned();
        }
        if let Some(ref genres) = input.genres {
            series.genres = genres
                .iter()
                .map(|g| g.trim().to_owned())
                .filter(|g| !g.is_empty())
                .collect();
        }
        if let Some(ref st) = status_str {
            series.status = parse_series_status_hint(st);
        }
        if let Some(ref dir) = direction_str {
            series.reading_direction = ReadingDirection::parse(dir);
        }

        // Best-effort cover download & thumbnail storage
        if let Some(ref cov_url) = cover_url
            && let Ok((cov_bytes, _)) = self.fetch_image_with_spec(&parsed, cov_url).await
        {
            let thumb_key = format!("series_{}", series.id);
            if let Ok(saved_key) = self
                .thumbnails
                .generate_from_bytes(&thumb_key, &cov_bytes)
                .await
            {
                series.cover_path = Some(saved_key);
            }
        }

        let created_count = self
            .append_online_chapters(&series, &series_ref, &toc_items, &HashSet::new(), 0)
            .await?;
        series.chapter_count = created_count;
        series.updated_at = Timestamp::now();
        self.series.save(&series).await?;
        self.catalog
            .bind_series_for_user(actor.id, series.id)
            .await?;

        self.events.publish(SystemEvent::MediaChanged {
            media_kind: MediaKind::Comic,
        });

        self.catalog.get_series_detail(actor, series.id).await
    }

    /// Re-fetches the online TOC for a tracked comic series and appends any new chapters.
    pub async fn refresh_tracked_series(
        &self,
        actor: &User,
        series_id: SeriesId,
    ) -> DomainResult<SeriesDetailDto> {
        self.catalog.require_series_bound(actor, series_id).await?;
        let mut series = self
            .series
            .find_by_id(series_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("series not found".into()))?;
        let Some(series_ref) = series.online_tracking() else {
            return Err(DomainError::Validation("该漫画系列不是在线追更系列".into()));
        };

        let existing = self.chapters.list_by_series(series_id).await?;
        let existing_urls: HashSet<String> = existing
            .iter()
            .filter_map(|c| c.online_tracking().map(|t| t.chapter_url))
            .collect();
        let start_number = existing.len() as u32;

        let comic_ctx = ComicJsContext {
            name: series.title.clone(),
            author: series.writers.join(", "),
            book_url: series_ref.comic_url.clone(),
            toc_url: series_ref.toc_url.clone(),
            cover_url: None,
            intro: Some(series.summary.clone()),
            reading_direction: Some(series.reading_direction.as_str().to_string()),
            ..Default::default()
        };

        let toc_target = if !series_ref.toc_url.trim().is_empty() {
            series_ref.toc_url.clone()
        } else {
            series_ref.comic_url.clone()
        };
        let toc_resp = self
            .fetch_chapters(actor, series_ref.source_id, &toc_target, comic_ctx)
            .await?;

        let added = self
            .append_online_chapters(
                &series,
                &series_ref,
                &toc_resp.chapters,
                &existing_urls,
                start_number,
            )
            .await?;

        if added > 0 {
            let total = self
                .chapters
                .list_by_series(series_id)
                .await?
                .into_iter()
                .filter(|c| !c.is_missing)
                .count() as u32;
            series.chapter_count = total;
            series.updated_at = Timestamp::now();
            self.series.save(&series).await?;
            self.events.publish(SystemEvent::MediaChanged {
                media_kind: MediaKind::Comic,
            });
        }

        self.catalog.get_series_detail(actor, series_id).await
    }

    async fn append_online_chapters(
        &self,
        series: &Series,
        series_ref: &OnlineTrackedSeriesRef,
        toc_items: &[OnlineComicChapter],
        existing_urls: &HashSet<String>,
        start_number: u32,
    ) -> DomainResult<u32> {
        let mut added = 0_u32;
        let mut seen = existing_urls.clone();
        for item in toc_items {
            if item.is_volume {
                continue;
            }
            let url = item.url.trim();
            if url.is_empty() || !seen.insert(url.to_string()) {
                continue;
            }
            let num = f64::from(start_number + added + 1);
            let ch_ref = OnlineTrackedChapterRef {
                source_id: series_ref.source_id,
                comic_url: series_ref.comic_url.clone(),
                toc_url: series_ref.toc_url.clone(),
                chapter_url: url.to_string(),
                storage_key: None,
                cached: false,
            };
            let source_path = serde_json::to_string(&ch_ref).map_err(|e| {
                DomainError::Validation(format!("serialize tracked chapter ref failed: {e}"))
            })?;
            let chapter = Chapter::new(
                series.id,
                series.library_id,
                ChapterKind::Chapter,
                num,
                None,
                item.title.clone(),
                0,
                ChapterSourceKind::OnlineTracked,
                source_path,
                0,
                String::new(),
            )?;
            self.chapters.save(&chapter).await?;
            added += 1;
        }
        Ok(added)
    }

    /// Ensures an `OnlineTracked` comic chapter has its pages downloaded and packaged as a local
    /// CBZ archive in managed storage. No-op for local chapters or already-cached tracked chapters.
    /// Safe to invoke concurrently from Web readers and OPDS / PSE clients.
    pub async fn ensure_tracked_chapter_cached(
        &self,
        chapter_id: ChapterId,
    ) -> DomainResult<Chapter> {
        let initial = self
            .chapters
            .find_by_id(chapter_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("chapter not found".into()))?;
        if !initial.is_online_tracked() {
            return Ok(initial);
        }
        if let Some(info) = initial.online_tracking()
            && info.cached
            && let Some(ref key) = info.storage_key
            && Path::new(key).is_file()
        {
            return Ok(initial);
        }

        let lock = get_or_insert_weak_lock(&self.chapter_cache_locks, chapter_id);
        let _guard = lock.lock().await;

        // Re-check after acquiring per-chapter lock
        let mut chapter = self
            .chapters
            .find_by_id(chapter_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("chapter not found".into()))?;
        if !chapter.is_online_tracked() {
            return Ok(chapter);
        }
        let mut info = chapter
            .online_tracking()
            .ok_or_else(|| DomainError::Validation("追更章节元数据无效，无法拉取页面".into()))?;
        if info.cached
            && let Some(ref key) = info.storage_key
            && Path::new(key).is_file()
        {
            return Ok(chapter);
        }

        let mut series = self
            .series
            .find_by_id(chapter.series_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("parent series not found".into()))?;

        let comic_ctx = ComicJsContext {
            name: series.title.clone(),
            author: series.writers.join(", "),
            book_url: info.comic_url.clone(),
            toc_url: info.toc_url.clone(),
            cover_url: None,
            intro: Some(series.summary.clone()),
            reading_direction: Some(series.reading_direction.as_str().to_string()),
            ..Default::default()
        };
        let chapter_ctx = ComicChapterJsContext {
            title: chapter.title.clone(),
            url: info.chapter_url.clone(),
            index: chapter.number.max(1.0) as usize - 1,
        };

        let (page_urls, parsed_spec) = self
            .fetch_pages_internal(info.source_id, &info.chapter_url, comic_ctx, chapter_ctx)
            .await?;

        if page_urls.is_empty() {
            return Err(DomainError::Validation(
                "在线漫画源未返回任何页面图片".into(),
            ));
        }
        let limited_urls: Vec<String> = page_urls
            .into_iter()
            .take(MAX_TRACKED_PAGES_PER_CHAPTER)
            .collect();

        // Download all page images with bounded concurrency
        let sem = Arc::new(Semaphore::new(PAGE_DOWNLOAD_CONCURRENCY));
        let mut tasks = Vec::with_capacity(limited_urls.len());
        let mut headers = parsed_spec.merged_image_headers();
        headers.retain(|k, _| !k.eq_ignore_ascii_case("range"));
        let headers = Arc::new(headers);
        let allow_private_ips = self.allow_private_ips;

        for (idx, img_url) in limited_urls.into_iter().enumerate() {
            let sem_c = Arc::clone(&sem);
            let headers_c = Arc::clone(&headers);
            tasks.push(tokio::spawn(async move {
                let Ok(_permit) = sem_c.acquire_owned().await else {
                    return Err(DomainError::Storage(
                        "page download scheduler closed".into(),
                    ));
                };
                let cfg = HttpClientConfig {
                    timeout_secs: DEFAULT_IMAGE_PROXY_TIMEOUT_SECS,
                    allow_private_ips,
                    proxy_url: None,
                };
                let (bytes, mime) = fetch_validated_image_bytes_with_config(
                    &img_url,
                    Some(&headers_c),
                    COMIC_IMAGE_PROXY_MAX_BYTES,
                    &cfg,
                )
                .await?;
                Ok((idx, bytes, mime.to_string()))
            }));
        }

        let mut downloaded: Vec<(usize, Vec<u8>, String)> = Vec::with_capacity(tasks.len());
        for handle in tasks {
            let res = handle.await.map_err(|e| {
                DomainError::Storage(format!("chapter page download join error: {e}"))
            })??;
            downloaded.push(res);
        }
        downloaded.sort_by_key(|(idx, _, _)| *idx);

        let page_count = downloaded.len() as u32;
        let first_page_bytes = downloaded.first().map(|(_, b, _)| b.clone());
        let series_title_for_xml = series.title.clone();
        let chapter_title_for_xml = chapter.title.clone();
        let chapter_num = chapter.number;

        let cbz_bytes = tokio::task::spawn_blocking(move || {
            build_comic_cbz(
                &series_title_for_xml,
                &chapter_title_for_xml,
                chapter_num,
                &downloaded,
            )
        })
        .await
        .map_err(|e| DomainError::Storage(format!("CBZ packaging join failed: {e}")))??;

        // Clean up stale managed file if re-caching
        if let Some(old_key) = info.storage_key.as_deref() {
            let _ = self.storage.delete_managed_file(old_key).await;
        }

        let series_folder = format!("tracked_{}", series.id);
        let filename = format!("ch_{}.cbz", chapter.id);
        let stored = self
            .storage
            .store_uploaded_archive(&series_folder, &filename, &cbz_bytes)
            .await?;

        // Generate chapter thumbnail (and fallback series cover) from page 1
        if let Some(ref first_bytes) = first_page_bytes {
            let ch_thumb_key = format!("chapter_{}", chapter.id);
            if let Ok(saved_key) = self
                .thumbnails
                .generate_from_bytes(&ch_thumb_key, first_bytes)
                .await
            {
                chapter.cover_path = Some(saved_key.clone());
                if series.cover_path.is_none() {
                    series.cover_path = Some(saved_key);
                }
            }
        }

        info.storage_key = Some(stored.source_path);
        info.cached = true;
        chapter.set_online_tracking(&info)?;
        chapter.page_count = page_count;
        chapter.file_size = stored.file_size;
        chapter.file_hash = stored.file_hash;
        chapter.is_missing = false;
        chapter.updated_at = Timestamp::now();
        self.chapters.save(&chapter).await?;

        series.updated_at = Timestamp::now();
        self.series.save(&series).await?;

        Ok(chapter)
    }

    pub async fn cache_tracked_chapter(
        &self,
        actor: &User,
        chapter_id: ChapterId,
    ) -> DomainResult<ChapterItemDto> {
        self.catalog
            .require_chapter_bound(actor, chapter_id)
            .await?;
        let ch = self.ensure_tracked_chapter_cached(chapter_id).await?;
        let (cur_page, done) = self
            .reading
            .get_chapter_progress(actor.id, ch.id)
            .await?
            .map(|p| (p.current_page, p.is_completed))
            .unwrap_or((0, false));
        Ok(ChapterItemDto::from_domain(ch, cur_page, done))
    }

    pub async fn cache_tracked_series(
        &self,
        actor: &User,
        series_id: SeriesId,
        chapter_ids: Option<Vec<ChapterId>>,
    ) -> DomainResult<ComicCacheReportDto> {
        self.catalog.require_series_bound(actor, series_id).await?;
        let series = self
            .series
            .find_by_id(series_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("series not found".into()))?;
        if series.online_tracking().is_none() {
            return Err(DomainError::Validation("该漫画系列不是在线追更系列".into()));
        }

        let mut chapters = self.chapters.list_by_series(series_id).await?;
        chapters.sort_by(|a, b| a.cmp_order(b));
        if let Some(filter_ids) = chapter_ids.filter(|v| !v.is_empty()) {
            let allowed: HashSet<ChapterId> = filter_ids.into_iter().collect();
            chapters.retain(|c| allowed.contains(&c.id));
        }

        let total_requested = chapters.len() as u32;
        let mut cached_now = 0_u32;
        let mut already_cached = 0_u32;
        let mut failed = Vec::new();

        for ch in chapters {
            if !ch.is_online_tracked() {
                continue;
            }
            if let Some(info) = ch.online_tracking()
                && info.cached
                && info
                    .storage_key
                    .as_deref()
                    .is_some_and(|p| Path::new(p).is_file())
            {
                already_cached += 1;
                continue;
            }
            match self.ensure_tracked_chapter_cached(ch.id).await {
                Ok(_) => cached_now += 1,
                Err(err) => failed.push(format!("{}: {err}", ch.title)),
            }
        }

        if cached_now > 0 {
            self.events.publish(SystemEvent::MediaChanged {
                media_kind: MediaKind::Comic,
            });
        }

        Ok(ComicCacheReportDto {
            series_id,
            total_requested,
            cached_now,
            already_cached,
            failed,
        })
    }
}

fn parse_series_status_hint(raw: &str) -> SeriesStatus {
    let lower = raw.trim().to_ascii_lowercase();
    if lower.contains("完结")
        || lower.contains("已完")
        || lower.contains("complete")
        || lower.contains("ended")
    {
        SeriesStatus::Completed
    } else if lower.contains("休刊") || lower.contains("暂停") || lower.contains("hiatus") {
        SeriesStatus::Hiatus
    } else {
        SeriesStatus::Ongoing
    }
}

fn ext_for_image_mime(mime: &str) -> &'static str {
    match mime.to_ascii_lowercase().as_str() {
        "image/png" => "png",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "image/avif" => "avif",
        _ => "jpg",
    }
}

fn build_comic_cbz(
    series_title: &str,
    chapter_title: &str,
    chapter_number: f64,
    pages: &[(usize, Vec<u8>, String)],
) -> DomainResult<Vec<u8>> {
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut zip = ZipWriter::new(&mut cursor);
        let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);

        let info_xml = format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<ComicInfo>\n  <Series>{}</Series>\n  <Title>{}</Title>\n  <Number>{}</Number>\n  <PageCount>{}</PageCount>\n</ComicInfo>\n",
            xml_escape(series_title),
            xml_escape(chapter_title),
            chapter_number,
            pages.len(),
        );
        zip.start_file("ComicInfo.xml", opts)
            .map_err(|e| DomainError::Storage(format!("CBZ write ComicInfo.xml failed: {e}")))?;
        zip.write_all(info_xml.as_bytes())
            .map_err(|e| DomainError::Storage(format!("CBZ write ComicInfo.xml failed: {e}")))?;

        for (idx, bytes, mime) in pages {
            let ext = ext_for_image_mime(mime);
            let name = format!("{:04}.{ext}", idx + 1);
            zip.start_file(&name, opts).map_err(|e| {
                DomainError::Storage(format!("CBZ start page '{name}' failed: {e}"))
            })?;
            zip.write_all(bytes).map_err(|e| {
                DomainError::Storage(format!("CBZ write page '{name}' failed: {e}"))
            })?;
        }

        zip.finish()
            .map_err(|e| DomainError::Storage(format!("CBZ finalize failed: {e}")))?;
    }
    Ok(cursor.into_inner())
}

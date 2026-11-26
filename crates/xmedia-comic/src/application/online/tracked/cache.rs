use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use jiff::Timestamp;
use serde::Serialize;
use uuid::Uuid;
use xmedia_core::application::ports::SystemEvent;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;
use xmedia_core::domain::user::User;
use xmedia_formats::comic::comicinfo::{ComicInfoMeta, render_comic_info_xml_preserving};
use xmedia_source::http::{HttpClientConfig, resolve_literal_http_url};
use xmedia_source::proxy::{DEFAULT_IMAGE_PROXY_TIMEOUT_SECS, fetch_image};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::application::dto::ChapterItemDto;
use crate::application::online::helpers::{CancelOnDrop, weak_lock};
use crate::application::online::{
    COMIC_IMAGE_PROXY_MAX_BYTES, ComicOnlineService, READER_OP_TIMEOUT, SEARCH_PER_SOURCE_TIMEOUT,
};
use crate::domain::chapter::Chapter;
use crate::domain::id::{ChapterId, SeriesId};
use crate::domain::online::{
    ComicChapterJsContext, ComicJsContext, ComicSourceSpec, strip_cross_origin_headers,
};
use crate::domain::series::CoverUpdate;

const MAX_TRACKED_PAGES_PER_CHAPTER: usize = 500;
const PAGE_DOWNLOAD_CONCURRENCY: usize = 4;

#[derive(Debug, Clone, Serialize)]
pub struct ComicCacheReportDto {
    pub series_id: SeriesId,
    pub total_requested: u32,
    pub cached_now: u32,
    pub already_cached: u32,
    pub failed: Vec<String>,
}

impl ComicOnlineService {
    async fn fetch_pages_internal(
        &self,
        source_id: Uuid,
        chapter_url: &str,
        comic_ctx: ComicJsContext,
        chapter_ctx: ComicChapterJsContext,
    ) -> DomainResult<(Vec<String>, ComicSourceSpec)> {
        let rec = self.source_crud.load_source(source_id).await?;
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
                .source_crud
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

    /// Ensures an `OnlineTracked` comic chapter has its pages downloaded and packaged as a local
    /// CBZ archive in managed storage. No-op for local chapters or already-cached tracked chapters.
    /// Safe to invoke concurrently from Web readers and OPDS / PSE clients.
    pub async fn ensure_tracked_chapter_cached(
        &self,
        chapter_id: ChapterId,
    ) -> DomainResult<Chapter> {
        let initial = self
            .chapters
            .query(crate::domain::repository::ChapterQuery::Id(chapter_id))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound("chapter not found".into()))?;
        if initial.source.online_tracking().is_none() {
            return Ok(initial);
        }
        if let Some(info) = initial.source.online_tracking()
            && let Some(ref key) = info.cached_archive_path
            && Path::new(key).is_file()
        {
            return Ok(initial);
        }

        let lock = weak_lock(&self.chapter_cache_locks, chapter_id);
        let _guard = lock.lock().await;

        let mut chapter = self
            .chapters
            .query(crate::domain::repository::ChapterQuery::Id(chapter_id))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound("chapter not found".into()))?;
        if chapter.source.online_tracking().is_none() {
            return Ok(chapter);
        }
        let info =
            chapter.source.online_tracking().cloned().ok_or_else(|| {
                DomainError::Validation("追更章节元数据无效，无法拉取页面".into())
            })?;
        if let Some(ref key) = info.cached_archive_path
            && Path::new(key).is_file()
        {
            return Ok(chapter);
        }

        let mut series = self
            .series
            .query(crate::domain::repository::SeriesQuery::Id(
                chapter.series_id,
            ))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound("parent series not found".into()))?;

        let comic_ctx = ComicJsContext {
            name: series.title.clone(),
            author: series.writers.join(", "),
            book_url: info.comic_url.clone(),
            toc_url: info.toc_url.clone(),
            cover_url: None,
            intro: Some(series.summary.clone()),
            reading_direction: Some(series.reading_direction.as_ref().to_string()),
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

        let total_pages = limited_urls.len();
        let page_count = total_pages as u32;
        let mut base_headers = parsed_spec.image_headers(None);
        base_headers.retain(|k, _| !k.eq_ignore_ascii_case("range"));
        let base_headers = Arc::new(base_headers);
        let source_url_arc = Arc::new(parsed_spec.source_url.clone());
        let allow_private_ips = self.allow_private_ips;

        let series_title_for_xml = series.title.clone();
        let chapter_title_for_xml = chapter.title.clone();
        let chapter_num = chapter.number;
        let (temp_cbz, mut zip_writer) = tokio::task::spawn_blocking(move || {
            let (guard, file) = TempCbzGuard::create_exclusive()?;
            let zip = init_disk_comic_cbz(
                file,
                &series_title_for_xml,
                &chapter_title_for_xml,
                chapter_num,
                total_pages,
            )?;
            Ok::<_, DomainError>((guard, zip))
        })
        .await
        .map_err(|e| DomainError::Storage(format!("CBZ temp init join failed: {e}")))??;

        let mut first_page_bytes: Option<Vec<u8>> = None;
        for (chunk_idx, url_batch) in limited_urls.chunks(PAGE_DOWNLOAD_CONCURRENCY).enumerate() {
            let base_idx = chunk_idx * PAGE_DOWNLOAD_CONCURRENCY;
            let mut tasks = Vec::with_capacity(url_batch.len());
            for (offset, img_url) in url_batch.iter().cloned().enumerate() {
                let idx = base_idx + offset;
                let headers_c = Arc::clone(&base_headers);
                let src_url_c = Arc::clone(&source_url_arc);
                tasks.push(tokio::spawn(async move {
                    let mut page_headers = (*headers_c).clone();
                    strip_cross_origin_headers(&mut page_headers, &src_url_c, &img_url);
                    let cfg = HttpClientConfig {
                        timeout_secs: DEFAULT_IMAGE_PROXY_TIMEOUT_SECS,
                        allow_private_ips,
                        proxy_url: None,
                    };
                    let (bytes, mime) = fetch_image(
                        &img_url,
                        Some(&page_headers),
                        COMIC_IMAGE_PROXY_MAX_BYTES,
                        &cfg,
                    )
                    .await?;
                    Ok::<_, DomainError>((idx, bytes, mime.to_string()))
                }));
            }

            let mut batch_pages: Vec<(usize, Vec<u8>, String)> = Vec::with_capacity(tasks.len());
            for handle in tasks {
                let res = handle.await.map_err(|e| {
                    DomainError::Storage(format!("chapter page download join error: {e}"))
                })??;
                batch_pages.push(res);
            }
            batch_pages.sort_by_key(|(idx, _, _)| *idx);
            if first_page_bytes.is_none()
                && let Some((_, bytes, _)) = batch_pages.first()
            {
                first_page_bytes = Some(bytes.clone());
            }

            let current_zip = zip_writer;
            zip_writer =
                tokio::task::spawn_blocking(move || append_pages_to_cbz(current_zip, &batch_pages))
                    .await
                    .map_err(|e| {
                        DomainError::Storage(format!("CBZ batch write join failed: {e}"))
                    })??;
        }

        tokio::task::spawn_blocking(move || finish_disk_comic_cbz(zip_writer))
            .await
            .map_err(|e| DomainError::Storage(format!("CBZ finalize join failed: {e}")))??;

        if let Some(old_key) = info.cached_archive_path.as_deref() {
            let _ = self.storage.delete_managed_file(old_key).await;
        }

        let series_folder = format!("tracked_{}", series.id);
        let filename = format!("ch_{}.cbz", chapter.id);
        let stored = self
            .storage
            .store_uploaded_archive_from_path(&series_folder, &filename, temp_cbz.path())
            .await?;

        let now = Timestamp::now();
        let mut cover_key = None;
        if let Some(ref first_bytes) = first_page_bytes {
            let ch_thumb_key = format!("chapter_{}", chapter.id);
            if let Ok(saved_key) = self
                .thumbnails
                .generate_from_bytes(&ch_thumb_key, first_bytes)
                .await
            {
                let _ = series.apply_cover(CoverUpdate::Fallback(saved_key.clone()), now);
                cover_key = Some(saved_key);
            }
        }

        chapter.cache_online_archive(
            stored.source_path,
            page_count,
            stored.file_size,
            stored.file_hash,
            cover_key,
            now,
        )?;
        self.chapters.save(&chapter).await?;

        series.updated_at = now;
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
            .query_progress(
                actor.id,
                crate::domain::repository::ComicProgressScope::Chapter(ch.id),
            )
            .await?
            .into_iter()
            .next()
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
            .query(crate::domain::repository::SeriesQuery::Id(series_id))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound("series not found".into()))?;
        if series.source.online_tracking().is_none() {
            return Err(DomainError::Validation("该漫画系列不是在线追更系列".into()));
        }

        let mut chapters = self
            .chapters
            .query(crate::domain::repository::ChapterQuery::Series(series_id))
            .await?;
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
            let Some(info) = ch.source.online_tracking() else {
                continue;
            };
            if info
                .cached_archive_path
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

fn ext_for_image_mime(mime: &str) -> &'static str {
    match mime.to_ascii_lowercase().as_str() {
        "image/png" => "png",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "image/avif" => "avif",
        _ => "jpg",
    }
}

struct TempCbzGuard {
    path: PathBuf,
}

impl TempCbzGuard {
    fn create_exclusive() -> DomainResult<(Self, std::fs::File)> {
        for _ in 0..8 {
            let path = std::env::temp_dir().join(format!("xmedia-comic-{}.cbz", Uuid::now_v7()));
            let mut opts = std::fs::OpenOptions::new();
            opts.read(true).write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                opts.mode(0o600);
            }
            match opts.open(&path) {
                Ok(file) => return Ok((Self { path }, file)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => {
                    return Err(DomainError::Storage(format!(
                        "创建临时 CBZ 缓存文件失败: {e}"
                    )));
                }
            }
        }
        Err(DomainError::Storage(
            "创建独占临时 CBZ 缓存文件失败: 重试次数耗尽".into(),
        ))
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempCbzGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn init_disk_comic_cbz(
    file: std::fs::File,
    series_title: &str,
    chapter_title: &str,
    chapter_number: f64,
    total_pages: usize,
) -> DomainResult<ZipWriter<std::fs::File>> {
    let mut zip = ZipWriter::new(file);
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let meta = ComicInfoMeta {
        series: Some(series_title.to_owned()),
        title: Some(chapter_title.to_owned()),
        number: Some(chapter_number),
        page_count: u32::try_from(total_pages).ok(),
        ..Default::default()
    };
    let info_xml = render_comic_info_xml_preserving(&meta, None, None);
    zip.start_file("ComicInfo.xml", opts)
        .map_err(|e| DomainError::Storage(format!("CBZ write ComicInfo.xml failed: {e}")))?;
    zip.write_all(info_xml.as_bytes())
        .map_err(|e| DomainError::Storage(format!("CBZ write ComicInfo.xml failed: {e}")))?;
    Ok(zip)
}

fn append_pages_to_cbz(
    mut zip: ZipWriter<std::fs::File>,
    pages: &[(usize, Vec<u8>, String)],
) -> DomainResult<ZipWriter<std::fs::File>> {
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for (idx, bytes, mime) in pages {
        let ext = ext_for_image_mime(mime);
        let name = format!("{:04}.{ext}", idx + 1);
        zip.start_file(&name, opts)
            .map_err(|e| DomainError::Storage(format!("CBZ start page '{name}' failed: {e}")))?;
        zip.write_all(bytes)
            .map_err(|e| DomainError::Storage(format!("CBZ write page '{name}' failed: {e}")))?;
    }
    Ok(zip)
}

fn finish_disk_comic_cbz(zip: ZipWriter<std::fs::File>) -> DomainResult<()> {
    let mut file = zip
        .finish()
        .map_err(|e| DomainError::Storage(format!("CBZ finalize failed: {e}")))?;
    file.flush()
        .map_err(|e| DomainError::Storage(format!("CBZ flush failed: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Read;
    use zip::ZipArchive;

    use super::*;

    #[test]
    fn disk_backed_cbz_writer_streams_batches_with_exclusive_permissions() {
        let (guard, file) = TempCbzGuard::create_exclusive().unwrap();
        let path = guard.path().to_path_buf();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }

        let zip = init_disk_comic_cbz(file, "A & B", "Ch <1>", 1.5, 3).unwrap();
        let zip = append_pages_to_cbz(
            zip,
            &[
                (0, b"png-1".to_vec(), "image/png".to_string()),
                (1, b"webp-2".to_vec(), "image/webp".to_string()),
            ],
        )
        .unwrap();
        let zip =
            append_pages_to_cbz(zip, &[(2, b"jpg-3".to_vec(), "image/jpeg".to_string())]).unwrap();
        finish_disk_comic_cbz(zip).unwrap();

        let read_file = std::fs::File::open(&path).unwrap();
        let mut archive = ZipArchive::new(read_file).unwrap();
        assert_eq!(archive.len(), 4);
        let mut xml = String::new();
        archive
            .by_name("ComicInfo.xml")
            .unwrap()
            .read_to_string(&mut xml)
            .unwrap();
        assert!(xml.contains("<Series>A &amp; B</Series>"));
        assert!(xml.contains("<PageCount>3</PageCount>"));
        assert!(archive.by_name("0001.png").is_ok());
        assert!(archive.by_name("0002.webp").is_ok());
        assert!(archive.by_name("0003.jpg").is_ok());

        drop(guard);
        assert!(!path.exists());
    }
}

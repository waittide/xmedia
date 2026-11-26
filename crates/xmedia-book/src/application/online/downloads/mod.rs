mod runner;

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Condvar, Mutex};

use jiff::Timestamp;
use tokio::sync::Notify;
use uuid::Uuid;
use xmedia_core::application::ports::{ByteChunkStream, ByteRangeSpec, ByteRangeStream};
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::user::{User, UserId};

use crate::domain::book::{BookId, BookSource};

use super::dto::{
    DownloadJobStatus, OnlineDownloadFormat, OnlineDownloadJobDto, StartDownloadInput,
};
use super::helpers::{StoredDownloadJob, compute_chapters_per_sec};
use super::{BookOnlineService, FINISHED_DOWNLOAD_TTL_SECS, MAX_ACTIVE_DOWNLOADS_PER_USER};

const MAX_READ_DOWNLOAD_BYTES: usize = 512 * 1024 * 1024;

async fn collect_byte_stream_bounded(
    mut stream: ByteChunkStream,
    max_bytes: usize,
) -> DomainResult<Vec<u8>> {
    let mut buf = Vec::new();
    while let Some(chunk_res) =
        std::future::poll_fn(|cx| futures_core::Stream::poll_next(stream.as_mut(), cx)).await
    {
        let chunk = chunk_res?;
        if buf.len().saturating_add(chunk.len()) > max_bytes {
            return Err(DomainError::Validation(format!(
                "下载产物超出内存读取上限 ({max_bytes} bytes)"
            )));
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(buf)
}

impl BookOnlineService {
    pub async fn start_download(
        self: &Arc<Self>,
        actor: &User,
        mut input: StartDownloadInput,
    ) -> DomainResult<OnlineDownloadJobDto> {
        if input.import_to_library || input.book_id.is_some() {
            actor.ensure_admin()?;
        }
        let src = self
            .sources
            .load_source_for_actor(actor, input.source_id)
            .await?;
        input.book_url = Self::validate_caller_source_url(&input.book_url, &src.raw_json)?;
        input.toc_url = Self::validate_caller_source_url(&input.toc_url, &src.raw_json)?;
        if let Some(ref cover_url) = input.cover_url
            && !cover_url.trim().is_empty()
        {
            let clean_cover = Self::validate_caller_http_url(cover_url, &src.raw_json)?;
            input.cover_url = Some(clean_cover);
        }

        if let Some(bid) = input.book_id {
            let book = self.catalog.find_book_raw(BookId(bid)).await?;
            match &book.source {
                BookSource::OnlineTracked {
                    source_id, toc_url, ..
                } => {
                    if *source_id != input.source_id {
                        return Err(DomainError::Validation(
                            "下载任务的书源与目标追更书籍不一致".into(),
                        ));
                    }
                    if !toc_url.trim().is_empty() && input.toc_url.trim() != toc_url.trim() {
                        return Err(DomainError::Validation(
                            "下载任务的目录地址与目标追更书籍不一致".into(),
                        ));
                    }
                }
                _ => {
                    return Err(DomainError::Validation(
                        "目标书籍不是在线追更书籍，无法关联写入缓存".into(),
                    ));
                }
            }
        }

        let now = Timestamp::now();
        let mut stale_dirs = Vec::new();
        {
            let mut guard = self.downloads.write().await;
            let stale_ids: Vec<Uuid> = guard
                .iter()
                .filter(|(_, job)| {
                    matches!(
                        job.dto.status,
                        DownloadJobStatus::Completed
                            | DownloadJobStatus::Failed
                            | DownloadJobStatus::Cancelled
                    ) && now
                        .as_second()
                        .saturating_sub(job.dto.updated_at.as_second())
                        > FINISHED_DOWNLOAD_TTL_SECS
                })
                .map(|(id, _)| *id)
                .collect();
            for sid in stale_ids {
                guard.remove(&sid);
                stale_dirs.push(self.download_dir.join(sid.to_string()));
            }

            let active_count = guard
                .values()
                .filter(|j| {
                    j.dto.user_id == actor.id.0
                        && matches!(
                            j.dto.status,
                            DownloadJobStatus::Pending | DownloadJobStatus::Running
                        )
                })
                .count();
            if active_count >= MAX_ACTIVE_DOWNLOADS_PER_USER {
                return Err(DomainError::Validation(format!(
                    "同时进行的下载任务数已达上限 ({MAX_ACTIVE_DOWNLOADS_PER_USER})，请等待或取消部分任务"
                )));
            }
        }
        for dir in stale_dirs {
            let _ = tokio::fs::remove_dir_all(&dir).await;
        }

        let job_id = Uuid::now_v7();

        let dto = OnlineDownloadJobDto {
            id: job_id,
            user_id: actor.id.0,
            source_id: input.source_id,
            source_name: src.name,
            book_name: input.book_name.clone(),
            author: input.author.clone(),
            format: input.format,
            status: DownloadJobStatus::Pending,
            total_chapters: 0,
            processed_chapters: 0,
            failed_chapters: 0,
            chapters_per_sec: 0.0,
            elapsed_ms: 0,
            current_chapter_title: None,
            output_file_name: None,
            file_size: None,
            import_to_library: input.import_to_library,
            imported_book_id: None,
            error: None,
            created_at: now,
            updated_at: now,
        };

        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_cond = Arc::new((Mutex::new(false), Condvar::new()));
        let cancel_notify = Arc::new(Notify::new());

        {
            let mut guard = self.downloads.write().await;
            guard.insert(
                job_id,
                StoredDownloadJob {
                    dto: dto.clone(),
                    output_path: None,
                    started_at: None,
                    cancel_flag: cancel_flag.clone(),
                    cancel_cond: cancel_cond.clone(),
                    cancel_notify: cancel_notify.clone(),
                },
            );
        }

        let svc = Arc::clone(self);
        let actor_clone = actor.clone();
        tokio::spawn(async move {
            svc.execute_download_job(
                job_id,
                actor_clone,
                input,
                cancel_flag,
                cancel_cond,
                cancel_notify,
            )
            .await;
        });

        Ok(dto)
    }

    pub async fn list_downloads(&self, user_id: UserId) -> Vec<OnlineDownloadJobDto> {
        let guard = self.downloads.read().await;
        let mut list: Vec<OnlineDownloadJobDto> = guard
            .values()
            .filter(|j| j.dto.user_id == user_id.0)
            .map(StoredDownloadJob::snapshot_dto)
            .collect();
        list.sort_by_key(|j| std::cmp::Reverse(j.created_at));
        list
    }

    pub async fn get_download(
        &self,
        user_id: UserId,
        job_id: Uuid,
    ) -> DomainResult<OnlineDownloadJobDto> {
        let guard = self.downloads.read().await;
        let slot = guard
            .get(&job_id)
            .ok_or_else(|| DomainError::NotFound("未找到下载任务".into()))?;
        if slot.dto.user_id != user_id.0 {
            return Err(DomainError::Forbidden("无权查看他人下载任务".into()));
        }
        Ok(slot.snapshot_dto())
    }

    pub async fn cancel_download(
        &self,
        user_id: UserId,
        job_id: Uuid,
    ) -> DomainResult<OnlineDownloadJobDto> {
        let mut guard = self.downloads.write().await;
        let slot = guard
            .get_mut(&job_id)
            .ok_or_else(|| DomainError::NotFound("未找到下载任务".into()))?;
        if slot.dto.user_id != user_id.0 {
            return Err(DomainError::Forbidden("无权取消他人下载任务".into()));
        }
        if matches!(
            slot.dto.status,
            DownloadJobStatus::Pending | DownloadJobStatus::Running
        ) {
            slot.signal_cancel();
            if let Some(started) = slot.started_at {
                let elapsed = started.elapsed().as_millis() as u64;
                slot.dto.elapsed_ms = elapsed;
                slot.dto.chapters_per_sec =
                    compute_chapters_per_sec(slot.dto.processed_chapters, elapsed);
            }
            slot.dto.status = DownloadJobStatus::Cancelled;
            slot.dto.current_chapter_title = None;
            slot.dto.error = None;
            slot.dto.updated_at = Timestamp::now();
        }
        Ok(slot.snapshot_dto())
    }

    pub async fn delete_download(&self, user_id: UserId, job_id: Uuid) -> DomainResult<()> {
        {
            let mut guard = self.downloads.write().await;
            let slot = guard
                .get(&job_id)
                .ok_or_else(|| DomainError::NotFound("未找到下载任务".into()))?;
            if slot.dto.user_id != user_id.0 {
                return Err(DomainError::Forbidden("无权删除他人下载任务".into()));
            }
            if matches!(
                slot.dto.status,
                DownloadJobStatus::Pending | DownloadJobStatus::Running
            ) {
                slot.signal_cancel();
            }
            guard.remove(&job_id);
        }

        let job_dir = self.download_dir.join(job_id.to_string());
        if let Err(e) = tokio::fs::remove_dir_all(&job_dir).await
            && e.kind() != std::io::ErrorKind::NotFound
        {
            return Err(DomainError::Storage(format!("清理下载任务目录失败: {e}")));
        }
        Ok(())
    }

    pub async fn open_download_file_stream(
        &self,
        user_id: UserId,
        job_id: Uuid,
    ) -> DomainResult<(String, &'static str, ByteRangeStream)> {
        let (file_name, format, path, imported_book_id) = {
            let guard = self.downloads.read().await;
            let slot = guard
                .get(&job_id)
                .ok_or_else(|| DomainError::NotFound("未找到下载任务".into()))?;
            if slot.dto.user_id != user_id.0 {
                return Err(DomainError::Forbidden("无权下载他人任务产物".into()));
            }
            let fname = slot
                .dto
                .output_file_name
                .clone()
                .unwrap_or_else(|| "novel.epub".to_string());
            (
                fname,
                slot.dto.format,
                slot.output_path.clone(),
                slot.dto.imported_book_id,
            )
        };

        let mime = match format {
            OnlineDownloadFormat::Epub => "application/epub+zip",
            OnlineDownloadFormat::Txt => "text/plain; charset=utf-8",
        };

        if let Some(p) = path {
            let stream = self
                .catalog
                .open_path_stream(&p, ByteRangeSpec::Full)
                .await?;
            return Ok((file_name, mime, stream));
        }
        if let Some(book_id) = imported_book_id {
            let (_book, stream) = self
                .catalog
                .open_book_file_stream_for_user(user_id, BookId(book_id), ByteRangeSpec::Full)
                .await?;
            return Ok((file_name, mime, stream));
        }
        Err(DomainError::Validation("任务尚未完成或产物不存在".into()))
    }

    pub async fn read_download_file(
        &self,
        user_id: UserId,
        job_id: Uuid,
    ) -> DomainResult<(String, &'static str, Vec<u8>)> {
        let (file_name, mime, stream) = self.open_download_file_stream(user_id, job_id).await?;
        let bytes = collect_byte_stream_bounded(stream.stream, MAX_READ_DOWNLOAD_BYTES).await?;
        Ok((file_name, mime, bytes))
    }
}

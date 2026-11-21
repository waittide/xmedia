use std::path::Path;

use xmedia_core::application::ports::BinaryPayload;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;

use super::MediaWorkbenchService;
use crate::domain::media::MediaFileDetail;
use crate::domain::workspace::{RootId, path_to_slash};
use crate::infra::inspectors::book_content::{
    BookContentDocumentDto, EditableBookChapterDto, InsertedEpubImageDto, PolishBookReportDto,
    SaveBookChapterInput, TransformBookContentResultDto, convert_txt_to_epub, insert_epub_image,
    polish_epub_book, read_book_content, read_epub_internal_asset, resegment_txt_file,
    save_book_content, transform_book_content_batch,
};
use crate::infra::inspectors::book_normalize::{
    NormalizeBookItemReportDto, NormalizeBookToEpubOptions, NormalizeBooksBatchReportDto,
    normalize_book_to_epub,
};
use crate::infra::inspectors::zh_text::BookTextTransformOptions;

impl MediaWorkbenchService {
    pub async fn read_epub_asset(
        &self,
        root_id: RootId,
        rel_path: &str,
        asset_path: &str,
    ) -> DomainResult<BinaryPayload> {
        let (_, sandbox, _) = self
            .resolve_media_file(root_id, rel_path, Some(MediaKind::Book))
            .await?;
        let abs = sandbox.abs_path;
        let asset_owned = asset_path.to_string();
        tokio::task::spawn_blocking(move || read_epub_internal_asset(&abs, &asset_owned))
            .await
            .map_err(|e| DomainError::Storage(format!("epub asset read task failed: {e}")))?
    }

    pub async fn get_book_content(
        &self,
        root_id: RootId,
        rel_path: &str,
    ) -> DomainResult<BookContentDocumentDto> {
        let (_, sandbox, _) = self
            .resolve_media_file(root_id, rel_path, Some(MediaKind::Book))
            .await?;
        let abs = sandbox.abs_path;
        tokio::task::spawn_blocking(move || read_book_content(&abs))
            .await
            .map_err(|e| DomainError::Storage(format!("book content read task failed: {e}")))?
    }

    pub async fn resegment_txt_book(
        &self,
        root_id: RootId,
        rel_path: &str,
        chapter_pattern: Option<String>,
        volume_pattern: Option<String>,
    ) -> DomainResult<Vec<EditableBookChapterDto>> {
        let (_, sandbox, _) = self
            .resolve_media_file(root_id, rel_path, Some(MediaKind::Book))
            .await?;
        let abs = sandbox.abs_path;
        tokio::task::spawn_blocking(move || {
            resegment_txt_file(&abs, chapter_pattern.as_deref(), volume_pattern.as_deref())
        })
        .await
        .map_err(|e| DomainError::Storage(format!("txt resegment task failed: {e}")))?
    }

    pub async fn save_book_content(
        &self,
        root_id: RootId,
        rel_path: &str,
        chapters: Vec<SaveBookChapterInput>,
        stylesheets: Option<Vec<String>>,
        force_horizontal_spine: bool,
    ) -> DomainResult<MediaFileDetail> {
        let (_, sandbox, _) = self
            .resolve_writable_media_file(root_id, rel_path, Some(MediaKind::Book))
            .await?;
        let abs = sandbox.abs_path.clone();
        tokio::task::spawn_blocking(move || {
            save_book_content(
                &abs,
                &chapters,
                stylesheets.as_deref(),
                force_horizontal_spine,
            )
        })
        .await
        .map_err(|e| DomainError::Storage(format!("book content write task failed: {e}")))??;
        self.inspect_file(root_id, rel_path, Some(MediaKind::Book))
            .await
    }

    pub async fn convert_txt_book_to_epub(
        &self,
        root_id: RootId,
        rel_path: &str,
        chapters: Option<Vec<SaveBookChapterInput>>,
        stylesheets: Option<Vec<String>>,
        delete_source_txt: bool,
    ) -> DomainResult<MediaFileDetail> {
        let (root, sandbox, _) = self
            .resolve_writable_media_file(root_id, rel_path, Some(MediaKind::Book))
            .await?;
        let abs = sandbox.abs_path.clone();
        let epub_abs = tokio::task::spawn_blocking(move || {
            convert_txt_to_epub(
                &abs,
                chapters.as_deref(),
                stylesheets.as_deref(),
                delete_source_txt,
            )
        })
        .await
        .map_err(|e| DomainError::Storage(format!("txt to epub task failed: {e}")))??;

        let new_rel = epub_abs
            .strip_prefix(&root.path)
            .map(path_to_slash)
            .map_err(|_| DomainError::Storage("generated epub outside root".into()))?;
        self.inspect_file(root_id, &new_rel, Some(MediaKind::Book))
            .await
    }

    pub async fn transform_book_content(
        &self,
        chapters: Vec<EditableBookChapterDto>,
        stylesheets: Vec<String>,
        options: BookTextTransformOptions,
    ) -> DomainResult<TransformBookContentResultDto> {
        tokio::task::spawn_blocking(move || {
            transform_book_content_batch(chapters, stylesheets, &options)
        })
        .await
        .map_err(|e| DomainError::Storage(format!("book transform task failed: {e}")))?
    }

    pub async fn insert_epub_chapter_image(
        &self,
        root_id: RootId,
        rel_path: &str,
        chapter_source_href: Option<String>,
        bytes: Vec<u8>,
        mime_hint: Option<String>,
    ) -> DomainResult<InsertedEpubImageDto> {
        let (_, sandbox, _) = self
            .resolve_writable_media_file(root_id, rel_path, Some(MediaKind::Book))
            .await?;
        let abs = sandbox.abs_path;
        tokio::task::spawn_blocking(move || {
            insert_epub_image(
                &abs,
                chapter_source_href.as_deref(),
                &bytes,
                mime_hint.as_deref(),
            )
        })
        .await
        .map_err(|e| DomainError::Storage(format!("insert epub image task failed: {e}")))?
    }

    pub async fn polish_book(
        &self,
        root_id: RootId,
        rel_path: &str,
    ) -> DomainResult<(PolishBookReportDto, MediaFileDetail)> {
        let (_, sandbox, _) = self
            .resolve_writable_media_file(root_id, rel_path, Some(MediaKind::Book))
            .await?;
        let abs = sandbox.abs_path;
        let report = tokio::task::spawn_blocking(move || polish_epub_book(&abs))
            .await
            .map_err(|e| DomainError::Storage(format!("polish epub task failed: {e}")))??;
        let detail = self
            .inspect_file(root_id, rel_path, Some(MediaKind::Book))
            .await?;
        Ok((report, detail))
    }

    pub async fn normalize_books_to_epub(
        &self,
        root_id: RootId,
        rel_paths: &[String],
        options: NormalizeBookToEpubOptions,
    ) -> DomainResult<NormalizeBooksBatchReportDto> {
        if rel_paths.is_empty() {
            return Err(DomainError::Validation("请至少选择一本图书".into()));
        }

        let mut items = Vec::with_capacity(rel_paths.len());
        let mut succeeded = 0usize;
        let mut skipped = 0usize;
        let mut failed = 0usize;
        let mut total_before_bytes = 0u64;
        let mut total_after_bytes = 0u64;

        for rel in rel_paths {
            let resolved = self
                .resolve_writable_media_file(root_id, rel, Some(MediaKind::Book))
                .await;
            let (root, sandbox, _) = match resolved {
                Ok(v) => v,
                Err(err) => {
                    failed += 1;
                    items.push(NormalizeBookItemReportDto {
                        source_rel_path: rel.clone(),
                        output_rel_path: None,
                        backup_rel_path: None,
                        source_format: Path::new(rel)
                            .extension()
                            .and_then(|s| s.to_str())
                            .unwrap_or_default()
                            .to_ascii_lowercase(),
                        before_bytes: 0,
                        after_bytes: 0,
                        chapter_count: 0,
                        compressed_images: 0,
                        removed_orphans: 0,
                        deleted_source: false,
                        skipped: false,
                        skip_reason: None,
                        error: Some(err.to_string()),
                    });
                    continue;
                }
            };

            let abs = sandbox.abs_path;
            let root_path = root.path.clone();
            let opts_clone = options.clone();
            let result =
                tokio::task::spawn_blocking(move || normalize_book_to_epub(&abs, &opts_clone))
                    .await
                    .map_err(|e| DomainError::Storage(format!("normalize book task failed: {e}")))
                    .and_then(|r| r);

            match result {
                Ok(out) => {
                    if out.skipped {
                        skipped += 1;
                    } else {
                        succeeded += 1;
                        total_before_bytes = total_before_bytes.saturating_add(out.before_bytes);
                        total_after_bytes = total_after_bytes.saturating_add(out.after_bytes);
                    }
                    let output_rel = out
                        .output_path
                        .strip_prefix(&root_path)
                        .map(path_to_slash)
                        .ok();
                    let backup_rel = out
                        .backup_path
                        .as_ref()
                        .and_then(|bp| bp.strip_prefix(&root_path).ok())
                        .map(path_to_slash);
                    items.push(NormalizeBookItemReportDto {
                        source_rel_path: rel.clone(),
                        output_rel_path: output_rel,
                        backup_rel_path: backup_rel,
                        source_format: out.source_format,
                        before_bytes: out.before_bytes,
                        after_bytes: out.after_bytes,
                        chapter_count: out.chapter_count,
                        compressed_images: out.compressed_images,
                        removed_orphans: out.removed_orphans,
                        deleted_source: out.deleted_source,
                        skipped: out.skipped,
                        skip_reason: out.skip_reason,
                        error: None,
                    });
                }
                Err(err) => {
                    failed += 1;
                    items.push(NormalizeBookItemReportDto {
                        source_rel_path: rel.clone(),
                        output_rel_path: None,
                        backup_rel_path: None,
                        source_format: Path::new(rel)
                            .extension()
                            .and_then(|s| s.to_str())
                            .unwrap_or_default()
                            .to_ascii_lowercase(),
                        before_bytes: 0,
                        after_bytes: 0,
                        chapter_count: 0,
                        compressed_images: 0,
                        removed_orphans: 0,
                        deleted_source: false,
                        skipped: false,
                        skip_reason: None,
                        error: Some(err.to_string()),
                    });
                }
            }
        }

        Ok(NormalizeBooksBatchReportDto {
            total: rel_paths.len(),
            succeeded,
            skipped,
            failed,
            total_before_bytes,
            total_after_bytes,
            items,
        })
    }
}

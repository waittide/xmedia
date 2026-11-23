use jiff::Timestamp;
use xmedia_core::application::ports::SystemEvent;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::{Library, MediaKind};
use xmedia_core::domain::user::{User, UserId, require_admin};

use crate::application::dto::{UploadArchiveInput, UploadArchivePathInput, UploadArchiveResultDto};
use crate::application::filename_parser::parse_chapter_filename;
use crate::application::lifecycle::{apply_comic_info_to_chapter, apply_comic_info_to_series};
use crate::application::ports::{ArchiveInspection, StoredUploadFile};
use crate::domain::chapter::{Chapter, ChapterSourceKind};
use crate::domain::id::SeriesId;
use crate::domain::series::Series;

use super::{ComicCatalogService, sanitize_folder_name};

impl ComicCatalogService {
    pub async fn upload_archive(
        &self,
        actor: &User,
        input: UploadArchiveInput,
    ) -> DomainResult<UploadArchiveResultDto> {
        require_admin(actor)?;
        validate_archive_filename(&input.filename)?;

        let inspection = self
            .archives
            .inspect_bytes(&input.filename, &input.bytes)
            .await?;
        if inspection.page_count == 0 {
            return Err(DomainError::Validation(
                "uploaded archive contains no readable comic images".into(),
            ));
        }

        let managed_lib = self.get_or_create_managed_library().await?;
        let series = self
            .resolve_upload_series(
                &managed_lib,
                input.target_series_id,
                input.series_title_override.as_deref(),
                &input.filename,
                &inspection,
            )
            .await?;

        let folder_slug = sanitize_folder_name(&series.title);
        let stored = self
            .storage
            .store_uploaded_archive(&folder_slug, &input.filename, &input.bytes)
            .await?;

        self.finalize_uploaded_chapter(
            actor.id,
            managed_lib,
            series,
            &input.filename,
            inspection,
            stored,
        )
        .await
    }

    pub async fn upload_archive_from_path(
        &self,
        actor: &User,
        input: UploadArchivePathInput,
    ) -> DomainResult<UploadArchiveResultDto> {
        require_admin(actor)?;
        validate_archive_filename(&input.filename)?;

        let temp_path_str = input.temp_path.to_string_lossy().into_owned();
        let inspection = self
            .archives
            .inspect_source(ChapterSourceKind::ArchiveFile, &temp_path_str)
            .await?;
        if inspection.page_count == 0 {
            return Err(DomainError::Validation(
                "uploaded archive contains no readable comic images".into(),
            ));
        }

        let managed_lib = self.get_or_create_managed_library().await?;
        let series = self
            .resolve_upload_series(
                &managed_lib,
                input.target_series_id,
                input.series_title_override.as_deref(),
                &input.filename,
                &inspection,
            )
            .await?;

        let folder_slug = sanitize_folder_name(&series.title);
        let stored = self
            .storage
            .store_uploaded_archive_from_path(&folder_slug, &input.filename, &input.temp_path)
            .await?;

        self.finalize_uploaded_chapter(
            actor.id,
            managed_lib,
            series,
            &input.filename,
            inspection,
            stored,
        )
        .await
    }

    async fn resolve_upload_series(
        &self,
        managed_lib: &Library,
        target_series_id: Option<SeriesId>,
        series_title_override: Option<&str>,
        filename: &str,
        inspection: &ArchiveInspection,
    ) -> DomainResult<Series> {
        if let Some(target_id) = target_series_id {
            let existing = self
                .series
                .find_by_id(target_id)
                .await?
                .ok_or_else(|| DomainError::NotFound("target series not found".into()))?;
            if existing.library_id != managed_lib.id {
                return Err(DomainError::Validation(
                    "target series belongs to a read-only local library".into(),
                ));
            }
            return Ok(existing);
        }

        let parsed = parse_chapter_filename(filename, 1);
        let desired_title = series_title_override
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .or_else(|| {
                inspection
                    .comic_info
                    .as_ref()
                    .and_then(|ci| ci.series.clone())
                    .filter(|s| !s.trim().is_empty())
            })
            .or_else(|| parsed.inferred_series.clone())
            .unwrap_or_else(|| parsed.clean_title.clone());

        let all_series = self.series.list_by_library(managed_lib.id).await?;
        if let Some(existing) = all_series
            .into_iter()
            .find(|s| s.title.eq_ignore_ascii_case(desired_title.trim()))
        {
            Ok(existing)
        } else {
            let folder_slug = sanitize_folder_name(&desired_title);
            let root = self.storage.ensure_managed_root().await?;
            let source_dir = format!("{root}/{folder_slug}");
            Series::new(managed_lib.id, desired_title, source_dir)
        }
    }

    async fn finalize_uploaded_chapter(
        &self,
        actor_id: UserId,
        managed_lib: Library,
        mut series: Series,
        filename: &str,
        inspection: ArchiveInspection,
        stored: StoredUploadFile,
    ) -> DomainResult<UploadArchiveResultDto> {
        let existing_chapters = self.chapters.list_by_series(series.id).await?;
        let fallback_num = (existing_chapters.len() + 1) as u32;
        let parsed_with_index = parse_chapter_filename(filename, fallback_num);

        let mut chapter = Chapter::new(
            series.id,
            managed_lib.id,
            parsed_with_index.kind,
            parsed_with_index.number,
            parsed_with_index.volume_number,
            parsed_with_index.clean_title,
            inspection.page_count,
            ChapterSourceKind::ArchiveFile,
            stored.source_path,
            stored.file_size,
            stored.file_hash,
        )?;

        if let Some(ci) = &inspection.comic_info {
            apply_comic_info_to_chapter(&mut chapter, ci);
            if !series.metadata_locked {
                apply_comic_info_to_series(&mut series, ci);
            }
        }

        if let Ok(first_page) = self
            .archives
            .read_page(
                chapter.source_kind,
                &chapter.source_path,
                1,
                &chapter.file_hash,
            )
            .await
        {
            let thumb_key = format!("ch_{}", chapter.id);
            if let Ok(saved_key) = self
                .thumbnails
                .generate_from_bytes(&thumb_key, &first_page.bytes)
                .await
            {
                if series.cover_path.is_none() {
                    series.cover_path = Some(saved_key.clone());
                }
                chapter.cover_path = Some(saved_key);
            }
        }

        self.chapters.save(&chapter).await?;

        let active_count = self
            .chapters
            .list_by_series(series.id)
            .await?
            .into_iter()
            .filter(|c| !c.is_missing)
            .count() as u32;
        series.chapter_count = active_count;
        series.updated_at = Timestamp::now();
        self.series.save(&series).await?;
        self.bindings
            .bind(actor_id, MediaKind::Comic, series.id.0)
            .await?;

        self.events.publish(SystemEvent::MediaChanged {
            media_kind: MediaKind::Comic,
        });

        Ok(UploadArchiveResultDto {
            series_id: series.id,
            series_title: series.title,
            chapter_id: chapter.id,
            chapter_title: chapter.title,
            page_count: chapter.page_count,
        })
    }
}

fn validate_archive_filename(filename: &str) -> DomainResult<()> {
    let lower_name = filename.to_lowercase();
    if !(lower_name.ends_with(".cbz")
        || lower_name.ends_with(".zip")
        || lower_name.ends_with(".epub"))
    {
        return Err(DomainError::UnsupportedFormat(
            "only .cbz, .zip, and .epub comic archives are supported".into(),
        ));
    }
    Ok(())
}

use std::collections::HashMap;

use jiff::Timestamp;
use uuid::Uuid;
use xmedia_core::application::bindings::{
    BindMediaItemsInput, BindMediaItemsResultDto, MediaBindingService, UnbindMediaItemResultDto,
};
use xmedia_core::application::ports::{ByteRangeSpec, ByteRangeStream, SystemEvent};
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::{LibraryId, MediaKind};
use xmedia_core::domain::user::{ComicReaderMode, User, require_admin};

use crate::application::dto::{
    AdjacentChapterDto, ChapterItemDto, EffectiveReaderPrefDto, ReaderContextDto,
};
use crate::application::ports::PageContent;
use crate::domain::chapter::{Chapter, ChapterMetadataPatch, ChapterSourceKind};
use crate::domain::id::{ChapterId, SeriesId};
use crate::domain::reading::{ChapterProgress, format_chapter_number};
use crate::domain::series::{ReadingDirection, Series, SeriesMetadataPatch};

use super::ComicCatalogService;

impl ComicCatalogService {
    pub async fn update_series_metadata(
        &self,
        actor: &User,
        series_id: SeriesId,
        patch: SeriesMetadataPatch,
    ) -> DomainResult<Series> {
        require_admin(actor)?;
        let mut series = self
            .series
            .find_by_id(series_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("series not found".into()))?;
        series.apply_patch(patch)?;
        self.series.save(&series).await?;
        Ok(series)
    }

    pub async fn update_series_cover(
        &self,
        actor: &User,
        series_id: SeriesId,
        image_bytes: &[u8],
    ) -> DomainResult<Series> {
        require_admin(actor)?;
        let mut series = self
            .series
            .find_by_id(series_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("series not found".into()))?;
        let now = Timestamp::now();
        let key = format!("series_{}", series.id);
        let saved_key = self
            .thumbnails
            .generate_from_bytes(&key, image_bytes)
            .await?;
        series.set_custom_cover(saved_key, now);
        self.series.save(&series).await?;
        Ok(series)
    }

    pub async fn bind_series(
        &self,
        actor: &User,
        input: BindMediaItemsInput,
    ) -> DomainResult<BindMediaItemsResultDto> {
        let target_user_id = MediaBindingService::resolve_target_user_id(actor, input.user_id)?;
        let mut to_bind: Vec<Uuid> = Vec::new();
        if let Some(lib_uuid) = input.library_id {
            let lib_id = LibraryId(lib_uuid);
            for s in self.series.list_by_library(lib_id).await? {
                if s.online_tracking().is_none() {
                    to_bind.push(s.id.0);
                }
            }
        }
        for raw_id in input.item_ids {
            let Some(s) = self.series.find_by_id(SeriesId(raw_id)).await? else {
                continue;
            };
            if s.online_tracking().is_some()
                && !actor.role.is_admin()
                && !self
                    .bindings
                    .is_bound(actor.id, MediaKind::Comic, s.id.0)
                    .await?
            {
                continue;
            }
            to_bind.push(s.id.0);
        }
        let total_requested = to_bind.len();
        let bound_count = self
            .bindings
            .bind_batch(target_user_id, MediaKind::Comic, &to_bind)
            .await?;
        Ok(BindMediaItemsResultDto {
            bound_count,
            total_requested,
        })
    }

    pub async fn unbind_series(
        &self,
        actor: &User,
        series_id: SeriesId,
        requested_user_id: Option<Uuid>,
    ) -> DomainResult<UnbindMediaItemResultDto> {
        let target_user_id = MediaBindingService::resolve_target_user_id(actor, requested_user_id)?;
        let (unbound, remaining_bindings, deleted_from_pool) = self
            .bindings
            .unbind_with_gc(target_user_id, MediaKind::Comic, series_id.0, || async {
                if let Some(s) = self.series.find_by_id(series_id).await?
                    && s.online_tracking().is_some()
                {
                    self.delete_series_without_bindings(s).await?;
                    return Ok(true);
                }
                Ok(false)
            })
            .await?;
        if unbound && !deleted_from_pool {
            self.reading
                .delete_by_user_and_series(target_user_id, series_id)
                .await?;
        }
        Ok(UnbindMediaItemResultDto {
            unbound,
            remaining_bindings,
            deleted_from_pool,
        })
    }

    pub(super) async fn delete_series_internal(&self, series: Series) -> DomainResult<()> {
        self.bindings
            .delete_item_bindings(MediaKind::Comic, series.id.0)
            .await?;
        self.delete_series_without_bindings(series).await
    }

    pub(super) async fn delete_series_without_bindings(&self, series: Series) -> DomainResult<()> {
        let series_id = series.id;
        let lib = self.libraries.find_by_id(series.library_id).await?;
        let chapters = self.chapters.list_by_series(series_id).await?;

        if lib.is_some_and(|l| l.kind.is_managed()) {
            for ch in &chapters {
                if let Some(path) = ch.managed_file_path() {
                    let _ = self.storage.delete_managed_file(path).await;
                }
            }
        }
        self.reading.delete_by_series(series_id).await?;
        self.chapters.delete_by_series(series_id).await?;
        self.series.delete(series_id).await?;
        self.events.publish(SystemEvent::MediaChanged {
            media_kind: MediaKind::Comic,
        });
        Ok(())
    }

    pub async fn delete_series(&self, actor: &User, series_id: SeriesId) -> DomainResult<()> {
        require_admin(actor)?;
        let series = self
            .series
            .find_by_id(series_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("series not found".into()))?;
        self.delete_series_internal(series).await
    }

    pub async fn delete_chapter(&self, actor: &User, chapter_id: ChapterId) -> DomainResult<()> {
        require_admin(actor)?;
        let chapter = self
            .chapters
            .find_by_id(chapter_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("chapter not found".into()))?;
        let lib = self.libraries.find_by_id(chapter.library_id).await?;
        if lib.is_some_and(|l| l.kind.is_managed())
            && let Some(path) = chapter.managed_file_path()
        {
            let _ = self.storage.delete_managed_file(path).await;
        }
        self.reading.delete_by_chapter(chapter_id).await?;
        self.chapters.delete(chapter_id).await?;

        if let Some(mut series) = self.series.find_by_id(chapter.series_id).await? {
            let mut remaining = self.chapters.list_by_series(series.id).await?;
            remaining.retain(|c| c.id != chapter_id);
            if remaining.is_empty() {
                self.bindings
                    .delete_item_bindings(MediaKind::Comic, series.id.0)
                    .await?;
                self.reading.delete_by_series(series.id).await?;
                self.series.delete(series.id).await?;
            } else {
                remaining.sort_by(|a, b| a.cmp_order(b));
                let now = Timestamp::now();
                let active_count = remaining.iter().filter(|c| !c.is_missing).count() as u32;
                let _ = series.sync_chapter_count(active_count, now);
                series.reconcile_cover_after_chapter_removed(
                    chapter.cover_path.as_deref(),
                    remaining.iter().find_map(|c| c.cover_path.clone()),
                    now,
                );
                series.updated_at = now;
                self.series.save(&series).await?;
            }
        }

        self.events.publish(SystemEvent::MediaChanged {
            media_kind: MediaKind::Comic,
        });
        Ok(())
    }

    pub async fn get_reader_context(
        &self,
        user: &User,
        chapter_id: ChapterId,
    ) -> DomainResult<ReaderContextDto> {
        let target = self
            .chapters
            .find_by_id(chapter_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("chapter not found".into()))?;
        self.require_series_bound(user, target.series_id).await?;
        if target.is_missing {
            return Err(DomainError::NotFound(
                "chapter archive is missing on disk".into(),
            ));
        }

        let series = self
            .series
            .find_by_id(target.series_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("parent series not found".into()))?;

        let mut chapters = self.chapters.list_by_series(series.id).await?;
        chapters.retain(|c| !c.is_missing);
        chapters.sort_by(|a, b| a.cmp_order(b));

        let prog_list = self
            .reading
            .list_series_progress(user.id, series.id)
            .await?;
        let prog_map: HashMap<ChapterId, ChapterProgress> =
            prog_list.into_iter().map(|p| (p.chapter_id, p)).collect();

        let current_idx = chapters.iter().position(|c| c.id == target.id);

        let to_adjacent = |ch: &Chapter| AdjacentChapterDto {
            id: ch.id,
            kind: ch.kind,
            number: ch.number,
            number_label: format_chapter_number(ch.number),
            volume_number: ch.volume_number,
            title: ch.title.clone(),
            page_count: ch.page_count,
            is_completed: prog_map.get(&ch.id).is_some_and(|p| p.is_completed),
        };

        let prev_chapter = current_idx
            .and_then(|idx| idx.checked_sub(1))
            .and_then(|prev_idx| chapters.get(prev_idx))
            .map(to_adjacent);
        let next_chapter = current_idx
            .and_then(|idx| chapters.get(idx + 1))
            .map(to_adjacent);
        let all_chapters: Vec<AdjacentChapterDto> = chapters.iter().map(to_adjacent).collect();

        let (cur_page, is_done) = prog_map
            .get(&target.id)
            .map(|p| (p.current_page, p.is_completed))
            .unwrap_or((1, false));

        let comic_prefs = &user.preferences.comic;
        let series_pref = self.reading.get_series_pref(user.id, series.id).await?;
        let default_mode_from_prefs =
            resolve_default_reader_mode(series.reading_direction, comic_prefs.default_reader_mode);

        let effective_pref = if let Some(sp) = series_pref {
            EffectiveReaderPrefDto {
                reader_mode: sp.reader_mode,
                fit_mode: sp.fit_mode,
                offset_cover_page: sp.offset_cover_page,
                dim_images_in_dark: comic_prefs.dim_images_in_dark,
                preload_pages: comic_prefs.preload_pages,
                webtoon_width: sp.webtoon_width,
            }
        } else {
            EffectiveReaderPrefDto {
                reader_mode: default_mode_from_prefs,
                fit_mode: comic_prefs.default_fit_mode,
                offset_cover_page: comic_prefs.offset_cover_page,
                dim_images_in_dark: comic_prefs.dim_images_in_dark,
                preload_pages: comic_prefs.preload_pages,
                webtoon_width: comic_prefs.webtoon_width,
            }
        };

        Ok(ReaderContextDto {
            series_id: series.id,
            series_title: series.title,
            reading_direction: series.reading_direction,
            chapter: ChapterItemDto::from_domain(target, cur_page.max(1), is_done),
            effective_pref,
            prev_chapter,
            next_chapter,
            all_chapters,
        })
    }

    pub async fn update_chapter_metadata(
        &self,
        actor: &User,
        chapter_id: ChapterId,
        patch: ChapterMetadataPatch,
    ) -> DomainResult<ChapterItemDto> {
        require_admin(actor)?;
        let mut chapter = self
            .chapters
            .find_by_id(chapter_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("chapter not found".into()))?;
        chapter.apply_patch(patch)?;
        self.chapters.save(&chapter).await?;
        let (current_page, is_completed) = self
            .reading
            .get_chapter_progress(actor.id, chapter.id)
            .await?
            .map(|p| (p.current_page, p.is_completed))
            .unwrap_or((0, false));
        Ok(ChapterItemDto::from_domain(
            chapter,
            current_page,
            is_completed,
        ))
    }

    pub async fn read_chapter_page(
        &self,
        user: &User,
        chapter_id: ChapterId,
        page_number: u32,
    ) -> DomainResult<PageContent> {
        let chapter = self
            .chapters
            .find_by_id(chapter_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("chapter not found".into()))?;
        self.require_series_bound(user, chapter.series_id).await?;
        if chapter.is_missing {
            return Err(DomainError::NotFound("chapter source is missing".into()));
        }
        if page_number == 0 || page_number > chapter.page_count {
            return Err(DomainError::Validation(format!(
                "page {page_number} out of range 1..={}",
                chapter.page_count
            )));
        }
        let Some((effective_kind, effective_path)) = chapter.resolved_local_source() else {
            return Err(DomainError::NotFound(
                "online tracked chapter is not cached yet".into(),
            ));
        };
        self.archives
            .read_page(
                effective_kind,
                effective_path,
                page_number,
                &chapter.file_hash,
            )
            .await
    }

    pub async fn open_chapter_archive_stream(
        &self,
        user: &User,
        chapter_id: ChapterId,
        spec: ByteRangeSpec,
    ) -> DomainResult<(ByteRangeStream, String, String)> {
        let chapter = self
            .chapters
            .find_by_id(chapter_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("chapter not found".into()))?;
        self.require_series_bound(user, chapter.series_id).await?;
        if chapter.is_missing {
            return Err(DomainError::NotFound("chapter source is missing".into()));
        }
        let Some((ChapterSourceKind::ArchiveFile, archive_path)) = chapter.resolved_local_source()
        else {
            return Err(DomainError::Validation(
                "direct download is only available for archive-backed chapters".into(),
            ));
        };
        self.archives
            .open_raw_archive_stream(archive_path, spec)
            .await
    }

    async fn ensure_cover_accessible(&self, user: &User, cover_key: &str) -> DomainResult<()> {
        let matched_series = self.series.find_series_by_cover_key(cover_key).await?;
        let mut matched_tracked_series: Vec<SeriesId> = Vec::new();
        for s in &matched_series {
            if s.is_online_tracked() {
                matched_tracked_series.push(s.id);
            } else {
                return Ok(());
            }
        }
        if matched_tracked_series.is_empty() {
            let matched_chapters = self.chapters.find_chapters_by_cover_key(cover_key).await?;
            let mut seen_series = std::collections::HashSet::new();
            for ch in matched_chapters {
                if !seen_series.insert(ch.series_id) {
                    continue;
                }
                if let Some(s) = self.series.find_by_id(ch.series_id).await? {
                    if s.is_online_tracked() {
                        matched_tracked_series.push(s.id);
                    } else {
                        return Ok(());
                    }
                }
            }
        }
        if !matched_tracked_series.is_empty() {
            let bound = self
                .bindings
                .list_bound_ids(user.id, MediaKind::Comic)
                .await?;
            if matched_tracked_series
                .iter()
                .any(|sid| bound.contains(&sid.0))
            {
                return Ok(());
            }
            return Err(DomainError::NotFound("cover not found".into()));
        }
        Ok(())
    }

    pub async fn read_cover(&self, user: &User, cover_key: &str) -> DomainResult<PageContent> {
        self.ensure_cover_accessible(user, cover_key).await?;
        self.thumbnails.read_thumbnail(cover_key).await
    }

    pub async fn read_cover_sized(
        &self,
        user: &User,
        cover_key: &str,
        max_size: Option<u32>,
    ) -> DomainResult<PageContent> {
        self.ensure_cover_accessible(user, cover_key).await?;
        match max_size {
            Some(size) => {
                self.thumbnails
                    .read_thumbnail_sized(cover_key, Some(size))
                    .await
            }
            None => self.thumbnails.read_thumbnail(cover_key).await,
        }
    }
}

fn resolve_default_reader_mode(
    series_direction: ReadingDirection,
    user_default: ComicReaderMode,
) -> ComicReaderMode {
    if series_direction == ReadingDirection::Webtoon && user_default == ComicReaderMode::RtlDouble {
        ComicReaderMode::Webtoon
    } else {
        user_default
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_reader_mode_honors_user_preference_for_standard_series() {
        for mode in [
            ComicReaderMode::RtlDouble,
            ComicReaderMode::LtrSingle,
            ComicReaderMode::LtrDouble,
            ComicReaderMode::Webtoon,
        ] {
            assert_eq!(
                resolve_default_reader_mode(ReadingDirection::Rtl, mode),
                mode
            );
            assert_eq!(
                resolve_default_reader_mode(ReadingDirection::Ltr, mode),
                mode
            );
        }
    }

    #[test]
    fn webtoon_series_auto_uses_webtoon_only_when_user_keeps_default_rtl_double() {
        assert_eq!(
            resolve_default_reader_mode(ReadingDirection::Webtoon, ComicReaderMode::RtlDouble),
            ComicReaderMode::Webtoon
        );
        assert_eq!(
            resolve_default_reader_mode(ReadingDirection::Webtoon, ComicReaderMode::LtrSingle),
            ComicReaderMode::LtrSingle
        );
        assert_eq!(
            resolve_default_reader_mode(ReadingDirection::Webtoon, ComicReaderMode::LtrDouble),
            ComicReaderMode::LtrDouble
        );
    }
}

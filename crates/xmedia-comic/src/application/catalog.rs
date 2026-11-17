use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use jiff::Timestamp;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::{Library, MediaKind};
use xmedia_core::domain::repository::LibraryRepository;
use xmedia_core::domain::user::{ComicReaderMode, User};

use crate::application::dto::{
    AdjacentChapterDto, ChapterItemDto, ComicLibraryStatsDto, ContinueReadingItemDto,
    EffectiveReaderPrefDto, HomeOverviewDto, ReaderContextDto, SeriesDetailDto, SeriesQueryFilter,
    SeriesSummaryDto, TaxonomyCountDto, TaxonomyOverviewDto, UploadArchiveInput,
    UploadArchiveResultDto,
};
use crate::application::filename_parser::parse_chapter_filename;
use crate::application::lifecycle::{apply_comic_info_to_chapter, apply_comic_info_to_series};
use crate::application::ports::{
    ArchiveReaderPort, ComicFileStoragePort, PageContent, ThumbnailGeneratorPort,
};
use crate::domain::chapter::{Chapter, ChapterMetadataPatch, ChapterSourceKind};
use crate::domain::id::{ChapterId, ComicShelfId, SeriesId};
use crate::domain::natural::natural_cmp;
use crate::domain::reading::{ChapterProgress, compute_continue_reading, format_chapter_number};
use crate::domain::repository::{ChapterRepository, ComicReadingRepository, SeriesRepository};
use crate::domain::series::{ReadingDirection, Series, SeriesMetadataPatch};

pub struct ComicCatalogService {
    libraries: Arc<dyn LibraryRepository>,
    series: Arc<dyn SeriesRepository>,
    chapters: Arc<dyn ChapterRepository>,
    reading: Arc<dyn ComicReadingRepository>,
    archives: Arc<dyn ArchiveReaderPort>,
    thumbnails: Arc<dyn ThumbnailGeneratorPort>,
    storage: Arc<dyn ComicFileStoragePort>,
}

impl ComicCatalogService {
    pub fn new(
        libraries: Arc<dyn LibraryRepository>,
        series: Arc<dyn SeriesRepository>,
        chapters: Arc<dyn ChapterRepository>,
        reading: Arc<dyn ComicReadingRepository>,
        archives: Arc<dyn ArchiveReaderPort>,
        thumbnails: Arc<dyn ThumbnailGeneratorPort>,
        storage: Arc<dyn ComicFileStoragePort>,
    ) -> Self {
        Self {
            libraries,
            series,
            chapters,
            reading,
            archives,
            thumbnails,
            storage,
        }
    }

    fn ensure_admin(actor: &User) -> DomainResult<()> {
        if actor.role.is_admin() {
            Ok(())
        } else {
            Err(DomainError::Forbidden(
                "administrator privileges required".into(),
            ))
        }
    }

    pub async fn home_overview(&self, user: &User) -> DomainResult<HomeOverviewDto> {
        let all_series = self.series.list_all().await?;
        let user_progress = self.reading.list_user_progress(user.id).await?;
        let favorites = self.reading.list_favorites(user.id).await?;
        let fav_set: HashSet<SeriesId> = favorites.iter().map(|f| f.series_id).collect();

        let mut progress_by_series: HashMap<SeriesId, Vec<ChapterProgress>> = HashMap::new();
        let mut completed_chapters_total = 0_u32;
        for p in user_progress {
            if p.is_completed {
                completed_chapters_total += 1;
            }
            progress_by_series.entry(p.series_id).or_default().push(p);
        }

        let mut total_chapters = 0_u32;
        let mut summaries: Vec<SeriesSummaryDto> = Vec::with_capacity(all_series.len());
        let mut continue_items: Vec<ContinueReadingItemDto> = Vec::new();

        for s in all_series {
            total_chapters += s.chapter_count;
            let is_fav = fav_set.contains(&s.id);
            let s_prog = progress_by_series
                .get(&s.id)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            let summary = self.build_series_summary(s, s_prog, is_fav).await?;
            if let Some(pointer) = summary.continue_reading.clone() {
                continue_items.push(ContinueReadingItemDto {
                    series: summary.clone(),
                    pointer,
                });
            }
            summaries.push(summary);
        }

        continue_items.sort_by_key(|a| std::cmp::Reverse(a.pointer.last_read_at));
        continue_items.truncate(16);

        let mut recently_updated = summaries.clone();
        recently_updated.sort_by_key(|a| std::cmp::Reverse(a.updated_at));
        recently_updated.truncate(18);

        let total_series = summaries.len() as u32;
        let mut recently_added = summaries;
        recently_added.sort_by_key(|a| std::cmp::Reverse(a.created_at));
        recently_added.truncate(18);

        Ok(HomeOverviewDto {
            continue_reading: continue_items,
            recently_updated,
            recently_added,
            stats: ComicLibraryStatsDto {
                series_count: total_series,
                chapter_count: total_chapters,
                completed_chapters: completed_chapters_total,
                favorites_count: favorites.len() as u32,
            },
        })
    }

    pub async fn list_series(
        &self,
        user: &User,
        filter: SeriesQueryFilter,
    ) -> DomainResult<Vec<SeriesSummaryDto>> {
        let base_list = if let Some(lib_id) = filter.library_id {
            self.series.list_by_library(lib_id).await?
        } else {
            self.series.list_all().await?
        };

        let shelf_filter: Option<HashSet<SeriesId>> = if let Some(shelf_id) = filter.shelf_id {
            let shelf = self
                .reading
                .find_shelf(shelf_id)
                .await?
                .ok_or_else(|| DomainError::NotFound("shelf not found".into()))?;
            if shelf.user_id != user.id {
                return Err(DomainError::Forbidden("access denied to shelf".into()));
            }
            Some(shelf.series_ids.into_iter().collect())
        } else {
            None
        };

        let user_progress = self.reading.list_user_progress(user.id).await?;
        let mut progress_by_series: HashMap<SeriesId, Vec<ChapterProgress>> = HashMap::new();
        for p in user_progress {
            progress_by_series.entry(p.series_id).or_default().push(p);
        }

        let favorites = self.reading.list_favorites(user.id).await?;
        let fav_set: HashSet<SeriesId> = favorites.into_iter().map(|f| f.series_id).collect();

        let search_q = filter
            .search
            .as_ref()
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty());

        let mut results = Vec::new();
        for s in base_list {
            if let Some(allowed) = &shelf_filter
                && !allowed.contains(&s.id)
            {
                continue;
            }
            if filter.favorites_only && !fav_set.contains(&s.id) {
                continue;
            }
            if let Some(status) = filter.status
                && s.status != status
            {
                continue;
            }
            if let Some(writer) = &filter.writer
                && !s
                    .writers
                    .iter()
                    .any(|w| w.eq_ignore_ascii_case(writer.trim()))
            {
                continue;
            }
            if let Some(penciller) = &filter.penciller
                && !s
                    .pencillers
                    .iter()
                    .any(|p| p.eq_ignore_ascii_case(penciller.trim()))
            {
                continue;
            }
            if let Some(tag) = &filter.tag
                && !s.tags.iter().any(|t| t.eq_ignore_ascii_case(tag.trim()))
            {
                continue;
            }
            if let Some(genre) = &filter.genre
                && !s
                    .genres
                    .iter()
                    .any(|g| g.eq_ignore_ascii_case(genre.trim()))
            {
                continue;
            }
            if let Some(q) = &search_q {
                let matched = s.title.to_lowercase().contains(q)
                    || s.alt_titles.iter().any(|a| a.to_lowercase().contains(q))
                    || s.summary.to_lowercase().contains(q)
                    || s.writers.iter().any(|w| w.to_lowercase().contains(q))
                    || s.pencillers.iter().any(|p| p.to_lowercase().contains(q))
                    || s.tags.iter().any(|t| t.to_lowercase().contains(q))
                    || s.genres.iter().any(|g| g.to_lowercase().contains(q));
                if !matched {
                    continue;
                }
            }

            let is_fav = fav_set.contains(&s.id);
            let s_prog = progress_by_series
                .get(&s.id)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            let summary = self.build_series_summary(s, s_prog, is_fav).await?;

            if let Some(rs) = &filter.read_status {
                let keep = match rs.as_str() {
                    "unread" => {
                        summary.completed_chapters == 0 && summary.continue_reading.is_none()
                    }
                    "completed" => {
                        summary.chapter_count > 0
                            && summary.completed_chapters >= summary.chapter_count
                    }
                    "in_progress" => {
                        summary.continue_reading.is_some()
                            && summary.completed_chapters < summary.chapter_count
                    }
                    _ => true,
                };
                if !keep {
                    continue;
                }
            }

            results.push(summary);
        }

        let sort_by = filter.sort_by.as_deref().unwrap_or("updated_at");
        let asc = filter.sort_dir.as_deref() == Some("asc");

        results.sort_by(|a, b| {
            let ord = match sort_by {
                "title" => natural_cmp(&a.title, &b.title),
                "created_at" => a.created_at.cmp(&b.created_at),
                "chapter_count" => a.chapter_count.cmp(&b.chapter_count),
                _ => a.updated_at.cmp(&b.updated_at),
            };
            if asc { ord } else { ord.reverse() }
        });

        Ok(results)
    }

    pub async fn get_series_detail(
        &self,
        user: &User,
        series_id: SeriesId,
    ) -> DomainResult<SeriesDetailDto> {
        let series = self
            .series
            .find_by_id(series_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("series not found".into()))?;

        let mut chapters = self.chapters.list_by_series(series_id).await?;
        chapters.sort_by(|a, b| a.cmp_order(b));

        let progress_list = self
            .reading
            .list_series_progress(user.id, series_id)
            .await?;
        let continue_reading = compute_continue_reading(series_id, &chapters, &progress_list);

        let prog_map: HashMap<ChapterId, ChapterProgress> = progress_list
            .into_iter()
            .map(|p| (p.chapter_id, p))
            .collect();

        let mut completed_chapters = 0_u32;
        let mut chapter_dtos = Vec::with_capacity(chapters.len());
        for ch in chapters {
            let (cur_page, is_done) = prog_map
                .get(&ch.id)
                .map(|p| (p.current_page, p.is_completed))
                .unwrap_or((0, false));
            if is_done && !ch.is_missing {
                completed_chapters += 1;
            }
            chapter_dtos.push(ChapterItemDto::from_domain(ch, cur_page, is_done));
        }

        let is_favorite = self.reading.is_favorite(user.id, series_id).await?;
        let shelves = self.reading.list_shelves(user.id).await?;
        let shelf_ids: Vec<ComicShelfId> = shelves
            .into_iter()
            .filter(|sh| sh.series_ids.contains(&series_id))
            .map(|sh| sh.id)
            .collect();
        let reader_pref = self.reading.get_series_pref(user.id, series_id).await?;
        let cover_url = series
            .cover_path
            .as_ref()
            .map(|key| format!("/api/v1/comic/covers/{key}"));

        Ok(SeriesDetailDto {
            series,
            cover_url,
            is_favorite,
            shelf_ids,
            completed_chapters,
            continue_reading,
            reader_pref,
            chapters: chapter_dtos,
        })
    }

    pub async fn update_series_metadata(
        &self,
        actor: &User,
        series_id: SeriesId,
        patch: SeriesMetadataPatch,
    ) -> DomainResult<Series> {
        Self::ensure_admin(actor)?;
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
        Self::ensure_admin(actor)?;
        let mut series = self
            .series
            .find_by_id(series_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("series not found".into()))?;
        let now = Timestamp::now();
        let key = format!("series_custom_{}_{}", series.id, now.as_second());
        let saved_key = self
            .thumbnails
            .generate_from_bytes(&key, image_bytes)
            .await?;
        series.cover_path = Some(saved_key);
        series.updated_at = now;
        self.series.save(&series).await?;
        Ok(series)
    }

    pub async fn delete_series(&self, actor: &User, series_id: SeriesId) -> DomainResult<()> {
        Self::ensure_admin(actor)?;
        let series = self
            .series
            .find_by_id(series_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("series not found".into()))?;
        let lib = self.libraries.find_by_id(series.library_id).await?;
        let chapters = self.chapters.list_by_series(series_id).await?;

        if lib.is_some_and(|l| l.kind.is_managed()) {
            for ch in &chapters {
                let _ = self.storage.delete_managed_file(&ch.source_path).await;
            }
        }
        self.reading.delete_by_series(series_id).await?;
        self.chapters.delete_by_series(series_id).await?;
        self.series.delete(series_id).await
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
        let default_mode_from_series = match series.reading_direction {
            ReadingDirection::Rtl => ComicReaderMode::RtlDouble,
            ReadingDirection::Ltr => match comic_prefs.default_reader_mode {
                ComicReaderMode::LtrSingle | ComicReaderMode::LtrDouble => {
                    comic_prefs.default_reader_mode
                }
                _ => ComicReaderMode::LtrSingle,
            },
            ReadingDirection::Webtoon => ComicReaderMode::Webtoon,
        };

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
                reader_mode: default_mode_from_series,
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
        Self::ensure_admin(actor)?;
        let mut chapter = self
            .chapters
            .find_by_id(chapter_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("chapter not found".into()))?;
        chapter.apply_patch(patch)?;
        self.chapters.save(&chapter).await?;
        Ok(ChapterItemDto::from_domain(chapter, 0, false))
    }

    pub async fn read_chapter_page(
        &self,
        chapter_id: ChapterId,
        page_number: u32,
    ) -> DomainResult<PageContent> {
        let chapter = self
            .chapters
            .find_by_id(chapter_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("chapter not found".into()))?;
        if chapter.is_missing {
            return Err(DomainError::NotFound("chapter source is missing".into()));
        }
        if page_number == 0 || page_number > chapter.page_count {
            return Err(DomainError::Validation(format!(
                "page {page_number} out of range 1..={}",
                chapter.page_count
            )));
        }
        self.archives
            .read_page(
                chapter.source_kind,
                &chapter.source_path,
                page_number,
                &chapter.file_hash,
            )
            .await
    }

    pub async fn download_chapter_archive(
        &self,
        chapter_id: ChapterId,
    ) -> DomainResult<(Vec<u8>, String, String)> {
        let chapter = self
            .chapters
            .find_by_id(chapter_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("chapter not found".into()))?;
        if chapter.is_missing || chapter.source_kind != ChapterSourceKind::ArchiveFile {
            return Err(DomainError::Validation(
                "direct download is only available for archive-backed chapters".into(),
            ));
        }
        self.archives.read_raw_archive(&chapter.source_path).await
    }

    pub async fn read_cover(&self, cover_key: &str) -> DomainResult<PageContent> {
        self.thumbnails.read_thumbnail(cover_key).await
    }

    pub async fn upload_archive(
        &self,
        actor: &User,
        input: UploadArchiveInput,
    ) -> DomainResult<UploadArchiveResultDto> {
        Self::ensure_admin(actor)?;
        let lower_name = input.filename.to_lowercase();
        if !(lower_name.ends_with(".cbz")
            || lower_name.ends_with(".zip")
            || lower_name.ends_with(".epub"))
        {
            return Err(DomainError::UnsupportedFormat(
                "only .cbz, .zip, and .epub comic archives are supported".into(),
            ));
        }

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
        let parsed = parse_chapter_filename(&input.filename, 1);

        let mut series = if let Some(target_id) = input.target_series_id {
            self.series
                .find_by_id(target_id)
                .await?
                .ok_or_else(|| DomainError::NotFound("target series not found".into()))?
        } else {
            let desired_title = input
                .series_title_override
                .as_ref()
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

            let all_series = self.series.list_all().await?;
            if let Some(existing) = all_series
                .into_iter()
                .find(|s| s.title.eq_ignore_ascii_case(desired_title.trim()))
            {
                existing
            } else {
                let folder_slug = sanitize_folder_name(&desired_title);
                let root = self.storage.ensure_managed_root().await?;
                let source_dir = format!("{root}/{folder_slug}");
                Series::new(managed_lib.id, desired_title, source_dir)?
            }
        };

        let folder_slug = sanitize_folder_name(&series.title);
        let stored = self
            .storage
            .store_uploaded_archive(&folder_slug, &input.filename, &input.bytes)
            .await?;

        let existing_chapters = self.chapters.list_by_series(series.id).await?;
        let fallback_num = (existing_chapters.len() + 1) as u32;
        let parsed_with_index = parse_chapter_filename(&input.filename, fallback_num);

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

        Ok(UploadArchiveResultDto {
            series_id: series.id,
            series_title: series.title,
            chapter_id: chapter.id,
            chapter_title: chapter.title,
            page_count: chapter.page_count,
        })
    }

    pub async fn taxonomy_overview(&self) -> DomainResult<TaxonomyOverviewDto> {
        let all = self.series.list_all().await?;
        let mut tags: HashMap<String, u32> = HashMap::new();
        let mut genres: HashMap<String, u32> = HashMap::new();
        let mut writers: HashMap<String, u32> = HashMap::new();
        let mut pencillers: HashMap<String, u32> = HashMap::new();

        for s in all {
            for t in s.tags {
                *tags.entry(t).or_insert(0) += 1;
            }
            for g in s.genres {
                *genres.entry(g).or_insert(0) += 1;
            }
            for w in s.writers {
                *writers.entry(w).or_insert(0) += 1;
            }
            for p in s.pencillers {
                *pencillers.entry(p).or_insert(0) += 1;
            }
        }

        Ok(TaxonomyOverviewDto {
            tags: to_sorted_counts(tags),
            genres: to_sorted_counts(genres),
            writers: to_sorted_counts(writers),
            pencillers: to_sorted_counts(pencillers),
        })
    }

    pub async fn build_series_summary(
        &self,
        series: Series,
        progress_list: &[ChapterProgress],
        is_favorite: bool,
    ) -> DomainResult<SeriesSummaryDto> {
        let (completed_chapters, continue_reading) = if progress_list.is_empty() {
            (0, None)
        } else {
            let mut chapters = self.chapters.list_by_series(series.id).await?;
            chapters.sort_by(|a, b| a.cmp_order(b));
            let active_ids: HashSet<ChapterId> = chapters
                .iter()
                .filter(|c| !c.is_missing)
                .map(|c| c.id)
                .collect();
            let done = progress_list
                .iter()
                .filter(|p| p.is_completed && active_ids.contains(&p.chapter_id))
                .count() as u32;
            let ptr = compute_continue_reading(series.id, &chapters, progress_list);
            (done, ptr)
        };

        let cover_url = series
            .cover_path
            .as_ref()
            .map(|key| format!("/api/v1/comic/covers/{key}"));

        Ok(SeriesSummaryDto {
            id: series.id,
            library_id: series.library_id,
            title: series.title,
            writers: series.writers,
            pencillers: series.pencillers,
            status: series.status,
            reading_direction: series.reading_direction,
            age_rating: series.age_rating,
            genres: series.genres,
            tags: series.tags,
            cover_url,
            chapter_count: series.chapter_count,
            completed_chapters,
            is_favorite,
            continue_reading,
            created_at: series.created_at,
            updated_at: series.updated_at,
        })
    }

    async fn get_or_create_managed_library(&self) -> DomainResult<Library> {
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
}

fn sanitize_folder_name(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            other => other,
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.');
    if trimmed.is_empty() {
        "untitled_series".into()
    } else {
        trimmed.to_owned()
    }
}

fn to_sorted_counts(map: HashMap<String, u32>) -> Vec<TaxonomyCountDto> {
    let mut vec: Vec<TaxonomyCountDto> = map
        .into_iter()
        .map(|(name, count)| TaxonomyCountDto { name, count })
        .collect();
    vec.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| natural_cmp(&a.name, &b.name))
    });
    vec
}

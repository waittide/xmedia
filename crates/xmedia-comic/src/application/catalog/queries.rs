use std::collections::{HashMap, HashSet};

use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::{LibraryId, MediaKind};
use xmedia_core::domain::user::User;

use crate::application::dto::{
    ChapterItemDto, ComicLibraryStatsDto, ContinueReadingItemDto, HomeOverviewDto,
    OnlineTrackedSeriesDto, PoolSeriesSummaryDto, SeriesDetailDto, SeriesQueryFilter,
    SeriesSummaryDto, TaxonomyOverviewDto,
};
use crate::domain::id::{ChapterId, ComicShelfId, SeriesId};
use crate::domain::natural::natural_cmp;
use crate::domain::reading::{ChapterProgress, compute_continue_reading};

use super::{ComicCatalogService, to_sorted_counts};

impl ComicCatalogService {
    pub async fn home_overview(&self, user: &User) -> DomainResult<HomeOverviewDto> {
        let bound_ids = self
            .bindings
            .list_bound_ids(user.id, MediaKind::Comic)
            .await?;
        let all_series: Vec<_> = if bound_ids.is_empty() {
            Vec::new()
        } else {
            self.series
                .list_all()
                .await?
                .into_iter()
                .filter(|s| bound_ids.contains(&s.id.0))
                .collect()
        };
        let user_progress: Vec<_> = self
            .reading
            .list_user_progress(user.id)
            .await?
            .into_iter()
            .filter(|p| bound_ids.contains(&p.series_id.0))
            .collect();
        let favorites: Vec<_> = self
            .reading
            .list_favorites(user.id)
            .await?
            .into_iter()
            .filter(|f| bound_ids.contains(&f.series_id.0))
            .collect();
        let fav_set: HashSet<SeriesId> = favorites.iter().map(|f| f.series_id).collect();

        let mut progress_by_series: HashMap<SeriesId, Vec<ChapterProgress>> = HashMap::new();
        for p in user_progress {
            progress_by_series.entry(p.series_id).or_default().push(p);
        }

        let series_ids_with_progress: Vec<SeriesId> = all_series
            .iter()
            .filter(|s| progress_by_series.contains_key(&s.id))
            .map(|s| s.id)
            .collect();
        let chapters_by_series = self
            .load_sorted_chapters_map(&series_ids_with_progress)
            .await?;

        let mut completed_chapters_total = 0_u32;
        for (sid, progs) in &progress_by_series {
            let active_chapter_ids: HashSet<ChapterId> = chapters_by_series
                .get(sid)
                .map(|list| {
                    list.iter()
                        .filter(|c| !c.is_missing)
                        .map(|c| c.id)
                        .collect()
                })
                .unwrap_or_default();
            for p in progs {
                if p.is_completed && active_chapter_ids.contains(&p.chapter_id) {
                    completed_chapters_total += 1;
                }
            }
        }

        let mut total_chapters = 0_u32;
        let mut distinct_tags: HashSet<String> = HashSet::new();
        let mut distinct_writers: HashSet<String> = HashSet::new();
        let mut summaries: Vec<SeriesSummaryDto> = Vec::with_capacity(all_series.len());
        let mut continue_items: Vec<ContinueReadingItemDto> = Vec::new();

        for s in all_series {
            total_chapters += s.chapter_count;
            for t in &s.tags {
                distinct_tags.insert(t.clone());
            }
            for w in &s.writers {
                distinct_writers.insert(w.clone());
            }
            let is_fav = fav_set.contains(&s.id);
            let s_prog = progress_by_series
                .get(&s.id)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            let s_chapters = chapters_by_series
                .get(&s.id)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            let summary = Self::build_series_summary_with_chapters(s, s_prog, s_chapters, is_fav);
            if let Some(pointer) = summary.continue_reading.clone() {
                continue_items.push(ContinueReadingItemDto {
                    series: summary.clone(),
                    pointer,
                });
            }
            summaries.push(summary);
        }

        continue_items.sort_by_key(|a| std::cmp::Reverse(a.pointer.last_read_at));
        let in_progress_count = continue_items.len() as u32;
        continue_items.truncate(16);

        let mut recently_updated = summaries.clone();
        recently_updated.sort_by_key(|a| std::cmp::Reverse(a.updated_at));
        recently_updated.truncate(18);

        let total_series = summaries.len() as u32;
        let bookmarks_count = self
            .reading
            .list_user_bookmarks(user.id)
            .await?
            .into_iter()
            .filter(|b| bound_ids.contains(&b.series_id.0))
            .count() as u32;
        let shelves_count = self.reading.list_shelves(user.id).await?.len() as u32;
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
                in_progress_count,
                bookmarks_count,
                favorites_count: favorites.len() as u32,
                shelves_count,
                tags_count: distinct_tags.len() as u32,
                writers_count: distinct_writers.len() as u32,
            },
        })
    }

    pub async fn list_series(
        &self,
        user: &User,
        filter: SeriesQueryFilter,
    ) -> DomainResult<Vec<SeriesSummaryDto>> {
        let bound_ids = self
            .bindings
            .list_bound_ids(user.id, MediaKind::Comic)
            .await?;
        if bound_ids.is_empty() {
            return Ok(Vec::new());
        }
        let base_list: Vec<_> = if let Some(lib_id) = filter.library_id {
            self.series.list_by_library(lib_id).await?
        } else {
            self.series.list_all().await?
        }
        .into_iter()
        .filter(|s| bound_ids.contains(&s.id.0))
        .collect();

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

        let mut matched_series = Vec::new();
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
            matched_series.push(s);
        }

        let ids_with_progress: Vec<SeriesId> = matched_series
            .iter()
            .filter(|s| progress_by_series.contains_key(&s.id))
            .map(|s| s.id)
            .collect();
        let chapters_by_series = self.load_sorted_chapters_map(&ids_with_progress).await?;

        let mut results = Vec::with_capacity(matched_series.len());
        for s in matched_series {
            let is_fav = fav_set.contains(&s.id);
            let s_prog = progress_by_series
                .get(&s.id)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            let s_chapters = chapters_by_series
                .get(&s.id)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            let summary = Self::build_series_summary_with_chapters(s, s_prog, s_chapters, is_fav);

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

    pub async fn list_pool_series(
        &self,
        user: &User,
        filter: SeriesQueryFilter,
    ) -> DomainResult<Vec<PoolSeriesSummaryDto>> {
        let bound_ids = self
            .bindings
            .list_bound_ids(user.id, MediaKind::Comic)
            .await?;
        let base_list = if let Some(lib_id) = filter.library_id {
            self.series.list_by_library(lib_id).await?
        } else {
            self.series.list_all().await?
        };

        let search_q = filter
            .search
            .as_ref()
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty());

        let mut results: Vec<PoolSeriesSummaryDto> = Vec::new();
        for s in base_list {
            if s.online_tracking().is_some() {
                continue;
            }
            if let Some(status) = filter.status
                && s.status != status
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
            let bound = bound_ids.contains(&s.id.0);
            let summary = Self::build_series_summary_with_chapters(s, &[], &[], false);
            results.push(PoolSeriesSummaryDto {
                series: summary,
                bound,
            });
        }

        let sort_by = filter.sort_by.as_deref().unwrap_or("updated_at");
        let asc = filter.sort_dir.as_deref() == Some("asc");
        results.sort_by(|a, b| {
            let ord = match sort_by {
                "title" => natural_cmp(&a.series.title, &b.series.title),
                "created_at" => a.series.created_at.cmp(&b.series.created_at),
                "chapter_count" => a.series.chapter_count.cmp(&b.series.chapter_count),
                _ => a.series.updated_at.cmp(&b.series.updated_at),
            };
            if asc { ord } else { ord.reverse() }
        });

        Ok(results)
    }

    pub async fn count_bound_series_by_library(
        &self,
        user: &User,
    ) -> DomainResult<HashMap<LibraryId, u64>> {
        let bound_ids = self
            .bindings
            .list_bound_ids(user.id, MediaKind::Comic)
            .await?;
        if bound_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let mut counts: HashMap<LibraryId, u64> = HashMap::new();
        for s in self.series.list_all().await? {
            if bound_ids.contains(&s.id.0) {
                *counts.entry(s.library_id).or_insert(0) += 1;
            }
        }
        Ok(counts)
    }

    pub async fn get_series_detail(
        &self,
        user: &User,
        series_id: SeriesId,
    ) -> DomainResult<SeriesDetailDto> {
        self.require_series_bound(user, series_id).await?;
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
        let online_tracking = series.online_tracking().map(|info| OnlineTrackedSeriesDto {
            source_id: info.source_id,
            comic_url: info.comic_url,
            toc_url: info.toc_url,
        });

        Ok(SeriesDetailDto {
            series,
            cover_url,
            is_favorite,
            shelf_ids,
            completed_chapters,
            continue_reading,
            reader_pref,
            online_tracking,
            chapters: chapter_dtos,
        })
    }

    pub async fn taxonomy_overview(&self, user: &User) -> DomainResult<TaxonomyOverviewDto> {
        let bound_ids = self
            .bindings
            .list_bound_ids(user.id, MediaKind::Comic)
            .await?;
        let all: Vec<_> = if bound_ids.is_empty() {
            Vec::new()
        } else {
            self.series
                .list_all()
                .await?
                .into_iter()
                .filter(|s| bound_ids.contains(&s.id.0))
                .collect()
        };
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
}

use std::collections::{HashMap, HashSet};

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::user::{ComicFitMode, ComicReaderMode, UserId};

use crate::domain::chapter::Chapter;
use crate::domain::id::{ChapterId, ComicBookmarkId, ComicShelfId, SeriesId};

#[derive(Debug, Clone, Copy)]
pub enum BatchReadScope {
    UpToChapter(ChapterId),
    Volume(f64),
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChapterProgress {
    pub user_id: UserId,
    pub series_id: SeriesId,
    pub chapter_id: ChapterId,
    pub current_page: u32,
    pub total_pages: u32,
    pub is_completed: bool,
    pub updated_at: Timestamp,
}

impl ChapterProgress {
    pub fn new(
        user_id: UserId,
        series_id: SeriesId,
        chapter_id: ChapterId,
        current_page: u32,
        total_pages: u32,
        force_completed: Option<bool>,
    ) -> Self {
        let clamped_total = total_pages.max(1);
        let clamped_page = current_page.clamp(1, clamped_total);
        let auto_completed = clamped_page >= clamped_total;
        let is_completed = force_completed.unwrap_or(auto_completed);

        Self {
            user_id,
            series_id,
            chapter_id,
            current_page: if is_completed {
                clamped_total
            } else {
                clamped_page
            },
            total_pages: clamped_total,
            is_completed,
            updated_at: Timestamp::now(),
        }
    }

    pub fn select_batch(chapters: &[Chapter], scope: BatchReadScope) -> Vec<Chapter> {
        match scope {
            BatchReadScope::UpToChapter(target_id) => select_up_to_chapter(chapters, target_id),
            BatchReadScope::Volume(vol) => select_in_volume(chapters, vol),
            BatchReadScope::All => select_all_present(chapters),
        }
    }
}

fn select_up_to_chapter(chapters: &[Chapter], target_id: ChapterId) -> Vec<Chapter> {
    let mut ordered = chapters.to_vec();
    ordered.sort_by(Chapter::cmp_order);
    let Some(target_idx) = ordered.iter().position(|c| c.id == target_id) else {
        return Vec::new();
    };
    ordered
        .into_iter()
        .take(target_idx + 1)
        .filter(|c| !c.is_missing)
        .collect()
}

fn select_in_volume(chapters: &[Chapter], volume_number: f64) -> Vec<Chapter> {
    chapters
        .iter()
        .filter(|ch| {
            !ch.is_missing
                && ch
                    .volume_number
                    .is_some_and(|v| (v - volume_number).abs() < 1e-6)
        })
        .cloned()
        .collect()
}

fn select_all_present(chapters: &[Chapter]) -> Vec<Chapter> {
    chapters
        .iter()
        .filter(|ch| !ch.is_missing)
        .cloned()
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeriesReaderPref {
    pub user_id: UserId,
    pub series_id: SeriesId,
    pub reader_mode: ComicReaderMode,
    pub fit_mode: ComicFitMode,
    pub offset_cover_page: bool,
    pub webtoon_width: u16,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContinueReadingPointer {
    pub series_id: SeriesId,
    pub chapter_id: ChapterId,
    pub chapter_number: String,
    pub chapter_title: String,
    pub current_page: u32,
    pub total_pages: u32,
    pub is_resuming_mid_chapter: bool,
    pub last_read_at: Timestamp,
}

#[derive(Debug, Clone, Default)]
pub struct SeriesReadingSummary {
    pub completed_chapters: u32,
    pub continue_reading: Option<ContinueReadingPointer>,
}

impl SeriesReadingSummary {
    pub fn compute(
        series_id: SeriesId,
        chapter_count: u32,
        ordered_chapters: &[Chapter],
        progress_list: &[ChapterProgress],
        read_status_filter: Option<&str>,
    ) -> Option<Self> {
        let (completed_chapters, continue_reading) = if progress_list.is_empty() {
            (0, None)
        } else {
            let done = count_completed_active_chapters(ordered_chapters, progress_list);
            let ptr = resolve_continue_pointer(series_id, ordered_chapters, progress_list);
            (done, ptr)
        };
        if !matches_read_status(
            read_status_filter,
            completed_chapters,
            chapter_count,
            continue_reading.is_some(),
        ) {
            return None;
        }
        Some(Self {
            completed_chapters,
            continue_reading,
        })
    }
}

fn count_completed_active_chapters(
    ordered_chapters: &[Chapter],
    progress_list: &[ChapterProgress],
) -> u32 {
    let active_ids: HashSet<ChapterId> = ordered_chapters
        .iter()
        .filter(|c| !c.is_missing)
        .map(|c| c.id)
        .collect();
    progress_list
        .iter()
        .filter(|p| p.is_completed && active_ids.contains(&p.chapter_id))
        .count() as u32
}

fn matches_read_status(
    read_status: Option<&str>,
    completed_chapters: u32,
    chapter_count: u32,
    has_continue_reading: bool,
) -> bool {
    let Some(rs) = read_status else {
        return true;
    };
    match rs {
        "unread" => completed_chapters == 0 && !has_continue_reading,
        "completed" => chapter_count > 0 && completed_chapters >= chapter_count,
        "in_progress" => has_continue_reading && completed_chapters < chapter_count,
        _ => true,
    }
}

fn resolve_continue_pointer(
    series_id: SeriesId,
    ordered_chapters: &[Chapter],
    progress_list: &[ChapterProgress],
) -> Option<ContinueReadingPointer> {
    let available: Vec<&Chapter> = ordered_chapters.iter().filter(|c| !c.is_missing).collect();
    if available.is_empty() || progress_list.is_empty() {
        return None;
    }

    let progress_map: HashMap<ChapterId, &ChapterProgress> =
        progress_list.iter().map(|p| (p.chapter_id, p)).collect();

    if let Some((ch, prog)) = find_latest_in_progress(&available, &progress_map) {
        return Some(build_continue_pointer(
            series_id,
            ch,
            prog.current_page,
            true,
            prog.updated_at,
        ));
    }

    let (idx, last_ts) = find_latest_completed_index(&available, &progress_map)?;
    let next_ch = find_first_unread_after(&available, idx, &progress_map)?;
    Some(build_continue_pointer(
        series_id, next_ch, 1, false, last_ts,
    ))
}

fn find_latest_in_progress<'a>(
    available: &[&'a Chapter],
    progress_map: &HashMap<ChapterId, &'a ChapterProgress>,
) -> Option<(&'a Chapter, &'a ChapterProgress)> {
    let mut best: Option<(&Chapter, &ChapterProgress)> = None;
    for &ch in available {
        if let Some(&prog) = progress_map.get(&ch.id)
            && !prog.is_completed
            && prog.current_page >= 1
        {
            match best {
                None => best = Some((ch, prog)),
                Some((_, prev)) if prog.updated_at >= prev.updated_at => best = Some((ch, prog)),
                _ => {}
            }
        }
    }
    best
}

fn find_latest_completed_index(
    available: &[&Chapter],
    progress_map: &HashMap<ChapterId, &ChapterProgress>,
) -> Option<(usize, Timestamp)> {
    let mut latest: Option<(usize, Timestamp)> = None;
    for (idx, ch) in available.iter().enumerate() {
        if let Some(prog) = progress_map.get(&ch.id)
            && prog.is_completed
        {
            let ts = match latest {
                Some((_, prev_ts)) => prev_ts.max(prog.updated_at),
                None => prog.updated_at,
            };
            latest = Some((idx, ts));
        }
    }
    latest
}

fn find_first_unread_after<'a>(
    available: &[&'a Chapter],
    completed_idx: usize,
    progress_map: &HashMap<ChapterId, &ChapterProgress>,
) -> Option<&'a Chapter> {
    available
        .iter()
        .skip(completed_idx + 1)
        .copied()
        .find(|ch| !progress_map.get(&ch.id).is_some_and(|p| p.is_completed))
}

fn build_continue_pointer(
    series_id: SeriesId,
    ch: &Chapter,
    current_page: u32,
    is_resuming_mid_chapter: bool,
    last_read_at: Timestamp,
) -> ContinueReadingPointer {
    ContinueReadingPointer {
        series_id,
        chapter_id: ch.id,
        chapter_number: ch.number_label(),
        chapter_title: ch.title.clone(),
        current_page,
        total_pages: ch.page_count.max(1),
        is_resuming_mid_chapter,
        last_read_at,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FavoriteSeries {
    pub user_id: UserId,
    pub series_id: SeriesId,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComicShelf {
    pub id: ComicShelfId,
    pub user_id: UserId,
    pub name: String,
    pub description: String,
    pub series_ids: Vec<SeriesId>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl ComicShelf {
    pub fn new(
        user_id: UserId,
        name: String,
        description: String,
        series_ids: Vec<SeriesId>,
    ) -> DomainResult<Self> {
        let clean_name = name.trim();
        if clean_name.is_empty() {
            return Err(DomainError::Validation("shelf name cannot be empty".into()));
        }
        let now = Timestamp::now();
        Ok(Self {
            id: ComicShelfId::new(),
            user_id,
            name: clean_name.to_owned(),
            description: description.trim().to_owned(),
            series_ids: dedupe_series_ids(series_ids),
            created_at: now,
            updated_at: now,
        })
    }

    pub fn update(
        &mut self,
        name: String,
        description: Option<String>,
        series_ids: Option<Vec<SeriesId>>,
        now: Timestamp,
    ) -> DomainResult<()> {
        let clean_name = name.trim();
        if clean_name.is_empty() {
            return Err(DomainError::Validation("shelf name cannot be empty".into()));
        }
        self.name = clean_name.to_owned();
        if let Some(desc) = description {
            self.description = desc.trim().to_owned();
        }
        if let Some(ids) = series_ids {
            self.series_ids = dedupe_series_ids(ids);
        }
        self.updated_at = now;
        Ok(())
    }

    pub fn set_contains(
        &mut self,
        series_id: SeriesId,
        should_contain: bool,
        now: Timestamp,
    ) -> bool {
        let currently_contains = self.series_ids.contains(&series_id);
        if should_contain && !currently_contains {
            self.series_ids.push(series_id);
            self.updated_at = now;
            true
        } else if !should_contain && currently_contains {
            self.series_ids.retain(|&s| s != series_id);
            self.updated_at = now;
            true
        } else {
            false
        }
    }
}

fn dedupe_series_ids(series_ids: Vec<SeriesId>) -> Vec<SeriesId> {
    let mut deduped = Vec::with_capacity(series_ids.len());
    for sid in series_ids {
        if !deduped.contains(&sid) {
            deduped.push(sid);
        }
    }
    deduped
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComicBookmark {
    pub id: ComicBookmarkId,
    pub user_id: UserId,
    pub series_id: SeriesId,
    pub chapter_id: ChapterId,
    pub page_number: u32,
    pub title: String,
    pub created_at: Timestamp,
}

impl ComicBookmark {
    pub fn new(
        user_id: UserId,
        series_id: SeriesId,
        chapter_id: ChapterId,
        page_number: u32,
        title: String,
    ) -> Self {
        Self {
            id: ComicBookmarkId::new(),
            user_id,
            series_id,
            chapter_id,
            page_number: page_number.max(1),
            title: title.trim().to_owned(),
            created_at: Timestamp::now(),
        }
    }
}

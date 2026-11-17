use std::collections::HashMap;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::user::{ComicFitMode, ComicReaderMode, UserId};

use crate::domain::chapter::Chapter;
use crate::domain::id::{ChapterId, ComicShelfId, SeriesId};

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

pub fn compute_continue_reading(
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

    let mut best_in_progress: Option<(&Chapter, &ChapterProgress)> = None;
    for ch in &available {
        if let Some(prog) = progress_map.get(&ch.id)
            && !prog.is_completed
            && prog.current_page >= 1
        {
            match best_in_progress {
                None => best_in_progress = Some((ch, prog)),
                Some((_, prev)) if prog.updated_at >= prev.updated_at => {
                    best_in_progress = Some((ch, prog));
                }
                _ => {}
            }
        }
    }

    if let Some((ch, prog)) = best_in_progress {
        return Some(ContinueReadingPointer {
            series_id,
            chapter_id: ch.id,
            chapter_number: format_chapter_number(ch.number),
            chapter_title: ch.title.clone(),
            current_page: prog.current_page,
            total_pages: ch.page_count.max(1),
            is_resuming_mid_chapter: true,
            last_read_at: prog.updated_at,
        });
    }

    let mut latest_completed_idx: Option<(usize, Timestamp)> = None;
    for (idx, ch) in available.iter().enumerate() {
        if let Some(prog) = progress_map.get(&ch.id)
            && prog.is_completed
        {
            let ts = match latest_completed_idx {
                Some((_, prev_ts)) => prev_ts.max(prog.updated_at),
                None => prog.updated_at,
            };
            latest_completed_idx = Some((idx, ts));
        }
    }

    if let Some((idx, last_ts)) = latest_completed_idx {
        for next_ch in available.iter().skip(idx + 1) {
            let is_done = progress_map
                .get(&next_ch.id)
                .is_some_and(|p| p.is_completed);
            if !is_done {
                return Some(ContinueReadingPointer {
                    series_id,
                    chapter_id: next_ch.id,
                    chapter_number: format_chapter_number(next_ch.number),
                    chapter_title: next_ch.title.clone(),
                    current_page: 1,
                    total_pages: next_ch.page_count.max(1),
                    is_resuming_mid_chapter: false,
                    last_read_at: last_ts,
                });
            }
        }
    }

    None
}

pub fn format_chapter_number(num: f64) -> String {
    if (num.fract()).abs() < 1e-6 {
        format!("{}", num.round() as i64)
    } else {
        let s = format!("{num:.2}");
        s.trim_end_matches('0').trim_end_matches('.').to_owned()
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
    pub fn new(user_id: UserId, name: String, description: String) -> DomainResult<Self> {
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
            series_ids: Vec::new(),
            created_at: now,
            updated_at: now,
        })
    }
}

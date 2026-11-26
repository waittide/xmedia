use std::collections::{HashMap, HashSet};

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::LibraryId;
use xmedia_formats::common::natural_cmp;

use crate::domain::id::SeriesId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SeriesStatus {
    #[default]
    Ongoing,
    Completed,
    Hiatus,
}

impl AsRef<str> for SeriesStatus {
    fn as_ref(&self) -> &str {
        match self {
            Self::Ongoing => "ongoing",
            Self::Completed => "completed",
            Self::Hiatus => "hiatus",
        }
    }
}

impl std::fmt::Display for SeriesStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_ref())
    }
}

impl std::str::FromStr for SeriesStatus {
    type Err = std::convert::Infallible;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(match value.trim().to_ascii_lowercase().as_str() {
            "completed" | "ended" => Self::Completed,
            "hiatus" | "paused" => Self::Hiatus,
            _ => Self::Ongoing,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ReadingDirection {
    #[default]
    Rtl,
    Ltr,
    Webtoon,
}

impl AsRef<str> for ReadingDirection {
    fn as_ref(&self) -> &str {
        match self {
            Self::Rtl => "rtl",
            Self::Ltr => "ltr",
            Self::Webtoon => "webtoon",
        }
    }
}

impl std::fmt::Display for ReadingDirection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_ref())
    }
}

impl std::str::FromStr for ReadingDirection {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "rtl" => Ok(Self::Rtl),
            "ltr" => Ok(Self::Ltr),
            "webtoon" | "vertical" | "scroll" => Ok(Self::Webtoon),
            _ => Err(()),
        }
    }
}

impl From<xmedia_formats::comic::ComicReadingDirection> for ReadingDirection {
    fn from(dir: xmedia_formats::comic::ComicReadingDirection) -> Self {
        match dir {
            xmedia_formats::comic::ComicReadingDirection::Rtl => Self::Rtl,
            xmedia_formats::comic::ComicReadingDirection::Ltr => Self::Ltr,
            xmedia_formats::comic::ComicReadingDirection::Webtoon => Self::Webtoon,
        }
    }
}

impl From<ReadingDirection> for xmedia_formats::comic::ComicReadingDirection {
    fn from(dir: ReadingDirection) -> Self {
        match dir {
            ReadingDirection::Rtl => Self::Rtl,
            ReadingDirection::Ltr => Self::Ltr,
            ReadingDirection::Webtoon => Self::Webtoon,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ComicInfoMetadata {
    pub series: Option<String>,
    pub series_group: Option<String>,
    pub title: Option<String>,
    pub number: Option<f64>,
    pub volume: Option<f64>,
    pub summary: Option<String>,
    pub writers: Vec<String>,
    pub pencillers: Vec<String>,
    pub publisher: Option<String>,
    pub genres: Vec<String>,
    pub tags: Vec<String>,
    pub age_rating: Option<String>,
    pub language_iso: Option<String>,
    pub year: Option<i32>,
    pub reading_direction: Option<ReadingDirection>,
    pub page_count: Option<u32>,
}

impl From<xmedia_formats::comic::comicinfo::ComicInfoMeta> for ComicInfoMetadata {
    fn from(ci: xmedia_formats::comic::comicinfo::ComicInfoMeta) -> Self {
        let mut pencillers = ci.pencillers;
        for artist in ci.cover_artists {
            if !pencillers
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(&artist))
            {
                pencillers.push(artist);
            }
        }
        Self {
            series: ci.series,
            series_group: ci.series_group,
            title: ci.title,
            number: ci.number,
            volume: ci.volume,
            summary: ci.summary,
            writers: ci.writers,
            pencillers,
            publisher: ci.publisher,
            genres: ci.genres,
            tags: ci.tags,
            age_rating: ci.age_rating,
            language_iso: ci.language_iso,
            year: ci.year,
            reading_direction: ci.reading_direction.map(ReadingDirection::from),
            page_count: ci.page_count,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OnlineTrackedSeriesMeta {
    pub source_id: Uuid,
    pub comic_url: String,
    #[serde(default)]
    pub toc_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SeriesSource {
    Local { source_dir: String },
    OnlineTracked(OnlineTrackedSeriesMeta),
}

impl SeriesSource {
    pub fn online_tracking(&self) -> Option<&OnlineTrackedSeriesMeta> {
        match self {
            Self::OnlineTracked(meta) => Some(meta),
            Self::Local { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Series {
    pub id: SeriesId,
    pub library_id: LibraryId,
    pub title: String,
    pub sort_title: String,
    pub alt_titles: Vec<String>,
    pub summary: String,
    pub writers: Vec<String>,
    pub pencillers: Vec<String>,
    pub publisher: Option<String>,
    pub status: SeriesStatus,
    pub reading_direction: ReadingDirection,
    pub age_rating: Option<String>,
    pub genres: Vec<String>,
    pub tags: Vec<String>,
    pub language: Option<String>,
    pub year: Option<i32>,
    pub cover_path: Option<String>,
    pub cover_is_custom: bool,
    pub chapter_count: u32,
    pub source: SeriesSource,
    pub metadata_locked: bool,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SeriesMetadataPatch {
    pub title: Option<String>,
    pub sort_title: Option<String>,
    pub alt_titles: Option<Vec<String>>,
    pub summary: Option<String>,
    pub writers: Option<Vec<String>>,
    pub pencillers: Option<Vec<String>>,
    pub publisher: Option<Option<String>>,
    pub status: Option<SeriesStatus>,
    pub reading_direction: Option<ReadingDirection>,
    pub age_rating: Option<Option<String>>,
    pub genres: Option<Vec<String>>,
    pub tags: Option<Vec<String>>,
    pub language: Option<Option<String>>,
    pub year: Option<Option<i32>>,
    pub metadata_locked: Option<bool>,
}

#[derive(Debug, Clone)]
pub enum CoverUpdate<'a> {
    Custom(String),
    Scanned(String),
    Fallback(String),
    ReconcileRemoved {
        removed_cover: Option<&'a str>,
        fallback_cover: Option<String>,
    },
}

#[derive(Debug, Clone, Default)]
pub struct SeriesCriteria<'a> {
    pub allowed_ids: Option<&'a HashSet<SeriesId>>,
    pub favorites: Option<&'a HashSet<SeriesId>>,
    pub favorites_only: bool,
    pub status: Option<SeriesStatus>,
    pub writer: Option<&'a str>,
    pub penciller: Option<&'a str>,
    pub tag: Option<&'a str>,
    pub genre: Option<&'a str>,
    pub search: Option<&'a str>,
}

#[derive(Debug, Clone, Default)]
pub struct SeriesTaxonomyBuckets {
    pub genres: Vec<(String, u32)>,
    pub tags: Vec<(String, u32)>,
    pub writers: Vec<(String, u32)>,
    pub pencillers: Vec<(String, u32)>,
    pub publishers: Vec<(String, u32)>,
}

impl Series {
    pub fn new(library_id: LibraryId, title: String, source: SeriesSource) -> DomainResult<Self> {
        let trimmed = title.trim();
        if trimmed.is_empty() {
            return Err(DomainError::Validation(
                "series title cannot be empty".into(),
            ));
        }
        let now = Timestamp::now();
        Ok(Self {
            id: SeriesId::new(),
            library_id,
            title: trimmed.to_owned(),
            sort_title: trimmed.to_owned(),
            alt_titles: Vec::new(),
            summary: String::new(),
            writers: Vec::new(),
            pencillers: Vec::new(),
            publisher: None,
            status: SeriesStatus::Ongoing,
            reading_direction: ReadingDirection::Rtl,
            age_rating: None,
            genres: Vec::new(),
            tags: Vec::new(),
            language: None,
            year: None,
            cover_path: None,
            cover_is_custom: false,
            chapter_count: 0,
            source,
            metadata_locked: false,
            created_at: now,
            updated_at: now,
        })
    }

    pub fn patch_metadata(&mut self, patch: SeriesMetadataPatch) -> DomainResult<()> {
        self.apply_patch(patch)
    }

    pub fn apply_comic_info(&mut self, ci: &ComicInfoMetadata) -> bool {
        self.merge_comic_info(ci)
    }

    pub fn apply_cover(&mut self, update: CoverUpdate<'_>, now: Timestamp) -> bool {
        match update {
            CoverUpdate::Custom(path) => {
                self.set_custom_cover(path, now);
                true
            }
            CoverUpdate::Scanned(path) => self.set_scanned_cover(path, now),
            CoverUpdate::Fallback(path) => self.set_fallback_cover_if_empty(path, now),
            CoverUpdate::ReconcileRemoved {
                removed_cover,
                fallback_cover,
            } => self.reconcile_cover_after_chapter_removed(removed_cover, fallback_cover, now),
        }
    }

    pub fn sync_chapter_count(&mut self, active_count: u32, now: Timestamp) -> bool {
        if self.chapter_count == active_count {
            return false;
        }
        self.chapter_count = active_count;
        self.updated_at = now;
        true
    }

    pub fn matches_criteria(&self, criteria: &SeriesCriteria<'_>) -> bool {
        if let Some(allowed) = criteria.allowed_ids
            && !allowed.contains(&self.id)
        {
            return false;
        }
        if criteria.favorites_only && !criteria.favorites.is_some_and(|f| f.contains(&self.id)) {
            return false;
        }
        if let Some(status) = criteria.status
            && self.status != status
        {
            return false;
        }
        if !matches_facet(&self.writers, criteria.writer)
            || !matches_facet(&self.pencillers, criteria.penciller)
            || !matches_facet(&self.tags, criteria.tag)
            || !matches_facet(&self.genres, criteria.genre)
        {
            return false;
        }
        if let Some(q) = normalize_search_query(criteria.search)
            && !self.matches_search_needle(&q)
        {
            return false;
        }
        true
    }

    pub fn tally_taxonomies<'a>(
        series: impl IntoIterator<Item = &'a Self>,
    ) -> SeriesTaxonomyBuckets {
        let mut genres = HashMap::new();
        let mut tags = HashMap::new();
        let mut writers = HashMap::new();
        let mut pencillers = HashMap::new();
        let mut publishers = HashMap::new();

        for s in series {
            accumulate_bucket(&mut genres, &s.genres);
            accumulate_bucket(&mut tags, &s.tags);
            accumulate_bucket(&mut writers, &s.writers);
            accumulate_bucket(&mut pencillers, &s.pencillers);
            if let Some(pub_name) = &s.publisher {
                accumulate_single(&mut publishers, pub_name);
            }
        }

        SeriesTaxonomyBuckets {
            genres: finalize_bucket(genres),
            tags: finalize_bucket(tags),
            writers: finalize_bucket(writers),
            pencillers: finalize_bucket(pencillers),
            publishers: finalize_bucket(publishers),
        }
    }

    fn matches_search_needle(&self, needle_lower: &str) -> bool {
        contains_lower(&self.title, needle_lower)
            || any_contains_lower(&self.alt_titles, needle_lower)
            || contains_lower(&self.summary, needle_lower)
            || any_contains_lower(&self.writers, needle_lower)
            || any_contains_lower(&self.pencillers, needle_lower)
            || any_contains_lower(&self.tags, needle_lower)
            || any_contains_lower(&self.genres, needle_lower)
    }

    fn set_custom_cover(&mut self, cover_path: String, now: Timestamp) {
        self.cover_path = Some(cover_path);
        self.cover_is_custom = true;
        self.updated_at = now;
    }

    fn set_scanned_cover(&mut self, cover_path: String, now: Timestamp) -> bool {
        if self.cover_is_custom || self.cover_path.as_deref() == Some(cover_path.as_str()) {
            return false;
        }
        self.cover_path = Some(cover_path);
        self.updated_at = now;
        true
    }

    fn set_fallback_cover_if_empty(&mut self, cover_path: String, now: Timestamp) -> bool {
        if self.cover_is_custom || self.cover_path.is_some() {
            return false;
        }
        self.cover_path = Some(cover_path);
        self.updated_at = now;
        true
    }

    fn reconcile_cover_after_chapter_removed(
        &mut self,
        removed_cover: Option<&str>,
        fallback_cover: Option<String>,
        now: Timestamp,
    ) -> bool {
        if !self.cover_is_custom
            && removed_cover.is_some()
            && self.cover_path.as_deref() == removed_cover
        {
            self.cover_path = fallback_cover;
            self.updated_at = now;
            return true;
        }
        false
    }

    fn apply_patch(&mut self, patch: SeriesMetadataPatch) -> DomainResult<()> {
        if let Some(title) = patch.title {
            let trimmed = title.trim();
            if trimmed.is_empty() {
                return Err(DomainError::Validation(
                    "series title cannot be empty".into(),
                ));
            }
            self.title = trimmed.to_owned();
        }
        if let Some(sort_title) = patch.sort_title {
            let trimmed = sort_title.trim();
            self.sort_title = if trimmed.is_empty() {
                self.title.clone()
            } else {
                trimmed.to_owned()
            };
        }
        if let Some(alt_titles) = patch.alt_titles {
            self.alt_titles = normalize_list(alt_titles);
        }
        if let Some(summary) = patch.summary {
            self.summary = summary.trim().to_owned();
        }
        if let Some(writers) = patch.writers {
            self.writers = normalize_list(writers);
        }
        if let Some(pencillers) = patch.pencillers {
            self.pencillers = normalize_list(pencillers);
        }
        if let Some(publisher) = patch.publisher {
            self.publisher = publisher.and_then(non_empty);
        }
        if let Some(status) = patch.status {
            self.status = status;
        }
        if let Some(dir) = patch.reading_direction {
            self.reading_direction = dir;
        }
        if let Some(age) = patch.age_rating {
            self.age_rating = age.and_then(non_empty);
        }
        if let Some(genres) = patch.genres {
            self.genres = normalize_list(genres);
        }
        if let Some(tags) = patch.tags {
            self.tags = normalize_list(tags);
        }
        if let Some(lang) = patch.language {
            self.language = lang.and_then(non_empty);
        }
        if let Some(year) = patch.year {
            if let Some(y) = year
                && !(1000..=3000).contains(&y)
            {
                return Err(DomainError::Validation(
                    "year must be between 1000 and 3000".into(),
                ));
            }
            self.year = year;
        }
        if let Some(locked) = patch.metadata_locked {
            self.metadata_locked = locked;
        }
        self.updated_at = Timestamp::now();
        Ok(())
    }

    fn merge_comic_info(&mut self, ci: &ComicInfoMetadata) -> bool {
        if let Some(series_title) = &ci.series
            && !series_title.trim().is_empty()
        {
            self.title = series_title.trim().to_owned();
            self.sort_title = self.title.clone();
        }
        if let Some(group) = &ci.series_group {
            let trimmed = group.trim();
            if !trimmed.is_empty() && !trimmed.eq_ignore_ascii_case(&self.title) {
                let mut combined = self.alt_titles.clone();
                combined.push(trimmed.to_owned());
                self.alt_titles = normalize_list(combined);
            }
        }
        if let Some(summary) = &ci.summary
            && self.summary.is_empty()
            && !summary.trim().is_empty()
        {
            self.summary = summary.trim().to_owned();
        }
        merge_into_list(&mut self.writers, &ci.writers);
        merge_into_list(&mut self.pencillers, &ci.pencillers);
        merge_into_list(&mut self.genres, &ci.genres);
        merge_into_list(&mut self.tags, &ci.tags);
        fill_opt_str(&mut self.publisher, ci.publisher.as_deref());
        fill_opt_str(&mut self.age_rating, ci.age_rating.as_deref());
        fill_opt_str(&mut self.language, ci.language_iso.as_deref());
        if self.year.is_none() {
            self.year = ci.year;
        }
        if let Some(dir) = ci.reading_direction {
            self.reading_direction = dir;
        }
        true
    }
}

fn merge_into_list(target: &mut Vec<String>, additions: &[String]) {
    if additions.is_empty() {
        return;
    }
    let mut combined = target.clone();
    combined.extend(additions.iter().cloned());
    *target = normalize_list(combined);
}

fn fill_opt_str(target: &mut Option<String>, candidate: Option<&str>) {
    if target.is_none()
        && let Some(val) = candidate.map(str::trim).filter(|s| !s.is_empty())
    {
        *target = Some(val.to_owned());
    }
}

fn matches_facet(values: &[String], expected: Option<&str>) -> bool {
    let Some(needle) = expected.map(str::trim).filter(|s| !s.is_empty()) else {
        return true;
    };
    values.iter().any(|item| item.eq_ignore_ascii_case(needle))
}

fn normalize_search_query(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_lowercase)
}

fn contains_lower(text: &str, needle_lower: &str) -> bool {
    text.to_lowercase().contains(needle_lower)
}

fn any_contains_lower(items: &[String], needle_lower: &str) -> bool {
    items.iter().any(|item| contains_lower(item, needle_lower))
}

fn accumulate_bucket(map: &mut HashMap<String, (String, u32)>, items: &[String]) {
    for item in items {
        accumulate_single(map, item);
    }
}

fn accumulate_single(map: &mut HashMap<String, (String, u32)>, raw: &str) {
    let clean = raw.trim();
    if clean.is_empty() {
        return;
    }
    let entry = map
        .entry(clean.to_lowercase())
        .or_insert_with(|| (clean.to_owned(), 0));
    entry.1 += 1;
}

fn finalize_bucket(map: HashMap<String, (String, u32)>) -> Vec<(String, u32)> {
    let mut items: Vec<(String, u32)> = map.into_values().collect();
    items.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| natural_cmp(&a.0, &b.0)));
    items
}

fn non_empty(value: String) -> Option<String> {
    let t = value.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_owned())
    }
}

pub(super) fn normalize_list(items: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for item in items {
        let t = item.trim();
        if !t.is_empty() && !out.iter().any(|existing| existing.eq_ignore_ascii_case(t)) {
            out.push(t.to_owned());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_cover_is_preserved_across_scans_and_chapter_removals() {
        let now = Timestamp::now();
        let mut s = Series::new(
            LibraryId::new(),
            "Test".into(),
            SeriesSource::Local {
                source_dir: "/comics/test".into(),
            },
        )
        .expect("valid series");
        assert!(!s.cover_is_custom);
        assert!(s.apply_cover(CoverUpdate::Scanned("scanned_1.jpg".into()), now));
        assert_eq!(s.cover_path.as_deref(), Some("scanned_1.jpg"));

        s.apply_cover(CoverUpdate::Custom("custom_cov.jpg".into()), now);
        assert!(s.cover_is_custom);
        assert!(!s.apply_cover(CoverUpdate::Scanned("scanned_2.jpg".into()), now));
        assert!(!s.apply_cover(CoverUpdate::Fallback("fallback.jpg".into()), now));
        s.apply_cover(
            CoverUpdate::ReconcileRemoved {
                removed_cover: Some("custom_cov.jpg"),
                fallback_cover: Some("other.jpg".into()),
            },
            now,
        );
        assert_eq!(s.cover_path.as_deref(), Some("custom_cov.jpg"));
    }
}

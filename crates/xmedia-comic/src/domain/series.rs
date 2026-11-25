use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::LibraryId;

use crate::domain::id::SeriesId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SeriesStatus {
    #[default]
    Ongoing,
    Completed,
    Hiatus,
}

impl SeriesStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ongoing => "ongoing",
            Self::Completed => "completed",
            Self::Hiatus => "hiatus",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value.to_ascii_lowercase().as_str() {
            "completed" | "ended" => Self::Completed,
            "hiatus" | "paused" => Self::Hiatus,
            _ => Self::Ongoing,
        }
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

impl ReadingDirection {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rtl => "rtl",
            Self::Ltr => "ltr",
            Self::Webtoon => "webtoon",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value.to_ascii_lowercase().as_str() {
            "ltr" => Self::Ltr,
            "webtoon" | "vertical" => Self::Webtoon,
            _ => Self::Rtl,
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
    pub fn kind_str(&self) -> &'static str {
        match self {
            Self::Local { .. } => "local",
            Self::OnlineTracked(_) => "online_tracked",
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

impl Series {
    pub fn new_local(
        library_id: LibraryId,
        title: String,
        source_dir: String,
    ) -> DomainResult<Self> {
        Self::with_source(library_id, title, SeriesSource::Local { source_dir })
    }

    pub fn new_online_tracked(
        library_id: LibraryId,
        title: String,
        meta: OnlineTrackedSeriesMeta,
    ) -> DomainResult<Self> {
        Self::with_source(library_id, title, SeriesSource::OnlineTracked(meta))
    }

    fn with_source(
        library_id: LibraryId,
        title: String,
        source: SeriesSource,
    ) -> DomainResult<Self> {
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

    pub fn is_online_tracked(&self) -> bool {
        matches!(self.source, SeriesSource::OnlineTracked(_))
    }

    pub fn online_tracking(&self) -> Option<&OnlineTrackedSeriesMeta> {
        match &self.source {
            SeriesSource::OnlineTracked(meta) => Some(meta),
            SeriesSource::Local { .. } => None,
        }
    }

    pub fn local_source_dir(&self) -> Option<&str> {
        match &self.source {
            SeriesSource::Local { source_dir } => Some(source_dir.as_str()),
            SeriesSource::OnlineTracked(_) => None,
        }
    }

    pub fn set_custom_cover(&mut self, cover_path: String, now: Timestamp) {
        self.cover_path = Some(cover_path);
        self.cover_is_custom = true;
        self.updated_at = now;
    }

    pub fn set_scanned_cover(&mut self, cover_path: String, now: Timestamp) -> bool {
        if self.cover_is_custom || self.cover_path.as_deref() == Some(cover_path.as_str()) {
            return false;
        }
        self.cover_path = Some(cover_path);
        self.updated_at = now;
        true
    }

    pub fn set_fallback_cover_if_empty(&mut self, cover_path: String, now: Timestamp) -> bool {
        if self.cover_is_custom || self.cover_path.is_some() {
            return false;
        }
        self.cover_path = Some(cover_path);
        self.updated_at = now;
        true
    }

    pub fn sync_chapter_count(&mut self, active_count: u32, now: Timestamp) -> bool {
        if self.chapter_count == active_count {
            return false;
        }
        self.chapter_count = active_count;
        self.updated_at = now;
        true
    }

    pub fn reconcile_cover_after_chapter_removed(
        &mut self,
        removed_cover: Option<&str>,
        fallback_cover: Option<String>,
        now: Timestamp,
    ) {
        if !self.cover_is_custom
            && removed_cover.is_some()
            && self.cover_path.as_deref() == removed_cover
        {
            self.cover_path = fallback_cover;
            self.updated_at = now;
        }
    }

    pub fn apply_patch(&mut self, patch: SeriesMetadataPatch) -> DomainResult<()> {
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
        if let Some(reading_direction) = patch.reading_direction {
            self.reading_direction = reading_direction;
        }
        if let Some(age_rating) = patch.age_rating {
            self.age_rating = age_rating.and_then(non_empty);
        }
        if let Some(genres) = patch.genres {
            self.genres = normalize_list(genres);
        }
        if let Some(tags) = patch.tags {
            self.tags = normalize_list(tags);
        }
        if let Some(language) = patch.language {
            self.language = language.and_then(non_empty);
        }
        if let Some(year) = patch.year {
            self.year = year;
        }
        if let Some(locked) = patch.metadata_locked {
            self.metadata_locked = locked;
        } else {
            self.metadata_locked = true;
        }
        self.updated_at = Timestamp::now();
        Ok(())
    }
}

fn non_empty(value: String) -> Option<String> {
    let t = value.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_owned())
    }
}

pub fn normalize_list(items: Vec<String>) -> Vec<String> {
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
        let mut s = Series::new_local(LibraryId::new(), "Test".into(), "/comics/test".into())
            .expect("valid series");
        assert!(!s.cover_is_custom);
        assert!(s.set_scanned_cover("scanned_1.jpg".into(), now));
        assert_eq!(s.cover_path.as_deref(), Some("scanned_1.jpg"));

        s.set_custom_cover("custom_cov.jpg".into(), now);
        assert!(s.cover_is_custom);
        assert!(!s.set_scanned_cover("scanned_2.jpg".into(), now));
        assert!(!s.set_fallback_cover_if_empty("fallback.jpg".into(), now));
        s.reconcile_cover_after_chapter_removed(
            Some("custom_cov.jpg"),
            Some("other.jpg".into()),
            now,
        );
        assert_eq!(s.cover_path.as_deref(), Some("custom_cov.jpg"));
    }
}

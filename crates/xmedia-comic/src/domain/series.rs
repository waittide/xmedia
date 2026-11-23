use jiff::Timestamp;
use serde::{Deserialize, Serialize};
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

pub const ONLINE_TRACKED_SERIES_DIR_PREFIX: &str = "online_tracked:";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OnlineTrackedSeriesRef {
    pub source_id: uuid::Uuid,
    pub comic_url: String,
    #[serde(default)]
    pub toc_url: String,
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
    pub chapter_count: u32,
    pub source_dir: String,
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
    pub fn new(library_id: LibraryId, title: String, source_dir: String) -> DomainResult<Self> {
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
            chapter_count: 0,
            source_dir,
            metadata_locked: false,
            created_at: now,
            updated_at: now,
        })
    }

    pub fn encode_online_tracking(info: &OnlineTrackedSeriesRef) -> DomainResult<String> {
        let json = serde_json::to_string(info)
            .map_err(|e| DomainError::Validation(format!("serialize tracked series ref: {e}")))?;
        Ok(format!("{ONLINE_TRACKED_SERIES_DIR_PREFIX}{json}"))
    }

    pub fn online_tracking(&self) -> Option<OnlineTrackedSeriesRef> {
        let raw = self
            .source_dir
            .strip_prefix(ONLINE_TRACKED_SERIES_DIR_PREFIX)?;
        serde_json::from_str::<OnlineTrackedSeriesRef>(raw).ok()
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

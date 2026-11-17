use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::user::UserId;

use crate::domain::book::BookId;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReadingLocation {
    pub progress_percent: f64,
    pub locator: String,
    pub chapter_index: Option<u32>,
    pub page_number: Option<u32>,
    pub scroll_ratio: Option<f64>,
}

impl ReadingLocation {
    pub fn new(
        progress_percent: f64,
        locator: impl Into<String>,
        chapter_index: Option<u32>,
        page_number: Option<u32>,
        scroll_ratio: Option<f64>,
    ) -> DomainResult<Self> {
        if !progress_percent.is_finite() || !(0.0..=100.0).contains(&progress_percent) {
            return Err(DomainError::Validation(
                "progress_percent must be between 0.0 and 100.0".into(),
            ));
        }
        if let Some(ratio) = scroll_ratio
            && (!ratio.is_finite() || !(0.0..=1.0).contains(&ratio))
        {
            return Err(DomainError::Validation(
                "scroll_ratio must be between 0.0 and 1.0".into(),
            ));
        }
        Ok(Self {
            progress_percent,
            locator: locator.into(),
            chapter_index,
            page_number,
            scroll_ratio,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReadingProgress {
    pub user_id: UserId,
    pub book_id: BookId,
    pub location: ReadingLocation,
    pub last_read_at: Timestamp,
}

impl ReadingProgress {
    pub fn new(user_id: UserId, book_id: BookId, location: ReadingLocation) -> Self {
        Self {
            user_id,
            book_id,
            location,
            last_read_at: Timestamp::now(),
        }
    }

    pub fn update_location(&mut self, location: ReadingLocation) {
        self.location = location;
        self.last_read_at = Timestamp::now();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BookmarkId(pub Uuid);

impl BookmarkId {
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for BookmarkId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for BookmarkId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bookmark {
    pub id: BookmarkId,
    pub user_id: UserId,
    pub book_id: BookId,
    pub title: String,
    pub note: Option<String>,
    pub location: ReadingLocation,
    pub created_at: Timestamp,
}

impl Bookmark {
    pub fn new(
        user_id: UserId,
        book_id: BookId,
        title: impl Into<String>,
        note: Option<String>,
        location: ReadingLocation,
    ) -> DomainResult<Self> {
        let title = title.into().trim().to_string();
        if title.is_empty() {
            return Err(DomainError::Validation(
                "bookmark title cannot be empty".into(),
            ));
        }
        let note = note.and_then(|n| {
            let trimmed = n.trim().to_string();
            (!trimmed.is_empty()).then_some(trimmed)
        });
        Ok(Self {
            id: BookmarkId::new(),
            user_id,
            book_id,
            title,
            note,
            location,
            created_at: Timestamp::now(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ShelfId(pub Uuid);

impl ShelfId {
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for ShelfId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ShelfId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shelf {
    pub id: ShelfId,
    pub user_id: UserId,
    pub name: String,
    pub is_favorite: bool,
    pub book_ids: Vec<BookId>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl Shelf {
    pub fn new(user_id: UserId, name: impl Into<String>, is_favorite: bool) -> DomainResult<Self> {
        let name = name.into().trim().to_string();
        if name.is_empty() || name.len() > 80 {
            return Err(DomainError::Validation(
                "shelf name must be between 1 and 80 characters".into(),
            ));
        }
        let now = Timestamp::now();
        Ok(Self {
            id: ShelfId::new(),
            user_id,
            name,
            is_favorite,
            book_ids: Vec::new(),
            created_at: now,
            updated_at: now,
        })
    }

    pub fn rename(&mut self, name: impl Into<String>) -> DomainResult<()> {
        let trimmed = name.into().trim().to_string();
        if trimmed.is_empty() || trimmed.len() > 80 {
            return Err(DomainError::Validation(
                "shelf name must be between 1 and 80 characters".into(),
            ));
        }
        self.name = trimmed;
        self.updated_at = Timestamp::now();
        Ok(())
    }

    pub fn add_book(&mut self, book_id: BookId) {
        if !self.book_ids.contains(&book_id) {
            self.book_ids.push(book_id);
            self.updated_at = Timestamp::now();
        }
    }

    pub fn remove_book(&mut self, book_id: BookId) {
        let before = self.book_ids.len();
        self.book_ids.retain(|&id| id != book_id);
        if self.book_ids.len() != before {
            self.updated_at = Timestamp::now();
        }
    }
}

use std::collections::{BTreeSet, HashSet};

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::LibraryId;
pub use xmedia_core::domain::media::{PagedResult, SortOrder};
use xmedia_core::domain::user::UserId;

xmedia_core::define_uuid_id!(BookId);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BookFormat {
    Epub,
    Txt,
    Pdf,
    Mobi,
    Azw3,
    Fb2,
}

impl BookFormat {
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext
            .trim()
            .trim_start_matches('.')
            .to_ascii_lowercase()
            .as_str()
        {
            "epub" => Some(Self::Epub),
            "txt" => Some(Self::Txt),
            "pdf" => Some(Self::Pdf),
            "mobi" | "prc" => Some(Self::Mobi),
            "azw3" | "azw" | "kf8" => Some(Self::Azw3),
            "fb2" => Some(Self::Fb2),
            _ => None,
        }
    }

    pub fn parse(value: &str) -> DomainResult<Self> {
        Self::from_extension(value).ok_or_else(|| {
            DomainError::UnsupportedFormat(format!("unsupported book format: {value}"))
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Epub => "epub",
            Self::Txt => "txt",
            Self::Pdf => "pdf",
            Self::Mobi => "mobi",
            Self::Azw3 => "azw3",
            Self::Fb2 => "fb2",
        }
    }

    pub fn mime_type(self) -> &'static str {
        match self {
            Self::Epub => "application/epub+zip",
            Self::Txt => "text/plain; charset=utf-8",
            Self::Pdf => "application/pdf",
            Self::Mobi => "application/x-mobipocket-ebook",
            Self::Azw3 => "application/vnd.amazon.ebook",
            Self::Fb2 => "application/x-fictionbook+xml",
        }
    }

    pub fn badge_label(self) -> &'static str {
        match self {
            Self::Epub => "EPUB",
            Self::Txt => "TXT",
            Self::Pdf => "PDF",
            Self::Mobi => "MOBI",
            Self::Azw3 => "AZW3",
            Self::Fb2 => "FB2",
        }
    }

    pub fn is_text_chapter_based(self) -> bool {
        matches!(self, Self::Txt | Self::Mobi | Self::Azw3 | Self::Fb2)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BookSource {
    MountedFile {
        relative_path: String,
        file_mtime: i64,
    },
    ManagedUpload {
        storage_key: String,
    },
    OnlineTracked {
        storage_key: String,
        source_id: uuid::Uuid,
        book_url: String,
        toc_url: String,
        #[serde(default)]
        last_chapter: Option<String>,
        #[serde(default)]
        cached_chapters: u32,
        #[serde(default)]
        total_chapters: u32,
        #[serde(default)]
        epub_dirty: bool,
    },
}

impl BookSource {
    pub fn is_managed(&self) -> bool {
        matches!(
            self,
            Self::ManagedUpload { .. } | Self::OnlineTracked { .. }
        )
    }

    pub fn storage_key(&self) -> Option<&str> {
        match self {
            Self::ManagedUpload { storage_key } | Self::OnlineTracked { storage_key, .. } => {
                Some(storage_key.as_str())
            }
            Self::MountedFile { .. } => None,
        }
    }

    pub fn is_online_tracked(&self) -> bool {
        matches!(self, Self::OnlineTracked { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BookMetadata {
    pub title: String,
    pub authors: Vec<String>,
    pub description: Option<String>,
    pub isbn: Option<String>,
    pub language: Option<String>,
    pub publisher: Option<String>,
    pub published_at: Option<String>,
    pub tags: Vec<String>,
    #[serde(default)]
    pub series: Option<String>,
    #[serde(default)]
    pub series_index: Option<f64>,
    #[serde(default)]
    pub rating: Option<f64>,
    pub page_count: Option<u32>,
    pub chapter_count: Option<u32>,
}

impl BookMetadata {
    pub fn new(title: impl Into<String>) -> DomainResult<Self> {
        let title = Self::normalize_title(title.into())?;
        Ok(Self {
            title,
            authors: Vec::new(),
            description: None,
            isbn: None,
            language: None,
            publisher: None,
            published_at: None,
            tags: Vec::new(),
            series: None,
            series_index: None,
            rating: None,
            page_count: None,
            chapter_count: None,
        })
    }

    pub fn normalize_title(raw: String) -> DomainResult<String> {
        let trimmed = raw.trim().to_string();
        if trimmed.is_empty() {
            return Err(DomainError::Validation("book title cannot be empty".into()));
        }
        Ok(trimmed)
    }

    pub fn normalize_list(items: impl IntoIterator<Item = String>) -> Vec<String> {
        let mut seen = BTreeSet::new();
        let mut out = Vec::new();
        for item in items {
            let trimmed = item.trim().to_string();
            if trimmed.is_empty() {
                continue;
            }
            let key = trimmed.to_lowercase();
            if seen.insert(key) {
                out.push(trimmed);
            }
        }
        out
    }

    pub fn normalize_optional(val: Option<String>) -> Option<String> {
        val.and_then(|s| {
            let t = s.trim().to_string();
            (!t.is_empty()).then_some(t)
        })
    }

    pub fn normalized(mut self) -> DomainResult<Self> {
        self.title = Self::normalize_title(self.title)?;
        self.authors = Self::normalize_list(self.authors);
        self.tags = Self::normalize_list(self.tags);
        self.description = Self::normalize_optional(self.description);
        self.isbn = Self::normalize_optional(self.isbn);
        self.language = Self::normalize_optional(self.language);
        self.publisher = Self::normalize_optional(self.publisher);
        self.published_at = Self::normalize_optional(self.published_at);
        self.series = Self::normalize_optional(self.series);
        if let Some(idx) = self.series_index
            && (!idx.is_finite() || idx < 0.0)
        {
            return Err(DomainError::Validation(
                "series_index must be a non-negative number".into(),
            ));
        }
        if let Some(rating) = self.rating
            && (!rating.is_finite() || !(0.0..=10.0).contains(&rating))
        {
            return Err(DomainError::Validation(
                "rating must be between 0.0 and 10.0".into(),
            ));
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Book {
    pub id: BookId,
    pub library_id: LibraryId,
    pub format: BookFormat,
    pub source: BookSource,
    pub file_size: u64,
    pub file_hash: String,
    pub cover_key: Option<String>,
    pub metadata: BookMetadata,
    pub is_missing: bool,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl Book {
    pub fn new(
        library_id: LibraryId,
        format: BookFormat,
        source: BookSource,
        file_size: u64,
        file_hash: impl Into<String>,
        cover_key: Option<String>,
        metadata: BookMetadata,
    ) -> DomainResult<Self> {
        let metadata = metadata.normalized()?;
        let now = Timestamp::now();
        Ok(Self {
            id: BookId::new(),
            library_id,
            format,
            source,
            file_size,
            file_hash: file_hash.into(),
            cover_key,
            metadata,
            is_missing: false,
            created_at: now,
            updated_at: now,
        })
    }

    pub fn update_metadata(&mut self, metadata: BookMetadata) -> DomainResult<()> {
        self.metadata = metadata.normalized()?;
        self.updated_at = Timestamp::now();
        Ok(())
    }

    pub fn set_tags(&mut self, tags: Vec<String>) {
        self.metadata.tags = BookMetadata::normalize_list(tags);
        self.updated_at = Timestamp::now();
    }

    pub fn set_cover(&mut self, cover_key: Option<String>) {
        self.cover_key = cover_key;
        self.updated_at = Timestamp::now();
    }

    pub fn mark_missing(&mut self) {
        if !self.is_missing {
            self.is_missing = true;
            self.updated_at = Timestamp::now();
        }
    }

    pub fn mark_present(&mut self, file_size: u64, file_hash: String, new_mtime: Option<i64>) {
        self.is_missing = false;
        self.file_size = file_size;
        self.file_hash = file_hash;
        if let (Some(mtime), BookSource::MountedFile { file_mtime, .. }) =
            (new_mtime, &mut self.source)
        {
            *file_mtime = mtime;
        }
        self.updated_at = Timestamp::now();
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BookSortField {
    #[default]
    CreatedAt,
    UpdatedAt,
    Title,
    FileSize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TagMatchMode {
    #[default]
    Any,
    All,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookQuery {
    pub keyword: Option<String>,
    pub authors: Vec<String>,
    pub tags: Vec<String>,
    pub tag_mode: TagMatchMode,
    pub formats: Vec<BookFormat>,
    pub library_ids: Vec<LibraryId>,
    pub favorite_for_user: Option<UserId>,
    pub shelf_book_ids: Option<Vec<BookId>>,
    pub allowed_book_ids: Option<HashSet<BookId>>,
    pub exclude_online_tracked: bool,
    pub include_missing: bool,
    pub sort_by: BookSortField,
    pub sort_order: SortOrder,
    pub page: u32,
    pub page_size: u32,
}

impl Default for BookQuery {
    fn default() -> Self {
        Self {
            keyword: None,
            authors: Vec::new(),
            tags: Vec::new(),
            tag_mode: TagMatchMode::Any,
            formats: Vec::new(),
            library_ids: Vec::new(),
            favorite_for_user: None,
            shelf_book_ids: None,
            allowed_book_ids: None,
            exclude_online_tracked: false,
            include_missing: false,
            sort_by: BookSortField::CreatedAt,
            sort_order: SortOrder::Desc,
            page: 1,
            page_size: 24,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FacetCount {
    pub name: String,
    pub count: u64,
}

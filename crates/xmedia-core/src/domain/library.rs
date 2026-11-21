#[cfg(feature = "server")]
use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::domain::error::{DomainError, DomainResult};

#[cfg(feature = "server")]
crate::define_uuid_id!(LibraryId);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
    Book,
    Comic,
    Music,
    Video,
}

impl MediaKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Book => "book",
            Self::Comic => "comic",
            Self::Music => "music",
            Self::Video => "video",
        }
    }

    pub fn default_managed_name(self) -> &'static str {
        match self {
            Self::Book => "Uploaded Books",
            Self::Comic => "Uploaded Comics",
            Self::Music => "Uploaded Music",
            Self::Video => "Uploaded Videos",
        }
    }

    pub fn parse(raw: &str) -> DomainResult<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "book" | "books" => Ok(Self::Book),
            "comic" | "comics" => Ok(Self::Comic),
            "music" | "audio" => Ok(Self::Music),
            "video" | "videos" => Ok(Self::Video),
            other => Err(DomainError::Validation(format!(
                "unknown media kind: {other}"
            ))),
        }
    }
}

#[cfg(feature = "server")]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LibraryKind {
    Managed,
    LocalMount { root_path: String },
}

#[cfg(feature = "server")]
impl LibraryKind {
    pub fn is_managed(&self) -> bool {
        matches!(self, Self::Managed)
    }

    pub fn root_path(&self) -> Option<&str> {
        match self {
            Self::Managed => None,
            Self::LocalMount { root_path } => Some(root_path.as_str()),
        }
    }
}

#[cfg(feature = "server")]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ScanStatus {
    #[default]
    Idle,
    Scanning {
        scanned: u64,
        #[serde(default)]
        total: Option<u64>,
    },
    Completed {
        scanned: u64,
        added: u64,
        updated: u64,
        missing: u64,
    },
    Failed {
        message: String,
    },
}

#[cfg(feature = "server")]
impl ScanStatus {
    pub fn is_scanning(&self) -> bool {
        matches!(self, Self::Scanning { .. })
    }
}

#[cfg(feature = "server")]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Library {
    pub id: LibraryId,
    pub name: String,
    pub media_kind: MediaKind,
    pub kind: LibraryKind,
    pub scan_status: ScanStatus,
    pub last_scanned_at: Option<Timestamp>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[cfg(feature = "server")]
impl Library {
    pub fn new_managed(media_kind: MediaKind, name: impl Into<String>) -> DomainResult<Self> {
        let name = Self::validate_name(name.into())?;
        let now = Timestamp::now();
        Ok(Self {
            id: LibraryId::new(),
            name,
            media_kind,
            kind: LibraryKind::Managed,
            scan_status: ScanStatus::Idle,
            last_scanned_at: None,
            created_at: now,
            updated_at: now,
        })
    }

    pub fn new_local_mount(
        media_kind: MediaKind,
        name: impl Into<String>,
        root_path: impl Into<String>,
    ) -> DomainResult<Self> {
        let name = Self::validate_name(name.into())?;
        let root_path = root_path.into().trim().to_string();
        if root_path.is_empty() {
            return Err(DomainError::Validation(
                "library root path cannot be empty".into(),
            ));
        }
        let now = Timestamp::now();
        Ok(Self {
            id: LibraryId::new(),
            name,
            media_kind,
            kind: LibraryKind::LocalMount { root_path },
            scan_status: ScanStatus::Idle,
            last_scanned_at: None,
            created_at: now,
            updated_at: now,
        })
    }

    fn validate_name(raw: String) -> DomainResult<String> {
        let trimmed = raw.trim().to_string();
        let char_len = trimmed.chars().count();
        if char_len == 0 || char_len > 128 {
            return Err(DomainError::Validation(
                "library name must be between 1 and 128 characters".into(),
            ));
        }
        Ok(trimmed)
    }

    pub fn rename(&mut self, name: impl Into<String>) -> DomainResult<()> {
        self.name = Self::validate_name(name.into())?;
        self.updated_at = Timestamp::now();
        Ok(())
    }

    pub fn mark_scanning(&mut self, scanned: u64, total: Option<u64>) {
        self.scan_status = ScanStatus::Scanning { scanned, total };
        self.updated_at = Timestamp::now();
    }

    pub fn mark_scan_completed(&mut self, scanned: u64, added: u64, updated: u64, missing: u64) {
        let now = Timestamp::now();
        self.scan_status = ScanStatus::Completed {
            scanned,
            added,
            updated,
            missing,
        };
        self.last_scanned_at = Some(now);
        self.updated_at = now;
    }

    pub fn mark_scan_failed(&mut self, message: impl Into<String>) {
        self.scan_status = ScanStatus::Failed {
            message: message.into(),
        };
        self.updated_at = Timestamp::now();
    }
}

use std::cmp::Ordering;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::LibraryId;

use crate::domain::id::{ChapterId, SeriesId};
use crate::domain::natural::{build_chapter_sort_key, natural_cmp};
use crate::domain::series::normalize_list;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ChapterKind {
    Volume,
    #[default]
    Chapter,
    Special,
}

impl ChapterKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Volume => "volume",
            Self::Chapter => "chapter",
            Self::Special => "special",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value.to_ascii_lowercase().as_str() {
            "volume" | "vol" => Self::Volume,
            "special" | "extra" | "omake" => Self::Special,
            _ => Self::Chapter,
        }
    }

    pub fn sort_rank(self) -> u8 {
        match self {
            Self::Volume => 0,
            Self::Chapter => 1,
            Self::Special => 2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ChapterSourceKind {
    #[default]
    ArchiveFile,
    ImageDirectory,
    OnlineTracked,
}

impl ChapterSourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ArchiveFile => "archive_file",
            Self::ImageDirectory => "image_directory",
            Self::OnlineTracked => "online_tracked",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "image_directory" => Self::ImageDirectory,
            "online_tracked" => Self::OnlineTracked,
            _ => Self::ArchiveFile,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OnlineTrackedChapterRef {
    pub source_id: uuid::Uuid,
    pub comic_url: String,
    #[serde(default)]
    pub toc_url: String,
    pub chapter_url: String,
    #[serde(default)]
    pub storage_key: Option<String>,
    #[serde(default)]
    pub cached: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chapter {
    pub id: ChapterId,
    pub series_id: SeriesId,
    pub library_id: LibraryId,
    pub kind: ChapterKind,
    pub number: f64,
    pub volume_number: Option<f64>,
    pub title: String,
    pub sort_key: String,
    pub page_count: u32,
    pub source_kind: ChapterSourceKind,
    pub source_path: String,
    pub file_size: u64,
    pub file_hash: String,
    pub cover_path: Option<String>,
    pub summary: String,
    pub writers: Vec<String>,
    pub pencillers: Vec<String>,
    pub is_missing: bool,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChapterMetadataPatch {
    pub kind: Option<ChapterKind>,
    pub number: Option<f64>,
    pub volume_number: Option<Option<f64>>,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub writers: Option<Vec<String>>,
    pub pencillers: Option<Vec<String>>,
}

impl Chapter {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        series_id: SeriesId,
        library_id: LibraryId,
        kind: ChapterKind,
        number: f64,
        volume_number: Option<f64>,
        title: String,
        page_count: u32,
        source_kind: ChapterSourceKind,
        source_path: String,
        file_size: u64,
        file_hash: String,
    ) -> DomainResult<Self> {
        if !number.is_finite() || number < 0.0 {
            return Err(DomainError::Validation(
                "chapter number must be a non-negative finite number".into(),
            ));
        }
        if let Some(vol) = volume_number
            && (!vol.is_finite() || vol < 0.0)
        {
            return Err(DomainError::Validation(
                "volume number must be a non-negative finite number".into(),
            ));
        }
        let clean_title = title.trim().to_owned();
        let sort_key =
            build_chapter_sort_key(kind.sort_rank(), volume_number, number, &clean_title);
        let now = Timestamp::now();

        Ok(Self {
            id: ChapterId::new(),
            series_id,
            library_id,
            kind,
            number,
            volume_number,
            title: clean_title,
            sort_key,
            page_count,
            source_kind,
            source_path,
            file_size,
            file_hash,
            cover_path: None,
            summary: String::new(),
            writers: Vec::new(),
            pencillers: Vec::new(),
            is_missing: false,
            created_at: now,
            updated_at: now,
        })
    }

    pub fn is_online_tracked(&self) -> bool {
        self.source_kind == ChapterSourceKind::OnlineTracked
    }

    pub fn online_tracking(&self) -> Option<OnlineTrackedChapterRef> {
        if self.source_kind != ChapterSourceKind::OnlineTracked {
            return None;
        }
        serde_json::from_str::<OnlineTrackedChapterRef>(&self.source_path).ok()
    }

    pub fn set_online_tracking(&mut self, info: &OnlineTrackedChapterRef) -> DomainResult<()> {
        self.source_kind = ChapterSourceKind::OnlineTracked;
        self.source_path = serde_json::to_string(info)
            .map_err(|e| DomainError::Validation(format!("serialize tracked chapter ref: {e}")))?;
        Ok(())
    }

    /// Returns the managed archive path on disk if this chapter owns a local archive file.
    pub fn managed_file_path(&self) -> Option<String> {
        match self.source_kind {
            ChapterSourceKind::ArchiveFile => Some(self.source_path.clone()),
            ChapterSourceKind::ImageDirectory => None,
            ChapterSourceKind::OnlineTracked => {
                self.online_tracking().and_then(|info| info.storage_key)
            }
        }
    }

    /// Returns `(effective_kind, fs_path)` for reading pages/archives when backed by local disk.
    pub fn resolved_local_source(&self) -> Option<(ChapterSourceKind, String)> {
        match self.source_kind {
            ChapterSourceKind::ArchiveFile => {
                Some((ChapterSourceKind::ArchiveFile, self.source_path.clone()))
            }
            ChapterSourceKind::ImageDirectory => {
                Some((ChapterSourceKind::ImageDirectory, self.source_path.clone()))
            }
            ChapterSourceKind::OnlineTracked => {
                let info = self.online_tracking()?;
                if info.cached
                    && let Some(key) = info.storage_key
                {
                    return Some((ChapterSourceKind::ArchiveFile, key));
                }
                None
            }
        }
    }

    pub fn apply_patch(&mut self, patch: ChapterMetadataPatch) -> DomainResult<()> {
        if let Some(kind) = patch.kind {
            self.kind = kind;
        }
        if let Some(number) = patch.number {
            if !number.is_finite() || number < 0.0 {
                return Err(DomainError::Validation(
                    "chapter number must be non-negative".into(),
                ));
            }
            self.number = number;
        }
        if let Some(vol) = patch.volume_number {
            if let Some(v) = vol
                && (!v.is_finite() || v < 0.0)
            {
                return Err(DomainError::Validation(
                    "volume number must be a non-negative finite number".into(),
                ));
            }
            self.volume_number = vol;
        }
        if let Some(title) = patch.title {
            self.title = title.trim().to_owned();
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
        self.sort_key = build_chapter_sort_key(
            self.kind.sort_rank(),
            self.volume_number,
            self.number,
            &self.title,
        );
        self.updated_at = Timestamp::now();
        Ok(())
    }

    pub fn cmp_order(&self, other: &Self) -> Ordering {
        let vol_a = self.volume_number.unwrap_or(0.0).max(0.0);
        let vol_b = other.volume_number.unwrap_or(0.0).max(0.0);
        self.kind
            .sort_rank()
            .cmp(&other.kind.sort_rank())
            .then_with(|| vol_a.total_cmp(&vol_b))
            .then_with(|| self.number.max(0.0).total_cmp(&other.number.max(0.0)))
            .then_with(|| natural_cmp(&self.title, &other.title))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_chapter(vol: Option<f64>, num: f64, title: &str) -> Chapter {
        Chapter::new(
            SeriesId::new(),
            LibraryId::new(),
            ChapterKind::Chapter,
            num,
            vol,
            title.to_string(),
            10,
            ChapterSourceKind::ArchiveFile,
            format!("{title}.cbz"),
            100,
            "hash".into(),
        )
        .unwrap()
    }

    #[test]
    fn cmp_order_satisfies_strict_weak_ordering_and_matches_sort_key() {
        let mut items = vec![
            mk_chapter(Some(2.0), 1.0, "Ch 1"),
            mk_chapter(None, 2.0, "Ch 2"),
            mk_chapter(Some(1.0), 3.0, "Ch 3"),
            mk_chapter(None, 1.0, "Ch 1"),
            mk_chapter(Some(0.0), 1.5, "Ch 1.5"),
        ];

        // Verify antisymmetry and transitivity across all triples
        for a in &items {
            assert_eq!(a.cmp_order(a), Ordering::Equal);
            for b in &items {
                assert_eq!(a.cmp_order(b), b.cmp_order(a).reverse());
                for c in &items {
                    if a.cmp_order(b) == Ordering::Less && b.cmp_order(c) == Ordering::Less {
                        assert_eq!(a.cmp_order(c), Ordering::Less);
                    }
                }
            }
        }

        items.sort_by(Chapter::cmp_order);
        let by_sort_key = {
            let mut cloned = items.clone();
            cloned.sort_by(|a, b| a.sort_key.cmp(&b.sort_key));
            cloned
        };
        let ordered_titles: Vec<&str> = items.iter().map(|c| c.title.as_str()).collect();
        let key_titles: Vec<&str> = by_sort_key.iter().map(|c| c.title.as_str()).collect();
        assert_eq!(ordered_titles, key_titles);
    }
}

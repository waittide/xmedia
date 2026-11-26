use std::cmp::Ordering;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::LibraryId;
use xmedia_formats::comic::filename::{ComicChapterKind, build_chapter_sort_key};
use xmedia_formats::common::natural_cmp;

use crate::domain::id::{ChapterId, SeriesId};
use crate::domain::series::{ComicInfoMetadata, normalize_list};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ChapterKind {
    Volume,
    #[default]
    Chapter,
    Special,
}

impl ChapterKind {
    fn sort_rank(self) -> u8 {
        match self {
            Self::Volume => 0,
            Self::Chapter => 1,
            Self::Special => 2,
        }
    }
}

impl AsRef<str> for ChapterKind {
    fn as_ref(&self) -> &str {
        match self {
            Self::Volume => "volume",
            Self::Chapter => "chapter",
            Self::Special => "special",
        }
    }
}

impl std::fmt::Display for ChapterKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_ref())
    }
}

impl std::str::FromStr for ChapterKind {
    type Err = std::convert::Infallible;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(match value.trim().to_ascii_lowercase().as_str() {
            "volume" | "vol" => Self::Volume,
            "special" | "extra" | "omake" => Self::Special,
            _ => Self::Chapter,
        })
    }
}

impl From<ComicChapterKind> for ChapterKind {
    fn from(kind: ComicChapterKind) -> Self {
        match kind {
            ComicChapterKind::Volume => Self::Volume,
            ComicChapterKind::Chapter => Self::Chapter,
            ComicChapterKind::Special => Self::Special,
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

impl AsRef<str> for ChapterSourceKind {
    fn as_ref(&self) -> &str {
        match self {
            Self::ArchiveFile => "archive_file",
            Self::ImageDirectory => "image_directory",
            Self::OnlineTracked => "online_tracked",
        }
    }
}

impl std::fmt::Display for ChapterSourceKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_ref())
    }
}

impl std::str::FromStr for ChapterSourceKind {
    type Err = std::convert::Infallible;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(match value.trim() {
            "image_directory" => Self::ImageDirectory,
            "online_tracked" => Self::OnlineTracked,
            _ => Self::ArchiveFile,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OnlineTrackedChapterMeta {
    pub source_id: Uuid,
    pub comic_url: String,
    #[serde(default)]
    pub toc_url: String,
    pub chapter_url: String,
    #[serde(default)]
    pub cached_archive_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChapterSource {
    Archive { path: String },
    ImageFolder { path: String },
    OnlineTracked(OnlineTrackedChapterMeta),
}

impl From<&ChapterSource> for ChapterSourceKind {
    fn from(source: &ChapterSource) -> Self {
        match source {
            ChapterSource::Archive { .. } => Self::ArchiveFile,
            ChapterSource::ImageFolder { .. } => Self::ImageDirectory,
            ChapterSource::OnlineTracked(_) => Self::OnlineTracked,
        }
    }
}

impl ChapterSource {
    pub fn online_tracking(&self) -> Option<&OnlineTrackedChapterMeta> {
        match self {
            Self::OnlineTracked(meta) => Some(meta),
            Self::Archive { .. } | Self::ImageFolder { .. } => None,
        }
    }

    pub fn resolved_local(&self) -> Option<(ChapterSourceKind, &str)> {
        match self {
            Self::Archive { path } => Some((ChapterSourceKind::ArchiveFile, path.as_str())),
            Self::ImageFolder { path } => Some((ChapterSourceKind::ImageDirectory, path.as_str())),
            Self::OnlineTracked(meta) => meta
                .cached_archive_path
                .as_deref()
                .map(|path| (ChapterSourceKind::ArchiveFile, path)),
        }
    }
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
    pub source: ChapterSource,
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

#[derive(Debug, Clone)]
pub enum ChapterScanUpdate {
    Missing,
    Present,
    Metrics {
        kind: ChapterKind,
        number: f64,
        volume_number: Option<f64>,
        title: String,
        page_count: u32,
        file_size: u64,
        file_hash: String,
    },
    MigrateLocal {
        series_id: SeriesId,
        source: ChapterSource,
        page_count: u32,
        file_size: u64,
        file_hash: String,
    },
    ReassignSeries(SeriesId),
    Cover(String),
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
        source: ChapterSource,
        file_size: u64,
        file_hash: String,
    ) -> DomainResult<Self> {
        validate_chapter_numbers(number, volume_number)?;
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
            source,
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

    pub fn number_label(&self) -> String {
        format_chapter_number(self.number)
    }

    pub fn patch_metadata(&mut self, patch: ChapterMetadataPatch) -> DomainResult<()> {
        self.apply_patch(patch)
    }

    pub fn apply_comic_info(&mut self, ci: &ComicInfoMetadata) {
        self.merge_comic_info(ci);
    }

    pub fn sync_scan(
        &mut self,
        update: ChapterScanUpdate,
        now: Timestamp,
    ) -> (bool, Option<SeriesId>) {
        match update {
            ChapterScanUpdate::Missing => {
                self.mark_missing(now);
                (true, None)
            }
            ChapterScanUpdate::Present => {
                self.mark_present(now);
                (true, None)
            }
            ChapterScanUpdate::Metrics {
                kind,
                number,
                volume_number,
                title,
                page_count,
                file_size,
                file_hash,
            } => {
                self.update_scanned_metrics(
                    kind,
                    number,
                    volume_number,
                    title,
                    page_count,
                    file_size,
                    file_hash,
                    now,
                );
                (true, None)
            }
            ChapterScanUpdate::MigrateLocal {
                series_id,
                source,
                page_count,
                file_size,
                file_hash,
            } => {
                let prev = self
                    .migrate_local_source(series_id, source, page_count, file_size, file_hash, now);
                (true, prev)
            }
            ChapterScanUpdate::ReassignSeries(series_id) => {
                let prev = self.reassign_series(series_id, now);
                (prev.is_some(), prev)
            }
            ChapterScanUpdate::Cover(cover_path) => (self.set_cover(cover_path), None),
        }
    }

    pub fn cache_online_archive(
        &mut self,
        archive_path: String,
        page_count: u32,
        file_size: u64,
        file_hash: String,
        cover_path: Option<String>,
        now: Timestamp,
    ) -> DomainResult<()> {
        if let Some(cov) = cover_path {
            let _ = self.set_cover(cov);
        }
        self.mark_cached_archive(archive_path, page_count, file_size, file_hash, now)
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

    fn mark_cached_archive(
        &mut self,
        archive_path: String,
        page_count: u32,
        file_size: u64,
        file_hash: String,
        now: Timestamp,
    ) -> DomainResult<()> {
        let ChapterSource::OnlineTracked(ref mut meta) = self.source else {
            return Err(DomainError::Validation(
                "cannot mark non-tracked chapter as cached".into(),
            ));
        };
        meta.cached_archive_path = Some(archive_path);
        self.page_count = page_count;
        self.file_size = file_size;
        self.file_hash = file_hash;
        self.is_missing = false;
        self.updated_at = now;
        Ok(())
    }

    fn mark_missing(&mut self, now: Timestamp) {
        self.is_missing = true;
        self.updated_at = now;
    }

    fn mark_present(&mut self, now: Timestamp) {
        self.is_missing = false;
        self.updated_at = now;
    }

    #[allow(clippy::too_many_arguments)]
    fn update_scanned_metrics(
        &mut self,
        kind: ChapterKind,
        number: f64,
        volume_number: Option<f64>,
        title: String,
        page_count: u32,
        file_size: u64,
        file_hash: String,
        now: Timestamp,
    ) {
        self.is_missing = false;
        self.kind = kind;
        self.number = number;
        self.volume_number = volume_number;
        self.title = title.trim().to_owned();
        self.page_count = page_count;
        self.file_size = file_size;
        self.file_hash = file_hash;
        self.refresh_sort_key();
        self.updated_at = now;
    }

    fn migrate_local_source(
        &mut self,
        series_id: SeriesId,
        source: ChapterSource,
        page_count: u32,
        file_size: u64,
        file_hash: String,
        now: Timestamp,
    ) -> Option<SeriesId> {
        let prev_series = (self.series_id != series_id).then_some(self.series_id);
        self.series_id = series_id;
        self.source = source;
        self.is_missing = false;
        self.page_count = page_count;
        self.file_size = file_size;
        self.file_hash = file_hash;
        self.updated_at = now;
        prev_series
    }

    fn reassign_series(&mut self, series_id: SeriesId, now: Timestamp) -> Option<SeriesId> {
        if self.series_id == series_id {
            None
        } else {
            let prev = self.series_id;
            self.series_id = series_id;
            self.updated_at = now;
            Some(prev)
        }
    }

    fn set_cover(&mut self, cover_path: String) -> bool {
        if self.cover_path.as_deref() == Some(cover_path.as_str()) {
            return false;
        }
        self.cover_path = Some(cover_path);
        true
    }

    fn refresh_sort_key(&mut self) {
        self.sort_key = build_chapter_sort_key(
            self.kind.sort_rank(),
            self.volume_number,
            self.number,
            &self.title,
        );
    }

    fn apply_patch(&mut self, patch: ChapterMetadataPatch) -> DomainResult<()> {
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
            validate_volume_number(vol)?;
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
        self.refresh_sort_key();
        self.updated_at = Timestamp::now();
        Ok(())
    }

    fn merge_comic_info(&mut self, ci: &ComicInfoMetadata) {
        if let Some(title) = &ci.title
            && !title.trim().is_empty()
        {
            self.title = title.trim().to_owned();
        }
        if let Some(num) = ci.number
            && num.is_finite()
            && num >= 0.0
        {
            self.number = num;
        }
        if let Some(vol) = ci.volume
            && vol.is_finite()
            && vol >= 0.0
        {
            self.volume_number = Some(vol);
        }
        if let Some(summary) = &ci.summary
            && !summary.trim().is_empty()
        {
            self.summary = summary.trim().to_owned();
        }
        if !ci.writers.is_empty() {
            self.writers = normalize_list(ci.writers.clone());
        }
        if !ci.pencillers.is_empty() {
            self.pencillers = normalize_list(ci.pencillers.clone());
        }
        self.refresh_sort_key();
    }
}

fn format_chapter_number(num: f64) -> String {
    if (num.fract()).abs() < 1e-6 {
        format!("{}", num.round() as i64)
    } else {
        let s = format!("{num:.2}");
        s.trim_end_matches('0').trim_end_matches('.').to_owned()
    }
}

fn validate_chapter_numbers(number: f64, volume_number: Option<f64>) -> DomainResult<()> {
    if !number.is_finite() || number < 0.0 {
        return Err(DomainError::Validation(
            "chapter number must be a non-negative finite number".into(),
        ));
    }
    validate_volume_number(volume_number)
}

fn validate_volume_number(volume_number: Option<f64>) -> DomainResult<()> {
    if let Some(vol) = volume_number
        && (!vol.is_finite() || vol < 0.0)
    {
        return Err(DomainError::Validation(
            "volume number must be a non-negative finite number".into(),
        ));
    }
    Ok(())
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
            ChapterSource::Archive {
                path: format!("{title}.cbz"),
            },
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

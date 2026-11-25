use std::collections::HashSet;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::LibraryId;
use xmedia_core::domain::media::{ProbedAudioStream, ProbedSubtitleStream};
use xmedia_formats::video::{SubtitleFormatExt, VideoFormatExt, extract_video_resolution};

use crate::domain::id::{ShowId, VideoId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VideoContainer {
    Mp4,
    Mkv,
    Webm,
    Mov,
    Avi,
    Ts,
    Flv,
    M4v,
}

impl VideoContainer {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mp4 => "mp4",
            Self::Mkv => "mkv",
            Self::Webm => "webm",
            Self::Mov => "mov",
            Self::Avi => "avi",
            Self::Ts => "ts",
            Self::Flv => "flv",
            Self::M4v => "m4v",
        }
    }

    pub fn extension(self) -> &'static str {
        self.as_str()
    }

    pub fn mime_type(self) -> &'static str {
        match self {
            Self::Mp4 | Self::M4v => "video/mp4",
            Self::Mkv => "video/x-matroska",
            Self::Webm => "video/webm",
            Self::Mov => "video/quicktime",
            Self::Avi => "video/x-msvideo",
            Self::Ts => "video/mp2t",
            Self::Flv => "video/x-flv",
        }
    }

    pub fn is_browser_native_container(self) -> bool {
        matches!(self, Self::Mp4 | Self::Webm | Self::M4v)
    }

    pub fn parse(raw: &str) -> DomainResult<Self> {
        Self::from_extension(raw).ok_or_else(|| {
            DomainError::UnsupportedFormat(format!("unsupported video container: {raw}"))
        })
    }

    pub fn from_extension(ext: &str) -> Option<Self> {
        match VideoFormatExt::from_extension(ext)? {
            VideoFormatExt::Mp4 => Some(Self::Mp4),
            VideoFormatExt::Mkv => Some(Self::Mkv),
            VideoFormatExt::Webm => Some(Self::Webm),
            VideoFormatExt::Mov => Some(Self::Mov),
            VideoFormatExt::Avi => Some(Self::Avi),
            VideoFormatExt::Ts | VideoFormatExt::M2ts | VideoFormatExt::Mts => Some(Self::Ts),
            VideoFormatExt::Flv => Some(Self::Flv),
            VideoFormatExt::M4v => Some(Self::M4v),
        }
    }
}

pub fn is_subtitle_extension(ext: &str) -> bool {
    SubtitleFormatExt::from_extension(ext).is_some()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SidecarSubtitle {
    pub id: String,
    pub format: String,
    pub language: Option<String>,
    pub label: String,
    pub path_or_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OnlineTrackedVideoSource {
    #[serde(default)]
    pub storage_key: Option<String>,
    pub source_id: Uuid,
    pub video_id: String,
    pub detail_url: String,
    pub play_url: String,
    #[serde(default)]
    pub group_name: Option<String>,
    #[serde(default)]
    pub episode_name: Option<String>,
    #[serde(default)]
    pub cached: bool,
    #[serde(default)]
    pub sidecar_subtitles: Vec<SidecarSubtitle>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum VideoSource {
    Managed {
        storage_key: String,
        #[serde(default)]
        sidecar_subtitles: Vec<SidecarSubtitle>,
    },
    LocalPath {
        relative_path: String,
        file_mtime: i64,
        #[serde(default)]
        sidecar_subtitles: Vec<SidecarSubtitle>,
    },
    OnlineTracked(Box<OnlineTrackedVideoSource>),
}

impl VideoSource {
    pub fn display_path(&self) -> &str {
        match self {
            Self::Managed { storage_key, .. } => storage_key,
            Self::LocalPath { relative_path, .. } => relative_path,
            Self::OnlineTracked(info) => {
                info.storage_key
                    .as_deref()
                    .unwrap_or(if !info.play_url.is_empty() {
                        info.play_url.as_str()
                    } else {
                        info.detail_url.as_str()
                    })
            }
        }
    }

    pub fn is_online_tracked(&self) -> bool {
        matches!(self, Self::OnlineTracked(_))
    }

    pub fn is_cached(&self) -> bool {
        match self {
            Self::Managed { .. } | Self::LocalPath { .. } => true,
            Self::OnlineTracked(info) => info.cached && info.storage_key.is_some(),
        }
    }

    pub fn online_tracking_info(&self) -> Option<&OnlineTrackedVideoSource> {
        match self {
            Self::OnlineTracked(info) => Some(info.as_ref()),
            _ => None,
        }
    }

    pub fn managed_file_keys(&self) -> Vec<&str> {
        let mut keys = Vec::new();
        match self {
            Self::Managed {
                storage_key,
                sidecar_subtitles,
            } => {
                keys.push(storage_key.as_str());
                for sc in sidecar_subtitles {
                    keys.push(sc.path_or_key.as_str());
                }
            }
            Self::OnlineTracked(info) => {
                if let Some(ref key) = info.storage_key {
                    keys.push(key.as_str());
                }
                for sc in &info.sidecar_subtitles {
                    keys.push(sc.path_or_key.as_str());
                }
            }
            Self::LocalPath { .. } => {}
        }
        keys
    }

    pub fn sidecar_subtitles(&self) -> &[SidecarSubtitle] {
        match self {
            Self::Managed {
                sidecar_subtitles, ..
            }
            | Self::LocalPath {
                sidecar_subtitles, ..
            } => sidecar_subtitles,
            Self::OnlineTracked(info) => &info.sidecar_subtitles,
        }
    }

    pub fn sidecar_subtitles_mut(&mut self) -> &mut Vec<SidecarSubtitle> {
        match self {
            Self::Managed {
                sidecar_subtitles, ..
            }
            | Self::LocalPath {
                sidecar_subtitles, ..
            } => sidecar_subtitles,
            Self::OnlineTracked(info) => &mut info.sidecar_subtitles,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ShowSource {
    #[default]
    Local,
    OnlineTracked {
        source_id: Uuid,
        item_id: String,
        detail_url: Option<String>,
    },
}

impl ShowSource {
    pub fn is_online_tracked(&self) -> bool {
        matches!(self, Self::OnlineTracked { .. })
    }

    pub fn online_tracked(&self) -> Option<(Uuid, &str, Option<&str>)> {
        match self {
            Self::Local => None,
            Self::OnlineTracked {
                source_id,
                item_id,
                detail_url,
            } => Some((*source_id, item_id.as_str(), detail_url.as_deref())),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoShow {
    pub id: ShowId,
    pub library_id: LibraryId,
    pub show_key: String,
    #[serde(default)]
    pub source: ShowSource,
    pub title: String,
    pub summary: String,
    pub year: Option<i32>,
    pub genres: Vec<String>,
    pub tags: Vec<String>,
    pub poster_key: Option<String>,
    pub season_count: u32,
    pub episode_count: u32,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl VideoShow {
    pub fn compute_show_key(library_id: LibraryId, title: &str) -> String {
        format!("{}:{}", library_id.0, title.trim().to_lowercase())
    }

    fn compute_tracked_unique_key(source_id: Uuid, item_id: &str) -> String {
        format!("tracked:{source_id}:{}", item_id.trim())
    }

    pub fn new_local(
        library_id: LibraryId,
        title: &str,
        year: Option<i32>,
        genres: Vec<String>,
        poster_key: Option<String>,
        now: Timestamp,
    ) -> Self {
        let clean_title = title.trim().to_string();
        let show_key = Self::compute_show_key(library_id, &clean_title);
        Self {
            id: ShowId::new(),
            library_id,
            show_key,
            source: ShowSource::Local,
            title: clean_title,
            summary: String::new(),
            year,
            genres,
            tags: Vec::new(),
            poster_key,
            season_count: 1,
            episode_count: 0,
            created_at: now,
            updated_at: now,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_online_tracked(
        library_id: LibraryId,
        source_id: Uuid,
        item_id: String,
        detail_url: Option<String>,
        title: String,
        summary: String,
        year: Option<i32>,
        genres: Vec<String>,
        poster_key: Option<String>,
        now: Timestamp,
    ) -> Self {
        let clean_item_id = item_id.trim().to_string();
        let show_key = Self::compute_tracked_unique_key(source_id, &clean_item_id);
        Self {
            id: ShowId::new(),
            library_id,
            show_key,
            source: ShowSource::OnlineTracked {
                source_id,
                item_id: clean_item_id,
                detail_url: detail_url
                    .map(|u| u.trim().to_string())
                    .filter(|u| !u.is_empty()),
            },
            title: title.trim().to_string(),
            summary: summary.trim().to_string(),
            year,
            genres,
            tags: Vec::new(),
            poster_key,
            season_count: 1,
            episode_count: 0,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn is_online_tracked(&self) -> bool {
        self.source.is_online_tracked()
    }

    pub fn online_tracked(&self) -> Option<(Uuid, &str, Option<&str>)> {
        self.source.online_tracked()
    }

    pub fn set_poster_key(&mut self, poster_key: String, now: Timestamp) {
        self.poster_key = Some(poster_key);
        self.updated_at = now;
    }

    pub fn fill_missing_scan_metadata(
        &mut self,
        year: Option<i32>,
        genres: &[String],
        poster_key: Option<&str>,
        now: Timestamp,
    ) -> bool {
        let mut dirty = false;
        if self.year.is_none() && year.is_some() {
            self.year = year;
            dirty = true;
        }
        if self.genres.is_empty() && !genres.is_empty() {
            self.genres = genres.to_vec();
            dirty = true;
        }
        if self.poster_key.is_none()
            && let Some(pk) = poster_key
        {
            self.poster_key = Some(pk.to_string());
            dirty = true;
        }
        if dirty {
            self.updated_at = now;
        }
        dirty
    }

    pub fn fill_missing_online_metadata(
        &mut self,
        summary: Option<&str>,
        year: Option<i32>,
        poster_key: Option<String>,
        detail_url: Option<String>,
        now: Timestamp,
    ) -> bool {
        let mut dirty = false;
        if self.poster_key.is_none() && poster_key.is_some() {
            self.poster_key = poster_key;
            dirty = true;
        }
        if self.summary.is_empty()
            && let Some(intro) = summary.map(str::trim).filter(|s| !s.is_empty())
        {
            self.summary = intro.to_string();
            dirty = true;
        }
        if self.year.is_none() && year.is_some() {
            self.year = year;
            dirty = true;
        }
        if let ShowSource::OnlineTracked {
            detail_url: ref mut existing_detail,
            ..
        } = self.source
            && existing_detail.is_none()
            && let Some(durl) = detail_url
                .map(|u| u.trim().to_string())
                .filter(|u| !u.is_empty())
        {
            *existing_detail = Some(durl);
            dirty = true;
        }
        if dirty {
            self.updated_at = now;
        }
        dirty
    }

    /// Applies a metadata patch to this show, syncing `show_key` on rename for local shows and
    /// normalizing `genres`/`tags`. Returns `Ok(true)` if the show title changed.
    pub fn apply_patch(&mut self, patch: ShowMetadataPatch) -> DomainResult<bool> {
        let mut title_changed = false;
        if let Some(title) = patch.title {
            let trimmed = title.trim();
            if trimmed.is_empty() {
                return Err(DomainError::Validation("show title cannot be empty".into()));
            }
            if self.title != trimmed {
                self.title = trimmed.to_string();
                if !self.source.is_online_tracked() {
                    self.show_key = Self::compute_show_key(self.library_id, &self.title);
                }
                title_changed = true;
            }
        }
        if let Some(summary) = patch.summary {
            self.summary = summary.trim().to_string();
        }
        if let Some(year) = patch.year {
            self.year = year;
        }
        if let Some(genres) = patch.genres {
            self.genres = normalize_string_list(genres);
        }
        if let Some(tags) = patch.tags {
            self.tags = normalize_string_list(tags);
        }
        self.updated_at = Timestamp::now();
        Ok(title_changed)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoItem {
    pub id: VideoId,
    pub library_id: LibraryId,
    pub show_id: Option<ShowId>,
    pub show_title: Option<String>,
    pub season_number: Option<u32>,
    pub episode_number: Option<u32>,
    pub title: String,
    pub summary: String,
    pub year: Option<i32>,
    pub genres: Vec<String>,
    pub tags: Vec<String>,
    pub container: VideoContainer,
    pub file_size: u64,
    pub file_hash: String,
    pub duration_secs: u32,
    pub bit_rate_kbps: Option<u32>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub video_codec: Option<String>,
    pub audio_streams: Vec<ProbedAudioStream>,
    pub subtitle_streams: Vec<ProbedSubtitleStream>,
    pub poster_key: Option<String>,
    pub source: VideoSource,
    pub is_missing: bool,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl VideoItem {
    pub fn can_direct_play_in_browser(&self) -> bool {
        if !self.container.is_browser_native_container() {
            return false;
        }
        let video_ok = match self.video_codec.as_deref() {
            Some(codec) => matches!(
                codec.to_ascii_lowercase().as_str(),
                "h264" | "avc1" | "vp8" | "vp9" | "av1"
            ),
            None => true,
        };
        let audio_ok = self.audio_streams.first().is_none_or(|a| {
            matches!(
                a.codec.to_ascii_lowercase().as_str(),
                "aac" | "mp3" | "opus" | "vorbis" | "flac"
            )
        });
        video_ok && audio_ok
    }

    pub fn resolution_label(&self) -> Option<String> {
        if let Some(h) = self.height {
            return Some(if h >= 2100 {
                "4K".to_string()
            } else if h >= 1400 {
                "1440p".to_string()
            } else if h >= 1000 {
                "1080p".to_string()
            } else if h >= 700 {
                "720p".to_string()
            } else if h >= 460 {
                "480p".to_string()
            } else {
                format!("{h}p")
            });
        }
        extract_video_resolution(self.source.display_path())
    }

    pub fn apply_patch(&mut self, patch: VideoMetadataPatch) -> DomainResult<()> {
        if let Some(title) = patch.title {
            let trimmed = title.trim();
            if trimmed.is_empty() {
                return Err(DomainError::Validation("title cannot be empty".into()));
            }
            self.title = trimmed.to_string();
        }
        if let Some(summary) = patch.summary {
            self.summary = summary.trim().to_string();
        }
        if let Some(year) = patch.year {
            self.year = year;
        }
        if let Some(genres) = patch.genres {
            self.genres = normalize_string_list(genres);
        }
        if let Some(tags) = patch.tags {
            self.tags = normalize_string_list(tags);
        }
        if let Some(season) = patch.season_number {
            self.season_number = season;
        }
        if let Some(ep) = patch.episode_number {
            self.episode_number = ep;
        }
        self.updated_at = Timestamp::now();
        Ok(())
    }

    pub fn set_poster_key(&mut self, poster_key: String, now: Timestamp) {
        self.poster_key = Some(poster_key);
        self.updated_at = now;
    }

    pub fn set_show_title(&mut self, show_title: String, now: Timestamp) {
        self.show_title = Some(show_title);
        self.updated_at = now;
    }

    pub fn mark_missing(&mut self, now: Timestamp) {
        self.is_missing = true;
        self.updated_at = now;
    }

    pub fn restore_present(&mut self, now: Timestamp) {
        self.is_missing = false;
        self.updated_at = now;
    }

    pub fn relocate_local_path(
        &mut self,
        relative_path: String,
        file_mtime: i64,
        sidecar_subtitles: Vec<SidecarSubtitle>,
        file_hash: String,
        now: Timestamp,
    ) {
        self.source = VideoSource::LocalPath {
            relative_path,
            file_mtime,
            sidecar_subtitles,
        };
        self.file_hash = file_hash;
        self.is_missing = false;
        self.updated_at = now;
    }

    #[allow(clippy::too_many_arguments)]
    pub fn mark_online_cached(
        &mut self,
        container: VideoContainer,
        storage_key: String,
        file_size: u64,
        file_hash: String,
        duration_secs: u32,
        bit_rate_kbps: Option<u32>,
        width: Option<u32>,
        height: Option<u32>,
        video_codec: Option<String>,
        audio_streams: Vec<ProbedAudioStream>,
        subtitle_streams: Vec<ProbedSubtitleStream>,
        sidecar_subtitles: Vec<SidecarSubtitle>,
        fallback_poster_key: Option<String>,
        now: Timestamp,
    ) -> DomainResult<()> {
        let VideoSource::OnlineTracked(ref mut tracking_info) = self.source else {
            return Err(DomainError::Validation("该视频不是在线追更资源".into()));
        };
        if self.poster_key.is_none() && fallback_poster_key.is_some() {
            self.poster_key = fallback_poster_key;
        }
        self.container = container;
        self.file_size = file_size;
        self.file_hash = file_hash;
        if duration_secs > 0 {
            self.duration_secs = duration_secs;
        }
        if bit_rate_kbps.is_some() {
            self.bit_rate_kbps = bit_rate_kbps;
        }
        if width.is_some() {
            self.width = width;
        }
        if height.is_some() {
            self.height = height;
        }
        if video_codec.is_some() {
            self.video_codec = video_codec;
        }
        if !audio_streams.is_empty() {
            self.audio_streams = audio_streams;
        }
        if !subtitle_streams.is_empty() {
            self.subtitle_streams = subtitle_streams;
        }
        tracking_info.storage_key = Some(storage_key);
        tracking_info.cached = true;
        tracking_info.sidecar_subtitles = sidecar_subtitles;
        self.updated_at = now;
        Ok(())
    }
}

pub fn normalize_string_list(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for raw in values {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        let key = trimmed.to_lowercase();
        if seen.insert(key) {
            out.push(trimmed.to_string());
        }
    }
    out
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct VideoMetadataPatch {
    pub title: Option<String>,
    pub summary: Option<String>,
    pub year: Option<Option<i32>>,
    pub genres: Option<Vec<String>>,
    pub tags: Option<Vec<String>>,
    pub season_number: Option<Option<u32>>,
    pub episode_number: Option<Option<u32>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ShowMetadataPatch {
    pub title: Option<String>,
    pub summary: Option<String>,
    pub year: Option<Option<i32>>,
    pub genres: Option<Vec<String>>,
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoKindFilter {
    #[default]
    All,
    Standalone,
    Episodes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoSortBy {
    #[default]
    CreatedAt,
    Title,
    Year,
    Duration,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct VideoQueryFilter {
    pub library_id: Option<LibraryId>,
    pub show_id: Option<ShowId>,
    #[serde(default)]
    pub kind: VideoKindFilter,
    pub genre: Option<String>,
    pub tag: Option<String>,
    pub year: Option<i32>,
    pub search: Option<String>,
    #[serde(default)]
    pub sort_by: VideoSortBy,
    #[serde(default)]
    pub sort_desc: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ShowQueryFilter {
    pub library_id: Option<LibraryId>,
    pub genre: Option<String>,
    pub tag: Option<String>,
    pub search: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_show(library_id: LibraryId) -> VideoShow {
        let now = Timestamp::now();
        VideoShow {
            id: ShowId::new(),
            library_id,
            show_key: VideoShow::compute_show_key(library_id, "Frieren"),
            source: ShowSource::Local,
            title: "Frieren".into(),
            summary: String::new(),
            year: Some(2023),
            genres: vec!["Anime".into()],
            tags: vec![],
            poster_key: None,
            season_count: 1,
            episode_count: 12,
            created_at: now,
            updated_at: now,
        }
    }

    fn sample_video(library_id: LibraryId) -> VideoItem {
        let now = Timestamp::now();
        VideoItem {
            id: VideoId::new(),
            library_id,
            show_id: None,
            show_title: None,
            season_number: None,
            episode_number: None,
            title: "Interstellar".into(),
            summary: String::new(),
            year: Some(2014),
            genres: vec![],
            tags: vec![],
            container: VideoContainer::Mp4,
            file_size: 1024,
            file_hash: "abc".into(),
            duration_secs: 600,
            bit_rate_kbps: None,
            width: Some(1920),
            height: Some(1080),
            video_codec: Some("h264".into()),
            audio_streams: vec![],
            subtitle_streams: vec![],
            poster_key: None,
            source: VideoSource::Managed {
                storage_key: "test.mp4".into(),
                sidecar_subtitles: vec![],
            },
            is_missing: false,
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn show_apply_patch_syncs_show_key_and_normalizes_genres_and_tags() {
        let lib_id = LibraryId::new();
        let mut show = sample_show(lib_id);

        let changed = show
            .apply_patch(ShowMetadataPatch {
                title: Some("  Frieren: Beyond Journey's End  ".into()),
                summary: Some("  An elf's journey.  ".into()),
                year: Some(Some(2024)),
                genres: Some(vec![
                    " Fantasy ".into(),
                    "fantasy".into(),
                    "".into(),
                    "   ".into(),
                    "Adventure".into(),
                    "ADVENTURE".into(),
                ]),
                tags: Some(vec![
                    " magic ".into(),
                    "Magic".into(),
                    "slice of life".into(),
                    "".into(),
                ]),
            })
            .unwrap();

        assert!(changed);
        assert_eq!(show.title, "Frieren: Beyond Journey's End");
        assert_eq!(
            show.show_key,
            VideoShow::compute_show_key(lib_id, "Frieren: Beyond Journey's End")
        );
        assert_eq!(show.summary, "An elf's journey.");
        assert_eq!(show.year, Some(2024));
        assert_eq!(show.genres, vec!["Fantasy", "Adventure"]);
        assert_eq!(show.tags, vec!["magic", "slice of life"]);

        // Re-applying the same title reports no title change
        let unchanged = show
            .apply_patch(ShowMetadataPatch {
                title: Some("Frieren: Beyond Journey's End".into()),
                ..Default::default()
            })
            .unwrap();
        assert!(!unchanged);

        // Blank title is rejected
        let err = show.apply_patch(ShowMetadataPatch {
            title: Some("   ".into()),
            ..Default::default()
        });
        assert!(matches!(err, Err(DomainError::Validation(_))));
    }

    #[test]
    fn video_apply_patch_normalizes_lists_and_rejects_blank_title() {
        let lib_id = LibraryId::new();
        let mut video = sample_video(lib_id);

        video
            .apply_patch(VideoMetadataPatch {
                title: Some("  Interstellar (IMAX)  ".into()),
                summary: Some("  Space epic ".into()),
                year: Some(Some(2014)),
                genres: Some(vec![" Sci-Fi ".into(), "sci-fi".into(), "Drama".into()]),
                tags: Some(vec![
                    "  space ".into(),
                    "".into(),
                    "SPACE".into(),
                    "4K".into(),
                ]),
                season_number: Some(Some(1)),
                episode_number: Some(Some(2)),
            })
            .unwrap();

        assert_eq!(video.title, "Interstellar (IMAX)");
        assert_eq!(video.summary, "Space epic");
        assert_eq!(video.genres, vec!["Sci-Fi", "Drama"]);
        assert_eq!(video.tags, vec!["space", "4K"]);
        assert_eq!(video.season_number, Some(1));
        assert_eq!(video.episode_number, Some(2));

        let err = video.apply_patch(VideoMetadataPatch {
            title: Some(" ".into()),
            ..Default::default()
        });
        assert!(matches!(err, Err(DomainError::Validation(_))));
    }

    #[test]
    fn online_tracked_show_uses_typed_show_source_without_key_prefix_parsing() {
        let lib_id = LibraryId::new();
        let src_id = Uuid::now_v7();
        let now = Timestamp::now();
        let mut show = VideoShow::new_online_tracked(
            lib_id,
            src_id,
            "vod-42".into(),
            Some("https://example.com/vod/42".into()),
            "Original Show".into(),
            String::new(),
            None,
            vec![],
            None,
            now,
        );

        assert!(show.is_online_tracked());
        assert_eq!(
            show.online_tracked(),
            Some((src_id, "vod-42", Some("https://example.com/vod/42")))
        );

        let initial_key = show.show_key.clone();
        let title_changed = show
            .apply_patch(ShowMetadataPatch {
                title: Some("Renamed Online Show".into()),
                ..Default::default()
            })
            .unwrap();
        assert!(title_changed);
        assert_eq!(show.title, "Renamed Online Show");
        assert_eq!(show.show_key, initial_key);
        assert!(show.is_online_tracked());
    }
}

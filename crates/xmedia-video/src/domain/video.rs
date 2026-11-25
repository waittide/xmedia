use std::collections::HashSet;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::LibraryId;
use xmedia_core::domain::media::{ProbedAudioStream, ProbedSubtitleStream};
use xmedia_formats::video::{SubtitleFormatExt, VideoFormatExt, extract_video_resolution};

use crate::domain::id::{ShowId, VideoId};
pub use crate::domain::show::{ShowMetadataPatch, ShowQueryFilter, ShowSource, VideoShow};

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

#[cfg(test)]
mod tests {
    use super::*;

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
}

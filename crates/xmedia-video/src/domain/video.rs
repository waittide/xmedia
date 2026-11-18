use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::LibraryId;
use xmedia_core::infra::ffmpeg::{ProbedAudioStream, ProbedSubtitleStream};

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
        match ext
            .trim()
            .trim_start_matches('.')
            .to_ascii_lowercase()
            .as_str()
        {
            "mp4" => Some(Self::Mp4),
            "mkv" => Some(Self::Mkv),
            "webm" => Some(Self::Webm),
            "mov" => Some(Self::Mov),
            "avi" => Some(Self::Avi),
            "ts" | "m2ts" | "mts" => Some(Self::Ts),
            "flv" => Some(Self::Flv),
            "m4v" => Some(Self::M4v),
            _ => None,
        }
    }
}

pub fn is_subtitle_extension(ext: &str) -> bool {
    matches!(
        ext.trim()
            .trim_start_matches('.')
            .to_ascii_lowercase()
            .as_str(),
        "srt" | "vtt" | "webvtt" | "ass" | "ssa"
    )
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
}

impl VideoSource {
    pub fn display_path(&self) -> &str {
        match self {
            Self::Managed { storage_key, .. } => storage_key,
            Self::LocalPath { relative_path, .. } => relative_path,
        }
    }

    pub fn sidecar_subtitles(&self) -> &[SidecarSubtitle] {
        match self {
            Self::Managed {
                sidecar_subtitles, ..
            }
            | Self::LocalPath {
                sidecar_subtitles, ..
            } => sidecar_subtitles,
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
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoShow {
    pub id: ShowId,
    pub library_id: LibraryId,
    pub show_key: String,
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
        let h = self.height?;
        Some(if h >= 2100 {
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
        })
    }
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

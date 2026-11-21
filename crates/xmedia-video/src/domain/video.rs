use std::collections::HashSet;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::LibraryId;
use xmedia_core::domain::media::{ProbedAudioStream, ProbedSubtitleStream};

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

    /// Applies a metadata patch to this show, syncing `show_key` on rename and normalizing
    /// `genres`/`tags`. Returns `Ok(true)` if the show title changed.
    pub fn apply_patch(&mut self, patch: ShowMetadataPatch) -> DomainResult<bool> {
        let mut title_changed = false;
        if let Some(title) = patch.title {
            let trimmed = title.trim();
            if trimmed.is_empty() {
                return Err(DomainError::Validation("show title cannot be empty".into()));
            }
            if self.title != trimmed {
                self.title = trimmed.to_string();
                self.show_key = Self::compute_show_key(self.library_id, &self.title);
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
}

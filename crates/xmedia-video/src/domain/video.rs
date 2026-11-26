use std::collections::HashSet;
use std::fmt;
use std::str::FromStr;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::LibraryId;
use xmedia_core::domain::media::{ProbedAudioStream, ProbedSubtitleStream};
use xmedia_core::domain::user::VideoQualityProfile;
use xmedia_formats::video::{VideoFormatExt, extract_video_resolution};

use crate::domain::id::{ShowId, VideoId};
pub use crate::domain::show::{
    MissingShowMetadata, ShowMetadataPatch, ShowQueryFilter, ShowSource, VideoShow,
};

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
}

impl AsRef<str> for VideoContainer {
    fn as_ref(&self) -> &str {
        container_ext_str(*self)
    }
}

impl fmt::Display for VideoContainer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(container_ext_str(*self))
    }
}

impl FromStr for VideoContainer {
    type Err = DomainError;

    fn from_str(raw: &str) -> DomainResult<Self> {
        parse_container_ext(raw).ok_or_else(|| {
            DomainError::UnsupportedFormat(format!("unsupported video container: {raw}"))
        })
    }
}

fn container_ext_str(container: VideoContainer) -> &'static str {
    match container {
        VideoContainer::Mp4 => "mp4",
        VideoContainer::Mkv => "mkv",
        VideoContainer::Webm => "webm",
        VideoContainer::Mov => "mov",
        VideoContainer::Avi => "avi",
        VideoContainer::Ts => "ts",
        VideoContainer::Flv => "flv",
        VideoContainer::M4v => "m4v",
    }
}

fn parse_container_ext(ext: &str) -> Option<VideoContainer> {
    match VideoFormatExt::from_extension(ext)? {
        VideoFormatExt::Mp4 => Some(VideoContainer::Mp4),
        VideoFormatExt::Mkv => Some(VideoContainer::Mkv),
        VideoFormatExt::Webm => Some(VideoContainer::Webm),
        VideoFormatExt::Mov => Some(VideoContainer::Mov),
        VideoFormatExt::Avi => Some(VideoContainer::Avi),
        VideoFormatExt::Ts | VideoFormatExt::M2ts | VideoFormatExt::Mts => Some(VideoContainer::Ts),
        VideoFormatExt::Flv => Some(VideoContainer::Flv),
        VideoFormatExt::M4v => Some(VideoContainer::M4v),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoTranscodeMode {
    /// Copy H.264 video stream without re-encoding, transcode audio to AAC into fragmented MP4
    RemuxVideoTranscodeAudio,
    /// Full H.264 + AAC real-time transcode into fragmented MP4 with optional max height
    FullTranscode { max_height: Option<u32> },
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
}

pub struct OnlineTrackedVideoSeed {
    pub library_id: LibraryId,
    pub show_id: Option<ShowId>,
    pub show_title: Option<String>,
    pub episode_number: Option<u32>,
    pub title: String,
    pub summary: String,
    pub year: Option<i32>,
    pub genres: Vec<String>,
    pub poster_key: Option<String>,
    pub source_id: Uuid,
    pub video_id: String,
    pub detail_url: String,
    pub play_url: String,
    pub group_name: String,
    pub episode_name: String,
    pub now: Timestamp,
}

pub struct CachedOnlineVideoUpdate {
    pub container: VideoContainer,
    pub storage_key: String,
    pub file_size: u64,
    pub file_hash: String,
    pub duration_secs: u32,
    pub bit_rate_kbps: Option<u32>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub video_codec: Option<String>,
    pub audio_streams: Vec<ProbedAudioStream>,
    pub subtitle_streams: Vec<ProbedSubtitleStream>,
    pub sidecar_subtitles: Vec<SidecarSubtitle>,
    pub fallback_poster_key: Option<String>,
    pub now: Timestamp,
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
    pub fn binding_id(&self) -> Uuid {
        self.show_id.map(|s| s.0).unwrap_or(self.id.0)
    }

    pub fn new_online_tracked(seed: OnlineTrackedVideoSeed) -> Self {
        let file_hash = build_online_file_hash(seed.source_id, &seed.video_id, seed.episode_number);
        let season_number = seed.show_id.map(|_| 1);
        Self {
            id: VideoId::new(),
            library_id: seed.library_id,
            show_id: seed.show_id,
            show_title: seed.show_title,
            season_number,
            episode_number: seed.episode_number,
            title: seed.title,
            summary: seed.summary,
            year: seed.year,
            genres: seed.genres,
            tags: Vec::new(),
            container: VideoContainer::Mp4,
            file_size: 0,
            file_hash,
            duration_secs: 0,
            bit_rate_kbps: None,
            width: None,
            height: None,
            video_codec: None,
            audio_streams: Vec::new(),
            subtitle_streams: Vec::new(),
            poster_key: seed.poster_key,
            source: VideoSource::OnlineTracked(Box::new(OnlineTrackedVideoSource {
                storage_key: None,
                source_id: seed.source_id,
                video_id: seed.video_id,
                detail_url: seed.detail_url,
                play_url: seed.play_url,
                group_name: Some(seed.group_name),
                episode_name: Some(seed.episode_name),
                cached: false,
                sidecar_subtitles: Vec::new(),
            })),
            is_missing: false,
            created_at: seed.now,
            updated_at: seed.now,
        }
    }

    fn can_direct_play_in_browser(&self) -> bool {
        self.container.is_browser_native_container()
            && is_browser_video_codec(self.video_codec.as_deref())
            && self
                .audio_streams
                .first()
                .is_none_or(|a| is_browser_audio_codec(&a.codec))
    }

    pub fn resolve_playback_plan(
        &self,
        quality: VideoQualityProfile,
        audio_stream_index: Option<u32>,
        start_secs: Option<u32>,
    ) -> DomainResult<Option<VideoTranscodeMode>> {
        validate_audio_stream_index(&self.audio_streams, audio_stream_index)?;
        let offset = start_secs.filter(|&s| s > 0);
        let default_audio_idx = self.audio_streams.first().map(|a| a.stream_index);
        let custom_audio = audio_stream_index.filter(|&idx| Some(idx) != default_audio_idx);

        if should_direct_play(
            quality,
            offset,
            custom_audio,
            self.can_direct_play_in_browser(),
        ) {
            return Ok(None);
        }
        Ok(Some(select_transcode_mode(
            quality,
            is_h264_codec(self.video_codec.as_deref()),
        )))
    }

    pub fn resolution_label(&self) -> Option<String> {
        self.height
            .map(format_height_tier)
            .or_else(|| extract_video_resolution(self.source.display_path()))
    }

    pub fn apply_patch(&mut self, patch: VideoMetadataPatch) -> DomainResult<()> {
        if let Some(title) = patch.title {
            self.title = validate_non_empty_title(&title)?;
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

    pub fn mark_online_cached(&mut self, update: CachedOnlineVideoUpdate) -> DomainResult<()> {
        let VideoSource::OnlineTracked(ref mut tracking_info) = self.source else {
            return Err(DomainError::Validation("该视频不是在线追更资源".into()));
        };
        if self.poster_key.is_none() && update.fallback_poster_key.is_some() {
            self.poster_key = update.fallback_poster_key;
        }
        self.container = update.container;
        self.file_size = update.file_size;
        self.file_hash = update.file_hash;
        if update.duration_secs > 0 {
            self.duration_secs = update.duration_secs;
        }
        if update.bit_rate_kbps.is_some() {
            self.bit_rate_kbps = update.bit_rate_kbps;
        }
        if update.width.is_some() {
            self.width = update.width;
        }
        if update.height.is_some() {
            self.height = update.height;
        }
        if update.video_codec.is_some() {
            self.video_codec = update.video_codec;
        }
        if !update.audio_streams.is_empty() {
            self.audio_streams = update.audio_streams;
        }
        if !update.subtitle_streams.is_empty() {
            self.subtitle_streams = update.subtitle_streams;
        }
        tracking_info.storage_key = Some(update.storage_key);
        tracking_info.cached = true;
        tracking_info.sidecar_subtitles = update.sidecar_subtitles;
        self.updated_at = update.now;
        Ok(())
    }
}

fn build_online_file_hash(source_id: Uuid, video_id: &str, episode_number: Option<u32>) -> String {
    match episode_number {
        Some(ep_num) => format!("online:{source_id}:{video_id}:{ep_num}"),
        None => format!("online:{source_id}:{video_id}"),
    }
}

fn is_browser_video_codec(codec: Option<&str>) -> bool {
    match codec {
        Some(raw) => matches!(
            raw.to_ascii_lowercase().as_str(),
            "h264" | "avc1" | "vp8" | "vp9" | "av1"
        ),
        None => true,
    }
}

fn is_browser_audio_codec(codec: &str) -> bool {
    matches!(
        codec.to_ascii_lowercase().as_str(),
        "aac" | "mp3" | "opus" | "vorbis" | "flac"
    )
}

fn is_h264_codec(codec: Option<&str>) -> bool {
    codec.is_some_and(|c| matches!(c.to_ascii_lowercase().as_str(), "h264" | "avc1"))
}

fn validate_audio_stream_index(
    streams: &[ProbedAudioStream],
    requested: Option<u32>,
) -> DomainResult<()> {
    if let Some(idx) = requested
        && !streams.iter().any(|a| a.stream_index == idx)
    {
        return Err(DomainError::Validation(format!(
            "invalid audio_stream index {idx} for this video"
        )));
    }
    Ok(())
}

fn should_direct_play(
    quality: VideoQualityProfile,
    offset: Option<u32>,
    custom_audio: Option<u32>,
    browser_compatible: bool,
) -> bool {
    match quality {
        VideoQualityProfile::Direct => offset.is_none() && custom_audio.is_none(),
        VideoQualityProfile::Auto => {
            offset.is_none() && custom_audio.is_none() && browser_compatible
        }
        VideoQualityProfile::P1080 | VideoQualityProfile::P720 | VideoQualityProfile::P480 => false,
    }
}

fn select_transcode_mode(quality: VideoQualityProfile, is_h264: bool) -> VideoTranscodeMode {
    match quality {
        VideoQualityProfile::Auto | VideoQualityProfile::Direct if is_h264 => {
            VideoTranscodeMode::RemuxVideoTranscodeAudio
        }
        VideoQualityProfile::Auto | VideoQualityProfile::Direct => {
            VideoTranscodeMode::FullTranscode { max_height: None }
        }
        VideoQualityProfile::P1080 => VideoTranscodeMode::FullTranscode {
            max_height: Some(1080),
        },
        VideoQualityProfile::P720 => VideoTranscodeMode::FullTranscode {
            max_height: Some(720),
        },
        VideoQualityProfile::P480 => VideoTranscodeMode::FullTranscode {
            max_height: Some(480),
        },
    }
}

fn format_height_tier(height: u32) -> String {
    if height >= 2100 {
        "4K".to_string()
    } else if height >= 1400 {
        "1440p".to_string()
    } else if height >= 1000 {
        "1080p".to_string()
    } else if height >= 700 {
        "720p".to_string()
    } else if height >= 460 {
        "480p".to_string()
    } else {
        format!("{height}p")
    }
}

fn validate_non_empty_title(raw: &str) -> DomainResult<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(DomainError::Validation("title cannot be empty".into()));
    }
    Ok(trimmed.to_string())
}

pub(in crate::domain) fn normalize_string_list(values: Vec<String>) -> Vec<String> {
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

pub fn sort_episode_items(items: &mut [VideoItem]) {
    items.sort_by(compare_episode_order);
}

fn compare_episode_order(a: &VideoItem, b: &VideoItem) -> std::cmp::Ordering {
    a.season_number
        .unwrap_or(1)
        .cmp(&b.season_number.unwrap_or(1))
        .then_with(|| {
            a.episode_number
                .unwrap_or(u32::MAX)
                .cmp(&b.episode_number.unwrap_or(u32::MAX))
        })
        .then_with(|| a.title.cmp(&b.title))
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

impl VideoQueryFilter {
    pub fn apply(&self, items: impl IntoIterator<Item = VideoItem>) -> Vec<VideoItem> {
        let search_lower = normalize_filter_term(self.search.as_deref());
        let genre_lower = normalize_filter_term(self.genre.as_deref());
        let tag_lower = normalize_filter_term(self.tag.as_deref());

        let mut filtered: Vec<VideoItem> = items
            .into_iter()
            .filter(|v| !v.is_missing)
            .filter(|v| self.show_id.is_none_or(|sid| v.show_id == Some(sid)))
            .filter(|v| matches_kind_filter(v, self.kind))
            .filter(|v| self.year.is_none_or(|y| v.year == Some(y)))
            .filter(|v| matches_case_insensitive_list(&v.genres, genre_lower.as_deref()))
            .filter(|v| matches_case_insensitive_list(&v.tags, tag_lower.as_deref()))
            .filter(|v| matches_video_search(v, search_lower.as_deref()))
            .collect();

        sort_filtered_videos(&mut filtered, self.sort_by, self.sort_desc);
        filtered
    }
}

pub(super) fn normalize_filter_term(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_lowercase)
}

pub(super) fn matches_case_insensitive_list(values: &[String], expected: Option<&str>) -> bool {
    expected.is_none_or(|needle| values.iter().any(|v| v.to_lowercase() == needle))
}

fn matches_kind_filter(video: &VideoItem, kind: VideoKindFilter) -> bool {
    match kind {
        VideoKindFilter::All => true,
        VideoKindFilter::Standalone => video.show_id.is_none(),
        VideoKindFilter::Episodes => video.show_id.is_some(),
    }
}

fn matches_video_search(video: &VideoItem, query: Option<&str>) -> bool {
    query.is_none_or(|q| {
        video.title.to_lowercase().contains(q)
            || video.summary.to_lowercase().contains(q)
            || video
                .show_title
                .as_ref()
                .is_some_and(|st| st.to_lowercase().contains(q))
    })
}

fn sort_filtered_videos(items: &mut [VideoItem], sort_by: VideoSortBy, sort_desc: Option<bool>) {
    let desc = sort_desc.unwrap_or(matches!(
        sort_by,
        VideoSortBy::CreatedAt | VideoSortBy::Year | VideoSortBy::Duration
    ));
    match sort_by {
        VideoSortBy::CreatedAt => items.sort_by_key(|v| v.created_at),
        VideoSortBy::Title => items.sort_by_key(|v| v.title.to_lowercase()),
        VideoSortBy::Year => items.sort_by_key(|v| v.year.unwrap_or(0)),
        VideoSortBy::Duration => items.sort_by_key(|v| v.duration_secs),
    }
    if desc {
        items.reverse();
    }
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

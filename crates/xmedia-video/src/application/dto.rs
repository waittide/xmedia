use bytes::Bytes;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use xmedia_core::application::dto::FfmpegStatusDto;
use xmedia_core::domain::library::LibraryId;
use xmedia_core::infra::ffmpeg::ProbedAudioStream;

use crate::domain::id::{ShowId, VideoId, VideoShelfId};
use crate::domain::video::VideoContainer;
use crate::domain::watch::WatchProgress;

#[derive(Debug, Clone)]
pub struct VideoUploadItem {
    pub filename: String,
    pub bytes: Bytes,
}

#[derive(Debug, Clone, Serialize)]
pub struct VideoUploadResultDto {
    pub uploaded_videos: Vec<VideoSummaryDto>,
    pub attached_subtitles_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct VideoSystemStatusDto {
    pub ffmpeg: FfmpegStatusDto,
    pub scanning: bool,
    pub video_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtitleTrackDto {
    /// e.g. `emb-2` for embedded stream 2, or `ext-<id>` for sidecar subtitle
    pub id: String,
    pub source_kind: String,
    pub codec_or_format: String,
    pub language: Option<String>,
    pub label: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoSummaryDto {
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
    pub mime_type: String,
    pub can_direct_play: bool,
    pub file_size: u64,
    pub duration_secs: u32,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub resolution: Option<String>,
    pub video_codec: Option<String>,
    pub poster_key: Option<String>,
    pub is_favorite: bool,
    pub progress: Option<WatchProgress>,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoDetailDto {
    pub video: VideoSummaryDto,
    pub bit_rate_kbps: Option<u32>,
    pub path: String,
    pub audio_streams: Vec<ProbedAudioStream>,
    pub subtitles: Vec<SubtitleTrackDto>,
    pub prev_episode_id: Option<VideoId>,
    pub next_episode_id: Option<VideoId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShowSummaryDto {
    pub id: ShowId,
    pub library_id: LibraryId,
    pub title: String,
    pub summary: String,
    pub year: Option<i32>,
    pub genres: Vec<String>,
    pub tags: Vec<String>,
    pub poster_key: Option<String>,
    pub season_count: u32,
    pub episode_count: u32,
    pub watched_episode_count: u32,
    pub is_favorite: bool,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeasonGroupDto {
    pub season_number: u32,
    pub episodes: Vec<VideoSummaryDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShowDetailDto {
    pub show: ShowSummaryDto,
    pub seasons: Vec<SeasonGroupDto>,
    pub continue_episode: Option<VideoSummaryDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoHomeOverviewDto {
    pub continue_watching: Vec<VideoSummaryDto>,
    pub recent_videos: Vec<VideoSummaryDto>,
    pub recent_shows: Vec<ShowSummaryDto>,
    pub total_videos: u64,
    pub total_shows: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoFavoritesDto {
    pub videos: Vec<VideoSummaryDto>,
    pub shows: Vec<ShowSummaryDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoShelfDto {
    pub id: VideoShelfId,
    pub name: String,
    pub description: String,
    pub video_count: usize,
    pub poster_key: Option<String>,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoShelfDetailDto {
    pub shelf: VideoShelfDto,
    pub videos: Vec<VideoSummaryDto>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SaveWatchProgressInput {
    pub position_secs: f64,
    pub duration_secs: Option<u32>,
    pub completed: Option<bool>,
    pub selected_audio_stream: Option<u32>,
    pub selected_subtitle: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpsertVideoShelfInput {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub video_ids: Vec<VideoId>,
}

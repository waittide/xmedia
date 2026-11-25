use serde::{Deserialize, Serialize};
use uuid::Uuid;
use xmedia_source::crud::{
    BatchSourceAction, BatchSourcesInput, BatchSourcesReportDto, ImportSourcesReportDto,
    SourceSummaryDto, SourceTestReportDto, SourceTestStepDto, UpdateSourceInput,
};

use crate::domain::catalog::StructuredLyrics;
use crate::domain::online::{ExecutionDiagnostics, MusicSourceTestMode};

pub type MusicSourceSummaryDto = SourceSummaryDto;
pub type ImportMusicSourcesReportDto = ImportSourcesReportDto<MusicSourceSummaryDto>;
pub type UpdateMusicSourceInput = UpdateSourceInput;
pub type BatchMusicSourceAction = BatchSourceAction;
pub type BatchMusicSourcesInput = BatchSourcesInput;
pub type BatchMusicSourcesReportDto = BatchSourcesReportDto;
pub type LoginActionResponseDto =
    xmedia_source::crud::LoginActionResponseDto<MusicSourceSummaryDto>;
pub type SourceDebugStepDto = SourceTestStepDto;
pub type StructuredLyricsDto = StructuredLyrics;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineMusicTrackDto {
    pub source_id: Uuid,
    pub source_name: String,
    pub track_id: String,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub duration_secs: Option<u32>,
    pub cover_url: Option<String>,
    pub proxy_cover_url: Option<String>,
    pub detail_url: Option<String>,
    pub play_url: Option<String>,
    pub url_id: Option<String>,
    pub lyric_id: Option<String>,
    pub cover_id: Option<String>,
    pub qualities: Vec<String>,
    pub format: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceSearchErrorDto {
    pub source_id: Uuid,
    pub source_name: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineMusicSearchResponseDto {
    pub keyword: String,
    pub page: u32,
    pub results: Vec<OnlineMusicTrackDto>,
    pub errors: Vec<SourceSearchErrorDto>,
    pub toasts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MusicExploreCategoryDto {
    pub title: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineMusicExploreResponseDto {
    pub source_id: Uuid,
    pub results: Vec<OnlineMusicTrackDto>,
    pub categories: Vec<MusicExploreCategoryDto>,
    pub diagnostics: ExecutionDiagnostics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineMusicResolvedTrackDto {
    pub source_id: Uuid,
    pub source_name: String,
    pub track: OnlineMusicTrackDto,
    pub stream_url: Option<String>,
    pub proxy_stream_url: Option<String>,
    pub cover_url: Option<String>,
    pub proxy_cover_url: Option<String>,
    pub lyrics: Option<StructuredLyricsDto>,
    pub translated_lyrics: Option<StructuredLyricsDto>,
    pub format: Option<String>,
    pub bitrate: Option<u32>,
    pub expires_in_secs: Option<u64>,
    pub diagnostics: ExecutionDiagnostics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineMusicStreamResponseDto {
    pub source_id: Uuid,
    pub stream_url: String,
    pub proxy_stream_url: String,
    pub format: Option<String>,
    pub bitrate: Option<u32>,
    pub expires_in_secs: Option<u64>,
    pub headers_applied: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct MusicSourceTestInput {
    pub keyword: Option<String>,
    pub mode: MusicSourceTestMode,
    pub target_url: Option<String>,
    pub explore_url: Option<String>,
    pub track_id: Option<String>,
    pub detail_url: Option<String>,
    pub play_url: Option<String>,
    pub url_id: Option<String>,
    pub cover_id: Option<String>,
    pub lyric_id: Option<String>,
    pub quality: Option<String>,
    pub title: Option<String>,
    pub artist: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct MusicSourceTestPayload {
    pub search_items: Vec<OnlineMusicTrackDto>,
    pub resolved_track: Option<OnlineMusicResolvedTrackDto>,
    pub stream_probe: Option<OnlineMusicStreamResponseDto>,
    pub lyrics: Option<StructuredLyricsDto>,
    pub explore_categories: Vec<MusicExploreCategoryDto>,
}

pub type MusicSourceTestReportDto =
    SourceTestReportDto<MusicSourceTestMode, MusicSourceTestPayload>;

#[derive(Debug, Clone, Deserialize)]
pub struct TrackOnlineMusicInput {
    pub source_id: Uuid,
    #[serde(default)]
    pub track_id: String,
    pub title: String,
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub album: Option<String>,
    #[serde(default)]
    pub duration_secs: Option<u32>,
    #[serde(default)]
    pub cover_url: Option<String>,
    #[serde(default)]
    pub cover_id: Option<String>,
    #[serde(default)]
    pub url_id: Option<String>,
    #[serde(default)]
    pub lyric_id: Option<String>,
    #[serde(default)]
    pub play_url: Option<String>,
    #[serde(default)]
    pub detail_url: Option<String>,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub quality: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TrackOnlineMusicBatchInput {
    pub tracks: Vec<TrackOnlineMusicInput>,
}

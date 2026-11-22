use serde::{Deserialize, Serialize};
use uuid::Uuid;
use xmedia_core::application::ports::ByteChunkStream;

use crate::source::model::{
    ExecutionDiagnostics, OnlineVideoCategory, OnlineVideoDetail, OnlineVideoPlayResult,
    OnlineVideoSummary,
};

pub type VideoSourceSummaryDto = xmedia_source::crud::SourceSummaryDto;
pub type ImportVideoSourcesReportDto =
    xmedia_source::crud::ImportSourcesReportDto<VideoSourceSummaryDto>;
pub type UpdateVideoSourceInput = xmedia_source::crud::UpdateSourceInput;
pub type BatchVideoSourceAction = xmedia_source::crud::BatchSourceAction;
pub type BatchVideoSourcesInput = xmedia_source::crud::BatchSourcesInput;
pub type BatchVideoSourcesReportDto = xmedia_source::crud::BatchSourcesReportDto;
pub type LoginActionResponseDto =
    xmedia_source::crud::LoginActionResponseDto<VideoSourceSummaryDto>;
pub type VideoSourceTestStepDto = xmedia_source::crud::SourceTestStepDto;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoSourceTestMode {
    #[default]
    Full,
    Categories,
    Search,
    #[serde(alias = "explore")]
    Browse,
    Detail,
    Play,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct VideoSourceTestInput {
    pub keyword: Option<String>,
    pub mode: VideoSourceTestMode,
    pub category_id: Option<String>,
    pub video_id: Option<String>,
    pub detail_url: Option<String>,
    pub play_url: Option<String>,
    pub group_name: Option<String>,
    pub target_url: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VideoSourceTestPayload {
    pub categories: Vec<OnlineVideoCategory>,
    pub search_items: Vec<OnlineVideoSummary>,
    pub video_detail: Option<OnlineVideoDetail>,
    pub play_result: Option<OnlineVideoPlayResult>,
}

pub type VideoSourceTestReportDto =
    xmedia_source::crud::SourceTestReportDto<VideoSourceTestMode, VideoSourceTestPayload>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineVideoCategoriesResponseDto {
    pub source_id: Uuid,
    pub source_name: String,
    pub categories: Vec<OnlineVideoCategory>,
    #[serde(default)]
    pub has_explore: bool,
    pub diagnostics: ExecutionDiagnostics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineVideoSearchItemDto {
    pub source_id: Uuid,
    pub source_name: String,
    #[serde(flatten)]
    pub video: OnlineVideoSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineVideoSearchErrorDto {
    pub source_id: Uuid,
    pub source_name: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineVideoSearchResponseDto {
    pub keyword: String,
    pub page: u32,
    pub results: Vec<OnlineVideoSearchItemDto>,
    pub errors: Vec<OnlineVideoSearchErrorDto>,
    pub toasts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineVideoBrowseResponseDto {
    pub source_id: Uuid,
    pub source_name: String,
    pub category_id: Option<String>,
    pub results: Vec<OnlineVideoSummary>,
    pub total: Option<u64>,
    pub page: u32,
    pub page_count: Option<u32>,
    pub diagnostics: ExecutionDiagnostics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineVideoDetailResponseDto {
    pub source_id: Uuid,
    pub source_name: String,
    #[serde(flatten)]
    pub detail: OnlineVideoDetail,
    pub diagnostics: ExecutionDiagnostics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineVideoPlayResponseDto {
    pub source_id: Uuid,
    pub source_name: String,
    #[serde(flatten)]
    pub play: OnlineVideoPlayResult,
    pub diagnostics: ExecutionDiagnostics,
}

pub struct ProxyStreamPayload {
    pub status: u16,
    pub content_type: String,
    pub content_length: Option<u64>,
    pub content_range: Option<String>,
    pub accept_ranges: String,
    pub stream: ByteChunkStream,
}

pub enum ProxyStreamOutcome {
    Stream(ProxyStreamPayload),
    RangeNotSatisfiable { content_range: Option<String> },
}

pub enum HlsProxyOutcome {
    Playlist(String),
    Segment(ProxyStreamPayload),
    RangeNotSatisfiable { content_range: Option<String> },
}

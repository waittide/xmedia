use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use url::Url;
use uuid::Uuid;
use xmedia_core::application::dto::FfmpegStatusDto;
use xmedia_core::application::ports::{
    BinaryPayload, ByteChunkStream, ByteRangeSpec, ByteRangeStream,
};
use xmedia_core::domain::error::DomainResult;
use xmedia_core::domain::library::Library;
use xmedia_core::domain::media::{ProbedAudioStream, ProbedSubtitleStream};
use xmedia_core::domain::user::VideoQualityProfile;

use crate::domain::id::VideoId;
use crate::domain::online::{
    OnlineVideoCategory, OnlineVideoDetail, OnlineVideoPlayResult, OnlineVideoSummary,
    SourceCallError, SourceCallOutput, SourceRuntimeState, VideoBrowseOutput, VideoSourceSpec,
};
pub use crate::domain::video::VideoTranscodeMode;
use crate::domain::video::{SidecarSubtitle, VideoContainer, VideoItem};

#[derive(Debug, Clone, Default)]
pub struct ProbedVideoInfo {
    pub duration_secs: u32,
    pub bit_rate_kbps: Option<u32>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub video_codec: Option<String>,
    pub container_format: Option<String>,
    pub audio_streams: Vec<ProbedAudioStream>,
    pub subtitle_streams: Vec<ProbedSubtitleStream>,
}

#[derive(Debug, Clone)]
pub struct StoredManagedVideo {
    pub storage_key: String,
    pub absolute_path: String,
    pub file_size: u64,
    pub file_hash: String,
}

#[derive(Debug, Clone, Default)]
pub struct ParsedNfoMetadata {
    pub title: Option<String>,
    pub show_title: Option<String>,
    pub summary: Option<String>,
    pub year: Option<i32>,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub genres: Vec<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ScannedVideoEntry {
    pub absolute_path: String,
    pub relative_path: String,
    pub container: VideoContainer,
    pub file_size: u64,
    pub file_mtime: i64,
    pub sidecar_subtitles: Vec<SidecarSubtitle>,
    pub directory_poster_path: Option<String>,
    pub nfo: Option<ParsedNfoMetadata>,
}

#[derive(Debug, Clone)]
pub struct VideoStreamTranscodeRequest {
    pub mode: VideoTranscodeMode,
    pub audio_stream_index: Option<u32>,
    pub start_secs: Option<u32>,
}

#[async_trait]
pub trait VideoStoragePort: Send + Sync {
    async fn save_managed_video(
        &self,
        video_id: VideoId,
        container: VideoContainer,
        bytes: Bytes,
    ) -> DomainResult<StoredManagedVideo>;

    async fn save_managed_video_from_path(
        &self,
        video_id: VideoId,
        container: VideoContainer,
        src_path: &Path,
    ) -> DomainResult<StoredManagedVideo> {
        let data = tokio::fs::read(src_path).await.map_err(|e| {
            xmedia_core::domain::error::DomainError::Storage(format!(
                "failed reading temp uploaded video: {e}"
            ))
        })?;
        self.save_managed_video(video_id, container, Bytes::from(data))
            .await
    }

    async fn save_managed_subtitle(
        &self,
        video_id: VideoId,
        sub_id: &str,
        ext: &str,
        bytes: Bytes,
    ) -> DomainResult<String>;

    async fn save_managed_subtitle_from_path(
        &self,
        video_id: VideoId,
        sub_id: &str,
        ext: &str,
        src_path: &Path,
    ) -> DomainResult<String> {
        let data = tokio::fs::read(src_path).await.map_err(|e| {
            xmedia_core::domain::error::DomainError::Storage(format!(
                "failed reading temp uploaded subtitle: {e}"
            ))
        })?;
        self.save_managed_subtitle(video_id, sub_id, ext, Bytes::from(data))
            .await
    }

    async fn delete_managed_file(&self, key: &str) -> DomainResult<()>;

    async fn save_poster_bytes(&self, bytes: &[u8]) -> DomainResult<String>;

    /// Saves a server-generated poster (either an FFmpeg raster frame or a server-synthesized
    /// SVG placeholder). Never call with untrusted user uploads or external library files.
    async fn save_generated_poster_bytes(&self, bytes: &[u8]) -> DomainResult<String> {
        self.save_poster_bytes(bytes).await
    }

    async fn read_poster(&self, key: &str, max_size: Option<u32>) -> DomainResult<BinaryPayload>;

    async fn resolve_video_fs_path(
        &self,
        library: &Library,
        video: &VideoItem,
    ) -> DomainResult<String>;

    async fn read_sidecar_subtitle_bytes(
        &self,
        library: &Library,
        video: &VideoItem,
        sub_id: &str,
    ) -> DomainResult<(String, Vec<u8>)>;

    async fn open_video_range_stream(
        &self,
        library: &Library,
        video: &VideoItem,
        spec: ByteRangeSpec,
    ) -> DomainResult<ByteRangeStream>;
}

#[async_trait]
pub trait VideoScannerPort: Send + Sync {
    async fn scan_video_entries(&self, root_path: &str) -> DomainResult<Vec<ScannedVideoEntry>>;
    async fn read_external_image(&self, path: &str) -> DomainResult<Option<Vec<u8>>>;
    async fn compute_file_fingerprint(&self, path: &Path) -> DomainResult<String>;
}

#[async_trait]
pub trait VideoMediaEnginePort: Send + Sync {
    async fn check_status(&self) -> FfmpegStatusDto;

    async fn probe_video(&self, fs_path: &Path) -> ProbedVideoInfo;

    async fn generate_poster_bytes(
        &self,
        fs_path: &Path,
        duration_secs: u32,
        fallback_title: &str,
    ) -> DomainResult<Vec<u8>>;

    async fn extract_embedded_subtitle_vtt(
        &self,
        fs_path: &Path,
        stream_index: u32,
    ) -> DomainResult<String>;

    fn convert_sidecar_subtitle_to_vtt(&self, format: &str, raw_bytes: &[u8]) -> String;

    fn shift_webvtt_timestamps(&self, vtt: &str, offset_secs: f64) -> String;

    async fn open_fmp4_transcode_stream(
        &self,
        fs_path: &Path,
        req: &VideoStreamTranscodeRequest,
    ) -> DomainResult<ByteChunkStream>;

    async fn remux_local_hls_to_mp4(
        &self,
        local_playlist_path: &Path,
        output_mp4_path: &Path,
    ) -> DomainResult<()>;
}

pub fn parse_quality_param(raw: Option<&str>) -> VideoQualityProfile {
    match raw.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
        Some("direct") | Some("raw") | Some("original") => VideoQualityProfile::Direct,
        Some("1080p") | Some("p1080") | Some("1080") => VideoQualityProfile::P1080,
        Some("720p") | Some("p720") | Some("720") => VideoQualityProfile::P720,
        Some("480p") | Some("p480") | Some("480") => VideoQualityProfile::P480,
        _ => VideoQualityProfile::Auto,
    }
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

pub trait VideoProxyUrlBuilder: Send + Sync {
    fn poster_proxy_url(&self, target_url: &str, source_id: Option<Uuid>) -> String;
    fn stream_proxy_url(&self, target_url: &str, source_id: Uuid, token: Option<&str>) -> String;
    fn hls_proxy_url(&self, target_url: &str, source_id: Uuid, token: Option<&str>) -> String;
    fn subtitle_proxy_url(&self, target_url: &str, source_id: Uuid, token: Option<&str>) -> String;
}

pub trait VideoSourceEnginePort: Send + Sync {
    fn fetch_categories_sync_with_control(
        &self,
        source: &VideoSourceSpec,
        state: SourceRuntimeState,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<Vec<OnlineVideoCategory>>, SourceCallError>;

    fn search_sync_with_control(
        &self,
        source: &VideoSourceSpec,
        state: SourceRuntimeState,
        keyword: &str,
        page: u32,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<Vec<OnlineVideoSummary>>, SourceCallError>;

    fn browse_sync_with_control(
        &self,
        source: &VideoSourceSpec,
        state: SourceRuntimeState,
        category_id: Option<&str>,
        page: u32,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<VideoBrowseOutput>, SourceCallError>;

    fn fetch_detail_sync_with_control(
        &self,
        source: &VideoSourceSpec,
        state: SourceRuntimeState,
        video_id: Option<&str>,
        detail_url: Option<&str>,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<OnlineVideoDetail>, SourceCallError>;

    fn resolve_play_sync_with_control(
        &self,
        source: &VideoSourceSpec,
        state: SourceRuntimeState,
        play_url: &str,
        group_name: Option<&str>,
        op_timeout: Duration,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<SourceCallOutput<OnlineVideoPlayResult>, SourceCallError>;

    fn run_login_action_sync(
        &self,
        source: &VideoSourceSpec,
        state: SourceRuntimeState,
        login_info: HashMap<String, String>,
        action: Option<&str>,
    ) -> Result<SourceCallOutput<String>, String>;
}

#[async_trait]
pub trait VideoStreamTransportPort: Send + Sync {
    async fn fetch_poster_bytes(
        &self,
        raw_url: &str,
        source_headers: &HashMap<String, String>,
    ) -> DomainResult<(Vec<u8>, &'static str)>;

    async fn fetch_subtitle_vtt(
        &self,
        raw_url: &str,
        source_headers: &HashMap<String, String>,
    ) -> DomainResult<String>;

    async fn open_stream_proxy(
        &self,
        raw_url: &str,
        source_headers: HashMap<String, String>,
        range_header: Option<&str>,
    ) -> DomainResult<ProxyStreamOutcome>;

    async fn open_hls_proxy(
        &self,
        raw_url: &str,
        source_headers: HashMap<String, String>,
        range_header: Option<&str>,
        rewrite_uri: &(dyn for<'a> Fn(&'a str) -> String + Send + Sync),
    ) -> DomainResult<HlsProxyOutcome>;

    async fn download_direct_video_to_path(
        &self,
        raw_url: &str,
        source_headers: HashMap<String, String>,
        dest_path: &Path,
        max_bytes: u64,
    ) -> DomainResult<VideoContainer>;

    async fn fetch_hls_playlist_text(
        &self,
        raw_url: &str,
        source_headers: HashMap<String, String>,
    ) -> DomainResult<(String, Url)>;

    async fn download_hls_asset_to_path(
        &self,
        raw_url: &str,
        source_headers: HashMap<String, String>,
        dest_path: &Path,
        total_bytes: u64,
        max_bytes: u64,
    ) -> DomainResult<u64>;
}

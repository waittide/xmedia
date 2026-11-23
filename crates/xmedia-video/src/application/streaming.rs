use std::path::Path;
use std::sync::Arc;

use xmedia_core::application::bindings::MediaBindingService;
use xmedia_core::application::dto::FfmpegStatusDto;
use xmedia_core::application::ports::{ByteChunkStream, ByteRangeSpec, ByteRangeStream};
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;
use xmedia_core::domain::repository::LibraryRepository;
use xmedia_core::domain::user::{UserId, VideoQualityProfile};

use crate::application::ports::{
    VideoMediaEnginePort, VideoStoragePort, VideoStreamTranscodeRequest, VideoTranscodeMode,
};
use crate::domain::id::VideoId;
use crate::domain::repository::VideoCatalogRepository;
use crate::domain::video::VideoItem;

pub enum ResolvedVideoStream {
    DirectRange {
        video: VideoItem,
        range_stream: ByteRangeStream,
        is_partial: bool,
    },
    TranscodedFmp4 {
        video: VideoItem,
        stream: ByteChunkStream,
    },
}

#[derive(Clone)]
pub struct VideoStreamingService {
    libraries: Arc<dyn LibraryRepository>,
    catalog: Arc<dyn VideoCatalogRepository>,
    bindings: Arc<MediaBindingService>,
    storage: Arc<dyn VideoStoragePort>,
    engine: Arc<dyn VideoMediaEnginePort>,
}

impl VideoStreamingService {
    pub fn new(
        libraries: Arc<dyn LibraryRepository>,
        catalog: Arc<dyn VideoCatalogRepository>,
        bindings: Arc<MediaBindingService>,
        storage: Arc<dyn VideoStoragePort>,
        engine: Arc<dyn VideoMediaEnginePort>,
    ) -> Self {
        Self {
            libraries,
            catalog,
            bindings,
            storage,
            engine,
        }
    }

    pub async fn get_ffmpeg_status(&self) -> FfmpegStatusDto {
        self.engine.check_status().await
    }

    pub async fn open_raw_download(
        &self,
        user_id: UserId,
        video_id: VideoId,
        spec: ByteRangeSpec,
    ) -> DomainResult<ResolvedVideoStream> {
        let (video, library) = self.load_bound_video_and_library(user_id, video_id).await?;
        let range_stream = self
            .storage
            .open_video_range_stream(&library, &video, spec)
            .await?;
        Ok(ResolvedVideoStream::DirectRange {
            video,
            range_stream,
            is_partial: spec.is_partial(),
        })
    }

    pub async fn open_stream(
        &self,
        user_id: UserId,
        video_id: VideoId,
        quality: VideoQualityProfile,
        audio_stream_index: Option<u32>,
        start_secs: Option<u32>,
        spec: ByteRangeSpec,
    ) -> DomainResult<ResolvedVideoStream> {
        let (video, library) = self.load_bound_video_and_library(user_id, video_id).await?;
        if let Some(requested_audio) = audio_stream_index
            && !video
                .audio_streams
                .iter()
                .any(|a| a.stream_index == requested_audio)
        {
            return Err(DomainError::Validation(format!(
                "invalid audio_stream index {requested_audio} for this video"
            )));
        }
        let offset = start_secs.filter(|&s| s > 0);
        let default_audio_idx = video.audio_streams.first().map(|a| a.stream_index);
        let custom_audio = audio_stream_index.filter(|&idx| Some(idx) != default_audio_idx);

        let wants_direct = match quality {
            VideoQualityProfile::Direct => offset.is_none() && custom_audio.is_none(),
            VideoQualityProfile::Auto => {
                offset.is_none() && custom_audio.is_none() && video.can_direct_play_in_browser()
            }
            VideoQualityProfile::P1080 | VideoQualityProfile::P720 | VideoQualityProfile::P480 => {
                false
            }
        };

        if wants_direct {
            let range_stream = self
                .storage
                .open_video_range_stream(&library, &video, spec)
                .await?;
            return Ok(ResolvedVideoStream::DirectRange {
                video,
                range_stream,
                is_partial: spec.is_partial(),
            });
        }

        let is_h264 = video
            .video_codec
            .as_deref()
            .is_some_and(|c| matches!(c.to_ascii_lowercase().as_str(), "h264" | "avc1"));

        let mode = match quality {
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
        };

        let fs_path = self.storage.resolve_video_fs_path(&library, &video).await?;
        let stream = self
            .engine
            .open_fmp4_transcode_stream(
                Path::new(&fs_path),
                &VideoStreamTranscodeRequest {
                    mode,
                    audio_stream_index,
                    start_secs: offset,
                },
            )
            .await?;

        Ok(ResolvedVideoStream::TranscodedFmp4 { video, stream })
    }

    async fn load_bound_video_and_library(
        &self,
        user_id: UserId,
        video_id: VideoId,
    ) -> DomainResult<(VideoItem, xmedia_core::domain::library::Library)> {
        let video = self
            .catalog
            .find_video_by_id(video_id)
            .await?
            .filter(|v| !v.is_missing)
            .ok_or_else(|| DomainError::NotFound("video not found".into()))?;
        let binding_id = video.show_id.map(|s| s.0).unwrap_or(video.id.0);
        self.bindings
            .require_bound(user_id, MediaKind::Video, binding_id, "video not found")
            .await?;
        let library = self
            .libraries
            .find_by_id(video.library_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("library not found".into()))?;
        Ok((video, library))
    }
}

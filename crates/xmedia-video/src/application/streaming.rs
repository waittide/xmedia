use std::path::Path;
use std::sync::Arc;

use uuid::Uuid;
use xmedia_core::application::bindings::MediaBindingService;
use xmedia_core::application::dto::FfmpegStatusDto;
use xmedia_core::application::ports::{
    BinaryPayload, ByteChunkStream, ByteRangeSpec, ByteRangeStream,
};
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::{Library, MediaKind};
use xmedia_core::domain::repository::LibraryRepository;
use xmedia_core::domain::user::{User, UserId, VideoQualityProfile};

use crate::application::dto::SubtitleTrackDto;
use crate::application::ports::{
    VideoMediaEnginePort, VideoStoragePort, VideoStreamTranscodeRequest,
};
use crate::application::projector::VideoProjector;
use crate::domain::id::VideoId;
use crate::domain::repository::{ShowScope, VideoCatalogRepository, VideoScope};
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
    projector: VideoProjector,
}

impl VideoStreamingService {
    pub(super) fn new(
        libraries: Arc<dyn LibraryRepository>,
        catalog: Arc<dyn VideoCatalogRepository>,
        bindings: Arc<MediaBindingService>,
        storage: Arc<dyn VideoStoragePort>,
        engine: Arc<dyn VideoMediaEnginePort>,
        projector: VideoProjector,
    ) -> Self {
        Self {
            libraries,
            catalog,
            bindings,
            storage,
            engine,
            projector,
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
        let offset = start_secs.filter(|&s| s > 0);
        let Some(mode) = video.resolve_playback_plan(quality, audio_stream_index, start_secs)?
        else {
            let range_stream = self
                .storage
                .open_video_range_stream(&library, &video, spec)
                .await?;
            return Ok(ResolvedVideoStream::DirectRange {
                video,
                range_stream,
                is_partial: spec.is_partial(),
            });
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

    pub async fn list_video_subtitles(
        &self,
        user_id: UserId,
        video_id: VideoId,
    ) -> DomainResult<Vec<SubtitleTrackDto>> {
        let item = self
            .projector
            .require_video_bound(user_id, video_id)
            .await?;
        Ok(SubtitleTrackDto::collect_from_video(&item))
    }

    pub async fn get_subtitle_webvtt(
        &self,
        user_id: UserId,
        video_id: VideoId,
        sub_id: &str,
        offset_secs: Option<f64>,
    ) -> DomainResult<String> {
        let (video, library) = self.load_bound_video_and_library(user_id, video_id).await?;

        let trimmed = sub_id.trim().trim_end_matches(".vtt");
        let vtt = if let Some(raw_idx) = trimmed.strip_prefix("emb-") {
            let stream_index = raw_idx
                .parse::<u32>()
                .map_err(|_| DomainError::Validation("invalid embedded subtitle id".into()))?;
            let sub_stream = video
                .subtitle_streams
                .iter()
                .find(|s| s.stream_index == stream_index)
                .ok_or_else(|| {
                    DomainError::NotFound(format!(
                        "embedded subtitle stream {stream_index} not found"
                    ))
                })?;
            if !sub_stream.is_text_based {
                return Err(DomainError::Validation(format!(
                    "embedded subtitle stream {stream_index} ({}) is not text-based",
                    sub_stream.codec
                )));
            }
            let fs_path = self.storage.resolve_video_fs_path(&library, &video).await?;
            self.engine
                .extract_embedded_subtitle_vtt(Path::new(&fs_path), stream_index)
                .await?
        } else {
            let ext_id = trimmed.strip_prefix("ext-").unwrap_or(trimmed);
            let (fmt, bytes) = self
                .storage
                .read_sidecar_subtitle_bytes(&library, &video, ext_id)
                .await?;
            self.engine.convert_sidecar_subtitle_to_vtt(&fmt, &bytes)
        };

        if let Some(offset) = offset_secs.filter(|&v| v.is_finite() && v > 0.0) {
            Ok(self.engine.shift_webvtt_timestamps(&vtt, offset))
        } else {
            Ok(vtt)
        }
    }

    pub async fn get_poster(
        &self,
        actor: Option<&User>,
        key: &str,
        max_size: Option<u32>,
    ) -> DomainResult<BinaryPayload> {
        if let Some(user) = actor {
            let videos = self
                .catalog
                .query_videos(VideoScope::PosterKey(key))
                .await?;
            let mut matched_pool = false;
            let mut matched_tracked_binding_ids: Vec<Uuid> = Vec::new();
            for v in &videos {
                if v.source.online_tracking_info().is_some() {
                    matched_tracked_binding_ids.push(v.binding_id());
                } else {
                    matched_pool = true;
                    break;
                }
            }
            if !matched_pool {
                let shows = self.catalog.query_shows(ShowScope::PosterKey(key)).await?;
                for s in &shows {
                    if s.online_tracked().is_some() {
                        matched_tracked_binding_ids.push(s.id.0);
                    } else {
                        matched_pool = true;
                        break;
                    }
                }
            }
            if !matched_pool && !matched_tracked_binding_ids.is_empty() {
                let bound = self
                    .bindings
                    .list_bound_ids(user.id, MediaKind::Video)
                    .await?;
                if !matched_tracked_binding_ids
                    .iter()
                    .any(|id| bound.contains(id))
                {
                    return Err(DomainError::NotFound("poster not found".into()));
                }
            }
        }
        self.storage.read_poster(key, max_size).await
    }

    async fn load_bound_video_and_library(
        &self,
        user_id: UserId,
        video_id: VideoId,
    ) -> DomainResult<(VideoItem, Library)> {
        let video = self
            .projector
            .require_video_bound(user_id, video_id)
            .await?;
        let library = self
            .libraries
            .find_by_id(video.library_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("library not found".into()))?;
        Ok((video, library))
    }
}

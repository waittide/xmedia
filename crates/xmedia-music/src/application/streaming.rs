use std::sync::Arc;

use xmedia_core::application::bindings::MediaBindingService;
use xmedia_core::application::ports::{ByteRangeSpec, ByteRangeStream};
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;
use xmedia_core::domain::repository::LibraryRepository;
use xmedia_core::domain::user::UserId;

use crate::application::ports::{
    AudioTranscoderPort, FfmpegStatusDto, MusicStoragePort, TranscodeFormat, TranscodeRequest,
    TranscodedAudioStream,
};
use crate::domain::catalog::{Track, TrackId};
use crate::domain::repository::MusicCatalogRepository;

pub enum ResolvedAudioStream {
    RawRange {
        track: Track,
        range_stream: ByteRangeStream,
        is_partial: bool,
    },
    Transcoded {
        track: Track,
        transcoded: TranscodedAudioStream,
    },
}

#[derive(Clone)]
pub struct MusicStreamingService {
    libraries: Arc<dyn LibraryRepository>,
    catalog: Arc<dyn MusicCatalogRepository>,
    bindings: Arc<MediaBindingService>,
    storage: Arc<dyn MusicStoragePort>,
    transcoder: Arc<dyn AudioTranscoderPort>,
}

impl MusicStreamingService {
    pub fn new(
        libraries: Arc<dyn LibraryRepository>,
        catalog: Arc<dyn MusicCatalogRepository>,
        bindings: Arc<MediaBindingService>,
        storage: Arc<dyn MusicStoragePort>,
        transcoder: Arc<dyn AudioTranscoderPort>,
    ) -> Self {
        Self {
            libraries,
            catalog,
            bindings,
            storage,
            transcoder,
        }
    }

    pub async fn get_ffmpeg_status(&self) -> FfmpegStatusDto {
        self.transcoder.check_status().await
    }

    pub async fn open_raw_download(
        &self,
        user_id: UserId,
        track_id: TrackId,
        spec: ByteRangeSpec,
    ) -> DomainResult<ResolvedAudioStream> {
        self.bindings
            .require_bound(user_id, MediaKind::Music, track_id.0, "track not found")
            .await?;
        let track = self
            .catalog
            .find_track_by_id(track_id)
            .await?
            .filter(|t| !t.is_missing)
            .ok_or_else(|| DomainError::NotFound("track not found".into()))?;
        let library = self
            .libraries
            .find_by_id(track.library_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("library not found".into()))?;

        let range_stream = self
            .storage
            .open_audio_stream(&library, &track, spec)
            .await?;
        Ok(ResolvedAudioStream::RawRange {
            track,
            range_stream,
            is_partial: spec.is_partial(),
        })
    }

    pub async fn open_stream(
        &self,
        user_id: UserId,
        track_id: TrackId,
        requested_format: Option<&str>,
        max_bit_rate_kbps: Option<u32>,
        time_offset_secs: Option<u32>,
        spec: ByteRangeSpec,
    ) -> DomainResult<ResolvedAudioStream> {
        self.bindings
            .require_bound(user_id, MediaKind::Music, track_id.0, "track not found")
            .await?;
        let track = self
            .catalog
            .find_track_by_id(track_id)
            .await?
            .filter(|t| !t.is_missing)
            .ok_or_else(|| DomainError::NotFound("track not found".into()))?;
        let library = self
            .libraries
            .find_by_id(track.library_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("library not found".into()))?;

        let norm_format = requested_format
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_ascii_lowercase);
        let max_br = max_bit_rate_kbps.filter(|&b| b > 0);
        let offset = time_offset_secs.filter(|&t| t > 0);

        let format_matches_source = |fmt: &str| -> bool {
            use crate::domain::catalog::AudioFormat;
            matches!(
                (fmt, track.format),
                ("mp3", AudioFormat::Mp3)
                    | ("flac", AudioFormat::Flac)
                    | ("opus", AudioFormat::Opus)
                    | ("ogg" | "oga", AudioFormat::Ogg)
                    | ("m4a" | "mp4", AudioFormat::M4a)
                    | ("alac", AudioFormat::Alac)
                    | ("aac", AudioFormat::Aac | AudioFormat::M4a)
                    | ("wav" | "wave", AudioFormat::Wav)
                    | ("aiff" | "aif" | "aifc", AudioFormat::Aiff)
                    | ("ape", AudioFormat::Ape)
                    | ("wv" | "wavpack", AudioFormat::Wv)
            )
        };
        let bitrate_fits = max_br.is_none_or(|limit_kbps| {
            !track.format.is_lossless() && track.bit_rate_kbps.is_some_and(|src| src <= limit_kbps)
        });

        let wants_raw = match norm_format.as_deref() {
            Some("raw") => offset.is_none(),
            Some(fmt) => offset.is_none() && format_matches_source(fmt) && bitrate_fits,
            None => offset.is_none() && bitrate_fits,
        };

        if wants_raw {
            let range_stream = self
                .storage
                .open_audio_stream(&library, &track, spec)
                .await?;
            return Ok(ResolvedAudioStream::RawRange {
                track,
                range_stream,
                is_partial: spec.is_partial(),
            });
        }

        let target_format = match norm_format.as_deref() {
            Some(fmt) if fmt != "raw" => TranscodeFormat::parse(fmt)?,
            _ => TranscodeFormat::Mp3,
        };

        let source_path = self.storage.resolve_audio_fs_path(&library, &track).await?;
        let transcoded = self
            .transcoder
            .transcode_stream(
                &source_path,
                track.duration_secs,
                &TranscodeRequest {
                    target_format,
                    max_bit_rate_kbps: max_br,
                    time_offset_secs: offset,
                },
            )
            .await?;

        Ok(ResolvedAudioStream::Transcoded { track, transcoded })
    }
}

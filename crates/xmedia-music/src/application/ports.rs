use std::path::Path;

use async_trait::async_trait;
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use xmedia_core::application::ports::{
    BinaryPayload, ByteChunkStream, ByteRangeSpec, ByteRangeStream,
};
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::Library;

use crate::domain::catalog::{AudioFormat, ReplayGain, StructuredLyrics, Track, TrackId};

#[derive(Debug, Clone)]
pub struct ProbedAudioMetadata {
    pub format: AudioFormat,
    pub title: String,
    pub track_artist: String,
    pub album_artist: String,
    pub album_title: String,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub year: Option<i32>,
    pub genre: Option<String>,
    pub is_compilation: bool,
    pub duration_secs: u32,
    pub bit_rate_kbps: Option<u32>,
    pub sample_rate_hz: Option<u32>,
    pub bit_depth: Option<u16>,
    pub channels: Option<u8>,
    pub replay_gain: ReplayGain,
    pub has_embedded_lyrics: bool,
    pub cover: Option<BinaryPayload>,
}

pub trait AudioMetadataProbePort: Send + Sync {
    fn probe_audio_file(
        &self,
        path: &str,
        ext_hint: &str,
        fallback_title: &str,
    ) -> DomainResult<ProbedAudioMetadata>;

    fn probe_audio_bytes(
        &self,
        bytes: &[u8],
        ext_hint: &str,
        fallback_title: &str,
    ) -> DomainResult<ProbedAudioMetadata>;
}

pub trait LyricsResolverPort: Send + Sync {
    fn resolve_lyrics(
        &self,
        audio_path: Option<&str>,
        sidecar_lrc_bytes: Option<&[u8]>,
    ) -> DomainResult<StructuredLyrics>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TranscodeFormat {
    Mp3,
    Opus,
    Aac,
    Flac,
    Wav,
}

impl TranscodeFormat {
    pub fn parse(raw: &str) -> DomainResult<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "mp3" => Ok(Self::Mp3),
            "opus" | "ogg" | "oga" => Ok(Self::Opus),
            "aac" | "m4a" | "mp4" => Ok(Self::Aac),
            "flac" | "alac" | "ape" | "wv" | "wavpack" => Ok(Self::Flac),
            "wav" | "wave" | "aiff" | "aif" | "aifc" => Ok(Self::Wav),
            other => Err(DomainError::UnsupportedFormat(format!(
                "unsupported transcode target format: {other}"
            ))),
        }
    }

    pub fn mime_type(self) -> &'static str {
        match self {
            Self::Mp3 => "audio/mpeg",
            Self::Opus => "audio/ogg",
            Self::Aac => "audio/aac",
            Self::Flac => "audio/flac",
            Self::Wav => "audio/wav",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Mp3 => "mp3",
            Self::Opus => "opus",
            Self::Aac => "aac",
            Self::Flac => "flac",
            Self::Wav => "wav",
        }
    }
}

pub use xmedia_core::application::dto::FfmpegStatusDto;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscodeRequest {
    pub target_format: TranscodeFormat,
    pub max_bit_rate_kbps: Option<u32>,
    pub time_offset_secs: Option<u32>,
}

pub struct TranscodedAudioStream {
    pub mime_type: String,
    pub content_extension: String,
    pub estimated_content_length: Option<u64>,
    pub stream: ByteChunkStream,
}

#[async_trait]
pub trait AudioTranscoderPort: Send + Sync {
    async fn check_status(&self) -> FfmpegStatusDto;

    async fn transcode_stream(
        &self,
        source_path: &str,
        duration_secs: u32,
        request: &TranscodeRequest,
    ) -> DomainResult<TranscodedAudioStream>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredManagedAudio {
    pub storage_key: String,
    pub file_hash: String,
    pub file_size: u64,
}

#[async_trait]
pub trait MusicStoragePort: Send + Sync {
    async fn save_managed_audio(
        &self,
        track_id: TrackId,
        format: AudioFormat,
        data: Bytes,
    ) -> DomainResult<StoredManagedAudio>;

    async fn save_managed_audio_from_path(
        &self,
        track_id: TrackId,
        format: AudioFormat,
        src_path: &Path,
    ) -> DomainResult<StoredManagedAudio> {
        let data = tokio::fs::read(src_path).await.map_err(|e| {
            DomainError::Storage(format!("failed reading temp uploaded audio: {e}"))
        })?;
        self.save_managed_audio(track_id, format, Bytes::from(data))
            .await
    }

    async fn save_managed_sidecar_lrc(
        &self,
        track_id: TrackId,
        data: Bytes,
    ) -> DomainResult<String>;

    async fn save_managed_sidecar_lrc_from_path(
        &self,
        track_id: TrackId,
        src_path: &Path,
    ) -> DomainResult<String> {
        let data = tokio::fs::read(src_path)
            .await
            .map_err(|e| DomainError::Storage(format!("failed reading temp uploaded lrc: {e}")))?;
        self.save_managed_sidecar_lrc(track_id, Bytes::from(data))
            .await
    }

    async fn delete_managed_file(&self, storage_key: &str) -> DomainResult<()>;

    async fn save_cover(&self, cover_id: &str, cover: &BinaryPayload) -> DomainResult<String>;

    async fn read_cover(
        &self,
        cover_key: &str,
        max_size: Option<u32>,
    ) -> DomainResult<BinaryPayload>;

    async fn resolve_audio_fs_path(&self, library: &Library, track: &Track)
    -> DomainResult<String>;

    async fn read_sidecar_lrc_for_track(
        &self,
        library: &Library,
        track: &Track,
    ) -> DomainResult<Option<Bytes>>;

    async fn open_audio_stream(
        &self,
        library: &Library,
        track: &Track,
        spec: ByteRangeSpec,
    ) -> DomainResult<ByteRangeStream>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedAudioEntry {
    pub absolute_path: String,
    pub relative_path: String,
    pub ext_hint: String,
    pub file_size: u64,
    pub file_mtime: i64,
    pub fallback_title: String,
    pub sidecar_lrc_relative_path: Option<String>,
    pub directory_cover_path: Option<String>,
}

#[async_trait]
pub trait MusicScannerPort: Send + Sync {
    async fn scan_audio_entries(&self, root_path: &str) -> DomainResult<Vec<ScannedAudioEntry>>;
    async fn read_directory_cover(
        &self,
        absolute_cover_path: &str,
    ) -> DomainResult<Option<BinaryPayload>>;
    async fn compute_file_fingerprint(&self, absolute_path: &str) -> DomainResult<String>;
}

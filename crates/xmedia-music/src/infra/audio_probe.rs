use std::path::Path;

use xmedia_core::application::ports::BinaryPayload;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_formats::error::FormatError;
use xmedia_formats::music::{AudioFormatExt, ParsedAudioMeta, probe_audio_bytes, probe_audio_path};

use crate::application::ports::{AudioMetadataProbePort, ProbedAudioMetadata};
use crate::domain::catalog::{AudioFormat, ReplayGain};

#[derive(Default)]
pub struct LoftyAudioProbe;

impl AudioMetadataProbePort for LoftyAudioProbe {
    fn probe_audio_file(
        &self,
        path: &str,
        ext_hint: &str,
        fallback_title: &str,
    ) -> DomainResult<ProbedAudioMetadata> {
        let parsed = probe_audio_path(Path::new(path), ext_hint, fallback_title)
            .map_err(map_format_error)?;
        Ok(map_parsed_audio_meta(parsed))
    }

    fn probe_audio_bytes(
        &self,
        bytes: &[u8],
        ext_hint: &str,
        fallback_title: &str,
    ) -> DomainResult<ProbedAudioMetadata> {
        let parsed =
            probe_audio_bytes(bytes, ext_hint, fallback_title).map_err(map_format_error)?;
        Ok(map_parsed_audio_meta(parsed))
    }
}

pub(crate) fn map_audio_format_ext(ext: AudioFormatExt) -> AudioFormat {
    match ext {
        AudioFormatExt::Flac => AudioFormat::Flac,
        AudioFormatExt::Mp3 => AudioFormat::Mp3,
        AudioFormatExt::M4a => AudioFormat::M4a,
        AudioFormatExt::Alac => AudioFormat::Alac,
        AudioFormatExt::Aac => AudioFormat::Aac,
        AudioFormatExt::Ogg => AudioFormat::Ogg,
        AudioFormatExt::Opus => AudioFormat::Opus,
        AudioFormatExt::Wav => AudioFormat::Wav,
        AudioFormatExt::Aiff => AudioFormat::Aiff,
        AudioFormatExt::Ape => AudioFormat::Ape,
        AudioFormatExt::Wv => AudioFormat::Wv,
    }
}

fn map_format_error(err: FormatError) -> DomainError {
    match err {
        FormatError::Validation(msg) => DomainError::Validation(msg),
        FormatError::UnsupportedFormat(msg) => DomainError::UnsupportedFormat(msg),
        FormatError::NotFound(msg) => DomainError::NotFound(msg),
        FormatError::Conflict(msg) => DomainError::Conflict(msg),
        FormatError::Io(msg) => DomainError::Storage(msg),
    }
}

fn map_parsed_audio_meta(parsed: ParsedAudioMeta) -> ProbedAudioMetadata {
    ProbedAudioMetadata {
        format: map_audio_format_ext(parsed.format),
        title: parsed.title,
        track_artist: parsed.track_artist,
        album_artist: parsed.effective_album_artist,
        album_title: parsed.album_title,
        track_number: parsed.track_number,
        disc_number: parsed.disc_number,
        year: parsed.year,
        genre: parsed.genre,
        is_compilation: parsed.is_compilation,
        duration_secs: parsed.duration_secs,
        bit_rate_kbps: parsed.bit_rate_kbps,
        sample_rate_hz: parsed.sample_rate_hz,
        bit_depth: parsed.bit_depth,
        channels: parsed.channels,
        replay_gain: ReplayGain {
            track_gain_db: parsed.replay_gain.track_gain_db,
            track_peak: parsed.replay_gain.track_peak,
            album_gain_db: parsed.replay_gain.album_gain_db,
            album_peak: parsed.replay_gain.album_peak,
        },
        has_embedded_lyrics: parsed.has_embedded_lyrics,
        cover: parsed.cover.map(|c| BinaryPayload {
            bytes: c.bytes,
            mime_type: c.mime_type,
        }),
    }
}

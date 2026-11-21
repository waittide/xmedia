use async_trait::async_trait;
use tracing::debug;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::infra::ffmpeg::FfmpegToolkit;

use crate::application::ports::{
    AudioTranscoderPort, FfmpegStatusDto, TranscodeFormat, TranscodeRequest, TranscodedAudioStream,
};

#[derive(Clone)]
pub struct FfmpegAudioTranscoder {
    ffmpeg: FfmpegToolkit,
}

impl FfmpegAudioTranscoder {
    pub fn new(ffmpeg: FfmpegToolkit) -> Self {
        Self { ffmpeg }
    }
}

#[async_trait]
impl AudioTranscoderPort for FfmpegAudioTranscoder {
    async fn check_status(&self) -> FfmpegStatusDto {
        self.ffmpeg.check_status().await
    }

    async fn transcode_stream(
        &self,
        source_path: &str,
        duration_secs: u32,
        request: &TranscodeRequest,
    ) -> DomainResult<TranscodedAudioStream> {
        let mut args: Vec<String> = Vec::new();

        let offset_secs = request.time_offset_secs.unwrap_or(0);
        if offset_secs > 0 {
            args.push("-ss".into());
            args.push(offset_secs.to_string());
        }

        args.extend([
            "-i".into(),
            source_path.to_string(),
            "-vn".into(),
            "-map".into(),
            "0:a:0".into(),
        ]);

        let effective_kbps: Option<u32> = match request.target_format {
            TranscodeFormat::Mp3 => {
                let kbps = request.max_bit_rate_kbps.unwrap_or(192).clamp(32, 320);
                args.extend([
                    "-c:a".into(),
                    "libmp3lame".into(),
                    "-b:a".into(),
                    format!("{kbps}k"),
                    "-f".into(),
                    "mp3".into(),
                ]);
                Some(kbps)
            }
            TranscodeFormat::Opus => {
                let kbps = request.max_bit_rate_kbps.unwrap_or(128).clamp(24, 320);
                args.extend([
                    "-c:a".into(),
                    "libopus".into(),
                    "-b:a".into(),
                    format!("{kbps}k"),
                    "-vbr".into(),
                    "on".into(),
                    "-f".into(),
                    "ogg".into(),
                ]);
                Some(kbps)
            }
            TranscodeFormat::Aac => {
                let kbps = request.max_bit_rate_kbps.unwrap_or(256).clamp(32, 320);
                args.extend([
                    "-c:a".into(),
                    "aac".into(),
                    "-b:a".into(),
                    format!("{kbps}k"),
                    "-f".into(),
                    "adts".into(),
                ]);
                Some(kbps)
            }
            TranscodeFormat::Flac => {
                args.extend(["-c:a".into(), "flac".into(), "-f".into(), "flac".into()]);
                None
            }
        };

        args.push("pipe:1".into());

        debug!(
            source = source_path,
            format = ?request.target_format,
            bitrate = ?effective_kbps,
            offset = offset_secs,
            "spawning ffmpeg real-time audio transcoder"
        );

        let stream = self.ffmpeg.spawn_stdout_stream(&args)?;

        let remaining_secs = duration_secs.saturating_sub(offset_secs);
        let estimated_content_length = effective_kbps.and_then(|kbps| {
            if remaining_secs > 0 {
                Some((u64::from(remaining_secs) * u64::from(kbps) * 1000) / 8)
            } else {
                None
            }
        });

        Ok(TranscodedAudioStream {
            mime_type: request.target_format.mime_type().to_string(),
            content_extension: request.target_format.extension().to_string(),
            estimated_content_length,
            stream,
        })
    }
}

use std::ffi::OsString;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use async_trait::async_trait;
use tokio::process::Command;
use tracing::{debug, warn};
use xmedia_core::application::dto::FfmpegStatusDto;
use xmedia_core::application::ports::ByteChunkStream;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::infra::ffmpeg::FfmpegToolkit;
use xmedia_formats::video::{convert_subtitle_bytes_to_vtt, shift_webvtt_timestamps};

use crate::application::ports::{
    ProbedVideoInfo, VideoMediaEnginePort, VideoStreamTranscodeRequest, VideoTranscodeMode,
};

pub(crate) const SAFE_HLS_ALLOWED_EXTENSIONS: &str =
    "ts,m4s,mp4,m4a,aac,mp3,ac3,eac3,vtt,bin,m3u8,key";
pub(crate) const SAFE_HLS_PROTOCOL_WHITELIST: &str = "file,crypto";

pub(crate) fn build_remux_local_hls_args(
    local_playlist_path: &Path,
    output_mp4_path: &Path,
) -> Vec<OsString> {
    vec![
        "-y".into(),
        "-v".into(),
        "error".into(),
        "-allowed_extensions".into(),
        SAFE_HLS_ALLOWED_EXTENSIONS.into(),
        "-protocol_whitelist".into(),
        SAFE_HLS_PROTOCOL_WHITELIST.into(),
        "-i".into(),
        local_playlist_path.as_os_str().to_os_string(),
        "-c".into(),
        "copy".into(),
        "-bsf:a".into(),
        "aac_adtstoasc".into(),
        "-movflags".into(),
        "+faststart".into(),
        "-f".into(),
        "mp4".into(),
        output_mp4_path.as_os_str().to_os_string(),
    ]
}

pub(crate) fn sanitize_ffmpeg_remux_error(stderr: &str, exit_code: Option<i32>) -> String {
    let lower = stderr.to_ascii_lowercase();
    let reason = if lower.contains("protocol not on whitelist")
        || lower.contains("not in allowed_extensions")
    {
        "安全策略拒绝了不受支持的协议或扩展名"
    } else if lower.contains("invalid data found")
        || lower.contains("error when loading first segment")
        || lower.contains("moov atom not found")
    {
        "HLS 媒体分片数据损坏或编码不受支持"
    } else if lower.contains("permission denied") || lower.contains("no such file") {
        "读取本地 HLS 缓存分片失败"
    } else {
        "媒体流封装失败"
    };
    match exit_code {
        Some(code) => format!("FFmpeg 封装 HLS 失败: {reason} (exit {code})"),
        None => format!("FFmpeg 封装 HLS 失败: {reason}"),
    }
}

#[derive(Clone)]
pub struct FfmpegVideoEngine {
    ffmpeg: FfmpegToolkit,
}

impl FfmpegVideoEngine {
    pub fn new(ffmpeg: FfmpegToolkit) -> Self {
        Self { ffmpeg }
    }
}

#[async_trait]
impl VideoMediaEnginePort for FfmpegVideoEngine {
    async fn check_status(&self) -> FfmpegStatusDto {
        self.ffmpeg.check_status().await
    }

    async fn probe_video(&self, fs_path: &Path) -> ProbedVideoInfo {
        let probed = self.ffmpeg.probe_media(fs_path).await.unwrap_or_default();
        ProbedVideoInfo {
            duration_secs: probed.duration_secs,
            bit_rate_kbps: probed.bit_rate_kbps,
            width: probed.width,
            height: probed.height,
            video_codec: probed.video_codec,
            container_format: probed.container_format,
            audio_streams: probed.audio_streams,
            subtitle_streams: probed.subtitle_streams,
        }
    }

    async fn generate_poster_bytes(
        &self,
        fs_path: &Path,
        duration_secs: u32,
        fallback_title: &str,
    ) -> DomainResult<Vec<u8>> {
        let seek_secs = if duration_secs >= 20 {
            (duration_secs / 7).clamp(2, 120)
        } else {
            0
        };
        if let Ok(jpeg) = self
            .ffmpeg
            .extract_video_frame_jpeg(fs_path, seek_secs, 640)
            .await
            && !jpeg.is_empty()
        {
            return Ok(jpeg);
        }

        Ok(generate_svg_poster(fallback_title).into_bytes())
    }

    async fn extract_embedded_subtitle_vtt(
        &self,
        fs_path: &Path,
        stream_index: u32,
    ) -> DomainResult<String> {
        self.ffmpeg
            .extract_embedded_subtitle_webvtt(fs_path, stream_index)
            .await
    }

    fn convert_sidecar_subtitle_to_vtt(&self, format: &str, raw_bytes: &[u8]) -> String {
        convert_subtitle_bytes_to_vtt(format, raw_bytes)
    }

    fn shift_webvtt_timestamps(&self, vtt: &str, offset_secs: f64) -> String {
        shift_webvtt_timestamps(vtt, offset_secs)
    }

    async fn open_fmp4_transcode_stream(
        &self,
        fs_path: &Path,
        req: &VideoStreamTranscodeRequest,
    ) -> DomainResult<ByteChunkStream> {
        let mut args: Vec<String> = Vec::new();
        if let Some(start) = req.start_secs.filter(|&s| s > 0) {
            args.push("-ss".into());
            args.push(start.to_string());
        }

        args.extend([
            "-i".into(),
            fs_path.to_string_lossy().into_owned(),
            "-map".into(),
            "0:v:0".into(),
        ]);

        if let Some(a_idx) = req.audio_stream_index {
            args.extend(["-map".into(), format!("0:{a_idx}?")]);
        } else {
            args.extend(["-map".into(), "0:a:0?".into()]);
        }

        match req.mode {
            VideoTranscodeMode::RemuxVideoTranscodeAudio => {
                args.extend(["-c:v".into(), "copy".into()]);
            }
            VideoTranscodeMode::FullTranscode { max_height } => {
                args.extend([
                    "-c:v".into(),
                    "libx264".into(),
                    "-preset".into(),
                    "veryfast".into(),
                    "-crf".into(),
                    "23".into(),
                    "-pix_fmt".into(),
                    "yuv420p".into(),
                ]);
                if let Some(h) = max_height {
                    args.extend(["-vf".into(), format!("scale=-2:'min({h},ih)'")]);
                }
            }
        }

        args.extend([
            "-c:a".into(),
            "aac".into(),
            "-ac".into(),
            "2".into(),
            "-b:a".into(),
            "192k".into(),
            "-sn".into(),
            "-movflags".into(),
            "frag_keyframe+empty_moov+default_base_moof".into(),
            "-f".into(),
            "mp4".into(),
            "pipe:1".into(),
        ]);

        debug!(
            path = %fs_path.display(),
            mode = ?req.mode,
            audio = ?req.audio_stream_index,
            start = ?req.start_secs,
            "spawning ffmpeg real-time fMP4 video stream"
        );
        self.ffmpeg.spawn_stdout_stream(&args)
    }

    async fn remux_local_hls_to_mp4(
        &self,
        local_playlist_path: &Path,
        output_mp4_path: &Path,
    ) -> DomainResult<()> {
        let args = build_remux_local_hls_args(local_playlist_path, output_mp4_path);
        let output = tokio::time::timeout(
            Duration::from_secs(900),
            Command::new(self.ffmpeg.ffmpeg_bin())
                .kill_on_drop(true)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .args(args)
                .output(),
        )
        .await
        .map_err(|_| DomainError::Storage("FFmpeg 封装 HLS 超时".into()))?
        .map_err(|e| DomainError::Storage(format!("启动 FFmpeg 封装 HLS 失败: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!(
                exit_code = ?output.status.code(),
                "ffmpeg local hls remux failed"
            );
            return Err(DomainError::Storage(sanitize_ffmpeg_remux_error(
                &stderr,
                output.status.code(),
            )));
        }
        Ok(())
    }
}

fn generate_svg_poster(title: &str) -> String {
    let escaped: String = title
        .chars()
        .take(36)
        .map(|c| match c {
            '&' => "&amp;".to_string(),
            '<' => "&lt;".to_string(),
            '>' => "&gt;".to_string(),
            '"' => "&quot;".to_string(),
            other => other.to_string(),
        })
        .collect();
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 360"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0%" stop-color="#1e1e2e"/><stop offset="100%" stop-color="#313244"/></linearGradient></defs><rect width="640" height="360" fill="url(#g)"/><polygon points="295,140 295,220 365,180" fill="#89b4fa" opacity="0.85"/><text x="320" y="290" fill="#cdd6f4" font-family="system-ui,sans-serif" font-size="22" font-weight="600" text-anchor="middle">{escaped}</text></svg>"##
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remux_local_hls_args_disallow_all_extensions_and_data_protocol() {
        let args: Vec<String> =
            build_remux_local_hls_args(Path::new("/tmp/hls/index.m3u8"), Path::new("/tmp/out.mp4"))
                .into_iter()
                .map(|s| s.to_string_lossy().into_owned())
                .collect();

        let ext_idx = args
            .iter()
            .position(|a| a == "-allowed_extensions")
            .expect("must pass -allowed_extensions");
        assert_eq!(
            args[ext_idx + 1],
            "ts,m4s,mp4,m4a,aac,mp3,ac3,eac3,vtt,bin,m3u8,key"
        );
        assert!(!args[ext_idx + 1].eq_ignore_ascii_case("ALL"));

        let proto_idx = args
            .iter()
            .position(|a| a == "-protocol_whitelist")
            .expect("must pass -protocol_whitelist");
        assert_eq!(args[proto_idx + 1], "file,crypto");
        assert!(!args[proto_idx + 1].contains("data"));
    }

    #[test]
    fn sanitize_ffmpeg_remux_error_never_leaks_paths_or_file_contents() {
        let leaked_stderr = "[hls @ 0x55a1] Error when loading first segment '/etc/passwd'\nroot:x:0:0:root:/root:/bin/bash\n/tmp/xmedia-hls-secret/index.m3u8: Invalid data found when processing input";
        let sanitized = sanitize_ffmpeg_remux_error(leaked_stderr, Some(1));
        assert!(!sanitized.contains("/etc/passwd"));
        assert!(!sanitized.contains("root:x:0:0"));
        assert!(!sanitized.contains("/tmp/xmedia-hls-secret"));
        assert!(sanitized.contains("FFmpeg 封装 HLS 失败"));
    }
}

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use bytes::{Bytes, BytesMut};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::sync::mpsc;
use tracing::debug;

use crate::application::dto::FfmpegStatusDto;
use crate::application::ports::ByteChunkStream;
use crate::domain::error::{DomainError, DomainResult};
pub use crate::domain::media::{ProbedAudioStream, ProbedSubtitleStream};
use crate::infra::stream::MpscByteStream;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProbedMediaInfo {
    pub duration_secs: u32,
    pub bit_rate_kbps: Option<u32>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub video_codec: Option<String>,
    pub container_format: Option<String>,
    pub audio_streams: Vec<ProbedAudioStream>,
    pub subtitle_streams: Vec<ProbedSubtitleStream>,
}

#[derive(Clone)]
pub struct FfmpegToolkit {
    ffmpeg_bin: String,
    ffprobe_bin: String,
}

impl FfmpegToolkit {
    pub fn new(ffmpeg_bin: impl Into<String>, ffprobe_bin: impl Into<String>) -> Self {
        Self {
            ffmpeg_bin: ffmpeg_bin.into(),
            ffprobe_bin: ffprobe_bin.into(),
        }
    }

    pub fn from_ffmpeg_bin(ffmpeg_bin: impl Into<String>) -> Self {
        let ffmpeg = ffmpeg_bin.into();
        let default_probe = if cfg!(windows) {
            "ffprobe.exe"
        } else {
            "ffprobe"
        };
        let ffprobe = if ffmpeg == "ffmpeg" || ffmpeg == "ffmpeg.exe" {
            default_probe.to_string()
        } else if let Some(parent) = Path::new(&ffmpeg)
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
        {
            let candidate = parent.join(default_probe);
            if candidate.is_file() {
                candidate.to_string_lossy().to_string()
            } else {
                default_probe.to_string()
            }
        } else {
            default_probe.to_string()
        };
        Self::new(ffmpeg, ffprobe)
    }

    pub fn ffmpeg_bin(&self) -> &str {
        &self.ffmpeg_bin
    }

    pub fn ffprobe_bin(&self) -> &str {
        &self.ffprobe_bin
    }

    pub async fn check_status(&self) -> FfmpegStatusDto {
        #[cfg(any(target_os = "android", target_os = "ios"))]
        {
            FfmpegStatusDto {
                ffmpeg_available: false,
                ffmpeg_version: None,
                ffprobe_available: false,
            }
        }

        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        {
            let ffmpeg_fut = Command::new(&self.ffmpeg_bin)
                .kill_on_drop(true)
                .arg("-version")
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .output();

            let (ffmpeg_available, ffmpeg_version) =
                match tokio::time::timeout(Duration::from_secs(3), ffmpeg_fut).await {
                    Ok(Ok(out)) if out.status.success() => {
                        let text = String::from_utf8_lossy(&out.stdout);
                        let first = text.lines().next().map(|l| l.trim().to_string());
                        (true, first)
                    }
                    _ => (false, None),
                };

            let ffprobe_fut = Command::new(&self.ffprobe_bin)
                .kill_on_drop(true)
                .arg("-version")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();

            let ffprobe_available = matches!(
                tokio::time::timeout(Duration::from_secs(3), ffprobe_fut).await,
                Ok(Ok(status)) if status.success()
            );

            FfmpegStatusDto {
                ffmpeg_available,
                ffmpeg_version,
                ffprobe_available,
            }
        }
    }

    pub async fn probe_media(&self, path: &Path) -> DomainResult<ProbedMediaInfo> {
        #[cfg(any(target_os = "android", target_os = "ios"))]
        {
            let _ = path;
            return Err(DomainError::Storage(
                "ffprobe subprocess execution is disabled on mobile platforms".into(),
            ));
        }

        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        {
            let fut = Command::new(&self.ffprobe_bin)
                .kill_on_drop(true)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .args([
                    "-v",
                    "quiet",
                    "-print_format",
                    "json",
                    "-show_format",
                    "-show_streams",
                ])
                .arg(path)
                .output();

            let out = tokio::time::timeout(Duration::from_secs(15), fut)
                .await
                .map_err(|_| DomainError::Storage("ffprobe timed out".into()))?
                .map_err(|e| DomainError::Storage(format!("failed to invoke ffprobe: {e}")))?;

            if !out.status.success() {
                return Err(DomainError::Storage(
                    "ffprobe exited with non-zero status".into(),
                ));
            }

            let raw: RawFfprobeOutput = serde_json::from_slice(&out.stdout)
                .map_err(|e| DomainError::Storage(format!("invalid ffprobe JSON: {e}")))?;

            let duration_secs = raw
                .format
                .as_ref()
                .and_then(|f| f.duration.as_deref())
                .and_then(|d| d.parse::<f64>().ok())
                .map(|d| d.round().max(0.0) as u32)
                .unwrap_or(0);

            let bit_rate_kbps = raw
                .format
                .as_ref()
                .and_then(|f| f.bit_rate.as_deref())
                .and_then(|b| b.parse::<u64>().ok())
                .map(|b| (b / 1000) as u32);

            let container_format = raw.format.and_then(|f| f.format_name);

            let mut width = None;
            let mut height = None;
            let mut video_codec = None;
            let mut audio_streams = Vec::new();
            let mut subtitle_streams = Vec::new();

            for s in raw.streams.unwrap_or_default() {
                let codec = s.codec_name.unwrap_or_else(|| "unknown".into());
                let is_default = s.disposition.as_ref().and_then(|d| d.default).unwrap_or(0) == 1;
                let language = s.tags.as_ref().and_then(|t| t.language.clone());
                let title = s.tags.as_ref().and_then(|t| t.title.clone());

                match s.codec_type.as_deref() {
                    Some("video") => {
                        // Ignore attached cover art streams when picking primary video stream
                        let attached_pic = s
                            .disposition
                            .as_ref()
                            .and_then(|d| d.attached_pic)
                            .unwrap_or(0)
                            == 1;
                        if !attached_pic && video_codec.is_none() {
                            video_codec = Some(codec);
                            width = s.width;
                            height = s.height;
                        }
                    }
                    Some("audio") => {
                        audio_streams.push(ProbedAudioStream {
                            stream_index: s.index.unwrap_or(0),
                            codec,
                            channels: s.channels.unwrap_or(2).min(16) as u8,
                            language,
                            title,
                            is_default,
                        });
                    }
                    Some("subtitle") => {
                        let is_text_based = matches!(
                            codec.as_str(),
                            "subrip" | "srt" | "ass" | "ssa" | "webvtt" | "mov_text" | "text"
                        );
                        subtitle_streams.push(ProbedSubtitleStream {
                            stream_index: s.index.unwrap_or(0),
                            codec,
                            language,
                            title,
                            is_text_based,
                            is_default,
                        });
                    }
                    _ => {}
                }
            }

            Ok(ProbedMediaInfo {
                duration_secs,
                bit_rate_kbps,
                width,
                height,
                video_codec,
                container_format,
                audio_streams,
                subtitle_streams,
            })
        }
    }

    pub async fn extract_video_frame_jpeg(
        &self,
        path: &Path,
        seek_secs: u32,
        max_width: u32,
    ) -> DomainResult<Vec<u8>> {
        let vf = format!("scale='min({max_width},iw)':-2");
        let fut = Command::new(&self.ffmpeg_bin)
            .kill_on_drop(true)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .arg("-nostdin")
            .arg("-v")
            .arg("error")
            .arg("-ss")
            .arg(seek_secs.to_string())
            .arg("-i")
            .arg(path)
            .args([
                "-frames:v",
                "1",
                "-vf",
                &vf,
                "-f",
                "image2pipe",
                "-vcodec",
                "mjpeg",
                "pipe:1",
            ])
            .output();

        let out = tokio::time::timeout(Duration::from_secs(15), fut)
            .await
            .map_err(|_| DomainError::Storage("ffmpeg frame extraction timed out".into()))?
            .map_err(|e| DomainError::Storage(format!("failed to invoke ffmpeg: {e}")))?;

        if !out.status.success() || out.stdout.is_empty() {
            return Err(DomainError::Storage(
                "ffmpeg failed to extract video frame".into(),
            ));
        }
        Ok(out.stdout)
    }

    pub async fn extract_embedded_subtitle_webvtt(
        &self,
        path: &Path,
        stream_index: u32,
    ) -> DomainResult<String> {
        let map_arg = format!("0:{stream_index}");
        let fut = Command::new(&self.ffmpeg_bin)
            .kill_on_drop(true)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .arg("-nostdin")
            .arg("-v")
            .arg("error")
            .arg("-i")
            .arg(path)
            .args(["-map", &map_arg, "-c:s", "webvtt", "-f", "webvtt", "pipe:1"])
            .output();

        let out = tokio::time::timeout(Duration::from_secs(20), fut)
            .await
            .map_err(|_| DomainError::Storage("ffmpeg subtitle extraction timed out".into()))?
            .map_err(|e| DomainError::Storage(format!("failed to invoke ffmpeg: {e}")))?;

        if !out.status.success() {
            return Err(DomainError::Storage(
                "ffmpeg failed to convert embedded subtitle to WebVTT".into(),
            ));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    pub fn spawn_stdout_stream(&self, args: &[String]) -> DomainResult<ByteChunkStream> {
        let mut cmd = Command::new(&self.ffmpeg_bin);
        cmd.kill_on_drop(true)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .args(["-nostdin", "-v", "error"])
            .args(args);

        let mut child = cmd.spawn().map_err(|e| {
            DomainError::Storage(format!(
                "failed to spawn ffmpeg ('{}'): {e}",
                self.ffmpeg_bin
            ))
        })?;

        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| DomainError::Storage("ffmpeg stdout pipe missing".into()))?;

        let (tx, rx) = mpsc::channel::<DomainResult<Bytes>>(16);

        tokio::spawn(async move {
            const CHUNK_SIZE: usize = 32 * 1024;
            let mut buf = BytesMut::with_capacity(CHUNK_SIZE);
            loop {
                if buf.capacity() < CHUNK_SIZE / 2 {
                    buf.reserve(CHUNK_SIZE);
                }
                let read_res = tokio::select! {
                    biased;
                    _ = tx.closed() => {
                        debug!("client disconnected while waiting on ffmpeg stdout, terminating process");
                        let _ = child.kill().await;
                        break;
                    }
                    res = stdout.read_buf(&mut buf) => res,
                };
                match read_res {
                    Ok(0) => {
                        let _ = child.wait().await;
                        break;
                    }
                    Ok(_) => {
                        let chunk = buf.split().freeze();
                        let send_ok = tokio::select! {
                            biased;
                            _ = tx.closed() => false,
                            res = tx.send(Ok(chunk)) => res.is_ok(),
                        };
                        if !send_ok {
                            debug!("client disconnected, terminating ffmpeg process");
                            let _ = child.kill().await;
                            break;
                        }
                    }
                    Err(e) => {
                        let _ = tx
                            .send(Err(DomainError::Storage(format!(
                                "ffmpeg stream read error: {e}"
                            ))))
                            .await;
                        let _ = child.kill().await;
                        break;
                    }
                }
            }
        });

        Ok(MpscByteStream::into_boxed(rx))
    }
}

impl Default for FfmpegToolkit {
    fn default() -> Self {
        Self::new("ffmpeg", "ffprobe")
    }
}

#[derive(Debug, Deserialize)]
struct RawFfprobeOutput {
    format: Option<RawFfprobeFormat>,
    streams: Option<Vec<RawFfprobeStream>>,
}

#[derive(Debug, Deserialize)]
struct RawFfprobeFormat {
    format_name: Option<String>,
    duration: Option<String>,
    bit_rate: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawFfprobeStream {
    index: Option<u32>,
    codec_type: Option<String>,
    codec_name: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    channels: Option<u32>,
    disposition: Option<RawFfprobeDisposition>,
    tags: Option<RawFfprobeTags>,
}

#[derive(Debug, Deserialize)]
struct RawFfprobeDisposition {
    default: Option<i32>,
    attached_pic: Option<i32>,
}

#[derive(Debug, Deserialize)]
struct RawFfprobeTags {
    language: Option<String>,
    title: Option<String>,
}

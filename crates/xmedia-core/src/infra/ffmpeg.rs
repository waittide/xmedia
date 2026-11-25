#[cfg(not(any(target_os = "android", target_os = "ios")))]
use std::ffi::OsString;
use std::path::Path;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use std::process::Stdio;
use std::sync::Arc;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use std::time::Duration;

#[cfg(not(any(target_os = "android", target_os = "ios")))]
use bytes::{Bytes, BytesMut};
use serde::{Deserialize, Serialize};
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use tokio::io::AsyncReadExt;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use tokio::process::Command;
use tokio::sync::Semaphore;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use tokio::sync::mpsc;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use tracing::debug;

#[cfg(not(any(target_os = "android", target_os = "ios")))]
const MAX_FFPROBE_STDOUT_BYTES: usize = 16 * 1024 * 1024;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
const MAX_FFMPEG_FRAME_STDOUT_BYTES: usize = 32 * 1024 * 1024;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
const MAX_FFMPEG_SUBTITLE_STDOUT_BYTES: usize = 32 * 1024 * 1024;

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn local_file_input_arg(path: &Path) -> OsString {
    let mut arg = OsString::from("file:");
    arg.push(path.as_os_str());
    arg
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
async fn run_command_bounded_stdout(
    cmd: &mut Command,
    timeout: Duration,
    max_stdout_bytes: usize,
    label: &str,
) -> DomainResult<(std::process::ExitStatus, Vec<u8>)> {
    let mut child = cmd
        .kill_on_drop(true)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| DomainError::Storage(format!("failed to invoke {label}: {e}")))?;

    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| DomainError::Storage(format!("{label} stdout pipe missing")))?;

    let run_fut = async move {
        let mut out = Vec::new();
        let mut buf = [0u8; 16 * 1024];
        loop {
            let n = stdout
                .read(&mut buf)
                .await
                .map_err(|e| DomainError::Storage(format!("{label} stdout read error: {e}")))?;
            if n == 0 {
                break;
            }
            if out.len().saturating_add(n) > max_stdout_bytes {
                let _ = child.kill().await;
                return Err(DomainError::Storage(format!(
                    "{label} output exceeded limit ({max_stdout_bytes} bytes)"
                )));
            }
            out.extend_from_slice(&buf[..n]);
        }
        let status = child
            .wait()
            .await
            .map_err(|e| DomainError::Storage(format!("failed to wait on {label}: {e}")))?;
        Ok((status, out))
    };

    tokio::time::timeout(timeout, run_fut)
        .await
        .map_err(|_| DomainError::Storage(format!("{label} timed out")))?
}

use crate::application::dto::FfmpegStatusDto;
use crate::application::ports::ByteChunkStream;
use crate::domain::error::{DomainError, DomainResult};
pub use crate::domain::media::{ProbedAudioStream, ProbedSubtitleStream};
#[cfg(not(any(target_os = "android", target_os = "ios")))]
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

fn default_transcode_concurrency() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get().clamp(4, 16))
        .unwrap_or(8)
}

#[derive(Clone)]
pub struct FfmpegToolkit {
    ffmpeg_bin: String,
    ffprobe_bin: String,
    transcode_semaphore: Arc<Semaphore>,
}

impl FfmpegToolkit {
    pub fn new(ffmpeg_bin: impl Into<String>, ffprobe_bin: impl Into<String>) -> Self {
        let max_streams = default_transcode_concurrency();
        Self {
            ffmpeg_bin: ffmpeg_bin.into(),
            ffprobe_bin: ffprobe_bin.into(),
            transcode_semaphore: Arc::new(Semaphore::new(max_streams)),
        }
    }

    pub fn with_max_concurrent_streams(mut self, max_streams: usize) -> Self {
        self.transcode_semaphore = Arc::new(Semaphore::new(max_streams.max(1)));
        self
    }

    pub fn available_stream_permits(&self) -> usize {
        self.transcode_semaphore.available_permits()
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
            let mut cmd = Command::new(&self.ffprobe_bin);
            cmd.args([
                "-v",
                "quiet",
                "-print_format",
                "json",
                "-show_format",
                "-show_streams",
            ])
            .arg(local_file_input_arg(path));

            let (status, stdout) = run_command_bounded_stdout(
                &mut cmd,
                Duration::from_secs(15),
                MAX_FFPROBE_STDOUT_BYTES,
                "ffprobe",
            )
            .await?;

            if !status.success() {
                return Err(DomainError::Storage(
                    "ffprobe exited with non-zero status".into(),
                ));
            }

            let raw: RawFfprobeOutput = serde_json::from_slice(&stdout)
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
        #[cfg(any(target_os = "android", target_os = "ios"))]
        {
            let _ = (path, seek_secs, max_width);
            return Err(DomainError::Storage(
                "ffmpeg subprocess execution is disabled on mobile platforms".into(),
            ));
        }

        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        {
            let vf = format!("scale='min({max_width},iw)':-2");
            let mut cmd = Command::new(&self.ffmpeg_bin);
            cmd.arg("-nostdin")
                .arg("-v")
                .arg("error")
                .arg("-ss")
                .arg(seek_secs.to_string())
                .arg("-i")
                .arg(local_file_input_arg(path))
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
                ]);

            let (status, stdout) = run_command_bounded_stdout(
                &mut cmd,
                Duration::from_secs(15),
                MAX_FFMPEG_FRAME_STDOUT_BYTES,
                "ffmpeg frame extraction",
            )
            .await?;

            if !status.success() || stdout.is_empty() {
                return Err(DomainError::Storage(
                    "ffmpeg failed to extract video frame".into(),
                ));
            }
            Ok(stdout)
        }
    }

    pub async fn extract_embedded_subtitle_webvtt(
        &self,
        path: &Path,
        stream_index: u32,
    ) -> DomainResult<String> {
        #[cfg(any(target_os = "android", target_os = "ios"))]
        {
            let _ = (path, stream_index);
            return Err(DomainError::Storage(
                "ffmpeg subprocess execution is disabled on mobile platforms".into(),
            ));
        }

        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        {
            let map_arg = format!("0:{stream_index}");
            let mut cmd = Command::new(&self.ffmpeg_bin);
            cmd.arg("-nostdin")
                .arg("-v")
                .arg("error")
                .arg("-i")
                .arg(local_file_input_arg(path))
                .args(["-map", &map_arg, "-c:s", "webvtt", "-f", "webvtt", "pipe:1"]);

            let (status, stdout) = run_command_bounded_stdout(
                &mut cmd,
                Duration::from_secs(20),
                MAX_FFMPEG_SUBTITLE_STDOUT_BYTES,
                "ffmpeg subtitle extraction",
            )
            .await?;

            if !status.success() {
                return Err(DomainError::Storage(
                    "ffmpeg failed to convert embedded subtitle to WebVTT".into(),
                ));
            }
            Ok(String::from_utf8_lossy(&stdout).into_owned())
        }
    }

    pub fn spawn_stdout_stream(&self, args: &[String]) -> DomainResult<ByteChunkStream> {
        #[cfg(any(target_os = "android", target_os = "ios"))]
        {
            let _ = args;
            return Err(DomainError::Storage(
                "ffmpeg subprocess execution is disabled on mobile platforms".into(),
            ));
        }

        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        {
            let permit = self
                .transcode_semaphore
                .clone()
                .try_acquire_owned()
                .map_err(|_| {
                    DomainError::TooManyRequests(
                        "too many concurrent media transcode streams; please try again shortly"
                            .into(),
                    )
                })?;

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
                let _permit = permit;
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
                            let _ = child.wait().await;
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
                                let _ = child.wait().await;
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
                            let _ = child.wait().await;
                            break;
                        }
                    }
                }
            });

            Ok(MpscByteStream::into_boxed(rx))
        }
    }
}

impl Default for FfmpegToolkit {
    fn default() -> Self {
        Self::new("ffmpeg", "ffprobe")
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[derive(Debug, Deserialize)]
struct RawFfprobeOutput {
    format: Option<RawFfprobeFormat>,
    streams: Option<Vec<RawFfprobeStream>>,
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[derive(Debug, Deserialize)]
struct RawFfprobeFormat {
    format_name: Option<String>,
    duration: Option<String>,
    bit_rate: Option<String>,
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
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

#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[derive(Debug, Deserialize)]
struct RawFfprobeDisposition {
    default: Option<i32>,
    attached_pic: Option<i32>,
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[derive(Debug, Deserialize)]
struct RawFfprobeTags {
    language: Option<String>,
    title: Option<String>,
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::time::Duration;

    use super::*;

    #[tokio::test]
    async fn spawn_stdout_stream_bounds_concurrency_and_releases_permit_on_drop() {
        let tmp = tempfile::TempDir::new().unwrap();
        let fake_bin = tmp.path().join("fake-ffmpeg-stream.sh");
        std::fs::write(&fake_bin, "#!/bin/sh\nprintf 'chunk'\nsleep 5\n").unwrap();
        let mut perms = std::fs::metadata(&fake_bin).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&fake_bin, perms).unwrap();

        let bin_str = fake_bin.to_string_lossy().to_string();
        let toolkit = FfmpegToolkit::new(&bin_str, &bin_str).with_max_concurrent_streams(2);
        assert_eq!(toolkit.available_stream_permits(), 2);

        let stream1 = toolkit.spawn_stdout_stream(&[]).unwrap();
        let stream2 = toolkit.spawn_stdout_stream(&[]).unwrap();
        assert_eq!(toolkit.available_stream_permits(), 0);

        // 3rd concurrent stream exceeds semaphore bound -> rejected with TooManyRequests
        let err = match toolkit.spawn_stdout_stream(&[]) {
            Ok(_) => panic!("expected 3rd stream to fail with TooManyRequests"),
            Err(e) => e,
        };
        assert!(matches!(err, DomainError::TooManyRequests(_)));

        // Dropping stream1 kills the child process and releases its permit
        drop(stream1);
        tokio::time::timeout(Duration::from_secs(2), async {
            while toolkit.available_stream_permits() == 0 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("semaphore permit must be released promptly after stream drop");

        let stream3 = toolkit.spawn_stdout_stream(&[]).unwrap();
        assert_eq!(toolkit.available_stream_permits(), 0);
        drop(stream2);
        drop(stream3);
    }
}

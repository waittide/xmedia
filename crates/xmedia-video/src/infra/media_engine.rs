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
use xmedia_core::infra::stream::decode_text_smart;

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
        let (decoded, _) = decode_text_smart(raw_bytes);
        let text = decoded.trim_start_matches('\u{feff}');
        match format
            .trim()
            .trim_start_matches('.')
            .to_ascii_lowercase()
            .as_str()
        {
            "vtt" | "webvtt" => {
                if text.trim_start().starts_with("WEBVTT") {
                    text.to_string()
                } else {
                    format!("WEBVTT\n\n{text}")
                }
            }
            "ass" | "ssa" => convert_ass_to_vtt(text),
            _ => convert_srt_to_vtt(text),
        }
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

pub fn convert_srt_to_vtt(srt: &str) -> String {
    let mut out = String::with_capacity(srt.len() + 16);
    out.push_str("WEBVTT\n\n");
    for raw_line in srt.lines() {
        let line = raw_line.trim_end_matches('\r');
        if line.contains("-->") {
            out.push_str(&line.replace(',', "."));
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}

pub fn convert_ass_to_vtt(ass: &str) -> String {
    let mut out = String::from("WEBVTT\n\n");
    let mut in_events = false;
    let mut start_col = 1usize;
    let mut end_col = 2usize;
    let mut text_col = 9usize;

    for raw_line in ass.lines() {
        let line = raw_line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_events = line.eq_ignore_ascii_case("[Events]");
            continue;
        }
        if !in_events {
            continue;
        }
        if let Some(fmt_spec) = line.strip_prefix("Format:") {
            let cols: Vec<&str> = fmt_spec.split(',').map(str::trim).collect();
            if let Some(i) = cols.iter().position(|c| c.eq_ignore_ascii_case("Start")) {
                start_col = i;
            }
            if let Some(i) = cols.iter().position(|c| c.eq_ignore_ascii_case("End")) {
                end_col = i;
            }
            if let Some(i) = cols.iter().position(|c| c.eq_ignore_ascii_case("Text")) {
                text_col = i;
            }
            continue;
        }
        if let Some(diag) = line.strip_prefix("Dialogue:") {
            let max_col = start_col.max(end_col).max(text_col);
            let parts: Vec<&str> = diag.splitn(max_col + 1, ',').collect();
            if let (Some(start_raw), Some(end_raw), Some(text_raw)) = (
                parts.get(start_col),
                parts.get(end_col),
                parts.get(text_col),
            ) && let (Some(start_vtt), Some(end_vtt)) = (
                ass_time_to_vtt(start_raw.trim()),
                ass_time_to_vtt(end_raw.trim()),
            ) {
                let cleaned = strip_ass_tags(text_raw);
                if !cleaned.trim().is_empty() {
                    out.push_str(&format!("{start_vtt} --> {end_vtt}\n{cleaned}\n\n"));
                }
            }
        }
    }
    out
}

fn ass_time_to_vtt(raw: &str) -> Option<String> {
    let mut it = raw.split(':');
    let h: u32 = it.next()?.trim().parse().ok()?;
    let m: u32 = it.next()?.trim().parse().ok()?;
    let sec_part = it.next()?.trim();
    let (s_str, frac_str) = sec_part.split_once('.').unwrap_or((sec_part, "0"));
    let s: u32 = s_str.parse().ok()?;
    let millis: u32 = match frac_str.len() {
        1 => frac_str.parse::<u32>().ok()? * 100,
        2 => frac_str.parse::<u32>().ok()? * 10,
        _ => frac_str.get(0..3)?.parse::<u32>().ok()?,
    };
    Some(format!("{h:02}:{m:02}:{s:02}.{millis:03}"))
}

fn strip_ass_tags(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(open_idx) = rest.find('{') {
        out.push_str(&rest[..open_idx]);
        let after_open = &rest[open_idx + 1..];
        if let Some(close_idx) = after_open.find('}') {
            rest = &after_open[close_idx + 1..];
        } else {
            // Unclosed '{' — preserve literal text rather than dropping trailing dialogue.
            out.push_str(&rest[open_idx..]);
            rest = "";
            break;
        }
    }
    out.push_str(rest);
    let converted = out
        .replace("\\N", "\n")
        .replace("\\n", "\n")
        .replace("\\h", " ");
    // Collapse blank lines so crafted `\N\N` cannot terminate a WebVTT cue prematurely.
    converted
        .lines()
        .map(str::trim_end)
        .filter(|l| !l.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn shift_webvtt_timestamps(vtt: &str, offset_secs: f64) -> String {
    if !offset_secs.is_finite() || offset_secs <= 0.0 {
        return vtt.to_string();
    }
    let offset_ms = (offset_secs * 1000.0).round() as i64;
    let normalized = vtt.replace("\r\n", "\n").replace('\r', "\n");
    let mut out_blocks: Vec<String> = Vec::new();

    for raw_block in normalized.split("\n\n") {
        let lines: Vec<&str> = raw_block.lines().collect();
        if lines.is_empty() {
            continue;
        }
        if let Some(timing_idx) = lines.iter().position(|l| l.contains("-->")) {
            let timing_line = lines[timing_idx];
            if let Some(shifted_timing) = shift_vtt_timing_line(timing_line, offset_ms) {
                let mut rebuilt = Vec::with_capacity(lines.len());
                rebuilt.extend(lines[..timing_idx].iter().map(|s| s.to_string()));
                rebuilt.push(shifted_timing);
                rebuilt.extend(lines[timing_idx + 1..].iter().map(|s| s.to_string()));
                out_blocks.push(rebuilt.join("\n"));
            } else if timing_line.starts_with("WEBVTT") {
                out_blocks.push(raw_block.to_string());
            }
            // Otherwise the cue ended before offset_ms and is dropped.
        } else {
            out_blocks.push(raw_block.to_string());
        }
    }

    if out_blocks.is_empty() {
        "WEBVTT\n\n".to_string()
    } else {
        format!("{}\n\n", out_blocks.join("\n\n"))
    }
}

fn shift_vtt_timing_line(line: &str, offset_ms: i64) -> Option<String> {
    let (left, right) = line.split_once("-->")?;
    let start_ms = parse_vtt_timestamp_ms(left.trim())?;
    let right_trimmed = right.trim_start();
    let mut split_end = right_trimmed.splitn(2, char::is_whitespace);
    let end_token = split_end.next()?.trim();
    let settings = split_end.next().unwrap_or("").trim();
    let end_ms = parse_vtt_timestamp_ms(end_token)?;

    let shifted_end = end_ms - offset_ms;
    if shifted_end <= 0 {
        return None;
    }
    let shifted_start = (start_ms - offset_ms).max(0);
    if settings.is_empty() {
        Some(format!(
            "{} --> {}",
            format_vtt_timestamp_ms(shifted_start),
            format_vtt_timestamp_ms(shifted_end)
        ))
    } else {
        Some(format!(
            "{} --> {} {settings}",
            format_vtt_timestamp_ms(shifted_start),
            format_vtt_timestamp_ms(shifted_end)
        ))
    }
}

fn parse_vtt_timestamp_ms(raw: &str) -> Option<i64> {
    let clean = raw.trim().replace(',', ".");
    let (hms, ms_str) = clean.split_once('.')?;
    let millis: i64 = match ms_str.len() {
        1 => ms_str.parse::<i64>().ok()? * 100,
        2 => ms_str.parse::<i64>().ok()? * 10,
        3 => ms_str.parse::<i64>().ok()?,
        _ => ms_str.get(0..3)?.parse::<i64>().ok()?,
    };
    let parts: Vec<&str> = hms.split(':').collect();
    let (h, m, s) = match parts.as_slice() {
        [mm, ss] => (0i64, mm.parse::<i64>().ok()?, ss.parse::<i64>().ok()?),
        [hh, mm, ss] => (
            hh.parse::<i64>().ok()?,
            mm.parse::<i64>().ok()?,
            ss.parse::<i64>().ok()?,
        ),
        _ => return None,
    };
    Some((((h * 60 + m) * 60 + s) * 1000) + millis)
}

fn format_vtt_timestamp_ms(total_ms: i64) -> String {
    let clamped = total_ms.max(0);
    let millis = clamped % 1000;
    let total_secs = clamped / 1000;
    let secs = total_secs % 60;
    let mins = (total_secs / 60) % 60;
    let hours = total_secs / 3600;
    format!("{hours:02}:{mins:02}:{secs:02}.{millis:03}")
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

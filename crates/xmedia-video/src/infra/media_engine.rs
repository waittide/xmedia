use std::path::Path;

use async_trait::async_trait;
use tracing::debug;
use xmedia_core::application::dto::FfmpegStatusDto;
use xmedia_core::application::ports::ByteChunkStream;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::infra::ffmpeg::{FfmpegToolkit, ProbedMediaInfo};
use xmedia_core::infra::stream::decode_text_smart;

use crate::application::ports::{
    VideoMediaEnginePort, VideoStreamTranscodeRequest, VideoTranscodeMode,
};

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

    async fn probe_video(&self, fs_path: &Path) -> ProbedMediaInfo {
        self.ffmpeg.probe_media(fs_path).await.unwrap_or_default()
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
            let parts: Vec<&str> = diag.splitn(text_col + 1, ',').collect();
            if parts.len() > text_col
                && let (Some(start_vtt), Some(end_vtt)) = (
                    ass_time_to_vtt(parts[start_col].trim()),
                    ass_time_to_vtt(parts[end_col].trim()),
                )
            {
                let cleaned = strip_ass_tags(parts[text_col]);
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
    let mut in_brace = false;
    for ch in raw.chars() {
        match ch {
            '{' => in_brace = true,
            '}' => in_brace = false,
            _ if !in_brace => out.push(ch),
            _ => {}
        }
    }
    out.replace("\\N", "\n").replace("\\n", "\n")
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

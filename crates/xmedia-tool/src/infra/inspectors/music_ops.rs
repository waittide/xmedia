mod cue;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use regex::Regex;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_formats::music::{
    AudioMetaPatch, apply_audio_patch_in_place, probe_audio_path, read_embedded_cover_from_path,
    read_sidecar_lrc, shift_and_clean_lrc_text, sidecar_lrc_path, write_embedded_cover_in_place,
    write_replaygain_in_place,
};

use crate::application::ops::{
    CueSplitReportDto, MusicCoverOptimizeReportDto, MusicLyricsToolInput, MusicLyricsToolReportDto,
    MusicToolCommand, MusicToolOutput, ReplayGainReportDto, ReplayGainTrackResultDto,
};
use crate::domain::zh_text::{ZhConvertDirection, convert_zh_plain};
use crate::infra::atomic_fs::{atomic_mutate_copy, atomic_write_bytes};
use crate::infra::inspectors::map_format_err;

#[cfg(test)]
pub(crate) use cue::parse_cue_sheet;
use cue::split_cue_album;

pub fn run_music_tool(cmd: MusicToolCommand) -> DomainResult<MusicToolOutput> {
    match cmd {
        MusicToolCommand::ProcessLyrics { path, input } => {
            process_music_lyrics(&path, &input).map(MusicToolOutput::Lyrics)
        }
        MusicToolCommand::OptimizeCover {
            path,
            max_dimension,
            jpeg_quality,
        } => optimize_music_cover(&path, max_dimension, jpeg_quality).map(MusicToolOutput::Cover),
        MusicToolCommand::ApplyReplayGain { items, ffmpeg_bin } => {
            apply_replaygain_batch(&items, &ffmpeg_bin).map(MusicToolOutput::ReplayGain)
        }
        MusicToolCommand::SplitCue { path, ffmpeg_bin } => {
            split_cue_album(&path, &ffmpeg_bin).map(MusicToolOutput::CueSplit)
        }
    }
}

static RE_LRC_TAG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\[(\d{1,3}):(\d{2})(?:[.:](\d{1,3}))?\]").expect("valid lrc timestamp regex")
});
static RE_EBUR128_INTEGRATED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"I:\s*(-?\d+(?:\.\d+)?)\s*LUFS").expect("valid ebur128 I regex"));
static RE_EBUR128_PEAK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"Peak:\s*(-?\d+(?:\.\d+)?)\s*dBFS").expect("valid peak regex"));

/// Pure LRC/lyrics text processor: timestamp shifting, credit noise stripping, and Zh T2S/S2T conversion.
pub fn transform_lrc_text(
    raw: &str,
    offset_ms: i64,
    strip_credit_noise: bool,
    zh_direction: Option<ZhConvertDirection>,
) -> (String, usize) {
    let (shifted, removed) = shift_and_clean_lrc_text(raw, offset_ms, strip_credit_noise);
    let Some(dir) = zh_direction else {
        return (shifted, removed);
    };

    let converted = shifted
        .lines()
        .map(|line| {
            let mut rebuilt = String::with_capacity(line.len());
            let mut last = 0usize;
            for m in RE_LRC_TAG.find_iter(line) {
                if m.start() > last {
                    rebuilt.push_str(&convert_zh_plain(&line[last..m.start()], dir));
                }
                rebuilt.push_str(m.as_str());
                last = m.end();
            }
            if last < line.len() {
                rebuilt.push_str(&convert_zh_plain(&line[last..], dir));
            }
            rebuilt
        })
        .collect::<Vec<_>>()
        .join("\n");

    (converted, removed)
}

pub fn process_music_lyrics(
    path: &Path,
    input: &MusicLyricsToolInput,
) -> DomainResult<MusicLyricsToolReportDto> {
    let parsed = probe_audio_path(path, "", "Untitled").map_err(map_format_err)?;
    let existing_embedded = parsed.embedded_lyrics.filter(|s| !s.trim().is_empty());
    let existing_sidecar = read_sidecar_lrc(path);

    let source_text = if let Some(ref custom) = input.custom_text {
        custom.clone()
    } else if input.sync_mode == "embedded_to_sidecar" {
        existing_embedded
            .clone()
            .or(existing_sidecar.clone())
            .unwrap_or_default()
    } else {
        existing_sidecar
            .clone()
            .or(existing_embedded.clone())
            .unwrap_or_default()
    };

    if source_text.trim().is_empty() {
        return Err(DomainError::Validation(
            "no embedded or sidecar lyrics available to process".into(),
        ));
    }

    let (final_lyrics, removed_lines) = transform_lrc_text(
        &source_text,
        input.offset_ms,
        input.strip_credit_noise,
        input.zh_direction,
    );

    let write_embedded = matches!(input.sync_mode.as_str(), "sidecar_to_embedded" | "both")
        || (input.sync_mode == "none" && existing_embedded.is_some());
    let write_sidecar = matches!(input.sync_mode.as_str(), "embedded_to_sidecar" | "both")
        || (input.sync_mode == "none" && existing_sidecar.is_some());

    if write_embedded {
        let patch = AudioMetaPatch {
            embedded_lyrics: Some(final_lyrics.clone()),
            ..Default::default()
        };
        atomic_mutate_copy(path, |staging| {
            apply_audio_patch_in_place(staging, &patch).map_err(map_format_err)
        })?;
    }

    if write_sidecar {
        let lrc_path = sidecar_lrc_path(path);
        atomic_write_bytes(&lrc_path, final_lyrics.as_bytes())?;
    }

    Ok(MusicLyricsToolReportDto {
        updated_embedded: write_embedded,
        updated_sidecar: write_sidecar,
        removed_lines,
        final_lyrics,
    })
}

pub fn optimize_music_cover(
    path: &Path,
    max_dimension: u32,
    jpeg_quality: u8,
) -> DomainResult<MusicCoverOptimizeReportDto> {
    let Some(asset) = read_embedded_cover_from_path(path).map_err(map_format_err)? else {
        return Ok(MusicCoverOptimizeReportDto {
            had_cover: false,
            optimized: false,
            before_bytes: 0,
            after_bytes: 0,
            width: None,
            height: None,
        });
    };
    let raw = asset.bytes;
    let before_bytes = raw.len() as u64;
    let img = image::load_from_memory(&raw)
        .map_err(|e| DomainError::Validation(format!("decode embedded cover failed: {e}")))?;
    let (orig_w, orig_h) = (img.width(), img.height());
    let max_dim = max_dimension.clamp(300, 2400);
    let resized = if orig_w > max_dim || orig_h > max_dim {
        img.resize(max_dim, max_dim, FilterType::Lanczos3)
    } else {
        img
    };

    let mut out = Vec::new();
    let mut enc = JpegEncoder::new_with_quality(&mut out, jpeg_quality.clamp(60, 95));
    enc.encode_image(&resized.to_rgb8())
        .map_err(|e| DomainError::Storage(format!("encode optimized cover failed: {e}")))?;

    let after_bytes = out.len() as u64;
    let (w, h) = (resized.width(), resized.height());

    if after_bytes >= before_bytes && w == orig_w && h == orig_h && before_bytes < 512 * 1024 {
        return Ok(MusicCoverOptimizeReportDto {
            had_cover: true,
            optimized: false,
            before_bytes,
            after_bytes: before_bytes,
            width: Some(w),
            height: Some(h),
        });
    }

    atomic_mutate_copy(path, |staging| {
        write_embedded_cover_in_place(staging, &out, "image/jpeg").map_err(map_format_err)
    })?;

    Ok(MusicCoverOptimizeReportDto {
        had_cover: true,
        optimized: true,
        before_bytes,
        after_bytes,
        width: Some(w),
        height: Some(h),
    })
}

fn measure_track_ebur128(path: &Path, ffmpeg_bin: &str) -> DomainResult<(f64, f64)> {
    let path_str = path.to_string_lossy();
    let out = crate::infra::inspectors::process::run_command_bounded(
        ffmpeg_bin,
        &[
            "-hide_banner",
            "-nostats",
            "-i",
            &path_str,
            "-filter_complex",
            "ebur128=peak=true",
            "-f",
            "null",
            "-",
        ],
        std::time::Duration::from_secs(120),
        4 * 1024 * 1024,
    )?;

    let stderr = String::from_utf8_lossy(&out.stderr);
    let integrated = RE_EBUR128_INTEGRATED
        .captures_iter(&stderr)
        .last()
        .and_then(|c| c[1].parse::<f64>().ok())
        .ok_or_else(|| {
            DomainError::Storage("failed to parse FFmpeg ebur128 integrated loudness".into())
        })?;
    let peak_dbfs = RE_EBUR128_PEAK
        .captures_iter(&stderr)
        .last()
        .and_then(|c| c[1].parse::<f64>().ok())
        .unwrap_or(0.0);

    let gain_db = (-18.0 - integrated).clamp(-30.0, 30.0);
    let linear_peak = 10f64.powf(peak_dbfs / 20.0).clamp(0.0, 2.0);
    Ok((gain_db, linear_peak))
}

fn write_replaygain_tags(
    path: &Path,
    track_gain: f64,
    track_peak: f64,
    album_gain: Option<f64>,
    album_peak: Option<f64>,
) -> DomainResult<()> {
    atomic_mutate_copy(path, |staging| {
        write_replaygain_in_place(staging, track_gain, track_peak, album_gain, album_peak)
            .map_err(map_format_err)
    })
}

pub fn apply_replaygain_batch(
    tracks: &[(String, PathBuf)],
    ffmpeg_bin: &str,
) -> DomainResult<ReplayGainReportDto> {
    if tracks.is_empty() {
        return Err(DomainError::Validation("no audio tracks selected".into()));
    }

    let mut measured = Vec::new();
    let mut failed_paths = Vec::new();
    for (rel, abs) in tracks {
        match measure_track_ebur128(abs, ffmpeg_bin) {
            Ok((gain, peak)) => measured.push((rel.clone(), abs.clone(), gain, peak)),
            Err(_) => failed_paths.push(rel.clone()),
        }
    }

    if measured.is_empty() {
        return Err(DomainError::Storage(
            "FFmpeg ReplayGain analysis failed for all tracks".into(),
        ));
    }

    let album_gain = if measured.len() > 1 {
        let avg = measured.iter().map(|m| m.2).sum::<f64>() / measured.len() as f64;
        Some(avg)
    } else {
        None
    };
    let album_peak = if measured.len() > 1 {
        Some(measured.iter().map(|m| m.3).fold(0.0f64, f64::max))
    } else {
        None
    };

    let mut results = Vec::with_capacity(measured.len());
    for (rel, abs, gain, peak) in measured {
        if write_replaygain_tags(&abs, gain, peak, album_gain, album_peak).is_ok() {
            results.push(ReplayGainTrackResultDto {
                rel_path: rel,
                track_gain_db: (gain * 100.0).round() / 100.0,
                track_peak: (peak * 1_000_000.0).round() / 1_000_000.0,
            });
        } else {
            failed_paths.push(rel);
        }
    }

    Ok(ReplayGainReportDto {
        processed: results.len(),
        album_gain_db: album_gain.map(|v| (v * 100.0).round() / 100.0),
        album_peak: album_peak.map(|v| (v * 1_000_000.0).round() / 1_000_000.0),
        tracks: results,
        failed_paths,
    })
}

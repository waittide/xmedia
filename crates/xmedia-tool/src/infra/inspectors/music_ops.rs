use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::LazyLock;

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use lofty::config::WriteOptions;
use lofty::file::TaggedFileExt;
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::prelude::{Accessor, AudioFile, ItemKey};
use lofty::tag::ItemValue;
use regex::Regex;
use serde::{Deserialize, Serialize};
use xmedia_core::domain::error::{DomainError, DomainResult};

use crate::infra::atomic_fs::{atomic_mutate_copy, atomic_write_bytes};
use crate::infra::inspectors::music::{
    ensure_mutable_primary_tag, open_tagged, read_sidecar_lrc, sidecar_lrc_path,
};
use crate::infra::inspectors::zh_text::{ZhConvertDirection, convert_zh_plain};

static RE_LRC_TAG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\[(\d{1,3}):(\d{2})(?:[.:](\d{1,3}))?\]").expect("valid lrc timestamp regex")
});

static RE_CREDIT_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^\s*(?:作词|作曲|编曲|制作人|监制|混音|母带|录音|吉他|贝斯|鼓|弦乐|和声|配唱|发行|出品|企划|统筹|版权|歌词制作|LRC制作|词|曲|Lyricist|Composer|Arranger|Producer|Mixing|Mastering)\s*[:：]|(?:QQ音乐|网易云音乐|酷狗音乐|酷我音乐|咪咕音乐|汽水音乐|版权所有|未经许可)",
    )
    .expect("valid credit noise regex")
});

static RE_EBUR128_INTEGRATED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"I:\s*(-?\d+(?:\.\d+)?)\s*LUFS").expect("valid ebur128 I regex"));
static RE_EBUR128_PEAK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"Peak:\s*(-?\d+(?:\.\d+)?)\s*dBFS").expect("valid peak regex"));

#[derive(Debug, Clone, Default, Deserialize)]
pub struct MusicLyricsToolInput {
    /// Positive delays lyrics, negative advances lyrics (milliseconds)
    #[serde(default)]
    pub offset_ms: i64,
    #[serde(default)]
    pub strip_credit_noise: bool,
    #[serde(default)]
    pub zh_direction: Option<ZhConvertDirection>,
    /// "none" | "sidecar_to_embedded" | "embedded_to_sidecar" | "both"
    #[serde(default = "default_sync_mode")]
    pub sync_mode: String,
    /// Optional explicit lyrics text override (e.g. edited in modal)
    pub custom_text: Option<String>,
}

fn default_sync_mode() -> String {
    "both".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MusicLyricsToolReportDto {
    pub updated_embedded: bool,
    pub updated_sidecar: bool,
    pub removed_lines: usize,
    pub final_lyrics: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MusicCoverOptimizeReportDto {
    pub had_cover: bool,
    pub optimized: bool,
    pub before_bytes: u64,
    pub after_bytes: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayGainTrackResultDto {
    pub rel_path: String,
    pub track_gain_db: f64,
    pub track_peak: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayGainReportDto {
    pub processed: usize,
    pub album_gain_db: Option<f64>,
    pub album_peak: Option<f64>,
    pub tracks: Vec<ReplayGainTrackResultDto>,
    pub failed_paths: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct BatchMusicAlbumSyncInput {
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub artist: Option<String>,
    pub year: Option<i32>,
    pub genre: Option<String>,
    pub disc_number: Option<u32>,
    #[serde(default)]
    pub auto_number_tracks: bool,
    #[serde(default = "default_track_start")]
    pub start_track: u32,
}

fn default_track_start() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchMusicAlbumSyncReportDto {
    pub updated_count: usize,
    pub failed_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CueSplitReportDto {
    pub created_files: Vec<String>,
    pub album_title: Option<String>,
    pub album_artist: Option<String>,
}

fn shift_timestamp_ms(min: i64, sec: i64, frac_raw: Option<&str>, offset_ms: i64) -> String {
    let frac_ms = match frac_raw {
        Some(f) if f.len() == 1 => f.parse::<i64>().unwrap_or(0) * 100,
        Some(f) if f.len() == 2 => f.parse::<i64>().unwrap_or(0) * 10,
        Some(f) => f.parse::<i64>().unwrap_or(0),
        None => 0,
    };
    let total = (min * 60_000 + sec * 1_000 + frac_ms + offset_ms).max(0);
    let new_min = total / 60_000;
    let new_sec = (total % 60_000) / 1_000;
    let new_cs = (total % 1_000) / 10;
    format!("[{new_min:02}:{new_sec:02}.{new_cs:02}]")
}

/// Pure LRC/lyrics text processor: timestamp shifting, credit noise stripping, and Zh T2S/S2T conversion.
pub fn transform_lrc_text(
    raw: &str,
    offset_ms: i64,
    strip_credit_noise: bool,
    zh_direction: Option<ZhConvertDirection>,
) -> (String, usize) {
    let mut out_lines = Vec::new();
    let mut removed = 0usize;

    for raw_line in raw.replace("\r\n", "\n").lines() {
        let mut line = raw_line.to_string();

        if offset_ms != 0 {
            line = RE_LRC_TAG
                .replace_all(&line, |caps: &regex::Captures<'_>| {
                    let min = caps[1].parse::<i64>().unwrap_or(0);
                    let sec = caps[2].parse::<i64>().unwrap_or(0);
                    let frac = caps.get(3).map(|m| m.as_str());
                    shift_timestamp_ms(min, sec, frac, offset_ms)
                })
                .into_owned();
        }

        if strip_credit_noise {
            let text_only = RE_LRC_TAG.replace_all(&line, "").trim().to_string();
            if !text_only.is_empty() && RE_CREDIT_LINE.is_match(&text_only) {
                removed += 1;
                continue;
            }
        }

        if let Some(dir) = zh_direction {
            // Convert only text outside LRC brackets
            let mut rebuilt = String::with_capacity(line.len());
            let mut last = 0usize;
            for m in RE_LRC_TAG.find_iter(&line) {
                if m.start() > last {
                    rebuilt.push_str(&convert_zh_plain(&line[last..m.start()], dir));
                }
                rebuilt.push_str(m.as_str());
                last = m.end();
            }
            if last < line.len() {
                rebuilt.push_str(&convert_zh_plain(&line[last..], dir));
            }
            line = rebuilt;
        }

        out_lines.push(line);
    }

    (out_lines.join("\n"), removed)
}

pub fn process_music_lyrics(
    path: &Path,
    input: &MusicLyricsToolInput,
) -> DomainResult<MusicLyricsToolReportDto> {
    let tagged = open_tagged(path)?;
    let existing_embedded = tagged
        .tags()
        .iter()
        .find_map(|t| {
            t.get_string(ItemKey::Lyrics)
                .or_else(|| t.get_string(ItemKey::UnsyncLyrics))
                .map(str::to_owned)
        })
        .filter(|s| !s.trim().is_empty());
    let existing_sidecar = read_sidecar_lrc(path);

    let source_text = if let Some(ref custom) = input.custom_text {
        custom.clone()
    } else {
        match input.sync_mode.as_str() {
            "embedded_to_sidecar" => existing_embedded
                .clone()
                .or(existing_sidecar.clone())
                .unwrap_or_default(),
            "sidecar_to_embedded" => existing_sidecar
                .clone()
                .or(existing_embedded.clone())
                .unwrap_or_default(),
            _ => existing_sidecar
                .clone()
                .or(existing_embedded.clone())
                .unwrap_or_default(),
        }
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
        atomic_mutate_copy(path, |staging| {
            let mut tf = open_tagged(staging)?;
            let tag = ensure_mutable_primary_tag(&mut tf);
            tag.remove_key(ItemKey::UnsyncLyrics);
            tag.insert_text(ItemKey::Lyrics, final_lyrics.clone());
            tf.save_to_path(staging, WriteOptions::default())
                .map_err(|e| DomainError::Storage(format!("save embedded lyrics failed: {e}")))?;
            Ok(())
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
    let tagged = open_tagged(path)?;
    let pic_bytes = tagged
        .tags()
        .iter()
        .flat_map(|t| t.pictures().iter())
        .find(|p| p.pic_type() == PictureType::CoverFront)
        .or_else(|| {
            tagged
                .tags()
                .iter()
                .flat_map(|t| t.pictures().iter())
                .next()
        })
        .map(|p| p.data().to_vec());

    let Some(raw) = pic_bytes else {
        return Ok(MusicCoverOptimizeReportDto {
            had_cover: false,
            optimized: false,
            before_bytes: 0,
            after_bytes: 0,
            width: None,
            height: None,
        });
    };

    let before_bytes = raw.len() as u64;
    let img = image::load_from_memory(&raw)
        .map_err(|e| DomainError::Validation(format!("decode embedded cover failed: {e}")))?;
    let max_dim = max_dimension.clamp(300, 2400);
    let resized = if img.width() > max_dim || img.height() > max_dim {
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

    if after_bytes >= before_bytes && w == resized.width() && before_bytes < 512 * 1024 {
        return Ok(MusicCoverOptimizeReportDto {
            had_cover: true,
            optimized: false,
            before_bytes,
            after_bytes: before_bytes,
            width: Some(w),
            height: Some(h),
        });
    }

    let new_pic = Picture::unchecked(out)
        .pic_type(PictureType::CoverFront)
        .mime_type(MimeType::Jpeg)
        .build();

    atomic_mutate_copy(path, |staging| {
        let mut tf = open_tagged(staging)?;
        let tag = ensure_mutable_primary_tag(&mut tf);
        tag.remove_picture_type(PictureType::CoverFront);
        tag.push_picture(new_pic.clone());
        tf.save_to_path(staging, WriteOptions::default())
            .map_err(|e| DomainError::Storage(format!("save optimized cover failed: {e}")))?;
        Ok(())
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
    let out = Command::new(ffmpeg_bin)
        .args([
            "-hide_banner",
            "-nostats",
            "-i",
            &path.to_string_lossy(),
            "-filter_complex",
            "ebur128=peak=true",
            "-f",
            "null",
            "-",
        ])
        .output()
        .map_err(|e| DomainError::Storage(format!("spawn ffmpeg ebur128 failed: {e}")))?;

    let stderr = String::from_utf8_lossy(&out.stderr);
    // Find the final Summary section's Integrated loudness & True Peak
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

    // ReplayGain 2.0 reference loudness is -18.0 LUFS
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
        let mut tf = open_tagged(staging)?;
        let tag = ensure_mutable_primary_tag(&mut tf);
        tag.insert(lofty::tag::TagItem::new(
            ItemKey::ReplayGainTrackGain,
            ItemValue::Text(format!("{track_gain:+.2} dB")),
        ));
        tag.insert(lofty::tag::TagItem::new(
            ItemKey::ReplayGainTrackPeak,
            ItemValue::Text(format!("{track_peak:.6}")),
        ));
        if let Some(ag) = album_gain {
            tag.insert(lofty::tag::TagItem::new(
                ItemKey::ReplayGainAlbumGain,
                ItemValue::Text(format!("{ag:+.2} dB")),
            ));
        }
        if let Some(ap) = album_peak {
            tag.insert(lofty::tag::TagItem::new(
                ItemKey::ReplayGainAlbumPeak,
                ItemValue::Text(format!("{ap:.6}")),
            ));
        }
        tf.save_to_path(staging, WriteOptions::default())
            .map_err(|e| DomainError::Storage(format!("save ReplayGain tags failed: {e}")))?;
        Ok(())
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
        measured.iter().map(|m| m.3).fold(0.0f64, f64::max).into()
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

#[derive(Debug, Clone)]
pub(crate) struct CueTrackEntry {
    pub number: u32,
    pub title: String,
    pub performer: Option<String>,
    pub start_secs: f64,
}

#[derive(Debug, Clone)]
pub(crate) struct ParsedCueSheet {
    pub album_title: Option<String>,
    pub album_performer: Option<String>,
    pub audio_file: String,
    pub tracks: Vec<CueTrackEntry>,
}

fn decode_cue_bytes(bytes: &[u8]) -> String {
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.trim_start_matches('\u{feff}').to_string();
    }
    let mut det = chardetng::EncodingDetector::new(chardetng::Iso2022JpDetection::Allow);
    det.feed(bytes, true);
    let enc = det.guess(None, chardetng::Utf8Detection::Allow);
    let (decoded, _, _) = enc.decode(bytes);
    decoded.trim_start_matches('\u{feff}').to_string()
}

fn extract_cue_quoted(line: &str) -> String {
    if let Some(start) = line.find('"')
        && let Some(end) = line[start + 1..].find('"')
    {
        return line[start + 1..start + 1 + end].trim().to_string();
    }
    line.split_whitespace()
        .skip(1)
        .collect::<Vec<_>>()
        .join(" ")
}

fn parse_cue_index_secs(raw: &str) -> Option<f64> {
    let parts: Vec<&str> = raw.trim().split(':').collect();
    if parts.len() != 3 {
        return None;
    }
    let min = parts[0].parse::<f64>().ok()?;
    let sec = parts[1].parse::<f64>().ok()?;
    let frames = parts[2].parse::<f64>().ok()?; // 75 frames per second in Red Book CD
    Some(min * 60.0 + sec + frames / 75.0)
}

pub(crate) fn parse_cue_sheet(raw_bytes: &[u8]) -> DomainResult<ParsedCueSheet> {
    let text = decode_cue_bytes(raw_bytes);
    let mut album_title = None;
    let mut album_performer = None;
    let mut audio_file = None;
    let mut tracks: Vec<CueTrackEntry> = Vec::new();
    let mut current: Option<CueTrackEntry> = None;

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        let upper = line.to_ascii_uppercase();
        if upper.starts_with("FILE ") {
            audio_file = Some(extract_cue_quoted(line));
        } else if upper.starts_with("TRACK ") {
            if let Some(tr) = current.take() {
                tracks.push(tr);
            }
            let num = line
                .split_whitespace()
                .nth(1)
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or((tracks.len() + 1) as u32);
            current = Some(CueTrackEntry {
                number: num,
                title: format!("Track {num:02}"),
                performer: None,
                start_secs: 0.0,
            });
        } else if upper.starts_with("TITLE ") {
            let val = extract_cue_quoted(line);
            if let Some(ref mut tr) = current {
                tr.title = val;
            } else {
                album_title = Some(val);
            }
        } else if upper.starts_with("PERFORMER ") {
            let val = extract_cue_quoted(line);
            if let Some(ref mut tr) = current {
                tr.performer = Some(val);
            } else {
                album_performer = Some(val);
            }
        } else if upper.starts_with("INDEX 01 ")
            && let Some(ref mut tr) = current
            && let Some(ts) = line.split_whitespace().nth(2)
            && let Some(secs) = parse_cue_index_secs(ts)
        {
            tr.start_secs = secs;
        }
    }
    if let Some(tr) = current.take() {
        tracks.push(tr);
    }

    let audio_file = audio_file
        .filter(|s| !s.is_empty())
        .ok_or_else(|| DomainError::Validation("CUE sheet missing FILE entry".into()))?;
    if tracks.is_empty() {
        return Err(DomainError::Validation(
            "CUE sheet contains no TRACK entries".into(),
        ));
    }

    Ok(ParsedCueSheet {
        album_title,
        album_performer,
        audio_file,
        tracks,
    })
}

fn sanitize_track_filename(raw: &str) -> String {
    raw.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        })
        .collect::<String>()
        .trim()
        .to_string()
}

pub fn split_cue_album(
    audio_or_cue_path: &Path,
    ffmpeg_bin: &str,
) -> DomainResult<CueSplitReportDto> {
    let cue_path = if audio_or_cue_path
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("cue"))
    {
        audio_or_cue_path.to_path_buf()
    } else {
        audio_or_cue_path.with_extension("cue")
    };

    if !cue_path.is_file() {
        return Err(DomainError::NotFound(format!(
            "companion CUE sheet '{}' not found",
            cue_path.display()
        )));
    }

    let raw_cue = std::fs::read(&cue_path)
        .map_err(|e| DomainError::Storage(format!("read cue sheet failed: {e}")))?;
    let parsed = parse_cue_sheet(&raw_cue)?;
    let parent = cue_path.parent().unwrap_or_else(|| Path::new("."));
    let canon_parent = parent
        .canonicalize()
        .map_err(|e| DomainError::Storage(format!("resolve cue directory failed: {e}")))?;

    let audio_rel = Path::new(&parsed.audio_file);
    if audio_rel.is_absolute()
        || audio_rel.components().any(|c| {
            matches!(
                c,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(DomainError::Validation(format!(
            "CUE audio path '{}' escapes parent directory",
            parsed.audio_file
        )));
    }

    let mut source_audio = parent.join(audio_rel);
    if !source_audio.is_file() {
        // Fallback: if CUE references .wav but actual file is .flac/.ape with same stem
        for ext in ["flac", "wav", "ape", "wv", "m4a"] {
            let candidate = cue_path.with_extension(ext);
            if candidate.is_file() {
                source_audio = candidate;
                break;
            }
        }
    }
    if !source_audio.is_file() {
        return Err(DomainError::NotFound(format!(
            "source audio '{}' referenced by CUE not found",
            parsed.audio_file
        )));
    }
    let canon_source_audio = source_audio
        .canonicalize()
        .map_err(|e| DomainError::Storage(format!("resolve CUE source audio failed: {e}")))?;
    if !canon_source_audio.starts_with(&canon_parent) {
        return Err(DomainError::Validation(format!(
            "CUE audio path '{}' escapes parent directory",
            parsed.audio_file
        )));
    }
    source_audio = canon_source_audio;

    let mut created_files = Vec::new();
    for i in 0..parsed.tracks.len() {
        let tr = &parsed.tracks[i];
        let next_start = parsed.tracks.get(i + 1).map(|n| n.start_secs);
        let safe_title = sanitize_track_filename(&tr.title);
        let out_name = format!("{:02} - {}.flac", tr.number, safe_title);
        let out_path = parent.join(&out_name);

        let mut args: Vec<String> = vec![
            "-y".into(),
            "-hide_banner".into(),
            "-loglevel".into(),
            "error".into(),
            "-i".into(),
            source_audio.to_string_lossy().into_owned(),
            "-ss".into(),
            format!("{:.3}", tr.start_secs),
        ];
        if let Some(end_s) = next_start {
            args.push("-to".into());
            args.push(format!("{end_s:.3}"));
        }
        args.extend([
            "-c:a".into(),
            "flac".into(),
            out_path.to_string_lossy().into_owned(),
        ]);

        let status = Command::new(ffmpeg_bin)
            .args(&args)
            .status()
            .map_err(|e| DomainError::Storage(format!("spawn ffmpeg cue split failed: {e}")))?;
        if !status.success() {
            return Err(DomainError::Storage(format!(
                "ffmpeg failed splitting track {:02}",
                tr.number
            )));
        }

        // Write standard metadata tags into the newly split FLAC
        if let Ok(mut tf) = open_tagged(&out_path) {
            let tag = ensure_mutable_primary_tag(&mut tf);
            tag.set_title(tr.title.clone());
            tag.set_track(tr.number);
            if let Some(ref album) = parsed.album_title {
                tag.set_album(album.clone());
            }
            if let Some(perf) = tr.performer.as_ref().or(parsed.album_performer.as_ref()) {
                tag.set_artist(perf.clone());
            }
            if let Some(ref album_artist) = parsed.album_performer {
                tag.insert_text(ItemKey::AlbumArtist, album_artist.clone());
            }
            let _ = tf.save_to_path(&out_path, WriteOptions::default());
        }

        created_files.push(out_name);
    }

    Ok(CueSplitReportDto {
        created_files,
        album_title: parsed.album_title,
        album_artist: parsed.album_performer,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lrc_transform_shifts_timestamps_strips_credits_and_converts_zh() {
        let raw = "[00:01.00]作词：方文山\n[00:02.50]QQ音乐独家首发\n[00:10.50]故事的小黃花\n[00:00.20]從出生那年就飄著";
        let (out, removed) = transform_lrc_text(raw, -500, true, Some(ZhConvertDirection::T2s));
        assert_eq!(removed, 2);
        assert_eq!(out, "[00:10.00]故事的小黄花\n[00:00.00]从出生那年就飘着");
    }

    #[test]
    fn cue_parser_extracts_album_and_track_timestamps() {
        let cue = r#"PERFORMER "周杰伦"
TITLE "叶惠美"
FILE "YeHuiMei.flac" WAVE
  TRACK 01 AUDIO
    TITLE "以父之名"
    PERFORMER "周杰伦"
    INDEX 01 00:00:00
  TRACK 02 AUDIO
    TITLE "晴天"
    INDEX 01 05:42:37
"#;
        let parsed = parse_cue_sheet(cue.as_bytes()).unwrap();
        assert_eq!(parsed.album_title.as_deref(), Some("叶惠美"));
        assert_eq!(parsed.album_performer.as_deref(), Some("周杰伦"));
        assert_eq!(parsed.audio_file, "YeHuiMei.flac");
        assert_eq!(parsed.tracks.len(), 2);
        assert_eq!(parsed.tracks[1].title, "晴天");
        assert!((parsed.tracks[1].start_secs - (5.0 * 60.0 + 42.0 + 37.0 / 75.0)).abs() < 0.01);
    }

    #[test]
    fn split_cue_album_rejects_audio_file_path_traversal() {
        let tmp = tempfile::tempdir().unwrap();
        let outside = tmp.path().join("secret.flac");
        std::fs::write(&outside, b"secret").unwrap();
        let album_dir = tmp.path().join("album");
        std::fs::create_dir_all(&album_dir).unwrap();

        let traversal_cue = album_dir.join("evil.cue");
        std::fs::write(
            &traversal_cue,
            "FILE \"../secret.flac\" WAVE\n  TRACK 01 AUDIO\n    TITLE \"T1\"\n    INDEX 01 00:00:00\n",
        )
        .unwrap();
        let err = split_cue_album(&traversal_cue, "ffmpeg").unwrap_err();
        assert!(matches!(err, DomainError::Validation(_)));

        let abs_cue = album_dir.join("abs.cue");
        std::fs::write(
            &abs_cue,
            format!(
                "FILE \"{}\" WAVE\n  TRACK 01 AUDIO\n    TITLE \"T1\"\n    INDEX 01 00:00:00\n",
                outside.display()
            ),
        )
        .unwrap();
        let err_abs = split_cue_album(&abs_cue, "ffmpeg").unwrap_err();
        assert!(matches!(err_abs, DomainError::Validation(_)));
    }
}

use std::path::Path;

use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_formats::decode_text_smart;
use xmedia_formats::music::{AudioMetaPatch, apply_audio_patch_in_place};

use super::CueSplitReportDto;

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
    let frames = parts[2].parse::<f64>().ok()?;
    Some(min * 60.0 + sec + frames / 75.0)
}

pub(crate) fn parse_cue_sheet(raw_bytes: &[u8]) -> DomainResult<ParsedCueSheet> {
    let (decoded, _) = decode_text_smart(raw_bytes);
    let text = decoded.trim_start_matches('\u{feff}');
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

        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let out = crate::infra::inspectors::process::run_command_bounded(
            ffmpeg_bin,
            &arg_refs,
            std::time::Duration::from_secs(300),
            2 * 1024 * 1024,
        )?;
        if !out.status.success() {
            return Err(DomainError::Storage(format!(
                "ffmpeg failed splitting track {:02}",
                tr.number
            )));
        }

        let _ = apply_audio_patch_in_place(
            &out_path,
            &AudioMetaPatch {
                title: Some(tr.title.clone()),
                track_number: Some(tr.number),
                album: parsed.album_title.clone(),
                artist: tr
                    .performer
                    .clone()
                    .or_else(|| parsed.album_performer.clone()),
                album_artist: parsed.album_performer.clone(),
                ..Default::default()
            },
        );

        created_files.push(out_name);
    }

    Ok(CueSplitReportDto {
        created_files,
        album_title: parsed.album_title,
        album_artist: parsed.album_performer,
    })
}

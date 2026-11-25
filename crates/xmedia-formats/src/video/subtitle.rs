use crate::common::decode_text_smart;

/// Converts SubRip (`.srt`) text into WebVTT.
pub fn convert_srt_to_vtt(srt: &str) -> String {
    let mut out = String::with_capacity(srt.len() + 16);
    out.push_str("WEBVTT\n\n");
    for raw_line in srt.trim_start_matches('\u{feff}').lines() {
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

/// Converts Advanced SubStation Alpha (`.ass` / `.ssa`) text into WebVTT, stripping override tags
/// (`{\pos(...)}`) and collapsing empty lines inside multi-line dialogue cues.
pub fn convert_ass_to_vtt(ass: &str) -> String {
    let mut out = String::from("WEBVTT\n\n");
    let mut in_events = false;
    let mut start_col = 1usize;
    let mut end_col = 2usize;
    let mut text_col = 9usize;

    for raw_line in ass.trim_start_matches('\u{feff}').lines() {
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

/// Decodes arbitrary-charset subtitle bytes (`.srt`, `.ass`, `.ssa`, `.vtt`) and converts them to WebVTT.
pub fn convert_subtitle_bytes_to_vtt(format_ext: &str, raw_bytes: &[u8]) -> String {
    let (decoded, _) = decode_text_smart(raw_bytes);
    let text = decoded.trim_start_matches('\u{feff}');
    match format_ext
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

/// Rebases WebVTT cue timestamps by subtracting `seek_offset_secs` (dropping cues that ended before `seek_offset_secs`).
pub fn shift_webvtt_timestamps(vtt: &str, seek_offset_secs: f64) -> String {
    if !seek_offset_secs.is_finite() || seek_offset_secs <= 0.0 {
        return vtt.to_string();
    }
    let offset_ms = (seek_offset_secs * 1000.0).round() as i64;
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
    converted
        .lines()
        .map(str::trim_end)
        .filter(|l| !l.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
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

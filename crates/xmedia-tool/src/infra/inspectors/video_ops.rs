use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use xmedia_core::domain::error::{DomainError, DomainResult};

use crate::infra::atomic_fs::atomic_write_bytes;
use crate::infra::inspectors::zh_text::{ZhConvertDirection, convert_zh_plain};

const SUB_EXTS: &[&str] = &["srt", "ass", "ssa", "vtt"];

static RE_SRT_TIMING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(?:(\d{1,2}):)?(\d{2}):(\d{2})[,.](\d{3})\s*-->\s*(?:(\d{1,2}):)?(\d{2}):(\d{2})[,.](\d{3})(.*)$",
    )
    .expect("valid srt timing regex")
});

static RE_ASS_DIALOGUE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(Dialogue:\s*[^,]*,)(\d+):(\d{2}):(\d{2})\.(\d{2}),(\d+):(\d{2}):(\d{2})\.(\d{2}),([^,]*,[^,]*,[^,]*,[^,]*,[^,]*,[^,]*,)(.*)$",
    )
    .expect("valid ass dialogue regex")
});

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoSubtitleInfoDto {
    pub filename: String,
    pub extension: String,
    pub file_size: u64,
    pub detected_encoding: String,
    pub is_utf8: bool,
    pub language_tag: Option<String>,
    pub forced: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ProcessSubtitleInput {
    pub subtitle_filename: String,
    #[serde(default)]
    pub convert_utf8: bool,
    #[serde(default)]
    pub offset_ms: i64,
    #[serde(default)]
    pub zh_direction: Option<ZhConvertDirection>,
    /// Optional Jellyfin language tag to rename to, e.g. "zh-CN", "zh-TW", "en", "ja"
    pub rename_language: Option<String>,
    #[serde(default)]
    pub forced: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessSubtitleReportDto {
    pub original_filename: String,
    pub final_filename: String,
    pub original_encoding: String,
    pub shifted_cues: usize,
    pub converted_utf8: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractSubtitleReportDto {
    pub created_filename: String,
    pub file_size: u64,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct BatchRemapEpisodesInput {
    pub show_title: Option<String>,
    pub season_number: Option<u32>,
    /// "sequential" | "absolute_offset"
    #[serde(default = "default_remap_mode")]
    pub mode: String,
    #[serde(default = "default_one")]
    pub start_episode: u32,
    /// Used when mode == "absolute_offset": new_ep = old_ep + episode_offset (e.g. -24 for S02)
    #[serde(default)]
    pub episode_offset: i32,
    #[serde(default)]
    pub write_season_nfo: bool,
}

fn default_remap_mode() -> String {
    "sequential".to_string()
}

fn default_one() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchRemapEpisodesReportDto {
    pub updated_count: usize,
    pub failed_paths: Vec<String>,
}

fn detect_encoding_and_decode(bytes: &[u8]) -> (String, String, bool) {
    let stripped = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    if let Ok(s) = std::str::from_utf8(stripped) {
        return ("UTF-8".to_string(), s.to_string(), true);
    }
    let mut det = chardetng::EncodingDetector::new(chardetng::Iso2022JpDetection::Allow);
    det.feed(bytes, true);
    let enc = det.guess(None, chardetng::Utf8Detection::Allow);
    let (decoded, _, _) = enc.decode(bytes);
    (
        enc.name().to_string(),
        decoded.trim_start_matches('\u{feff}').to_string(),
        false,
    )
}

fn parse_subtitle_lang_and_forced(video_stem: &str, sub_filename: &str) -> (Option<String>, bool) {
    let without_ext = Path::new(sub_filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(sub_filename);
    let suffix = if without_ext
        .get(..video_stem.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(video_stem))
    {
        without_ext[video_stem.len()..].trim_start_matches(['.', '-', '_', ' '])
    } else {
        ""
    };
    if suffix.is_empty() {
        return (None, false);
    }
    let parts: Vec<&str> = suffix
        .split(['.', '_', ' '])
        .filter(|s| !s.is_empty())
        .collect();
    let sub_tokens: Vec<String> = suffix
        .split(['.', '_', '-', ' '])
        .filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_lowercase())
        .collect();
    let forced = sub_tokens.iter().any(|p| p == "forced");
    let has_token = |candidates: &[&str]| {
        parts
            .iter()
            .any(|p| candidates.iter().any(|c| p.eq_ignore_ascii_case(c)))
            || sub_tokens.iter().any(|p| candidates.iter().any(|c| p == c))
    };

    let lang = if has_token(&["zh-cn", "zh-hans", "chs", "sc", "gb", "简体"]) {
        Some("zh-CN".to_string())
    } else if has_token(&["zh-tw", "zh-hk", "zh-hant", "cht", "tc", "big5", "繁体"]) {
        Some("zh-TW".to_string())
    } else if has_token(&["zh", "chi", "zho"]) {
        Some("zh-CN".to_string())
    } else if has_token(&["en", "eng", "en-us", "en-gb"]) {
        Some("en".to_string())
    } else if has_token(&["ja", "jpn", "jp"]) {
        Some("ja".to_string())
    } else {
        parts
            .first()
            .filter(|p| !p.eq_ignore_ascii_case("forced"))
            .map(|p| (*p).to_string())
    };
    (lang, forced)
}

pub fn list_video_subtitles(video_path: &Path) -> DomainResult<Vec<VideoSubtitleInfoDto>> {
    let parent = video_path
        .parent()
        .ok_or_else(|| DomainError::Validation("video path has no parent dir".into()))?;
    let stem = video_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    let stem_lower = stem.to_ascii_lowercase();

    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(parent) else {
        return Ok(out);
    };
    for entry in rd.flatten() {
        let p = entry.path();
        if !p.is_file() {
            continue;
        }
        let ext = p
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default();
        if !SUB_EXTS.contains(&ext.as_str()) {
            continue;
        }
        let Some(fname) = p.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let fname_lower = fname.to_ascii_lowercase();
        if !fname_lower.starts_with(&stem_lower) {
            continue;
        }
        let remainder = &fname_lower[stem_lower.len()..];
        if !remainder.starts_with('.') && !remainder.starts_with('_') && !remainder.starts_with('-')
        {
            continue;
        }
        let bytes = std::fs::read(&p).unwrap_or_default();
        let (enc, _, is_utf8) = detect_encoding_and_decode(&bytes);
        let (lang, forced) = parse_subtitle_lang_and_forced(stem, fname);
        out.push(VideoSubtitleInfoDto {
            filename: fname.to_string(),
            extension: ext,
            file_size: bytes.len() as u64,
            detected_encoding: enc,
            is_utf8,
            language_tag: lang,
            forced,
        });
    }
    out.sort_by(|a, b| a.filename.cmp(&b.filename));
    Ok(out)
}

fn format_srt_ms(total_ms: i64, dot_sep: bool) -> String {
    let clamped = total_ms.max(0);
    let h = clamped / 3_600_000;
    let m = (clamped % 3_600_000) / 60_000;
    let s = (clamped % 60_000) / 1_000;
    let ms = clamped % 1_000;
    let sep = if dot_sep { '.' } else { ',' };
    format!("{h:02}:{m:02}:{s:02}{sep}{ms:03}")
}

fn format_ass_ms(total_ms: i64) -> String {
    let clamped = total_ms.max(0);
    let h = clamped / 3_600_000;
    let m = (clamped % 3_600_000) / 60_000;
    let s = (clamped % 60_000) / 1_000;
    let cs = (clamped % 1_000) / 10;
    format!("{h}:{m:02}:{s:02}.{cs:02}")
}

/// Converts Zh text in ASS dialogue while skipping `{...}` override tags and `\N`/`\n` escapes.
fn convert_ass_dialogue_text(text: &str, dir: ZhConvertDirection) -> String {
    let mut out = String::with_capacity(text.len());
    let mut buf = String::new();
    let mut in_brace = false;

    for ch in text.chars() {
        if in_brace {
            out.push(ch);
            if ch == '}' {
                in_brace = false;
            }
        } else if ch == '{' {
            if !buf.is_empty() {
                out.push_str(&convert_zh_plain(&buf, dir));
                buf.clear();
            }
            in_brace = true;
            out.push(ch);
        } else {
            buf.push(ch);
        }
    }
    if !buf.is_empty() {
        out.push_str(&convert_zh_plain(&buf, dir));
    }
    out
}

pub fn transform_subtitle_content(
    raw: &str,
    ext: &str,
    offset_ms: i64,
    zh_direction: Option<ZhConvertDirection>,
) -> (String, usize) {
    let is_ass = matches!(ext, "ass" | "ssa");
    let is_vtt = ext == "vtt";
    let mut out_lines = Vec::new();
    let mut shifted_cues = 0usize;

    for raw_line in raw.replace("\r\n", "\n").lines() {
        if is_ass {
            if let Some(caps) = RE_ASS_DIALOGUE.captures(raw_line) {
                let mut start_ms = caps[2].parse::<i64>().unwrap_or(0) * 3_600_000
                    + caps[3].parse::<i64>().unwrap_or(0) * 60_000
                    + caps[4].parse::<i64>().unwrap_or(0) * 1_000
                    + caps[5].parse::<i64>().unwrap_or(0) * 10;
                let mut end_ms = caps[6].parse::<i64>().unwrap_or(0) * 3_600_000
                    + caps[7].parse::<i64>().unwrap_or(0) * 60_000
                    + caps[8].parse::<i64>().unwrap_or(0) * 1_000
                    + caps[9].parse::<i64>().unwrap_or(0) * 10;
                if offset_ms != 0 {
                    start_ms += offset_ms;
                    end_ms += offset_ms;
                    shifted_cues += 1;
                }
                let body = if let Some(dir) = zh_direction {
                    convert_ass_dialogue_text(&caps[11], dir)
                } else {
                    caps[11].to_string()
                };
                out_lines.push(format!(
                    "{}{},{},{}{}",
                    &caps[1],
                    format_ass_ms(start_ms),
                    format_ass_ms(end_ms),
                    &caps[10],
                    body
                ));
            } else {
                out_lines.push(raw_line.to_string());
            }
        } else if let Some(caps) = RE_SRT_TIMING.captures(raw_line) {
            if offset_ms != 0 {
                let start_h = caps
                    .get(1)
                    .and_then(|m| m.as_str().parse::<i64>().ok())
                    .unwrap_or(0);
                let end_h = caps
                    .get(5)
                    .and_then(|m| m.as_str().parse::<i64>().ok())
                    .unwrap_or(0);
                let start_ms = start_h * 3_600_000
                    + caps[2].parse::<i64>().unwrap_or(0) * 60_000
                    + caps[3].parse::<i64>().unwrap_or(0) * 1_000
                    + caps[4].parse::<i64>().unwrap_or(0)
                    + offset_ms;
                let end_ms = end_h * 3_600_000
                    + caps[6].parse::<i64>().unwrap_or(0) * 60_000
                    + caps[7].parse::<i64>().unwrap_or(0) * 1_000
                    + caps[8].parse::<i64>().unwrap_or(0)
                    + offset_ms;
                shifted_cues += 1;
                out_lines.push(format!(
                    "{} --> {}{}",
                    format_srt_ms(start_ms, is_vtt),
                    format_srt_ms(end_ms, is_vtt),
                    &caps[9]
                ));
            } else {
                out_lines.push(raw_line.to_string());
            }
        } else if let Some(dir) = zh_direction {
            if raw_line.trim().chars().all(|c| c.is_ascii_digit()) {
                out_lines.push(raw_line.to_string());
            } else {
                out_lines.push(convert_zh_plain(raw_line, dir));
            }
        } else {
            out_lines.push(raw_line.to_string());
        }
    }

    (out_lines.join("\n"), shifted_cues)
}

fn validate_subtitle_lang_tag(raw: &str) -> DomainResult<&str> {
    let lang = raw.trim();
    if lang.is_empty()
        || lang.len() > 64
        || lang.contains('/')
        || lang.contains('\\')
        || lang.contains("..")
        || lang.starts_with('.')
        || lang.ends_with('.')
        || lang
            .chars()
            .any(|c| c.is_control() || matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|'))
    {
        return Err(DomainError::Validation(format!(
            "invalid subtitle language tag '{lang}'"
        )));
    }
    Ok(lang)
}

pub fn process_video_subtitle(
    video_path: &Path,
    input: &ProcessSubtitleInput,
) -> DomainResult<ProcessSubtitleReportDto> {
    let parent = video_path
        .parent()
        .ok_or_else(|| DomainError::Validation("video path has no parent dir".into()))?;
    if input.subtitle_filename.is_empty()
        || input.subtitle_filename.contains('/')
        || input.subtitle_filename.contains('\\')
        || input.subtitle_filename.contains("..")
        || input.subtitle_filename.chars().any(char::is_control)
    {
        return Err(DomainError::Validation("invalid subtitle filename".into()));
    }
    let sub_path = parent.join(&input.subtitle_filename);
    if sub_path.parent() != Some(parent) {
        return Err(DomainError::Validation("invalid subtitle filename".into()));
    }
    if !sub_path.is_file() {
        return Err(DomainError::NotFound(format!(
            "subtitle '{}' not found",
            input.subtitle_filename
        )));
    }
    let ext = sub_path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_else(|| "srt".to_string());

    let raw_bytes = std::fs::read(&sub_path)
        .map_err(|e| DomainError::Storage(format!("read subtitle failed: {e}")))?;
    let (orig_encoding, decoded, was_utf8) = detect_encoding_and_decode(&raw_bytes);

    let (transformed, shifted_cues) =
        transform_subtitle_content(&decoded, &ext, input.offset_ms, input.zh_direction);

    let should_rewrite =
        input.convert_utf8 || input.offset_ms != 0 || input.zh_direction.is_some() || !was_utf8;

    let mut target_path: PathBuf = sub_path.clone();
    let mut final_filename = input.subtitle_filename.clone();
    if let Some(ref lang_raw) = input.rename_language {
        let trimmed = lang_raw.trim();
        if !trimmed.is_empty() {
            let lang = validate_subtitle_lang_tag(trimmed)?;
            let video_stem = video_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("video");
            let forced_part = if input.forced { ".forced" } else { "" };
            final_filename = format!("{video_stem}.{lang}{forced_part}.{ext}");
            target_path = parent.join(&final_filename);
            if target_path.parent() != Some(parent) {
                return Err(DomainError::Validation(
                    "subtitle rename target escapes parent directory".into(),
                ));
            }
        }
    }

    if should_rewrite {
        atomic_write_bytes(&target_path, transformed.as_bytes())?;
        if target_path != sub_path {
            let _ = std::fs::remove_file(&sub_path);
        }
    } else if target_path != sub_path {
        std::fs::rename(&sub_path, &target_path)
            .map_err(|e| DomainError::Storage(format!("rename subtitle failed: {e}")))?;
    }

    Ok(ProcessSubtitleReportDto {
        original_filename: input.subtitle_filename.clone(),
        final_filename,
        original_encoding: orig_encoding,
        shifted_cues,
        converted_utf8: should_rewrite && !was_utf8,
    })
}

pub fn extract_embedded_subtitle(
    video_path: &Path,
    stream_index: u32,
    language_tag: Option<&str>,
    output_ext: &str,
    ffmpeg_bin: &str,
) -> DomainResult<ExtractSubtitleReportDto> {
    let parent = video_path
        .parent()
        .ok_or_else(|| DomainError::Validation("video path has no parent dir".into()))?;
    let stem = video_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("video");
    let ext = match output_ext.to_ascii_lowercase().as_str() {
        "ass" => "ass",
        "vtt" => "vtt",
        _ => "srt",
    };
    let lang = match language_tag.map(str::trim).filter(|s| !s.is_empty()) {
        Some(raw) => validate_subtitle_lang_tag(raw)?,
        None => "zh-CN",
    };
    let created_filename = format!("{stem}.{lang}.{ext}");
    let out_path = parent.join(&created_filename);
    if out_path.parent() != Some(parent) {
        return Err(DomainError::Validation(
            "subtitle output path escapes parent directory".into(),
        ));
    }

    let map_arg = format!("0:{stream_index}");
    let video_str = video_path.to_string_lossy();
    let out_str = out_path.to_string_lossy();
    let out = crate::infra::inspectors::process::run_command_bounded(
        ffmpeg_bin,
        &[
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-i",
            &video_str,
            "-map",
            &map_arg,
            &out_str,
        ],
        std::time::Duration::from_secs(180),
        2 * 1024 * 1024,
    )?;

    if !out.status.success() || !out_path.is_file() {
        return Err(DomainError::Storage(format!(
            "ffmpeg failed extracting subtitle stream 0:{stream_index}"
        )));
    }

    let file_size = std::fs::metadata(&out_path).map(|m| m.len()).unwrap_or(0);
    Ok(ExtractSubtitleReportDto {
        created_filename,
        file_size,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srt_and_ass_transforms_shift_time_and_preserve_ass_override_tags() {
        let srt = "1\n00:00:05,500 --> 00:00:08,000\n軟體開發測試\n";
        let (out_srt, cues) =
            transform_subtitle_content(srt, "srt", 1500, Some(ZhConvertDirection::T2s));
        assert_eq!(cues, 1);
        assert!(out_srt.contains("00:00:07,000 --> 00:00:09,500"));
        assert!(out_srt.contains("软件开发测试"));

        let ass = "Dialogue: 0,0:00:10.50,0:00:13.20,Default,,0,0,0,,{\\fn黑體\\fs24}這是一段測試";
        let (out_ass, ass_cues) =
            transform_subtitle_content(ass, "ass", -500, Some(ZhConvertDirection::T2s));
        assert_eq!(ass_cues, 1);
        assert!(out_ass.contains("0:00:10.00,0:00:12.70"));
        // Style tag inside {...} must stay untouched while dialogue text converts to simplified
        assert!(out_ass.contains("{\\fn黑體\\fs24}这是一段测试"));
    }

    #[test]
    fn subtitle_gbk_to_utf8_and_jellyfin_rename_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let vid = tmp.path().join("Frieren - S01E01.mkv");
        std::fs::write(&vid, b"dummy").unwrap();
        let sub = tmp.path().join("Frieren - S01E01.chs.srt");
        let (gbk_bytes, _, _) =
            encoding_rs::GB18030.encode("1\n00:00:01,000 --> 00:00:03,000\n你好世界\n");
        std::fs::write(&sub, gbk_bytes).unwrap();

        let listed = list_video_subtitles(&vid).unwrap();
        assert_eq!(listed.len(), 1);
        assert!(!listed[0].is_utf8);
        assert_eq!(listed[0].language_tag.as_deref(), Some("zh-CN"));

        let rep = process_video_subtitle(
            &vid,
            &ProcessSubtitleInput {
                subtitle_filename: "Frieren - S01E01.chs.srt".into(),
                convert_utf8: true,
                offset_ms: 500,
                zh_direction: None,
                rename_language: Some("zh-CN".into()),
                forced: false,
            },
        )
        .unwrap();
        assert_eq!(rep.final_filename, "Frieren - S01E01.zh-CN.srt");
        assert!(rep.converted_utf8);
        let final_text = std::fs::read_to_string(tmp.path().join(&rep.final_filename)).unwrap();
        assert!(final_text.contains("00:00:01,500 --> 00:00:03,500"));
        assert!(final_text.contains("你好世界"));
    }

    #[test]
    fn subtitle_language_tag_blocks_path_traversal_and_control_chars() {
        let tmp = tempfile::tempdir().unwrap();
        let vid = tmp.path().join("Movie.mkv");
        std::fs::write(&vid, b"dummy").unwrap();
        let sub = tmp.path().join("Movie.chs.srt");
        std::fs::write(&sub, b"1\n00:00:01,000 --> 00:00:02,000\nHi\n").unwrap();

        for bad_lang in [
            "../../pwn",
            "zh/../../x",
            "zh\\..\\x",
            "zh..cn",
            "zh\x00cn",
            "zh\ncn",
        ] {
            let err = process_video_subtitle(
                &vid,
                &ProcessSubtitleInput {
                    subtitle_filename: "Movie.chs.srt".into(),
                    rename_language: Some(bad_lang.into()),
                    ..Default::default()
                },
            )
            .unwrap_err();
            assert!(matches!(err, DomainError::Validation(_)));

            let ext_err =
                extract_embedded_subtitle(&vid, 0, Some(bad_lang), "srt", "ffmpeg").unwrap_err();
            assert!(matches!(ext_err, DomainError::Validation(_)));
        }
    }
}

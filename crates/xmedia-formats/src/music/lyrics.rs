use std::fs::File;
use std::io::{BufReader, Read, Seek};
use std::path::Path;
use std::sync::LazyLock;

use lofty::config::{ParseOptions, ParsingMode};
use lofty::file::AudioFile;
use lofty::id3::v2::{Frame, SynchronizedTextFrame};
use lofty::mpeg::MpegFile;
use regex::Regex;
use serde::{Deserialize, Serialize};

use super::{collect_ordered_tags, open_tagged_reader};
use crate::common::decode_text_smart;

static RE_LRC_TAG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\[(\d{1,3}):(\d{2})(?:[.:](\d{1,3}))?\]").expect("valid lrc timestamp regex")
});

static RE_CREDIT_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^\s*(?:作词|作曲|编曲|制作人|监制|混音|母带|录音|吉他|贝斯|鼓|弦乐|和声|配唱|发行|出品|企划|统筹|版权|歌词制作|LRC制作|词|曲|Lyricist|Composer|Arranger|Producer|Mixing|Mastering)\s*[:：]|(?:QQ音乐|网易云音乐|酷狗音乐|酷我音乐|咪咕音乐|汽水音乐|版权所有|未经许可)",
    )
    .expect("valid credit noise regex")
});

/// Single timestamped line in synchronized lyrics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LyricLine {
    pub start_ms: u64,
    pub text: String,
}

/// Parsed synchronized or plain-text lyrics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuredLyrics {
    pub lang: String,
    pub synced: bool,
    pub display_artist: Option<String>,
    pub display_title: Option<String>,
    pub offset_ms: i64,
    pub lines: Vec<LyricLine>,
    pub plain_text: String,
}

impl StructuredLyrics {
    pub fn empty() -> Self {
        Self {
            lang: "und".to_string(),
            synced: false,
            display_artist: None,
            display_title: None,
            offset_ms: 0,
            lines: Vec::new(),
            plain_text: String::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty() && self.plain_text.trim().is_empty()
    }
}

/// Decodes `.lrc` bytes with automatic charset detection (UTF-8, UTF-16, GB18030, Big5, Shift-JIS)
/// and parses synchronized or plain-text lyrics.
pub fn parse_lrc_bytes(raw_bytes: &[u8]) -> StructuredLyrics {
    let (decoded, _) = decode_text_smart(raw_bytes);
    parse_lyrics_str(&decoded)
}

/// Parses an `.lrc` or plain lyrics string into `StructuredLyrics`.
pub fn parse_lyrics_str(input: &str) -> StructuredLyrics {
    let mut lang = "und".to_string();
    let mut display_artist: Option<String> = None;
    let mut display_title: Option<String> = None;
    let mut offset_ms: i64 = 0;
    let mut synced_lines: Vec<LyricLine> = Vec::new();
    let mut plain_lines: Vec<String> = Vec::new();

    for raw_line in input.trim_start_matches('\u{feff}').lines() {
        let line = raw_line.trim_end_matches('\r').trim();
        if line.is_empty() {
            plain_lines.push(String::new());
            continue;
        }

        if let Some((tag_key, tag_val)) = parse_lrc_meta_tag(line) {
            match tag_key.as_str() {
                "ar" if !tag_val.is_empty() => display_artist = Some(tag_val),
                "ti" if !tag_val.is_empty() => display_title = Some(tag_val),
                "la" if !tag_val.is_empty() => lang = tag_val.to_ascii_lowercase(),
                "offset" => {
                    if let Ok(off) = tag_val.parse::<i64>() {
                        offset_ms = off;
                    }
                }
                _ => {}
            }
            continue;
        }

        let (timestamps, lyric_body) = extract_leading_timestamps(line);
        if !timestamps.is_empty() {
            let body = lyric_body.trim().to_string();
            for ts in timestamps {
                synced_lines.push(LyricLine {
                    start_ms: ts,
                    text: body.clone(),
                });
            }
        } else {
            plain_lines.push(line.to_string());
        }
    }

    if !synced_lines.is_empty() {
        synced_lines.sort_by_key(|l| l.start_ms);
        let plain = synced_lines
            .iter()
            .filter(|l| !l.text.is_empty())
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        return StructuredLyrics {
            lang,
            synced: true,
            display_artist,
            display_title,
            offset_ms,
            lines: synced_lines,
            plain_text: plain,
        };
    }

    let plain_text = plain_lines.join("\n").trim().to_string();
    StructuredLyrics {
        lang,
        synced: false,
        display_artist,
        display_title,
        offset_ms: 0,
        lines: Vec::new(),
        plain_text,
    }
}

/// Extracts embedded lyrics (`SYLT` binary frame on MP3, or `Lyrics` / `UnsyncLyrics` text items)
/// from a `Read + Seek` audio stream.
pub fn extract_embedded_lyrics_from_reader<R: Read + Seek>(
    reader: &mut R,
    ext_hint: &str,
) -> Option<StructuredLyrics> {
    let opts = ParseOptions::new().parsing_mode(ParsingMode::Relaxed);
    if reader.rewind().is_ok()
        && let Ok(mp3) = MpegFile::read_from(&mut *reader, opts)
        && let Some(id3v2) = mp3.id3v2()
    {
        for frame in id3v2 {
            if frame.id_str() == "SYLT"
                && let Frame::Binary(bin) = frame
                && let Ok(sylt) = SynchronizedTextFrame::parse(&bin.data, frame.flags())
                && !sylt.content.is_empty()
            {
                let lang = std::str::from_utf8(&sylt.language)
                    .unwrap_or("und")
                    .trim_matches('\0')
                    .to_ascii_lowercase();
                let mut lines: Vec<LyricLine> = sylt
                    .content
                    .into_iter()
                    .map(|(ts, txt)| LyricLine {
                        start_ms: u64::from(ts),
                        text: txt.trim().to_string(),
                    })
                    .collect();
                lines.sort_by_key(|l| l.start_ms);
                let plain = lines
                    .iter()
                    .filter(|l| !l.text.is_empty())
                    .map(|l| l.text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n");
                return Some(StructuredLyrics {
                    lang: if lang.is_empty() {
                        "und".to_string()
                    } else {
                        lang
                    },
                    synced: true,
                    display_artist: None,
                    display_title: None,
                    offset_ms: 0,
                    lines,
                    plain_text: plain,
                });
            }
        }
    }

    let _ = reader.rewind();
    let tagged = open_tagged_reader(reader, ext_hint).ok()?;
    for tag in collect_ordered_tags(&tagged) {
        for key in [
            lofty::tag::ItemKey::Lyrics,
            lofty::tag::ItemKey::UnsyncLyrics,
        ] {
            if let Some(text) = tag.get_string(key)
                && !text.trim().is_empty()
            {
                return Some(parse_lyrics_str(text));
            }
        }
    }
    None
}

/// Extracts embedded lyrics from an audio file on disk.
pub fn extract_embedded_lyrics_from_path(path: &Path) -> Option<StructuredLyrics> {
    let file = File::open(path).ok()?;
    let mut reader = BufReader::new(file);
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    extract_embedded_lyrics_from_reader(&mut reader, ext)
}

/// Resolves lyrics preferring non-empty `sidecar_lrc_bytes` over embedded audio lyrics.
pub fn resolve_hybrid_lyrics(
    audio_path: Option<&Path>,
    sidecar_lrc_bytes: Option<&[u8]>,
) -> StructuredLyrics {
    if let Some(raw_lrc) = sidecar_lrc_bytes {
        let parsed = parse_lrc_bytes(raw_lrc);
        if !parsed.is_empty() {
            return parsed;
        }
    }
    if let Some(path) = audio_path
        && let Some(embedded) = extract_embedded_lyrics_from_path(path)
        && !embedded.is_empty()
    {
        return embedded;
    }
    StructuredLyrics::empty()
}

/// Shifts LRC `[mm:ss.xx]` timestamps by `offset_ms` and optionally strips vendor/credit noise lines.
/// Returns `(transformed_text, removed_line_count)`.
pub fn shift_and_clean_lrc_text(
    raw: &str,
    offset_ms: i64,
    strip_credit_noise: bool,
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
        out_lines.push(line);
    }

    (out_lines.join("\n"), removed)
}

fn shift_timestamp_ms(min: i64, sec: i64, frac_raw: Option<&str>, offset_ms: i64) -> String {
    let frac_ms = match frac_raw {
        Some(f) if f.len() == 1 => f.parse::<i64>().unwrap_or(0) * 100,
        Some(f) if f.len() == 2 => f.parse::<i64>().unwrap_or(0) * 10,
        Some(f) => f.get(..3).unwrap_or(f).parse::<i64>().unwrap_or(0),
        None => 0,
    };
    let total = (min * 60_000 + sec * 1_000 + frac_ms + offset_ms).max(0);
    let new_min = total / 60_000;
    let new_sec = (total % 60_000) / 1_000;
    let new_cs = (total % 1_000) / 10;
    format!("[{new_min:02}:{new_sec:02}.{new_cs:02}]")
}

fn parse_lrc_meta_tag(line: &str) -> Option<(String, String)> {
    if !line.starts_with('[') || !line.ends_with(']') {
        return None;
    }
    let inner = &line[1..line.len() - 1];
    if inner.contains('[') || inner.contains(']') {
        return None;
    }
    let (k, v) = inner.split_once(':')?;
    let key = k.trim().to_ascii_lowercase();
    if matches!(
        key.as_str(),
        "ar" | "ti" | "al" | "au" | "lr" | "length" | "by" | "offset" | "re" | "ve" | "la" | "id"
    ) {
        Some((key, v.trim().to_string()))
    } else {
        None
    }
}

fn extract_leading_timestamps(mut rest: &str) -> (Vec<u64>, &str) {
    let mut out = Vec::new();
    while let Some(after_open) = rest.strip_prefix('[') {
        let Some(close_idx) = after_open.find(']') else {
            break;
        };
        let token = &after_open[..close_idx];
        let Some(ms) = parse_single_lrc_timestamp(token) else {
            break;
        };
        out.push(ms);
        rest = &after_open[close_idx + 1..];
    }
    (out, rest)
}

fn parse_ascii_u64(raw: &str) -> Option<u64> {
    if raw.is_empty() || !raw.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    raw.parse().ok()
}

fn parse_single_lrc_timestamp(token: &str) -> Option<u64> {
    let (mins_str, sec_part) = token.trim().split_once(':')?;
    let mins = parse_ascii_u64(mins_str.trim())?;
    let (secs_str, frac_opt) = if let Some((s, f)) = sec_part.split_once('.') {
        (s, Some(f))
    } else if let Some((s, f)) = sec_part.split_once(':') {
        (s, Some(f))
    } else {
        (sec_part, None)
    };
    let secs = parse_ascii_u64(secs_str.trim())?;
    let frac_ms = match frac_opt {
        None => 0,
        Some(f) => {
            let clean = f.trim();
            if clean.is_empty() {
                0
            } else if !clean.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            } else {
                match clean.len() {
                    1 => clean.parse::<u64>().ok()? * 100,
                    2 => clean.parse::<u64>().ok()? * 10,
                    _ => clean.get(..3)?.parse::<u64>().ok()?,
                }
            }
        }
    };
    Some(mins.saturating_mul(60_000) + secs.saturating_mul(1_000) + frac_ms)
}

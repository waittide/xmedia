use std::fs::File;
use std::io::BufReader;

use lofty::config::{ParseOptions, ParsingMode};
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::id3::v2::{Frame, SynchronizedTextFrame};
use lofty::mpeg::MpegFile;
use lofty::probe::Probe;
use lofty::tag::ItemKey;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::infra::stream::decode_text_smart;

use crate::application::ports::LyricsResolverPort;
use crate::domain::catalog::{LyricLine, StructuredLyrics};

#[derive(Default)]
pub struct HybridLyricsResolver;

impl LyricsResolverPort for HybridLyricsResolver {
    fn resolve_lyrics(
        &self,
        audio_path: Option<&str>,
        sidecar_lrc_bytes: Option<&[u8]>,
    ) -> DomainResult<StructuredLyrics> {
        if let Some(raw_lrc) = sidecar_lrc_bytes {
            let parsed = parse_lrc_bytes(raw_lrc);
            if !parsed.is_empty() {
                return Ok(parsed);
            }
        }

        if let Some(path) = audio_path
            && let Some(embedded) = extract_embedded_lyrics(path)
            && !embedded.is_empty()
        {
            return Ok(embedded);
        }

        Ok(StructuredLyrics::empty())
    }
}

fn parse_lrc_bytes(raw_bytes: &[u8]) -> StructuredLyrics {
    let (decoded, _) = decode_text_smart(raw_bytes);
    parse_lyrics_str(&decoded)
}

pub fn parse_lyrics_str(input: &str) -> StructuredLyrics {
    let mut display_artist: Option<String> = None;
    let mut display_title: Option<String> = None;
    let mut offset_ms: i64 = 0;
    let mut synced_lines: Vec<LyricLine> = Vec::new();
    let mut plain_lines: Vec<String> = Vec::new();

    for raw_line in input.lines() {
        let line = raw_line.trim_end_matches('\r').trim();
        if line.is_empty() {
            plain_lines.push(String::new());
            continue;
        }

        if let Some((tag_key, tag_val)) = parse_lrc_meta_tag(line) {
            match tag_key.as_str() {
                "ar" if !tag_val.is_empty() => display_artist = Some(tag_val),
                "ti" if !tag_val.is_empty() => display_title = Some(tag_val),
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
            lang: "und".to_string(),
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
        lang: "und".to_string(),
        synced: false,
        display_artist,
        display_title,
        offset_ms: 0,
        lines: Vec::new(),
        plain_text,
    }
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
    let mins: u64 = parse_ascii_u64(mins_str.trim())?;

    let (secs_str, frac_opt) = if let Some((s, f)) = sec_part.split_once('.') {
        (s, Some(f))
    } else if let Some((s, f)) = sec_part.split_once(':') {
        (s, Some(f))
    } else {
        (sec_part, None)
    };

    let secs: u64 = parse_ascii_u64(secs_str.trim())?;
    let frac_ms: u64 = match frac_opt {
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

fn extract_embedded_lyrics(path: &str) -> Option<StructuredLyrics> {
    let opts = ParseOptions::new().parsing_mode(ParsingMode::Relaxed);

    if let Ok(file) = File::open(path) {
        let mut reader = BufReader::new(file);
        if let Ok(mp3) = MpegFile::read_from(&mut reader, opts)
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
                        lang,
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
    }

    let tagged = Probe::open(path)
        .ok()?
        .options(opts)
        .guess_file_type()
        .ok()?
        .read()
        .ok()?;

    for tag in tagged.tags() {
        for key in [ItemKey::Lyrics, ItemKey::UnsyncLyrics] {
            if let Some(text) = tag.get_string(key)
                && !text.trim().is_empty()
            {
                return Some(parse_lyrics_str(text));
            }
        }
    }

    None
}

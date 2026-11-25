use std::sync::LazyLock;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use encoding_rs::UTF_8;
use regex::Regex;
use serde::de::DeserializeOwned;
use xmedia_core::domain::error::{DomainError, DomainResult};

use crate::domain::scraper::{
    ScrapeCandidate, ScrapeQuery, ScrapeTargetKind, ScrapedMetadata, score_candidate_relevance,
};
use crate::infra::scrapers::cn_novel_common::{
    decode_html_entities, read_response_text, strip_inline_html,
};

pub async fn read_json_response<T: DeserializeOwned>(
    resp: reqwest::Response,
    label: &str,
) -> DomainResult<T> {
    let text = read_response_text(resp, UTF_8).await?;
    serde_json::from_str(text.trim())
        .map_err(|e| DomainError::Storage(format!("invalid {label} JSON: {e}")))
}

static RE_LRC_TIME_TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[(\d{1,3}):(\d{2})(?:[.:](\d{1,3}))?\]").unwrap());

static RE_LRC_META_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^\[(ti|ar|al|by|offset|kana|total|sign|qq|hash|id|language|uche|ve):[^\]]*\]\s*$",
    )
    .unwrap()
});

/// Builds a combined search keyword (`"{keyword} {artist}"`) without duplicating tokens already in `keyword`.
pub fn build_music_search_keyword(query: &ScrapeQuery) -> String {
    let kw = query.keyword.trim();
    let mut parts = Vec::new();
    if !kw.is_empty() {
        parts.push(kw.to_string());
    }
    if let Some(artist) = query
        .artist_or_author
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        && !kw.to_lowercase().contains(&artist.to_lowercase())
    {
        parts.push(artist.to_string());
    }
    parts.join(" ")
}

pub fn decode_base64_utf8(raw: &str) -> Option<String> {
    let compact: String = raw.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    if compact.is_empty() {
        return None;
    }
    let bytes = BASE64_STANDARD.decode(compact.as_bytes()).ok()?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    (!text.trim().is_empty()).then_some(text)
}

/// Converts a millisecond epoch timestamp (commonly anchored at `00:00:00 UTC+8` on Chinese music APIs)
/// into `(year, YYYY-MM-DD)`.
pub fn parse_epoch_ms_year_and_date(ms: i64) -> (Option<i32>, Option<String>) {
    if ms <= 0 {
        return (None, None);
    }
    let secs = if ms > 100_000_000_000 {
        ms.div_euclid(1000)
    } else {
        ms
    };
    let Ok(ts) = jiff::Timestamp::from_second(secs) else {
        return (None, None);
    };
    let cst = jiff::tz::Offset::from_seconds(8 * 3600)
        .map(jiff::tz::TimeZone::fixed)
        .unwrap_or(jiff::tz::TimeZone::UTC);
    let zdt = ts.to_zoned(cst);
    let y = i32::from(zdt.year());
    if !(1900..=2100).contains(&y) {
        return (None, None);
    }
    let m = zdt.month();
    let d = zdt.day();
    (Some(y), Some(format!("{y:04}-{m:02}-{d:02}")))
}

/// Decodes HTML entities twice (handling QQ Music's `&#38;apos;` -> `&apos;` -> `'`).
pub fn unescape_music_text(raw: &str) -> String {
    let first = decode_html_entities(raw).replace("&apos;", "'");
    decode_html_entities(&first).replace("&apos;", "'")
}

/// Splits compound Chinese/Western artist strings (`"周杰伦、费玉清"`, `"周杰伦&费玉清"`, `"A / B"`) into deduplicated names.
pub fn split_music_artists(raw: &str) -> Vec<String> {
    let cleaned = strip_inline_html(&unescape_music_text(raw));
    if cleaned.is_empty() {
        return Vec::new();
    }
    let normalized = cleaned.replace(" / ", "、").replace('；', "、");
    let mut out = Vec::new();
    for seg in normalized.split(['、', '&']) {
        let name = seg.trim();
        if !name.is_empty() && !out.iter().any(|e: &String| e.eq_ignore_ascii_case(name)) {
            out.push(name.to_string());
        }
    }
    if out.is_empty() {
        out.push(cleaned);
    }
    out
}

pub fn format_duration_mmss(secs: u32) -> Option<String> {
    if secs == 0 {
        return None;
    }
    Some(format!("{}:{:02}", secs / 60, secs % 60))
}

fn parse_time_tag_ms(caps: &regex::Captures) -> u64 {
    let mins: u64 = caps[1].parse().unwrap_or(0);
    let secs: u64 = caps[2].parse().unwrap_or(0);
    let frac_ms: u64 = match caps.get(3).map(|m| m.as_str()) {
        Some(f) if f.len() == 1 => f.parse::<u64>().unwrap_or(0) * 100,
        Some(f) if f.len() == 2 => f.parse::<u64>().unwrap_or(0) * 10,
        Some(f) => f.get(..3).unwrap_or(f).parse::<u64>().unwrap_or(0),
        None => 0,
    };
    mins * 60_000 + secs * 1_000 + frac_ms
}

fn format_lrc_tag_centis(ms: u64) -> String {
    let total_cs = (ms + 5) / 10;
    let mins = total_cs / 6000;
    let rem = total_cs % 6000;
    let secs = rem / 100;
    let cs = rem % 100;
    format!("[{mins:02}:{secs:02}.{cs:02}]")
}

fn is_instrumental_or_placeholder(text: &str) -> bool {
    let t = text.trim();
    t.is_empty()
        || t.contains("此歌曲为没有填词的纯音乐")
        || t.contains("纯音乐，请欣赏")
        || t.contains("纯音乐,请欣赏")
        || t == "暂无歌词"
        || t == "纯音乐"
}

fn parse_timestamped_lines(text: &str) -> Vec<(u64, String, String)> {
    let mut entries = Vec::new();
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || RE_LRC_META_LINE.is_match(line) {
            continue;
        }
        let tags: Vec<(u64, String)> = RE_LRC_TIME_TAG
            .captures_iter(line)
            .map(|c| (parse_time_tag_ms(&c), c[0].to_string()))
            .collect();
        if tags.is_empty() {
            continue;
        }
        let body = RE_LRC_TIME_TAG.replace_all(line, "").trim().to_string();
        for (ms, raw_tag) in tags {
            entries.push((ms, raw_tag, body.clone()));
        }
    }
    entries.sort_by_key(|(ms, _, _)| *ms);
    entries
}

/// Cleans LRC or plain lyrics, optionally merging timestamped translation (`raw_trans`) into bilingual LRC,
/// and returns `(synced_lyrics, plain_lyrics)`.
pub fn normalize_lrc_and_extract_plain(
    raw_lrc: &str,
    raw_trans: Option<&str>,
) -> (Option<String>, Option<String>) {
    let decoded = unescape_music_text(raw_lrc)
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    if is_instrumental_or_placeholder(&decoded) {
        return (None, None);
    }

    let orig_timed = parse_timestamped_lines(&decoded);
    if orig_timed.is_empty() {
        // Plain lyrics without timestamps
        let plain_lines: Vec<String> = decoded
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !RE_LRC_META_LINE.is_match(l))
            .map(str::to_string)
            .collect();
        if plain_lines.is_empty() {
            return (None, None);
        }
        let plain = plain_lines.join("\n");
        if is_instrumental_or_placeholder(&plain) {
            return (None, None);
        }
        return (None, Some(plain));
    }

    // Check body content isn't solely an instrumental notice
    let non_empty_bodies: Vec<&str> = orig_timed
        .iter()
        .map(|(_, _, b)| b.as_str())
        .filter(|b| !b.is_empty())
        .collect();
    if non_empty_bodies.is_empty()
        || (non_empty_bodies.len() <= 2
            && non_empty_bodies
                .iter()
                .all(|b| is_instrumental_or_placeholder(b)))
    {
        return (None, None);
    }

    let trans_timed: Vec<(u64, String)> = raw_trans
        .map(unescape_music_text)
        .map(|t| t.replace("\r\n", "\n").replace('\r', "\n"))
        .map(|t| {
            parse_timestamped_lines(&t)
                .into_iter()
                .filter(|(_, _, body)| {
                    let b = body.trim();
                    !b.is_empty() && b != "//" && b != "/"
                })
                .map(|(ms, _, body)| (ms, body))
                .collect()
        })
        .unwrap_or_default();

    let mut synced_lines = Vec::with_capacity(orig_timed.len() + trans_timed.len());
    let mut plain_lines = Vec::with_capacity(orig_timed.len());
    let mut used_trans = vec![false; trans_timed.len()];

    for (ms, raw_tag, body) in &orig_timed {
        synced_lines.push(format!("{raw_tag}{body}"));
        if !body.is_empty() {
            plain_lines.push(body.clone());
        }
        if !body.is_empty()
            && !trans_timed.is_empty()
            && let Some((t_idx, (_, t_body))) = trans_timed
                .iter()
                .enumerate()
                .filter(|(idx, _)| !used_trans[*idx])
                .min_by_key(|(_, (tms, _))| ms.abs_diff(*tms))
                .filter(|(_, (tms, _))| ms.abs_diff(*tms) <= 120)
        {
            used_trans[t_idx] = true;
            if t_body.trim() != body.trim() {
                synced_lines.push(format!("{raw_tag}{t_body}"));
                plain_lines.push(t_body.clone());
            }
        }
    }

    let synced = (!synced_lines.is_empty()).then(|| synced_lines.join("\n"));
    let plain = (!plain_lines.is_empty()).then(|| plain_lines.join("\n"));
    (synced, plain)
}

/// Converts Kuwo's `[{"time": "2.25", "lineLyric": "..."}]` entries into `(synced_lyrics, plain_lyrics)`.
pub fn format_kuwo_lrclist(items: &[(f64, String)]) -> (Option<String>, Option<String>) {
    if items.is_empty() {
        return (None, None);
    }
    let mut lrc_buf = Vec::with_capacity(items.len());
    for (secs, raw_line) in items {
        if !secs.is_finite() || *secs < 0.0 {
            continue;
        }
        let ms = (*secs * 1000.0).round() as u64;
        let tag = format_lrc_tag_centis(ms);
        let text = unescape_music_text(raw_line).trim().to_string();
        lrc_buf.push(format!("{tag}{text}"));
    }
    normalize_lrc_and_extract_plain(&lrc_buf.join("\n"), None)
}

fn norm_match(raw: &str) -> String {
    raw.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

fn is_cjk_char(c: char) -> bool {
    matches!(c as u32, 0x4E00..=0x9FFF | 0x3400..=0x4DBF)
}

/// Detects impersonator artist names that insert spaces between Chinese characters (e.g. "邓 紫 棋").
fn has_spaced_cjk_impersonation(name: &str) -> bool {
    let chars: Vec<char> = name.trim().chars().collect();
    chars
        .windows(3)
        .any(|w| is_cjk_char(w[0]) && w[1].is_whitespace() && is_cjk_char(w[2]))
}

/// Scores a music track or album candidate, rewarding matching artist/album/duration and penalizing
/// karaoke accompaniments (`伴奏`/`KTV`), cover versions (`原唱`/`翻唱`), ringtones, or unrequested Live versions.
pub fn score_music_candidate(
    query: &ScrapeQuery,
    title: &str,
    artists: &[String],
    album: Option<&str>,
    year: Option<i32>,
    duration_secs: Option<u32>,
    popularity_votes: u32,
) -> i32 {
    let base_before_parens = title
        .find(['(', '（', '【'])
        .map(|idx| title[..idx].trim())
        .filter(|s| !s.is_empty() && *s != title.trim());

    let mut score = score_candidate_relevance(
        &query.keyword,
        query.year,
        title,
        base_before_parens,
        year,
        popularity_votes,
    );

    let kw_lower = query.keyword.to_lowercase();
    let title_lower = title.to_lowercase();

    // Heavy penalty for accompaniment / cover / ringtone / DJ remix when user didn't ask for it
    let heavy_noise_markers = [
        "伴奏",
        "ktv",
        "翻唱",
        "原唱",
        "铃声",
        "彩铃",
        "消音",
        "纯享版",
        "cover",
        "karaoke",
        "instrumental",
    ];
    for marker in heavy_noise_markers {
        if title_lower.contains(marker) && !kw_lower.contains(marker) {
            score -= 480;
            break;
        }
    }

    let mild_variant_markers = ["live", "现场", "演唱会", "dj", "remix", "串烧"];
    for marker in mild_variant_markers {
        if title_lower.contains(marker) && !kw_lower.contains(marker) {
            score -= 180;
            break;
        }
    }

    if artists.iter().any(|a| has_spaced_cjk_impersonation(a)) {
        score -= 550;
    }

    if let Some(want_artist) = query
        .artist_or_author
        .as_deref()
        .map(norm_match)
        .filter(|s| !s.is_empty())
    {
        let clean_norms: Vec<(usize, String)> = artists
            .iter()
            .enumerate()
            .filter(|(_, a)| !has_spaced_cjk_impersonation(a))
            .map(|(i, a)| (i, norm_match(a)))
            .filter(|(_, s)| !s.is_empty())
            .collect();

        if let Some((idx, _)) = clean_norms.iter().find(|(_, c)| c == &want_artist) {
            score += if *idx == 0 { 560 } else { 420 };
        } else if let Some((idx, _)) = clean_norms
            .iter()
            .find(|(_, c)| c.contains(&want_artist) || want_artist.contains(c))
        {
            score += if *idx == 0 { 460 } else { 280 };
        } else if !artists.is_empty() {
            score -= 320;
        }
    }

    if let Some(want_album) = query
        .album_or_series
        .as_deref()
        .map(norm_match)
        .filter(|s| !s.is_empty())
        && let Some(got_album) = album.map(norm_match).filter(|s| !s.is_empty())
    {
        if got_album == want_album {
            score += 260;
        } else if got_album.contains(&want_album) || want_album.contains(&got_album) {
            score += 120;
        }
    }

    if let (Some(want_dur), Some(got_dur)) = (query.duration_secs, duration_secs)
        && want_dur > 0
        && got_dur > 0
    {
        let diff = want_dur.abs_diff(got_dur);
        if diff <= 2 {
            score += 180;
        } else if diff <= 6 {
            score += 80;
        } else if diff > 25 {
            score -= 140;
        }
    }

    score
}

#[derive(Debug, Clone, Default)]
pub struct MusicCandidateDraft {
    pub external_id: String,
    pub title: String,
    pub artists: Vec<String>,
    pub album: Option<String>,
    pub year: Option<i32>,
    pub duration_secs: Option<u32>,
    pub song_count: Option<u32>,
    pub summary: Option<String>,
    pub cover_url: Option<String>,
    pub popularity_votes: u32,
}

pub fn build_music_subtitle(
    target: ScrapeTargetKind,
    artists: &[String],
    album: Option<&str>,
    duration_secs: Option<u32>,
    song_count: Option<u32>,
) -> Option<String> {
    let mut sub = Vec::new();
    if !artists.is_empty() {
        sub.push(artists.join(" / "));
    }
    if target == ScrapeTargetKind::MusicAlbum {
        if let Some(cnt) = song_count.filter(|&c| c > 0) {
            sub.push(format!("{cnt} 首"));
        }
    } else {
        if let Some(alb) = album.map(str::trim).filter(|s| !s.is_empty()) {
            sub.push(alb.to_string());
        }
        if let Some(dur) = duration_secs.and_then(format_duration_mmss) {
            sub.push(dur);
        }
    }
    (!sub.is_empty()).then(|| sub.join(" · "))
}

pub fn rank_music_candidates(
    provider_id: &str,
    provider_name: &str,
    target: ScrapeTargetKind,
    query: &ScrapeQuery,
    drafts: impl IntoIterator<Item = MusicCandidateDraft>,
    limit: usize,
) -> Vec<ScrapeCandidate> {
    let is_album = target == ScrapeTargetKind::MusicAlbum;
    let mut scored: Vec<(i32, usize, ScrapeCandidate)> = drafts
        .into_iter()
        .enumerate()
        .filter_map(|(idx, d)| {
            if d.external_id.trim().is_empty() || d.title.trim().is_empty() {
                return None;
            }
            let album_for_score = if is_album {
                d.album.as_deref().or(Some(d.title.as_str()))
            } else {
                d.album.as_deref()
            };
            let score = score_music_candidate(
                query,
                &d.title,
                &d.artists,
                album_for_score,
                d.year,
                d.duration_secs,
                d.popularity_votes,
            );
            let subtitle = build_music_subtitle(
                target,
                &d.artists,
                d.album.as_deref(),
                d.duration_secs,
                d.song_count,
            );
            Some((
                score,
                idx,
                ScrapeCandidate {
                    provider_id: provider_id.to_string(),
                    provider_name: provider_name.to_string(),
                    external_id: d.external_id,
                    target,
                    title: d.title,
                    subtitle,
                    year: d.year,
                    summary: d.summary.filter(|s| !s.trim().is_empty()),
                    cover_url: d.cover_url,
                    synced_lyrics: None,
                    plain_lyrics: None,
                },
            ))
        })
        .collect();

    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    scored.into_iter().take(limit).map(|(_, _, c)| c).collect()
}

pub fn music_candidate_from_detail(
    provider_id: &str,
    provider_name: &str,
    target: ScrapeTargetKind,
    detail: ScrapedMetadata,
) -> Option<ScrapeCandidate> {
    let title = detail.title?;
    let is_album = target == ScrapeTargetKind::MusicAlbum;
    let subtitle = build_music_subtitle(
        target,
        &detail.creators,
        detail.series_or_album.as_deref(),
        None,
        None,
    );
    Some(ScrapeCandidate {
        provider_id: provider_id.to_string(),
        provider_name: provider_name.to_string(),
        external_id: detail.external_id,
        target,
        title,
        subtitle,
        year: detail.year,
        summary: detail.summary,
        cover_url: detail.cover_url,
        synced_lyrics: if is_album { None } else { detail.synced_lyrics },
        plain_lyrics: if is_album { None } else { detail.plain_lyrics },
    })
}

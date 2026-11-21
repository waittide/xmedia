use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

pub(super) const VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "mkv", "webm", "mov", "avi", "ts", "m2ts", "mts", "m4v", "flv",
];

fn parse_season_dir_number(dir: &Path) -> Option<u32> {
    let name = dir.file_name().and_then(|s| s.to_str())?;
    let trimmed = name.trim();
    let lower = trimmed.to_ascii_lowercase();
    if lower == "specials" {
        return Some(0);
    }
    if let Some(rest) = lower.strip_prefix("season") {
        let digits = rest.trim();
        if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
            return digits.parse::<u32>().ok();
        }
    }
    if let Some(rest) = lower.strip_prefix('s')
        && !rest.is_empty()
        && rest.len() <= 3
        && rest.chars().all(|c| c.is_ascii_digit())
    {
        return rest.parse::<u32>().ok();
    }
    if let Some(rest) = trimmed.strip_prefix('第')
        && let Some(num_str) = rest.strip_suffix('季')
    {
        let digits = num_str.trim();
        if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
            return digits.parse::<u32>().ok();
        }
    }
    None
}

pub(crate) fn is_season_dir(dir: &Path) -> bool {
    parse_season_dir_number(dir).is_some()
}

pub(crate) fn dir_has_single_video(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    let mut count = 0usize;
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_file()
            && p.extension()
                .and_then(|s| s.to_str())
                .is_some_and(|e| VIDEO_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
        {
            count += 1;
            if count > 1 {
                return false;
            }
        }
    }
    count == 1
}

pub(super) fn list_sidecar_subtitles(video_path: &Path) -> Vec<String> {
    let Some(parent) = video_path.parent() else {
        return Vec::new();
    };
    let Some(stem) = video_path.file_stem().and_then(|s| s.to_str()) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(parent) else {
        return Vec::new();
    };
    let mut subs = Vec::new();
    for entry in entries.flatten() {
        let p = entry.path();
        if !p.is_file() {
            continue;
        }
        let Some(fname) = p.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let lower = fname.to_ascii_lowercase();
        let is_sub = lower.ends_with(".srt")
            || lower.ends_with(".ass")
            || lower.ends_with(".ssa")
            || lower.ends_with(".vtt");
        if is_sub && fname.starts_with(stem) {
            subs.push(fname.to_string());
        }
    }
    subs.sort();
    subs
}

static RE_RELEASE_NOISE_TAG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(?:[\(\[]\s*(?:19\d{2}|20\d{2})\s*[\)\]]|\b(?:2160p|1080p|720p|480p|4k|uhd|bluray|bdrip|web-dl|webrip|hdtv|remux|x264|x265|h\.?264|h\.?265|hevc|av1|10bit|8bit|aac|ac3|dts|flac|hdr10|dv)\b.*$)",
    )
    .expect("valid release noise regex")
});
static RE_LEADING_GROUP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:\[[^\]]+\]\s*)+").expect("valid group regex"));
static RE_SE_EPISODE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)S(\d{1,2})E(\d{1,4})").expect("valid regex"));
static RE_ZH_EPISODE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"第\s*0*(\d{1,4})\s*[话話集回]").expect("valid zh ep regex"));
static RE_EP_WORD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:^|[\s._\-\[\(])(?:EP?|Episode)\s*0*(\d{1,4})(?:$|[\s._\-\]\)])")
        .expect("valid ep word regex")
});
static RE_DASH_EPISODE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\s+-\s+0*(\d{1,4})(?:v\d)?(?:$|[\s.\[\(])").expect("valid dash ep regex")
});
static RE_BRACKET_EPISODE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[0*(\d{1,3})(?:v\d)?\]").expect("valid bracket ep regex"));
static RE_YEAR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:^|[\s\.\(\[_-])(19\d{2}|20\d{2})(?:$|[\s\.\)\]_-])").expect("valid year regex")
});

fn strip_release_noise(raw: &str) -> String {
    let without_group = RE_LEADING_GROUP.replace(raw.trim(), "");
    let without_noise = RE_RELEASE_NOISE_TAG.replace(&without_group, "");
    without_noise
        .replace(['.', '_'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_matches([' ', '-', '–', '_'])
        .to_string()
}

fn is_unlikely_episode_num(n: u32) -> bool {
    matches!(n, 264 | 265 | 480 | 720 | 1080 | 2160) || (1900..=2099).contains(&n)
}

fn infer_show_from_parent(video_path: &Path) -> Option<String> {
    video_path.parent().and_then(|p| {
        let target_dir = if is_season_dir(p) { p.parent()? } else { p };
        let dir_name = target_dir.file_name()?.to_string_lossy();
        let cleaned = strip_release_noise(&dir_name);
        (!cleaned.is_empty()).then_some(cleaned)
    })
}

pub(super) fn guess_episode_and_year(
    video_path: &Path,
) -> (
    bool,
    String,
    Option<String>,
    Option<u32>,
    Option<u32>,
    Option<i32>,
) {
    let stem = video_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Untitled");

    let mut year = RE_YEAR
        .captures(stem)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse::<i32>().ok());
    if year.is_none()
        && let Some(parent_str) = video_path.parent().and_then(|p| p.to_str())
    {
        year = RE_YEAR
            .captures(parent_str)
            .and_then(|c| c.get(1))
            .and_then(|m| m.as_str().parse::<i32>().ok());
    }

    let season_from_dir = video_path.parent().and_then(parse_season_dir_number);

    if let Some(caps) = RE_SE_EPISODE.captures(stem) {
        let full_match = caps.get(0).unwrap();
        let season = caps.get(1).and_then(|m| m.as_str().parse::<u32>().ok());
        let ep = caps.get(2).and_then(|m| m.as_str().parse::<u32>().ok());
        let prefix = strip_release_noise(&stem[..full_match.start()]);
        let suffix = strip_release_noise(&stem[full_match.end()..]);

        let show_guess = if !prefix.is_empty() {
            Some(prefix)
        } else {
            infer_show_from_parent(video_path)
        };
        let ep_title = if !suffix.is_empty() {
            suffix
        } else {
            format!("S{:02}E{:02}", season.unwrap_or(1), ep.unwrap_or(1))
        };
        return (true, ep_title, show_guess, season, ep, year);
    }

    // Check anime/Chinese episode naming: `第02话`, `EP02`, ` - 02 `, `[02]`, or bare `02` inside a Season folder
    let extra_ep_match: Option<(usize, usize, u32)> = RE_ZH_EPISODE
        .captures(stem)
        .and_then(|c| {
            let full = c.get(0)?;
            let ep = c.get(1)?.as_str().parse::<u32>().ok()?;
            Some((full.start(), full.end(), ep))
        })
        .or_else(|| {
            RE_EP_WORD.captures(stem).and_then(|c| {
                let full = c.get(0)?;
                let ep = c.get(1)?.as_str().parse::<u32>().ok()?;
                Some((full.start(), full.end(), ep))
            })
        })
        .or_else(|| {
            RE_DASH_EPISODE.captures(stem).and_then(|c| {
                let full = c.get(0)?;
                let ep = c.get(1)?.as_str().parse::<u32>().ok()?;
                (!is_unlikely_episode_num(ep)).then_some((full.start(), full.end(), ep))
            })
        })
        .or_else(|| {
            RE_BRACKET_EPISODE.captures(stem).and_then(|c| {
                let full = c.get(0)?;
                let ep = c.get(1)?.as_str().parse::<u32>().ok()?;
                (!is_unlikely_episode_num(ep)).then_some((full.start(), full.end(), ep))
            })
        })
        .or_else(|| {
            if season_from_dir.is_some() {
                let trimmed = stem.trim();
                let digits_len = trimmed.bytes().take_while(|b| b.is_ascii_digit()).count();
                if digits_len > 0 && digits_len <= 3 {
                    let ep = trimmed[..digits_len].parse::<u32>().ok()?;
                    if !is_unlikely_episode_num(ep) {
                        return Some((0, digits_len, ep));
                    }
                }
            }
            None
        });

    if let Some((m_start, m_end, ep)) = extra_ep_match {
        let prefix = strip_release_noise(&stem[..m_start]);
        let suffix = strip_release_noise(&stem[m_end..]);
        let season = season_from_dir.or(Some(1));
        let show_guess = if !prefix.is_empty() {
            Some(prefix)
        } else {
            infer_show_from_parent(video_path)
        };
        let ep_title = if !suffix.is_empty() {
            suffix
        } else {
            format!("S{:02}E{:02}", season.unwrap_or(1), ep)
        };
        return (true, ep_title, show_guess, season, Some(ep), year);
    }

    // Movie / standalone video: strip `(YYYY)` or `.YYYY.1080p...` from clean title
    let mut clean_movie_title = strip_release_noise(stem);
    if let Some(y) = year {
        let trailing_year = format!(" {y}");
        if let Some(stripped) = clean_movie_title.strip_suffix(&trailing_year)
            && !stripped.trim().is_empty()
        {
            clean_movie_title = stripped.trim_matches([' ', '-', '_']).to_string();
        }
    }
    if clean_movie_title.is_empty() {
        clean_movie_title = stem.to_string();
    }

    (false, clean_movie_title, None, None, None, year)
}

use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use super::VideoFormatExt;

/// Parsed episode/movie metadata inferred from a video filename and parent folder hierarchy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedVideoFilename {
    pub is_episode: bool,
    pub title: String,
    pub show_title: Option<String>,
    pub season_number: Option<u32>,
    pub episode_number: Option<u32>,
    pub year: Option<i32>,
    pub resolution: Option<String>,
}

static RE_RESOLUTION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(?:^|[\s._\-\[\(])(2160p|1440p|1080p|1080i|720p|576p|480p|4k|uhd|fhd|3840x2160|2560x1440|1920x1080|1280x720)(?:$|[\s._\-\]\)])",
    )
    .expect("valid resolution regex")
});

static RE_RELEASE_NOISE_TAG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(?:[\(\[]\s*(?:19\d{2}|20\d{2})\s*[\)\]]|(?:^|[\s._\-\[\(])(?:2160p|1440p|1080p|1080i|720p|576p|480p|4k|uhd|fhd|bluray|bdrip|web-dl|webrip|hdtv|remux|x264|x265|h\.?264|h\.?265|hevc|av1|10bit|8bit|aac|ac3|dts|flac|hdr10|dv)(?:$|[\s._\-\]\)]).*$)",
    )
    .expect("valid release noise regex")
});

static RE_LEADING_GROUP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:\[[^\]]+\]\s*)+").expect("valid group regex"));
static RE_SE_EPISODE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)S(\d{1,2})E(\d{1,4})").expect("valid sxxexx regex"));
static RE_ZH_EPISODE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"第\s*0*(\d{1,4})\s*[话話集回]").expect("valid zh ep regex"));
static RE_EP_WORD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:^|[\s._\-\[\(])(?:EP?|Episode)\s*0*(\d{1,4})(?:$|[\s._\-\]\)])")
        .expect("valid ep word regex")
});
static RE_DASH_EPISODE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\s*[-–]\s*0*(\d{1,4})(?:v\d)?(?:$|[\s.\[\(\-–])").expect("valid dash ep regex")
});
static RE_BRACKET_EPISODE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[0*(\d{1,3})(?:v\d)?\]").expect("valid bracket ep regex"));
static RE_YEAR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:^|[\s\.\(\[_-])(19\d{2}|20\d{2})(?:$|[\s\.\)\]_-])").expect("valid year regex")
});
static RE_LEADING_NUM: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:EP?|e)?0*(?P<ep>\d{1,4})(?:[.\s_\-–]+(?P<rest>.+))?$")
        .expect("valid leading num regex")
});

/// Extracts canonical video resolution (`"2160p"`, `"1440p"`, `"1080p"`, `"720p"`, `"576p"`, `"480p"`) from a stem/path.
pub fn extract_video_resolution(raw: &str) -> Option<String> {
    let token = RE_RESOLUTION.captures(raw)?.get(1)?.as_str();
    let canonical = match token.to_ascii_lowercase().as_str() {
        "2160p" | "4k" | "uhd" | "3840x2160" => "2160p",
        "1440p" | "2560x1440" => "1440p",
        "1080p" | "1080i" | "fhd" | "1920x1080" => "1080p",
        "720p" | "1280x720" => "720p",
        "576p" => "576p",
        "480p" => "480p",
        _ => return None,
    };
    Some(canonical.to_string())
}

/// Parses a season folder name (`Season 1`, `S02`, `Specials`, `第1季`) into its season number.
pub fn parse_season_dir_name(name: &str) -> Option<u32> {
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

pub fn parse_season_dir_number(dir: &Path) -> Option<u32> {
    dir.file_name()
        .and_then(|s| s.to_str())
        .and_then(parse_season_dir_name)
}

pub fn is_season_dir(dir: &Path) -> bool {
    parse_season_dir_number(dir).is_some()
}

pub fn dir_has_single_video(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    let mut count = 0usize;
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_file()
            && p.extension()
                .and_then(|s| s.to_str())
                .and_then(VideoFormatExt::from_extension)
                .is_some()
        {
            count += 1;
            if count > 1 {
                return false;
            }
        }
    }
    count == 1
}

pub fn list_sidecar_subtitles(video_path: &Path) -> Vec<String> {
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

/// Parses a relative path or filename (e.g. `"Show/Season 1/01 - Pilot.mkv"` or `"Matrix.1999.2160p.mkv"`)
/// into `ParsedVideoFilename`.
pub fn parse_video_path(relative_or_filename: &str) -> ParsedVideoFilename {
    let normalized = relative_or_filename.replace('\\', "/");
    let parts: Vec<&str> = normalized
        .split('/')
        .filter(|s| !s.trim().is_empty())
        .collect();
    let filename = parts.last().copied().unwrap_or(relative_or_filename);
    let stem = filename
        .rsplit_once('.')
        .filter(|(s, ext)| !s.is_empty() && VideoFormatExt::from_extension(ext).is_some())
        .map(|(s, _)| s)
        .or_else(|| filename.rsplit_once('.').map(|(s, _)| s))
        .unwrap_or(filename)
        .trim();

    let parent_dir = (parts.len() >= 2).then(|| parts[parts.len() - 2].trim());
    let grandparent_dir = (parts.len() >= 3).then(|| parts[parts.len() - 3].trim());
    parse_video_components(stem, parent_dir, grandparent_dir)
}

/// Parses a filesystem `Path` into `ParsedVideoFilename`.
pub fn guess_video_from_path(video_path: &Path) -> ParsedVideoFilename {
    let stem = video_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Untitled");
    let parent_dir = video_path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str());
    let grandparent_dir = video_path
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str());
    parse_video_components(stem, parent_dir, grandparent_dir)
}

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

fn strip_trailing_year(title: &str, year: Option<i32>) -> String {
    if let Some(y) = year {
        let suffix = format!(" {y}");
        if let Some(stripped) = title.strip_suffix(&suffix)
            && !stripped.trim().is_empty()
        {
            return stripped.trim_matches([' ', '-', '–', '_']).to_string();
        }
    }
    title.to_string()
}

fn is_unlikely_episode_num(n: u32) -> bool {
    matches!(n, 264 | 265 | 480 | 576 | 720 | 1080 | 1440 | 2160) || (1900..=2099).contains(&n)
}

fn parse_video_components(
    stem: &str,
    parent_dir: Option<&str>,
    grandparent_dir: Option<&str>,
) -> ParsedVideoFilename {
    let resolution =
        extract_video_resolution(stem).or_else(|| parent_dir.and_then(extract_video_resolution));
    let year = RE_YEAR
        .captures(stem)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse::<i32>().ok())
        .or_else(|| {
            parent_dir
                .and_then(|p| RE_YEAR.captures(p))
                .and_then(|c| c.get(1))
                .and_then(|m| m.as_str().parse::<i32>().ok())
        })
        .or_else(|| {
            grandparent_dir
                .and_then(|p| RE_YEAR.captures(p))
                .and_then(|c| c.get(1))
                .and_then(|m| m.as_str().parse::<i32>().ok())
        });

    let season_from_dir = parent_dir.and_then(parse_season_dir_name);
    let inferred_parent_show = || -> Option<String> {
        let dir_raw = if season_from_dir.is_some() {
            grandparent_dir?
        } else {
            parent_dir?
        };
        let cleaned = strip_trailing_year(&strip_release_noise(dir_raw), year);
        (!cleaned.is_empty()).then_some(cleaned)
    };

    if let Some(caps) = RE_SE_EPISODE.captures(stem) {
        let full = caps.get(0).unwrap();
        let season = caps.get(1).and_then(|m| m.as_str().parse::<u32>().ok());
        let ep = caps.get(2).and_then(|m| m.as_str().parse::<u32>().ok());
        let prefix = strip_trailing_year(&strip_release_noise(&stem[..full.start()]), year);
        let suffix = strip_release_noise(&stem[full.end()..]);
        let show_title = if !prefix.is_empty() {
            Some(prefix)
        } else {
            inferred_parent_show()
        };
        let title = if !suffix.is_empty() {
            suffix
        } else {
            format!("S{:02}E{:02}", season.unwrap_or(1), ep.unwrap_or(1))
        };
        return ParsedVideoFilename {
            is_episode: true,
            title,
            show_title,
            season_number: season,
            episode_number: ep,
            year,
            resolution,
        };
    }

    // Check folder hierarchy: Show / Season 1 / 01 - Pilot.mkv or Show / 01 - Pilot.mkv
    if let Some(p_dir) = parent_dir {
        if let Some(season) = season_from_dir {
            if let Some((ep, ep_title)) = extract_leading_episode(stem)
                && let Some(show) = inferred_parent_show()
            {
                return ParsedVideoFilename {
                    is_episode: true,
                    title: ep_title.unwrap_or_else(|| format!("Episode {ep}")),
                    show_title: Some(show),
                    season_number: Some(season),
                    episode_number: Some(ep),
                    year,
                    resolution,
                };
            }
        } else if year.is_none() {
            let trimmed_stem = stem.trim();
            let has_explicit_ep_sep = trimmed_stem.chars().all(|c| c.is_ascii_digit())
                || trimmed_stem
                    .trim_start_matches(|c: char| c.is_ascii_digit())
                    .trim_start()
                    .starts_with(['-', '–', '_']);
            if has_explicit_ep_sep && let Some((ep, ep_title)) = extract_leading_episode(stem) {
                let show = strip_release_noise(p_dir);
                if !show.is_empty() {
                    return ParsedVideoFilename {
                        is_episode: true,
                        title: ep_title.unwrap_or_else(|| format!("Episode {ep}")),
                        show_title: Some(show),
                        season_number: Some(1),
                        episode_number: Some(ep),
                        year,
                        resolution,
                    };
                }
            }
        }
    }

    // Anime / Chinese / EP episode patterns: `第05话`, `EP05`, ` - 05 `, `[05]`
    let extra_ep = RE_ZH_EPISODE
        .captures(stem)
        .and_then(|c| {
            let m = c.get(0)?;
            Some((m.start(), m.end(), c.get(1)?.as_str().parse::<u32>().ok()?))
        })
        .or_else(|| {
            RE_EP_WORD.captures(stem).and_then(|c| {
                let m = c.get(0)?;
                Some((m.start(), m.end(), c.get(1)?.as_str().parse::<u32>().ok()?))
            })
        })
        .or_else(|| {
            RE_DASH_EPISODE.captures(stem).and_then(|c| {
                let m = c.get(0)?;
                let ep = c.get(1)?.as_str().parse::<u32>().ok()?;
                (!is_unlikely_episode_num(ep)).then_some((m.start(), m.end(), ep))
            })
        })
        .or_else(|| {
            RE_BRACKET_EPISODE.captures(stem).and_then(|c| {
                let m = c.get(0)?;
                let ep = c.get(1)?.as_str().parse::<u32>().ok()?;
                (!is_unlikely_episode_num(ep)).then_some((m.start(), m.end(), ep))
            })
        });

    if let Some((m_start, m_end, ep)) = extra_ep {
        let prefix = strip_trailing_year(&strip_release_noise(&stem[..m_start]), year);
        let suffix = strip_release_noise(&stem[m_end..]);
        let season = season_from_dir.or(Some(1));
        let show_title = if !prefix.is_empty() {
            Some(prefix)
        } else {
            inferred_parent_show()
        };
        let title = if !suffix.is_empty() {
            suffix
        } else {
            format!("Episode {ep}")
        };
        return ParsedVideoFilename {
            is_episode: true,
            title,
            show_title,
            season_number: season,
            episode_number: Some(ep),
            year,
            resolution,
        };
    }

    let clean_movie = strip_trailing_year(&strip_release_noise(stem), year);
    ParsedVideoFilename {
        is_episode: false,
        title: if clean_movie.is_empty() {
            if stem.is_empty() {
                "Untitled Video".to_string()
            } else {
                stem.to_string()
            }
        } else {
            clean_movie
        },
        show_title: None,
        season_number: None,
        episode_number: None,
        year,
        resolution,
    }
}

fn extract_leading_episode(stem: &str) -> Option<(u32, Option<String>)> {
    let caps = RE_LEADING_NUM.captures(stem)?;
    let ep = caps["ep"].parse::<u32>().ok()?;
    if ep == 0 || is_unlikely_episode_num(ep) {
        return None;
    }
    let rest = caps
        .name("rest")
        .map(|m| strip_release_noise(m.as_str()))
        .filter(|s| !s.is_empty());
    Some((ep, rest))
}

/// Extracts `(language_code, display_label)` from a sidecar subtitle filename safely across multi-byte UTF-8 stems.
pub fn parse_sidecar_subtitle_lang_and_label(
    video_stem: &str,
    sub_filename: &str,
) -> (Option<String>, String) {
    let sub_stem = sub_filename
        .rsplit_once('.')
        .map(|(s, _)| s)
        .unwrap_or(sub_filename);
    let suffix = if sub_stem
        .get(..video_stem.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(video_stem))
    {
        sub_stem[video_stem.len()..]
            .trim_start_matches(['.', '_', '-', ' '])
            .trim()
    } else {
        ""
    };

    if suffix.is_empty() {
        return (None, "External".to_string());
    }
    let lower = suffix.to_ascii_lowercase();
    let (lang, label) = match lower.as_str() {
        "zh" | "chs" | "sc" | "zh-cn" | "zh-hans" => {
            (Some("zh-CN".to_string()), "简体中文".to_string())
        }
        "cht" | "tc" | "zh-tw" | "zh-hant" => (Some("zh-TW".to_string()), "繁體中文".to_string()),
        "en" | "eng" | "en-us" => (Some("en".to_string()), "English".to_string()),
        "ja" | "jpn" | "jp" => (Some("ja".to_string()), "日本語".to_string()),
        other => (Some(other.to_string()), suffix.to_string()),
    };
    (lang, label)
}

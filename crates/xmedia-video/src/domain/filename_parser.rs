use std::sync::LazyLock;

use regex::Regex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedVideoFilename {
    pub title: String,
    pub show_title: Option<String>,
    pub season_number: Option<u32>,
    pub episode_number: Option<u32>,
    pub year: Option<i32>,
}

static RE_SXXEYY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(?P<show>.+?)[.\s_\-\[\(]+S(?P<season>\d{1,2})E(?P<ep>\d{1,4})(?:[.\s_\-\]\)]+(?P<ep_title>.*))?$")
        .expect("valid regex")
});

static RE_EP_ONLY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(?P<show>.+?)\s*[-–]\s*(?:EP?|第)?(?P<ep>\d{1,4})(?:话|集)?(?:\s*[-–\s]\s*(?P<ep_title>.*))?$")
        .expect("valid regex")
});

static RE_YEAR_SUFFIX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?P<head>.+?)[.\s_\-]*[\(\[](?P<year>19\d{2}|20\d{2})[\)\]](?:[.\s_\-].*)?$")
        .expect("valid regex")
});

static RE_SEASON_DIR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(?:season|s|第)\s*(?P<season>\d{1,2})(?:季)?$").expect("valid regex")
});

static RE_LEADING_NUM: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:EP?|e)?(?P<ep>\d{1,4})(?:[.\s_\-]+(?P<rest>.+))?$").expect("valid regex")
});

fn clean_dots_and_underscores(raw: &str) -> String {
    let replaced = raw.replace(['.', '_'], " ");
    replaced
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

pub fn parse_video_path(relative_or_filename: &str) -> ParsedVideoFilename {
    let normalized = relative_or_filename.replace('\\', "/");
    let parts: Vec<&str> = normalized
        .split('/')
        .filter(|s| !s.trim().is_empty())
        .collect();
    let filename = parts.last().copied().unwrap_or(relative_or_filename);
    let stem = filename
        .rsplit_once('.')
        .map(|(s, _)| s)
        .unwrap_or(filename)
        .trim();

    if let Some(caps) = RE_SXXEYY.captures(stem) {
        let show = clean_dots_and_underscores(&caps["show"]);
        let season = caps["season"].parse::<u32>().ok();
        let ep = caps["ep"].parse::<u32>().ok();
        let ep_title = caps
            .name("ep_title")
            .map(|m| clean_dots_and_underscores(m.as_str()))
            .filter(|s| !s.is_empty());

        let title = ep_title
            .unwrap_or_else(|| format!("S{:02}E{:02}", season.unwrap_or(1), ep.unwrap_or(1)));
        return ParsedVideoFilename {
            title,
            show_title: if show.is_empty() { None } else { Some(show) },
            season_number: season,
            episode_number: ep,
            year: None,
        };
    }

    // Check directory hierarchy: ShowName / Season 1 / 01 - Pilot.mkv  or  ShowName / 01.mkv
    if parts.len() >= 2 {
        let parent_dir = parts[parts.len() - 2].trim();
        if let Some(scaps) = RE_SEASON_DIR.captures(parent_dir)
            && parts.len() >= 3
        {
            let show = clean_dots_and_underscores(parts[parts.len() - 3]);
            let season = scaps["season"].parse::<u32>().ok();
            let (ep, ep_title) = extract_leading_episode(stem);
            if ep.is_some() && !show.is_empty() {
                return ParsedVideoFilename {
                    title: ep_title.unwrap_or_else(|| format!("Episode {}", ep.unwrap_or(1))),
                    show_title: Some(show),
                    season_number: season,
                    episode_number: ep,
                    year: None,
                };
            }
        } else {
            let (ep, ep_title) = extract_leading_episode(stem);
            if let Some(ep_num) = ep {
                let show = clean_dots_and_underscores(parent_dir);
                if !show.is_empty() {
                    return ParsedVideoFilename {
                        title: ep_title.unwrap_or_else(|| format!("Episode {ep_num}")),
                        show_title: Some(show),
                        season_number: Some(1),
                        episode_number: Some(ep_num),
                        year: None,
                    };
                }
            }
        }
    }

    if let Some(caps) = RE_EP_ONLY.captures(stem) {
        let show = clean_dots_and_underscores(&caps["show"]);
        let ep = caps["ep"].parse::<u32>().ok();
        let ep_title = caps
            .name("ep_title")
            .map(|m| clean_dots_and_underscores(m.as_str()))
            .filter(|s| !s.is_empty());
        if !show.is_empty() && ep.is_some() {
            return ParsedVideoFilename {
                title: ep_title.unwrap_or_else(|| format!("Episode {}", ep.unwrap_or(1))),
                show_title: Some(show),
                season_number: Some(1),
                episode_number: ep,
                year: None,
            };
        }
    }

    if let Some(caps) = RE_YEAR_SUFFIX.captures(stem) {
        let head = clean_dots_and_underscores(&caps["head"]);
        let year = caps["year"].parse::<i32>().ok();
        if !head.is_empty() {
            return ParsedVideoFilename {
                title: head,
                show_title: None,
                season_number: None,
                episode_number: None,
                year,
            };
        }
    }

    let cleaned = clean_dots_and_underscores(stem);
    ParsedVideoFilename {
        title: if cleaned.is_empty() {
            "Untitled Video".to_string()
        } else {
            cleaned
        },
        show_title: None,
        season_number: None,
        episode_number: None,
        year: None,
    }
}

fn extract_leading_episode(stem: &str) -> (Option<u32>, Option<String>) {
    if let Some(caps) = RE_LEADING_NUM.captures(stem)
        && let Ok(ep) = caps["ep"].parse::<u32>()
        && (1..=1900).contains(&ep)
    {
        let rest = caps
            .name("rest")
            .map(|m| clean_dots_and_underscores(m.as_str().trim_start_matches(['-', '–', ' '])))
            .filter(|s| !s.is_empty());
        return (Some(ep), rest);
    }
    (None, None)
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidecar_subtitle_parser_handles_multibyte_utf8_without_panicking() {
        // video_stem byte len (2) lands mid-character inside 3-byte UTF-8 '电'
        let (lang, label) = parse_sidecar_subtitle_lang_and_label("ab", "电影.chs.srt");
        assert_eq!(lang, None);
        assert_eq!(label, "External");

        let (lang2, label2) =
            parse_sidecar_subtitle_lang_and_label("中文电影", "中文电影.zh-CN.srt");
        assert_eq!(lang2.as_deref(), Some("zh-CN"));
        assert_eq!(label2, "简体中文");

        let (lang3, label3) = parse_sidecar_subtitle_lang_and_label("Movie", "movie.en.srt");
        assert_eq!(lang3.as_deref(), Some("en"));
        assert_eq!(label3, "English");
    }
}

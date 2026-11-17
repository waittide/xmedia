use std::collections::HashMap;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use crate::domain::media::MediaMetadataPayload;
use crate::domain::workspace::sanitize_filename_component;

static RE_WRAPPER_BLOCK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\([^\(\)]*\{[^\(\)]*\}[^\(\)]*\)|\[[^\[\]]*\{[^\[\]]*\}[^\[\]]*\]")
        .expect("valid wrapper regex")
});

static RE_TOKEN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\{([a-zA-Z0-9_]+)(?::0?(\d{1,2}))?\}").expect("valid token regex")
});

/// Formats a variable value with optional zero-padding width `{var:N}`.
///
/// - Numeric / leading-digit strings (`"1"`, `"12.5"`) have their leading integer part
///   zero-padded to `width` characters (`"01"`, `"012.5"`), never truncating longer numbers (`"105"`).
/// - Non-numeric strings are truncated to at most `width` Unicode scalar values if `width` is specified.
pub fn format_token_value(raw: &str, width: Option<usize>) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let Some(w) = width.filter(|&w| w > 0) else {
        return trimmed.to_string();
    };

    let digit_len = trimmed.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digit_len > 0 {
        let (digits, rest) = trimmed.split_at(digit_len);
        if digits.len() < w {
            let mut padded = String::with_capacity(w + rest.len());
            for _ in 0..(w - digits.len()) {
                padded.push('0');
            }
            padded.push_str(digits);
            padded.push_str(rest);
            return padded;
        }
        return trimmed.to_string();
    }

    trimmed.chars().take(w).collect()
}

fn build_token_map(
    meta: &MediaMetadataPayload,
    fallback_stem: &str,
) -> HashMap<&'static str, String> {
    let mut map = HashMap::with_capacity(12);
    let mut put = |key: &'static str, val: &str| {
        map.insert(key, sanitize_filename_component(val));
    };

    match meta {
        MediaMetadataPayload::Book(b) => {
            let author = b
                .authors
                .first()
                .map(String::as_str)
                .filter(|s| !s.trim().is_empty())
                .unwrap_or("Unknown Author");
            let year = b
                .published_at
                .as_deref()
                .and_then(|d| d.trim().get(..4))
                .unwrap_or("");
            put(
                "title",
                if b.title.trim().is_empty() {
                    fallback_stem
                } else {
                    &b.title
                },
            );
            put("author", author);
            put("year", year);
            put("isbn", b.isbn.as_deref().unwrap_or(""));
            put("publisher", b.publisher.as_deref().unwrap_or(""));
        }
        MediaMetadataPayload::Comic(c) => {
            put("series", c.series.as_deref().unwrap_or("Series"));
            put("title", c.title.as_deref().unwrap_or(fallback_stem));
            put("number", c.number.as_deref().unwrap_or("1"));
            put(
                "volume",
                &c.volume.map(|v| v.to_string()).unwrap_or_default(),
            );
            put("year", &c.year.map(|y| y.to_string()).unwrap_or_default());
            put("publisher", c.publisher.as_deref().unwrap_or(""));
        }
        MediaMetadataPayload::Music(m) => {
            let artist = if m.artist.trim().is_empty() {
                "Unknown Artist"
            } else {
                m.artist.as_str()
            };
            put(
                "track",
                &m.track_number
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "0".to_string()),
            );
            put(
                "disc",
                &m.disc_number
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "1".to_string()),
            );
            put("artist", artist);
            put("album", &m.album);
            put(
                "title",
                if m.title.trim().is_empty() {
                    fallback_stem
                } else {
                    &m.title
                },
            );
            put("year", &m.year.map(|y| y.to_string()).unwrap_or_default());
        }
        MediaMetadataPayload::Video(v) => {
            let season = v.season_number.unwrap_or(1);
            let episode = v.episode_number.unwrap_or(1);
            let show = v
                .show_title
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or(if v.title.trim().is_empty() {
                    fallback_stem
                } else {
                    &v.title
                });

            let redundant_se_code = format!("S{season:02}E{episode:02}");
            let ep_or_movie_title = if v.title.trim().is_empty() {
                if v.is_episode { "" } else { fallback_stem }
            } else if v.is_episode
                && (v.title.trim().eq_ignore_ascii_case(&redundant_se_code)
                    || v.title.trim().eq_ignore_ascii_case(show.trim()))
            {
                ""
            } else {
                v.title.trim()
            };

            let tmdb = v
                .tmdb_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or("");
            let bgm = v
                .bangumi_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or("");
            let jellyfin_id = if !tmdb.is_empty() {
                format!("[tmdbid-{}]", sanitize_filename_component(tmdb))
            } else if !bgm.is_empty() {
                format!("[bgmid-{}]", sanitize_filename_component(bgm))
            } else {
                String::new()
            };

            put("title", ep_or_movie_title);
            put("original_title", v.original_title.as_deref().unwrap_or(""));
            put("show", show);
            put("season", &season.to_string());
            put("episode", &episode.to_string());
            put("year", &v.year.map(|y| y.to_string()).unwrap_or_default());
            put("tmdbid", tmdb);
            put("bangumiid", bgm);
            // Allow `jellyfin_id` to keep its `[...]` brackets safely
            map.insert("jellyfin_id", jellyfin_id);
        }
    }

    map
}

fn replace_tokens_in_text(
    segment: &str,
    vars: &HashMap<&'static str, String>,
) -> (String, usize, usize) {
    let mut total_tokens = 0usize;
    let mut non_empty_tokens = 0usize;

    let rendered = RE_TOKEN
        .replace_all(segment, |caps: &regex::Captures<'_>| {
            total_tokens += 1;
            let key = caps[1].to_ascii_lowercase();
            let width = caps.get(2).and_then(|m| m.as_str().parse::<usize>().ok());
            let raw_val = vars.get(key.as_str()).map(String::as_str).unwrap_or("");
            let formatted = format_token_value(raw_val, width);
            if !formatted.is_empty() {
                non_empty_tokens += 1;
            }
            formatted
        })
        .into_owned();

    (rendered, total_tokens, non_empty_tokens)
}

fn render_single_segment(raw_seg: &str, vars: &HashMap<&'static str, String>) -> String {
    // 1. Evaluate `(...)` and `[...]` wrapper blocks first: if a wrapper contains one or more
    //    `{var}` tokens and ALL of them evaluate to empty, drop the entire wrapper block cleanly
    //    (e.g. `({year})` or `[tmdbid-{tmdbid}]` disappears when missing).
    let after_wrappers = RE_WRAPPER_BLOCK
        .replace_all(raw_seg, |caps: &regex::Captures<'_>| {
            let block = &caps[0];
            let (inner_rendered, tok_count, non_empty) = replace_tokens_in_text(block, vars);
            if tok_count > 0 && non_empty == 0 {
                String::new()
            } else {
                inner_rendered
            }
        })
        .into_owned();

    // 2. Replace any remaining top-level `{var}` / `{var:N}` tokens.
    let (substituted, _, _) = replace_tokens_in_text(&after_wrappers, vars);

    // 3. Clean up empty brackets, duplicate whitespace, and dangling ` - ` separators.
    let collapsed_ws = substituted
        .replace("()", "")
        .replace("[]", "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    // Collapse repeated `" - - "` from empty middle tokens (e.g. `{a} - {empty} - {b}` -> `{a} - {b}`)
    let mut cleaned = collapsed_ws;
    while cleaned.contains("- -") {
        cleaned = cleaned.replace("- -", "-");
        cleaned = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    }

    cleaned
        .trim_matches([' ', '-', '_', '.'])
        .trim()
        .to_string()
}

/// Renders a rename/archive path template (supporting `/` directory hierarchy, `{var}`, and `{var:N}` zero-padding).
///
/// Returns `(target_rel_path_with_ext, final_file_stem)`:
/// - When `tpl` contains `/` (or `\`), the rendered hierarchy is anchored at the `ManagedRoot` root
///   directory (idempotent Jellyfin layout, never double-nesting already organized files).
/// - When `tpl` is a single filename without `/`, it stays inside `current_parent_rel`.
pub fn render_template_target(
    tpl: &str,
    current_parent_rel: &str,
    meta: &MediaMetadataPayload,
    fallback_path: &Path,
    ext: &str,
) -> (String, String) {
    let fallback_stem = fallback_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Untitled");
    let vars = build_token_map(meta, fallback_stem);

    let normalized_tpl = tpl.trim().replace('\\', "/");
    let has_hierarchy = normalized_tpl.contains('/');

    let mut rendered_segments: Vec<String> = normalized_tpl
        .split('/')
        .map(|seg| render_single_segment(seg, &vars))
        .filter(|seg| !seg.is_empty() && seg != "." && seg != "..")
        .collect();

    if rendered_segments.is_empty() {
        rendered_segments.push(sanitize_filename_component(fallback_stem));
    }

    let final_stem = rendered_segments
        .pop()
        .unwrap_or_else(|| sanitize_filename_component(fallback_stem));
    let final_filename = if ext.trim().is_empty() {
        final_stem.clone()
    } else {
        format!("{final_stem}.{}", ext.trim().trim_start_matches('.'))
    };

    let dir_prefix = if has_hierarchy {
        rendered_segments.join("/")
    } else {
        current_parent_rel.trim_matches('/').to_string()
    };

    let target_rel = if dir_prefix.is_empty() {
        final_filename
    } else {
        format!("{dir_prefix}/{final_filename}")
    };

    (target_rel, final_stem)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{format_token_value, render_template_target};
    use crate::domain::media::{MediaMetadataPayload, MusicFileMeta, VideoFileMeta};

    #[test]
    fn width_specifier_pads_numbers_and_preserves_decimals_or_longer_digits() {
        assert_eq!(format_token_value("1", None), "1");
        assert_eq!(format_token_value("1", Some(2)), "01");
        assert_eq!(format_token_value("3", Some(3)), "003");
        assert_eq!(format_token_value("108", Some(2)), "108");
        assert_eq!(format_token_value("12.5", Some(3)), "012.5");
        assert_eq!(format_token_value("", Some(2)), "");
    }

    #[test]
    fn jellyfin_movie_and_episode_templates_render_hierarchy_and_drop_empty_wrappers() {
        let movie = MediaMetadataPayload::Video(VideoFileMeta {
            container: "mp4".into(),
            is_episode: false,
            title: "千与千寻".into(),
            year: Some(2001),
            tmdb_id: Some("129".into()),
            ..Default::default()
        });

        let (rel, stem) = render_template_target(
            "{title} ({year}) [tmdbid-{tmdbid}]/{title} ({year})",
            "unsorted",
            &movie,
            Path::new("/media/unsorted/raw.mp4"),
            "mp4",
        );
        assert_eq!(rel, "千与千寻 (2001) [tmdbid-129]/千与千寻 (2001).mp4");
        assert_eq!(stem, "千与千寻 (2001)");

        // Missing year and missing tmdbid cleanly drop `({year})` and `[tmdbid-{tmdbid}]`
        let movie_no_year = MediaMetadataPayload::Video(VideoFileMeta {
            container: "mkv".into(),
            is_episode: false,
            title: "My Neighbor Totoro".into(),
            year: None,
            tmdb_id: None,
            ..Default::default()
        });
        let (rel2, _) = render_template_target(
            "{title} ({year}) [tmdbid-{tmdbid}]/{title} ({year})",
            "",
            &movie_no_year,
            Path::new("/media/totoro.mkv"),
            "mkv",
        );
        assert_eq!(rel2, "My Neighbor Totoro/My Neighbor Totoro.mkv");

        // Episode with `{season:2}` and `{episode:2}`
        let ep = MediaMetadataPayload::Video(VideoFileMeta {
            container: "mkv".into(),
            is_episode: true,
            show_title: Some("葬送的芙莉莲".into()),
            title: "冒险的终点".into(),
            season_number: Some(1),
            episode_number: Some(2),
            year: Some(2023),
            ..Default::default()
        });
        let (ep_rel, ep_stem) = render_template_target(
            "{show} ({year})/Season {season:2}/{show} - S{season:2}E{episode:2} - {title}",
            "葬送的芙莉莲 (2023)/Season 01",
            &ep,
            Path::new("/media/frieren.mkv"),
            "mkv",
        );
        assert_eq!(
            ep_rel,
            "葬送的芙莉莲 (2023)/Season 01/葬送的芙莉莲 - S01E02 - 冒险的终点.mkv"
        );
        assert_eq!(ep_stem, "葬送的芙莉莲 - S01E02 - 冒险的终点");

        // Flat template without `/` stays in `current_parent_rel`
        let song = MediaMetadataPayload::Music(MusicFileMeta {
            title: "晴天".into(),
            artist: "周杰伦".into(),
            track_number: Some(3),
            ..Default::default()
        });
        let (song_rel, _) = render_template_target(
            "{track:2} - {artist} - {title}",
            "Jay/YeHuiMei",
            &song,
            Path::new("/music/Jay/YeHuiMei/03.flac"),
            "flac",
        );
        assert_eq!(song_rel, "Jay/YeHuiMei/03 - 周杰伦 - 晴天.flac");
    }
}

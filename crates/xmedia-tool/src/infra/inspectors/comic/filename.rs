use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

pub(crate) fn is_image_entry(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with('/') || lower.contains("__macosx") {
        return false;
    }
    lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".png")
        || lower.ends_with(".webp")
        || lower.ends_with(".gif")
        || lower.ends_with(".avif")
        || lower.ends_with(".bmp")
}

pub(crate) fn natural_sort_key(name: &str) -> (u8, Vec<(bool, u64, String)>) {
    let lower = name.to_ascii_lowercase();
    let base = lower.rsplit('/').next().unwrap_or(&lower);
    let priority = if base.starts_with("000_cover.") || base.starts_with("cover.") {
        0
    } else {
        1
    };
    let mut parts = Vec::new();
    let mut cur_digits = String::new();
    let mut cur_text = String::new();
    for ch in lower.chars() {
        if ch.is_ascii_digit() {
            if !cur_text.is_empty() {
                parts.push((false, 0, std::mem::take(&mut cur_text)));
            }
            cur_digits.push(ch);
        } else {
            if !cur_digits.is_empty() {
                let n = cur_digits.parse::<u64>().unwrap_or(0);
                cur_digits.clear();
                parts.push((true, n, String::new()));
            }
            cur_text.push(ch);
        }
    }
    if !cur_digits.is_empty() {
        let n = cur_digits.parse::<u64>().unwrap_or(0);
        parts.push((true, n, String::new()));
    }
    if !cur_text.is_empty() {
        parts.push((false, 0, cur_text));
    }
    (priority, parts)
}

const GENERIC_COMIC_FOLDER_NAMES: &[&str] = &[
    "comic",
    "comics",
    "manga",
    "mangas",
    "book",
    "books",
    "video",
    "videos",
    "music",
    "sample",
    "samples",
    "download",
    "downloads",
    "inbox",
    "library",
    "media",
    "漫画",
    "图书",
    "书籍",
];

pub(crate) fn is_generic_comic_folder(name: &str) -> bool {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return true;
    }
    let lower = trimmed.to_ascii_lowercase();
    GENERIC_COMIC_FOLDER_NAMES
        .iter()
        .any(|g| lower == *g || trimmed == *g)
}

pub(super) fn find_series_cover_path(comic_path: &Path) -> Option<PathBuf> {
    let parent = comic_path.parent()?;
    let dir_name = parent.file_name()?.to_str()?;
    if is_generic_comic_folder(dir_name) {
        return None;
    }
    const CANDIDATES: &[&str] = &[
        "cover.jpg",
        "cover.jpeg",
        "cover.png",
        "cover.webp",
        "folder.jpg",
        "folder.jpeg",
        "folder.png",
        "folder.webp",
    ];
    CANDIDATES
        .iter()
        .map(|name| parent.join(name))
        .find(|p| p.is_file())
}

static RE_COMIC_LEADING_BRACKET: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:\[[^\]]*\]|\([^)]*\)|\s)+").expect("valid bracket regex"));
static RE_COMIC_VOL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(?:第\s*0*(\d+(?:\.\d+)?)\s*[卷册]|(?:^|[\s_\-\.\[\(])(?:vol\.?|v)\s*0*(\d+(?:\.\d+)?))",
    )
    .expect("valid vol regex")
});
static RE_COMIC_CH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(?:第\s*0*(\d+(?:\.\d+)?)\s*[话話回章集]|(?:(?:^|[\s_\-\.\[\(])(?:ch(?:apter)?\.?|c)|#)\s*0*(\d+(?:\.\d+)?))",
    )
    .expect("valid ch regex")
});

/// Infers `(series, number, volume, optional_chapter_title)` from a comic filename stem and non-generic parent folder.
pub(super) fn guess_comic_details(
    comic_path: &Path,
) -> (Option<String>, Option<String>, Option<f64>, Option<String>) {
    let raw_stem = comic_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .trim();

    // Strip leading bracket tags such as `[Author]` or `(C103)`
    let cleaned_stem = RE_COMIC_LEADING_BRACKET
        .replace(raw_stem, "")
        .trim()
        .to_string();

    let mut cut_idx = cleaned_stem.len();
    let mut matched_end = 0usize;
    let mut volume: Option<f64> = None;
    let mut number: Option<String> = None;

    if let Some(m) = RE_COMIC_VOL.captures(&cleaned_stem) {
        let full = m.get(0).expect("full match");
        cut_idx = cut_idx.min(full.start());
        matched_end = matched_end.max(full.end());
        volume = m
            .get(1)
            .or_else(|| m.get(2))
            .and_then(|g| g.as_str().parse::<f64>().ok())
            .filter(|v| v.is_finite() && *v >= 0.0);
    }
    if let Some(m) = RE_COMIC_CH.captures(&cleaned_stem) {
        let full = m.get(0).expect("full match");
        cut_idx = cut_idx.min(full.start());
        matched_end = matched_end.max(full.end());
        number = m
            .get(1)
            .or_else(|| m.get(2))
            .map(|g| g.as_str().to_string());
    }
    let after_idx = if volume.is_some() || number.is_some() {
        matched_end
    } else {
        cleaned_stem.len()
    };

    let prefix = cleaned_stem[..cut_idx]
        .trim_matches([' ', '_', '-', '.', '·', '—'])
        .replace('_', " ")
        .trim()
        .to_string();

    let is_bare_digits = !prefix.is_empty() && prefix.chars().all(|c| c.is_ascii_digit());
    if is_bare_digits && number.is_none() {
        let trimmed_zero = prefix.trim_start_matches('0');
        number = Some(if trimmed_zero.is_empty() {
            "0".to_string()
        } else {
            trimmed_zero.to_string()
        });
    }

    let series = if !prefix.is_empty() && !is_bare_digits {
        Some(prefix)
    } else {
        comic_path
            .parent()
            .and_then(|p| p.file_name())
            .map(|s| s.to_string_lossy().into_owned())
            .filter(|dir_name| !is_generic_comic_folder(dir_name))
    };

    let guessed_title = if after_idx < cleaned_stem.len() {
        let suffix = cleaned_stem[after_idx..]
            .trim_matches([' ', '_', '-', '.', '·', '—', ':'])
            .replace('_', " ")
            .trim()
            .to_string();
        (!suffix.is_empty()).then_some(suffix)
    } else {
        None
    };

    (series, number, volume, guessed_title)
}

/// Infers `(series, number, volume)` from a comic filename stem and non-generic parent folder.
#[cfg(test)]
pub(crate) fn guess_comic_from_filename(
    comic_path: &Path,
) -> (Option<String>, Option<String>, Option<f64>) {
    let (series, number, volume, _) = guess_comic_details(comic_path);
    (series, number, volume)
}

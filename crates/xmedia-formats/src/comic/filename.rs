use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

/// Fractional-aware natural string comparator (`1 < 1.5 < 2 < 10`, case-insensitive).
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let mut ia = a.chars().peekable();
    let mut ib = b.chars().peekable();

    loop {
        match (ia.peek(), ib.peek()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(&ca), Some(&cb)) => {
                if ca.is_ascii_digit() && cb.is_ascii_digit() {
                    let na = take_number_tokens(&mut ia);
                    let nb = take_number_tokens(&mut ib);
                    match cmp_number_tokens(&na, &nb) {
                        Ordering::Equal => continue,
                        non_eq => return non_eq,
                    }
                } else {
                    let la = ca.to_lowercase().next().unwrap_or(ca);
                    let lb = cb.to_lowercase().next().unwrap_or(cb);
                    ia.next();
                    ib.next();
                    match la.cmp(&lb) {
                        Ordering::Equal => continue,
                        non_eq => return non_eq,
                    }
                }
            }
        }
    }
}

/// Natural comparator for comic page filenames that prioritizes explicit `000_cover.*` / `cover.*` first.
pub fn natural_page_entry_cmp(a: &str, b: &str) -> Ordering {
    let prio = |name: &str| -> u8 {
        let base = name.rsplit('/').next().unwrap_or(name).to_ascii_lowercase();
        if base.starts_with("000_cover.") || base.starts_with("cover.") {
            0
        } else {
            1
        }
    };
    prio(a).cmp(&prio(b)).then_with(|| natural_cmp(a, b))
}

#[derive(Debug, PartialEq, Eq)]
struct NumberToken {
    int_part: u128,
    frac_part: String,
    raw_len: usize,
}

fn take_number_tokens<I>(iter: &mut std::iter::Peekable<I>) -> NumberToken
where
    I: Iterator<Item = char> + Clone,
{
    let mut raw_int = String::new();
    while let Some(&c) = iter.peek() {
        if c.is_ascii_digit() {
            raw_int.push(c);
            iter.next();
        } else {
            break;
        }
    }

    let mut frac_part = String::new();
    let mut clone = iter.clone();
    if clone.next() == Some('.') && clone.peek().is_some_and(|c| c.is_ascii_digit()) {
        iter.next();
        while let Some(&c) = iter.peek() {
            if c.is_ascii_digit() {
                frac_part.push(c);
                iter.next();
            } else {
                break;
            }
        }
    }

    let int_part = raw_int.parse::<u128>().unwrap_or(u128::MAX);
    NumberToken {
        int_part,
        frac_part,
        raw_len: raw_int.len(),
    }
}

fn cmp_number_tokens(a: &NumberToken, b: &NumberToken) -> Ordering {
    match a.int_part.cmp(&b.int_part) {
        Ordering::Equal => {}
        non_eq => return non_eq,
    }
    let fa = a.frac_part.trim_end_matches('0');
    let fb = b.frac_part.trim_end_matches('0');
    match fa.cmp(fb) {
        Ordering::Equal => a.raw_len.cmp(&b.raw_len),
        non_eq => non_eq,
    }
}

fn natural_title_sort_key(title: &str) -> String {
    let mut out = String::with_capacity(title.len() + 16);
    let mut chars = title.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_ascii_digit() {
            let mut digits = String::new();
            while let Some(&d) = chars.peek() {
                if d.is_ascii_digit() {
                    digits.push(d);
                    chars.next();
                } else {
                    break;
                }
            }
            let trimmed = digits.trim_start_matches('0');
            let normalized = if trimmed.is_empty() { "0" } else { trimmed };
            if normalized.len() < 18 {
                for _ in 0..(18 - normalized.len()) {
                    out.push('0');
                }
            }
            out.push_str(normalized);
        } else {
            for lc in c.to_lowercase() {
                out.push(lc);
            }
            chars.next();
        }
    }
    out
}

pub fn build_chapter_sort_key(
    kind_rank: u8,
    volume: Option<f64>,
    number: f64,
    title: &str,
) -> String {
    let vol = volume.filter(|v| v.is_finite()).unwrap_or(0.0).max(0.0);
    let vol_scaled = (vol * 1000.0).round().min(9_999_999_999_999_999.0) as u64;
    let num = if number.is_finite() {
        number.max(0.0)
    } else {
        0.0
    };
    let num_scaled = (num * 1000.0).round().min(9_999_999_999_999_999.0) as u64;
    format!(
        "{kind_rank:02}:{vol_scaled:016}:{num_scaled:016}:{}",
        natural_title_sort_key(title)
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ComicChapterKind {
    Chapter,
    Volume,
    Special,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedComicChapterName {
    pub inferred_series: Option<String>,
    pub kind: ComicChapterKind,
    pub volume_number: Option<f64>,
    pub number: f64,
    pub clean_title: String,
}

pub fn strip_comic_archive_ext(name: &str) -> &str {
    for ext in [
        ".cbz", ".zip", ".epub", ".cbr", ".cb7", ".CBZ", ".ZIP", ".EPUB", ".CBR", ".CB7",
    ] {
        if let Some(stripped) = name.strip_suffix(ext) {
            return stripped;
        }
    }
    name
}

fn strip_leading_brackets(mut s: &str) -> &str {
    loop {
        s = s.trim_start();
        if let Some(rest) = s.strip_prefix('[')
            && let Some((_, after)) = rest.split_once(']')
        {
            s = after;
            continue;
        }
        if let Some(rest) = s.strip_prefix('【')
            && let Some((_, after)) = rest.split_once('】')
        {
            s = after;
            continue;
        }
        break;
    }
    s
}

fn parse_leading_float(s: &str) -> Option<f64> {
    let mut buf = String::new();
    let mut seen_dot = false;
    for ch in s.chars() {
        if ch.is_ascii_digit() {
            buf.push(ch);
        } else if ch == '.' && !seen_dot && !buf.is_empty() {
            seen_dot = true;
            buf.push('.');
        } else {
            break;
        }
    }
    let clean = buf.trim_end_matches('.');
    if clean.is_empty() {
        None
    } else {
        clean
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite() && *v >= 0.0)
    }
}

fn extract_prefixed_number(lower: &str, prefixes: &[&str]) -> Option<f64> {
    for prefix in prefixes {
        let mut search_from = 0;
        while let Some(pos) = lower[search_from..].find(prefix) {
            let abs_pos = search_from + pos;
            let boundary_ok = abs_pos == 0
                || !lower[..abs_pos]
                    .chars()
                    .next_back()
                    .is_some_and(|c| c.is_ascii_alphabetic());
            let after = &lower[abs_pos + prefix.len()..];
            if boundary_ok && let Some(num) = parse_leading_float(after.trim_start()) {
                return Some(num);
            }
            search_from = abs_pos + prefix.len();
        }
    }
    None
}

fn extract_single_letter_shorthand(lower: &str, letter: char) -> Option<f64> {
    let chars: Vec<(usize, char)> = lower.char_indices().collect();
    for (idx, &(byte_pos, ch)) in chars.iter().enumerate() {
        if ch == letter {
            let prev_ok = idx == 0 || !chars[idx - 1].1.is_ascii_alphabetic();
            if prev_ok
                && idx + 1 < chars.len()
                && chars[idx + 1].1.is_ascii_digit()
                && let Some(num) = parse_leading_float(&lower[byte_pos + 1..])
            {
                return Some(num);
            }
        }
    }
    None
}

fn extract_chinese_unit_number(text: &str, units: &[char]) -> Option<f64> {
    let mut search_from = 0usize;
    while let Some(rel) = text[search_from..].find('第') {
        let pos = search_from + rel;
        let after = text[pos + '第'.len_utf8()..].trim_start();
        let num_str: String = after
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        if !num_str.is_empty() {
            let remaining = after[num_str.len()..].trim_start();
            if remaining.chars().next().is_some_and(|u| units.contains(&u))
                && let Ok(val) = num_str.parse::<f64>()
                && val.is_finite()
                && val >= 0.0
            {
                return Some(val);
            }
        }
        search_from = pos + '第'.len_utf8();
    }
    None
}

fn extract_first_number(s: &str) -> Option<f64> {
    for (idx, ch) in s.char_indices() {
        if ch.is_ascii_digit() {
            return parse_leading_float(&s[idx..]);
        }
    }
    None
}

fn extract_last_number(s: &str) -> Option<f64> {
    let mut found = None;
    let mut i = 0;
    let bytes = s.as_bytes();
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            if let Some(val) = parse_leading_float(&s[i..]) {
                found = Some(val);
            }
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    found
}

/// Parses a comic chapter/volume filename (`Series - Vol.02 Ch.12.5.cbz`, `第03卷.cbz`, `SP01`, etc.).
pub fn parse_comic_chapter_filename(filename: &str, fallback_index: u32) -> ParsedComicChapterName {
    let stem = strip_comic_archive_ext(filename).trim();
    let lower = stem.to_lowercase();

    let (inferred_series, rest) = if let Some((left, right)) = stem.split_once(" - ") {
        let clean_left = strip_leading_brackets(left).trim().to_owned();
        (
            if clean_left.is_empty() {
                None
            } else {
                Some(clean_left)
            },
            right.trim(),
        )
    } else {
        (None, stem)
    };

    let rest_lower = rest.to_lowercase();
    let volume_number = extract_prefixed_number(&rest_lower, &["vol.", "vol ", "volume ", "v."])
        .or_else(|| extract_single_letter_shorthand(&rest_lower, 'v'))
        .or_else(|| extract_chinese_unit_number(rest, &['卷', '册']));

    let has_sp_token = rest_lower.strip_prefix("sp").is_some_and(|after| {
        after.is_empty()
            || !after
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic())
    });

    let is_special = rest_lower.contains("special")
        || rest_lower.contains("extra")
        || rest_lower.contains("omake")
        || has_sp_token
        || rest.contains("番外")
        || rest.contains("特别篇")
        || rest.contains("外传");

    let explicit_chapter = extract_prefixed_number(
        &rest_lower,
        &[
            "chapter ", "chap.", "chap ", "ch.", "ch ", "ep.", "ep ", "#",
        ],
    )
    .or_else(|| extract_single_letter_shorthand(&rest_lower, 'c'))
    .or_else(|| extract_chinese_unit_number(rest, &['话', '話', '回', '集']));

    let (kind, number) = if is_special {
        let sp_num = explicit_chapter
            .or_else(|| extract_prefixed_number(&rest_lower, &["sp.", "sp ", "sp"]))
            .or_else(|| extract_first_number(&lower))
            .unwrap_or(fallback_index as f64);
        (ComicChapterKind::Special, sp_num)
    } else if let Some(ch_num) = explicit_chapter {
        (ComicChapterKind::Chapter, ch_num)
    } else if let Some(vol_num) = volume_number {
        (ComicChapterKind::Volume, vol_num)
    } else if let Some(any_num) = extract_last_number(rest) {
        (ComicChapterKind::Chapter, any_num)
    } else {
        (ComicChapterKind::Chapter, fallback_index as f64)
    };

    ParsedComicChapterName {
        inferred_series,
        kind,
        volume_number,
        number,
        clean_title: if rest.is_empty() {
            filename.to_owned()
        } else {
            rest.to_owned()
        },
    }
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

pub fn is_generic_comic_folder(name: &str) -> bool {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return true;
    }
    let lower = trimmed.to_ascii_lowercase();
    GENERIC_COMIC_FOLDER_NAMES
        .iter()
        .any(|g| lower == *g || trimmed == *g)
}

pub fn find_series_cover_path(comic_path: &Path) -> Option<PathBuf> {
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

/// Infers `(series, number_str, volume, optional_chapter_title)` from a comic archive or image-dir path.
pub fn guess_comic_details_from_path(
    comic_path: &Path,
) -> (Option<String>, Option<String>, Option<f64>, Option<String>) {
    let raw_stem = if comic_path.is_dir() {
        comic_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .trim()
    } else {
        comic_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .trim()
    };

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

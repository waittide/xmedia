use crate::domain::chapter::ChapterKind;

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedChapterName {
    pub inferred_series: Option<String>,
    pub kind: ChapterKind,
    pub volume_number: Option<f64>,
    pub number: f64,
    pub clean_title: String,
}

pub fn parse_chapter_filename(filename: &str, fallback_index: u32) -> ParsedChapterName {
    let stem = strip_archive_ext(filename).trim();
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
        .or_else(|| extract_v_shorthand(&rest_lower))
        .or_else(|| extract_chinese_unit_number(rest, &['卷', '册']));

    let is_special = rest_lower.contains("special")
        || rest_lower.contains("extra")
        || rest_lower.contains("omake")
        || rest_lower.starts_with("sp")
        || rest.contains("番外")
        || rest.contains("特别篇")
        || rest.contains("外传");

    let explicit_chapter = extract_prefixed_number(
        &rest_lower,
        &[
            "chapter ", "chap.", "chap ", "ch.", "ch ", "ep.", "ep ", "#",
        ],
    )
    .or_else(|| extract_c_shorthand(&rest_lower))
    .or_else(|| extract_chinese_unit_number(rest, &['话', '話', '回', '集']));

    let (kind, number) = if is_special {
        let sp_num = explicit_chapter
            .or_else(|| extract_prefixed_number(&rest_lower, &["sp.", "sp ", "sp"]))
            .or_else(|| extract_first_number(&lower))
            .unwrap_or(fallback_index as f64);
        (ChapterKind::Special, sp_num)
    } else if let Some(ch_num) = explicit_chapter {
        (ChapterKind::Chapter, ch_num)
    } else if let Some(vol_num) = volume_number {
        (ChapterKind::Volume, vol_num)
    } else if let Some(any_num) = extract_last_number(rest) {
        (ChapterKind::Chapter, any_num)
    } else {
        (ChapterKind::Chapter, fallback_index as f64)
    };

    let clean_title = if rest.is_empty() {
        filename.to_owned()
    } else {
        rest.to_owned()
    };

    ParsedChapterName {
        inferred_series,
        kind,
        volume_number,
        number,
        clean_title,
    }
}

pub fn strip_archive_ext(name: &str) -> &str {
    for ext in [".cbz", ".zip", ".epub", ".CBZ", ".ZIP", ".EPUB"] {
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

fn extract_v_shorthand(lower: &str) -> Option<f64> {
    extract_single_letter_shorthand(lower, 'v')
}

fn extract_c_shorthand(lower: &str) -> Option<f64> {
    extract_single_letter_shorthand(lower, 'c')
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
    if let Some(pos) = text.find('第') {
        let after = text[pos + '第'.len_utf8()..].trim_start();
        let num_str: String = after
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        if !num_str.is_empty() {
            let remaining = after[num_str.len()..].trim_start();
            if remaining.chars().next().is_some_and(|u| units.contains(&u))
                && let Ok(val) = num_str.parse::<f64>()
            {
                return Some(val);
            }
        }
    }
    None
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
        clean.parse::<f64>().ok()
    }
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

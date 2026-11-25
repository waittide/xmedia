use std::collections::{BTreeSet, HashMap};
use std::sync::{LazyLock, Mutex};

use regex::Regex;
use xmedia_core::domain::error::{DomainError, DomainResult};

use crate::domain::zh_text::{ZhConvertDirection, convert_zh_plain};

use super::BatchTextReplaceRule;

pub(super) fn non_empty_trimmed(opt: &Option<String>) -> Option<String> {
    opt.as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

pub(super) fn split_tokens(raw: &str) -> Vec<String> {
    raw.split(['/', ';', '；', '、', '\n'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

pub(super) fn merge_tag_edits(
    current: &[String],
    add: &[String],
    remove: &[String],
) -> Option<Vec<String>> {
    if add.is_empty() && remove.is_empty() {
        return None;
    }
    let remove_lower: BTreeSet<String> = remove
        .iter()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut out: Vec<String> = Vec::new();
    for t in current {
        let trimmed = t.trim();
        let low = trimmed.to_lowercase();
        if !trimmed.is_empty() && !remove_lower.contains(&low) && seen.insert(low) {
            out.push(trimmed.to_string());
        }
    }
    for t in add {
        let trimmed = t.trim();
        let low = trimmed.to_lowercase();
        if !trimmed.is_empty() && !remove_lower.contains(&low) && seen.insert(low) {
            out.push(trimmed.to_string());
        }
    }
    if out == current { None } else { Some(out) }
}

pub(super) fn render_title_template(
    tpl: &str,
    file_stem: &str,
    cur_title: &str,
    cur_creator: &str,
    cur_series: &str,
    cur_year: &str,
    counter: usize,
) -> String {
    tpl.replace("${filename}", file_stem)
        .replace("${title}", cur_title)
        .replace("${creator}", cur_creator)
        .replace("${series}", cur_series)
        .replace("${year}", cur_year)
        .replace("${counter}", &counter.to_string())
        .replace("${counter:2}", &format!("{counter:02}"))
        .replace("${counter:3}", &format!("{counter:03}"))
        .trim()
        .to_string()
}

const MAX_BATCH_REGEX_CACHE: usize = 64;

static BATCH_REGEX_CACHE: LazyLock<Mutex<HashMap<String, Regex>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn compile_cached_user_regex(pattern: &str) -> DomainResult<Regex> {
    {
        let guard = BATCH_REGEX_CACHE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(re) = guard.get(pattern) {
            return Ok(re.clone());
        }
    }
    let compiled = regex::RegexBuilder::new(pattern)
        .size_limit(2 * 1024 * 1024)
        .dfa_size_limit(2 * 1024 * 1024)
        .build()
        .map_err(|e| DomainError::Validation(format!("无效的正则表达式: {e}")))?;
    let mut guard = BATCH_REGEX_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if guard.len() >= MAX_BATCH_REGEX_CACHE && !guard.contains_key(pattern) {
        guard.clear();
    }
    guard.insert(pattern.to_string(), compiled.clone());
    Ok(compiled)
}

pub(super) struct CompiledReplacer {
    regex: Option<Regex>,
    literal: String,
    replace: String,
    pub(super) target_title: bool,
    pub(super) target_series: bool,
    pub(super) target_creator: bool,
    pub(super) target_summary: bool,
}

impl CompiledReplacer {
    pub(super) fn compile(rule: &BatchTextReplaceRule) -> DomainResult<Option<Self>> {
        if rule.find.is_empty() {
            return Ok(None);
        }
        let regex = if rule.use_regex {
            Some(compile_cached_user_regex(&rule.find)?)
        } else {
            None
        };
        let has = |f: &str| {
            if rule.fields.is_empty() {
                f == "title" || f == "series"
            } else {
                rule.fields.iter().any(|x| x.eq_ignore_ascii_case(f))
            }
        };
        Ok(Some(Self {
            regex,
            literal: rule.find.clone(),
            replace: rule.replace.clone(),
            target_title: has("title"),
            target_series: has("series"),
            target_creator: has("creator"),
            target_summary: has("summary"),
        }))
    }

    pub(super) fn apply(&self, input: &str) -> String {
        if let Some(ref re) = self.regex {
            re.replace_all(input, self.replace.as_str()).into_owned()
        } else {
            input.replace(&self.literal, &self.replace)
        }
    }
}

pub(super) fn transform_str(
    cur: &str,
    replacer: Option<&CompiledReplacer>,
    should_replace: bool,
    zh_dir: Option<ZhConvertDirection>,
) -> Option<String> {
    let mut next = cur.to_string();
    if should_replace && let Some(r) = replacer {
        next = r.apply(&next);
    }
    if let Some(dir) = zh_dir {
        next = convert_zh_plain(&next, dir);
    }
    let next = next.trim().to_string();
    if next == cur.trim() { None } else { Some(next) }
}

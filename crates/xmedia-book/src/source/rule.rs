use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use regex::Regex;
use scraper::{ElementRef, Html, Selector};

use crate::source::http::resolve_relative_url;
use crate::source::js_runtime::LegadoJsSession;

const MAX_RULE_REGEX_CACHE: usize = 256;

static RULE_REGEX_CACHE: LazyLock<Mutex<HashMap<String, Regex>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub(crate) fn cached_regex(pattern: &str) -> Option<Regex> {
    {
        let guard = RULE_REGEX_CACHE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(re) = guard.get(pattern) {
            return Some(re.clone());
        }
    }
    let compiled = Regex::new(pattern).ok()?;
    let mut guard = RULE_REGEX_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if guard.len() >= MAX_RULE_REGEX_CACHE && !guard.contains_key(pattern) {
        guard.clear();
    }
    guard.insert(pattern.to_string(), compiled.clone());
    Some(compiled)
}

/// Returns true when evaluating `rule` may require a QuickJS runtime.
pub fn rule_needs_js(rule: &str) -> bool {
    let trimmed = rule.trim();
    if trimmed.is_empty() {
        return false;
    }
    if split_js_pipeline(trimmed).is_some() {
        return true;
    }
    let mut rest = trimmed;
    while let Some(open) = rest.find("{{") {
        let after_open = &rest[open + 2..];
        let Some(close) = after_open.find("}}") else {
            break;
        };
        let expr = after_open[..close].trim();
        let is_pure_non_js = expr.starts_with("$.")
            || expr.starts_with("$[")
            || expr.starts_with("@css:")
            || expr.starts_with("@CSS:")
            || expr.starts_with("@Json:")
            || expr.starts_with("@json:");
        if !is_pure_non_js {
            return true;
        }
        rest = &after_open[close + 2..];
    }
    false
}

/// Unified input passed between chained Legado rules.
#[derive(Debug, Clone)]
pub enum RuleValue {
    Text(String),
    Json(serde_json::Value),
}

impl RuleValue {
    pub fn from_raw(raw: String) -> Self {
        let trimmed = raw.trim();
        if ((trimmed.starts_with('{') && trimmed.ends_with('}'))
            || (trimmed.starts_with('[') && trimmed.ends_with(']')))
            && let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed)
        {
            return Self::Json(v);
        }
        Self::Text(raw)
    }

    pub fn as_text(&self) -> String {
        match self {
            Self::Text(s) => s.clone(),
            Self::Json(serde_json::Value::String(s)) => s.clone(),
            Self::Json(serde_json::Value::Null) => String::new(),
            Self::Json(serde_json::Value::Number(n)) => n.to_string(),
            Self::Json(serde_json::Value::Bool(b)) => b.to_string(),
            Self::Json(other) => other.to_string(),
        }
    }

    pub fn as_json(&self) -> Option<serde_json::Value> {
        match self {
            Self::Json(v) => Some(v.clone()),
            Self::Text(s) => {
                let t = s.trim();
                if t.starts_with('{') || t.starts_with('[') {
                    serde_json::from_str(t).ok()
                } else {
                    None
                }
            }
        }
    }
}

const MAX_RULE_EVAL_DEPTH: usize = 32;
const MAX_JSON_RECURSIVE_DEPTH: usize = 128;

/// Evaluates a Legado rule to a single string (supports `<js>`, `@js:`, `{{...}}`, `##regex##rep`, `||`, `&&`, JSONPath, CSS/Legado shorthand).
pub fn eval_rule_string(
    input: &RuleValue,
    rule: &str,
    base_url: &str,
    js: Option<&mut LegadoJsSession>,
) -> Result<String, String> {
    eval_rule_string_depth(input, rule, base_url, js, 0)
}

fn eval_rule_string_depth(
    input: &RuleValue,
    rule: &str,
    base_url: &str,
    mut js: Option<&mut LegadoJsSession>,
    depth: usize,
) -> Result<String, String> {
    if depth >= MAX_RULE_EVAL_DEPTH {
        return Err(format!("规则递归嵌套深度超过上限 ({MAX_RULE_EVAL_DEPTH})"));
    }
    let trimmed = rule.trim();
    if trimmed.is_empty() {
        return Ok(String::new());
    }

    // 1. Check `<js>...</js>` or `@js:` pipeline
    if let Some((prefix_rule, js_code, suffix_rule)) = split_js_pipeline(trimmed) {
        let stage1 = if prefix_rule.is_empty() {
            input.clone()
        } else {
            let s =
                eval_rule_string_depth(input, prefix_rule, base_url, js.as_deref_mut(), depth + 1)?;
            RuleValue::from_raw(s)
        };
        let js_session = js
            .as_deref_mut()
            .ok_or_else(|| "规则包含 JS 代码，但当前上下文未启用 JS 沙箱".to_string())?;
        let js_out = js_session.eval_rule_js(js_code, &stage1, base_url)?;
        if suffix_rule.is_empty() {
            return Ok(js_out.as_text());
        }
        return eval_rule_string_depth(&js_out, suffix_rule, base_url, js, depth + 1);
    }

    // 2. Check Mustache interpolation `{{...}}`
    if trimmed.contains("{{") && trimmed.contains("}}") {
        let (template_part, regex_suffix) = split_regex_replace_suffix(trimmed);
        let interpolated = eval_mustache_template(template_part, input, base_url, js)?;
        return Ok(apply_regex_replace(&interpolated, regex_suffix));
    }

    // 3. Fallback (`||`) and concat (`&&`) outside quotes
    if let Some(parts) = split_top_level_op(trimmed, "||") {
        for part in parts {
            let res = eval_rule_string_depth(input, part, base_url, js.as_deref_mut(), depth + 1)?;
            if !res.trim().is_empty() {
                return Ok(res);
            }
        }
        return Ok(String::new());
    }
    if let Some(parts) = split_top_level_op(trimmed, "&&") {
        let mut out = Vec::new();
        for part in parts {
            let res = eval_rule_string_depth(input, part, base_url, js.as_deref_mut(), depth + 1)?;
            if !res.trim().is_empty() {
                out.push(res);
            }
        }
        return Ok(out.join("\n"));
    }

    // 4. Regex replace suffix `##pattern##replacement`
    let (core_rule, regex_suffix) = split_regex_replace_suffix(trimmed);
    let extracted = eval_non_js_single(input, core_rule, base_url);
    Ok(apply_regex_replace(&extracted, regex_suffix))
}

/// Evaluates a Legado rule to a list of items (used for `bookList`, `chapterList`, etc.).
pub fn eval_rule_list(
    input: &RuleValue,
    rule: &str,
    base_url: &str,
    js: Option<&mut LegadoJsSession>,
) -> Result<Vec<RuleValue>, String> {
    eval_rule_list_depth(input, rule, base_url, js, 0)
}

fn eval_rule_list_depth(
    input: &RuleValue,
    rule: &str,
    base_url: &str,
    mut js: Option<&mut LegadoJsSession>,
    depth: usize,
) -> Result<Vec<RuleValue>, String> {
    if depth >= MAX_RULE_EVAL_DEPTH {
        return Err(format!(
            "列表规则递归嵌套深度超过上限 ({MAX_RULE_EVAL_DEPTH})"
        ));
    }
    let trimmed = rule.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    // 1. `<js>...</js>` or `@js:` pipeline
    if let Some((prefix_rule, js_code, suffix_rule)) = split_js_pipeline(trimmed) {
        let stage1 = if prefix_rule.is_empty() {
            input.clone()
        } else {
            let s =
                eval_rule_string_depth(input, prefix_rule, base_url, js.as_deref_mut(), depth + 1)?;
            RuleValue::from_raw(s)
        };
        let js_session = js
            .as_deref_mut()
            .ok_or_else(|| "列表规则包含 JS 代码，但当前上下文未启用 JS 沙箱".to_string())?;
        let js_out = js_session.eval_rule_js(js_code, &stage1, base_url)?;
        if suffix_rule.is_empty() {
            return Ok(value_to_list(js_out));
        }
        return eval_rule_list_depth(&js_out, suffix_rule, base_url, js, depth + 1);
    }

    // 2. Fallback `||`
    if let Some(parts) = split_top_level_op(trimmed, "||") {
        for part in parts {
            let list = eval_rule_list_depth(input, part, base_url, js.as_deref_mut(), depth + 1)?;
            if !list.is_empty() {
                return Ok(list);
            }
        }
        return Ok(Vec::new());
    }

    // 3. Concat `&&`
    if let Some(parts) = split_top_level_op(trimmed, "&&") {
        let mut combined = Vec::new();
        for part in parts {
            combined.extend(eval_rule_list(input, part, base_url, js.as_deref_mut())?);
        }
        return Ok(combined);
    }

    // 4. Interleave `%%`
    if let Some(parts) = split_top_level_op(trimmed, "%%") {
        let mut lists = Vec::with_capacity(parts.len());
        let mut max_len = 0usize;
        for part in parts {
            let l = eval_rule_list(input, part, base_url, js.as_deref_mut())?;
            max_len = max_len.max(l.len());
            lists.push(l);
        }
        let mut interleaved = Vec::new();
        for i in 0..max_len {
            for l in &lists {
                if let Some(item) = l.get(i) {
                    interleaved.push(item.clone());
                }
            }
        }
        return Ok(interleaved);
    }

    Ok(eval_non_js_list(input, trimmed))
}

/// Evaluates a non-JS rule directly (callable from `java.getString` inside JS sandbox).
pub fn eval_non_js_single(input: &RuleValue, rule: &str, base_url: &str) -> String {
    let trimmed = rule.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let (core, regex_suffix) = split_regex_replace_suffix(trimmed);

    // JSONPath (`$.foo`, `@Json:$.foo`, `$[0]`)
    if let Some(jp) = core
        .strip_prefix("@Json:")
        .or_else(|| core.strip_prefix("@json:"))
        .or_else(|| core.starts_with("$.").then_some(core))
        .or_else(|| core.starts_with("$[").then_some(core))
        .or_else(|| (core == "$").then_some(core))
    {
        if let Some(json_val) = input.as_json() {
            let matches = eval_jsonpath(&json_val, jp);
            let joined = matches
                .into_iter()
                .map(json_val_to_string)
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("\n");
            return apply_regex_replace(&joined, regex_suffix);
        }
        return String::new();
    }

    // Direct JSON field access fallback if input is a JSON object and rule looks like a bare key
    if let RuleValue::Json(serde_json::Value::Object(map)) = input
        && !core.contains('@')
        && !core.contains('.')
        && !core.contains(' ')
        && let Some(v) = map.get(core)
    {
        return apply_regex_replace(&json_val_to_string(v.clone()), regex_suffix);
    }

    // Regex capture rule (`:pattern`)
    if let Some(pat) = core.strip_prefix(':') {
        let text = input.as_text();
        if let Some(re) = cached_regex(pat)
            && let Some(caps) = re.captures(&text)
        {
            let matched = caps
                .get(1)
                .or_else(|| caps.get(0))
                .map(|m| m.as_str().to_string())
                .unwrap_or_default();
            return apply_regex_replace(&matched, regex_suffix);
        }
        return String::new();
    }

    // CSS / Legado HTML rule
    let html_str = input.as_text();
    let extracted = eval_html_single(&html_str, core, base_url);
    apply_regex_replace(&extracted, regex_suffix)
}

pub fn eval_non_js_list(input: &RuleValue, rule: &str) -> Vec<RuleValue> {
    let trimmed = rule.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    // Optional leading `-` reverses order in Legado (`-$.data` or `-class.item`)
    let (reverse, core) = if let Some(rest) = trimmed.strip_prefix('-') {
        (true, rest.trim())
    } else if let Some(rest) = trimmed.strip_prefix('+') {
        (false, rest.trim())
    } else {
        (false, trimmed)
    };

    let mut items = if let Some(jp) = core
        .strip_prefix("@Json:")
        .or_else(|| core.strip_prefix("@json:"))
        .or_else(|| core.starts_with("$.").then_some(core))
        .or_else(|| core.starts_with("$[").then_some(core))
        .or_else(|| (core == "$").then_some(core))
    {
        if let Some(json_val) = input.as_json() {
            let raw_matches = eval_jsonpath(&json_val, jp);
            let mut flattened = Vec::new();
            for m in raw_matches {
                match m {
                    serde_json::Value::Array(arr) => {
                        for el in arr {
                            flattened.push(RuleValue::Json(el));
                        }
                    }
                    other => flattened.push(RuleValue::Json(other)),
                }
            }
            flattened
        } else {
            Vec::new()
        }
    } else {
        let html_str = input.as_text();
        eval_html_list(&html_str, core)
            .into_iter()
            .map(RuleValue::Text)
            .collect()
    };

    if reverse {
        items.reverse();
    }
    items
}

fn value_to_list(val: RuleValue) -> Vec<RuleValue> {
    match val {
        RuleValue::Json(serde_json::Value::Array(arr)) => {
            arr.into_iter().map(RuleValue::Json).collect()
        }
        RuleValue::Json(serde_json::Value::Null) => Vec::new(),
        RuleValue::Text(s) if s.trim().is_empty() => Vec::new(),
        other => vec![other],
    }
}

/// Splits a rule into `(prefix_rule, js_code, suffix_rule)` if it contains `<js>...</js>` or `@js:`.
pub fn split_js_pipeline(rule: &str) -> Option<(&str, &str, &str)> {
    let lower = rule.to_ascii_lowercase();
    if let Some(start) = lower.find("<js>") {
        let code_start = start + 4;
        if let Some(rel_end) = lower[code_start..].find("</js>") {
            let code_end = code_start + rel_end;
            let prefix = rule[..start].trim();
            let code = rule[code_start..code_end].trim();
            let suffix = rule[code_end + 5..].trim();
            return Some((prefix, code, suffix));
        }
    }
    if let Some(idx) = lower.find("@js:") {
        let prefix = rule[..idx].trim();
        let code = rule[idx + 4..].trim();
        return Some((prefix, code, ""));
    }
    None
}

fn eval_mustache_template(
    template: &str,
    input: &RuleValue,
    base_url: &str,
    mut js: Option<&mut LegadoJsSession>,
) -> Result<String, String> {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find("{{") {
        out.push_str(&rest[..open]);
        let after_open = &rest[open + 2..];
        let Some(close) = after_open.find("}}") else {
            out.push_str(&rest[open..]);
            return Ok(out);
        };
        let expr = after_open[..close].trim();
        let val = if expr.starts_with("$.")
            || expr.starts_with("$[")
            || expr.starts_with("@css:")
            || expr.starts_with("@Json:")
            || expr.starts_with("@json:")
        {
            eval_non_js_single(input, expr, base_url)
        } else if let Some(js_session) = js.as_deref_mut() {
            js_session
                .eval_rule_js(expr, input, base_url)
                .map(|v| v.as_text())
                .unwrap_or_else(|_| eval_non_js_single(input, expr, base_url))
        } else {
            eval_non_js_single(input, expr, base_url)
        };
        out.push_str(&val);
        rest = &after_open[close + 2..];
    }
    out.push_str(rest);
    Ok(out)
}

#[derive(Debug, Clone, Copy)]
struct RegexReplaceSpec<'a> {
    pattern: &'a str,
    replacement: &'a str,
    only_first: bool,
}

fn find_double_hash_outside_mustache(rule: &str) -> Option<usize> {
    let bytes = rule.as_bytes();
    let mut i = 0usize;
    let mut mustache_depth = 0usize;
    while i + 1 < bytes.len() {
        if bytes[i] == b'{' && bytes[i + 1] == b'{' {
            mustache_depth += 1;
            i += 2;
            continue;
        }
        if bytes[i] == b'}' && bytes[i + 1] == b'}' && mustache_depth > 0 {
            mustache_depth -= 1;
            i += 2;
            continue;
        }
        if mustache_depth == 0 && bytes[i] == b'#' && bytes[i + 1] == b'#' {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn split_regex_replace_suffix(rule: &str) -> (&str, Option<RegexReplaceSpec<'_>>) {
    let Some(first_idx) = find_double_hash_outside_mustache(rule) else {
        return (rule, None);
    };
    let core = rule[..first_idx].trim();
    let tail = &rule[first_idx + 2..];
    if let Some(second_idx) = tail.find("##") {
        let pattern = &tail[..second_idx];
        let after = &tail[second_idx + 2..];
        if let Some(rep) = after.strip_suffix("###") {
            return (
                core,
                Some(RegexReplaceSpec {
                    pattern,
                    replacement: rep,
                    only_first: true,
                }),
            );
        }
        return (
            core,
            Some(RegexReplaceSpec {
                pattern,
                replacement: after,
                only_first: false,
            }),
        );
    }
    (
        core,
        Some(RegexReplaceSpec {
            pattern: tail,
            replacement: "",
            only_first: false,
        }),
    )
}

fn apply_regex_replace(input: &str, spec: Option<RegexReplaceSpec<'_>>) -> String {
    let Some(s) = spec else {
        return input.to_string();
    };
    if s.pattern.is_empty() {
        return input.to_string();
    }
    let Some(re) = cached_regex(s.pattern) else {
        return input.to_string();
    };
    if s.only_first {
        re.replace(input, s.replacement).into_owned()
    } else {
        re.replace_all(input, s.replacement).into_owned()
    }
}

fn split_top_level_op<'a>(input: &'a str, op: &str) -> Option<Vec<&'a str>> {
    let bytes = input.as_bytes();
    let op_bytes = op.as_bytes();
    let mut parts = Vec::new();
    let mut last = 0usize;
    let mut depth_paren = 0i32;
    let mut depth_bracket = 0i32;
    let mut depth_brace = 0i32;
    let mut in_quote: Option<u8> = None;

    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = in_quote {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == q {
                in_quote = None;
            }
            i += 1;
            continue;
        }
        match b {
            b'"' | b'\'' => in_quote = Some(b),
            b'(' => depth_paren += 1,
            b')' => depth_paren = (depth_paren - 1).max(0),
            b'[' => depth_bracket += 1,
            b']' => depth_bracket = (depth_bracket - 1).max(0),
            b'{' => depth_brace += 1,
            b'}' => depth_brace = (depth_brace - 1).max(0),
            _ => {
                if depth_paren == 0
                    && depth_bracket == 0
                    && depth_brace == 0
                    && bytes[i..].starts_with(op_bytes)
                {
                    parts.push(input[last..i].trim());
                    i += op_bytes.len();
                    last = i;
                    continue;
                }
            }
        }
        i += 1;
    }

    if parts.is_empty() {
        None
    } else {
        parts.push(input[last..].trim());
        Some(parts)
    }
}

// ---------------------------------------------------------------------------
// JSONPath Evaluator ($.data, $.data[*].title, $..name, $[0].id)
// ---------------------------------------------------------------------------

#[derive(Debug)]
enum JsonPathStep {
    Field(String),
    RecursiveField(String),
    Index(isize),
    Wildcard,
    Slice(Option<isize>, Option<isize>),
}

pub fn eval_jsonpath(root: &serde_json::Value, path: &str) -> Vec<serde_json::Value> {
    let trimmed = path.trim();
    if trimmed == "$" {
        return vec![root.clone()];
    }
    let Some(rest) = trimmed.strip_prefix('$') else {
        return Vec::new();
    };

    let steps = parse_jsonpath_steps(rest);
    let mut current = vec![root];

    for step in steps {
        let mut next = Vec::new();
        for node in current {
            match &step {
                JsonPathStep::Field(key) => {
                    if let serde_json::Value::Object(map) = node
                        && let Some(child) = map.get(key)
                    {
                        next.push(child);
                    } else if let serde_json::Value::Array(arr) = node {
                        // Legado convenience: `$.list.title` maps over array elements
                        for el in arr {
                            if let serde_json::Value::Object(map) = el
                                && let Some(child) = map.get(key)
                            {
                                next.push(child);
                            }
                        }
                    }
                }
                JsonPathStep::RecursiveField(key) => {
                    collect_recursive_field(node, key, &mut next);
                }
                JsonPathStep::Index(idx) => {
                    if let serde_json::Value::Array(arr) = node {
                        let resolved = if *idx < 0 {
                            arr.len().checked_sub(idx.unsigned_abs())
                        } else {
                            Some(*idx as usize)
                        };
                        if let Some(i) = resolved
                            && let Some(el) = arr.get(i)
                        {
                            next.push(el);
                        }
                    }
                }
                JsonPathStep::Wildcard => match node {
                    serde_json::Value::Array(arr) => next.extend(arr.iter()),
                    serde_json::Value::Object(map) => next.extend(map.values()),
                    _ => {}
                },
                JsonPathStep::Slice(start, end) => {
                    if let serde_json::Value::Array(arr) = node {
                        let len = arr.len() as isize;
                        let s = start
                            .map(|v| if v < 0 { (len + v).max(0) } else { v.min(len) })
                            .unwrap_or(0) as usize;
                        let e = end
                            .map(|v| if v < 0 { (len + v).max(0) } else { v.min(len) })
                            .unwrap_or(len) as usize;
                        if s < e {
                            next.extend(arr[s..e].iter());
                        }
                    }
                }
            }
        }
        current = next;
        if current.is_empty() {
            break;
        }
    }

    current.into_iter().cloned().collect()
}

fn collect_recursive_field<'a>(
    node: &'a serde_json::Value,
    key: &str,
    out: &mut Vec<&'a serde_json::Value>,
) {
    collect_recursive_field_depth(node, key, out, 0);
}

fn collect_recursive_field_depth<'a>(
    node: &'a serde_json::Value,
    key: &str,
    out: &mut Vec<&'a serde_json::Value>,
    depth: usize,
) {
    if depth >= MAX_JSON_RECURSIVE_DEPTH {
        return;
    }
    match node {
        serde_json::Value::Object(map) => {
            if let Some(v) = map.get(key) {
                out.push(v);
            }
            for v in map.values() {
                collect_recursive_field_depth(v, key, out, depth + 1);
            }
        }
        serde_json::Value::Array(arr) => {
            for el in arr {
                collect_recursive_field_depth(el, key, out, depth + 1);
            }
        }
        _ => {}
    }
}

fn parse_jsonpath_steps(mut s: &str) -> Vec<JsonPathStep> {
    let mut steps = Vec::new();
    while !s.is_empty() {
        if let Some(rest) = s.strip_prefix("..") {
            let end = rest.find(['.', '[']).unwrap_or(rest.len());
            let key = &rest[..end];
            if !key.is_empty() {
                steps.push(JsonPathStep::RecursiveField(key.to_string()));
            }
            s = &rest[end..];
        } else if let Some(rest) = s.strip_prefix('.') {
            let end = rest.find(['.', '[']).unwrap_or(rest.len());
            let key = &rest[..end];
            if key == "*" {
                steps.push(JsonPathStep::Wildcard);
            } else if !key.is_empty() {
                steps.push(JsonPathStep::Field(key.to_string()));
            }
            s = &rest[end..];
        } else if let Some(rest) = s.strip_prefix('[') {
            let Some(close) = rest.find(']') else {
                break;
            };
            let inside = rest[..close].trim();
            if inside == "*" {
                steps.push(JsonPathStep::Wildcard);
            } else if (inside.starts_with('\'') && inside.ends_with('\''))
                || (inside.starts_with('"') && inside.ends_with('"'))
            {
                steps.push(JsonPathStep::Field(inside[1..inside.len() - 1].to_string()));
            } else if let Some((a, b)) = inside.split_once(':') {
                let start = a.trim().parse::<isize>().ok();
                let end = b.trim().parse::<isize>().ok();
                steps.push(JsonPathStep::Slice(start, end));
            } else if let Ok(idx) = inside.parse::<isize>() {
                steps.push(JsonPathStep::Index(idx));
            } else if !inside.is_empty() {
                steps.push(JsonPathStep::Field(inside.to_string()));
            }
            s = &rest[close + 1..];
        } else {
            break;
        }
    }
    steps
}

fn json_val_to_string(v: serde_json::Value) -> String {
    match v {
        serde_json::Value::Null => String::new(),
        serde_json::Value::String(s) => s,
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Array(arr) => arr
            .into_iter()
            .map(json_val_to_string)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(", "),
        other => other.to_string(),
    }
}

// ---------------------------------------------------------------------------
// HTML / CSS + Legado Shorthand Evaluator
// ---------------------------------------------------------------------------

fn eval_html_list(html_str: &str, rule: &str) -> Vec<String> {
    let doc = Html::parse_fragment(html_str);
    let clean = rule
        .strip_prefix("@css:")
        .or_else(|| rule.strip_prefix("@CSS:"))
        .unwrap_or(rule)
        .trim();

    let elements = select_elements_chained(&doc, clean);
    elements.into_iter().map(|el| el.html()).collect()
}

fn eval_html_single(html_str: &str, rule: &str, base_url: &str) -> String {
    let doc = Html::parse_fragment(html_str);
    let clean = rule
        .strip_prefix("@css:")
        .or_else(|| rule.strip_prefix("@CSS:"))
        .unwrap_or(rule)
        .trim();

    // Split terminal extractor after last `@`
    let (selector_part, attr_part) = if let Some((left, right)) = clean.rsplit_once('@') {
        if is_attr_extractor(right.trim()) {
            (left.trim(), right.trim())
        } else {
            (clean, "text")
        }
    } else if is_attr_extractor(clean) {
        ("", clean)
    } else {
        (clean, "text")
    };

    let elements: Vec<ElementRef<'_>> = if selector_part.is_empty() {
        doc.root_element()
            .children()
            .filter_map(ElementRef::wrap)
            .collect()
    } else {
        select_elements_chained(&doc, selector_part)
    };

    if elements.is_empty() {
        return String::new();
    }

    let values: Vec<String> = elements
        .into_iter()
        .filter_map(|el| extract_attr_from_element(el, attr_part, base_url))
        .filter(|s| !s.is_empty())
        .collect();

    values.join("\n")
}

fn is_attr_extractor(s: &str) -> bool {
    if s.is_empty() || s.contains(' ') || s.contains('.') || s.contains('#') || s.contains('>') {
        return false;
    }
    let lower = s.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "text"
            | "textnodes"
            | "owntext"
            | "html"
            | "all"
            | "href"
            | "src"
            | "srcset"
            | "content"
            | "title"
            | "alt"
            | "value"
            | "id"
            | "class"
            | "name"
            | "rel"
            | "type"
            | "style"
            | "poster"
            | "action"
            | "target"
            | "placeholder"
            | "datetime"
            | "lang"
            | "property"
    ) || lower.starts_with("data-")
        || lower.starts_with("aria-")
}

fn extract_attr_from_element(el: ElementRef<'_>, attr: &str, base_url: &str) -> Option<String> {
    match attr.to_ascii_lowercase().as_str() {
        "text" => {
            let txt = el.text().collect::<Vec<_>>().join("");
            Some(txt.trim().to_string())
        }
        "textnodes" => {
            let lines: Vec<String> = el
                .text()
                .map(|t| t.trim())
                .filter(|t| !t.is_empty())
                .map(str::to_owned)
                .collect();
            Some(lines.join("\n"))
        }
        "owntext" => {
            let txt: String = el
                .children()
                .filter_map(|c| c.value().as_text().map(|t| t.text.as_ref()))
                .collect::<Vec<_>>()
                .join("");
            Some(txt.trim().to_string())
        }
        "html" => Some(el.inner_html()),
        "all" => Some(el.html()),
        "href" | "src" | "data-src" | "data-original" => el
            .value()
            .attr(attr)
            .map(|v| resolve_relative_url(v, base_url)),
        other => el.value().attr(other).map(|v| v.trim().to_string()),
    }
}

fn select_elements_chained<'a>(doc: &'a Html, chain: &str) -> Vec<ElementRef<'a>> {
    let segments: Vec<&str> = chain
        .split('@')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if segments.is_empty() {
        return Vec::new();
    }

    let mut current: Vec<ElementRef<'a>> = vec![doc.root_element()];

    for seg in segments {
        let mut next = Vec::new();
        let (css_expr, index_filter) = parse_legado_html_segment(seg);
        if css_expr == "__children__" {
            for parent in &current {
                for child in parent.children().filter_map(ElementRef::wrap) {
                    next.push(child);
                }
            }
        } else if let Ok(selector) = Selector::parse(&css_expr) {
            for parent in &current {
                for matched in parent.select(&selector) {
                    next.push(matched);
                }
            }
        }
        current = apply_index_filter(next, index_filter);
        if current.is_empty() {
            break;
        }
    }

    current
}

#[derive(Debug, Clone)]
enum IndexFilter {
    None,
    Single(isize),
    Exclude(Vec<isize>),
    Slice(Option<isize>, Option<isize>),
}

/// Translates Legado shorthand (`class.item.0`, `tag.a.-1`, `id.content`, `children`, `div.list[0:5]`, `tr[!0]`)
/// into a standard CSS selector plus optional index filter.
fn parse_legado_html_segment(seg: &str) -> (String, IndexFilter) {
    let mut s = seg.trim();
    let mut filter = IndexFilter::None;

    // Bracket index/slice suffix: `[0]`, `[-1]`, `[0:5]`, `[!0]`
    if let Some(open) = s.rfind('[')
        && s.ends_with(']')
    {
        let inside = s[open + 1..s.len() - 1].trim();
        let is_index_bracket =
            inside.starts_with('!') || inside.contains(':') || inside.parse::<isize>().is_ok();
        if is_index_bracket {
            if let Some(excl) = inside.strip_prefix('!') {
                let ids: Vec<isize> = excl
                    .split([',', ':'])
                    .filter_map(|p| p.trim().parse::<isize>().ok())
                    .collect();
                filter = IndexFilter::Exclude(ids);
            } else if let Some((a, b)) = inside.split_once(':') {
                filter = IndexFilter::Slice(
                    a.trim().parse::<isize>().ok(),
                    b.trim().parse::<isize>().ok(),
                );
            } else if let Ok(idx) = inside.parse::<isize>() {
                filter = IndexFilter::Single(idx);
            }
            s = s[..open].trim();
        }
    }

    // Legado shorthand prefixes
    if s.eq_ignore_ascii_case("children") {
        return ("__children__".to_string(), filter);
    }

    if let Some(rest) = s.strip_prefix("class.") {
        let (name, idx_opt) = split_trailing_dot_index(rest);
        if let Some(idx) = idx_opt {
            filter = IndexFilter::Single(idx);
        }
        let css = name
            .split_whitespace()
            .filter(|c| !c.is_empty())
            .map(|c| format!(".{c}"))
            .collect::<String>();
        return (css, filter);
    }

    if let Some(rest) = s.strip_prefix("id.") {
        let (name, idx_opt) = split_trailing_dot_index(rest);
        if let Some(idx) = idx_opt {
            filter = IndexFilter::Single(idx);
        }
        return (format!("#{name}"), filter);
    }

    if let Some(rest) = s.strip_prefix("tag.") {
        let (name, idx_opt) = split_trailing_dot_index(rest);
        if let Some(idx) = idx_opt {
            filter = IndexFilter::Single(idx);
        }
        return (name.to_string(), filter);
    }

    // Plain CSS selector with optional trailing `.0` / `.-1` Legado index
    let (clean_css, idx_opt) = split_trailing_dot_index(s);
    if let Some(idx) = idx_opt {
        filter = IndexFilter::Single(idx);
    }
    (clean_css.to_string(), filter)
}

fn split_trailing_dot_index(s: &str) -> (&str, Option<isize>) {
    if let Some((head, tail)) = s.rsplit_once('.')
        && !head.is_empty()
        && let Ok(idx) = tail.parse::<isize>()
    {
        return (head, Some(idx));
    }
    (s, None)
}

fn apply_index_filter<T>(items: Vec<T>, filter: IndexFilter) -> Vec<T> {
    let len = items.len();
    match filter {
        IndexFilter::None => items,
        IndexFilter::Single(idx) => {
            let resolved = if idx < 0 {
                len.checked_sub(idx.unsigned_abs())
            } else {
                Some(idx as usize)
            };
            resolved
                .and_then(|i| items.into_iter().nth(i))
                .into_iter()
                .collect()
        }
        IndexFilter::Exclude(excluded) => {
            let mut bad = std::collections::HashSet::new();
            for idx in excluded {
                let resolved = if idx < 0 {
                    len.checked_sub(idx.unsigned_abs())
                } else {
                    Some(idx as usize)
                };
                if let Some(i) = resolved {
                    bad.insert(i);
                }
            }
            items
                .into_iter()
                .enumerate()
                .filter_map(|(i, v)| (!bad.contains(&i)).then_some(v))
                .collect()
        }
        IndexFilter::Slice(start, end) => {
            let ilen = len as isize;
            let s = start
                .map(|v| {
                    if v < 0 {
                        (ilen + v).max(0)
                    } else {
                        v.min(ilen)
                    }
                })
                .unwrap_or(0) as usize;
            let e = end
                .map(|v| {
                    if v < 0 {
                        (ilen + v).max(0)
                    } else {
                        v.min(ilen)
                    }
                })
                .unwrap_or(ilen) as usize;
            items
                .into_iter()
                .enumerate()
                .filter_map(|(i, v)| (i >= s && i < e).then_some(v))
                .collect()
        }
    }
}

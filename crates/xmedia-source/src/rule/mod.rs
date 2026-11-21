mod html;
mod jsonpath;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use regex::Regex;

pub use self::jsonpath::eval_jsonpath;

use self::html::{eval_html_list, eval_html_single};
use self::jsonpath::json_val_to_string;

const MAX_RULE_REGEX_CACHE: usize = 256;
const MAX_RULE_EVAL_DEPTH: usize = 32;

static RULE_REGEX_CACHE: LazyLock<Mutex<HashMap<String, Regex>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn cached_regex(pattern: &str) -> Option<Regex> {
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
#[derive(Debug, Clone, PartialEq)]
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

/// Decouples `rule` from `rquickjs` / `JsExecutionContext`.
pub trait JsRuleEvaluator {
    fn eval_rule_js(
        &mut self,
        code: &str,
        input: &RuleValue,
        base_url: &str,
    ) -> Result<RuleValue, String>;

    fn check_active(&self) -> Result<(), String> {
        Ok(())
    }
}

/// Evaluates a Legado rule to a single string (supports `<js>`, `@js:`, `{{...}}`, `##regex##rep`, `||`, `&&`, JSONPath, CSS/Legado shorthand).
pub fn eval_rule_string(
    input: &RuleValue,
    rule: &str,
    base_url: &str,
    mut js: Option<&mut dyn JsRuleEvaluator>,
) -> Result<String, String> {
    eval_rule_string_depth(input, rule, base_url, &mut js, 0)
}

fn eval_rule_string_depth(
    input: &RuleValue,
    rule: &str,
    base_url: &str,
    js: &mut Option<&mut dyn JsRuleEvaluator>,
    depth: usize,
) -> Result<String, String> {
    if depth >= MAX_RULE_EVAL_DEPTH {
        return Err(format!("规则递归嵌套深度超过上限 ({MAX_RULE_EVAL_DEPTH})"));
    }
    let trimmed = rule.trim();
    if trimmed.is_empty() {
        return Ok(String::new());
    }

    if let Some((prefix_rule, js_code, suffix_rule)) = split_js_pipeline(trimmed) {
        let stage1 = if prefix_rule.is_empty() {
            input.clone()
        } else {
            let s = eval_rule_string_depth(input, prefix_rule, base_url, js, depth + 1)?;
            RuleValue::from_raw(s)
        };
        let js_out = {
            let js_session = js
                .as_deref_mut()
                .ok_or_else(|| "规则包含 JS 代码，但当前上下文未启用 JS 沙箱".to_string())?;
            js_session.eval_rule_js(js_code, &stage1, base_url)?
        };
        if suffix_rule.is_empty() {
            return Ok(js_out.as_text());
        }
        return eval_rule_string_depth(&js_out, suffix_rule, base_url, js, depth + 1);
    }

    if trimmed.contains("{{") && trimmed.contains("}}") {
        let (template_part, regex_suffix) = split_regex_replace_suffix(trimmed);
        let interpolated = eval_mustache_template(template_part, input, base_url, js)?;
        return Ok(apply_regex_replace(&interpolated, regex_suffix));
    }

    if let Some(parts) = split_top_level_op(trimmed, "||") {
        for part in parts {
            let res = eval_rule_string_depth(input, part, base_url, js, depth + 1)?;
            if !res.trim().is_empty() {
                return Ok(res);
            }
        }
        return Ok(String::new());
    }
    if let Some(parts) = split_top_level_op(trimmed, "&&") {
        let mut out = Vec::new();
        for part in parts {
            let res = eval_rule_string_depth(input, part, base_url, js, depth + 1)?;
            if !res.trim().is_empty() {
                out.push(res);
            }
        }
        return Ok(out.join("\n"));
    }

    let (core_rule, regex_suffix) = split_regex_replace_suffix(trimmed);
    let extracted = eval_non_js_single(input, core_rule, base_url);
    Ok(apply_regex_replace(&extracted, regex_suffix))
}

/// Evaluates a Legado rule to a list of items (used for `bookList`, `chapterList`, etc.).
pub fn eval_rule_list(
    input: &RuleValue,
    rule: &str,
    base_url: &str,
    mut js: Option<&mut dyn JsRuleEvaluator>,
) -> Result<Vec<RuleValue>, String> {
    eval_rule_list_depth(input, rule, base_url, &mut js, 0)
}

fn eval_rule_list_depth(
    input: &RuleValue,
    rule: &str,
    base_url: &str,
    js: &mut Option<&mut dyn JsRuleEvaluator>,
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

    if let Some((prefix_rule, js_code, suffix_rule)) = split_js_pipeline(trimmed) {
        let stage1 = if prefix_rule.is_empty() {
            input.clone()
        } else {
            let s = eval_rule_string_depth(input, prefix_rule, base_url, js, depth + 1)?;
            RuleValue::from_raw(s)
        };
        let js_out = {
            let js_session = js
                .as_deref_mut()
                .ok_or_else(|| "列表规则包含 JS 代码，但当前上下文未启用 JS 沙箱".to_string())?;
            js_session.eval_rule_js(js_code, &stage1, base_url)?
        };
        if suffix_rule.is_empty() {
            return Ok(value_to_list(js_out));
        }
        return eval_rule_list_depth(&js_out, suffix_rule, base_url, js, depth + 1);
    }

    if let Some(parts) = split_top_level_op(trimmed, "||") {
        for part in parts {
            let list = eval_rule_list_depth(input, part, base_url, js, depth + 1)?;
            if !list.is_empty() {
                return Ok(list);
            }
        }
        return Ok(Vec::new());
    }

    if let Some(parts) = split_top_level_op(trimmed, "&&") {
        let mut combined = Vec::new();
        for part in parts {
            combined.extend(eval_rule_list_depth(input, part, base_url, js, depth + 1)?);
        }
        return Ok(combined);
    }

    if let Some(parts) = split_top_level_op(trimmed, "%%") {
        let mut lists = Vec::with_capacity(parts.len());
        let mut max_len = 0usize;
        for part in parts {
            let l = eval_rule_list_depth(input, part, base_url, js, depth + 1)?;
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

    if let RuleValue::Json(serde_json::Value::Object(map)) = input
        && !core.contains('@')
        && !core.contains('.')
        && !core.contains(' ')
        && let Some(v) = map.get(core)
    {
        return apply_regex_replace(&json_val_to_string(v.clone()), regex_suffix);
    }

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

    let html_str = input.as_text();
    let extracted = eval_html_single(&html_str, core, base_url);
    apply_regex_replace(&extracted, regex_suffix)
}

pub fn eval_non_js_list(input: &RuleValue, rule: &str) -> Vec<RuleValue> {
    let trimmed = rule.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

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

fn is_benign_mustache_js_miss(err: &str) -> bool {
    let lower = err.to_ascii_lowercase();
    if err.contains("InternalError:")
        || err.contains("执行已取消")
        || err.contains("执行超过单次操作总超时上限")
        || err.contains("(interrupted)")
        || lower.contains("out of memory")
        || lower.contains("allocation failed")
        || lower.contains("maximum call stack")
        || lower.contains("stack overflow")
    {
        return false;
    }
    err.contains("SyntaxError:") || err.contains("ReferenceError:")
}

fn eval_mustache_template(
    template: &str,
    input: &RuleValue,
    base_url: &str,
    js: &mut Option<&mut dyn JsRuleEvaluator>,
) -> Result<String, String> {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find("{{") {
        if let Some(js_session) = js.as_deref() {
            js_session.check_active()?;
        }
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
            let eval_res = js_session.eval_rule_js(expr, input, base_url);
            js_session.check_active()?;
            match eval_res {
                Ok(v) => v.as_text(),
                Err(err) if is_benign_mustache_js_miss(&err) => {
                    eval_non_js_single(input, expr, base_url)
                }
                Err(err) => return Err(err),
            }
        } else {
            eval_non_js_single(input, expr, base_url)
        };
        out.push_str(&val);
        rest = &after_open[close + 2..];
    }
    if let Some(js_session) = js.as_deref() {
        js_session.check_active()?;
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

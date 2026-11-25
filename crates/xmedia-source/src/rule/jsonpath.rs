const MAX_JSON_RECURSIVE_DEPTH: usize = 128;

#[derive(Debug, Clone, Copy)]
enum FilterNumber {
    Signed(i64),
    Unsigned(u64),
    Float(f64),
}

#[derive(Debug, Clone)]
enum FilterLiteral {
    String(String),
    Number(FilterNumber),
    Bool(bool),
    Null,
}

#[derive(Debug, Clone)]
enum JsonPathFilter {
    Eq {
        path: Vec<String>,
        rhs: FilterLiteral,
    },
    Ne {
        path: Vec<String>,
        rhs: FilterLiteral,
    },
    Truthy {
        path: Vec<String>,
        negated: bool,
    },
}

#[derive(Debug)]
enum JsonPathStep {
    Field(String),
    RecursiveField(String),
    Index(isize),
    Wildcard,
    Slice(Option<isize>, Option<isize>),
    Filter(JsonPathFilter),
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
                JsonPathStep::Filter(filter) => match node {
                    serde_json::Value::Array(arr) => {
                        for el in arr {
                            if eval_filter_on_value(el, filter) {
                                next.push(el);
                            }
                        }
                    }
                    serde_json::Value::Object(map) => {
                        for val in map.values() {
                            if eval_filter_on_value(val, filter) {
                                next.push(val);
                            }
                        }
                    }
                    _ => {}
                },
            }
        }
        current = next;
        if current.is_empty() {
            break;
        }
    }

    current.into_iter().cloned().collect()
}

fn resolve_filter_path<'a>(
    mut node: &'a serde_json::Value,
    path: &[String],
) -> Option<&'a serde_json::Value> {
    for seg in path {
        match node {
            serde_json::Value::Object(map) => {
                node = map.get(seg)?;
            }
            _ => return None,
        }
    }
    Some(node)
}

fn filter_value_equals(lhs: Option<&serde_json::Value>, rhs: &FilterLiteral) -> bool {
    match (lhs, rhs) {
        (None, FilterLiteral::Null) | (Some(serde_json::Value::Null), FilterLiteral::Null) => true,
        (Some(serde_json::Value::String(s)), FilterLiteral::String(r)) => s == r,
        (Some(serde_json::Value::Bool(b)), FilterLiteral::Bool(r)) => b == r,
        (Some(serde_json::Value::Number(n)), FilterLiteral::Number(r)) => {
            if let Some(l) = n.as_i64() {
                match *r {
                    FilterNumber::Signed(ri) => return l == ri,
                    FilterNumber::Unsigned(ru) => return u64::try_from(l).ok() == Some(ru),
                    FilterNumber::Float(_) => {}
                }
            } else if let Some(l) = n.as_u64() {
                match *r {
                    FilterNumber::Signed(ri) => return i64::try_from(l).ok() == Some(ri),
                    FilterNumber::Unsigned(ru) => return l == ru,
                    FilterNumber::Float(_) => {}
                }
            }
            let rhs_f = match *r {
                FilterNumber::Signed(ri) => ri as f64,
                FilterNumber::Unsigned(ru) => ru as f64,
                FilterNumber::Float(rf) => rf,
            };
            n.as_f64().is_some_and(|v| (v - rhs_f).abs() < f64::EPSILON)
        }
        _ => false,
    }
}

fn filter_value_is_truthy(val: Option<&serde_json::Value>) -> bool {
    match val {
        None | Some(serde_json::Value::Null) => false,
        Some(serde_json::Value::Bool(b)) => *b,
        Some(serde_json::Value::Number(n)) => n.as_f64().is_some_and(|v| v != 0.0),
        Some(serde_json::Value::String(s)) => !s.is_empty(),
        Some(serde_json::Value::Array(a)) => !a.is_empty(),
        Some(serde_json::Value::Object(m)) => !m.is_empty(),
    }
}

fn eval_filter_on_value(node: &serde_json::Value, filter: &JsonPathFilter) -> bool {
    match filter {
        JsonPathFilter::Eq { path, rhs } => {
            filter_value_equals(resolve_filter_path(node, path), rhs)
        }
        JsonPathFilter::Ne { path, rhs } => {
            !filter_value_equals(resolve_filter_path(node, path), rhs)
        }
        JsonPathFilter::Truthy { path, negated } => {
            let t = filter_value_is_truthy(resolve_filter_path(node, path));
            if *negated { !t } else { t }
        }
    }
}

fn parse_filter_field_path(raw: &str) -> Option<Vec<String>> {
    let trimmed = raw.trim();
    let rest = trimmed.strip_prefix("@.")?;
    if rest.is_empty() {
        return None;
    }
    let parts: Vec<String> = rest
        .split('.')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect();
    if parts.is_empty() { None } else { Some(parts) }
}

fn unescape_filter_string(inner: &str) -> String {
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('\'') => out.push('\''),
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn parse_filter_literal(raw: &str) -> Option<FilterLiteral> {
    let t = raw.trim();
    if (t.starts_with('\'') && t.ends_with('\'') && t.len() >= 2)
        || (t.starts_with('"') && t.ends_with('"') && t.len() >= 2)
    {
        return Some(FilterLiteral::String(unescape_filter_string(
            &t[1..t.len() - 1],
        )));
    }
    if t == "true" {
        return Some(FilterLiteral::Bool(true));
    }
    if t == "false" {
        return Some(FilterLiteral::Bool(false));
    }
    if t == "null" {
        return Some(FilterLiteral::Null);
    }
    if !t.contains(['.', 'e', 'E']) {
        if let Ok(num) = t.parse::<i64>() {
            return Some(FilterLiteral::Number(FilterNumber::Signed(num)));
        }
        if let Ok(num) = t.parse::<u64>() {
            return Some(FilterLiteral::Number(FilterNumber::Unsigned(num)));
        }
    }
    if let Ok(num) = t.parse::<f64>()
        && num.is_finite()
    {
        return Some(FilterLiteral::Number(FilterNumber::Float(num)));
    }
    None
}

fn parse_jsonpath_filter(expr: &str) -> Option<JsonPathFilter> {
    let mut trimmed = expr.trim();
    while trimmed.starts_with('(') && trimmed.ends_with(')') && trimmed.len() >= 2 {
        trimmed = trimmed[1..trimmed.len() - 1].trim();
    }
    for (op, is_eq) in [("===", true), ("!==", false), ("==", true), ("!=", false)] {
        if let Some((lhs, rhs)) = split_filter_op(trimmed, op)
            && let (Some(path), Some(lit)) =
                (parse_filter_field_path(lhs), parse_filter_literal(rhs))
        {
            return Some(if is_eq {
                JsonPathFilter::Eq { path, rhs: lit }
            } else {
                JsonPathFilter::Ne { path, rhs: lit }
            });
        }
    }
    if let Some(rest) = trimmed.strip_prefix('!')
        && let Some(path) = parse_filter_field_path(rest.trim())
    {
        return Some(JsonPathFilter::Truthy {
            path,
            negated: true,
        });
    }
    if let Some(path) = parse_filter_field_path(trimmed) {
        return Some(JsonPathFilter::Truthy {
            path,
            negated: false,
        });
    }
    None
}

fn split_filter_op<'a>(s: &'a str, op: &str) -> Option<(&'a str, &'a str)> {
    let mut in_single = false;
    let mut in_double = false;
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if in_single {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'\'' {
                in_single = false;
            }
            i += 1;
            continue;
        }
        if in_double {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'"' {
                in_double = false;
            }
            i += 1;
            continue;
        }
        if b == b'\'' {
            in_single = true;
            i += 1;
            continue;
        }
        if b == b'"' {
            in_double = true;
            i += 1;
            continue;
        }
        if bytes[i..].starts_with(op.as_bytes()) {
            return Some((&s[..i], &s[i + op.len()..]));
        }
        i += 1;
    }
    None
}

fn find_matching_bracket(s: &str) -> Option<usize> {
    let mut in_single = false;
    let mut in_double = false;
    let mut depth = 1usize;
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if in_single {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'\'' {
                in_single = false;
            }
        } else if in_double {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'"' {
                in_double = false;
            }
        } else if b == b'\'' {
            in_single = true;
        } else if b == b'"' {
            in_double = true;
        } else if b == b'[' {
            depth += 1;
        } else if b == b']' {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
        i += 1;
    }
    None
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
            let Some(close) = find_matching_bracket(rest) else {
                break;
            };
            let inside = rest[..close].trim();
            if inside == "*" {
                steps.push(JsonPathStep::Wildcard);
            } else if let Some(expr) = inside.strip_prefix('?')
                && let Some(filter) = parse_jsonpath_filter(expr)
            {
                steps.push(JsonPathStep::Filter(filter));
            } else if inside.len() >= 2
                && ((inside.starts_with('\'') && inside.ends_with('\''))
                    || (inside.starts_with('"') && inside.ends_with('"')))
            {
                steps.push(JsonPathStep::Field(inside[1..inside.len() - 1].to_string()));
            } else if inside == "'" || inside == "\"" {
                break;
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

pub(super) fn json_val_to_string(v: serde_json::Value) -> String {
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

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn parse_quoted_bracket_guard_handles_single_quote_and_empty_inside_without_panic() {
        // Directly verify single-character quote tokens inside brackets never slice out of bounds,
        // even if a caller constructs or modifies bracket contents directly.
        for malformed in ["$[']", "$[\"]", "$[]", "$[", "$['unclosed", "$[\"unclosed"] {
            let doc = serde_json::json!({"a": 1, "'": 2, "\"": 3, "": 4});
            let _ = eval_jsonpath(&doc, malformed);
        }

        // Empty quoted field `$['']` and `$[""]` (len == 2) safely resolves key `""`.
        let doc_empty_key = serde_json::json!({"": "empty-key-val", "ok": "yes"});
        assert_eq!(
            eval_jsonpath(&doc_empty_key, "$['']"),
            vec![serde_json::json!("empty-key-val")]
        );
        assert_eq!(
            eval_jsonpath(&doc_empty_key, "$[\"\"]"),
            vec![serde_json::json!("empty-key-val")]
        );
    }
}

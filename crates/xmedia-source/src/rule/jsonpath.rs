const MAX_JSON_RECURSIVE_DEPTH: usize = 128;

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

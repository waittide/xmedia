use serde_json::Value;
use url::Url;
use xmedia_core::domain::error::{DomainError, DomainResult};

use super::url_cookie::{LegadoUrlOptions, resolve_relative_url};

/// Percent-encodes a template component (`{{key}}`, `{{catId}}`, etc.) following RFC 3986
/// unreserved characters so query delimiters (`&`, `=`, `?`, `#`, `/`) and whitespace
/// cannot alter URL path/query structure.
pub fn url_encode_component(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.as_bytes() {
        let is_unreserved =
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~');
        if is_unreserved {
            out.push(*byte as char);
        } else {
            out.push('%');
            out.push(hex_upper(byte >> 4));
            out.push(hex_upper(byte & 0x0F));
        }
    }
    out
}

fn hex_upper(nibble: u8) -> char {
    match nibble {
        0..=9 => (b'0' + nibble) as char,
        10..=15 => (b'A' + (nibble - 10)) as char,
        _ => '0',
    }
}

/// Escapes `input` for safe interpolation inside an existing JSON string literal
/// (without surrounding outer quotes).
pub fn json_escape_str_inner(input: &str) -> String {
    let serialized = serde_json::to_string(input).unwrap_or_else(|_| "\"\"".to_string());
    if serialized.len() >= 2 {
        serialized[1..serialized.len() - 1].to_string()
    } else {
        String::new()
    }
}

fn is_url_encoded_form_body(
    options_obj: &serde_json::Map<String, Value>,
    body_template: &str,
) -> bool {
    if let Some(Value::Object(headers)) = options_obj.get("headers") {
        for (k, v) in headers {
            if k.eq_ignore_ascii_case("content-type")
                && let Some(ct) = v.as_str()
            {
                let lower = ct.to_ascii_lowercase();
                if lower.contains("application/x-www-form-urlencoded") {
                    return true;
                }
                if lower.contains("application/json") || lower.contains("text/plain") {
                    return false;
                }
            }
        }
    }
    // Without an explicit Content-Type, treat key=value form templates (not JSON objects/arrays)
    // as form-urlencoded when they look like query pairs (`a=b&...`).
    let trimmed = body_template.trim_start();
    !trimmed.starts_with('{') && !trimmed.starts_with('[') && trimmed.contains('=')
}

/// Splits a concrete (non-template) URL and an optional trailing Legado `,{"method":...}` options object.
pub fn split_legado_url_and_raw_options(raw: &str) -> (&str, Option<&str>) {
    let trimmed = raw.trim();
    for (idx, ch) in trimmed.char_indices() {
        if ch != ',' {
            continue;
        }
        let after = trimmed[idx + 1..].trim();
        if !after.starts_with('{') || !after.ends_with('}') {
            continue;
        }
        if serde_json::from_str::<LegadoUrlOptions>(after).is_ok()
            || serde_json::from_str::<Value>(after).is_ok_and(|v| v.is_object())
        {
            return (trimmed[..idx].trim_end(), Some(after));
        }
    }
    (trimmed, None)
}

/// Splits a Legado URL template (which may contain `{{...}}` inside the URL and/or options JSON)
/// into `(url_template, Option<options_json_template>)`.
pub fn split_legado_url_template_and_options(template: &str) -> (&str, Option<&str>) {
    let trimmed = template.trim();
    let bytes = trimmed.as_bytes();
    let mut i = 0usize;
    let mut mustache_depth = 0usize;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'{' && bytes[i + 1] == b'{' {
            mustache_depth += 1;
            i += 2;
            continue;
        }
        if mustache_depth > 0 && i + 1 < bytes.len() && bytes[i] == b'}' && bytes[i + 1] == b'}' {
            mustache_depth -= 1;
            i += 2;
            continue;
        }
        if mustache_depth == 0 && bytes[i] == b',' {
            let after = trimmed[i + 1..].trim();
            if after.starts_with('{') && after.ends_with('}') && looks_like_json_template(after) {
                return (trimmed[..i].trim_end(), Some(after));
            }
        }
        i += 1;
    }
    (trimmed, None)
}

fn looks_like_json_template(raw: &str) -> bool {
    if serde_json::from_str::<Value>(raw).is_ok_and(|v| v.is_object()) {
        return true;
    }
    // Replace `{{...}}` placeholders with a safe JSON token (`0`) and retry object parsing
    // in case unquoted placeholders such as `"page":{{page}}` appear.
    let masked = mask_mustache_with_token(raw, "0");
    serde_json::from_str::<Value>(&masked).is_ok_and(|v| v.is_object())
}

fn mask_mustache_with_token(input: &str, token: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(open) = rest.find("{{") {
        out.push_str(&rest[..open]);
        let after_open = &rest[open + 2..];
        let Some(close) = after_open.find("}}") else {
            out.push_str(&rest[open..]);
            return out;
        };
        out.push_str(token);
        rest = &after_open[close + 2..];
    }
    out.push_str(rest);
    out
}

/// Validates and resolves a caller-supplied literal `http://` / `https://` URL.
/// Rejects `data:` URLs, non-HTTP(S) schemes, and any trailing `,{"method":...}` Legado options.
pub fn resolve_literal_http_url(raw: &str, base_url: Option<&str>) -> DomainResult<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(DomainError::Validation("URL 不能为空".to_string()));
    }
    if trimmed
        .get(..5)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("data:"))
    {
        return Err(DomainError::Validation(
            "该接口不接受 data: URL".to_string(),
        ));
    }
    let (_, options_part) = split_legado_url_and_raw_options(trimmed);
    if options_part.is_some() {
        return Err(DomainError::Validation(
            "该接口不接受带 Legado 请求配置选项的 URL".to_string(),
        ));
    }

    let resolved = match base_url.map(str::trim).filter(|b| !b.is_empty()) {
        Some(base) => resolve_relative_url(trimmed, base),
        None => trimmed.to_string(),
    };

    let parsed = Url::parse(&resolved)
        .map_err(|e| DomainError::Validation(format!("URL 格式无效 '{trimmed}': {e}")))?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err(DomainError::Validation(format!(
            "仅支持 http/https URL: '{trimmed}'"
        )));
    }
    Ok(parsed.into())
}

/// Substitutes `{{name}}` placeholders in `url_part` using `vars`:
/// - `raw_passthrough_var`: optional variable name (e.g., `"playUrl"`) that is left unencoded
///   ONLY when the entire trimmed `url_part` equals `{{name}}`.
/// - All other substitutions in `url_part` are percent-encoded with `url_encode_component`.
pub fn substitute_url_part_vars(
    url_part: &str,
    vars: &[(&str, &str)],
    raw_passthrough_var: Option<&str>,
) -> String {
    if let Some(raw_var) = raw_passthrough_var {
        let exact_placeholder = format!("{{{{{raw_var}}}}}");
        if url_part.trim() == exact_placeholder
            && let Some((_, val)) = vars.iter().find(|(k, _)| *k == raw_var)
        {
            return (*val).to_string();
        }
    }
    replace_mustache_vars(url_part, |name| {
        vars.iter()
            .find(|(k, _)| *k == name)
            .map(|(_, val)| url_encode_component(val))
    })
}

/// Substitutes `{{name}}` placeholders inside a `LegadoUrlOptions` JSON template safely:
/// - Parses JSON structure before substitution so placeholder values can never alter `method`,
///   `headers`, `body`, or inject new keys into `LegadoUrlOptions`.
/// - In `body`, form-urlencoded bodies percent-encode placeholder values, and JSON bodies
///   JSON-escape them.
/// - Re-validates the serialized JSON as `LegadoUrlOptions`.
pub fn substitute_legado_options_vars(
    raw_opts: &str,
    vars: &[(&str, &str)],
) -> DomainResult<String> {
    let mut root: Value = serde_json::from_str(raw_opts)
        .map_err(|e| DomainError::Validation(format!("URL 请求配置 JSON 格式无效: {e}")))?;
    let Some(obj) = root.as_object_mut() else {
        return Err(DomainError::Validation(
            "URL 请求配置必须是 JSON 对象".to_string(),
        ));
    };

    let form_body = obj
        .get("body")
        .and_then(Value::as_str)
        .is_some_and(|body_tpl| is_url_encoded_form_body(obj, body_tpl));

    for (key, val) in obj.iter_mut() {
        if key == "body" {
            if let Value::String(s) = val {
                let json_inner = s.trim_start().starts_with('{') || s.trim_start().starts_with('[');
                *s = replace_mustache_vars(s, |name| {
                    vars.iter().find(|(k, _)| *k == name).map(|(_, raw_val)| {
                        if form_body {
                            url_encode_component(raw_val)
                        } else if json_inner {
                            json_escape_str_inner(raw_val)
                        } else {
                            (*raw_val).to_string()
                        }
                    })
                });
            } else {
                substitute_json_value_strings(val, vars);
            }
        } else {
            substitute_json_value_strings(val, vars);
        }
    }

    let serialized = serde_json::to_string(&root)
        .map_err(|e| DomainError::Validation(format!("序列化 URL 请求配置失败: {e}")))?;
    serde_json::from_str::<LegadoUrlOptions>(&serialized)
        .map_err(|e| DomainError::Validation(format!("URL 请求配置校验失败: {e}")))?;
    Ok(serialized)
}

fn substitute_json_value_strings(val: &mut Value, vars: &[(&str, &str)]) {
    match val {
        Value::String(s) => {
            *s = replace_mustache_vars(s, |name| {
                vars.iter()
                    .find(|(k, _)| *k == name)
                    .map(|(_, raw_val)| (*raw_val).to_string())
            });
        }
        Value::Array(items) => {
            for item in items {
                substitute_json_value_strings(item, vars);
            }
        }
        Value::Object(map) => {
            for (_, v) in map.iter_mut() {
                substitute_json_value_strings(v, vars);
            }
        }
        _ => {}
    }
}

fn replace_mustache_vars<F>(template: &str, mut lookup: F) -> String
where
    F: FnMut(&str) -> Option<String>,
{
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find("{{") {
        out.push_str(&rest[..open]);
        let after_open = &rest[open + 2..];
        let Some(close) = after_open.find("}}") else {
            out.push_str(&rest[open..]);
            return out;
        };
        let key = after_open[..close].trim();
        if let Some(replacement) = lookup(key) {
            out.push_str(&replacement);
        } else {
            out.push_str(&rest[open..open + 2 + close + 2]);
        }
        rest = &after_open[close + 2..];
    }
    out.push_str(rest);
    out
}

/// Evaluates `template` with variable substitution while isolating any trailing `,{"method":...}`
/// `LegadoUrlOptions` block from injection and percent-encoding URL query/path components.
pub fn substitute_legado_url_template_vars(
    template: &str,
    vars: &[(&str, &str)],
    raw_passthrough_var: Option<&str>,
) -> DomainResult<String> {
    let (url_part, opt_part) = split_legado_url_template_and_options(template);
    let rendered_url = substitute_url_part_vars(url_part, vars, raw_passthrough_var);
    if let Some(raw_opts) = opt_part {
        let rendered_opts = substitute_legado_options_vars(raw_opts, vars)?;
        Ok(format!("{rendered_url},{rendered_opts}"))
    } else {
        Ok(rendered_url)
    }
}

/// Resolves a relative URL against `base_url` while preserving any trailing `,{"method":...}`
/// Legado options block intact (preventing `Url::join` from percent-encoding option JSON quotes).
pub fn resolve_relative_url_preserving_options(raw_or_rel: &str, base_url: &str) -> String {
    let (url_part, opt_part) = split_legado_url_and_raw_options(raw_or_rel);
    let resolved = resolve_relative_url(url_part, base_url);
    if let Some(opts) = opt_part {
        format!("{resolved},{opts}")
    } else {
        resolved
    }
}

/// Computes a structural signature of a JSON value (preserving object keys, array lengths,
/// and primitive types while ignoring string/number contents).
/// Used to verify that dynamic template interpolation did not inject new JSON keys or alter types.
pub fn json_structure_signature(val: &Value) -> String {
    match val {
        Value::Null => "null".to_string(),
        Value::Bool(_) => "bool".to_string(),
        Value::Number(_) => "num".to_string(),
        Value::String(_) => "str".to_string(),
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(json_structure_signature).collect();
            format!("[{}]", inner.join(","))
        }
        Value::Object(map) => {
            let mut entries: Vec<(&String, &Value)> = map.iter().collect();
            entries.sort_by(|a, b| a.0.cmp(b.0));
            let inner: Vec<String> = entries
                .into_iter()
                .map(|(k, v)| format!("{k:?}:{}", json_structure_signature(v)))
                .collect();
            format!("{{{}}}", inner.join(","))
        }
    }
}

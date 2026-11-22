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
    let mut root: Value = match serde_json::from_str(raw_opts) {
        Ok(v) => v,
        Err(err) => {
            if looks_like_json_template(raw_opts) {
                return eval_json_options_template(
                    raw_opts,
                    vars,
                    |expr| Err(format!("URL 请求配置包含未支持的模板表达式: {expr}")),
                    "URL 请求配置",
                )
                .map_err(DomainError::Validation);
            }
            return Err(DomainError::Validation(format!(
                "URL 请求配置 JSON 格式无效: {err}"
            )));
        }
    };
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

/// Structural token produced by [`json_raw_structure_signature`] without collapsing duplicate keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsonSigToken {
    ObjectStart,
    Key(String),
    ObjectEnd,
    ArrayStart,
    ArrayEnd,
    Scalar,
}

/// Parses raw JSON text into a structural token sequence preserving object key order and duplicates.
/// Returns `None` if `raw` is not valid JSON or contains non-scalar values where scalars are expected.
pub fn json_raw_structure_signature(raw: &str) -> Option<Vec<JsonSigToken>> {
    struct Parser<'a> {
        src: &'a str,
        bytes: &'a [u8],
        pos: usize,
        tokens: Vec<JsonSigToken>,
    }

    impl<'a> Parser<'a> {
        fn skip_ws(&mut self) {
            while self.pos < self.bytes.len()
                && matches!(self.bytes[self.pos], b' ' | b'\t' | b'\n' | b'\r')
            {
                self.pos += 1;
            }
        }

        fn parse_string(&mut self) -> Option<String> {
            if self.bytes.get(self.pos) != Some(&b'"') {
                return None;
            }
            let start = self.pos;
            self.pos += 1;
            while self.pos < self.bytes.len() {
                match self.bytes[self.pos] {
                    b'\\' => {
                        self.pos += 1;
                        let ch = self.src[self.pos..].chars().next()?;
                        self.pos += ch.len_utf8();
                    }
                    b'"' => {
                        self.pos += 1;
                        return serde_json::from_str::<String>(&self.src[start..self.pos]).ok();
                    }
                    _ => {
                        self.pos += 1;
                    }
                }
            }
            None
        }

        fn parse_value(&mut self) -> Option<()> {
            self.skip_ws();
            let b = *self.bytes.get(self.pos)?;
            match b {
                b'{' => {
                    self.pos += 1;
                    self.tokens.push(JsonSigToken::ObjectStart);
                    self.skip_ws();
                    if self.bytes.get(self.pos) == Some(&b'}') {
                        self.pos += 1;
                        self.tokens.push(JsonSigToken::ObjectEnd);
                        return Some(());
                    }
                    loop {
                        self.skip_ws();
                        let key = self.parse_string()?;
                        self.tokens.push(JsonSigToken::Key(key));
                        self.skip_ws();
                        if self.bytes.get(self.pos) != Some(&b':') {
                            return None;
                        }
                        self.pos += 1;
                        self.parse_value()?;
                        self.skip_ws();
                        match self.bytes.get(self.pos)? {
                            b',' => {
                                self.pos += 1;
                            }
                            b'}' => {
                                self.pos += 1;
                                self.tokens.push(JsonSigToken::ObjectEnd);
                                return Some(());
                            }
                            _ => return None,
                        }
                    }
                }
                b'[' => {
                    self.pos += 1;
                    self.tokens.push(JsonSigToken::ArrayStart);
                    self.skip_ws();
                    if self.bytes.get(self.pos) == Some(&b']') {
                        self.pos += 1;
                        self.tokens.push(JsonSigToken::ArrayEnd);
                        return Some(());
                    }
                    loop {
                        self.parse_value()?;
                        self.skip_ws();
                        match self.bytes.get(self.pos)? {
                            b',' => {
                                self.pos += 1;
                            }
                            b']' => {
                                self.pos += 1;
                                self.tokens.push(JsonSigToken::ArrayEnd);
                                return Some(());
                            }
                            _ => return None,
                        }
                    }
                }
                b'"' => {
                    self.parse_string()?;
                    self.tokens.push(JsonSigToken::Scalar);
                    Some(())
                }
                _ => {
                    let start = self.pos;
                    while self.pos < self.bytes.len()
                        && !matches!(
                            self.bytes[self.pos],
                            b' ' | b'\t' | b'\n' | b'\r' | b',' | b'}' | b']'
                        )
                    {
                        self.pos += 1;
                    }
                    let lit = &self.src[start..self.pos];
                    let val = serde_json::from_str::<Value>(lit).ok()?;
                    if val.is_object() || val.is_array() {
                        return None;
                    }
                    self.tokens.push(JsonSigToken::Scalar);
                    Some(())
                }
            }
        }
    }

    let mut parser = Parser {
        src: raw,
        bytes: raw.as_bytes(),
        pos: 0,
        tokens: Vec::new(),
    };
    parser.parse_value()?;
    parser.skip_ws();
    (parser.pos == parser.bytes.len()).then_some(parser.tokens)
}

/// Evaluates a `LegadoUrlOptions` JSON template that may mix static `vars` and JS `{{...}}`
/// expressions (both inside JSON string literals and as unquoted scalar slots like `"webView":{{flag}}`).
///
/// - String-literal interpolations are JSON-escaped via [`json_escape_str_inner`] so values cannot
///   break out of quotes or emit duplicate `"method"` / `"headers"` / `"charset"` keys.
/// - Unquoted interpolations are probed as JSON scalars (never objects/arrays) and verified against
///   [`json_raw_structure_signature`] so `expected_sig` is never skipped and structural mutations
///   are rejected.
pub fn eval_json_options_template<F>(
    raw_opt_json: &str,
    vars: &[(&str, &str)],
    mut eval_js: F,
    label: &str,
) -> Result<String, String>
where
    F: FnMut(&str) -> Result<String, String>,
{
    let (form_body, json_str_body) = {
        let masked = mask_mustache_with_token(raw_opt_json, "0");
        serde_json::from_str::<Value>(&masked)
            .ok()
            .and_then(|v| {
                v.as_object().and_then(|obj| {
                    obj.get("body").and_then(Value::as_str).map(|body_tpl| {
                        let is_form = is_url_encoded_form_body(obj, body_tpl);
                        let trimmed_body = body_tpl.trim_start();
                        let is_json_str = !is_form
                            && (trimmed_body.starts_with('{') || trimmed_body.starts_with('['));
                        (is_form, is_json_str)
                    })
                })
            })
            .unwrap_or((false, false))
    };

    let bytes = raw_opt_json.as_bytes();
    let mut eval_opt = String::with_capacity(raw_opt_json.len() + 32);
    let mut baseline_opt = String::with_capacity(raw_opt_json.len() + 32);
    let mut in_string = false;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut top_key_expected = false;
    let mut reading_top_key = false;
    let mut top_key_start = 0usize;
    let mut current_top_key: Option<String> = None;
    let mut pos = 0usize;

    while pos < bytes.len() {
        if in_string && bytes[pos] == b'\\' {
            eval_opt.push('\\');
            baseline_opt.push('\\');
            pos += 1;
            if let Some(ch) = raw_opt_json[pos..].chars().next() {
                eval_opt.push(ch);
                baseline_opt.push(ch);
                pos += ch.len_utf8();
            }
            continue;
        }
        if raw_opt_json[pos..].starts_with("{{") {
            let after_open = &raw_opt_json[pos + 2..];
            if let Some(close) = after_open.find("}}") {
                let expr = after_open[..close].trim();
                baseline_opt.push_str(if in_string {
                    "_XMEDIA_PROBE_"
                } else {
                    "\"_XMEDIA_PROBE_\""
                });

                let in_body_string = in_string
                    && !reading_top_key
                    && brace_depth == 1
                    && bracket_depth == 0
                    && current_top_key.as_deref() == Some("body");
                let in_form_body_string = form_body && in_body_string;
                let in_json_str_body = json_str_body && in_body_string;

                let raw_val = if let Some((_, static_val)) = vars.iter().find(|(k, _)| *k == expr) {
                    if in_form_body_string {
                        url_encode_component(static_val)
                    } else {
                        (*static_val).to_string()
                    }
                } else {
                    eval_js(&format!("{{{{{expr}}}}}"))?
                };

                if in_string {
                    let escaped = if in_json_str_body {
                        json_escape_str_inner(&json_escape_str_inner(&raw_val))
                    } else {
                        json_escape_str_inner(&raw_val)
                    };
                    eval_opt.push_str(&escaped);
                } else {
                    let trimmed_val = raw_val.trim();
                    if let Ok(v) = serde_json::from_str::<Value>(trimmed_val)
                        && !v.is_object()
                        && !v.is_array()
                    {
                        eval_opt.push_str(trimmed_val);
                    } else {
                        let serialized =
                            serde_json::to_string(&raw_val).unwrap_or_else(|_| "\"\"".to_string());
                        eval_opt.push_str(&serialized);
                    }
                }
                pos += 2 + close + 2;
                continue;
            }
        }
        if bytes[pos] == b'"' {
            if !in_string {
                in_string = true;
                if brace_depth == 1 && bracket_depth == 0 && top_key_expected {
                    reading_top_key = true;
                    top_key_start = pos;
                }
            } else {
                in_string = false;
                if reading_top_key {
                    reading_top_key = false;
                    current_top_key =
                        serde_json::from_str::<String>(&raw_opt_json[top_key_start..=pos]).ok();
                }
            }
            eval_opt.push('"');
            baseline_opt.push('"');
            pos += 1;
            continue;
        }
        if !in_string {
            match bytes[pos] {
                b'{' => {
                    if brace_depth == 0 && bracket_depth == 0 {
                        top_key_expected = true;
                    }
                    brace_depth += 1;
                }
                b'}' => {
                    brace_depth = brace_depth.saturating_sub(1);
                    if brace_depth == 0 {
                        top_key_expected = false;
                        current_top_key = None;
                    }
                }
                b'[' => {
                    bracket_depth += 1;
                }
                b']' => {
                    bracket_depth = bracket_depth.saturating_sub(1);
                }
                b',' if brace_depth == 1 && bracket_depth == 0 => {
                    top_key_expected = true;
                    current_top_key = None;
                }
                b':' if brace_depth == 1 && bracket_depth == 0 => {
                    top_key_expected = false;
                }
                _ => {}
            }
        }
        if let Some(ch) = raw_opt_json[pos..].chars().next() {
            eval_opt.push(ch);
            baseline_opt.push(ch);
            pos += ch.len_utf8();
        } else {
            break;
        }
    }

    let expected_sig = json_raw_structure_signature(&baseline_opt);
    let actual_sig = json_raw_structure_signature(&eval_opt);
    if expected_sig.is_none()
        || actual_sig != expected_sig
        || serde_json::from_str::<LegadoUrlOptions>(&eval_opt).is_err()
    {
        return Err(format!("{label} 请求选项 JSON 结构无效或不允许被参数修改"));
    }
    Ok(eval_opt)
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

/// Returns `true` when any non-empty caller-controlled template variable contains characters
/// capable of injecting or mutating a Legado `,{"method":...}` JSON options block.
pub fn has_suspicious_option_chars(values: &[&str]) -> bool {
    values.iter().any(|s| {
        !s.is_empty()
            && s.chars().any(|c| {
                matches!(c, ',' | '{' | '}' | '[' | ']' | '"' | '\'' | '\\' | ';') || c.is_control()
            })
    })
}

/// Counts occurrences of `,` followed by optional whitespace and `{` in `s`.
pub fn count_comma_open_brace(s: &str) -> usize {
    s.bytes()
        .enumerate()
        .filter(|&(i, b)| b == b',' && s[i + 1..].trim_start().starts_with('{'))
        .count()
}

/// Compares the Legado `,{"method":...}` options structure produced by a `@js:`/`<js>` URL
/// pipeline against a baseline `probe_out` evaluated with benign probe variables.
/// Rejects both newly injected option blocks and structural mutations to existing option JSON.
pub fn verify_js_pipeline_url_options(
    eval_out: &str,
    probe_out: &str,
    label: &str,
) -> Result<(), String> {
    let (eval_url_part, Some(eval_opt_json)) = split_legado_url_and_raw_options(eval_out.trim())
    else {
        return Ok(());
    };
    let (probe_url_part, Some(probe_opt_json)) = split_legado_url_and_raw_options(probe_out.trim())
    else {
        return Err(format!("{label} 模板参数不允许附加请求选项"));
    };
    if (split_legado_url_and_raw_options(probe_url_part.trim())
        .1
        .is_none()
        && split_legado_url_and_raw_options(eval_url_part.trim())
            .1
            .is_some())
        || count_comma_open_brace(eval_url_part) != count_comma_open_brace(probe_url_part)
    {
        return Err(format!("{label} 模板参数不允许附加请求选项"));
    }
    let expected_sig = json_raw_structure_signature(probe_opt_json);
    let actual_sig = json_raw_structure_signature(eval_opt_json);
    if expected_sig.is_none()
        || actual_sig != expected_sig
        || serde_json::from_str::<LegadoUrlOptions>(eval_opt_json).is_err()
    {
        return Err(format!("{label} 模板参数不允许修改请求选项结构"));
    }
    Ok(())
}

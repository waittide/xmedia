use std::collections::HashMap;
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};

pub fn de_opt_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let val = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(match val {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(s)) => {
            if s.is_empty() {
                None
            } else {
                Some(s)
            }
        }
        Some(serde_json::Value::Number(n)) => Some(n.to_string()),
        Some(serde_json::Value::Bool(b)) => Some(b.to_string()),
        Some(other) => Some(other.to_string()),
    })
}

pub fn de_string_or_default<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(de_opt_string(deserializer)?.unwrap_or_default())
}

pub fn de_bool_true<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    let val = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(match val {
        Some(serde_json::Value::Bool(b)) => b,
        Some(serde_json::Value::Number(n)) => n.as_i64().unwrap_or(1) != 0,
        Some(serde_json::Value::String(s)) => {
            !matches!(s.trim().to_ascii_lowercase().as_str(), "false" | "0" | "")
        }
        _ => true,
    })
}

pub fn de_i64_zero<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: Deserializer<'de>,
{
    let val = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(match val {
        Some(serde_json::Value::Number(n)) => n.as_i64().unwrap_or(0),
        Some(serde_json::Value::String(s)) => s.trim().parse::<i64>().unwrap_or(0),
        _ => 0,
    })
}

pub fn default_true() -> bool {
    true
}

/// Parsed Legado `concurrentRate` policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConcurrentRateSpec {
    pub max_concurrency: usize,
    pub window_requests: usize,
    pub window_ms: u64,
}

/// Parses `concurrentRate` into a rate-limit specification.
///
/// Supports both `"requests/millis"` (e.g. `"10/1000"`) and pure millisecond
/// interval (e.g. `"200"`, meaning 1 request per 200ms). Returns `None` when
/// unset, `"0"`, or invalid.
pub fn parse_concurrent_rate(concurrent_rate: Option<&str>) -> Option<ConcurrentRateSpec> {
    let raw = concurrent_rate?.trim();
    if raw.is_empty() || raw == "0" {
        return None;
    }
    if let Some((req_part, ms_part)) = raw.split_once('/') {
        let max_requests = req_part.trim().parse::<usize>().ok()?;
        let window_ms = ms_part.trim().parse::<u64>().ok()?;
        if max_requests == 0 || window_ms == 0 {
            return None;
        }
        return Some(ConcurrentRateSpec {
            max_concurrency: max_requests.clamp(1, 16),
            window_requests: max_requests,
            window_ms,
        });
    }
    let interval_ms = raw.parse::<u64>().ok()?;
    if interval_ms == 0 {
        return None;
    }
    Some(ConcurrentRateSpec {
        max_concurrency: 1,
        window_requests: 1,
        window_ms: interval_ms,
    })
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LoginUiStyle {
    #[serde(default)]
    pub layout_flex_grow: Option<f64>,
    #[serde(default)]
    pub layout_flex_basis_percent: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LoginUiItem {
    #[serde(default)]
    pub name: String,
    #[serde(rename = "type", default)]
    pub item_type: String,
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default)]
    pub style: Option<LoginUiStyle>,
}

/// Parses a `loginUi` JSON array string into `Vec<LoginUiItem>`, returning empty on invalid/blank input.
pub fn parse_login_ui_json(raw: Option<&str>) -> Vec<LoginUiItem> {
    let Some(raw) = raw else {
        return Vec::new();
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    serde_json::from_str::<Vec<LoginUiItem>>(trimmed).unwrap_or_default()
}

/// Persisted plugin source format (`json_rule` for Legado/JSON rules, `js_module` for Plugin v2 JS).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginFormat {
    #[default]
    JsonRule,
    JsModule,
}

impl PluginFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::JsonRule => "json_rule",
            Self::JsModule => "js_module",
        }
    }

    pub fn from_db_str(raw: &str) -> Self {
        match raw.trim() {
            "js_module" | "js" => Self::JsModule,
            _ => Self::JsonRule,
        }
    }
}

/// Persisted per-plugin runtime state (variables, login form values, cookies, cache, settings).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceRuntimeState {
    #[serde(default)]
    pub variable: String,
    #[serde(default)]
    pub login_info: HashMap<String, String>,
    #[serde(default)]
    pub cookies: HashMap<String, String>,
    #[serde(default)]
    pub cache: HashMap<String, String>,
    #[serde(default)]
    pub settings: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub android_id: String,
}

impl SourceRuntimeState {
    pub fn ensure_android_id(&mut self, seed: &str) {
        if self.android_id.trim().is_empty() {
            use md5::{Digest, Md5};
            let mut hasher = Md5::new();
            hasher.update(b"xmedia-legado-device:");
            hasher.update(seed.as_bytes());
            let digest = hex::encode(hasher.finalize());
            self.android_id = digest[..16].to_string();
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BrowserRedirectHint {
    pub url: String,
    pub title: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExecutionDiagnostics {
    pub toasts: Vec<String>,
    pub logs: Vec<String>,
    pub browser_urls: Vec<BrowserRedirectHint>,
}

#[derive(Debug, Clone)]
pub struct SourceCallOutput<T> {
    pub data: T,
    pub state: SourceRuntimeState,
    pub diagnostics: ExecutionDiagnostics,
}

#[derive(Debug, Clone)]
pub struct SourceCallError {
    pub message: String,
    pub diagnostics: ExecutionDiagnostics,
}

impl SourceCallError {
    pub fn new(message: impl Into<String>, diagnostics: ExecutionDiagnostics) -> Self {
        Self {
            message: message.into(),
            diagnostics,
        }
    }
}

impl fmt::Display for SourceCallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for SourceCallError {}

impl From<SourceCallError> for String {
    fn from(value: SourceCallError) -> Self {
        value.message
    }
}

/// Domain-agnostic source metadata needed by JS `source.*` bridge and `jsLib`.
#[derive(Debug, Clone, Default)]
pub struct SourceMetadata {
    pub source_name: String,
    pub source_url: String,
    pub header: Option<String>,
    pub login_url: Option<String>,
    pub login_ui: Option<String>,
    pub js_lib: Option<String>,
    pub enabled_cookie_jar: bool,
    pub raw_json: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Deserialize)]
    struct LenientSample {
        #[serde(default, deserialize_with = "de_opt_string")]
        opt_s: Option<String>,
        #[serde(default, deserialize_with = "de_string_or_default")]
        req_s: String,
        #[serde(default = "default_true", deserialize_with = "de_bool_true")]
        flag: bool,
        #[serde(default, deserialize_with = "de_i64_zero")]
        num: i64,
    }

    #[test]
    fn parse_concurrent_rate_supports_ratio_and_interval_formats() {
        assert_eq!(
            parse_concurrent_rate(Some("10/1000")),
            Some(ConcurrentRateSpec {
                max_concurrency: 10,
                window_requests: 10,
                window_ms: 1000,
            })
        );
        assert_eq!(
            parse_concurrent_rate(Some("200")),
            Some(ConcurrentRateSpec {
                max_concurrency: 1,
                window_requests: 1,
                window_ms: 200,
            })
        );
        assert_eq!(parse_concurrent_rate(Some("0")), None);
        assert_eq!(parse_concurrent_rate(Some("0/1000")), None);
        assert_eq!(parse_concurrent_rate(Some("10/0")), None);
        assert_eq!(parse_concurrent_rate(None), None);
        assert_eq!(
            parse_concurrent_rate(Some("50/1000")),
            Some(ConcurrentRateSpec {
                max_concurrency: 16,
                window_requests: 50,
                window_ms: 1000,
            })
        );
    }

    #[test]
    fn lenient_deserializers_handle_mixed_json_types() {
        let parsed: LenientSample =
            serde_json::from_str(r#"{"opt_s": 123, "req_s": true, "flag": "false", "num": "42"}"#)
                .unwrap();
        assert_eq!(parsed.opt_s.as_deref(), Some("123"));
        assert_eq!(parsed.req_s, "true");
        assert!(!parsed.flag);
        assert_eq!(parsed.num, 42);

        let empty_str: LenientSample =
            serde_json::from_str(r#"{"opt_s": "", "req_s": null, "flag": 1, "num": "bad"}"#)
                .unwrap();
        assert_eq!(empty_str.opt_s, None);
        assert_eq!(empty_str.req_s, "");
        assert!(empty_str.flag);
        assert_eq!(empty_str.num, 0);

        let defaults: LenientSample = serde_json::from_str(r#"{}"#).unwrap();
        assert_eq!(defaults.opt_s, None);
        assert_eq!(defaults.req_s, "");
        assert!(defaults.flag);
        assert_eq!(defaults.num, 0);
    }

    #[test]
    fn parse_login_ui_json_handles_valid_empty_and_invalid() {
        assert!(parse_login_ui_json(None).is_empty());
        assert!(parse_login_ui_json(Some("   ")).is_empty());
        assert!(parse_login_ui_json(Some("{not-an-array")).is_empty());

        let items = parse_login_ui_json(Some(
            r#"[{"name":"username","type":"text"},{"name":"pwd","type":"password","action":"login()"}]"#,
        ));
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].name, "username");
        assert_eq!(items[1].item_type, "password");
        assert_eq!(items[1].action.as_deref(), Some("login()"));
    }

    #[test]
    fn ensure_android_id_is_deterministic_and_non_overwriting() {
        let mut s1 = SourceRuntimeState::default();
        s1.ensure_android_id("https://source.example.com");
        assert_eq!(s1.android_id.len(), 16);
        assert!(s1.android_id.chars().all(|c| c.is_ascii_hexdigit()));

        let mut s2 = SourceRuntimeState::default();
        s2.ensure_android_id("https://source.example.com");
        assert_eq!(s1.android_id, s2.android_id);

        let expected = s1.android_id.clone();
        s1.ensure_android_id("https://other.example.com");
        assert_eq!(s1.android_id, expected);
    }
}

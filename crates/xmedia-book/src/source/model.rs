use std::collections::HashMap;

use serde::{Deserialize, Deserializer, Serialize};

fn de_opt_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
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

fn de_string_or_default<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(de_opt_string(deserializer)?.unwrap_or_default())
}

fn de_bool_true<'de, D>(deserializer: D) -> Result<bool, D::Error>
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

fn de_i64_zero<'de, D>(deserializer: D) -> Result<i64, D::Error>
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

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegadoSearchRule {
    #[serde(default, deserialize_with = "de_opt_string")]
    pub book_list: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub author: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub book_url: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub cover_url: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub intro: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub kind: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub last_chapter: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub word_count: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub check_key_word: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegadoBookInfoRule {
    #[serde(default, deserialize_with = "de_opt_string")]
    pub init: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub author: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub cover_url: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub intro: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub kind: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub last_chapter: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub word_count: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub toc_url: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub can_re_name: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegadoTocRule {
    #[serde(default, deserialize_with = "de_opt_string")]
    pub pre_update_js: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub chapter_list: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub chapter_name: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub chapter_url: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub next_toc_url: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub update_time: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub is_volume: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub is_vip: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegadoContentRule {
    #[serde(default, deserialize_with = "de_opt_string")]
    pub content: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub title: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub next_content_url: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub replace_regex: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub source_regex: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub web_js: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegadoBookSource {
    #[serde(default, deserialize_with = "de_string_or_default")]
    pub book_source_name: String,
    #[serde(default, deserialize_with = "de_string_or_default")]
    pub book_source_url: String,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub book_source_group: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub book_source_comment: Option<String>,
    #[serde(default, deserialize_with = "de_i64_zero")]
    pub book_source_type: i64,
    #[serde(default = "default_true", deserialize_with = "de_bool_true")]
    pub enabled: bool,
    #[serde(default = "default_true", deserialize_with = "de_bool_true")]
    pub enabled_explore: bool,
    #[serde(default = "default_true", deserialize_with = "de_bool_true")]
    pub enabled_cookie_jar: bool,
    #[serde(default, deserialize_with = "de_i64_zero")]
    pub weight: i64,
    #[serde(default, deserialize_with = "de_i64_zero")]
    pub custom_order: i64,
    #[serde(default, deserialize_with = "de_i64_zero")]
    pub respond_time: i64,
    #[serde(default, deserialize_with = "de_i64_zero")]
    pub last_update_time: i64,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub header: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub search_url: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub explore_url: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub book_url_pattern: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub login_url: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub login_ui: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub login_check_js: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub js_lib: Option<String>,
    #[serde(default)]
    pub rule_search: Option<LegadoSearchRule>,
    #[serde(default)]
    pub rule_explore: Option<LegadoSearchRule>,
    #[serde(default)]
    pub rule_book_info: Option<LegadoBookInfoRule>,
    #[serde(default)]
    pub rule_toc: Option<LegadoTocRule>,
    #[serde(default)]
    pub rule_content: Option<LegadoContentRule>,
}

impl LegadoBookSource {
    /// Parses either a single Legado JSON object or a JSON array of book sources.
    pub fn parse_json_batch(raw: &str) -> Result<Vec<(Self, String)>, String> {
        let trimmed = raw.trim().trim_start_matches('\u{feff}');
        if trimmed.is_empty() {
            return Err("书源 JSON 内容为空".into());
        }
        let val: serde_json::Value =
            serde_json::from_str(trimmed).map_err(|e| format!("书源 JSON 解析失败: {e}"))?;
        let items = match val {
            serde_json::Value::Array(arr) => arr,
            obj @ serde_json::Value::Object(_) => vec![obj],
            _ => return Err("书源 JSON 必须是对象或对象数组".into()),
        };

        let mut out = Vec::with_capacity(items.len());
        for (idx, item) in items.into_iter().enumerate() {
            let item_raw = serde_json::to_string(&item)
                .map_err(|e| format!("第 {} 条书源序列化失败: {e}", idx + 1))?;
            let parsed: Self = serde_json::from_value(item)
                .map_err(|e| format!("第 {} 条书源结构解析失败: {e}", idx + 1))?;
            if parsed.book_source_name.trim().is_empty() && parsed.book_source_url.trim().is_empty()
            {
                continue;
            }
            out.push((parsed, item_raw));
        }
        if out.is_empty() {
            return Err("未解析到有效书源（缺少 bookSourceName / bookSourceUrl）".into());
        }
        Ok(out)
    }

    pub fn parse_login_ui(&self) -> Vec<LoginUiItem> {
        let Some(ref raw) = self.login_ui else {
            return Vec::new();
        };
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Vec::new();
        }
        serde_json::from_str::<Vec<LoginUiItem>>(trimmed).unwrap_or_default()
    }
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
    #[serde(default)]
    pub style: Option<LoginUiStyle>,
}

/// Persisted per-source runtime state (variables, login form values, cookies, cache).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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

/// Context about the current book passed to JS rules (`book` global object).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BookJsContext {
    pub name: String,
    pub author: String,
    pub book_url: String,
    pub toc_url: String,
    pub cover_url: Option<String>,
    pub intro: Option<String>,
    pub dur_chapter_index: usize,
    pub dur_chapter_title: String,
    pub custom_variable: String,
}

/// Context about the current chapter passed to JS rules (`chapter` global object).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ChapterJsContext {
    pub index: usize,
    pub title: String,
    pub url: String,
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchBookResult {
    pub name: String,
    pub author: String,
    pub book_url: String,
    pub cover_url: Option<String>,
    pub intro: Option<String>,
    pub kind: Option<String>,
    pub last_chapter: Option<String>,
    pub word_count: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OnlineBookDetail {
    pub name: String,
    pub author: String,
    pub book_url: String,
    pub toc_url: String,
    pub cover_url: Option<String>,
    pub intro: Option<String>,
    pub kind: Option<String>,
    pub last_chapter: Option<String>,
    pub word_count: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OnlineChapterItem {
    pub index: usize,
    pub title: String,
    pub url: String,
    pub update_time: Option<String>,
    pub is_volume: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OnlineChapterContent {
    pub title: String,
    pub content: String,
    pub raw_html: String,
}

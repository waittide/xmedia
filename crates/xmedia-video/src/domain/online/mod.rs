pub mod types;

use std::borrow::Cow;
use std::collections::HashMap;

use serde::{Deserialize, Deserializer, Serialize};
use xmedia_source::crud::{ParsedSourceSummaryMeta, SourceSpecAdapter};
pub use xmedia_source::http::proxy::has_origin_affinity;
pub use xmedia_source::model::{
    ConcurrentRateSpec, ExecutionDiagnostics, LoginUiItem, SourceCallError, SourceCallOutput,
    SourceMetadata, SourceRuntimeState, de_bool_true, de_i64_zero, de_opt_string,
    de_string_or_default, default_true, parse_concurrent_rate, parse_login_ui_json,
};

pub use types::*;

pub fn de_string_vec_lenient<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let val = Option::<serde_json::Value>::deserialize(deserializer)?;
    let Some(v) = val else {
        return Ok(Vec::new());
    };
    match v {
        serde_json::Value::Null => Ok(Vec::new()),
        serde_json::Value::Array(arr) => Ok(arr
            .into_iter()
            .filter_map(|item| match item {
                serde_json::Value::String(s) => {
                    let t = s.trim().to_string();
                    (!t.is_empty()).then_some(t)
                }
                serde_json::Value::Number(n) => Some(n.to_string()),
                _ => None,
            })
            .collect()),
        serde_json::Value::String(s) => Ok(s
            .split([',', '，', '|'])
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .map(ToString::to_string)
            .collect()),
        _ => Ok(Vec::new()),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoSourceFormat {
    #[default]
    #[serde(alias = "applecms", alias = "applecmsV10")]
    ApplecmsV10,
    Rule,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoCategoriesRule {
    #[serde(default, alias = "bookList", deserialize_with = "de_opt_string")]
    pub list: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub id: Option<String>,
    #[serde(default, alias = "title", deserialize_with = "de_opt_string")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoSearchRule {
    #[serde(default, alias = "bookList", deserialize_with = "de_opt_string")]
    pub list: Option<String>,
    #[serde(default, alias = "id", deserialize_with = "de_opt_string")]
    pub vod_id: Option<String>,
    #[serde(default, alias = "title", deserialize_with = "de_opt_string")]
    pub name: Option<String>,
    #[serde(default, alias = "posterUrl", deserialize_with = "de_opt_string")]
    pub cover_url: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub remarks: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub year: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub area: Option<String>,
    #[serde(default, alias = "category", deserialize_with = "de_opt_string")]
    pub type_name: Option<String>,
    #[serde(default, alias = "bookUrl", deserialize_with = "de_opt_string")]
    pub detail_url: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoDetailRule {
    #[serde(default, deserialize_with = "de_opt_string")]
    pub request_url: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub init: Option<String>,
    #[serde(default, alias = "title", deserialize_with = "de_opt_string")]
    pub name: Option<String>,
    #[serde(default, alias = "posterUrl", deserialize_with = "de_opt_string")]
    pub cover_url: Option<String>,
    #[serde(default, alias = "description", deserialize_with = "de_opt_string")]
    pub intro: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub actors: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub director: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub year: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub area: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub remarks: Option<String>,
    #[serde(default, alias = "groups", deserialize_with = "de_opt_string")]
    pub play_groups: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub group_name: Option<String>,
    #[serde(default, alias = "chapterList", deserialize_with = "de_opt_string")]
    pub episodes: Option<String>,
    #[serde(default, alias = "chapterName", deserialize_with = "de_opt_string")]
    pub episode_name: Option<String>,
    #[serde(default, alias = "chapterUrl", deserialize_with = "de_opt_string")]
    pub episode_url: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub play_url: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoPlayRule {
    #[serde(default, deserialize_with = "de_opt_string")]
    pub request_url: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub stream_url: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub stream_type: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub headers: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub subtitles: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoSourceSpec {
    #[serde(
        default,
        alias = "name",
        alias = "bookSourceName",
        deserialize_with = "de_string_or_default"
    )]
    pub source_name: String,
    #[serde(
        default,
        alias = "url",
        alias = "bookSourceUrl",
        deserialize_with = "de_string_or_default"
    )]
    pub source_url: String,
    #[serde(
        default,
        alias = "group",
        alias = "bookSourceGroup",
        deserialize_with = "de_opt_string"
    )]
    pub source_group: Option<String>,
    #[serde(
        default,
        alias = "comment",
        alias = "bookSourceComment",
        deserialize_with = "de_opt_string"
    )]
    pub source_comment: Option<String>,
    #[serde(default, alias = "mode")]
    pub source_format: Option<VideoSourceFormat>,
    #[serde(default = "default_true", deserialize_with = "de_bool_true")]
    pub enabled: bool,
    #[serde(default = "default_true", deserialize_with = "de_bool_true")]
    pub enabled_explore: bool,
    #[serde(default = "default_true", deserialize_with = "de_bool_true")]
    pub enabled_cookie_jar: bool,
    #[serde(default = "default_true", deserialize_with = "de_bool_true")]
    pub searchable: bool,
    #[serde(default, deserialize_with = "de_i64_zero")]
    pub weight: i64,
    #[serde(default, deserialize_with = "de_i64_zero")]
    pub custom_order: i64,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub header: Option<String>,
    #[serde(default, alias = "streamHeader", deserialize_with = "de_opt_string")]
    pub media_header: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub concurrent_rate: Option<String>,
    #[serde(default, alias = "checkKeyWord", deserialize_with = "de_opt_string")]
    pub check_keyword: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub login_url: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub login_ui: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub login_check_js: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub js_lib: Option<String>,

    // AppleCMS v10 mode fields
    #[serde(default, alias = "api_url", deserialize_with = "de_opt_string")]
    pub api_url: Option<String>,
    #[serde(default, deserialize_with = "de_string_vec_lenient")]
    pub preferred_play_from: Vec<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub play_url_filter_regex: Option<String>,

    // Generic Rule mode fields
    #[serde(default, deserialize_with = "de_opt_string")]
    pub search_url: Option<String>,
    #[serde(default, alias = "browseUrl", deserialize_with = "de_opt_string")]
    pub explore_url: Option<String>,
    #[serde(default, alias = "categoryUrl", deserialize_with = "de_opt_string")]
    pub categories_url: Option<String>,
    #[serde(default)]
    pub rule_categories: Option<VideoCategoriesRule>,
    #[serde(default)]
    pub rule_search: Option<VideoSearchRule>,
    #[serde(default)]
    pub rule_explore: Option<VideoSearchRule>,
    #[serde(default)]
    pub rule_detail: Option<VideoDetailRule>,
    #[serde(default)]
    pub rule_play: Option<VideoPlayRule>,
}

impl VideoSourceSpec {
    pub const DEFAULT_PLAY_FILTER_REGEX: &str = r"(?i)\.(m3u8|mp4|mkv|webm)(\?|$)";

    pub fn effective_source_url(&self) -> &str {
        let primary = self.source_url.trim();
        if !primary.is_empty() {
            return primary;
        }
        self.api_url.as_deref().unwrap_or("").trim()
    }

    pub fn effective_api_url(&self) -> &str {
        if let Some(ref api) = self.api_url {
            let t = api.trim();
            if !t.is_empty() {
                return t;
            }
        }
        self.source_url.trim()
    }

    pub fn effective_format(&self) -> VideoSourceFormat {
        if let Some(fmt) = self.source_format {
            return fmt;
        }
        let has_api_url = self
            .api_url
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty());
        if has_api_url && self.rule_search.is_none() {
            VideoSourceFormat::ApplecmsV10
        } else {
            VideoSourceFormat::Rule
        }
    }

    pub fn source_type_code(&self) -> i64 {
        match self.effective_format() {
            VideoSourceFormat::ApplecmsV10 => 0,
            VideoSourceFormat::Rule => 1,
        }
    }

    pub fn effective_media_header(&self) -> Option<&str> {
        self.media_header
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .or_else(|| {
                self.header
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
            })
    }

    pub fn effective_play_filter_regex(&self) -> &str {
        self.play_url_filter_regex
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(Self::DEFAULT_PLAY_FILTER_REGEX)
    }

    pub fn parse_login_ui(&self) -> Vec<LoginUiItem> {
        parse_login_ui_json(self.login_ui.as_deref())
    }

    pub fn parse_concurrent_rate(&self) -> Option<ConcurrentRateSpec> {
        parse_concurrent_rate(self.concurrent_rate.as_deref())
    }

    /// Merges `header` and `mediaHeader` (`mediaHeader` overrides case-insensitively on conflicts).
    pub fn parse_media_header_map(&self) -> HashMap<String, String> {
        let mut merged = parse_header_json_map(self.header.as_deref());
        for (k, v) in parse_header_json_map(self.media_header.as_deref()) {
            merged.retain(|existing_k, _| !existing_k.eq_ignore_ascii_case(&k));
            merged.insert(k, v);
        }
        merged
    }

    /// Merges `header` and `mediaHeader`, stripping sensitive static headers (`authorization`,
    /// `cookie`, `x-api-key`, `proxy-authorization`) whenever `target_url` does not match the
    /// host or registrable domain (eTLD+1) of `effective_source_url()`.
    pub fn parse_media_header_map_for_target(&self, target_url: &str) -> HashMap<String, String> {
        let mut merged = self.parse_media_header_map();
        strip_cross_origin_sensitive_headers(&mut merged, self.effective_source_url(), target_url);
        merged
    }

    pub fn parse_request_header_map(&self) -> HashMap<String, String> {
        parse_header_json_map(self.header.as_deref())
    }

    pub fn parse_json_batch(raw: &str) -> Result<Vec<(Self, String)>, String> {
        let mut batch = xmedia_source::crud::parse_json_batch::<Self, _>(
            raw,
            "影视源",
            "未解析到有效的影视源条目",
            |src| src.source_name.trim().is_empty() && src.effective_source_url().trim().is_empty(),
        )?;

        for (spec, item_raw) in &mut batch {
            spec.source_name = spec.source_name.trim().to_string();
            if spec.source_url.trim().is_empty() {
                let effective = spec.effective_source_url().to_string();
                if !effective.is_empty() {
                    spec.source_url = effective.clone();
                    if let Ok(mut val) = serde_json::from_str::<serde_json::Value>(item_raw)
                        && let Some(obj) = val.as_object_mut()
                    {
                        obj.insert("sourceUrl".into(), serde_json::Value::String(effective));
                        if let Ok(serialized) = serde_json::to_string(&val) {
                            *item_raw = serialized;
                        }
                    }
                }
            } else {
                spec.source_url = spec.source_url.trim().to_string();
            }
        }
        Ok(batch)
    }
}

impl SourceSpecAdapter for VideoSourceSpec {
    const ENTITY_LABEL: &'static str = "影视源";
    const JSON_GROUP_KEY: &'static str = "sourceGroup";

    fn is_blank(&self) -> bool {
        self.source_name.trim().is_empty() && self.effective_source_url().trim().is_empty()
    }

    fn parse_batch(raw_json: &str) -> Result<Vec<(Self, String)>, String> {
        Self::parse_json_batch(raw_json)
    }

    fn source_name(&self) -> &str {
        &self.source_name
    }

    fn effective_source_url(&self) -> Cow<'_, str> {
        Cow::Borrowed(Self::effective_source_url(self))
    }

    fn group_name(&self) -> Option<&str> {
        self.source_group.as_deref()
    }

    fn enabled(&self) -> bool {
        self.enabled
    }

    fn weight(&self) -> i64 {
        self.weight
    }

    fn source_type_code(&self) -> i64 {
        Self::source_type_code(self)
    }

    fn summary_meta(&self) -> ParsedSourceSummaryMeta {
        let login_ui = self.parse_login_ui();
        let has_login_url = self
            .login_url
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty())
            || !login_ui.is_empty();
        let comment = self.source_comment.clone();
        let check_keyword = self
            .check_keyword
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);
        ParsedSourceSummaryMeta {
            comment,
            check_keyword,
            has_login_url,
            login_ui,
        }
    }
}

impl From<&VideoSourceSpec> for SourceMetadata {
    fn from(s: &VideoSourceSpec) -> Self {
        Self {
            source_name: s.source_name.clone(),
            source_url: s.effective_source_url().to_string(),
            header: s.header.clone(),
            login_url: s.login_url.clone(),
            login_ui: s.login_ui.clone(),
            js_lib: s.js_lib.clone(),
            enabled_cookie_jar: s.enabled_cookie_jar,
            raw_json: serde_json::to_string(s).ok(),
        }
    }
}

pub fn parse_header_json_map(raw: Option<&str>) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return out;
    };
    if let Ok(serde_json::Value::Object(map)) = serde_json::from_str(raw) {
        for (k, v) in map {
            let key_trimmed = k.trim();
            if key_trimmed.is_empty() {
                continue;
            }
            let val_str = match v {
                serde_json::Value::String(s) => s,
                serde_json::Value::Null => continue,
                other => other.to_string(),
            };
            out.retain(|existing_k: &String, _| !existing_k.eq_ignore_ascii_case(key_trimmed));
            out.insert(key_trimmed.to_string(), val_str);
        }
    }
    out
}

pub fn strip_cross_origin_sensitive_headers(
    headers: &mut HashMap<String, String>,
    source_url: &str,
    target_url: &str,
) {
    let same_affinity = has_origin_affinity(source_url, target_url);
    headers.retain(|k, _| {
        let trimmed = k.trim();
        !trimmed.eq_ignore_ascii_case("proxy-authorization")
            && (same_affinity || !xmedia_source::http::proxy::is_sensitive_origin_header(trimmed))
    });
}

#[cfg(test)]
mod origin_affinity_tests {
    use super::*;

    #[test]
    fn parse_media_header_map_for_target_strips_sensitive_headers_on_cross_origin() {
        let spec = VideoSourceSpec {
            source_name: "PrivateVod".into(),
            source_url: "https://vod.cinema.com.cn".into(),
            header: Some(
                r#"{"Authorization":"Bearer vod-token","Cookie":"vip=1","X-Api-Key":"vk-9","Referer":"https://vod.cinema.com.cn/"}"#
                    .into(),
            ),
            ..Default::default()
        };

        let same_site =
            spec.parse_media_header_map_for_target("https://hls.cinema.com.cn/live/index.m3u8");
        assert_eq!(
            same_site.get("Authorization").map(String::as_str),
            Some("Bearer vod-token")
        );
        assert_eq!(same_site.get("Cookie").map(String::as_str), Some("vip=1"));

        for cross_url in [
            "https://other.com.cn/live/index.m3u8",
            "https://cdn-external.example.net/seg0.ts",
            "https://app2.workers.dev/seg0.ts",
        ] {
            let filtered = spec.parse_media_header_map_for_target(cross_url);
            assert!(
                !filtered
                    .keys()
                    .any(|k| k.eq_ignore_ascii_case("authorization"))
            );
            assert!(!filtered.keys().any(|k| k.eq_ignore_ascii_case("cookie")));
            assert!(!filtered.keys().any(|k| k.eq_ignore_ascii_case("x-api-key")));
            assert!(
                !filtered
                    .keys()
                    .any(|k| k.eq_ignore_ascii_case("proxy-authorization"))
            );
            assert_eq!(
                filtered.get("Referer").map(String::as_str),
                Some("https://vod.cinema.com.cn/")
            );
        }
    }
}

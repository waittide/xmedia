pub mod types;

use std::borrow::Cow;
use std::collections::HashMap;

use serde::{Deserialize, Deserializer, Serialize};
use xmedia_source::crud::{ParsedSourceSummaryMeta, SourceSpecAdapter};
use xmedia_source::http::normalize_cookie_key;
pub use xmedia_source::http::proxy::{has_origin_affinity, strip_cross_origin_headers};
pub use xmedia_source::model::{
    ConcurrentRateSpec, ExecutionDiagnostics, LoginUiItem, SourceCallError, SourceCallOutput,
    SourceMetadata, SourceRuntimeState, de_bool_true, de_i64_zero, de_opt_string,
    de_string_or_default, default_true, parse_concurrent_rate, parse_login_ui_json,
};

pub use types::*;

fn de_string_vec_lenient<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let val = Option::<serde_json::Value>::deserialize(deserializer)?;
    let Some(v) = val else {
        return Ok(Vec::new());
    };
    match v {
        serde_json::Value::Null => Ok(Vec::new()),
        serde_json::Value::Array(arr) => Ok(parse_array_strings(arr)),
        serde_json::Value::String(s) => Ok(split_delimited_strings(&s)),
        _ => Ok(Vec::new()),
    }
}

fn parse_array_strings(arr: Vec<serde_json::Value>) -> Vec<String> {
    arr.into_iter()
        .filter_map(|item| match item {
            serde_json::Value::String(s) => non_empty_trimmed(&s).map(ToString::to_string),
            serde_json::Value::Number(n) => Some(n.to_string()),
            _ => None,
        })
        .collect()
}

fn split_delimited_strings(raw: &str) -> Vec<String> {
    raw.split([',', '，', '|'])
        .filter_map(non_empty_trimmed)
        .map(ToString::to_string)
        .collect()
}

fn non_empty_trimmed(raw: &str) -> Option<&str> {
    let trimmed = raw.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoSourceFormat {
    #[default]
    ApplecmsV10,
    Rule,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoCategoriesRule {
    #[serde(default, deserialize_with = "de_opt_string")]
    pub list: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub id: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoSearchRule {
    #[serde(default, deserialize_with = "de_opt_string")]
    pub list: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub vod_id: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub cover_url: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub remarks: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub year: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub area: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub type_name: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub detail_url: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoDetailRule {
    #[serde(default, deserialize_with = "de_opt_string")]
    pub request_url: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub init: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub cover_url: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
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
    #[serde(default, deserialize_with = "de_opt_string")]
    pub play_groups: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub group_name: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub episodes: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub episode_name: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
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

#[derive(Debug, Clone, Copy)]
pub enum VideoSourceBackend<'a> {
    Applecms {
        api_url: &'a str,
        preferred_play_from: &'a [String],
        play_filter_regex: &'a str,
    },
    Rule {
        enabled_explore: bool,
        categories: Option<(&'a str, &'a VideoCategoriesRule)>,
        search_url: Option<&'a str>,
        search_rule: Option<&'a VideoSearchRule>,
        explore_url: Option<&'a str>,
        explore_rule: Option<&'a VideoSearchRule>,
        detail_rule: Option<&'a VideoDetailRule>,
        play_rule: Option<&'a VideoPlayRule>,
    },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoSourceSpec {
    #[serde(default, deserialize_with = "de_string_or_default")]
    pub source_name: String,
    #[serde(default, deserialize_with = "de_string_or_default")]
    pub source_url: String,
    #[serde(default, deserialize_with = "de_opt_string")]
    source_group: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    source_comment: Option<String>,
    #[serde(default)]
    source_format: Option<VideoSourceFormat>,
    #[serde(default = "default_true", deserialize_with = "de_bool_true")]
    enabled: bool,
    #[serde(default = "default_true", deserialize_with = "de_bool_true")]
    enabled_explore: bool,
    #[serde(default = "default_true", deserialize_with = "de_bool_true")]
    pub enabled_cookie_jar: bool,
    #[serde(default = "default_true", deserialize_with = "de_bool_true")]
    pub searchable: bool,
    #[serde(default, deserialize_with = "de_i64_zero")]
    weight: i64,
    #[serde(default, deserialize_with = "de_i64_zero")]
    custom_order: i64,
    #[serde(default, deserialize_with = "de_opt_string")]
    header: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    media_header: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    concurrent_rate: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    check_keyword: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub login_url: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    login_ui: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub login_check_js: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    js_lib: Option<String>,

    // AppleCMS v10 mode fields
    #[serde(default, deserialize_with = "de_opt_string")]
    api_url: Option<String>,
    #[serde(default, deserialize_with = "de_string_vec_lenient")]
    pub preferred_play_from: Vec<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    play_url_filter_regex: Option<String>,

    // Generic Rule mode fields
    #[serde(default, deserialize_with = "de_opt_string")]
    search_url: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    explore_url: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    categories_url: Option<String>,
    #[serde(default)]
    rule_categories: Option<VideoCategoriesRule>,
    #[serde(default)]
    rule_search: Option<VideoSearchRule>,
    #[serde(default)]
    rule_explore: Option<VideoSearchRule>,
    #[serde(default)]
    rule_detail: Option<VideoDetailRule>,
    #[serde(default)]
    rule_play: Option<VideoPlayRule>,
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

    pub fn effective_format(&self) -> VideoSourceFormat {
        if let Some(fmt) = self.source_format {
            return fmt;
        }
        let has_api_url = self
            .api_url
            .as_deref()
            .and_then(non_empty_trimmed)
            .is_some();
        if has_api_url && self.rule_search.is_none() {
            VideoSourceFormat::ApplecmsV10
        } else {
            VideoSourceFormat::Rule
        }
    }

    pub fn backend(&self) -> VideoSourceBackend<'_> {
        match self.effective_format() {
            VideoSourceFormat::ApplecmsV10 => VideoSourceBackend::Applecms {
                api_url: resolve_api_url(self.api_url.as_deref(), &self.source_url),
                preferred_play_from: &self.preferred_play_from,
                play_filter_regex: resolve_play_filter_regex(self.play_url_filter_regex.as_deref()),
            },
            VideoSourceFormat::Rule => {
                let categories = self
                    .categories_url
                    .as_deref()
                    .and_then(non_empty_trimmed)
                    .zip(self.rule_categories.as_ref());
                VideoSourceBackend::Rule {
                    enabled_explore: self.enabled_explore,
                    categories,
                    search_url: self.search_url.as_deref().and_then(non_empty_trimmed),
                    search_rule: self.rule_search.as_ref(),
                    explore_url: self.explore_url.as_deref().and_then(non_empty_trimmed),
                    explore_rule: self.rule_explore.as_ref().or(self.rule_search.as_ref()),
                    detail_rule: self.rule_detail.as_ref(),
                    play_rule: self.rule_play.as_ref(),
                }
            }
        }
    }

    pub fn parse_concurrent_rate(&self) -> Option<ConcurrentRateSpec> {
        parse_concurrent_rate(self.concurrent_rate.as_deref())
    }

    pub fn default_test_keyword(&self) -> &str {
        self.check_keyword
            .as_deref()
            .and_then(non_empty_trimmed)
            .unwrap_or("电影")
    }

    /// Merges `header` and `mediaHeader` (`mediaHeader` overrides case-insensitively), strips
    /// cross-origin sensitive static headers and static `Range`, and optionally injects stored
    /// domain cookies when `enabled_cookie_jar` is active and no static `Cookie` header is present.
    pub fn build_media_headers(
        &self,
        target_url: &str,
        cookies: Option<&HashMap<String, String>>,
    ) -> HashMap<String, String> {
        let mut merged = merge_media_headers_for_target(
            self.header.as_deref(),
            self.media_header.as_deref(),
            self.effective_source_url(),
            target_url,
        );
        merged.retain(|k, _| !k.eq_ignore_ascii_case("range"));
        if let Some(jar) = cookies
            && self.enabled_cookie_jar
            && !merged.keys().any(|k| k.eq_ignore_ascii_case("cookie"))
            && let Some(cookie) = lookup_cookie_for_url(jar, target_url)
        {
            merged.insert("Cookie".to_string(), cookie);
        }
        merged
    }

    fn normalize_batch_entry(&mut self, item_raw: &mut String) {
        self.source_name = self.source_name.trim().to_string();
        if self.source_url.trim().is_empty() {
            let effective = self.effective_source_url().to_string();
            if !effective.is_empty() {
                self.source_url = effective.clone();
                inject_source_url_into_raw(item_raw, effective);
            }
        } else {
            self.source_url = self.source_url.trim().to_string();
        }
    }
}

fn resolve_api_url<'a>(api_url: Option<&'a str>, source_url: &'a str) -> &'a str {
    api_url
        .and_then(non_empty_trimmed)
        .unwrap_or_else(|| source_url.trim())
}

fn resolve_play_filter_regex(play_url_filter_regex: Option<&str>) -> &str {
    play_url_filter_regex
        .and_then(non_empty_trimmed)
        .unwrap_or(VideoSourceSpec::DEFAULT_PLAY_FILTER_REGEX)
}

fn merge_media_headers_for_target(
    header: Option<&str>,
    media_header: Option<&str>,
    source_url: &str,
    target_url: &str,
) -> HashMap<String, String> {
    let mut merged = parse_header_json_map(header);
    for (k, v) in parse_header_json_map(media_header) {
        insert_case_insensitive_header(&mut merged, k, v);
    }
    strip_cross_origin_headers(&mut merged, source_url, target_url);
    merged
}

fn inject_source_url_into_raw(item_raw: &mut String, effective_url: String) {
    if let Ok(mut val) = serde_json::from_str::<serde_json::Value>(item_raw)
        && let Some(obj) = val.as_object_mut()
    {
        obj.insert("sourceUrl".into(), serde_json::Value::String(effective_url));
        if let Ok(serialized) = serde_json::to_string(&val) {
            *item_raw = serialized;
        }
    }
}

impl SourceSpecAdapter for VideoSourceSpec {
    const ENTITY_LABEL: &'static str = "影视源";
    const JSON_GROUP_KEY: &'static str = "sourceGroup";

    fn is_blank(&self) -> bool {
        self.source_name.trim().is_empty() && self.effective_source_url().trim().is_empty()
    }

    fn parse_batch(raw_json: &str) -> Result<Vec<(Self, String)>, String> {
        let mut batch = xmedia_source::crud::parse_json_batch::<Self, _>(
            raw_json,
            "影视源",
            "未解析到有效的影视源条目",
            Self::is_blank,
        )?;
        for (spec, item_raw) in &mut batch {
            spec.normalize_batch_entry(item_raw);
        }
        Ok(batch)
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
        match self.effective_format() {
            VideoSourceFormat::ApplecmsV10 => 0,
            VideoSourceFormat::Rule => 1,
        }
    }

    fn summary_meta(&self) -> ParsedSourceSummaryMeta {
        let login_ui = parse_login_ui_json(self.login_ui.as_deref());
        let has_login_url = self
            .login_url
            .as_deref()
            .and_then(non_empty_trimmed)
            .is_some()
            || !login_ui.is_empty();
        let comment = self.source_comment.clone();
        let check_keyword = self
            .check_keyword
            .as_deref()
            .and_then(non_empty_trimmed)
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

fn insert_case_insensitive_header(map: &mut HashMap<String, String>, key: String, value: String) {
    map.retain(|existing_k, _| !existing_k.eq_ignore_ascii_case(&key));
    map.insert(key, value);
}

fn parse_header_json_map(raw: Option<&str>) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let Some(raw) = raw.and_then(non_empty_trimmed) else {
        return out;
    };
    if let Ok(serde_json::Value::Object(map)) = serde_json::from_str(raw) {
        for (k, v) in map {
            let Some(key_trimmed) = non_empty_trimmed(&k) else {
                continue;
            };
            let val_str = match v {
                serde_json::Value::String(s) => s,
                serde_json::Value::Null => continue,
                other => other.to_string(),
            };
            insert_case_insensitive_header(&mut out, key_trimmed.to_string(), val_str);
        }
    }
    out
}

fn lookup_cookie_for_url(cookies: &HashMap<String, String>, target_url: &str) -> Option<String> {
    let key = normalize_cookie_key(target_url);
    if key.is_empty() {
        return None;
    }
    if let Some(v) = cookies.get(&key)
        && !v.trim().is_empty()
    {
        return Some(v.clone());
    }
    match_subdomain_cookie(cookies, &key)
}

fn match_subdomain_cookie(cookies: &HashMap<String, String>, host_key: &str) -> Option<String> {
    let mut best: Option<(&str, &str)> = None;
    for (stored_host, val) in cookies {
        let clean_stored = stored_host.trim_start_matches('.');
        if clean_stored.is_empty() || val.trim().is_empty() {
            continue;
        }
        if host_key.len() > clean_stored.len()
            && host_key.ends_with(clean_stored)
            && host_key.as_bytes()[host_key.len() - clean_stored.len() - 1] == b'.'
        {
            let replace = match best {
                None => true,
                Some((prev_host, _)) => {
                    clean_stored.len() > prev_host.len()
                        || (clean_stored.len() == prev_host.len() && clean_stored < prev_host)
                }
            };
            if replace {
                best = Some((clean_stored, val.as_str()));
            }
        }
    }
    best.map(|(_, v)| v.to_string())
}

#[cfg(test)]
mod origin_affinity_tests {
    use super::*;

    #[test]
    fn build_media_headers_strips_sensitive_headers_on_cross_origin() {
        let spec = VideoSourceSpec {
            source_name: "PrivateVod".into(),
            source_url: "https://vod.cinema.com.cn".into(),
            header: Some(
                r#"{"Authorization":"Bearer vod-token","Cookie":"vip=1","X-Api-Key":"vk-9","Referer":"https://vod.cinema.com.cn/"}"#
                    .into(),
            ),
            ..Default::default()
        };

        let same_site = spec.build_media_headers("https://hls.cinema.com.cn/live/index.m3u8", None);
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
            let filtered = spec.build_media_headers(cross_url, None);
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

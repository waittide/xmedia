use std::collections::HashMap;

use serde::{Deserialize, Deserializer, Serialize};
pub use xmedia_source::model::{
    ConcurrentRateSpec, ExecutionDiagnostics, LoginUiItem, SourceCallError, SourceCallOutput,
    SourceMetadata, SourceRuntimeState, de_bool_true, de_i64_zero, de_opt_string,
    de_string_or_default, default_true, parse_concurrent_rate, parse_login_ui_json,
};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineVideoCategory {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub parent_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineVideoSummary {
    pub vod_id: String,
    pub title: String,
    #[serde(default)]
    pub cover_url: Option<String>,
    #[serde(default)]
    pub proxy_cover_url: Option<String>,
    #[serde(default)]
    pub remarks: Option<String>,
    #[serde(default)]
    pub year: Option<String>,
    #[serde(default)]
    pub area: Option<String>,
    #[serde(default)]
    pub type_name: Option<String>,
    pub detail_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineVideoEpisode {
    pub index: usize,
    pub name: String,
    pub play_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineVideoPlayGroup {
    pub name: String,
    pub episodes: Vec<OnlineVideoEpisode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineVideoDetail {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub cover_url: Option<String>,
    #[serde(default)]
    pub proxy_cover_url: Option<String>,
    #[serde(default)]
    pub intro: Option<String>,
    #[serde(default)]
    pub year: Option<String>,
    #[serde(default)]
    pub area: Option<String>,
    #[serde(default)]
    pub actors: Option<String>,
    #[serde(default)]
    pub director: Option<String>,
    #[serde(default)]
    pub remarks: Option<String>,
    pub play_groups: Vec<OnlineVideoPlayGroup>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineVideoSubtitle {
    pub label: String,
    #[serde(default)]
    pub lang: Option<String>,
    pub url: String,
    #[serde(default)]
    pub proxy_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineVideoPlayResult {
    pub stream_url: String,
    pub proxy_stream_url: String,
    pub stream_type: String,
    pub headers_applied: bool,
    #[serde(default)]
    pub subtitles: Vec<OnlineVideoSubtitle>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct VideoJsContext {
    pub vod_id: String,
    pub title: String,
    pub detail_url: String,
    pub play_url: String,
    pub group_name: String,
}

fn is_sensitive_static_header(name: &str) -> bool {
    matches!(
        name.trim().to_ascii_lowercase().as_str(),
        "authorization" | "cookie" | "x-api-key" | "proxy-authorization"
    )
}

fn extract_http_host(raw_url: &str, allow_schemeless: bool) -> Option<String> {
    let trimmed = raw_url.trim();
    let after_scheme = if trimmed
        .get(..7)
        .is_some_and(|p| p.eq_ignore_ascii_case("http://"))
    {
        &trimmed[7..]
    } else if trimmed
        .get(..8)
        .is_some_and(|p| p.eq_ignore_ascii_case("https://"))
    {
        &trimmed[8..]
    } else if allow_schemeless && !trimmed.contains("://") && !trimmed.is_empty() {
        trimmed
    } else {
        return None;
    };
    let authority = after_scheme
        .split(['/', '?', '#', '\\'])
        .next()
        .unwrap_or("");
    let host_port = authority
        .rsplit_once('@')
        .map(|(_, hp)| hp)
        .unwrap_or(authority);
    let host = if let Some(rest) = host_port.strip_prefix('[') {
        let (inside, after) = rest.split_once(']')?;
        if !after.is_empty()
            && !after
                .strip_prefix(':')
                .is_some_and(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
        {
            return None;
        }
        inside
    } else {
        host_port
            .split_once(':')
            .map(|(h, _)| h)
            .unwrap_or(host_port)
    };
    let normalized = host.trim().trim_end_matches('.').to_ascii_lowercase();
    if normalized.is_empty()
        || normalized
            .bytes()
            .any(|b| b.is_ascii_whitespace() || b.is_ascii_control())
    {
        return None;
    }
    Some(normalized)
}

fn registrable_domain(host: &str) -> Option<&str> {
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() < 2 || labels.iter().any(|l| l.is_empty()) {
        return None;
    }
    let n = labels.len();
    let tld = labels[n - 1];
    let sld = labels[n - 2];
    let is_multipart_suffix = (tld.len() == 2
        && matches!(
            sld,
            "com" | "co" | "net" | "org" | "gov" | "edu" | "ac" | "or" | "ne" | "go" | "idv"
        ))
        || matches!(
            (sld, tld),
            ("github", "io")
                | ("pages", "dev")
                | ("workers", "dev")
                | ("r2", "dev")
                | ("fly", "dev")
                | ("vercel", "app")
                | ("netlify", "app")
                | ("onrender", "com")
                | ("cloudfront", "net")
        );
    let keep = if is_multipart_suffix { 3 } else { 2 };
    if labels.len() < keep {
        return None;
    }
    let mut offset = 0usize;
    for l in &labels[..labels.len() - keep] {
        offset += l.len() + 1;
    }
    Some(&host[offset..])
}

pub fn has_origin_affinity(source_url: &str, target_url: &str) -> bool {
    let Some(sh) = extract_http_host(source_url, true) else {
        return false;
    };
    let Some(th) = extract_http_host(target_url, false) else {
        return false;
    };
    if sh == th {
        return true;
    }
    if sh.parse::<std::net::IpAddr>().is_ok() || th.parse::<std::net::IpAddr>().is_ok() {
        return false;
    }
    matches!(
        (registrable_domain(&sh), registrable_domain(&th)),
        (Some(sr), Some(tr)) if sr == tr
    )
}

pub fn strip_cross_origin_sensitive_headers(
    headers: &mut HashMap<String, String>,
    source_url: &str,
    target_url: &str,
) {
    if !has_origin_affinity(source_url, target_url) {
        headers.retain(|k, _| !is_sensitive_static_header(k));
    }
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

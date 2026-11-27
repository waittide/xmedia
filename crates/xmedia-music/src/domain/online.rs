use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Deserializer, Serialize};
use xmedia_source::crud::{ParsedSourceSummaryMeta, SourceSpecAdapter};
pub use xmedia_source::http::proxy::{has_origin_affinity, strip_cross_origin_headers};
pub use xmedia_source::http::url_encode_component;
use xmedia_source::http::{normalize_cookie_key, resolve_relative_url_preserving_options};
pub use xmedia_source::model::{
    BrowserRedirectHint, ConcurrentRateSpec, ExecutionDiagnostics, LoginUiItem, LoginUiStyle,
    SourceCallError, SourceCallOutput, SourceMetadata, SourceRuntimeState, de_bool_true,
    de_i64_zero, de_opt_string, de_string_or_default, default_true, parse_concurrent_rate,
    parse_login_ui_json,
};
use xmedia_source::rule::split_js_pipeline;

use crate::domain::catalog::{AudioFormat, StructuredLyrics};

fn de_string_list_or_csv<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let val = Option::<serde_json::Value>::deserialize(deserializer)?;
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    if let Some(v) = val {
        parse_quality_json_value(v, &mut seen, &mut out);
    }
    Ok(out)
}

fn parse_quality_json_value(
    val: serde_json::Value,
    seen: &mut HashSet<String>,
    out: &mut Vec<String>,
) {
    match val {
        serde_json::Value::Null => {}
        serde_json::Value::Array(arr) => {
            for item in arr {
                match item {
                    serde_json::Value::String(s) => push_clean_quality(&s, seen, out),
                    serde_json::Value::Number(n) => push_clean_quality(&n.to_string(), seen, out),
                    _ => {}
                }
            }
        }
        serde_json::Value::String(s) => {
            for part in s.split([',', ';', '|']) {
                push_clean_quality(part, seen, out);
            }
        }
        serde_json::Value::Number(n) => push_clean_quality(&n.to_string(), seen, out),
        other => push_clean_quality(&other.to_string(), seen, out),
    }
}

fn push_clean_quality(raw: &str, seen: &mut HashSet<String>, out: &mut Vec<String>) {
    let t = raw.trim();
    if !t.is_empty() && seen.insert(t.to_string()) {
        out.push(t.to_string());
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MusicRuleKind {
    Search,
    Explore,
    TrackDetail,
    Stream,
    Cover,
    Lyric,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicSearchRule {
    #[serde(default, deserialize_with = "de_opt_string")]
    pub track_list: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub track_id: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub title: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub artist: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub album: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub duration: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub duration_unit: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub cover_url: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub cover_id: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub url_id: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub lyric_id: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub play_url: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub detail_url: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub format: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub check_key_word: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicTrackDetailRule {
    #[serde(default, deserialize_with = "de_opt_string")]
    pub request_url: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub init: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub track_id: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub title: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub artist: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub album: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub duration: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub duration_unit: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub cover_url: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub cover_id: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub url_id: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub lyric_id: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub play_url: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub lyric: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub format: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicStreamRule {
    #[serde(default, deserialize_with = "de_opt_string")]
    pub request_url: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub stream_url: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub format: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub bitrate: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub headers: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub expires_in_secs: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicCoverRule {
    #[serde(default, deserialize_with = "de_opt_string")]
    pub request_url: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub cover_url: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicLyricRule {
    #[serde(default, deserialize_with = "de_opt_string")]
    pub request_url: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub lyric: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub translated_lyric: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub plain_lyric: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicSourceSpec {
    #[serde(default, deserialize_with = "de_string_or_default")]
    pub source_name: String,
    #[serde(default, deserialize_with = "de_string_or_default")]
    pub source_url: String,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub source_group: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub source_comment: Option<String>,
    #[serde(default = "default_true", deserialize_with = "de_bool_true")]
    pub enabled: bool,
    #[serde(default, deserialize_with = "de_i64_zero")]
    pub weight: i64,
    #[serde(default, deserialize_with = "de_i64_zero")]
    pub custom_order: i64,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub header: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub media_header: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub concurrent_rate: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub search_url: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub explore_url: Option<String>,
    #[serde(default, deserialize_with = "de_string_list_or_csv")]
    pub supported_qualities: Vec<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub login_url: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub login_ui: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub login_check_js: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub js_lib: Option<String>,
    #[serde(default = "default_true", deserialize_with = "de_bool_true")]
    pub enabled_cookie_jar: bool,
    #[serde(default, deserialize_with = "de_i64_zero")]
    pub respond_time: i64,
    #[serde(default, deserialize_with = "de_i64_zero")]
    pub last_update_time: i64,
    #[serde(default)]
    pub rule_search: Option<MusicSearchRule>,
    #[serde(default)]
    pub rule_explore: Option<MusicSearchRule>,
    #[serde(default)]
    pub rule_track_detail: Option<MusicTrackDetailRule>,
    #[serde(default)]
    pub rule_stream: Option<MusicStreamRule>,
    #[serde(default)]
    pub rule_cover: Option<MusicCoverRule>,
    #[serde(default)]
    pub rule_lyric: Option<MusicLyricRule>,
    #[serde(skip)]
    pub v2_plugin: Option<(xmedia_source::plugin_v2::PluginV2Manifest, String)>,
}

impl From<&MusicSourceSpec> for SourceMetadata {
    fn from(src: &MusicSourceSpec) -> Self {
        Self {
            source_name: src.source_name.clone(),
            source_url: src.source_url.clone(),
            header: src.header.clone(),
            login_url: src.login_url.clone(),
            login_ui: src.login_ui.clone(),
            js_lib: src.js_lib.clone(),
            enabled_cookie_jar: src.enabled_cookie_jar,
            raw_json: None,
        }
    }
}

fn is_blank_music_source(parsed: &MusicSourceSpec) -> bool {
    parsed.source_name.trim().is_empty() && parsed.source_url.trim().is_empty()
}

fn sync_music_source_group(spec: MusicSourceSpec, item_raw: String) -> (MusicSourceSpec, String) {
    let group = spec.source_group.as_deref().map(str::trim).unwrap_or("");
    let synced = xmedia_source::crud::sync_raw_json_fields(
        &item_raw,
        None,
        None,
        Some(group),
        MusicSourceSpec::JSON_GROUP_KEY,
    );
    (spec, synced)
}

impl SourceSpecAdapter for MusicSourceSpec {
    const MEDIA_KIND: xmedia_core::domain::library::MediaKind =
        xmedia_core::domain::library::MediaKind::Music;
    const ENTITY_LABEL: &'static str = "音乐插件";
    const JSON_GROUP_KEY: &'static str = "sourceGroup";

    fn parse_batch(raw_json: &str) -> Result<Vec<(Self, String)>, String> {
        let parsed = xmedia_source::crud::parse_json_batch(
            raw_json,
            "音乐源",
            "未解析到有效音乐源（缺少 sourceName / sourceUrl）",
            is_blank_music_source,
        )?;
        Ok(parsed
            .into_iter()
            .map(|(spec, item_raw)| sync_music_source_group(spec, item_raw))
            .collect())
    }

    fn from_v2_manifest(
        manifest: xmedia_source::plugin_v2::PluginV2Manifest,
        js_code: String,
    ) -> Result<Self, String> {
        let header = (!manifest.headers.is_empty())
            .then(|| serde_json::to_string(&manifest.headers).ok())
            .flatten();
        let media_header = (!manifest.media_headers.is_empty())
            .then(|| serde_json::to_string(&manifest.media_headers).ok())
            .flatten();
        let explore_url = (!manifest.explore_categories.is_empty())
            .then(|| {
                let arr: Vec<serde_json::Value> = manifest
                    .explore_categories
                    .iter()
                    .map(|c| serde_json::json!({ "title": c.title, "url": c.id }))
                    .collect();
                serde_json::to_string(&arr).ok()
            })
            .flatten();
        Ok(Self {
            source_name: manifest.name.clone(),
            source_url: manifest.base_url.clone(),
            source_group: (!manifest.group.is_empty()).then(|| manifest.group.clone()),
            source_comment: manifest.description.clone(),
            enabled: manifest.enabled,
            weight: manifest.weight,
            header,
            media_header,
            concurrent_rate: manifest.rate_limit.clone(),
            explore_url,
            supported_qualities: manifest.qualities.clone(),
            enabled_cookie_jar: true,
            v2_plugin: Some((manifest, js_code)),
            ..Default::default()
        })
    }

    fn source_name(&self) -> &str {
        &self.source_name
    }

    fn effective_source_url(&self) -> Cow<'_, str> {
        Cow::Borrowed(self.source_url.trim())
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
        0
    }

    fn summary_meta(&self) -> ParsedSourceSummaryMeta {
        if let Some((ref m, _)) = self.v2_plugin {
            return ParsedSourceSummaryMeta {
                version: Some(m.version.clone()),
                comment: m.description.clone(),
                check_keyword: m.check_keyword.clone(),
                has_login_url: m.auth.is_supported(),
                login_ui: Vec::new(),
                auth: Some(m.auth.clone()),
                settings_schema: m.settings.clone(),
            };
        }
        let login_ui = self.parse_login_ui();
        let has_login_url = self.has_any_login_config(&login_ui);
        ParsedSourceSummaryMeta {
            comment: self.source_comment.clone(),
            check_keyword: self.extract_check_keyword().map(str::to_string),
            has_login_url,
            login_ui,
            ..Default::default()
        }
    }
}

impl MusicSourceSpec {
    pub fn has_rule(&self, kind: MusicRuleKind) -> bool {
        if let Some((ref m, _)) = self.v2_plugin {
            return match kind {
                MusicRuleKind::Search => m.has_search,
                MusicRuleKind::Explore => m.has_explore,
                MusicRuleKind::TrackDetail => m.has_detail,
                MusicRuleKind::Stream => m.has_stream,
                MusicRuleKind::Cover => m.has_cover_resolver,
                MusicRuleKind::Lyric => m.has_lyrics,
            };
        }
        match kind {
            MusicRuleKind::Search => self.rule_search.is_some(),
            MusicRuleKind::Explore => self.rule_explore.is_some() || self.rule_search.is_some(),
            MusicRuleKind::TrackDetail => self.rule_track_detail.is_some(),
            MusicRuleKind::Stream => self.rule_stream.is_some(),
            MusicRuleKind::Cover => self.rule_cover.is_some(),
            MusicRuleKind::Lyric => self.rule_lyric.is_some(),
        }
    }

    pub fn default_test_keyword(&self) -> String {
        self.extract_check_keyword().unwrap_or("Mojito").to_string()
    }

    pub fn list_rule(&self, explore: bool) -> Result<&MusicSearchRule, String> {
        if explore {
            self.rule_explore
                .as_ref()
                .or(self.rule_search.as_ref())
                .ok_or_else(|| format!("音源「{}」未配置浏览解析规则", self.source_name))
        } else {
            self.rule_search
                .as_ref()
                .ok_or_else(|| format!("音源「{}」未配置 ruleSearch", self.source_name))
        }
    }

    pub fn resolve_quality<'a>(&'a self, requested: Option<&'a str>) -> (&'a str, bool) {
        let effective = requested
            .or_else(|| self.supported_qualities.first().map(String::as_str))
            .unwrap_or("320");
        let matches_single_or_absent = requested.is_none_or(|q| {
            self.supported_qualities.len() == 1
                && self.supported_qualities[0].eq_ignore_ascii_case(q)
        });
        (effective, matches_single_or_absent)
    }

    pub fn parse_header_map(raw: Option<&str>) -> HashMap<String, String> {
        let mut out = HashMap::new();
        let Some(s) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
            return out;
        };
        if let Ok(serde_json::Value::Object(map)) = serde_json::from_str(s) {
            parse_json_header_entries(map, &mut out);
        }
        out
    }

    /// Merges `header` and `mediaHeader` (`mediaHeader` takes precedence on conflicts).
    pub fn parse_media_headers(&self) -> HashMap<String, String> {
        let mut merged = Self::parse_header_map(self.header.as_deref());
        for (k, v) in Self::parse_header_map(self.media_header.as_deref()) {
            insert_header_case_insensitive(&mut merged, k, v);
        }
        merged
    }

    /// Builds sanitized media headers for `target_url` including cookie-jar cookies when enabled
    /// and stripping static `Range` overrides.
    pub fn build_media_headers_with_cookies(
        &self,
        cookies: &HashMap<String, String>,
        target_url: &str,
    ) -> HashMap<String, String> {
        let mut merged = self.parse_media_headers_for_target(target_url);
        strip_range_header(&mut merged);
        if !merged.keys().any(|k| k.eq_ignore_ascii_case("cookie"))
            && self.enabled_cookie_jar
            && let Some(cookie) = lookup_cookie_for_url(cookies, target_url)
        {
            merged.insert("Cookie".to_string(), cookie);
        }
        merged
    }

    fn parse_login_ui(&self) -> Vec<LoginUiItem> {
        parse_login_ui_json(self.login_ui.as_deref())
    }

    fn extract_check_keyword(&self) -> Option<&str> {
        if let Some((ref m, _)) = self.v2_plugin {
            return m
                .check_keyword
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty());
        }
        self.rule_search
            .as_ref()
            .and_then(|r| r.check_key_word.as_deref())
            .map(str::trim)
            .filter(|s| !s.is_empty())
    }

    fn has_any_login_config(&self, login_ui: &[LoginUiItem]) -> bool {
        self.login_url
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty())
            || !login_ui.is_empty()
    }

    fn parse_media_headers_for_target(&self, target_url: &str) -> HashMap<String, String> {
        let mut merged = self.parse_media_headers();
        strip_cross_origin_headers(&mut merged, &self.source_url, target_url);
        merged
    }
}

fn parse_json_header_entries(
    map: serde_json::Map<String, serde_json::Value>,
    out: &mut HashMap<String, String>,
) {
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
        insert_header_case_insensitive(out, key_trimmed.to_string(), val_str);
    }
}

fn insert_header_case_insensitive(out: &mut HashMap<String, String>, key: String, val: String) {
    out.retain(|existing_k: &String, _| !existing_k.eq_ignore_ascii_case(&key));
    out.insert(key, val);
}

fn strip_range_header(headers: &mut HashMap<String, String>) {
    headers.retain(|k, _| !k.eq_ignore_ascii_case("range"));
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
    let mut best: Option<(&str, &str)> = None;
    for (stored_host, val) in cookies {
        let clean_stored = stored_host.trim_start_matches('.');
        if clean_stored.is_empty() || val.trim().is_empty() {
            continue;
        }
        if is_subdomain_cookie_match(&key, clean_stored)
            && is_better_cookie_domain(best, clean_stored)
        {
            best = Some((clean_stored, val.as_str()));
        }
    }
    best.map(|(_, v)| v.to_string())
}

fn is_subdomain_cookie_match(host_key: &str, stored_domain: &str) -> bool {
    host_key.len() > stored_domain.len()
        && host_key.ends_with(stored_domain)
        && host_key.as_bytes()[host_key.len() - stored_domain.len() - 1] == b'.'
}

fn is_better_cookie_domain(current: Option<(&str, &str)>, candidate: &str) -> bool {
    match current {
        None => true,
        Some((prev_host, _)) => {
            candidate.len() > prev_host.len()
                || (candidate.len() == prev_host.len() && candidate < prev_host)
        }
    }
}

/// Normalizes multi-value artist outputs (`"A\nB"`) into `"A / B"`.
pub fn normalize_artist(raw: &str) -> String {
    raw.lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" / ")
}

/// Parses duration values formatted as `hh:mm:ss`, `mm:ss`, seconds, or milliseconds.
pub fn parse_duration_secs(raw: &str, unit: Option<&str>) -> Option<u32> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.contains(':') {
        return parse_colon_duration(trimmed);
    }
    let val = trimmed.parse::<f64>().ok()?;
    if !val.is_finite() || val < 0.0 {
        return None;
    }
    Some(scale_numeric_duration(val, unit).round() as u32)
}

fn parse_colon_duration(trimmed: &str) -> Option<u32> {
    let parts: Vec<&str> = trimmed.split(':').collect();
    match parts.len() {
        2 => parse_mm_ss(parts[0], parts[1]),
        3 => parse_hh_mm_ss(parts[0], parts[1], parts[2]),
        _ => None,
    }
}

fn parse_mm_ss(mins_raw: &str, secs_raw: &str) -> Option<u32> {
    let mins = mins_raw.trim().parse::<f64>().ok()?;
    let secs = secs_raw.trim().parse::<f64>().ok()?;
    if mins < 0.0 || secs < 0.0 {
        return None;
    }
    Some((mins * 60.0 + secs).round() as u32)
}

fn parse_hh_mm_ss(hrs_raw: &str, mins_raw: &str, secs_raw: &str) -> Option<u32> {
    let hrs = hrs_raw.trim().parse::<f64>().ok()?;
    let mins = mins_raw.trim().parse::<f64>().ok()?;
    let secs = secs_raw.trim().parse::<f64>().ok()?;
    if hrs < 0.0 || mins < 0.0 || secs < 0.0 {
        return None;
    }
    Some((hrs * 3600.0 + mins * 60.0 + secs).round() as u32)
}

fn scale_numeric_duration(val: f64, unit: Option<&str>) -> f64 {
    let unit_norm = unit
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_ascii_lowercase);
    match unit_norm.as_deref() {
        Some("ms") => val / 1000.0,
        Some("s") | Some("sec") | Some("secs") => val,
        _ if val >= 36_000.0 => val / 1000.0,
        _ => val,
    }
}

/// Infers audio format from URL path extension (`flac`, `mp3`, `m4a`, `alac`, `aac`, `ogg`, `opus`, `wav`, `aiff`, `ape`, `wv`).
pub fn infer_audio_format(url: &str) -> Option<String> {
    let ext = extract_url_path_extension(url)?;
    ext.parse::<AudioFormat>().ok().map(|fmt| fmt.to_string())
}

fn extract_url_path_extension(url: &str) -> Option<&str> {
    let without_frag = url.split('#').next().unwrap_or(url);
    let without_query = without_frag.split('?').next().unwrap_or(without_frag);
    let last_seg = without_query.rsplit('/').next().unwrap_or(without_query);
    Some(last_seg.rsplit_once('.')?.1)
}

pub fn is_http_url(u: &str) -> bool {
    let trimmed = u.trim();
    trimmed
        .get(..7)
        .is_some_and(|p| p.eq_ignore_ascii_case("http://"))
        || trimmed
            .get(..8)
            .is_some_and(|p| p.eq_ignore_ascii_case("https://"))
}

/// Context passed into track detail / stream / cover / lyric resolution and JS `globalThis.track`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MusicTrackContext {
    pub track_id: String,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub duration_secs: Option<u32>,
    pub cover_url: Option<String>,
    pub cover_id: Option<String>,
    pub url_id: Option<String>,
    pub lyric_id: Option<String>,
    pub play_url: Option<String>,
    pub detail_url: Option<String>,
    pub quality: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OnlineMusicTrackItem {
    pub track_id: String,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub duration_secs: Option<u32>,
    pub cover_url: Option<String>,
    pub cover_id: Option<String>,
    pub url_id: Option<String>,
    pub lyric_id: Option<String>,
    pub play_url: Option<String>,
    pub detail_url: Option<String>,
    pub format: Option<String>,
}

impl OnlineMusicTrackItem {
    pub fn to_context(&self, quality: Option<String>) -> MusicTrackContext {
        MusicTrackContext {
            track_id: self.track_id.clone(),
            title: self.title.clone(),
            artist: self.artist.clone(),
            album: self.album.clone(),
            duration_secs: self.duration_secs,
            cover_url: self.cover_url.clone(),
            cover_id: self.cover_id.clone(),
            url_id: self.url_id.clone(),
            lyric_id: self.lyric_id.clone(),
            play_url: self.play_url.clone(),
            detail_url: self.detail_url.clone(),
            quality,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineMusicExploreCategory {
    pub title: String,
    pub url: String,
}

impl OnlineMusicExploreCategory {
    pub fn parse_list(raw: &str, base_url: &str) -> Vec<Self> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Vec::new();
        }
        if let Some(json_cats) = try_parse_json_explore_categories(trimmed, base_url) {
            return json_cats;
        }
        let mut out = Vec::new();
        for raw_entry in split_explore_entries_outside_js(trimmed) {
            if let Some(cat) = parse_text_explore_entry(raw_entry.trim(), base_url) {
                out.push(cat);
            }
        }
        out
    }
}

fn try_parse_json_explore_categories(
    trimmed: &str,
    base_url: &str,
) -> Option<Vec<OnlineMusicExploreCategory>> {
    if !trimmed.starts_with('[') {
        return None;
    }
    let arr = serde_json::from_str::<Vec<serde_json::Value>>(trimmed).ok()?;
    let out: Vec<OnlineMusicExploreCategory> = arr
        .into_iter()
        .filter_map(|item| parse_json_explore_item(item, base_url))
        .collect();
    (!out.is_empty()).then_some(out)
}

fn parse_json_explore_item(
    item: serde_json::Value,
    base_url: &str,
) -> Option<OnlineMusicExploreCategory> {
    let title = item
        .get("title")
        .or_else(|| item.get("name"))
        .and_then(|v| v.as_str())?
        .trim();
    let url = item.get("url").and_then(|v| v.as_str())?.trim();
    if title.is_empty() || url.is_empty() {
        return None;
    }
    Some(OnlineMusicExploreCategory {
        title: title.to_string(),
        url: normalize_explore_category_url(url, base_url),
    })
}

fn parse_text_explore_entry(entry: &str, base_url: &str) -> Option<OnlineMusicExploreCategory> {
    if entry.is_empty() {
        return None;
    }
    if let Some((title, url)) = split_title_and_explore_url(entry) {
        let t = title.trim();
        let u = url.trim();
        if !t.is_empty() && !u.is_empty() {
            return Some(OnlineMusicExploreCategory {
                title: t.to_string(),
                url: normalize_explore_category_url(u, base_url),
            });
        }
        return None;
    }
    if entry.starts_with("http://")
        || entry.starts_with("https://")
        || entry.starts_with('/')
        || split_js_pipeline(entry).is_some()
    {
        return Some(OnlineMusicExploreCategory {
            title: "默认榜单".to_string(),
            url: normalize_explore_category_url(entry, base_url),
        });
    }
    None
}

fn normalize_explore_category_url(raw_url: &str, base_url: &str) -> String {
    let trimmed = raw_url.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if split_js_pipeline(trimmed).is_some() || trimmed.contains("{{") || trimmed.contains("}}") {
        return trimmed.to_string();
    }
    resolve_relative_url_preserving_options(trimmed, base_url)
}

fn split_explore_entries_outside_js(raw: &str) -> Vec<&str> {
    let bytes = raw.as_bytes();
    let mut entries = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    let mut in_tag_js = false;
    let mut in_at_js = false;

    while i < bytes.len() {
        if !in_tag_js
            && bytes
                .get(i..i + 4)
                .is_some_and(|s| s.eq_ignore_ascii_case(b"<js>"))
        {
            in_tag_js = true;
            in_at_js = false;
            i += 4;
            continue;
        }
        if in_tag_js
            && bytes
                .get(i..i + 5)
                .is_some_and(|s| s.eq_ignore_ascii_case(b"</js>"))
        {
            in_tag_js = false;
            i += 5;
            continue;
        }
        if !in_tag_js
            && !in_at_js
            && bytes
                .get(i..i + 4)
                .is_some_and(|s| s.eq_ignore_ascii_case(b"@js:"))
        {
            in_at_js = true;
            i += 4;
            continue;
        }
        if !in_tag_js {
            if bytes[i] == b'\n' || bytes[i] == b'\r' {
                entries.push(&raw[start..i]);
                in_at_js = false;
                i += 1;
                start = i;
                continue;
            }
            if !in_at_js && bytes.get(i..i + 2) == Some(b"&&") {
                entries.push(&raw[start..i]);
                i += 2;
                start = i;
                continue;
            }
        }
        i += 1;
    }
    if start < bytes.len() {
        entries.push(&raw[start..]);
    }
    entries
}

fn find_js_prefix_offset(entry: &str) -> usize {
    let lower = entry.to_ascii_lowercase();
    [lower.find("@js:"), lower.find("<js>")]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or(entry.len())
}

fn split_title_and_explore_url(entry: &str) -> Option<(&str, &str)> {
    let js_offset = find_js_prefix_offset(entry);
    let (title, url) = entry.split_once("::")?;
    (title.len() < js_offset).then_some((title, url))
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OnlineMusicExploreResult {
    pub tracks: Vec<OnlineMusicTrackItem>,
    pub categories: Vec<OnlineMusicExploreCategory>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OnlineMusicStreamResult {
    pub stream_url: String,
    pub format: String,
    pub bitrate: Option<u32>,
    pub headers: HashMap<String, String>,
    pub expires_in_secs: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineMusicLyricsResult {
    pub lyrics: StructuredLyrics,
    pub translated_lyrics: Option<StructuredLyrics>,
    pub raw_lyric: String,
    pub raw_translated_lyric: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineMusicResolvedTrack {
    pub track: OnlineMusicTrackItem,
    pub stream: Option<OnlineMusicStreamResult>,
    pub cover_url: Option<String>,
    pub lyrics: Option<OnlineMusicLyricsResult>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MusicSourceTestMode {
    #[default]
    Full,
    Search,
    Explore,
    Detail,
    Stream,
    Lyric,
}

#[cfg(test)]
mod origin_affinity_tests {
    use super::*;

    #[test]
    fn parse_media_headers_for_target_strips_sensitive_headers_on_cross_origin() {
        let spec = MusicSourceSpec {
            source_name: "HiFiMusic".into(),
            source_url: "https://api.music.example.com".into(),
            header: Some(
                r#"{"Authorization":"Bearer mus-secret","Cookie":"u=42","X-Api-Key":"ak-1","Referer":"https://music.example.com/"}"#
                    .into(),
            ),
            media_header: Some(r#"{"Proxy-Authorization":"Basic c2VjcmV0"}"#.into()),
            ..Default::default()
        };
        let empty_cookies = HashMap::new();

        let same_domain = spec.build_media_headers_with_cookies(
            &empty_cookies,
            "https://audio-cdn.music.example.com/track.flac",
        );
        assert_eq!(
            same_domain.get("Authorization").map(String::as_str),
            Some("Bearer mus-secret")
        );
        assert_eq!(same_domain.get("Cookie").map(String::as_str), Some("u=42"));

        let cross_origin = spec.build_media_headers_with_cookies(
            &empty_cookies,
            "https://thirdparty-storage.net/track.flac",
        );
        assert!(
            !cross_origin
                .keys()
                .any(|k| k.eq_ignore_ascii_case("authorization"))
        );
        assert!(
            !cross_origin
                .keys()
                .any(|k| k.eq_ignore_ascii_case("cookie"))
        );
        assert!(
            !cross_origin
                .keys()
                .any(|k| k.eq_ignore_ascii_case("x-api-key"))
        );
        assert!(
            !cross_origin
                .keys()
                .any(|k| k.eq_ignore_ascii_case("proxy-authorization"))
        );
        assert_eq!(
            cross_origin.get("Referer").map(String::as_str),
            Some("https://music.example.com/")
        );
    }
}

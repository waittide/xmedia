use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Deserializer, Serialize};
use xmedia_source::crud::{ParsedSourceSummaryMeta, SourceSpecAdapter};
use xmedia_source::http::normalize_cookie_key;
pub use xmedia_source::http::proxy::has_origin_affinity;
pub use xmedia_source::model::{
    BrowserRedirectHint, ConcurrentRateSpec, ExecutionDiagnostics, LoginUiItem, LoginUiStyle,
    SourceCallError, SourceCallOutput, SourceMetadata, SourceRuntimeState, de_bool_true,
    de_i64_zero, de_opt_string, de_string_or_default, default_true, parse_concurrent_rate,
    parse_login_ui_json,
};

use crate::domain::catalog::{AudioFormat, StructuredLyrics};

/// Deserializes either `["128", "320", "flac"]`, `[128, 320]`, or `"128,320,flac"`.
pub fn de_string_list_or_csv<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let val = Option::<serde_json::Value>::deserialize(deserializer)?;
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut push_clean = |s: &str| {
        let t = s.trim();
        if !t.is_empty() && seen.insert(t.to_string()) {
            out.push(t.to_string());
        }
    };
    match val {
        None | Some(serde_json::Value::Null) => {}
        Some(serde_json::Value::Array(arr)) => {
            for item in arr {
                match item {
                    serde_json::Value::String(s) => push_clean(&s),
                    serde_json::Value::Number(n) => push_clean(&n.to_string()),
                    _ => {}
                }
            }
        }
        Some(serde_json::Value::String(s)) => {
            for part in s.split([',', ';', '|']) {
                push_clean(part);
            }
        }
        Some(serde_json::Value::Number(n)) => push_clean(&n.to_string()),
        Some(other) => push_clean(&other.to_string()),
    }
    Ok(out)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicSearchRule {
    #[serde(
        default,
        alias = "list",
        alias = "songList",
        deserialize_with = "de_opt_string"
    )]
    pub track_list: Option<String>,
    #[serde(default, alias = "id", deserialize_with = "de_opt_string")]
    pub track_id: Option<String>,
    #[serde(default, alias = "name", deserialize_with = "de_opt_string")]
    pub title: Option<String>,
    #[serde(
        default,
        alias = "author",
        alias = "singer",
        deserialize_with = "de_opt_string"
    )]
    pub artist: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub album: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub duration: Option<String>,
    /// Optional unit hint: `"s"` | `"ms"` | `"auto"` (default `"auto"`).
    #[serde(default, deserialize_with = "de_opt_string")]
    pub duration_unit: Option<String>,
    #[serde(default, alias = "picUrl", deserialize_with = "de_opt_string")]
    pub cover_url: Option<String>,
    #[serde(
        default,
        alias = "picId",
        alias = "pic_id",
        deserialize_with = "de_opt_string"
    )]
    pub cover_id: Option<String>,
    #[serde(default, alias = "url_id", deserialize_with = "de_opt_string")]
    pub url_id: Option<String>,
    #[serde(default, alias = "lyric_id", deserialize_with = "de_opt_string")]
    pub lyric_id: Option<String>,
    #[serde(default, alias = "streamUrl", deserialize_with = "de_opt_string")]
    pub play_url: Option<String>,
    #[serde(default, alias = "trackUrl", deserialize_with = "de_opt_string")]
    pub detail_url: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub format: Option<String>,
    #[serde(default, alias = "checkKeyword", deserialize_with = "de_opt_string")]
    pub check_key_word: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicTrackDetailRule {
    #[serde(default, alias = "url", deserialize_with = "de_opt_string")]
    pub request_url: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub init: Option<String>,
    #[serde(default, alias = "id", deserialize_with = "de_opt_string")]
    pub track_id: Option<String>,
    #[serde(default, alias = "name", deserialize_with = "de_opt_string")]
    pub title: Option<String>,
    #[serde(
        default,
        alias = "author",
        alias = "singer",
        deserialize_with = "de_opt_string"
    )]
    pub artist: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub album: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub duration: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub duration_unit: Option<String>,
    #[serde(default, alias = "picUrl", deserialize_with = "de_opt_string")]
    pub cover_url: Option<String>,
    #[serde(
        default,
        alias = "picId",
        alias = "pic_id",
        deserialize_with = "de_opt_string"
    )]
    pub cover_id: Option<String>,
    #[serde(default, alias = "url_id", deserialize_with = "de_opt_string")]
    pub url_id: Option<String>,
    #[serde(default, alias = "lyric_id", deserialize_with = "de_opt_string")]
    pub lyric_id: Option<String>,
    #[serde(default, alias = "streamUrl", deserialize_with = "de_opt_string")]
    pub play_url: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub lyric: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub format: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicStreamRule {
    #[serde(default, alias = "url", deserialize_with = "de_opt_string")]
    pub request_url: Option<String>,
    #[serde(
        default,
        alias = "urlRule",
        alias = "playUrl",
        deserialize_with = "de_opt_string"
    )]
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
    #[serde(default, alias = "url", deserialize_with = "de_opt_string")]
    pub request_url: Option<String>,
    #[serde(default, alias = "urlRule", deserialize_with = "de_opt_string")]
    pub cover_url: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicLyricRule {
    #[serde(default, alias = "url", deserialize_with = "de_opt_string")]
    pub request_url: Option<String>,
    #[serde(default, alias = "syncedLyrics", deserialize_with = "de_opt_string")]
    pub lyric: Option<String>,
    #[serde(default, alias = "tlyric", deserialize_with = "de_opt_string")]
    pub translated_lyric: Option<String>,
    #[serde(default, alias = "plainLyrics", deserialize_with = "de_opt_string")]
    pub plain_lyric: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicSourceSpec {
    #[serde(default, alias = "name", deserialize_with = "de_string_or_default")]
    pub source_name: String,
    #[serde(default, alias = "url", deserialize_with = "de_string_or_default")]
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
    #[serde(default, alias = "Header", deserialize_with = "de_opt_string")]
    pub header: Option<String>,
    #[serde(default, alias = "MediaHeader", deserialize_with = "de_opt_string")]
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

impl SourceSpecAdapter for MusicSourceSpec {
    const ENTITY_LABEL: &'static str = "音源";
    const JSON_GROUP_KEY: &'static str = "sourceGroup";

    fn parse_batch(raw_json: &str) -> Result<Vec<(Self, String)>, String> {
        let parsed = Self::parse_json_batch(raw_json)?;
        Ok(parsed
            .into_iter()
            .map(|(spec, item_raw)| {
                let group = spec.source_group.as_deref().map(str::trim).unwrap_or("");
                let synced = xmedia_source::crud::sync_raw_json_fields(
                    &item_raw,
                    None,
                    None,
                    Some(group),
                    Self::JSON_GROUP_KEY,
                );
                (spec, synced)
            })
            .collect())
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
        let login_ui = self.parse_login_ui();
        let has_login_url = self
            .login_url
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty())
            || !login_ui.is_empty();
        let check_keyword = self
            .rule_search
            .as_ref()
            .and_then(|r| r.check_key_word.as_deref())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);
        ParsedSourceSummaryMeta {
            comment: self.source_comment.clone(),
            check_keyword,
            has_login_url,
            login_ui,
        }
    }
}

impl MusicSourceSpec {
    /// Parses either a single MusicSource JSON object or a JSON array of music sources.
    pub fn parse_json_batch(raw: &str) -> Result<Vec<(Self, String)>, String> {
        xmedia_source::crud::parse_json_batch(
            raw,
            "音乐源",
            "未解析到有效音乐源（缺少 sourceName / sourceUrl）",
            |parsed: &Self| {
                parsed.source_name.trim().is_empty() && parsed.source_url.trim().is_empty()
            },
        )
    }

    pub fn parse_login_ui(&self) -> Vec<LoginUiItem> {
        parse_login_ui_json(self.login_ui.as_deref())
    }

    pub fn parse_concurrent_rate(&self) -> Option<ConcurrentRateSpec> {
        parse_concurrent_rate(self.concurrent_rate.as_deref())
    }

    pub fn parse_header_map(raw: Option<&str>) -> HashMap<String, String> {
        let mut out = HashMap::new();
        let Some(s) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
            return out;
        };
        if let Ok(serde_json::Value::Object(map)) = serde_json::from_str(s) {
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

    /// Merges `header` and `mediaHeader` (`mediaHeader` takes precedence on conflicts).
    pub fn parse_media_headers(&self) -> HashMap<String, String> {
        let mut merged = Self::parse_header_map(self.header.as_deref());
        for (k, v) in Self::parse_header_map(self.media_header.as_deref()) {
            merged.retain(|existing_k, _| !existing_k.eq_ignore_ascii_case(&k));
            merged.insert(k, v);
        }
        merged
    }

    /// Merges `header` and `mediaHeader`, stripping sensitive static credentials (`authorization`,
    /// `cookie`, `x-api-key`, `proxy-authorization`) when `target_url` does not match `source_url`
    /// by host or registrable domain (eTLD+1).
    pub fn parse_media_headers_for_target(&self, target_url: &str) -> HashMap<String, String> {
        let mut merged = self.parse_media_headers();
        strip_cross_origin_sensitive_headers(&mut merged, &self.source_url, target_url);
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
        merged.retain(|k, _| !k.eq_ignore_ascii_case("range"));
        if !merged.keys().any(|k| k.eq_ignore_ascii_case("cookie"))
            && self.enabled_cookie_jar
            && let Some(cookie) = lookup_cookie_for_url(cookies, target_url)
        {
            merged.insert("Cookie".to_string(), cookie);
        }
        merged
    }
}

pub fn lookup_cookie_for_url(
    cookies: &HashMap<String, String>,
    target_url: &str,
) -> Option<String> {
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
        if key.len() > clean_stored.len()
            && key.ends_with(clean_stored)
            && key.as_bytes()[key.len() - clean_stored.len() - 1] == b'.'
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

/// Percent-encodes a query/text component (UTF-8 bytes outside RFC 3986 unreserved set).
pub fn url_encode_component(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() * 3);
    for b in raw.as_bytes() {
        let ch = *b;
        if ch.is_ascii_alphanumeric() || matches!(ch, b'-' | b'_' | b'.' | b'~') {
            out.push(ch as char);
        } else {
            out.push('%');
            out.push(hex_upper(ch >> 4));
            out.push(hex_upper(ch & 0x0F));
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
        let parts: Vec<&str> = trimmed.split(':').collect();
        match parts.len() {
            2 => {
                let mins = parts[0].trim().parse::<f64>().ok()?;
                let secs = parts[1].trim().parse::<f64>().ok()?;
                if mins < 0.0 || secs < 0.0 {
                    return None;
                }
                return Some((mins * 60.0 + secs).round() as u32);
            }
            3 => {
                let hrs = parts[0].trim().parse::<f64>().ok()?;
                let mins = parts[1].trim().parse::<f64>().ok()?;
                let secs = parts[2].trim().parse::<f64>().ok()?;
                if hrs < 0.0 || mins < 0.0 || secs < 0.0 {
                    return None;
                }
                return Some((hrs * 3600.0 + mins * 60.0 + secs).round() as u32);
            }
            _ => return None,
        }
    }
    let val = trimmed.parse::<f64>().ok()?;
    if !val.is_finite() || val < 0.0 {
        return None;
    }
    let unit_norm = unit
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_ascii_lowercase);
    let secs = match unit_norm.as_deref() {
        Some("ms") => val / 1000.0,
        Some("s") | Some("sec") | Some("secs") => val,
        _ => {
            if val >= 36_000.0 {
                val / 1000.0
            } else {
                val
            }
        }
    };
    Some(secs.round() as u32)
}

/// Infers audio format from URL path extension (`flac`, `mp3`, `m4a`, `alac`, `aac`, `ogg`, `opus`, `wav`, `aiff`, `ape`, `wv`).
pub fn infer_audio_format(url: &str) -> Option<String> {
    let without_frag = url.split('#').next().unwrap_or(url);
    let without_query = without_frag.split('?').next().unwrap_or(without_frag);
    let last_seg = without_query.rsplit('/').next().unwrap_or(without_query);
    let ext = last_seg.rsplit_once('.')?.1;
    AudioFormat::from_extension(ext).map(|fmt| fmt.as_str().to_string())
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
    #[serde(alias = "track_info")]
    Detail,
    Stream,
    #[serde(alias = "lyrics")]
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

        let same_domain =
            spec.parse_media_headers_for_target("https://audio-cdn.music.example.com/track.flac");
        assert_eq!(
            same_domain.get("Authorization").map(String::as_str),
            Some("Bearer mus-secret")
        );
        assert_eq!(same_domain.get("Cookie").map(String::as_str), Some("u=42"));

        let cross_origin =
            spec.parse_media_headers_for_target("https://thirdparty-storage.net/track.flac");
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

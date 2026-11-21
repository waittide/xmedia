use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Deserializer, Serialize};

pub use xmedia_source::model::{
    BrowserRedirectHint, ConcurrentRateSpec, ExecutionDiagnostics, LoginUiItem, LoginUiStyle,
    SourceCallError, SourceCallOutput, SourceMetadata, SourceRuntimeState, de_bool_true,
    de_i64_zero, de_opt_string, de_string_or_default, default_true, parse_concurrent_rate,
    parse_login_ui_json,
};

use crate::domain::catalog::StructuredLyrics;

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
                let val_str = match v {
                    serde_json::Value::String(s) => s,
                    serde_json::Value::Null => continue,
                    other => other.to_string(),
                };
                out.insert(k, val_str);
            }
        }
        out
    }

    /// Merges `header` and `mediaHeader` (`mediaHeader` takes precedence on conflicts).
    pub fn parse_media_headers(&self) -> HashMap<String, String> {
        let mut merged = Self::parse_header_map(self.header.as_deref());
        for (k, v) in Self::parse_header_map(self.media_header.as_deref()) {
            // Replace any case-insensitive duplicate key so mediaHeader wins cleanly.
            merged.retain(|existing_k, _| !existing_k.eq_ignore_ascii_case(&k));
            merged.insert(k, v);
        }
        merged
    }
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

use std::collections::HashMap;

use serde::{Deserialize, Deserializer, Serialize};
use xmedia_source::http::is_forbidden_custom_header;

pub use xmedia_source::model::{
    BrowserRedirectHint, ConcurrentRateSpec, ExecutionDiagnostics, LoginUiItem, LoginUiStyle,
    SourceCallError, SourceCallOutput, SourceMetadata, SourceRuntimeState, de_bool_true,
    de_i64_zero, de_opt_string, de_string_or_default, default_true, parse_concurrent_rate,
    parse_login_ui_json,
};

use crate::domain::series::ReadingDirection;

fn default_comic_source_type() -> i64 {
    2
}

fn de_comic_source_type<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: Deserializer<'de>,
{
    let val = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(match val {
        None | Some(serde_json::Value::Null) => 2,
        Some(serde_json::Value::Number(n)) => n.as_i64().unwrap_or(2),
        Some(serde_json::Value::String(s)) => {
            let trimmed = s.trim().to_ascii_lowercase();
            if trimmed.is_empty() || trimmed == "comic" || trimmed == "manga" || trimmed == "image"
            {
                2
            } else {
                trimmed.parse::<i64>().unwrap_or(2)
            }
        }
        _ => 2,
    })
}

pub fn parse_reading_direction_opt(raw: &str) -> Option<ReadingDirection> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "rtl" => Some(ReadingDirection::Rtl),
        "ltr" => Some(ReadingDirection::Ltr),
        "webtoon" | "vertical" | "scroll" => Some(ReadingDirection::Webtoon),
        _ => None,
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComicSearchRule {
    #[serde(
        default,
        alias = "comicList",
        alias = "list",
        deserialize_with = "de_opt_string"
    )]
    pub book_list: Option<String>,
    #[serde(default, alias = "title", deserialize_with = "de_opt_string")]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub author: Option<String>,
    #[serde(
        default,
        alias = "comicUrl",
        alias = "url",
        deserialize_with = "de_opt_string"
    )]
    pub book_url: Option<String>,
    #[serde(default, alias = "cover", deserialize_with = "de_opt_string")]
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
pub struct ComicDetailRule {
    #[serde(default, deserialize_with = "de_opt_string")]
    pub init: Option<String>,
    #[serde(default, alias = "title", deserialize_with = "de_opt_string")]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub author: Option<String>,
    #[serde(default, alias = "cover", deserialize_with = "de_opt_string")]
    pub cover_url: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub intro: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub kind: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub status: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub last_chapter: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub word_count: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub reading_direction: Option<String>,
    #[serde(default, alias = "chaptersUrl", deserialize_with = "de_opt_string")]
    pub toc_url: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub can_re_name: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComicTocRule {
    #[serde(default, deserialize_with = "de_opt_string")]
    pub pre_update_js: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub chapter_list: Option<String>,
    #[serde(
        default,
        alias = "name",
        alias = "title",
        deserialize_with = "de_opt_string"
    )]
    pub chapter_name: Option<String>,
    #[serde(default, alias = "url", deserialize_with = "de_opt_string")]
    pub chapter_url: Option<String>,
    #[serde(default, alias = "nextChaptersUrl", deserialize_with = "de_opt_string")]
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
pub struct ComicPagesRule {
    #[serde(
        default,
        alias = "pages",
        alias = "images",
        deserialize_with = "de_opt_string"
    )]
    pub content: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub title: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub image_url: Option<String>,
    #[serde(default, alias = "nextPageUrl", deserialize_with = "de_opt_string")]
    pub next_content_url: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub replace_regex: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub source_regex: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub web_js: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub image_header: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComicSourceSpec {
    #[serde(
        default,
        alias = "bookSourceName",
        deserialize_with = "de_string_or_default"
    )]
    pub source_name: String,
    #[serde(
        default,
        alias = "bookSourceUrl",
        deserialize_with = "de_string_or_default"
    )]
    pub source_url: String,
    #[serde(default, alias = "bookSourceGroup", deserialize_with = "de_opt_string")]
    pub source_group: Option<String>,
    #[serde(
        default,
        alias = "bookSourceComment",
        deserialize_with = "de_opt_string"
    )]
    pub source_comment: Option<String>,
    #[serde(
        default = "default_comic_source_type",
        alias = "bookSourceType",
        deserialize_with = "de_comic_source_type"
    )]
    pub source_type: i64,
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
    pub image_header: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub reading_direction: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub search_url: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub explore_url: Option<String>,
    #[serde(default, alias = "bookUrlPattern", deserialize_with = "de_opt_string")]
    pub comic_url_pattern: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub login_url: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub login_ui: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub login_check_js: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub js_lib: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string")]
    pub concurrent_rate: Option<String>,
    #[serde(default)]
    pub rule_search: Option<ComicSearchRule>,
    #[serde(default)]
    pub rule_explore: Option<ComicSearchRule>,
    #[serde(default, alias = "ruleBookInfo")]
    pub rule_detail: Option<ComicDetailRule>,
    #[serde(default, alias = "ruleChapters")]
    pub rule_toc: Option<ComicTocRule>,
    #[serde(default, alias = "ruleContent")]
    pub rule_pages: Option<ComicPagesRule>,
}

impl From<&ComicSourceSpec> for SourceMetadata {
    fn from(src: &ComicSourceSpec) -> Self {
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

fn overlay_header_json(target: &mut HashMap<String, String>, raw: Option<&str>) {
    let Some(text) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return;
    };
    let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(text) else {
        return;
    };
    for (k, v) in map {
        let key_trimmed = k.trim();
        if key_trimmed.is_empty() || is_forbidden_custom_header(key_trimmed) {
            continue;
        }
        let val_str = match v {
            serde_json::Value::String(s) => s,
            serde_json::Value::Null => continue,
            other => other.to_string(),
        };
        let existing_keys: Vec<String> = target
            .keys()
            .filter(|existing| existing.eq_ignore_ascii_case(key_trimmed))
            .cloned()
            .collect();
        for old in existing_keys {
            target.remove(&old);
        }
        target.insert(key_trimmed.to_string(), val_str);
    }
}

impl ComicSourceSpec {
    /// Parses either a single JSON object or a JSON array of comic sources.
    pub fn parse_json_batch(raw: &str) -> Result<Vec<(Self, String)>, String> {
        xmedia_source::crud::parse_json_batch(
            raw,
            "漫画源",
            "未解析到有效漫画源（缺少 sourceName / sourceUrl）",
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

    pub fn effective_reading_direction(&self) -> ReadingDirection {
        self.reading_direction
            .as_deref()
            .and_then(parse_reading_direction_opt)
            .unwrap_or(ReadingDirection::Rtl)
    }

    /// Merges `header`, `imageHeader`, and `rulePages.imageHeader`, stripping forbidden hop-by-hop headers.
    pub fn merged_image_headers(&self) -> HashMap<String, String> {
        let mut out = HashMap::new();
        overlay_header_json(&mut out, self.header.as_deref());
        overlay_header_json(&mut out, self.image_header.as_deref());
        if let Some(ref rp) = self.rule_pages {
            overlay_header_json(&mut out, rp.image_header.as_deref());
        }
        out
    }
}

/// Context about the current comic passed to JS rules (`book` & `comic` global objects).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ComicJsContext {
    #[serde(alias = "title")]
    pub name: String,
    pub author: String,
    #[serde(alias = "comic_url")]
    pub book_url: String,
    pub toc_url: String,
    pub cover_url: Option<String>,
    pub intro: Option<String>,
    pub reading_direction: Option<String>,
    pub dur_chapter_index: usize,
    pub dur_chapter_title: String,
    pub custom_variable: String,
}

/// Context about the current chapter passed to JS rules (`chapter` global object).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ComicChapterJsContext {
    pub index: usize,
    pub title: String,
    pub url: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExploreCategoryItem {
    pub title: String,
    pub url: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ComicSearchResult {
    pub title: String,
    pub author: String,
    pub comic_url: String,
    pub cover_url: Option<String>,
    pub intro: Option<String>,
    pub kind: Option<String>,
    pub last_chapter: Option<String>,
    pub word_count: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OnlineComicDetail {
    pub title: String,
    pub author: String,
    pub comic_url: String,
    pub toc_url: String,
    pub cover_url: Option<String>,
    pub intro: Option<String>,
    pub kind: Option<String>,
    pub status: Option<String>,
    pub last_chapter: Option<String>,
    pub word_count: Option<String>,
    pub reading_direction: Option<ReadingDirection>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OnlineComicChapter {
    pub index: usize,
    pub title: String,
    pub url: String,
    pub update_time: Option<String>,
    pub is_volume: bool,
}

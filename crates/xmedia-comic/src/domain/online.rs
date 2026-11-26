use std::borrow::Cow;
use std::collections::HashMap;

use serde::{Deserialize, Deserializer, Serialize};
use xmedia_source::crud::{ParsedSourceSummaryMeta, SourceSpecAdapter};
use xmedia_source::http::is_forbidden_custom_header;
pub use xmedia_source::http::proxy::{has_origin_affinity, strip_cross_origin_headers};
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComicSourceTestMode {
    #[default]
    Full,
    Search,
    Explore,
    #[serde(alias = "book_info")]
    Detail,
    #[serde(alias = "toc")]
    Chapters,
    #[serde(alias = "content")]
    Pages,
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

fn canonicalize_group_key_in_value(val: &mut serde_json::Value) {
    match val {
        serde_json::Value::Array(arr) => {
            for item in arr {
                canonicalize_group_key_in_value(item);
            }
        }
        serde_json::Value::Object(obj) => {
            if let Some(legacy_group) = obj.remove("bookSourceGroup")
                && !obj.contains_key("sourceGroup")
            {
                obj.insert("sourceGroup".into(), legacy_group);
            }
        }
        _ => {}
    }
}

impl ComicSourceSpec {
    fn is_blank(&self) -> bool {
        self.source_name.trim().is_empty() && self.source_url.trim().is_empty()
    }

    pub fn default_test_keyword(&self) -> String {
        self.rule_search
            .as_ref()
            .and_then(|r| r.check_key_word.as_deref())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("海贼王")
            .to_string()
    }

    pub fn supports_stage(&self, mode: ComicSourceTestMode) -> bool {
        match mode {
            ComicSourceTestMode::Detail => self.rule_detail.is_some(),
            ComicSourceTestMode::Chapters => self
                .rule_toc
                .as_ref()
                .and_then(|t| t.chapter_list.as_deref())
                .is_some_and(|s| !s.trim().is_empty()),
            ComicSourceTestMode::Search => self.rule_search.is_some(),
            ComicSourceTestMode::Explore => {
                self.rule_explore.is_some() || self.rule_search.is_some()
            }
            ComicSourceTestMode::Pages => self.rule_pages.is_some(),
            ComicSourceTestMode::Full => true,
        }
    }

    pub fn list_rules(
        &self,
        explore: bool,
    ) -> Result<(Option<&ComicSearchRule>, Option<&ComicSearchRule>), SourceCallError> {
        if explore {
            if self.rule_explore.is_none() && self.rule_search.is_none() {
                return Err(SourceCallError::new(
                    format!(
                        "漫画源「{}」未配置 ruleExplore 或 ruleSearch",
                        self.source_name
                    ),
                    ExecutionDiagnostics::default(),
                ));
            }
            Ok((self.rule_explore.as_ref(), self.rule_search.as_ref()))
        } else {
            let rule = self.rule_search.as_ref().ok_or_else(|| {
                SourceCallError::new(
                    format!("漫画源「{}」未配置 ruleSearch", self.source_name),
                    ExecutionDiagnostics::default(),
                )
            })?;
            Ok((Some(rule), None))
        }
    }

    pub fn detail_rule_or_fallback(
        &self,
        comic_url: &str,
        final_url: &str,
        fallback_hint: Option<&ComicJsContext>,
    ) -> Result<&ComicDetailRule, Box<OnlineComicDetail>> {
        self.rule_detail.as_ref().ok_or_else(|| {
            Box::new(OnlineComicDetail {
                title: fallback_hint.map(|h| h.name.clone()).unwrap_or_default(),
                author: fallback_hint.map(|h| h.author.clone()).unwrap_or_default(),
                comic_url: comic_url.to_string(),
                toc_url: final_url.to_string(),
                cover_url: fallback_hint.and_then(|h| h.cover_url.clone()),
                intro: fallback_hint.and_then(|h| h.intro.clone()),
                kind: None,
                status: None,
                last_chapter: None,
                word_count: None,
                reading_direction: Some(self.effective_reading_direction()),
            })
        })
    }
}

impl SourceSpecAdapter for ComicSourceSpec {
    const ENTITY_LABEL: &'static str = "漫画源";
    const JSON_GROUP_KEY: &'static str = "sourceGroup";

    fn parse_batch(raw_json: &str) -> Result<Vec<(Self, String)>, String> {
        let trimmed = raw_json.trim().trim_start_matches('\u{feff}').trim();
        let normalized_cow: Cow<'_, str> = if trimmed.contains("bookSourceGroup")
            && let Ok(mut parsed_val) = serde_json::from_str::<serde_json::Value>(trimmed)
        {
            canonicalize_group_key_in_value(&mut parsed_val);
            match serde_json::to_string(&parsed_val) {
                Ok(s) => Cow::Owned(s),
                Err(_) => Cow::Borrowed(raw_json),
            }
        } else {
            Cow::Borrowed(raw_json)
        };
        xmedia_source::crud::parse_json_batch(
            &normalized_cow,
            Self::ENTITY_LABEL,
            "未解析到有效漫画源（缺少 sourceName / sourceUrl）",
            Self::is_blank,
        )
    }

    fn source_name(&self) -> &str {
        &self.source_name
    }

    fn effective_source_url(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.source_url)
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
        self.source_type
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

const DEFAULT_IMAGE_ACCEPT: &str = "image/avif,image/webp,image/apng,image/*,*/*;q=0.8";

fn has_header_case_insensitive(map: &HashMap<String, String>, target: &str) -> bool {
    map.keys().any(|k| k.eq_ignore_ascii_case(target))
}

fn remove_header_case_insensitive(map: &mut HashMap<String, String>, target: &str) {
    let matching: Vec<String> = map
        .keys()
        .filter(|k| k.eq_ignore_ascii_case(target))
        .cloned()
        .collect();
    for k in matching {
        map.remove(&k);
    }
}

impl ComicSourceSpec {
    fn parse_login_ui(&self) -> Vec<LoginUiItem> {
        parse_login_ui_json(self.login_ui.as_deref())
    }

    pub fn parse_concurrent_rate(&self) -> Option<ConcurrentRateSpec> {
        parse_concurrent_rate(self.concurrent_rate.as_deref())
    }

    pub fn effective_reading_direction(&self) -> ReadingDirection {
        self.reading_direction
            .as_deref()
            .and_then(|s| s.parse().ok())
            .unwrap_or_default()
    }

    pub fn image_headers(&self, target_url: Option<&str>) -> HashMap<String, String> {
        let mut out = self.base_image_headers();
        if let Some(url) = target_url {
            strip_cross_origin_headers(&mut out, &self.source_url, url);
        }
        out
    }

    fn base_image_headers(&self) -> HashMap<String, String> {
        let mut out = HashMap::new();
        overlay_header_json(&mut out, self.header.as_deref());
        let had_base_accept = has_header_case_insensitive(&out, "accept");

        let mut img_override = HashMap::new();
        overlay_header_json(&mut img_override, self.image_header.as_deref());
        if let Some(ref rp) = self.rule_pages {
            overlay_header_json(&mut img_override, rp.image_header.as_deref());
        }
        let has_image_accept = has_header_case_insensitive(&img_override, "accept");

        for (k, v) in img_override {
            remove_header_case_insensitive(&mut out, &k);
            out.insert(k, v);
        }

        if had_base_accept && !has_image_accept {
            remove_header_case_insensitive(&mut out, "accept");
            out.insert("Accept".to_string(), DEFAULT_IMAGE_ACCEPT.to_string());
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
pub struct OnlineComicItem {
    pub title: String,
    pub author: String,
    pub comic_url: String,
    pub cover_url: Option<String>,
    pub intro: Option<String>,
    pub kind: Option<String>,
    pub last_chapter: Option<String>,
    pub word_count: Option<String>,
}

pub type ComicSearchResult = OnlineComicItem;

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

#[cfg(test)]
mod origin_affinity_tests {
    use super::*;

    #[test]
    fn merged_image_headers_for_target_strips_sensitive_headers_on_cross_origin() {
        let spec = ComicSourceSpec {
            source_name: "AuthComic".into(),
            source_url: "https://api.manga-site.co.uk".into(),
            header: Some(
                r#"{"Authorization":"Bearer secret-tok","Cookie":"sid=123","X-Api-Key":"k99","Referer":"https://manga-site.co.uk/"}"#
                    .into(),
            ),
            ..Default::default()
        };

        let same_site = spec.image_headers(Some("https://img.cdn.manga-site.co.uk/1.jpg"));
        assert_eq!(
            same_site.get("Authorization").map(String::as_str),
            Some("Bearer secret-tok")
        );
        assert_eq!(same_site.get("Cookie").map(String::as_str), Some("sid=123"));
        assert_eq!(same_site.get("X-Api-Key").map(String::as_str), Some("k99"));

        for cross_url in [
            "https://evil-collector.co.uk/1.jpg",
            "https://attacker.example.org/steal.png",
            "http://192.0.2.10/1.jpg",
        ] {
            let filtered = spec.image_headers(Some(cross_url));
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
                Some("https://manga-site.co.uk/")
            );
        }
    }
}

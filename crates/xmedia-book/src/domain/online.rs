use std::borrow::Cow;

use serde::{Deserialize, Serialize};

pub use xmedia_formats::book::epub_write::{PackBookMetadata, PackChapterInput};
use xmedia_source::crud::{ParsedSourceSummaryMeta, SourceSpecAdapter};
pub use xmedia_source::model::{
    BrowserRedirectHint, ConcurrentRateSpec, ExecutionDiagnostics, LoginUiItem, LoginUiStyle,
    SourceCallError, SourceCallOutput, SourceMetadata, SourceRuntimeState, de_bool_true,
    de_i64_zero, de_opt_string, de_string_or_default, default_true, parse_concurrent_rate,
    parse_login_ui_json,
};

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
    #[serde(default, deserialize_with = "de_opt_string")]
    pub concurrent_rate: Option<String>,
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

impl From<&LegadoBookSource> for SourceMetadata {
    fn from(src: &LegadoBookSource) -> Self {
        Self {
            source_name: src.book_source_name.clone(),
            source_url: src.book_source_url.clone(),
            header: src.header.clone(),
            login_url: src.login_url.clone(),
            login_ui: src.login_ui.clone(),
            js_lib: src.js_lib.clone(),
            enabled_cookie_jar: src.enabled_cookie_jar,
            raw_json: None,
        }
    }
}

impl LegadoBookSource {
    pub fn default_test_keyword(&self) -> String {
        self.rule_search
            .as_ref()
            .and_then(|r| r.check_key_word.as_deref())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("我的")
            .to_string()
    }

    pub fn require_search_rule(&self) -> Result<&LegadoSearchRule, SourceCallError> {
        self.rule_search.as_ref().ok_or_else(|| {
            SourceCallError::new(
                format!("书源「{}」未配置 ruleSearch", self.book_source_name),
                ExecutionDiagnostics::default(),
            )
        })
    }

    pub fn book_info_or_fallback(
        &self,
        book_url: &str,
        final_url: &str,
        fallback_hint: Option<&BookJsContext>,
    ) -> Result<&LegadoBookInfoRule, Box<OnlineBookDetail>> {
        self.rule_book_info.as_ref().ok_or_else(|| {
            Box::new(OnlineBookDetail {
                name: fallback_hint.map(|h| h.name.clone()).unwrap_or_default(),
                author: fallback_hint.map(|h| h.author.clone()).unwrap_or_default(),
                book_url: book_url.to_string(),
                toc_url: final_url.to_string(),
                cover_url: fallback_hint.and_then(|h| h.cover_url.clone()),
                intro: fallback_hint.and_then(|h| h.intro.clone()),
                kind: None,
                last_chapter: None,
                word_count: None,
            })
        })
    }
    fn is_blank(&self) -> bool {
        self.book_source_name.trim().is_empty() && self.book_source_url.trim().is_empty()
    }

    fn parse_login_ui(&self) -> Vec<LoginUiItem> {
        parse_login_ui_json(self.login_ui.as_deref())
    }

    /// Parses Legado `concurrentRate` into a rate-limit specification.
    pub fn parse_concurrent_rate(&self) -> Option<ConcurrentRateSpec> {
        parse_concurrent_rate(self.concurrent_rate.as_deref())
    }

    /// Builds sanitized image headers for `target_url`, stripping cross-origin credentials
    /// and attaching same-site cookie-jar entries when `enabled_cookie_jar` is true.
    pub fn build_image_headers_with_cookies(
        &self,
        cookies: &std::collections::HashMap<String, String>,
        target_url: &str,
    ) -> std::collections::HashMap<String, String> {
        let mut out: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        if let Some(raw) = self
            .header
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            && let Ok(serde_json::Value::Object(map)) = serde_json::from_str(raw)
        {
            for (k, v) in map {
                if k.eq_ignore_ascii_case("range") {
                    continue;
                }
                let val_str = match v {
                    serde_json::Value::String(s) => s,
                    other => other.to_string(),
                };
                out.retain(|existing, _| !existing.eq_ignore_ascii_case(&k));
                out.insert(k, val_str);
            }
        }
        xmedia_source::proxy::strip_cross_origin_headers(
            &mut out,
            &self.book_source_url,
            target_url,
        );
        if !out.keys().any(|k| k.eq_ignore_ascii_case("cookie")) && self.enabled_cookie_jar {
            let ck = xmedia_source::http::lookup_cookie_in_jar(cookies, target_url);
            if !ck.is_empty() {
                out.insert("Cookie".to_string(), ck);
            }
        }
        out
    }

    /// Returns `true` when this Legado book source uses `data:` pseudo-URLs (e.g.
    /// `data:;base64,...,{"type":"gydetail"}`) to pass structured state between stages.
    pub fn supports_data_pseudo_urls(&self) -> bool {
        has_data_marker(self.search_url.as_deref())
            || has_data_marker(self.explore_url.as_deref())
            || has_data_marker(self.js_lib.as_deref())
            || search_rule_has_data(self.rule_search.as_ref())
            || search_rule_has_data(self.rule_explore.as_ref())
            || info_rule_has_data(self.rule_book_info.as_ref())
            || toc_rule_has_data(self.rule_toc.as_ref())
            || content_rule_has_data(self.rule_content.as_ref())
    }
}

fn has_data_marker(opt: Option<&str>) -> bool {
    let Some(raw) = opt else {
        return false;
    };
    let lower = raw.to_ascii_lowercase();
    lower.contains("data:") || lower.contains("hexdecodetostring")
}

fn search_rule_has_data(rule: Option<&LegadoSearchRule>) -> bool {
    rule.is_some_and(|r| {
        has_data_marker(r.book_list.as_deref()) || has_data_marker(r.book_url.as_deref())
    })
}

fn info_rule_has_data(rule: Option<&LegadoBookInfoRule>) -> bool {
    rule.is_some_and(|r| {
        has_data_marker(r.init.as_deref()) || has_data_marker(r.toc_url.as_deref())
    })
}

fn toc_rule_has_data(rule: Option<&LegadoTocRule>) -> bool {
    rule.is_some_and(|r| {
        has_data_marker(r.chapter_list.as_deref()) || has_data_marker(r.chapter_url.as_deref())
    })
}

fn content_rule_has_data(rule: Option<&LegadoContentRule>) -> bool {
    rule.is_some_and(|r| {
        has_data_marker(r.content.as_deref()) || has_data_marker(r.title.as_deref())
    })
}

fn is_windows_reserved_stem(stem: &str) -> bool {
    matches!(
        stem.to_ascii_uppercase().as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}

fn replace_illegal_stem_chars(name: &str) -> String {
    name.trim()
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect()
}

pub fn sanitize_novel_stem(name: &str) -> String {
    let cleaned = replace_illegal_stem_chars(name);
    let trimmed = cleaned.trim_matches('.').trim();
    if trimmed.is_empty() {
        return "novel".to_string();
    }
    let truncated: String = trimmed.chars().take(80).collect();
    if is_windows_reserved_stem(&truncated) {
        format!("_{truncated}")
    } else {
        truncated
    }
}

impl SourceSpecAdapter for LegadoBookSource {
    const MEDIA_KIND: xmedia_core::domain::library::MediaKind =
        xmedia_core::domain::library::MediaKind::Book;
    const ENTITY_LABEL: &'static str = "书籍插件";
    const JSON_GROUP_KEY: &'static str = "bookSourceGroup";

    fn parse_batch(raw_json: &str) -> Result<Vec<(Self, String)>, String> {
        xmedia_source::crud::parse_json_batch(
            raw_json,
            Self::ENTITY_LABEL,
            "未解析到有效书源（缺少 bookSourceName / bookSourceUrl）",
            Self::is_blank,
        )
    }

    fn source_name(&self) -> &str {
        &self.book_source_name
    }

    fn effective_source_url(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.book_source_url)
    }

    fn group_name(&self) -> Option<&str> {
        self.book_source_group.as_deref()
    }

    fn enabled(&self) -> bool {
        self.enabled
    }

    fn weight(&self) -> i64 {
        self.weight
    }

    fn source_type_code(&self) -> i64 {
        self.book_source_type
    }

    fn summary_meta(&self) -> ParsedSourceSummaryMeta {
        let login_ui = self.parse_login_ui();
        let has_login_url = self
            .login_url
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty())
            || !login_ui.is_empty();
        let comment = self.book_source_comment.clone();
        let check_keyword = self
            .rule_search
            .as_ref()
            .and_then(|r| r.check_key_word.as_deref())
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
pub struct OnlineBookItem {
    pub name: String,
    pub author: String,
    pub book_url: String,
    pub cover_url: Option<String>,
    pub intro: Option<String>,
    pub kind: Option<String>,
    pub last_chapter: Option<String>,
    pub word_count: Option<String>,
}

pub type SearchBookResult = OnlineBookItem;

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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceTestMode {
    #[default]
    Full,
    Search,
    BookInfo,
    Toc,
    Content,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackedChapterMeta {
    pub index: usize,
    pub title: String,
    pub url: String,
    #[serde(default)]
    pub cached: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TrackedManifest {
    pub chapters: Vec<TrackedChapterMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackedChapterFile {
    pub index: usize,
    pub title: String,
    pub url: String,
    pub content: String,
}

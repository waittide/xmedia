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
    /// Parses either a single Legado JSON object or a JSON array of book sources.
    pub fn parse_json_batch(raw: &str) -> Result<Vec<(Self, String)>, String> {
        xmedia_source::crud::parse_json_batch(
            raw,
            "书源",
            "未解析到有效书源（缺少 bookSourceName / bookSourceUrl）",
            |parsed: &Self| {
                parsed.book_source_name.trim().is_empty()
                    && parsed.book_source_url.trim().is_empty()
            },
        )
    }

    pub fn parse_login_ui(&self) -> Vec<LoginUiItem> {
        parse_login_ui_json(self.login_ui.as_deref())
    }

    /// Parses Legado `concurrentRate` into a rate-limit specification.
    pub fn parse_concurrent_rate(&self) -> Option<ConcurrentRateSpec> {
        parse_concurrent_rate(self.concurrent_rate.as_deref())
    }

    /// Returns `true` when this Legado book source uses `data:` pseudo-URLs (e.g.
    /// `data:;base64,...,{"type":"gydetail"}`) to pass structured state between stages.
    pub fn supports_data_pseudo_urls(&self) -> bool {
        fn has_data_marker(opt: Option<&str>) -> bool {
            let Some(raw) = opt else {
                return false;
            };
            let lower = raw.to_ascii_lowercase();
            lower.contains("data:") || lower.contains("hexdecodetostring")
        }

        if has_data_marker(self.search_url.as_deref())
            || has_data_marker(self.explore_url.as_deref())
            || has_data_marker(self.js_lib.as_deref())
        {
            return true;
        }
        if let Some(ref r) = self.rule_search
            && (has_data_marker(r.book_list.as_deref()) || has_data_marker(r.book_url.as_deref()))
        {
            return true;
        }
        if let Some(ref r) = self.rule_explore
            && (has_data_marker(r.book_list.as_deref()) || has_data_marker(r.book_url.as_deref()))
        {
            return true;
        }
        if let Some(ref r) = self.rule_book_info
            && (has_data_marker(r.init.as_deref()) || has_data_marker(r.toc_url.as_deref()))
        {
            return true;
        }
        if let Some(ref r) = self.rule_toc
            && (has_data_marker(r.chapter_list.as_deref())
                || has_data_marker(r.chapter_url.as_deref()))
        {
            return true;
        }
        if let Some(ref r) = self.rule_content
            && (has_data_marker(r.content.as_deref()) || has_data_marker(r.title.as_deref()))
        {
            return true;
        }
        false
    }
}

impl SourceSpecAdapter for LegadoBookSource {
    const ENTITY_LABEL: &'static str = "书源";
    const JSON_GROUP_KEY: &'static str = "bookSourceGroup";

    fn parse_batch(raw_json: &str) -> Result<Vec<(Self, String)>, String> {
        Self::parse_json_batch(raw_json)
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

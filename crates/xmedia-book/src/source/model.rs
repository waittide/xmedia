use serde::{Deserialize, Serialize};

pub use xmedia_source::model::{
    BrowserRedirectHint, ConcurrentRateSpec, ExecutionDiagnostics, LoginUiItem, LoginUiStyle,
    SourceCallError, SourceCallOutput, SourceMetadata, de_bool_true, de_i64_zero, de_opt_string,
    de_string_or_default, default_true, parse_concurrent_rate, parse_login_ui_json,
};

pub use crate::domain::online::SourceRuntimeState;

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
pub struct SearchBookResult {
    pub name: String,
    pub author: String,
    pub book_url: String,
    pub cover_url: Option<String>,
    pub intro: Option<String>,
    pub kind: Option<String>,
    pub last_chapter: Option<String>,
    pub word_count: Option<String>,
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_concurrent_rate_supports_ratio_and_interval_formats() {
        let ratio: LegadoBookSource = serde_json::from_str(
            r#"{"bookSourceName":"A","bookSourceUrl":"https://a.test","concurrentRate":"10/1000"}"#,
        )
        .unwrap();
        assert_eq!(
            ratio.parse_concurrent_rate(),
            Some(ConcurrentRateSpec {
                max_concurrency: 10,
                window_requests: 10,
                window_ms: 1000,
            })
        );

        let interval: LegadoBookSource = serde_json::from_str(
            r#"{"bookSourceName":"B","bookSourceUrl":"https://b.test","concurrentRate":200}"#,
        )
        .unwrap();
        assert_eq!(
            interval.parse_concurrent_rate(),
            Some(ConcurrentRateSpec {
                max_concurrency: 1,
                window_requests: 1,
                window_ms: 200,
            })
        );

        let disabled: LegadoBookSource = serde_json::from_str(
            r#"{"bookSourceName":"C","bookSourceUrl":"https://c.test","concurrentRate":"0"}"#,
        )
        .unwrap();
        assert_eq!(disabled.parse_concurrent_rate(), None);

        let capped: LegadoBookSource = serde_json::from_str(
            r#"{"bookSourceName":"D","bookSourceUrl":"https://d.test","concurrentRate":"50/1000"}"#,
        )
        .unwrap();
        assert_eq!(
            capped.parse_concurrent_rate(),
            Some(ConcurrentRateSpec {
                max_concurrency: 16,
                window_requests: 50,
                window_ms: 1000,
            })
        );
    }
}

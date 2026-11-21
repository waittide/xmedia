use std::sync::LazyLock;

use async_trait::async_trait;
use encoding_rs::UTF_8;
use regex::Regex;
use serde::Deserialize;
use xmedia_core::domain::error::{DomainError, DomainResult};

use crate::domain::config::ProviderCredentials;
use crate::domain::ports::ScraperProviderPort;
use crate::domain::scraper::{
    ProviderDescriptor, ScrapeCandidate, ScrapeQuery, ScrapeTargetKind, ScrapedMetadata,
};
use crate::infra::scrapers::cn_novel_common::{
    NovelCandidateDraft, NovelMetadataDraft, build_browser_http_client, extract_digits_book_id,
    extract_meta_map, extract_numeric_id_from_query, is_antibot_or_captcha_html,
    normalize_cover_url, novel_candidate_from_detail, parse_year_and_date, rank_novel_candidates,
    read_response_text, strip_html_to_text, strip_inline_html,
};

pub struct K17Provider;

const K17_PUBLIC_APP_KEY: &str = "4037465544";

static RE_17K_H1: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)<h1>\s*<a[^>]*>(.*?)</a>\s*</h1>"#).unwrap());
static RE_17K_INTRO: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)<p[^>]+class="[^"]*intro[^"]*"[^>]*>(.*?)</p>"#).unwrap());
static RE_17K_CATEGORY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)作品类别：\s*</th>\s*<td[^>]*>\s*<a[^>]*>(.*?)</a>"#).unwrap()
});
static RE_17K_UPDATE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)<em>\s*更新\s*[:：]\s*([^<]+)</em>"#).unwrap());
static RE_17K_OG_TITLE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^《([^》]+)》\s*(.*?)著"#).unwrap());

#[derive(Debug, Deserialize, Default)]
struct K17SearchEnvelope {
    #[serde(default)]
    data: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct K17Channel {
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct K17BookItem {
    #[serde(default)]
    pub id: u64,
    #[serde(default)]
    pub book_name: String,
    #[serde(default)]
    pub author_name: String,
    #[serde(default)]
    pub intro: String,
    #[serde(default)]
    pub cover: String,
    #[serde(default, rename = "bookChannel")]
    pub book_channel: Option<K17Channel>,
    #[serde(default)]
    pub category_name_2: Option<String>,
    #[serde(default)]
    pub last_update_chapter_date: Option<serde_json::Value>,
}

pub(crate) fn map_k17_search_values(
    values: Vec<serde_json::Value>,
    query: &ScrapeQuery,
) -> Vec<ScrapeCandidate> {
    let drafts = values.into_iter().filter_map(|val| {
        let item: K17BookItem = serde_json::from_value(val).ok()?;
        if item.id == 0 || item.book_name.trim().is_empty() {
            return None;
        }
        let author = strip_inline_html(&item.author_name);
        let category = item
            .category_name_2
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| {
                item.book_channel
                    .as_ref()
                    .map(|c| c.name.as_str())
                    .filter(|s| !s.trim().is_empty())
            })
            .map(str::to_string);
        let (year, _) = match &item.last_update_chapter_date {
            Some(serde_json::Value::String(s)) => parse_year_and_date(s),
            Some(serde_json::Value::Number(n)) => n
                .as_i64()
                .and_then(|ms| {
                    let secs = if ms > 10_000_000_000 { ms / 1000 } else { ms };
                    jiff::Timestamp::from_second(secs)
                        .ok()
                        .map(|ts| i32::from(ts.to_zoned(jiff::tz::TimeZone::UTC).year()))
                })
                .map(|y| (Some(y), None))
                .unwrap_or((None, None)),
            _ => (None, None),
        };
        let summary = strip_html_to_text(&item.intro);
        Some(NovelCandidateDraft {
            external_id: item.id.to_string(),
            title: strip_inline_html(&item.book_name),
            author: (!author.is_empty()).then_some(author),
            category,
            year,
            summary: (!summary.is_empty()).then_some(summary),
            cover_url: normalize_cover_url(&item.cover),
            popularity_votes: 0,
        })
    });

    rank_novel_candidates("17k", "17K小说网", query, drafts, 12)
}

pub(crate) fn parse_k17_detail_html(book_id: &str, html: &str) -> DomainResult<ScrapedMetadata> {
    if is_antibot_or_captcha_html(html) {
        return Err(DomainError::NotFound(format!(
            "17K book '{book_id}' unavailable"
        )));
    }
    let meta = extract_meta_map(html);

    let mut og_title_parsed: Option<String> = None;
    let mut og_author_parsed: Option<String> = None;
    if let Some(og_t) = meta.get("og:title")
        && let Some(caps) = RE_17K_OG_TITLE.captures(og_t.trim())
    {
        og_title_parsed = Some(strip_inline_html(&caps[1]));
        let a = strip_inline_html(&caps[2]);
        if !a.is_empty() {
            og_author_parsed = Some(a);
        }
    }

    let title = RE_17K_H1
        .captures(html)
        .map(|c| strip_inline_html(&c[1]))
        .filter(|s| !s.is_empty())
        .or(og_title_parsed)
        .ok_or_else(|| DomainError::NotFound(format!("17K book '{book_id}' not found")))?;

    let author = meta
        .get("og:novel:author")
        .map(|s| strip_inline_html(s))
        .filter(|s| !s.is_empty())
        .or(og_author_parsed);

    let summary = RE_17K_INTRO
        .captures(html)
        .map(|c| strip_html_to_text(&c[1]))
        .filter(|s| !s.is_empty())
        .or_else(|| {
            meta.get("og:description")
                .map(|s| strip_html_to_text(s))
                .filter(|s| !s.is_empty())
        });

    let category = RE_17K_CATEGORY
        .captures(html)
        .map(|c| strip_inline_html(&c[1]))
        .filter(|s| !s.is_empty())
        .or_else(|| {
            meta.get("og:novel:category")
                .map(|s| strip_inline_html(s))
                .filter(|s| !s.is_empty())
        });

    let (year, published_date) = RE_17K_UPDATE
        .captures(html)
        .map(|c| parse_year_and_date(&c[1]))
        .unwrap_or((None, None));

    let cover_url = meta.get("og:image").and_then(|s| normalize_cover_url(s));

    Ok(NovelMetadataDraft {
        title,
        author,
        year,
        published_date,
        summary,
        genres: category.into_iter().collect(),
        cover_url,
        ..Default::default()
    }
    .into_metadata("17k", "17K小说网", book_id))
}

#[async_trait]
impl ScraperProviderPort for K17Provider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "17k".to_string(),
            name: "17K小说网".to_string(),
            description: "17K小说网公开搜索与作品详情书名、作者、封面、分类与简介刮削".to_string(),
            website: "https://www.17k.com".to_string(),
            supported_targets: vec![ScrapeTargetKind::Book],
            requires_api_key: false,
        }
    }

    async fn search(
        &self,
        query: &ScrapeQuery,
        creds: &ProviderCredentials,
    ) -> DomainResult<Vec<ScrapeCandidate>> {
        let kw = query.keyword.trim();
        if kw.is_empty() {
            return Ok(Vec::new());
        }

        if let Some(id) = extract_numeric_id_from_query(kw, &["17k.com/book/", "17k.com/list/"])
            && (kw.contains("17k.com") || id.len() >= 5)
            && let Ok(detail) = self
                .fetch_detail(&id, ScrapeTargetKind::Book, "zh-CN", creds)
                .await
            && let Some(cand) = novel_candidate_from_detail("17k", "17K小说网", id, detail)
        {
            return Ok(vec![cand]);
        }

        let client = build_browser_http_client(creds, true)?;
        let resp = client
            .get("https://api.ali.17k.com/v2/book/search")
            .query(&[
                ("app_key", K17_PUBLIC_APP_KEY),
                ("key", kw),
                ("page", "1"),
                ("num", "12"),
            ])
            .send()
            .await
            .map_err(|e| DomainError::Storage(format!("17K search failed: {e}")))?;
        if !resp.status().is_success() {
            return Ok(Vec::new());
        }
        let body = read_response_text(resp, UTF_8).await?;
        let env: K17SearchEnvelope = serde_json::from_str(&body).unwrap_or_default();
        Ok(map_k17_search_values(env.data, query))
    }

    async fn fetch_detail(
        &self,
        external_id: &str,
        _target: ScrapeTargetKind,
        _locale: &str,
        creds: &ProviderCredentials,
    ) -> DomainResult<ScrapedMetadata> {
        let book_id = extract_digits_book_id(external_id, "17K")?;
        let client = build_browser_http_client(creds, false)?;
        let url = format!("https://www.17k.com/book/{book_id}.html");
        let resp = client
            .get(&url)
            .header(reqwest::header::REFERER, "https://www.17k.com/")
            .send()
            .await
            .map_err(|e| DomainError::Storage(format!("17K detail failed: {e}")))?;
        if !resp.status().is_success() {
            return Err(DomainError::NotFound(format!(
                "17K book '{book_id}' returned HTTP {}",
                resp.status()
            )));
        }
        let html = read_response_text(resp, UTF_8).await?;
        parse_k17_detail_html(&book_id, &html)
    }
}

use std::sync::LazyLock;

use async_trait::async_trait;
use encoding_rs::GB18030;
use regex::Regex;
use xmedia_core::domain::error::{DomainError, DomainResult};

use super::common::{
    NovelCandidateDraft, NovelMetadataDraft, extract_digits_book_id, novel_candidate_from_detail,
    rank_novel_candidates,
};
use crate::application::ports::ScraperProviderPort;
use crate::domain::config::ProviderCredentials;
use crate::domain::scraper::{
    ProviderDescriptor, ScrapeCandidate, ScrapeQuery, ScrapeTargetKind, ScrapedMetadata,
};
use crate::infra::scrapers::html_util::{
    build_browser_http_client, extract_meta_map, extract_numeric_id_from_query,
    is_antibot_or_captcha_html, normalize_cover_url, parse_year_and_date, percent_encode_gb18030,
    read_response_text, strip_html_to_text, strip_inline_html,
};

pub struct FalooProvider;

static RE_FL_TITLE_LINK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<h1[^>]*>\s*<a[^>]+href="[^"]*faloo\.com/(\d+)\.html"[^>]*>(.*?)</a>"#)
        .unwrap()
});
static RE_FL_COVER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<img[^>]+class="[^"]*TYImg[^"]*"[^>]+(?:data-original|src)="([^"]+)""#)
        .unwrap()
});
static RE_FL_AUTHOR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)class="TwoBox02_09"[^>]*>.*?<a[^>]*>(.*?)</a>"#).unwrap());
static RE_FL_CATEGORY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)<a[^>]+href="[^"]*l_3_[^"]*"[^>]*>(.*?)</a>"#).unwrap());
static RE_FL_SUMMARY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<div[^>]+class="[^"]*TwoBox02_06[^"]*"[^>]*>\s*<a[^>]*>(.*?)</a>"#).unwrap()
});
static RE_FL_DETAIL_INTRO: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<div[^>]+class="[^"]*T-L-T-C-Box1[^"]*"[^>]*>(.*?)</div>"#).unwrap()
});

fn clean_faloo_summary(raw: &str) -> String {
    let text = strip_html_to_text(raw);
    let filtered: Vec<&str> = text
        .lines()
        .filter(|line| !line.contains("飞卢小说网提醒您：本小说及人物纯属虚构"))
        .collect();
    filtered.join("\n").trim().to_string()
}

pub(crate) fn parse_faloo_search_html(html: &str, query: &ScrapeQuery) -> Vec<ScrapeCandidate> {
    if is_antibot_or_captcha_html(html) {
        return Vec::new();
    }

    let mut seen_ids = std::collections::HashSet::new();
    let mut drafts = Vec::new();

    for chunk in html.split(r#"class="TwoBox02_02""#).skip(1) {
        let Some(t_cap) = RE_FL_TITLE_LINK.captures(chunk) else {
            continue;
        };
        let book_id = t_cap[1].trim().to_string();
        let title = strip_inline_html(&t_cap[2]);
        if book_id.is_empty() || title.is_empty() || !seen_ids.insert(book_id.clone()) {
            continue;
        }

        let cover_url = RE_FL_COVER
            .captures(chunk)
            .and_then(|c| normalize_cover_url(&c[1]));
        let author = RE_FL_AUTHOR
            .captures(chunk)
            .map(|c| strip_inline_html(&c[1]))
            .filter(|s| !s.is_empty());
        let category = RE_FL_CATEGORY
            .captures(chunk)
            .map(|c| strip_inline_html(&c[1]))
            .filter(|s| !s.is_empty());
        let summary = RE_FL_SUMMARY
            .captures(chunk)
            .map(|c| clean_faloo_summary(&c[1]))
            .filter(|s| !s.is_empty());

        drafts.push(NovelCandidateDraft {
            external_id: book_id,
            title,
            author,
            category,
            year: None,
            summary,
            cover_url,
            popularity_votes: 0,
        });
    }

    rank_novel_candidates("faloo", "飞卢小说网", query, drafts, 12)
}

pub(crate) fn parse_faloo_detail_html(book_id: &str, html: &str) -> DomainResult<ScrapedMetadata> {
    if is_antibot_or_captcha_html(html) {
        return Err(DomainError::NotFound(format!(
            "Faloo book '{book_id}' unavailable"
        )));
    }
    let meta = extract_meta_map(html);

    let title = meta
        .get("og:novel:book_name")
        .or_else(|| meta.get("og:title"))
        .map(|s| strip_inline_html(s))
        .filter(|s| !s.is_empty())
        .ok_or_else(|| DomainError::NotFound(format!("Faloo book '{book_id}' not found")))?;

    let author = meta
        .get("og:novel:author")
        .map(|s| strip_inline_html(s))
        .filter(|s| !s.is_empty());

    let category = meta
        .get("og:novel:category")
        .map(|s| strip_inline_html(s))
        .filter(|s| !s.is_empty());

    let summary = RE_FL_DETAIL_INTRO
        .captures(html)
        .map(|c| clean_faloo_summary(&c[1]))
        .filter(|s| !s.is_empty())
        .or_else(|| {
            meta.get("og:description")
                .or_else(|| meta.get("description"))
                .map(|s| clean_faloo_summary(s))
                .filter(|s| !s.is_empty())
        });

    let cover_url = meta.get("og:image").and_then(|s| normalize_cover_url(s));

    let (year, published_date) = meta
        .get("og:novel:update_time")
        .map(|s| parse_year_and_date(s))
        .unwrap_or((None, None));

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
    .into_metadata("faloo", "飞卢小说网", book_id))
}

#[async_trait]
impl ScraperProviderPort for FalooProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "faloo".to_string(),
            name: "飞卢小说网".to_string(),
            description: "飞卢小说网公开搜索与作品详情书名、作者、高清封面、分类与简介刮削"
                .to_string(),
            website: "https://b.faloo.com".to_string(),
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

        if let Some(id) = extract_numeric_id_from_query(kw, &["b.faloo.com/", "wap.faloo.com/"])
            && (kw.contains("faloo.com") || id.len() >= 5)
            && let Ok(detail) = self
                .fetch_detail(&id, ScrapeTargetKind::Book, "zh-CN", creds)
                .await
            && let Some(cand) = novel_candidate_from_detail("faloo", "飞卢小说网", id, detail)
        {
            return Ok(vec![cand]);
        }

        let client = build_browser_http_client(creds, false)?;
        let url = format!(
            "https://b.faloo.com/l/0/1.html?k={}",
            percent_encode_gb18030(kw)
        );
        let resp = client
            .get(&url)
            .header(reqwest::header::REFERER, "https://b.faloo.com/")
            .send()
            .await
            .map_err(|e| DomainError::Storage(format!("Faloo search failed: {e}")))?;
        if !resp.status().is_success() {
            return Ok(Vec::new());
        }
        let html = read_response_text(resp, GB18030).await?;
        Ok(parse_faloo_search_html(&html, query))
    }

    async fn fetch_detail(
        &self,
        external_id: &str,
        _target: ScrapeTargetKind,
        _locale: &str,
        creds: &ProviderCredentials,
    ) -> DomainResult<ScrapedMetadata> {
        let book_id = extract_digits_book_id(external_id, "Faloo")?;
        let client = build_browser_http_client(creds, false)?;
        let url = format!("https://b.faloo.com/{book_id}.html");
        let resp = client
            .get(&url)
            .header(reqwest::header::REFERER, "https://b.faloo.com/")
            .send()
            .await
            .map_err(|e| DomainError::Storage(format!("Faloo detail failed: {e}")))?;
        if !resp.status().is_success() {
            return Err(DomainError::NotFound(format!(
                "Faloo book '{book_id}' returned HTTP {}",
                resp.status()
            )));
        }
        let html = read_response_text(resp, GB18030).await?;
        parse_faloo_detail_html(&book_id, &html)
    }
}

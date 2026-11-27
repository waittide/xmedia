use std::sync::LazyLock;

use async_trait::async_trait;
use encoding_rs::{GB18030, UTF_8};
use regex::Regex;
use serde::Deserialize;
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
    build_browser_http_client, extract_numeric_id_from_query, is_antibot_or_captcha_html,
    normalize_cover_url, parse_year_and_date, percent_encode_gb18030, read_response_text,
    strip_html_to_text, strip_inline_html,
};

pub struct JjwxcProvider;

static RE_JJ_BOOK_LINK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)<a[^>]+href="/book2/(\d+)"[^>]*>(.*?)</a>"#).unwrap());
static RE_JJ_AUTHOR_LINK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)<a[^>]+href="/wapauthor/\d+"[^>]*>(.*?)</a>"#).unwrap());
static RE_LEADING_FLOAT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([0-9]+(?:\.[0-9]+)?)").unwrap());

#[derive(Debug, Deserialize, Default)]
pub(crate) struct JjwxcNovelDetail {
    #[serde(default, rename = "novelId")]
    pub novel_id: String,
    #[serde(default, rename = "novelName")]
    pub novel_name: String,
    #[serde(default, rename = "authorName")]
    pub author_name: String,
    #[serde(default, rename = "novelClass")]
    pub novel_class: String,
    #[serde(default, rename = "novelTags")]
    pub novel_tags: String,
    #[serde(default, rename = "isSystemCover")]
    pub is_system_cover: String,
    #[serde(default, rename = "novelCover")]
    pub novel_cover: String,
    #[serde(default, rename = "originalCover")]
    pub original_cover: String,
    #[serde(default, rename = "novelIntro")]
    pub novel_intro: String,
    #[serde(default, rename = "novelIntroShort")]
    pub novel_intro_short: String,
    #[serde(default, rename = "renewDate")]
    pub renew_date: String,
    #[serde(default)]
    pub series: String,
    #[serde(default, rename = "novelReviewScore")]
    pub novel_review_score: String,
}

pub(crate) fn parse_jjwxc_wap_search_html(html: &str, query: &ScrapeQuery) -> Vec<ScrapeCandidate> {
    if is_antibot_or_captcha_html(html) {
        return Vec::new();
    }

    let mut seen_ids = std::collections::HashSet::new();
    let mut drafts = Vec::new();

    for chunk in html.split("<li").skip(1) {
        let Some(b_cap) = RE_JJ_BOOK_LINK.captures(chunk) else {
            continue;
        };
        let novel_id = b_cap[1].trim().to_string();
        let title = strip_inline_html(&b_cap[2]);
        if novel_id.is_empty() || title.is_empty() || !seen_ids.insert(novel_id.clone()) {
            continue;
        }

        let author = RE_JJ_AUTHOR_LINK
            .captures(chunk)
            .map(|c| strip_inline_html(&c[1]))
            .filter(|s| !s.is_empty());

        drafts.push(NovelCandidateDraft {
            external_id: novel_id,
            title,
            author,
            ..Default::default()
        });
    }

    rank_novel_candidates("jjwxc", "晋江文学城", query, drafts, 12)
}

pub(crate) fn map_jjwxc_detail(
    fallback_id: &str,
    d: JjwxcNovelDetail,
) -> DomainResult<ScrapedMetadata> {
    let title = strip_inline_html(&d.novel_name);
    if title.is_empty() {
        return Err(DomainError::NotFound(format!(
            "JJWXC novel '{fallback_id}' not found"
        )));
    }
    let ext_id = if d.novel_id.trim().is_empty() {
        fallback_id.to_string()
    } else {
        d.novel_id.trim().to_string()
    };

    let author = strip_inline_html(&d.author_name);
    let summary = {
        let full = strip_html_to_text(&d.novel_intro);
        if !full.is_empty() {
            Some(full)
        } else {
            let short = strip_html_to_text(&d.novel_intro_short);
            (!short.is_empty()).then_some(short)
        }
    };

    let cover_url = if d.is_system_cover.trim() == "1" && d.original_cover.trim().is_empty() {
        None
    } else {
        normalize_cover_url(&d.original_cover).or_else(|| normalize_cover_url(&d.novel_cover))
    };

    let genres: Vec<String> = d
        .novel_class
        .split('-')
        .map(strip_inline_html)
        .filter(|s| !s.is_empty())
        .collect();

    let mut tags = Vec::new();
    for raw_t in d.novel_tags.split([',', '，', ' ']) {
        let t = strip_inline_html(raw_t);
        if !t.is_empty() && !tags.contains(&t) {
            tags.push(t);
        }
    }

    let series_or_album = {
        let s = strip_inline_html(&d.series);
        (!s.is_empty() && s != "无所属系列").then_some(s)
    };

    let (year, published_date) = parse_year_and_date(&d.renew_date);

    let rating = RE_LEADING_FLOAT
        .captures(&d.novel_review_score)
        .and_then(|c| c[1].parse::<f32>().ok())
        .filter(|&r| r > 0.0 && r <= 10.0);

    Ok(NovelMetadataDraft {
        title,
        series_or_album,
        author: (!author.is_empty()).then_some(author),
        year,
        published_date,
        rating,
        summary,
        genres,
        tags,
        cover_url,
    }
    .into_metadata("jjwxc", "晋江文学城", ext_id))
}

#[async_trait]
impl ScraperProviderPort for JjwxcProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "jjwxc".to_string(),
            name: "晋江文学城".to_string(),
            description: "晋江文学城小说书名、作者、所属系列、原图封面、题材标签与文案公开刮削"
                .to_string(),
            website: "https://www.jjwxc.net".to_string(),
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

        if let Some(id) =
            extract_numeric_id_from_query(kw, &["novelid=", "novelId=", "jjwxc.net/book2/"])
            && (kw.contains("jjwxc") || kw.contains("novel") || id.len() >= 5)
            && let Ok(detail) = self
                .fetch_detail(&id, ScrapeTargetKind::Book, "zh-CN", creds)
                .await
            && let Some(cand) = novel_candidate_from_detail("jjwxc", "晋江文学城", id, detail)
        {
            return Ok(vec![cand]);
        }

        let client = build_browser_http_client(creds, true)?;
        let url = format!(
            "https://m.jjwxc.net/search?kw={}&t=1",
            percent_encode_gb18030(kw)
        );
        let resp = client
            .get(&url)
            .header(reqwest::header::REFERER, "https://m.jjwxc.net/")
            .send()
            .await
            .map_err(|e| DomainError::Storage(format!("JJWXC search failed: {e}")))?;
        if !resp.status().is_success() {
            return Ok(Vec::new());
        }
        let html = read_response_text(resp, GB18030).await?;
        Ok(parse_jjwxc_wap_search_html(&html, query))
    }

    async fn fetch_detail(
        &self,
        external_id: &str,
        _target: ScrapeTargetKind,
        _locale: &str,
        creds: &ProviderCredentials,
    ) -> DomainResult<ScrapedMetadata> {
        let novel_id = extract_digits_book_id(external_id, "JJWXC")?;
        let client = build_browser_http_client(creds, true)?;
        let resp = client
            .get("https://app-cdn.jjwxc.net/androidapi/novelbasicinfo")
            .query(&[("novelId", novel_id.as_str())])
            .send()
            .await
            .map_err(|e| DomainError::Storage(format!("JJWXC detail failed: {e}")))?;
        if !resp.status().is_success() {
            return Err(DomainError::NotFound(format!(
                "JJWXC novel '{novel_id}' returned HTTP {}",
                resp.status()
            )));
        }
        let body = read_response_text(resp, UTF_8).await?;
        let parsed: JjwxcNovelDetail = serde_json::from_str(&body).map_err(|e| {
            DomainError::NotFound(format!("JJWXC novel '{novel_id}' invalid payload: {e}"))
        })?;
        map_jjwxc_detail(&novel_id, parsed)
    }
}

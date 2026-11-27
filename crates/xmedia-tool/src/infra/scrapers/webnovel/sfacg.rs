use std::sync::LazyLock;

use async_trait::async_trait;
use encoding_rs::UTF_8;
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
    is_antibot_or_captcha_html, normalize_cover_url, parse_year_and_date, read_response_text,
    strip_html_to_text, strip_inline_html,
};

pub struct SfacgProvider;

static RE_SF_SEARCH_UL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)<ul[^>]*style="width:100%"[^>]*>(.*?)</ul>"#).unwrap());
static RE_SF_SEARCH_LINK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<a[^>]+href="https?://book\.sfacg\.com/Novel/(\d+)[^"]*"[^>]*>(.*?)</a>"#)
        .unwrap()
});
static RE_SF_SEARCH_IMG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)<img[^>]+src="([^"]+)""#).unwrap());
static RE_SF_SEARCH_INFO: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)综合信息：\s*([^/<\r\n]+)(?:/\s*([^<\r\n]+))?\s*<br\s*/?>\s*(.*?)\s*</li>"#)
        .unwrap()
});

static RE_SF_DETAIL_TITLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<h1[^>]+class="[^"]*title[^"]*"[^>]*>\s*<span[^>]+class="[^"]*text[^"]*"[^>]*>(.*?)</span>"#).unwrap()
});
static RE_SF_DETAIL_AUTHOR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<div[^>]+class="[^"]*author-name[^"]*"[^>]*>\s*<span[^>]*>(.*?)</span>"#)
        .unwrap()
});
static RE_SF_DETAIL_INTRO: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<p[^>]+class="[^"]*introduce[^"]*"[^>]*>(.*?)</p>"#).unwrap()
});
static RE_SF_DETAIL_COVER_MAIN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<div[^>]+class="[^"]*summary-pic[^"]*"[^>]*>.*?<img[^>]+src="([^"]+)""#)
        .unwrap()
});
static RE_SF_DETAIL_COVER_ANY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)((?:https?:)?//rs\.sfacg\.com/web/novel/images/NovelCover/Big/[^"'\s>]+)"#)
        .unwrap()
});
static RE_SF_DETAIL_GENRE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"类型\s*[:：]\s*([^<\s]+)"#).unwrap());
static RE_SF_DETAIL_UPDATE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"更新\s*[:：]\s*([0-9]{4}[-/.][0-9]{1,2}[-/.][0-9]{1,2})"#).unwrap()
});
static RE_SF_DETAIL_TAG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<li[^>]+class="[^"]*\btag\b[^"]*"[^>]*>.*?<span[^>]+class="[^"]*text[^"]*"[^>]*>(.*?)</span>"#).unwrap()
});
static RE_TRAILING_PARENS_NUM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\(\d+\)$").unwrap());

fn upgrade_sfacg_cover(raw: &str) -> Option<String> {
    normalize_cover_url(raw).map(|u| u.replace("/NovelCover/big/", "/NovelCover/Big/"))
}

pub(crate) fn parse_sfacg_search_html(html: &str, query: &ScrapeQuery) -> Vec<ScrapeCandidate> {
    if is_antibot_or_captcha_html(html) {
        return Vec::new();
    }

    let drafts = RE_SF_SEARCH_UL.captures_iter(html).filter_map(|ul_cap| {
        let block = &ul_cap[1];
        let link_cap = RE_SF_SEARCH_LINK.captures(block)?;
        let book_id = link_cap[1].trim().to_string();
        let title = strip_inline_html(&link_cap[2]);
        let cover_url = RE_SF_SEARCH_IMG
            .captures(block)
            .and_then(|c| upgrade_sfacg_cover(&c[1]));

        let (author, year, summary) = if let Some(info) = RE_SF_SEARCH_INFO.captures(block) {
            let a = strip_inline_html(&info[1]);
            let (y, _) = info
                .get(2)
                .map(|m| parse_year_and_date(m.as_str()))
                .unwrap_or((None, None));
            let s = strip_html_to_text(&info[3]);
            (
                (!a.is_empty()).then_some(a),
                y,
                (!s.is_empty()).then_some(s),
            )
        } else {
            (None, None, None)
        };

        Some(NovelCandidateDraft {
            external_id: book_id,
            title,
            author,
            category: None,
            year,
            summary,
            cover_url,
            popularity_votes: 0,
        })
    });

    rank_novel_candidates("sfacg", "菠萝包轻小说", query, drafts, 12)
}

pub(crate) fn parse_sfacg_detail_html(book_id: &str, html: &str) -> DomainResult<ScrapedMetadata> {
    if is_antibot_or_captcha_html(html) {
        return Err(DomainError::NotFound(format!(
            "SFACG book '{book_id}' unavailable"
        )));
    }
    let meta = extract_meta_map(html);
    let kw_parts: Vec<&str> = meta
        .get("keywords")
        .map(|s| s.split(',').map(str::trim).collect())
        .unwrap_or_default();

    let title = RE_SF_DETAIL_TITLE
        .captures(html)
        .map(|c| {
            strip_inline_html(&c[1])
                .trim_end_matches(" VIP")
                .trim()
                .to_string()
        })
        .filter(|s| !s.is_empty())
        .or_else(|| {
            kw_parts
                .first()
                .map(|s| strip_inline_html(s))
                .filter(|s| !s.is_empty())
        })
        .ok_or_else(|| DomainError::NotFound(format!("SFACG book '{book_id}' not found")))?;

    let author = RE_SF_DETAIL_AUTHOR
        .captures(html)
        .map(|c| strip_inline_html(&c[1]))
        .filter(|s| !s.is_empty())
        .or_else(|| {
            kw_parts
                .get(1)
                .map(|s| strip_inline_html(s))
                .filter(|s| !s.is_empty())
        });

    let summary = RE_SF_DETAIL_INTRO
        .captures(html)
        .map(|c| strip_html_to_text(&c[1]))
        .filter(|s| !s.is_empty());

    let cover_url = RE_SF_DETAIL_COVER_MAIN
        .captures(html)
        .and_then(|c| upgrade_sfacg_cover(&c[1]))
        .or_else(|| {
            RE_SF_DETAIL_COVER_ANY
                .captures(html)
                .and_then(|c| upgrade_sfacg_cover(&c[1]))
        });

    let genre = RE_SF_DETAIL_GENRE
        .captures(html)
        .map(|c| strip_inline_html(&c[1]))
        .filter(|s| !s.is_empty());

    let (year, published_date) = RE_SF_DETAIL_UPDATE
        .captures(html)
        .map(|c| parse_year_and_date(&c[1]))
        .unwrap_or((None, None));

    let mut tags = Vec::new();
    for cap in RE_SF_DETAIL_TAG.captures_iter(html) {
        let raw_t = strip_inline_html(&cap[1]);
        let cleaned = RE_TRAILING_PARENS_NUM
            .replace(&raw_t, "")
            .trim()
            .to_string();
        if !cleaned.is_empty() && !tags.contains(&cleaned) {
            tags.push(cleaned);
        }
    }

    Ok(NovelMetadataDraft {
        title,
        author,
        year,
        published_date,
        summary,
        genres: genre.into_iter().collect(),
        tags,
        cover_url,
        ..Default::default()
    }
    .into_metadata("sfacg", "菠萝包轻小说", book_id))
}

#[async_trait]
impl ScraperProviderPort for SfacgProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "sfacg".to_string(),
            name: "菠萝包轻小说".to_string(),
            description: "SF轻小说（菠萝包）公开搜索与作品页书名、作者、高清封面、标签与简介刮削"
                .to_string(),
            website: "https://book.sfacg.com".to_string(),
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

        if let Some(id) = extract_numeric_id_from_query(kw, &["sfacg.com/Novel/", "sfacg.com/b/"])
            && (kw.contains("sfacg.com") || id.len() >= 5)
            && let Ok(detail) = self
                .fetch_detail(&id, ScrapeTargetKind::Book, "zh-CN", creds)
                .await
            && let Some(cand) = novel_candidate_from_detail("sfacg", "菠萝包轻小说", id, detail)
        {
            return Ok(vec![cand]);
        }

        let client = build_browser_http_client(creds, false)?;
        let resp = client
            .get("https://s.sfacg.com/")
            .query(&[("Key", kw), ("S", "1"), ("SS", "0")])
            .header(reqwest::header::REFERER, "https://book.sfacg.com/")
            .send()
            .await
            .map_err(|e| DomainError::Storage(format!("SFACG search failed: {e}")))?;
        if !resp.status().is_success() {
            return Ok(Vec::new());
        }
        let html = read_response_text(resp, UTF_8).await?;
        Ok(parse_sfacg_search_html(&html, query))
    }

    async fn fetch_detail(
        &self,
        external_id: &str,
        _target: ScrapeTargetKind,
        _locale: &str,
        creds: &ProviderCredentials,
    ) -> DomainResult<ScrapedMetadata> {
        let book_id = extract_digits_book_id(external_id, "SFACG")?;
        let client = build_browser_http_client(creds, false)?;
        let url = format!("https://book.sfacg.com/Novel/{book_id}/");
        let resp = client
            .get(&url)
            .header(reqwest::header::REFERER, "https://book.sfacg.com/")
            .send()
            .await
            .map_err(|e| DomainError::Storage(format!("SFACG detail failed: {e}")))?;
        if !resp.status().is_success() {
            return Err(DomainError::NotFound(format!(
                "SFACG book '{book_id}' returned HTTP {}",
                resp.status()
            )));
        }
        let html = read_response_text(resp, UTF_8).await?;
        parse_sfacg_detail_html(&book_id, &html)
    }
}

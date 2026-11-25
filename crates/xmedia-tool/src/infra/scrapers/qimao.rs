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

pub struct QimaoProvider;

static RE_QM_TITLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<div[^>]+class="[^"]*title\b[^"]*"[^>]*>\s*<span[^>]+class="[^"]*txt[^"]*"[^>]*>(.*?)</span>"#).unwrap()
});
static RE_QM_SCORE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<span[^>]+class="[^"]*score[^"]*"[^>]*>\s*([0-9]+(?:\.[0-9]+)?)"#).unwrap()
});
static RE_QM_AUTHOR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)作者：\s*<em[^>]*>\s*(?:<a[^>]*>(.*?)</a>|(.*?))\s*</em>"#).unwrap()
});
static RE_QM_COVER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<div[^>]+class="[^"]*wrap-pic[^"]*"[^>]*>\s*<img[^>]+src="([^"]+)""#)
        .unwrap()
});
static RE_QM_TAGS_WRAP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<div[^>]+class="[^"]*tags-wrap[^"]*"[^>]*>(.*?)</div>"#).unwrap()
});
static RE_QM_TAG_LINK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)<a[^>]*>(.*?)</a>"#).unwrap());
static RE_QM_INTRO: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)<p[^>]+class="[^"]*intro[^"]*"[^>]*>(.*?)</p>"#).unwrap());

#[derive(Debug, Deserialize, Default)]
struct QimaoSearchEnvelope {
    #[serde(default)]
    data: QimaoSearchData,
}

#[derive(Debug, Deserialize, Default)]
struct QimaoSearchData {
    #[serde(default)]
    search_list: Vec<QimaoSearchItem>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct QimaoSearchItem {
    #[serde(default)]
    pub book_id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub intro: String,
    #[serde(default)]
    pub image_link: String,
    #[serde(default)]
    pub category2_name: String,
    #[serde(default)]
    pub update_time: String,
}

pub(crate) fn map_qimao_search_items(
    items: Vec<QimaoSearchItem>,
    query: &ScrapeQuery,
) -> Vec<ScrapeCandidate> {
    let drafts = items.into_iter().map(|item| {
        let author = strip_inline_html(&item.author);
        let category = strip_inline_html(&item.category2_name);
        let summary = strip_html_to_text(&item.intro);
        let (year, _) = parse_year_and_date(&item.update_time);
        NovelCandidateDraft {
            external_id: item.book_id.trim().to_string(),
            title: strip_inline_html(&item.title),
            author: (!author.is_empty()).then_some(author),
            category: (!category.is_empty()).then_some(category),
            year,
            summary: (!summary.is_empty()).then_some(summary),
            cover_url: normalize_cover_url(&item.image_link),
            popularity_votes: 0,
        }
    });

    rank_novel_candidates("qimao", "七猫免费小说", query, drafts, 12)
}

pub(crate) fn parse_qimao_detail_html(book_id: &str, html: &str) -> DomainResult<ScrapedMetadata> {
    if is_antibot_or_captcha_html(html) {
        return Err(DomainError::NotFound(format!(
            "Qimao book '{book_id}' unavailable"
        )));
    }
    let meta = extract_meta_map(html);

    let title = RE_QM_TITLE
        .captures(html)
        .map(|c| strip_inline_html(&c[1]))
        .filter(|s| !s.is_empty())
        .or_else(|| {
            meta.get("keywords").and_then(|kw| {
                let start = kw.find('《')? + '《'.len_utf8();
                let end = kw[start..].find('》')? + start;
                let t = kw[start..end].trim().to_string();
                (!t.is_empty()).then_some(t)
            })
        })
        .ok_or_else(|| DomainError::NotFound(format!("Qimao book '{book_id}' not found")))?;

    let author = RE_QM_AUTHOR
        .captures(html)
        .and_then(|c| c.get(1).or_else(|| c.get(2)))
        .map(|m| strip_inline_html(m.as_str()))
        .filter(|s| !s.is_empty());

    let rating = RE_QM_SCORE
        .captures(html)
        .and_then(|c| c[1].parse::<f32>().ok())
        .filter(|&r| r > 0.0 && r <= 10.0);

    let cover_url = RE_QM_COVER
        .captures(html)
        .and_then(|c| normalize_cover_url(&c[1]));

    let summary = RE_QM_INTRO
        .captures(html)
        .map(|c| strip_html_to_text(&c[1]))
        .filter(|s| !s.is_empty())
        .or_else(|| meta.get("description").map(|d| strip_html_to_text(d)));

    let mut genres = Vec::new();
    let mut tags = Vec::new();
    if let Some(wrap) = RE_QM_TAGS_WRAP.captures(html) {
        for (idx, cap) in RE_QM_TAG_LINK.captures_iter(&wrap[1]).enumerate() {
            let lbl = strip_inline_html(&cap[1]);
            if !lbl.is_empty() {
                if idx == 0 {
                    genres.push(lbl.clone());
                } else if !tags.contains(&lbl) {
                    tags.push(lbl);
                }
            }
        }
    }

    Ok(NovelMetadataDraft {
        title,
        author,
        rating,
        summary,
        genres,
        tags,
        cover_url,
        ..Default::default()
    }
    .into_metadata("qimao", "七猫免费小说", book_id))
}

#[async_trait]
impl ScraperProviderPort for QimaoProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "qimao".to_string(),
            name: "七猫免费小说".to_string(),
            description: "七猫中文网公开搜索与书库详情书名、作者、封面、评分与分类简介刮削"
                .to_string(),
            website: "https://www.qimao.com".to_string(),
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

        if let Some(id) = extract_numeric_id_from_query(kw, &["qimao.com/shuku/"])
            && (kw.contains("qimao.com") || id.len() >= 5)
            && let Ok(detail) = self
                .fetch_detail(&id, ScrapeTargetKind::Book, "zh-CN", creds)
                .await
            && let Some(cand) = novel_candidate_from_detail("qimao", "七猫免费小说", id, detail)
        {
            return Ok(vec![cand]);
        }

        let client = build_browser_http_client(creds, false)?;
        let resp = client
            .get("https://www.qimao.com/api/search/result")
            .query(&[("keyword", kw), ("page", "1")])
            .header(
                reqwest::header::REFERER,
                "https://www.qimao.com/search/index/",
            )
            .send()
            .await
            .map_err(|e| DomainError::Storage(format!("Qimao search failed: {e}")))?;
        if !resp.status().is_success() {
            return Err(DomainError::Storage(format!(
                "Qimao search returned HTTP {}",
                resp.status()
            )));
        }
        let body = read_response_text(resp, UTF_8).await?;
        let env: QimaoSearchEnvelope = serde_json::from_str(&body)
            .map_err(|e| DomainError::Storage(format!("invalid Qimao search JSON: {e}")))?;
        Ok(map_qimao_search_items(env.data.search_list, query))
    }

    async fn fetch_detail(
        &self,
        external_id: &str,
        _target: ScrapeTargetKind,
        _locale: &str,
        creds: &ProviderCredentials,
    ) -> DomainResult<ScrapedMetadata> {
        let book_id = extract_digits_book_id(external_id, "Qimao")?;
        let client = build_browser_http_client(creds, false)?;
        let url = format!("https://www.qimao.com/shuku/{book_id}/");
        let resp = client
            .get(&url)
            .header(reqwest::header::REFERER, "https://www.qimao.com/")
            .send()
            .await
            .map_err(|e| DomainError::Storage(format!("Qimao detail failed: {e}")))?;
        if !resp.status().is_success() {
            return Err(DomainError::NotFound(format!(
                "Qimao book '{book_id}' returned HTTP {}",
                resp.status()
            )));
        }
        let html = read_response_text(resp, UTF_8).await?;
        parse_qimao_detail_html(&book_id, &html)
    }
}

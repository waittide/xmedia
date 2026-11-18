use std::sync::LazyLock;

use async_trait::async_trait;
use encoding_rs::UTF_8;
use regex::Regex;
use xmedia_core::domain::error::{DomainError, DomainResult};

use crate::domain::config::ProviderCredentials;
use crate::domain::ports::ScraperProviderPort;
use crate::domain::scraper::{
    ProviderDescriptor, ScrapeCandidate, ScrapeQuery, ScrapeTargetKind, ScrapedMetadata,
};
use crate::infra::scrapers::cn_novel_common::{
    build_browser_http_client, build_novel_subtitle, extract_meta_map,
    extract_numeric_id_from_query, is_antibot_or_captcha_html, normalize_cover_url,
    parse_year_and_date, percent_encode_utf8, read_response_text, score_novel_candidate,
    strip_html_to_text, strip_inline_html,
};

pub struct CiweimaoProvider;

static RE_CWM_ITEM_ID: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)(?:data-book-id="(\d+)"|href="https?://www\.ciweimao\.com/book/(\d+)")"#)
        .unwrap()
});
static RE_CWM_ITEM_TITLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<p[^>]+class="[^"]*tit[^"]*"[^>]*>\s*<a[^>]*>(.*?)</a>"#).unwrap()
});
static RE_CWM_ITEM_AUTHOR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)小说作者\s*[:：]\s*(?:<a[^>]*>(.*?)</a>|([^<\n]+))"#).unwrap()
});
static RE_CWM_ITEM_COVER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)<img[^>]+(?:data-original|src)="([^"]+)""#).unwrap());
static RE_CWM_ITEM_DESC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<div[^>]+class="[^"]*desc[^"]*"[^>]*>(.*?)</div>"#).unwrap()
});
static RE_CWM_LABEL_BOX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<p[^>]+class="[^"]*label-box[^"]*"[^>]*>(.*?)</p>"#).unwrap()
});
static RE_CWM_SPAN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)<span[^>]*>(.*?)</span>"#).unwrap());
static RE_CWM_UPDATE_TIME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"更新时间\s*[:：]\s*([0-9]{4}-[0-9]{1,2}-[0-9]{1,2})"#).unwrap());

pub(crate) fn parse_ciweimao_search_html(html: &str, query: &ScrapeQuery) -> Vec<ScrapeCandidate> {
    if is_antibot_or_captcha_html(html) {
        return Vec::new();
    }

    let mut seen_ids = std::collections::HashSet::new();
    let mut scored: Vec<(i32, usize, ScrapeCandidate)> = Vec::new();

    for (idx, chunk) in html.split("<li").skip(1).enumerate() {
        let Some(id_caps) = RE_CWM_ITEM_ID.captures(chunk) else {
            continue;
        };
        let Some(book_id) = id_caps
            .get(1)
            .or_else(|| id_caps.get(2))
            .map(|m| m.as_str().to_string())
        else {
            continue;
        };
        if !seen_ids.insert(book_id.clone()) {
            continue;
        }

        let Some(title) = RE_CWM_ITEM_TITLE
            .captures(chunk)
            .map(|c| strip_inline_html(&c[1]))
            .filter(|s| !s.is_empty())
        else {
            continue;
        };

        let author = RE_CWM_ITEM_AUTHOR
            .captures(chunk)
            .and_then(|c| c.get(1).or_else(|| c.get(2)))
            .map(|m| strip_inline_html(m.as_str()))
            .filter(|s| !s.is_empty());

        let cover_url = RE_CWM_ITEM_COVER
            .captures(chunk)
            .and_then(|c| normalize_cover_url(&c[1]));

        let summary = RE_CWM_ITEM_DESC
            .captures(chunk)
            .map(|c| strip_html_to_text(&c[1]))
            .filter(|s| !s.is_empty());

        let (year, _) = RE_CWM_UPDATE_TIME
            .captures(chunk)
            .map(|c| parse_year_and_date(&c[1]))
            .unwrap_or((None, None));

        let rel = score_novel_candidate(query, &title, author.as_deref(), year, 0);
        scored.push((
            rel,
            idx,
            ScrapeCandidate {
                provider_id: "ciweimao".to_string(),
                provider_name: "刺猬猫".to_string(),
                external_id: book_id,
                target: ScrapeTargetKind::Book,
                title,
                subtitle: build_novel_subtitle(author.as_deref(), None),
                year,
                summary,
                cover_url,
                synced_lyrics: None,
                plain_lyrics: None,
            },
        ));
    }

    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    scored.into_iter().take(12).map(|(_, _, c)| c).collect()
}

pub(crate) fn parse_ciweimao_detail_html(
    book_id: &str,
    html: &str,
) -> DomainResult<ScrapedMetadata> {
    if is_antibot_or_captcha_html(html) {
        return Err(DomainError::NotFound(format!(
            "Ciweimao book '{book_id}' unavailable"
        )));
    }
    let meta = extract_meta_map(html);

    let title = meta
        .get("og:novel:book_name")
        .map(|s| strip_inline_html(s))
        .filter(|s| !s.is_empty())
        .ok_or_else(|| DomainError::NotFound(format!("Ciweimao book '{book_id}' not found")))?;

    let author = meta
        .get("og:novel:author")
        .map(|s| strip_inline_html(s))
        .filter(|s| !s.is_empty());

    let category = meta
        .get("og:novel:category")
        .map(|s| strip_inline_html(s))
        .filter(|s| !s.is_empty());

    let summary = meta
        .get("og:description")
        .or_else(|| meta.get("description"))
        .map(|s| strip_html_to_text(s))
        .filter(|s| !s.is_empty());

    let cover_url = meta.get("og:image").and_then(|s| normalize_cover_url(s));

    let (year, published_date) = meta
        .get("og:novel:update_time")
        .map(|s| parse_year_and_date(s))
        .or_else(|| {
            RE_CWM_UPDATE_TIME
                .captures(html)
                .map(|c| parse_year_and_date(&c[1]))
        })
        .unwrap_or((None, None));

    let mut tags = Vec::new();
    if let Some(lbl_box) = RE_CWM_LABEL_BOX.captures(html) {
        for cap in RE_CWM_SPAN.captures_iter(&lbl_box[1]) {
            let t = strip_inline_html(&cap[1]);
            if !t.is_empty() && !tags.contains(&t) {
                tags.push(t);
            }
        }
    }

    let mut external_ids = std::collections::BTreeMap::new();
    external_ids.insert("ciweimao".to_string(), book_id.to_string());

    Ok(ScrapedMetadata {
        provider_id: "ciweimao".to_string(),
        external_id: book_id.to_string(),
        target: Some(ScrapeTargetKind::Book),
        title: Some(title),
        creators: author.into_iter().collect(),
        publisher_or_studio: Some("刺猬猫".to_string()),
        year,
        published_date,
        language: Some("zh-CN".to_string()),
        summary,
        genres: category.into_iter().collect(),
        tags,
        cover_url,
        external_ids,
        ..Default::default()
    })
}

#[async_trait]
impl ScraperProviderPort for CiweimaoProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "ciweimao".to_string(),
            name: "刺猬猫".to_string(),
            description: "刺猬猫阅读轻小说/二次元作品书名、作者、封面、分类标签与简介刮削"
                .to_string(),
            website: "https://www.ciweimao.com".to_string(),
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

        if let Some(id) = extract_numeric_id_from_query(kw, &["ciweimao.com/book/"])
            && (kw.contains("ciweimao.com") || id.len() >= 6)
            && let Ok(detail) = self
                .fetch_detail(&id, ScrapeTargetKind::Book, "zh-CN", creds)
                .await
            && let Some(title) = detail.title
        {
            return Ok(vec![ScrapeCandidate {
                provider_id: "ciweimao".to_string(),
                provider_name: "刺猬猫".to_string(),
                external_id: id,
                target: ScrapeTargetKind::Book,
                title,
                subtitle: build_novel_subtitle(
                    detail.creators.first().map(String::as_str),
                    detail.genres.first().map(String::as_str),
                ),
                year: detail.year,
                summary: detail.summary,
                cover_url: detail.cover_url,
                synced_lyrics: None,
                plain_lyrics: None,
            }]);
        }

        let client = build_browser_http_client(creds, false)?;
        let url = format!(
            "https://www.ciweimao.com/get-search-book-list/0-0-0-0-0-0/all/{}/1",
            percent_encode_utf8(kw)
        );
        let resp = client
            .get(&url)
            .header(reqwest::header::REFERER, "https://www.ciweimao.com/")
            .send()
            .await
            .map_err(|e| DomainError::Storage(format!("Ciweimao search failed: {e}")))?;
        if !resp.status().is_success() {
            return Ok(Vec::new());
        }
        let html = read_response_text(resp, UTF_8).await?;
        Ok(parse_ciweimao_search_html(&html, query))
    }

    async fn fetch_detail(
        &self,
        external_id: &str,
        _target: ScrapeTargetKind,
        _locale: &str,
        creds: &ProviderCredentials,
    ) -> DomainResult<ScrapedMetadata> {
        let book_id: String = external_id
            .trim()
            .chars()
            .filter(|c| c.is_ascii_digit())
            .collect();
        if book_id.is_empty() {
            return Err(DomainError::Validation("empty Ciweimao book id".into()));
        }
        let client = build_browser_http_client(creds, false)?;
        let url = format!("https://www.ciweimao.com/book/{book_id}");
        let resp = client
            .get(&url)
            .header(reqwest::header::REFERER, "https://www.ciweimao.com/")
            .send()
            .await
            .map_err(|e| DomainError::Storage(format!("Ciweimao detail failed: {e}")))?;
        if !resp.status().is_success() {
            return Err(DomainError::NotFound(format!(
                "Ciweimao book '{book_id}' returned HTTP {}",
                resp.status()
            )));
        }
        let html = read_response_text(resp, UTF_8).await?;
        parse_ciweimao_detail_html(&book_id, &html)
    }
}

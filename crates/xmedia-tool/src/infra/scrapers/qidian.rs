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
    build_browser_http_client, build_novel_subtitle, extract_meta_map,
    extract_numeric_id_from_query, is_antibot_or_captcha_html, normalize_cover_url,
    parse_year_and_date, percent_encode_utf8, read_response_text, score_novel_candidate,
    strip_html_to_text, strip_inline_html,
};

pub struct QidianProvider;

static RE_PAGE_CONTEXT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<script[^>]+id="vite-plugin-ssr_pageContext"[^>]*>(.*?)</script>"#).unwrap()
});

#[derive(Debug, Deserialize, Default)]
struct QidianSearchRecord {
    #[serde(default)]
    bid: u64,
    #[serde(default, rename = "bName")]
    b_name: String,
    #[serde(default, rename = "bAuth")]
    b_auth: String,
    #[serde(default)]
    desc: String,
    #[serde(default)]
    cat: String,
    #[serde(default, rename = "subCateName")]
    sub_cate_name: String,
    #[serde(default, rename = "imgUrl")]
    img_url: String,
    #[serde(default, rename = "lastUpdateTime")]
    last_update_time: String,
    #[serde(default, rename = "updateTime")]
    update_time: String,
    #[serde(default, rename = "recomendCnt")]
    recommend_cnt: u64,
}

fn upgrade_qidian_cover(raw: &str, bid: u64) -> Option<String> {
    if let Some(mut url) = normalize_cover_url(raw) {
        for suffix in ["/180", "/150", "/90"] {
            if url.ends_with(suffix) {
                let base_len = url.len() - suffix.len();
                url.truncate(base_len);
                url.push_str("/600");
                break;
            }
        }
        return Some(url);
    }
    if bid > 0 {
        Some(format!(
            "https://bookcover.yuewen.com/qdbimg/349573/{bid}/600"
        ))
    } else {
        None
    }
}

pub(crate) fn parse_qidian_search_html(html: &str, query: &ScrapeQuery) -> Vec<ScrapeCandidate> {
    if is_antibot_or_captcha_html(html) {
        return Vec::new();
    }
    let Some(caps) = RE_PAGE_CONTEXT.captures(html) else {
        return Vec::new();
    };
    let Ok(root) = serde_json::from_str::<serde_json::Value>(&caps[1]) else {
        return Vec::new();
    };
    let Some(records_val) = root
        .pointer("/pageContext/pageProps/pageData/bookInfo/records")
        .cloned()
    else {
        return Vec::new();
    };
    let records: Vec<QidianSearchRecord> = serde_json::from_value(records_val).unwrap_or_default();

    let mut scored: Vec<(i32, usize, ScrapeCandidate)> = records
        .into_iter()
        .enumerate()
        .filter_map(|(idx, rec)| {
            if rec.bid == 0 {
                return None;
            }
            let title = strip_inline_html(&rec.b_name);
            if title.is_empty() {
                return None;
            }
            let author = strip_inline_html(&rec.b_auth);
            let cat = if !rec.sub_cate_name.trim().is_empty() {
                rec.sub_cate_name.trim()
            } else {
                rec.cat.trim()
            };
            let date_src = if !rec.last_update_time.trim().is_empty() {
                &rec.last_update_time
            } else {
                &rec.update_time
            };
            let (year, _) = parse_year_and_date(date_src);
            let summary = strip_html_to_text(&rec.desc);
            let pop = u32::try_from(rec.recommend_cnt.min(100_000)).unwrap_or(0);
            let score = score_novel_candidate(
                query,
                &title,
                (!author.is_empty()).then_some(author.as_str()),
                year,
                pop,
            );
            Some((
                score,
                idx,
                ScrapeCandidate {
                    provider_id: "qidian".to_string(),
                    provider_name: "起点中文网".to_string(),
                    external_id: rec.bid.to_string(),
                    target: ScrapeTargetKind::Book,
                    title,
                    subtitle: build_novel_subtitle(
                        (!author.is_empty()).then_some(author.as_str()),
                        (!cat.is_empty()).then_some(cat),
                    ),
                    year,
                    summary: (!summary.is_empty()).then_some(summary),
                    cover_url: upgrade_qidian_cover(&rec.img_url, rec.bid),
                    synced_lyrics: None,
                    plain_lyrics: None,
                },
            ))
        })
        .collect();

    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    scored.into_iter().take(12).map(|(_, _, c)| c).collect()
}

pub(crate) fn parse_qidian_detail_html(bid_str: &str, html: &str) -> DomainResult<ScrapedMetadata> {
    if is_antibot_or_captcha_html(html) {
        return Err(DomainError::NotFound(format!(
            "Qidian book '{bid_str}' unavailable"
        )));
    }
    let bid_num = bid_str.parse::<u64>().unwrap_or(0);

    if let Some(caps) = RE_PAGE_CONTEXT.captures(html)
        && let Ok(root) = serde_json::from_str::<serde_json::Value>(&caps[1])
        && let Some(bi) = root.pointer("/pageContext/pageProps/pageData/bookInfo")
    {
        let title = bi
            .get("bName")
            .or_else(|| bi.get("bookName"))
            .and_then(|v| v.as_str())
            .map(strip_inline_html)
            .filter(|s| !s.is_empty());
        if let Some(title) = title {
            let author = bi
                .get("authorName")
                .or_else(|| bi.get("bAuth"))
                .and_then(|v| v.as_str())
                .map(strip_inline_html)
                .filter(|s| !s.is_empty());
            let desc = bi
                .get("desc")
                .and_then(|v| v.as_str())
                .map(strip_html_to_text)
                .filter(|s| !s.is_empty());
            let upd_time = bi
                .get("updTime")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let (year, published_date) = parse_year_and_date(upd_time);
            let raw_img = bi.get("imgUrl").and_then(|v| v.as_str()).unwrap_or("");
            let cover_url = upgrade_qidian_cover(raw_img, bid_num);

            let mut genres = Vec::new();
            let mut tags = Vec::new();
            if let Some(chan) = bi
                .get("chanName")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                genres.push(chan.to_string());
            }
            if let Some(sub_cat) = bi
                .get("subCateName")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                tags.push(sub_cat.to_string());
            }
            if let Some(tag_name) = bi
                .pointer("/bookTag/tagName")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                for part in tag_name.split(['-', '·', ',', '，', ' ']) {
                    let t = part.trim();
                    if !t.is_empty() && !tags.iter().any(|x| x == t) {
                        tags.push(t.to_string());
                    }
                }
            }

            let mut external_ids = std::collections::BTreeMap::new();
            external_ids.insert("qidian".to_string(), bid_str.to_string());

            return Ok(ScrapedMetadata {
                provider_id: "qidian".to_string(),
                external_id: bid_str.to_string(),
                target: Some(ScrapeTargetKind::Book),
                title: Some(title),
                creators: author.into_iter().collect(),
                publisher_or_studio: Some("起点中文网".to_string()),
                year,
                published_date,
                language: Some("zh-CN".to_string()),
                summary: desc,
                genres,
                tags,
                cover_url,
                external_ids,
                ..Default::default()
            });
        }
    }

    let meta = extract_meta_map(html);
    let title = meta
        .get("og:novel:book_name")
        .or_else(|| meta.get("og:title"))
        .map(|s| strip_inline_html(s))
        .filter(|s| !s.is_empty())
        .ok_or_else(|| DomainError::NotFound(format!("Qidian book '{bid_str}' not found")))?;
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
    let (year, published_date) = meta
        .get("og:novel:update_time")
        .map(|s| parse_year_and_date(s))
        .unwrap_or((None, None));
    let cover_url = meta
        .get("og:image")
        .and_then(|s| upgrade_qidian_cover(s, bid_num))
        .or_else(|| upgrade_qidian_cover("", bid_num));

    let mut external_ids = std::collections::BTreeMap::new();
    external_ids.insert("qidian".to_string(), bid_str.to_string());

    Ok(ScrapedMetadata {
        provider_id: "qidian".to_string(),
        external_id: bid_str.to_string(),
        target: Some(ScrapeTargetKind::Book),
        title: Some(title),
        creators: author.into_iter().collect(),
        publisher_or_studio: Some("起点中文网".to_string()),
        year,
        published_date,
        language: Some("zh-CN".to_string()),
        summary,
        genres: category.into_iter().collect(),
        cover_url,
        external_ids,
        ..Default::default()
    })
}

#[async_trait]
impl ScraperProviderPort for QidianProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "qidian".to_string(),
            name: "起点中文网".to_string(),
            description: "阅文起点中文网原创小说书名、作者、封面、分类标签与简介公开刮削"
                .to_string(),
            website: "https://www.qidian.com".to_string(),
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
            extract_numeric_id_from_query(kw, &["qidian.com/info/", "qidian.com/book/"])
            && (kw.contains("qidian.com") || id.len() >= 6)
            && let Ok(detail) = self
                .fetch_detail(&id, ScrapeTargetKind::Book, "zh-CN", creds)
                .await
            && let Some(title) = detail.title
        {
            return Ok(vec![ScrapeCandidate {
                provider_id: "qidian".to_string(),
                provider_name: "起点中文网".to_string(),
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

        let client = build_browser_http_client(creds, true)?;
        let url = format!("https://m.qidian.com/so/{}.html", percent_encode_utf8(kw));
        let resp = client
            .get(&url)
            .header(reqwest::header::REFERER, "https://m.qidian.com/")
            .send()
            .await
            .map_err(|e| DomainError::Storage(format!("Qidian search failed: {e}")))?;
        if !resp.status().is_success() {
            return Ok(Vec::new());
        }
        let html = read_response_text(resp, UTF_8).await?;
        Ok(parse_qidian_search_html(&html, query))
    }

    async fn fetch_detail(
        &self,
        external_id: &str,
        _target: ScrapeTargetKind,
        _locale: &str,
        creds: &ProviderCredentials,
    ) -> DomainResult<ScrapedMetadata> {
        let bid: String = external_id
            .trim()
            .chars()
            .filter(|c| c.is_ascii_digit())
            .collect();
        if bid.is_empty() {
            return Err(DomainError::Validation("empty Qidian book id".into()));
        }
        let client = build_browser_http_client(creds, true)?;
        let url = format!("https://m.qidian.com/book/{bid}.html");
        let resp = client
            .get(&url)
            .header(reqwest::header::REFERER, "https://m.qidian.com/")
            .send()
            .await
            .map_err(|e| DomainError::Storage(format!("Qidian detail failed: {e}")))?;
        if !resp.status().is_success() {
            return Err(DomainError::NotFound(format!(
                "Qidian book '{bid}' returned HTTP {}",
                resp.status()
            )));
        }
        let html = read_response_text(resp, UTF_8).await?;
        parse_qidian_detail_html(&bid, &html)
    }
}

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
    extract_numeric_id_from_query, is_antibot_or_captcha_html, normalize_cover_url,
    novel_candidate_from_detail, parse_year_and_date, rank_novel_candidates, read_response_text,
    strip_html_to_text, strip_inline_html,
};

pub struct FanqieProvider;

static RE_LD_JSON: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<script[^>]+type="application/ld\+json"[^>]*>(.*?)</script>"#).unwrap()
});
static RE_FQ_TITLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<div[^>]+class="[^"]*info-name[^"]*"[^>]*>\s*<h1[^>]*>(.*?)</h1>"#).unwrap()
});
static RE_FQ_AUTHOR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<span[^>]+class="[^"]*author-name-text[^"]*"[^>]*>(.*?)</span>"#).unwrap()
});
static RE_FQ_ABSTRACT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<div[^>]+class="[^"]*page-abstract-content[^"]*"[^>]*>(.*?)</div>"#).unwrap()
});
static RE_FQ_LABELS_BLOCK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<div[^>]+class="[^"]*info-label[^"]*"[^>]*>(.*?)</div>"#).unwrap()
});
static RE_FQ_LABEL_GREY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<span[^>]+class="[^"]*info-label-grey[^"]*"[^>]*>(.*?)</span>"#).unwrap()
});
static RE_FQ_COVER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<img[^>]+class="[^"]*book-cover-img[^"]*"[^>]+src="([^"]+)""#).unwrap()
});

#[derive(Debug, Deserialize, Default)]
struct FanqieSearchEnvelope {
    #[serde(default)]
    code: i32,
    #[serde(default)]
    data: FanqieSearchData,
}

#[derive(Debug, Deserialize, Default)]
struct FanqieSearchData {
    #[serde(default)]
    ret_data: Vec<FanqieSearchItem>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct FanqieSearchItem {
    #[serde(default)]
    pub book_id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub author: String,
    #[serde(default, rename = "abstract")]
    pub abstract_text: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub thumb_url: String,
    #[serde(default)]
    pub add_bookshelf_count: String,
}

pub(crate) fn map_fanqie_search_items(
    items: Vec<FanqieSearchItem>,
    query: &ScrapeQuery,
) -> Vec<ScrapeCandidate> {
    let drafts = items.into_iter().map(|item| {
        let author = strip_inline_html(&item.author);
        let category = strip_inline_html(&item.category);
        let summary = strip_html_to_text(&item.abstract_text);
        let popularity_votes = item
            .add_bookshelf_count
            .trim()
            .parse::<u32>()
            .unwrap_or(0)
            .min(50_000);
        NovelCandidateDraft {
            external_id: item.book_id.trim().to_string(),
            title: strip_inline_html(&item.title),
            author: (!author.is_empty()).then_some(author),
            category: (!category.is_empty()).then_some(category),
            year: None,
            summary: (!summary.is_empty()).then_some(summary),
            cover_url: normalize_cover_url(&item.thumb_url),
            popularity_votes,
        }
    });

    rank_novel_candidates("fanqie", "番茄小说", query, drafts, 12)
}

pub(crate) fn parse_fanqie_detail_html(book_id: &str, html: &str) -> DomainResult<ScrapedMetadata> {
    if is_antibot_or_captcha_html(html) {
        return Err(DomainError::NotFound(format!(
            "Fanqie book '{book_id}' unavailable"
        )));
    }

    let mut ld_author: Option<String> = None;
    let mut ld_cover: Option<String> = None;
    let mut ld_date: Option<String> = None;

    if let Some(caps) = RE_LD_JSON.captures(html)
        && let Ok(val) = serde_json::from_str::<serde_json::Value>(caps[1].trim())
    {
        ld_author = val
            .pointer("/author/0/name")
            .and_then(|v| v.as_str())
            .map(strip_inline_html)
            .filter(|s| !s.is_empty());
        ld_cover = val
            .pointer("/image/0")
            .and_then(|v| v.as_str())
            .and_then(normalize_cover_url);
        ld_date = val
            .get("datePublished")
            .or_else(|| val.get("dateModified"))
            .and_then(|v| v.as_str())
            .map(str::to_string);
    }

    let title = RE_FQ_TITLE
        .captures(html)
        .map(|c| strip_inline_html(&c[1]))
        .filter(|s| !s.is_empty())
        .ok_or_else(|| DomainError::NotFound(format!("Fanqie book '{book_id}' not found")))?;

    let author = RE_FQ_AUTHOR
        .captures(html)
        .map(|c| strip_inline_html(&c[1]))
        .filter(|s| !s.is_empty())
        .or(ld_author);

    let summary = RE_FQ_ABSTRACT
        .captures(html)
        .map(|c| strip_html_to_text(&c[1]))
        .filter(|s| !s.is_empty());

    let mut genres = Vec::new();
    let mut tags = Vec::new();
    if let Some(block) = RE_FQ_LABELS_BLOCK.captures(html) {
        for (idx, cap) in RE_FQ_LABEL_GREY.captures_iter(&block[1]).enumerate() {
            let lbl = strip_inline_html(&cap[1]);
            if !lbl.is_empty() {
                if idx == 0 {
                    genres.push(lbl.clone());
                }
                if !tags.contains(&lbl) {
                    tags.push(lbl);
                }
            }
        }
    }

    let cover_url = RE_FQ_COVER
        .captures(html)
        .and_then(|c| normalize_cover_url(&c[1]))
        .or(ld_cover);

    let (year, published_date) = ld_date
        .as_deref()
        .map(parse_year_and_date)
        .unwrap_or((None, None));

    Ok(NovelMetadataDraft {
        title,
        author,
        year,
        published_date,
        summary,
        genres,
        tags,
        cover_url,
        ..Default::default()
    }
    .into_metadata("fanqie", "番茄小说", book_id))
}

pub(crate) fn parse_fanqie_mobile_detail_json(
    book_id: &str,
    body: &str,
) -> DomainResult<ScrapedMetadata> {
    let root: serde_json::Value = serde_json::from_str(body).map_err(|_| {
        DomainError::NotFound(format!("Fanqie mobile book '{book_id}' invalid JSON"))
    })?;
    let data = root.get("data").filter(|v| v.is_object()).ok_or_else(|| {
        DomainError::NotFound(format!("Fanqie mobile book '{book_id}' not found"))
    })?;

    let title = data
        .get("book_name")
        .or_else(|| data.get("title"))
        .and_then(|v| v.as_str())
        .map(strip_inline_html)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            DomainError::NotFound(format!("Fanqie mobile book '{book_id}' not found"))
        })?;

    let author = data
        .get("author")
        .and_then(|v| v.as_str())
        .map(strip_inline_html)
        .filter(|s| !s.is_empty());

    let summary = data
        .get("abstract")
        .or_else(|| data.get("book_abstract_v2"))
        .and_then(|v| v.as_str())
        .map(strip_html_to_text)
        .filter(|s| !s.is_empty());

    let category = data
        .get("category")
        .and_then(|v| v.as_str())
        .map(strip_inline_html)
        .filter(|s| !s.is_empty());

    let mut genres: Vec<String> = category.clone().into_iter().collect();
    let mut tags: Vec<String> = category.into_iter().collect();
    if let Some(raw_tags) = data.get("tags").and_then(|v| v.as_str()) {
        for part in raw_tags.split([',', '，', ';', '；', '、', '·']) {
            let t = strip_inline_html(part);
            if !t.is_empty() {
                if genres.is_empty() {
                    genres.push(t.clone());
                }
                if !tags.contains(&t) {
                    tags.push(t);
                }
            }
        }
    }

    let cover_url = data
        .get("expand_thumb_url")
        .or_else(|| data.get("thumb_url"))
        .or_else(|| data.get("audio_thumb_uri"))
        .and_then(|v| v.as_str())
        .and_then(normalize_cover_url);

    let rating = data
        .get("score")
        .and_then(|v| {
            v.as_f64()
                .or_else(|| v.as_str().and_then(|s| s.trim().parse::<f64>().ok()))
        })
        .map(|s| s as f32)
        .filter(|&r| r > 0.0);

    let (year, published_date) = data
        .get("create_time")
        .or_else(|| data.get("last_publish_time"))
        .and_then(|v| v.as_str())
        .map(parse_year_and_date)
        .unwrap_or((None, None));

    Ok(NovelMetadataDraft {
        title,
        author,
        year,
        published_date,
        rating,
        summary,
        genres,
        tags,
        cover_url,
        ..Default::default()
    }
    .into_metadata("fanqie", "番茄小说", book_id))
}

#[async_trait]
impl ScraperProviderPort for FanqieProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "fanqie".to_string(),
            name: "番茄小说".to_string(),
            description: "番茄免费小说公开搜索与详情页书名、作者、原图封面、题材标签与简介刮削"
                .to_string(),
            website: "https://fanqienovel.com".to_string(),
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

        if let Some(id) = extract_numeric_id_from_query(kw, &["fanqienovel.com/page/"])
            && (kw.contains("fanqienovel.com") || id.len() >= 15)
            && let Ok(detail) = self
                .fetch_detail(&id, ScrapeTargetKind::Book, "zh-CN", creds)
                .await
            && let Some(cand) = novel_candidate_from_detail("fanqie", "番茄小说", id, detail)
        {
            return Ok(vec![cand]);
        }

        let client = build_browser_http_client(creds, true)?;
        let resp = client
            .get("https://novel.snssdk.com/api/novel/channel/homepage/search/search/v1/")
            .query(&[
                ("device_platform", "android"),
                ("parent_enterfrom", "novel_channel_search.tab."),
                ("offset", "0"),
                ("aid", "13"),
                ("q", kw),
            ])
            .send()
            .await
            .map_err(|e| DomainError::Storage(format!("Fanqie search failed: {e}")))?;
        if !resp.status().is_success() {
            return Err(DomainError::Storage(format!(
                "Fanqie search returned HTTP {}",
                resp.status()
            )));
        }
        let body = read_response_text(resp, UTF_8).await?;
        let env: FanqieSearchEnvelope = serde_json::from_str(&body)
            .map_err(|e| DomainError::Storage(format!("invalid Fanqie search JSON: {e}")))?;
        if env.code != 0 {
            return Err(DomainError::Storage(format!(
                "Fanqie search returned error code {}",
                env.code
            )));
        }
        Ok(map_fanqie_search_items(env.data.ret_data, query))
    }

    async fn fetch_detail(
        &self,
        external_id: &str,
        _target: ScrapeTargetKind,
        _locale: &str,
        creds: &ProviderCredentials,
    ) -> DomainResult<ScrapedMetadata> {
        let book_id = extract_digits_book_id(external_id, "Fanqie")?;
        let client = build_browser_http_client(creds, false)?;
        let url = format!("https://fanqienovel.com/page/{book_id}");
        let mut last_err: Option<DomainError> = None;

        match client
            .get(&url)
            .header(reqwest::header::REFERER, "https://fanqienovel.com/")
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(html) = read_response_text(resp, UTF_8).await {
                    match parse_fanqie_detail_html(&book_id, &html) {
                        Ok(meta) => return Ok(meta),
                        Err(e) => last_err = Some(e),
                    }
                }
            }
            Ok(resp) => {
                last_err = Some(DomainError::NotFound(format!(
                    "Fanqie book '{book_id}' returned HTTP {}",
                    resp.status()
                )));
            }
            Err(e) => {
                last_err = Some(DomainError::Storage(format!("Fanqie detail failed: {e}")));
            }
        }

        // Fallback to Fanqie mobile book info API (covers app-only / copyright / non-web books that 404 on fanqienovel.com)
        let mobile_client = build_browser_http_client(creds, true)?;
        if let Ok(resp) = mobile_client
            .get("https://novel.snssdk.com/api/novel/book/info/v1/")
            .query(&[
                ("book_id", book_id.as_str()),
                ("aid", "13"),
                ("device_platform", "android"),
            ])
            .send()
            .await
            && resp.status().is_success()
            && let Ok(body) = read_response_text(resp, UTF_8).await
            && let Ok(meta) = parse_fanqie_mobile_detail_json(&book_id, &body)
        {
            return Ok(meta);
        }

        Err(last_err
            .unwrap_or_else(|| DomainError::NotFound(format!("Fanqie book '{book_id}' not found"))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fanqie_mobile_detail_json_fallback() {
        let json = r#"{
            "code": 0,
            "data": {
                "book_id": "7171641987538357284",
                "book_name": "测试番茄书名",
                "author": "测试作者",
                "abstract": "第一段<br>第二段",
                "category": "都市日常",
                "tags": "都市日常,轻松,日常",
                "thumb_url": "https://p3-novel.byteimg.com/cover.jpg",
                "score": "9.2",
                "create_time": "2023-05-10"
            }
        }"#;
        let meta = parse_fanqie_mobile_detail_json("7171641987538357284", json)
            .expect("mobile fallback json should parse");
        assert_eq!(meta.title.as_deref(), Some("测试番茄书名"));
        assert_eq!(meta.creators, vec!["测试作者"]);
        assert_eq!(meta.genres, vec!["都市日常"]);
        assert_eq!(meta.tags, vec!["都市日常", "轻松", "日常"]);
        assert_eq!(meta.year, Some(2023));
        assert_eq!(meta.rating, Some(9.2));
        assert_eq!(meta.summary.as_deref(), Some("第一段\n第二段"));
    }
}

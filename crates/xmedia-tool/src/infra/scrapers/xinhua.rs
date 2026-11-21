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
    score_candidate_relevance,
};
use crate::infra::scrapers::cn_novel_common::{
    build_browser_http_client, decode_html_entities, extract_numeric_id_from_query,
    normalize_cover_url, parse_year_and_date, read_response_text, strip_html_to_text,
    strip_inline_html,
};

pub struct XinhuaBookProvider;

const XHSD_SEARCH_URL: &str = "https://search.xhsd.com/search";
const XHSD_ITEM_PREFIX: &str = "https://item.xhsd.com/items/";
const XHSD_REFERER: &str = "https://search.xhsd.com/";

static RE_PRODUCT_LI: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<li[^>]+class="[^"]*\bproduct\b[^"]*"[^>]*>(.*?)</li>"#).unwrap()
});
static RE_ITEM_LINK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)href="(?:https?:)?//item\.xhsd\.com/items/(\d+)[^"]*""#).unwrap()
});
static RE_PRODUCT_DESC_A: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<p[^>]+class="[^"]*\bproduct-desc\b[^"]*"[^>]*>.*?<a[^>]*>(.*?)</a>"#)
        .unwrap()
});
static RE_PRODUCT_AUTHOR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<p[^>]+class="[^"]*\bproduct-author\b[^"]*"[^>]*>(.*?)</p>"#).unwrap()
});
static RE_XINHUA_IMG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)(?:data-original|src)="((?:https?:)?//img\d\.xinhuashudian\.com/[^"]+)""#)
        .unwrap()
});
static RE_DATA_ITEM_ATTR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<div[^>]+class="[^"]*\bitem-title\b[^"]*"[^>]+data-item="([^"]+)""#).unwrap()
});
static RE_DATA_DETAIL_ATTR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?is)<div[^>]+class="[^"]*\bspu-tab-item-detail\b[^"]*"[^>]+data-detail="([^"]+)""#,
    )
    .unwrap()
});
static RE_ATTR_LI: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?is)<li[^>]+class="[^"]*\battribute-li\b[^"]*"[^>]*>.*?<span[^>]+class="[^"]*\battr-label\b[^"]*"[^>]*>(.*?)</span>.*?<span[^>]+class="[^"]*\battr-val\b[^"]*"[^>]*>(.*?)</span>"#,
    )
    .unwrap()
});

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct XhsdDataItem {
    #[serde(default)]
    name: String,
    #[serde(default)]
    main_image: String,
    #[serde(default)]
    other_attributes: Vec<XhsdAttrGroup>,
    #[serde(default)]
    other_attrs: Vec<XhsdAttrGroup>,
    #[serde(default)]
    extra: Option<XhsdExtra>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct XhsdAttrGroup {
    #[serde(default)]
    other_attributes: Vec<XhsdKv>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct XhsdKv {
    #[serde(default)]
    attr_key: String,
    #[serde(default)]
    attr_val: String,
}

#[derive(Debug, Deserialize, Default)]
struct XhsdExtra {
    #[serde(default)]
    author: String,
}

#[derive(Debug, Deserialize, Default)]
struct XhsdDetailTab {
    #[serde(default)]
    title: String,
    #[serde(default)]
    content: String,
}

/// Maps Chinese Library Classification (中图分类号) code prefixes to readable book genres.
pub(crate) fn map_clc_code_to_genres(code: &str) -> Vec<String> {
    let upper = code.trim().to_ascii_uppercase();
    if upper.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let top = match upper.as_bytes()[0] {
        b'A' => Some("马克思主义·哲学"),
        b'B' => Some("哲学·宗教"),
        b'C' => Some("社会科学"),
        b'D' => Some("政治·法律"),
        b'E' => Some("军事"),
        b'F' => Some("经济"),
        b'G' => Some("文化·教育·体育"),
        b'H' => Some("语言·文字"),
        b'I' => Some("文学"),
        b'J' => Some("艺术"),
        b'K' => Some("历史·地理"),
        b'N' => Some("自然科学"),
        b'O' => Some("数理科学和化学"),
        b'P' => Some("天文学·地球科学"),
        b'Q' => Some("生物科学"),
        b'R' => Some("医药·卫生"),
        b'S' => Some("农业科学"),
        b'T' => Some("工业技术"),
        b'U' => Some("交通运输"),
        b'V' => Some("航空·航天"),
        b'X' => Some("环境科学"),
        b'Z' => Some("综合性图书"),
        _ => None,
    };
    if let Some(t) = top {
        out.push(t.to_string());
    }
    if upper.starts_with("I247") {
        out.push("中国当代小说".to_string());
    } else if upper.starts_with("I242") {
        out.push("中国古典小说".to_string());
    } else if upper.starts_with("TP3") || upper.starts_with("TP18") {
        out.push("计算机与人工智能".to_string());
    } else if upper.starts_with("K8") {
        out.push("人物传记".to_string());
    } else if upper.starts_with("B84") {
        out.push("心理学".to_string());
    }
    out
}

fn normalize_xinhua_cover_url(raw: &str) -> Option<String> {
    let decoded = decode_html_entities(raw);
    let clean = decoded.split('?').next().unwrap_or(&decoded);
    normalize_cover_url(clean)
}

pub(crate) fn parse_xinhua_detail_html(item_id: &str, html: &str) -> Option<ScrapedMetadata> {
    let mut title: Option<String> = None;
    let mut cover_url: Option<String> = None;
    let mut author: Option<String> = None;
    let mut publisher: Option<String> = None;
    let mut pub_date_raw: Option<String> = None;
    let mut isbn: Option<String> = None;
    let mut clc_code: Option<String> = None;

    if let Some(caps) = RE_DATA_ITEM_ATTR.captures(html) {
        let unescaped = decode_html_entities(&caps[1]).replace("&quot;", "\"");
        if let Ok(data) = serde_json::from_str::<XhsdDataItem>(&unescaped) {
            let t = strip_inline_html(&data.name);
            if !t.is_empty() {
                title = Some(t);
            }
            cover_url = normalize_xinhua_cover_url(&data.main_image);
            for grp in data.other_attributes.into_iter().chain(data.other_attrs) {
                for kv in grp.other_attributes {
                    let k = kv.attr_key.trim();
                    let v = strip_inline_html(&kv.attr_val);
                    if v.is_empty() {
                        continue;
                    }
                    match k {
                        "作者" if author.is_none() => author = Some(v),
                        "出版社" if publisher.is_none() => publisher = Some(v),
                        "出版时间" | "出版日期" if pub_date_raw.is_none() => {
                            pub_date_raw = Some(v)
                        }
                        _ => {}
                    }
                }
            }
            if author.is_none()
                && let Some(ext) = data.extra
            {
                let a = strip_inline_html(&ext.author);
                if !a.is_empty() {
                    author = Some(a);
                }
            }
        }
    }

    for caps in RE_ATTR_LI.captures_iter(html) {
        let label = strip_inline_html(&caps[1]);
        let val = strip_inline_html(&caps[2]);
        if val.is_empty() {
            continue;
        }
        if label.contains("ISBN") && isbn.is_none() {
            isbn = Some(val.replace('-', ""));
        } else if label.contains("出版") && label.contains("社") && publisher.is_none() {
            publisher = Some(val);
        } else if label.contains("出版") && label.contains("时间") && pub_date_raw.is_none() {
            pub_date_raw = Some(val);
        } else if label.contains("作者") && author.is_none() {
            author = Some(val);
        } else if label.contains("中图分类") && clc_code.is_none() {
            clc_code = Some(val);
        }
    }

    if title.is_none() {
        static RE_H1: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"(?is)<h1[^>]*>(.*?)</h1>").unwrap());
        title = RE_H1
            .captures(html)
            .map(|c| strip_inline_html(&c[1]))
            .filter(|s| !s.is_empty());
    }
    if cover_url.is_none() {
        cover_url = RE_XINHUA_IMG
            .captures(html)
            .and_then(|c| normalize_xinhua_cover_url(&c[1]));
    }

    let final_title = title?;

    let mut summary: Option<String> = None;
    if let Some(caps) = RE_DATA_DETAIL_ATTR.captures(html) {
        let unescaped = decode_html_entities(&caps[1]).replace("&quot;", "\"");
        if let Ok(tabs) = serde_json::from_str::<Vec<XhsdDetailTab>>(&unescaped) {
            for tab in tabs {
                if tab.title.contains("内容") {
                    let cleaned = strip_html_to_text(&tab.content)
                        .trim_start_matches("【内容简介】：")
                        .trim_start_matches("【内容简介】:")
                        .trim()
                        .to_string();
                    if !cleaned.is_empty() {
                        summary = Some(cleaned);
                        break;
                    }
                }
            }
        }
    }

    let creators: Vec<String> = author
        .map(|a| {
            a.split(['/', '、', '，', ','])
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    let (year, published_date) = pub_date_raw
        .as_deref()
        .map(parse_year_and_date)
        .unwrap_or((None, None));

    let genres = clc_code
        .as_deref()
        .map(map_clc_code_to_genres)
        .unwrap_or_default();

    Some(ScrapedMetadata {
        provider_id: "xinhua".to_string(),
        external_id: item_id.to_string(),
        target: Some(ScrapeTargetKind::Book),
        title: Some(final_title),
        creators,
        publisher_or_studio: publisher,
        year,
        published_date,
        language: Some("zh-CN".to_string()),
        isbn,
        summary,
        genres: genres.clone(),
        tags: genres,
        cover_url,
        ..Default::default()
    })
}

#[async_trait]
impl ScraperProviderPort for XinhuaBookProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "xinhua".to_string(),
            name: "新华书店".to_string(),
            description: "新华书店网上商城正版图书库（擅长中文出版物 ISBN 精准核验、中图分类号映射与官方无水印封面）".to_string(),
            website: "https://www.xhsd.com".to_string(),
            supported_targets: vec![ScrapeTargetKind::Book],
            requires_api_key: false,
        }
    }

    async fn search(
        &self,
        query: &ScrapeQuery,
        creds: &ProviderCredentials,
    ) -> DomainResult<Vec<ScrapeCandidate>> {
        if let Some(item_id) =
            extract_numeric_id_from_query(&query.keyword, &["item.xhsd.com/items/", "items/"])
            && query.keyword.contains("xhsd.com")
            && let Ok(detail) = self
                .fetch_detail(&item_id, ScrapeTargetKind::Book, "zh-CN", creds)
                .await
            && let Some(title) = detail.title
        {
            return Ok(vec![ScrapeCandidate {
                provider_id: "xinhua".to_string(),
                provider_name: "新华书店".to_string(),
                external_id: detail.external_id,
                target: ScrapeTargetKind::Book,
                title,
                subtitle: detail.publisher_or_studio,
                year: detail.year,
                summary: detail.summary,
                cover_url: detail.cover_url,
                synced_lyrics: None,
                plain_lyrics: None,
            }]);
        }

        let isbn_clean = query
            .isbn
            .as_deref()
            .map(|s| s.replace('-', "").trim().to_string())
            .filter(|s| s.len() >= 10 && s.bytes().all(|b| b.is_ascii_alphanumeric()));

        let search_kw = if let Some(ref iv) = isbn_clean {
            iv.clone()
        } else if !query.keyword.trim().is_empty() {
            let mut kw = query.keyword.trim().to_string();
            if let Some(author) = query
                .artist_or_author
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                && !kw.to_lowercase().contains(&author.to_lowercase())
            {
                kw.push(' ');
                kw.push_str(author);
            }
            kw
        } else {
            return Ok(Vec::new());
        };

        let client = build_browser_http_client(creds, false)?;
        let resp = client
            .get(XHSD_SEARCH_URL)
            .header(reqwest::header::REFERER, XHSD_REFERER)
            .query(&[("keyword", search_kw.as_str())])
            .send()
            .await
            .map_err(|e| DomainError::Storage(format!("新华书店 search failed: {e}")))?;

        if !resp.status().is_success() {
            return Ok(Vec::new());
        }
        let html = read_response_text(resp, UTF_8).await?;

        let score_kw = if !query.keyword.trim().is_empty() {
            query.keyword.trim()
        } else {
            search_kw.as_str()
        };

        let mut scored: Vec<(i32, usize, ScrapeCandidate)> = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for (idx, li_caps) in RE_PRODUCT_LI.captures_iter(&html).enumerate() {
            let block = &li_caps[1];
            let Some(link_caps) = RE_ITEM_LINK.captures(block) else {
                continue;
            };
            let item_id = link_caps[1].to_string();
            if !seen.insert(item_id.clone()) {
                continue;
            }
            let title = RE_PRODUCT_DESC_A
                .captures(block)
                .map(|c| strip_inline_html(&c[1]))
                .unwrap_or_default();
            if title.is_empty() {
                continue;
            }
            let sub = RE_PRODUCT_AUTHOR
                .captures(block)
                .map(|c| strip_inline_html(&c[1]))
                .filter(|s| !s.is_empty());
            let cover_url = RE_XINHUA_IMG
                .captures(block)
                .and_then(|c| normalize_xinhua_cover_url(&c[1]));

            let mut score = score_candidate_relevance(score_kw, query.year, &title, None, None, 0);
            if isbn_clean.is_some() && idx == 0 {
                score += 800;
            }

            scored.push((
                score,
                idx,
                ScrapeCandidate {
                    provider_id: "xinhua".to_string(),
                    provider_name: "新华书店".to_string(),
                    external_id: item_id,
                    target: ScrapeTargetKind::Book,
                    title,
                    subtitle: sub,
                    year: None,
                    summary: None,
                    cover_url,
                    synced_lyrics: None,
                    plain_lyrics: None,
                },
            ));
        }

        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        Ok(scored.into_iter().take(12).map(|(_, _, c)| c).collect())
    }

    async fn fetch_detail(
        &self,
        external_id: &str,
        _target: ScrapeTargetKind,
        _locale: &str,
        creds: &ProviderCredentials,
    ) -> DomainResult<ScrapedMetadata> {
        let id = external_id.trim();
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
            return Err(DomainError::Validation(format!(
                "invalid 新华书店 ID '{id}'"
            )));
        }
        let client = build_browser_http_client(creds, false)?;
        let url = format!("{XHSD_ITEM_PREFIX}{id}");
        let resp = client
            .get(&url)
            .header(reqwest::header::REFERER, XHSD_REFERER)
            .send()
            .await
            .map_err(|e| DomainError::Storage(format!("新华书店 detail failed: {e}")))?;
        if !resp.status().is_success() {
            return Err(DomainError::NotFound(format!(
                "新华书店 item '{id}' HTTP {}",
                resp.status()
            )));
        }
        let html = read_response_text(resp, UTF_8).await?;
        parse_xinhua_detail_html(id, &html)
            .ok_or_else(|| DomainError::NotFound(format!("新华书店 item '{id}' parse empty")))
    }
}

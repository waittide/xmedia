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
use crate::infra::scrapers::cn_music_common::read_json_response;
use crate::infra::scrapers::cn_novel_common::{
    build_browser_http_client, extract_numeric_id_from_query, normalize_cover_url,
    parse_year_and_date, read_response_text, strip_html_to_text, strip_inline_html,
};

pub struct DoubanBookProvider;

const DOUBAN_SUGGEST_URL: &str = "https://book.douban.com/j/subject_suggest";
const DOUBAN_WEB_SEARCH_URL: &str = "https://www.douban.com/search";
const DOUBAN_SUBJECT_PREFIX: &str = "https://book.douban.com/subject/";
const DOUBAN_ISBN_PREFIX: &str = "https://book.douban.com/isbn/";
pub const DOUBAN_REFERER: &str = "https://book.douban.com/";

static RE_LD_JSON: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<script[^>]+type="application/ld\+json"[^>]*>(.*?)</script>"#).unwrap()
});
static RE_INFO_BLOCK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)<div[^>]+id="info"[^>]*>(.*?)</div>"#).unwrap());
static RE_RATING_AVG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<strong[^>]+property="v:average"[^>]*>(.*?)</strong>"#).unwrap()
});
static RE_NBG_COVER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<a[^>]+class="[^"]*\bnbg\b[^"]*"[^>]+href="([^"]+)""#).unwrap()
});
static RE_MAINPIC_IMG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<div[^>]+id="mainpic"[^>]*>.*?<img[^>]+src="([^"]+)""#).unwrap()
});
static RE_SINGLE_INTRO: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<div[^>]+class="[^"]*\bintro\b[^"]*"[^>]*>(.*?)</div>"#).unwrap()
});
static RE_TAG_CRITERIA: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"criteria\s*=\s*'([^']+)'"#).unwrap());
static RE_SEARCH_CARD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<div[^>]+class="result"[^>]*>(.*?)</div>\s*</div>"#).unwrap()
});
static RE_SUBJECT_ID_IN_TEXT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"subject[/%2F]+(\d{5,12})"#).unwrap());
static RE_CARD_TITLE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)<h3[^>]*>.*?<a[^>]*>(.*?)</a>"#).unwrap());
static RE_CARD_SUB: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<span[^>]+class="subject-cast"[^>]*>(.*?)</span>"#).unwrap()
});
static RE_CARD_IMG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)<img[^>]+src="([^"]*doubanio\.com[^"]+)""#).unwrap());

#[derive(Debug, Deserialize, Default)]
struct DoubanSuggestItem {
    #[serde(default)]
    id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    author_name: String,
    #[serde(default)]
    year: String,
    #[serde(default)]
    pic: String,
    #[serde(default, rename = "type")]
    item_type: String,
}

#[derive(Debug, Deserialize, Default)]
struct DoubanLdPerson {
    #[serde(default)]
    name: String,
}

#[derive(Debug, Deserialize, Default)]
struct DoubanLdBook {
    #[serde(default)]
    name: String,
    #[serde(default)]
    author: Vec<DoubanLdPerson>,
    #[serde(default)]
    isbn: String,
}

pub(crate) fn upgrade_douban_cover(raw: &str) -> Option<String> {
    let u = normalize_cover_url(raw)?;
    if u.contains("update_image") || u.contains("book-default") {
        return None;
    }
    Some(
        u.replace("/view/subject/s/public/", "/view/subject/l/public/")
            .replace("/view/subject/m/public/", "/view/subject/l/public/")
            .replace("/spic/", "/lpic/")
            .replace("/mpic/", "/lpic/"),
    )
}

pub(crate) fn parse_douban_subject_html(
    subject_id: &str,
    html: &str,
    target: ScrapeTargetKind,
) -> Option<ScrapedMetadata> {
    if html.contains("sec.douban.com") || html.contains("页面不存在") {
        return None;
    }

    let ld_book: Option<DoubanLdBook> = RE_LD_JSON.captures(html).and_then(|c| {
        let cleaned: String = c[1]
            .chars()
            .filter(|&ch| ch >= ' ' || ch == '\n' || ch == '\t')
            .collect();
        serde_json::from_str(&cleaned).ok()
    });

    let mut title = ld_book
        .as_ref()
        .map(|b| strip_inline_html(&b.name))
        .filter(|s| !s.is_empty());

    let mut creators: Vec<String> = ld_book
        .as_ref()
        .map(|b| {
            b.author
                .iter()
                .map(|a| strip_inline_html(&a.name))
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();

    let mut isbn = ld_book
        .as_ref()
        .map(|b| b.isbn.trim().to_string())
        .filter(|s| !s.is_empty());

    let mut subtitle: Option<String> = None;
    let mut original_title: Option<String> = None;
    let mut publisher: Option<String> = None;
    let mut pub_date_raw: Option<String> = None;
    let mut series: Option<String> = None;

    if title.is_none() {
        static RE_V_ITEM: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r#"(?is)<span[^>]+property="v:itemreviewed"[^>]*>(.*?)</span>"#).unwrap()
        });
        title = RE_V_ITEM
            .captures(html)
            .map(|c| strip_inline_html(&c[1]))
            .filter(|s| !s.is_empty());
    }

    if let Some(info_caps) = RE_INFO_BLOCK.captures(html) {
        let info_text = strip_html_to_text(&info_caps[1]);
        for raw_line in info_text.lines() {
            let Some((k_raw, v_raw)) = raw_line
                .split_once(':')
                .or_else(|| raw_line.split_once('：'))
            else {
                continue;
            };
            let k = k_raw.trim();
            let v = v_raw.trim();
            if v.is_empty() {
                continue;
            }
            match k {
                "作者" if creators.is_empty() => {
                    for part in v.split(['/', '、']) {
                        let a = part.trim();
                        if !a.is_empty() && !creators.iter().any(|e| e == a) {
                            creators.push(a.to_string());
                        }
                    }
                }
                "译者" => {
                    for part in v.split(['/', '、']) {
                        let t = part.trim();
                        if !t.is_empty() {
                            let labeled = format!("{t} (译)");
                            if !creators.contains(&labeled) && !creators.iter().any(|e| e == t) {
                                creators.push(labeled);
                            }
                        }
                    }
                }
                "出版社" => publisher = Some(v.to_string()),
                "出品方" if publisher.is_none() => publisher = Some(v.to_string()),
                "副标题" => subtitle = Some(v.to_string()),
                "原作名" => original_title = Some(v.to_string()),
                "出版年" | "出版时间" => pub_date_raw = Some(v.to_string()),
                "丛书" => series = Some(v.to_string()),
                "ISBN" if isbn.is_none() => isbn = Some(v.replace('-', "")),
                _ => {}
            }
        }
    }

    let mut full_title = title?;
    if let Some(sub) = subtitle.filter(|s| !s.is_empty() && !full_title.contains(s)) {
        full_title = format!("{full_title}：{sub}");
    }

    let (year, published_date) = pub_date_raw
        .as_deref()
        .map(parse_db_year_and_date)
        .unwrap_or((None, None));

    let rating = RE_RATING_AVG
        .captures(html)
        .and_then(|c| strip_inline_html(&c[1]).parse::<f32>().ok())
        .filter(|&r| r > 0.0);

    let cover_url = RE_NBG_COVER
        .captures(html)
        .and_then(|c| upgrade_douban_cover(&c[1]))
        .or_else(|| {
            RE_MAINPIC_IMG
                .captures(html)
                .and_then(|c| upgrade_douban_cover(&c[1]))
        });

    // Prefer the last `.intro` inside `#link-report` (which holds the unfolded `.all.hidden` text)
    let report_slice = if let Some(idx) = html.find("id=\"link-report\"") {
        let tail = &html[idx..];
        let end = tail.find("id=\"author-intro\"").unwrap_or_else(|| {
            let mut limit = tail.len().min(25_000);
            while limit > 0 && !tail.is_char_boundary(limit) {
                limit -= 1;
            }
            limit
        });
        &tail[..end]
    } else {
        html
    };
    let summary = RE_SINGLE_INTRO
        .captures_iter(report_slice)
        .filter_map(|c| {
            let txt = strip_html_to_text(&c[1]);
            (!txt.is_empty() && !txt.contains("(展开全部)")).then_some(txt)
        })
        .last();

    let mut tags = Vec::new();
    if let Some(caps) = RE_TAG_CRITERIA.captures(html) {
        for part in caps[1].split('|') {
            if let Some(tag_raw) = part.strip_prefix("7:") {
                let t = strip_inline_html(tag_raw);
                if !t.is_empty() && !tags.contains(&t) {
                    tags.push(t);
                }
            }
        }
    }
    let genres: Vec<String> = tags.iter().take(3).cloned().collect();

    let series_or_album =
        series.or_else(|| (target == ScrapeTargetKind::Comic).then(|| full_title.clone()));

    Some(ScrapedMetadata {
        provider_id: "douban".to_string(),
        external_id: subject_id.to_string(),
        target: Some(target),
        title: Some(full_title),
        original_title,
        series_or_album,
        creators,
        publisher_or_studio: publisher,
        year,
        published_date,
        language: Some("zh-CN".to_string()),
        isbn,
        rating,
        summary,
        genres,
        tags,
        cover_url,
        ..Default::default()
    })
}

fn parse_db_year_and_date(raw: &str) -> (Option<i32>, Option<String>) {
    let (y, d) = parse_year_and_date(raw);
    if y.is_some() && d.as_deref().is_some_and(|s| s.len() >= 8) {
        return (y, d);
    }
    static RE_YM: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\b((?:19|20)\d{2})[-/.年](\d{1,2})\b").unwrap());
    if let Some(caps) = RE_YM.captures(raw)
        && let (Ok(yr), Ok(mo)) = (caps[1].parse::<i32>(), caps[2].parse::<u32>())
        && (1900..=2100).contains(&yr)
        && (1..=12).contains(&mo)
    {
        return (Some(yr), Some(format!("{yr:04}-{mo:02}-01")));
    }
    (y, d)
}

#[async_trait]
impl ScraperProviderPort for DoubanBookProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "douban".to_string(),
            name: "豆瓣读书".to_string(),
            description: "豆瓣读书中文出版物与漫画单行本权威资料库（支持 ISBN/书名检索、丛书、译者、评分、简介与代理高清封面）".to_string(),
            website: "https://book.douban.com".to_string(),
            supported_targets: vec![ScrapeTargetKind::Book, ScrapeTargetKind::Comic],
            requires_api_key: false,
        }
    }

    async fn search(
        &self,
        query: &ScrapeQuery,
        creds: &ProviderCredentials,
    ) -> DomainResult<Vec<ScrapeCandidate>> {
        let target = query.target.unwrap_or(ScrapeTargetKind::Book);

        if let Some(direct_id) =
            extract_numeric_id_from_query(&query.keyword, &["book.douban.com/subject/", "subject/"])
            && query.keyword.contains("douban.com")
            && let Ok(detail) = self.fetch_detail(&direct_id, target, "zh-CN", creds).await
            && let Some(title) = detail.title
        {
            return Ok(vec![ScrapeCandidate {
                provider_id: "douban".to_string(),
                provider_name: "豆瓣读书".to_string(),
                external_id: detail.external_id,
                target,
                title,
                subtitle: (!detail.creators.is_empty()).then(|| detail.creators.join(" / ")),
                year: detail.year,
                summary: detail.summary,
                cover_url: detail.cover_url,
                synced_lyrics: None,
                plain_lyrics: None,
            }]);
        }

        let client = build_browser_http_client(creds, false)?;
        let isbn_clean = query
            .isbn
            .as_deref()
            .map(|s| s.replace('-', "").trim().to_string())
            .filter(|s| s.len() >= 10 && s.bytes().all(|b| b.is_ascii_alphanumeric()));

        // 1. Direct ISBN lookup via `/isbn/{isbn}/` -> `/subject/{id}/`
        if let Some(ref isbn_val) = isbn_clean {
            let isbn_url = format!("{DOUBAN_ISBN_PREFIX}{isbn_val}/");
            if let Ok(resp) = client
                .get(&isbn_url)
                .header(reqwest::header::REFERER, DOUBAN_REFERER)
                .send()
                .await
                && resp.status().is_success()
            {
                let final_u = resp.url().as_str().to_string();
                if let Some(sub_id) = extract_numeric_id_from_query(&final_u, &["subject/"])
                    && let Ok(html) = read_response_text(resp, UTF_8).await
                    && let Some(detail) = parse_douban_subject_html(&sub_id, &html, target)
                    && let Some(title) = detail.title
                {
                    let mut sub = Vec::new();
                    if !detail.creators.is_empty() {
                        sub.push(detail.creators.join(" / "));
                    }
                    if let Some(pub_s) = detail.publisher_or_studio {
                        sub.push(pub_s);
                    }
                    return Ok(vec![ScrapeCandidate {
                        provider_id: "douban".to_string(),
                        provider_name: "豆瓣读书".to_string(),
                        external_id: sub_id,
                        target,
                        title,
                        subtitle: (!sub.is_empty()).then(|| sub.join(" · ")),
                        year: detail.year,
                        summary: detail.summary,
                        cover_url: detail.cover_url,
                        synced_lyrics: None,
                        plain_lyrics: None,
                    }]);
                }
            }
        }

        let mut search_kw = if !query.keyword.trim().is_empty() {
            query.keyword.trim().to_string()
        } else if let Some(ref isbn_val) = isbn_clean {
            isbn_val.clone()
        } else {
            return Ok(Vec::new());
        };
        if let Some(author) = query
            .artist_or_author
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            && !search_kw.to_lowercase().contains(&author.to_lowercase())
        {
            search_kw.push(' ');
            search_kw.push_str(author);
        }

        let mut candidates: Vec<(i32, usize, ScrapeCandidate)> = Vec::new();
        let mut seen_ids = std::collections::HashSet::new();

        // 2. Fast JSON `subject_suggest`
        if let Ok(resp) = client
            .get(DOUBAN_SUGGEST_URL)
            .header(reqwest::header::REFERER, DOUBAN_REFERER)
            .query(&[("q", search_kw.as_str())])
            .send()
            .await
            && resp.status().is_success()
            && let Ok(items) =
                read_json_response::<Vec<DoubanSuggestItem>>(resp, "豆瓣 suggest").await
        {
            for (idx, it) in items.into_iter().enumerate() {
                if it.id.trim().is_empty() || (!it.item_type.is_empty() && it.item_type != "b") {
                    continue;
                }
                let title = strip_inline_html(&it.title);
                if title.is_empty() || !seen_ids.insert(it.id.clone()) {
                    continue;
                }
                let (year, _) = parse_year_and_date(&it.year);
                let author = strip_inline_html(&it.author_name);
                let score = score_candidate_relevance(
                    query.keyword.trim(),
                    query.year,
                    &title,
                    None,
                    year,
                    0,
                );
                candidates.push((
                    score,
                    idx,
                    ScrapeCandidate {
                        provider_id: "douban".to_string(),
                        provider_name: "豆瓣读书".to_string(),
                        external_id: it.id,
                        target,
                        title,
                        subtitle: (!author.is_empty()).then_some(author),
                        year,
                        summary: None,
                        cover_url: upgrade_douban_cover(&it.pic),
                        synced_lyrics: None,
                        plain_lyrics: None,
                    },
                ));
            }
        }

        // 3. Supplement via web search (`cat=1001`) if suggest had few results
        if candidates.len() < 3
            && let Ok(resp) = client
                .get(DOUBAN_WEB_SEARCH_URL)
                .header(reqwest::header::REFERER, DOUBAN_REFERER)
                .query(&[("cat", "1001"), ("q", search_kw.as_str())])
                .send()
                .await
            && resp.status().is_success()
            && let Ok(html) = read_response_text(resp, UTF_8).await
            && !html.contains("sec.douban.com")
        {
            for (idx, caps) in RE_SEARCH_CARD.captures_iter(&html).enumerate() {
                let block = &caps[1];
                let Some(id_caps) = RE_SUBJECT_ID_IN_TEXT.captures(block) else {
                    continue;
                };
                let sid = id_caps[1].to_string();
                if !seen_ids.insert(sid.clone()) {
                    continue;
                }
                let Some(t_caps) = RE_CARD_TITLE.captures(block) else {
                    continue;
                };
                let title = strip_inline_html(&t_caps[1]);
                if title.is_empty() {
                    continue;
                }
                let sub = RE_CARD_SUB
                    .captures(block)
                    .map(|c| strip_inline_html(&c[1]))
                    .filter(|s| !s.is_empty());
                let year = sub.as_deref().and_then(|s| parse_year_and_date(s).0);
                let cover_url = RE_CARD_IMG
                    .captures(block)
                    .and_then(|c| upgrade_douban_cover(&c[1]));
                let score = score_candidate_relevance(
                    query.keyword.trim(),
                    query.year,
                    &title,
                    None,
                    year,
                    0,
                );
                candidates.push((
                    score,
                    100 + idx,
                    ScrapeCandidate {
                        provider_id: "douban".to_string(),
                        provider_name: "豆瓣读书".to_string(),
                        external_id: sid,
                        target,
                        title,
                        subtitle: sub,
                        year,
                        summary: None,
                        cover_url,
                        synced_lyrics: None,
                        plain_lyrics: None,
                    },
                ));
            }
        }

        candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        Ok(candidates.into_iter().take(15).map(|(_, _, c)| c).collect())
    }

    async fn fetch_detail(
        &self,
        external_id: &str,
        target: ScrapeTargetKind,
        _locale: &str,
        creds: &ProviderCredentials,
    ) -> DomainResult<ScrapedMetadata> {
        let id = external_id.trim();
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
            return Err(DomainError::Validation(format!(
                "invalid 豆瓣读书 ID '{id}'"
            )));
        }
        let client = build_browser_http_client(creds, false)?;
        let url = format!("{DOUBAN_SUBJECT_PREFIX}{id}/");
        let resp = client
            .get(&url)
            .header(reqwest::header::REFERER, DOUBAN_REFERER)
            .send()
            .await
            .map_err(|e| DomainError::Storage(format!("豆瓣读书 detail failed: {e}")))?;
        if !resp.status().is_success() {
            return Err(DomainError::NotFound(format!(
                "豆瓣读书 subject '{id}' HTTP {}",
                resp.status()
            )));
        }
        let html = read_response_text(resp, UTF_8).await?;
        parse_douban_subject_html(id, &html, target).ok_or_else(|| {
            DomainError::NotFound(format!("豆瓣读书 subject '{id}' blocked or missing"))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_subject_html_handles_multibyte_char_at_report_slice_boundary() {
        let mut html = String::from(
            r#"<html><body><h1><span property="v:itemreviewed">测试书名</span></h1><div id="link-report"><div class="intro"><p>简介内容</p></div>"#,
        );
        let link_idx = html.find(r#"id="link-report""#).unwrap();
        let pad_len = 24_999 - (html.len() - link_idx);
        html.push_str(&"a".repeat(pad_len));
        // '园' occupies bytes 24999..25002 relative to id="link-report"
        html.push_str("园</div></body></html>");

        let parsed = parse_douban_subject_html("1234567", &html, ScrapeTargetKind::Book)
            .expect("should parse without UTF-8 boundary panic");
        assert_eq!(parsed.title.as_deref(), Some("测试书名"));
        assert_eq!(parsed.summary.as_deref(), Some("简介内容"));
    }
}

use async_trait::async_trait;
use serde::Deserialize;
use xmedia_core::domain::error::{DomainError, DomainResult};

use crate::application::ports::ScraperProviderPort;
use crate::domain::config::ProviderCredentials;
use crate::domain::scraper::{
    ProviderDescriptor, ScrapeCandidate, ScrapeQuery, ScrapeTargetKind, ScrapedMetadata,
    score_candidate_relevance,
};
use crate::infra::scrapers::html_util::{
    build_browser_http_client, normalize_cover_url, read_json_response, strip_html_to_text,
    strip_inline_html,
};

pub struct NeoDbProvider;

const NEODB_SEARCH_URL: &str = "https://neodb.social/api/catalog/search";
const NEODB_BOOK_PREFIX: &str = "https://neodb.social/api/book/";

#[derive(Debug, Deserialize, Default)]
pub(crate) struct NeoDbBookItem {
    #[serde(default)]
    pub uuid: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub display_title: Option<serde_json::Value>,
    #[serde(default)]
    pub orig_title: Option<serde_json::Value>,
    #[serde(default)]
    pub subtitle: Option<serde_json::Value>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub brief: Option<String>,
    #[serde(default)]
    pub cover_image_url: Option<String>,
    #[serde(default)]
    pub rating: Option<f32>,
    #[serde(default)]
    pub rating_count: Option<i64>,
    #[serde(default)]
    pub author: serde_json::Value,
    #[serde(default)]
    pub translator: serde_json::Value,
    #[serde(default)]
    pub pub_house: Option<serde_json::Value>,
    #[serde(default)]
    pub publisher: Option<serde_json::Value>,
    #[serde(default)]
    pub pub_year: Option<i32>,
    #[serde(default)]
    pub pub_month: Option<i32>,
    #[serde(default)]
    pub isbn: Option<serde_json::Value>,
    #[serde(default)]
    pub series: Option<serde_json::Value>,
    #[serde(default)]
    pub language: Option<serde_json::Value>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Deserialize, Default)]
struct NeoDbSearchEnvelope {
    #[serde(default)]
    data: Vec<NeoDbBookItem>,
}

fn extract_neodb_uuid(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if let Some(idx) = trimmed.find("neodb.social/book/") {
        let after = &trimmed[idx + "neodb.social/book/".len()..];
        let id: String = after
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect();
        if id.len() >= 16 {
            return Some(id);
        }
    }
    if trimmed.len() == 22
        && trimmed.bytes().all(|b| b.is_ascii_alphanumeric())
        && trimmed.bytes().any(|b| b.is_ascii_alphabetic())
        && trimmed.bytes().any(|b| b.is_ascii_digit())
    {
        return Some(trimmed.to_string());
    }
    None
}

fn val_first_str(val: &Option<serde_json::Value>) -> Option<String> {
    match val {
        Some(serde_json::Value::String(s)) => {
            let t = s.trim();
            (!t.is_empty()).then(|| t.to_string())
        }
        Some(serde_json::Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str())
            .map(str::trim)
            .find(|s| !s.is_empty())
            .map(str::to_string),
        _ => None,
    }
}

fn val_str_vec(val: &serde_json::Value) -> Vec<String> {
    match val {
        serde_json::Value::Array(arr) => arr
            .iter()
            .filter_map(|v| v.as_str())
            .map(strip_inline_html)
            .filter(|s| !s.is_empty())
            .collect(),
        serde_json::Value::String(s) => {
            let t = strip_inline_html(s);
            (!t.is_empty()).then_some(vec![t]).unwrap_or_default()
        }
        _ => Vec::new(),
    }
}

pub(crate) fn map_neodb_detail(item: NeoDbBookItem, target: ScrapeTargetKind) -> ScrapedMetadata {
    let disp = val_first_str(&item.display_title);
    let base_title = disp
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(&item.title);
    let mut title = strip_inline_html(base_title);
    if let Some(sub) = val_first_str(&item.subtitle)
        .map(|s| strip_inline_html(&s))
        .filter(|s| !s.is_empty() && !title.contains(s))
    {
        title = format!("{title}：{sub}");
    }

    let original_title = val_first_str(&item.orig_title)
        .map(|s| strip_inline_html(&s))
        .filter(|s| !s.is_empty());

    let mut creators = val_str_vec(&item.author);
    for t in val_str_vec(&item.translator) {
        let labeled = format!("{t} (译)");
        if !creators.contains(&labeled) && !creators.contains(&t) {
            creators.push(labeled);
        }
    }

    let publisher = val_first_str(&item.pub_house)
        .or_else(|| val_first_str(&item.publisher))
        .map(|p| strip_inline_html(&p))
        .filter(|s| !s.is_empty());

    let year = item.pub_year.filter(|&y| (1000..=2100).contains(&y));
    let published_date = match (year, item.pub_month.filter(|&m| (1..=12).contains(&m))) {
        (Some(y), Some(m)) => Some(format!("{y:04}-{m:02}-01")),
        (Some(y), None) => Some(y.to_string()),
        _ => None,
    };

    let series_or_album = val_first_str(&item.series)
        .map(|s| strip_inline_html(&s))
        .filter(|s| !s.is_empty())
        .or_else(|| {
            (target == ScrapeTargetKind::Comic && !title.is_empty()).then(|| title.clone())
        });

    let summary = item
        .description
        .or(item.brief)
        .map(|d| strip_html_to_text(&d))
        .filter(|s| !s.is_empty());

    let tags: Vec<String> = item
        .tags
        .into_iter()
        .map(|t| strip_inline_html(&t))
        .filter(|s| !s.is_empty())
        .take(15)
        .collect();
    let genres: Vec<String> = tags.iter().take(3).cloned().collect();

    let cover_url = item
        .cover_image_url
        .as_deref()
        .and_then(normalize_cover_url);
    let isbn = val_first_str(&item.isbn);
    let language = val_first_str(&item.language);

    ScrapedMetadata {
        provider_id: "neodb".to_string(),
        external_id: item.uuid,
        target: Some(target),
        title: (!title.is_empty()).then_some(title),
        original_title,
        series_or_album,
        creators,
        publisher_or_studio: publisher,
        year,
        published_date,
        language,
        isbn,
        rating: item.rating.filter(|&r| r > 0.0),
        summary,
        genres,
        tags,
        cover_url,
        ..Default::default()
    }
}

#[async_trait]
impl ScraperProviderPort for NeoDbProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "neodb".to_string(),
            name: "NeoDB".to_string(),
            description: "联邦宇宙开放书影音资料库（免密钥 JSON 接口，覆盖中文出版图书、译作、漫画单行本、ISBN、丛书与无防盗链高清封面）".to_string(),
            website: "https://neodb.social".to_string(),
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

        if let Some(uuid) = extract_neodb_uuid(&query.keyword)
            && let Ok(detail) = self.fetch_detail(&uuid, target, "zh-CN", creds).await
            && let Some(title) = detail.title
        {
            return Ok(vec![ScrapeCandidate {
                provider_id: "neodb".to_string(),
                provider_name: "NeoDB".to_string(),
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
            .filter(|s| !s.is_empty());

        let mut items: Vec<NeoDbBookItem> = Vec::new();
        if let Some(ref isbn_q) = isbn_clean
            && let Ok(resp) = client
                .get(NEODB_SEARCH_URL)
                .query(&[("query", isbn_q.as_str()), ("category", "book")])
                .send()
                .await
            && resp.status().is_success()
            && let Ok(env) = read_json_response::<NeoDbSearchEnvelope>(resp, "NeoDB ISBN").await
        {
            items = env.data;
        }

        if items.is_empty() && !query.keyword.trim().is_empty() {
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
            let resp = client
                .get(NEODB_SEARCH_URL)
                .query(&[("query", kw.as_str()), ("category", "book")])
                .send()
                .await
                .map_err(|e| DomainError::Storage(format!("NeoDB search failed: {e}")))?;
            if resp.status().is_success() {
                let env: NeoDbSearchEnvelope = read_json_response(resp, "NeoDB search").await?;
                items = env.data;
            }
        }

        let score_kw = if !query.keyword.trim().is_empty() {
            query.keyword.trim()
        } else {
            isbn_clean.as_deref().unwrap_or("")
        };

        let mut scored: Vec<(i32, usize, ScrapeCandidate)> = Vec::new();
        for (idx, item) in items.into_iter().enumerate() {
            if item.uuid.trim().is_empty() {
                continue;
            }
            let disp = val_first_str(&item.display_title);
            let title = strip_inline_html(
                disp.as_deref()
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or(&item.title),
            );
            if title.is_empty() {
                continue;
            }
            let year = item.pub_year.filter(|&y| (1000..=2100).contains(&y));
            let votes = u32::try_from(item.rating_count.unwrap_or(0)).unwrap_or(0);
            let orig_t = val_first_str(&item.orig_title);
            let item_isbn = val_first_str(&item.isbn);
            let mut score = score_candidate_relevance(
                score_kw,
                query.year,
                &title,
                orig_t.as_deref(),
                year,
                votes,
            );
            if let (Some(want_isbn), Some(got_isbn)) = (isbn_clean.as_deref(), item_isbn.as_deref())
                && got_isbn.replace('-', "").eq_ignore_ascii_case(want_isbn)
            {
                score += 1500;
            }

            let authors_list = val_str_vec(&item.author);
            let mut sub_parts: Vec<String> = Vec::new();
            if !authors_list.is_empty() {
                sub_parts.push(authors_list.join(" / "));
            }
            if let Some(pub_h) = val_first_str(&item.pub_house)
                .or_else(|| val_first_str(&item.publisher))
                .map(|s| strip_inline_html(&s))
                .filter(|s| !s.is_empty())
            {
                sub_parts.push(pub_h);
            }

            let summary = item
                .brief
                .or(item.description)
                .map(|d| strip_inline_html(&d))
                .filter(|s| !s.is_empty());

            scored.push((
                score,
                idx,
                ScrapeCandidate {
                    provider_id: "neodb".to_string(),
                    provider_name: "NeoDB".to_string(),
                    external_id: item.uuid,
                    target,
                    title,
                    subtitle: (!sub_parts.is_empty()).then(|| sub_parts.join(" · ")),
                    year,
                    summary,
                    cover_url: item
                        .cover_image_url
                        .as_deref()
                        .and_then(normalize_cover_url),
                    synced_lyrics: None,
                    plain_lyrics: None,
                },
            ));
        }

        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        Ok(scored.into_iter().take(15).map(|(_, _, c)| c).collect())
    }

    async fn fetch_detail(
        &self,
        external_id: &str,
        target: ScrapeTargetKind,
        _locale: &str,
        creds: &ProviderCredentials,
    ) -> DomainResult<ScrapedMetadata> {
        let uuid = external_id.trim();
        if uuid.is_empty()
            || uuid.len() > 64
            || !uuid
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(DomainError::Validation("invalid NeoDB uuid".into()));
        }
        let client = build_browser_http_client(creds, false)?;
        let url = format!("{NEODB_BOOK_PREFIX}{uuid}");
        let resp = client
            .get(&url)
            .send()
            .await
            .map_err(|e| DomainError::Storage(format!("NeoDB detail failed: {e}")))?;
        if !resp.status().is_success() {
            return Err(DomainError::NotFound(format!(
                "NeoDB book '{uuid}' not found (HTTP {})",
                resp.status()
            )));
        }
        let item: NeoDbBookItem = read_json_response(resp, "NeoDB detail").await?;
        Ok(map_neodb_detail(item, target))
    }
}

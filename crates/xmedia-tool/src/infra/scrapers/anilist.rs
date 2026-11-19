use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
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
    strip_html_to_text, strip_inline_html,
};

pub struct AniListProvider;

const ANILIST_GRAPHQL_URL: &str = "https://graphql.anilist.co";

const SEARCH_GQL: &str = r#"
query ($search: String, $formats: [MediaFormat]) {
  Page(page: 1, perPage: 15) {
    media(search: $search, type: MANGA, format_in: $formats, sort: [SEARCH_MATCH, POPULARITY_DESC]) {
      id
      title { romaji english native }
      synonyms
      startDate { year month day }
      averageScore
      popularity
      description(asHtml: false)
      coverImage { extraLarge large }
      genres
    }
  }
}
"#;

const DETAIL_GQL: &str = r#"
query ($id: Int) {
  Media(id: $id, type: MANGA) {
    id
    countryOfOrigin
    title { romaji english native }
    synonyms
    startDate { year month day }
    averageScore
    description(asHtml: false)
    coverImage { extraLarge large }
    bannerImage
    genres
    tags { name rank isMediaSpoiler }
    staff(perPage: 16) {
      edges {
        role
        node { name { full native } }
      }
    }
  }
}
"#;

#[derive(Debug, Deserialize, Default)]
pub(crate) struct AlTitle {
    #[serde(default)]
    pub romaji: Option<String>,
    #[serde(default)]
    pub english: Option<String>,
    #[serde(default)]
    pub native: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct AlDate {
    #[serde(default)]
    pub year: Option<i32>,
    #[serde(default)]
    pub month: Option<u32>,
    #[serde(default)]
    pub day: Option<u32>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AlCover {
    #[serde(default)]
    pub extra_large: Option<String>,
    #[serde(default)]
    pub large: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AlTag {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub rank: Option<i32>,
    #[serde(default)]
    pub is_media_spoiler: bool,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct AlPersonName {
    #[serde(default)]
    pub full: Option<String>,
    #[serde(default)]
    pub native: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct AlStaffNode {
    #[serde(default)]
    pub name: AlPersonName,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct AlStaffEdge {
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub node: AlStaffNode,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct AlStaffConnection {
    #[serde(default)]
    pub edges: Vec<AlStaffEdge>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AlMedia {
    #[serde(default)]
    pub id: u64,
    #[serde(default)]
    pub country_of_origin: Option<String>,
    #[serde(default)]
    pub title: AlTitle,
    #[serde(default)]
    pub synonyms: Vec<String>,
    #[serde(default)]
    pub start_date: Option<AlDate>,
    #[serde(default)]
    pub average_score: Option<f32>,
    #[serde(default)]
    pub popularity: Option<u32>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub cover_image: Option<AlCover>,
    #[serde(default)]
    pub banner_image: Option<String>,
    #[serde(default)]
    pub genres: Vec<String>,
    #[serde(default)]
    pub tags: Vec<AlTag>,
    #[serde(default)]
    pub staff: Option<AlStaffConnection>,
}

#[derive(Debug, Deserialize, Default)]
struct AlPageData {
    #[serde(default)]
    media: Vec<AlMedia>,
}

#[derive(Debug, Deserialize, Default)]
struct AlSearchData {
    #[serde(default, rename = "Page")]
    page: Option<AlPageData>,
}

#[derive(Debug, Deserialize, Default)]
struct AlDetailData {
    #[serde(default, rename = "Media")]
    media: Option<AlMedia>,
}

#[derive(Debug, Deserialize, Default)]
struct AlEnvelope<T> {
    #[serde(default)]
    data: Option<T>,
}

fn has_hanzi_without_kana(s: &str) -> bool {
    let mut saw_hanzi = false;
    for c in s.chars() {
        let code = c as u32;
        if (0x3040..=0x30FF).contains(&code) {
            return false;
        }
        if (0x4E00..=0x9FFF).contains(&code) {
            saw_hanzi = true;
        }
    }
    saw_hanzi
}

fn choose_anilist_titles(m: &AlMedia, locale: &str) -> (Option<String>, Option<String>) {
    let native = m
        .title
        .native
        .as_deref()
        .map(strip_inline_html)
        .filter(|s| !s.is_empty());
    let english = m
        .title
        .english
        .as_deref()
        .map(strip_inline_html)
        .filter(|s| !s.is_empty());
    let romaji = m
        .title
        .romaji
        .as_deref()
        .map(strip_inline_html)
        .filter(|s| !s.is_empty());
    let zh_synonym = m
        .synonyms
        .iter()
        .map(|s| strip_inline_html(s))
        .find(|s| has_hanzi_without_kana(s));

    let primary = if locale.starts_with("en") {
        english
            .clone()
            .or_else(|| romaji.clone())
            .or_else(|| native.clone())
    } else if locale.starts_with("zh") {
        zh_synonym
            .or_else(|| native.clone())
            .or_else(|| english.clone())
            .or_else(|| romaji.clone())
    } else {
        native
            .clone()
            .or_else(|| romaji.clone())
            .or_else(|| english.clone())
    };

    let orig = native.or(romaji).or(english);
    (primary, orig)
}

pub(crate) fn map_anilist_detail(
    m: AlMedia,
    target: ScrapeTargetKind,
    locale: &str,
) -> ScrapedMetadata {
    let (title, original_title) = choose_anilist_titles(&m, locale);
    let mut creators = Vec::new();
    let mut illustrators = Vec::new();

    if let Some(staff) = m.staff {
        for edge in staff.edges {
            let role_low = edge.role.to_ascii_lowercase();
            let person = if locale.starts_with("en") {
                edge.node.name.full.or(edge.node.name.native)
            } else {
                edge.node.name.native.or(edge.node.name.full)
            };
            let Some(name) = person
                .map(|s| strip_inline_html(&s))
                .filter(|s| !s.is_empty())
            else {
                continue;
            };
            let is_story = role_low.contains("story")
                || role_low.contains("original creator")
                || role_low.contains("author");
            let is_art = role_low.contains("art") || role_low.contains("illustrat");

            if is_story && !creators.contains(&name) {
                creators.push(name.clone());
            }
            if is_art && !illustrators.contains(&name) {
                illustrators.push(name);
            }
        }
    }

    let year = m
        .start_date
        .as_ref()
        .and_then(|d| d.year)
        .filter(|&y| (1800..=2100).contains(&y));
    let published_date = m.start_date.as_ref().and_then(|d| {
        let y = d.year.filter(|&y| (1800..=2100).contains(&y))?;
        let mo = d.month.filter(|&v| (1..=12).contains(&v)).unwrap_or(1);
        let dy = d.day.filter(|&v| (1..=31).contains(&v)).unwrap_or(1);
        Some(format!("{y:04}-{mo:02}-{dy:02}"))
    });

    let cover_url = m
        .cover_image
        .as_ref()
        .and_then(|c| c.extra_large.as_deref().or(c.large.as_deref()))
        .and_then(normalize_cover_url);
    let backdrop_url = m.banner_image.as_deref().and_then(normalize_cover_url);

    let genres: Vec<String> = m
        .genres
        .into_iter()
        .map(|g| strip_inline_html(&g))
        .filter(|s| !s.is_empty())
        .collect();
    let tags: Vec<String> = m
        .tags
        .into_iter()
        .filter(|t| !t.is_media_spoiler && t.rank.unwrap_or(100) >= 50)
        .map(|t| strip_inline_html(&t.name))
        .filter(|s| !s.is_empty())
        .take(12)
        .collect();

    let summary = m
        .description
        .as_deref()
        .map(strip_html_to_text)
        .filter(|s| !s.is_empty());

    let language = match m.country_of_origin.as_deref() {
        Some("JP") => Some("ja".to_string()),
        Some("KR") => Some("ko".to_string()),
        Some("CN" | "TW") => Some("zh".to_string()),
        _ => None,
    };

    let series_or_album = (target == ScrapeTargetKind::Comic)
        .then(|| title.clone())
        .flatten();

    ScrapedMetadata {
        provider_id: "anilist".to_string(),
        external_id: m.id.to_string(),
        target: Some(target),
        title,
        original_title,
        series_or_album,
        creators,
        illustrators_or_directors: illustrators,
        year,
        published_date,
        language,
        rating: m.average_score.filter(|&s| s > 0.0).map(|s| s / 10.0),
        summary,
        genres,
        tags,
        cover_url,
        backdrop_url,
        ..Default::default()
    }
}

#[async_trait]
impl ScraperProviderPort for AniListProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "anilist".to_string(),
            name: "AniList".to_string(),
            description: "全球 ACGN 权威 GraphQL 资料库（免密钥支持日漫/韩漫/轻小说，提供超清竖版封面、原作/作画分离、多语别名与流派标签）".to_string(),
            website: "https://anilist.co".to_string(),
            supported_targets: vec![ScrapeTargetKind::Comic, ScrapeTargetKind::Book],
            requires_api_key: false,
        }
    }

    async fn search(
        &self,
        query: &ScrapeQuery,
        creds: &ProviderCredentials,
    ) -> DomainResult<Vec<ScrapeCandidate>> {
        let target = query.target.unwrap_or(ScrapeTargetKind::Comic);
        let locale = query.locale.as_deref().unwrap_or("zh-CN");

        if let Some(id) = extract_numeric_id_from_query(&query.keyword, &["anilist.co/manga/"])
            && query.keyword.contains("anilist.co")
            && let Ok(detail) = self.fetch_detail(&id, target, locale, creds).await
            && let Some(title) = detail.title
        {
            return Ok(vec![ScrapeCandidate {
                provider_id: "anilist".to_string(),
                provider_name: "AniList".to_string(),
                external_id: detail.external_id,
                target,
                title,
                subtitle: detail.original_title,
                year: detail.year,
                summary: detail.summary,
                cover_url: detail.cover_url,
                synced_lyrics: None,
                plain_lyrics: None,
            }]);
        }

        let kw = query.keyword.trim();
        if kw.is_empty() {
            return Ok(Vec::new());
        }

        let formats = if target == ScrapeTargetKind::Book {
            json!(["NOVEL"])
        } else {
            json!(["MANGA", "ONE_SHOT"])
        };

        let client = build_browser_http_client(creds, false)?;
        let resp = client
            .post(ANILIST_GRAPHQL_URL)
            .json(&json!({
                "query": SEARCH_GQL,
                "variables": {
                    "search": kw,
                    "formats": formats,
                }
            }))
            .send()
            .await
            .map_err(|e| DomainError::Storage(format!("AniList search failed: {e}")))?;

        if !resp.status().is_success() {
            return Ok(Vec::new());
        }

        let env: AlEnvelope<AlSearchData> = read_json_response(resp, "AniList search").await?;
        let list = env
            .data
            .and_then(|d| d.page)
            .map(|p| p.media)
            .unwrap_or_default();

        let mut scored: Vec<(i32, usize, ScrapeCandidate)> = Vec::new();
        for (idx, m) in list.into_iter().enumerate() {
            if m.id == 0 {
                continue;
            }
            let (primary_opt, orig_opt) = choose_anilist_titles(&m, locale);
            let Some(title) = primary_opt else {
                continue;
            };
            let year = m
                .start_date
                .as_ref()
                .and_then(|d| d.year)
                .filter(|&y| (1800..=2100).contains(&y));
            let votes = m.popularity.unwrap_or(0);
            let score =
                score_candidate_relevance(kw, query.year, &title, orig_opt.as_deref(), year, votes);
            let sub = orig_opt.filter(|o| o != &title);
            let cover_url = m
                .cover_image
                .as_ref()
                .and_then(|c| c.extra_large.as_deref().or(c.large.as_deref()))
                .and_then(normalize_cover_url);
            let summary = m
                .description
                .as_deref()
                .map(strip_html_to_text)
                .filter(|s| !s.is_empty());

            scored.push((
                score,
                idx,
                ScrapeCandidate {
                    provider_id: "anilist".to_string(),
                    provider_name: "AniList".to_string(),
                    external_id: m.id.to_string(),
                    target,
                    title,
                    subtitle: sub,
                    year,
                    summary,
                    cover_url,
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
        locale: &str,
        creds: &ProviderCredentials,
    ) -> DomainResult<ScrapedMetadata> {
        let id_num: u64 = external_id
            .trim()
            .parse()
            .map_err(|_| DomainError::Validation(format!("invalid AniList ID '{external_id}'")))?;

        let client = build_browser_http_client(creds, false)?;
        let resp = client
            .post(ANILIST_GRAPHQL_URL)
            .json(&json!({
                "query": DETAIL_GQL,
                "variables": { "id": id_num }
            }))
            .send()
            .await
            .map_err(|e| DomainError::Storage(format!("AniList detail failed: {e}")))?;

        if !resp.status().is_success() {
            return Err(DomainError::NotFound(format!(
                "AniList media '{id_num}' HTTP {}",
                resp.status()
            )));
        }

        let env: AlEnvelope<AlDetailData> = read_json_response(resp, "AniList detail").await?;
        let media = env
            .data
            .and_then(|d| d.media)
            .ok_or_else(|| DomainError::NotFound(format!("AniList media '{id_num}' not found")))?;

        Ok(map_anilist_detail(media, target, locale))
    }
}

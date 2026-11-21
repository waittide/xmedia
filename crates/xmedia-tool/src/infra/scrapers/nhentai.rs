use std::collections::BTreeSet;
use std::sync::LazyLock;

use async_trait::async_trait;
use jiff::Timestamp;
use regex::Regex;
use serde::Deserialize;
use xmedia_core::domain::error::{DomainError, DomainResult};

use crate::domain::config::ProviderCredentials;
use crate::domain::ports::ScraperProviderPort;
use crate::domain::scraper::{
    ProviderDescriptor, ScrapeCandidate, ScrapeQuery, ScrapeTargetKind, ScrapedMetadata,
    score_candidate_relevance,
};
use crate::infra::scrapers::cn_novel_common::{build_browser_http_client, read_response_text};

pub struct NhentaiProvider;

const NHENTAI_API_BASE: &str = "https://nhentai.net/api";
const NHENTAI_WEB_BASE: &str = "https://nhentai.net";
const NHENTAI_THUMB_BASE: &str = "https://t.nhentai.net/galleries";

static RE_GALLERY_ID_QUERY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(?:nhentai\.net/g/|#)?(\d{1,8})(?:/|\s*$)").unwrap());

#[derive(Debug, Deserialize)]
struct NhentaiSearchResponse {
    #[serde(default)]
    result: Vec<NhentaiGallery>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct NhentaiGallery {
    pub id: serde_json::Value,
    #[serde(default)]
    pub media_id: String,
    #[serde(default)]
    pub title: NhentaiTitle,
    #[serde(default)]
    pub images: NhentaiImages,
    #[serde(default)]
    pub scanlator: Option<String>,
    #[serde(default)]
    pub upload_date: Option<i64>,
    #[serde(default)]
    pub tags: Vec<NhentaiTag>,
    #[serde(default)]
    pub num_pages: Option<u32>,
    #[serde(default)]
    pub num_favorites: Option<u32>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct NhentaiTitle {
    #[serde(default)]
    pub english: Option<String>,
    #[serde(default)]
    pub japanese: Option<String>,
    #[serde(default)]
    pub pretty: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct NhentaiImages {
    #[serde(default)]
    pub cover: Option<NhentaiImageSpec>,
    #[serde(default)]
    pub thumbnail: Option<NhentaiImageSpec>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct NhentaiImageSpec {
    #[serde(default, rename = "t")]
    pub image_type: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct NhentaiTag {
    #[serde(default, rename = "type")]
    pub tag_type: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub count: u64,
}

impl NhentaiGallery {
    pub fn id_string(&self) -> String {
        match &self.id {
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::String(s) => s.trim().to_string(),
            _ => String::new(),
        }
    }
}

pub(crate) fn extract_nhentai_gallery_id(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.chars().all(|c| c.is_ascii_digit()) && (1..=8).contains(&trimmed.len()) {
        return Some(trimmed.to_string());
    }
    if trimmed.starts_with('#')
        && trimmed.len() >= 2
        && trimmed[1..].chars().all(|c| c.is_ascii_digit())
    {
        return Some(trimmed[1..].to_string());
    }
    if trimmed.to_ascii_lowercase().contains("nhentai.net/g/") {
        return RE_GALLERY_ID_QUERY
            .captures(trimmed)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().to_string());
    }
    None
}

fn image_ext_from_code(code: &str) -> &'static str {
    match code.trim().to_ascii_lowercase().as_str() {
        "p" | "png" => "png",
        "g" | "gif" => "gif",
        "w" | "webp" => "webp",
        _ => "jpg",
    }
}

pub(crate) fn build_nhentai_cover_url(gallery: &NhentaiGallery) -> Option<String> {
    let media_id = gallery.media_id.trim();
    if media_id.is_empty() || !media_id.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let ext = gallery
        .images
        .cover
        .as_ref()
        .or(gallery.images.thumbnail.as_ref())
        .map(|s| image_ext_from_code(&s.image_type))
        .unwrap_or("jpg");
    Some(format!("{NHENTAI_THUMB_BASE}/{media_id}/cover.{ext}"))
}

fn map_language_iso(lang_name: &str) -> Option<&'static str> {
    match lang_name.trim().to_ascii_lowercase().as_str() {
        "chinese" => Some("zh"),
        "japanese" => Some("ja"),
        "english" => Some("en"),
        "korean" => Some("ko"),
        _ => None,
    }
}

fn format_upload_date(unix_secs: Option<i64>) -> (Option<i32>, Option<String>) {
    let Some(secs) = unix_secs.filter(|&s| s > 0) else {
        return (None, None);
    };
    let Ok(ts) = Timestamp::from_second(secs) else {
        return (None, None);
    };
    let zdt = ts.to_zoned(jiff::tz::TimeZone::UTC);
    let date = zdt.date();
    let year = i32::from(date.year());
    let iso = format!("{:04}-{:02}-{:02}", date.year(), date.month(), date.day());
    (Some(year), Some(iso))
}

fn pick_primary_and_original_title(title: &NhentaiTitle) -> (String, Option<String>) {
    let jp = title
        .japanese
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let pretty = title
        .pretty
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let en = title
        .english
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let primary = jp
        .or(pretty)
        .or(en)
        .unwrap_or("Untitled Gallery")
        .to_string();

    let secondary = if jp.is_some() {
        en.or(pretty)
    } else {
        en.filter(|s| Some(*s) != pretty)
    }
    .filter(|s| *s != primary.as_str())
    .map(ToString::to_string);

    (primary, secondary)
}

pub(crate) fn map_nhentai_candidate(gallery: &NhentaiGallery) -> ScrapeCandidate {
    let (title, _) = pick_primary_and_original_title(&gallery.title);
    let (year, _) = format_upload_date(gallery.upload_date);

    let mut artists = Vec::new();
    let mut groups = Vec::new();
    let mut langs = Vec::new();
    for tag in &gallery.tags {
        let name = tag.name.trim();
        if name.is_empty() {
            continue;
        }
        match tag.tag_type.as_str() {
            "artist" => artists.push(name.to_string()),
            "group" => groups.push(name.to_string()),
            "language" if name != "translated" => langs.push(name.to_string()),
            _ => {}
        }
    }

    let mut sub_parts = Vec::new();
    if !artists.is_empty() {
        sub_parts.push(artists.join(", "));
    } else if !groups.is_empty() {
        sub_parts.push(groups.join(", "));
    }
    if !langs.is_empty() {
        sub_parts.push(langs.join("/"));
    }
    if let Some(pages) = gallery.num_pages.filter(|&p| p > 0) {
        sub_parts.push(format!("{pages}P"));
    }

    ScrapeCandidate {
        provider_id: "nhentai".into(),
        provider_name: "nhentai".into(),
        external_id: gallery.id_string(),
        target: ScrapeTargetKind::Comic,
        title,
        subtitle: (!sub_parts.is_empty()).then(|| sub_parts.join(" · ")),
        year,
        summary: None,
        cover_url: build_nhentai_cover_url(gallery),
        synced_lyrics: None,
        plain_lyrics: None,
    }
}

pub(crate) fn map_nhentai_detail(gallery: &NhentaiGallery) -> ScrapedMetadata {
    let (title, original_title) = pick_primary_and_original_title(&gallery.title);
    let (year, published_date) = format_upload_date(gallery.upload_date);

    let mut artists = Vec::new();
    let mut groups = Vec::new();
    let mut parodies = Vec::new();
    let mut characters = Vec::new();
    let mut categories = Vec::new();
    let mut raw_tags: Vec<(&str, u64)> = Vec::new();
    let mut language: Option<String> = None;

    for tag in &gallery.tags {
        let name = tag.name.trim();
        if name.is_empty() {
            continue;
        }
        match tag.tag_type.as_str() {
            "artist" => artists.push(name.to_string()),
            "group" => groups.push(name.to_string()),
            "parody" if !name.eq_ignore_ascii_case("original") => {
                parodies.push(name.to_string());
            }
            "character" => characters.push(name.to_string()),
            "category" => categories.push(name.to_string()),
            "language" if !name.eq_ignore_ascii_case("translated") => {
                if language.is_none() {
                    language = map_language_iso(name)
                        .map(ToString::to_string)
                        .or_else(|| Some(name.to_string()));
                }
            }
            "tag" => raw_tags.push((name, tag.count)),
            _ => {}
        }
    }

    raw_tags.sort_by_key(|a| std::cmp::Reverse(a.1));

    let creators = if !artists.is_empty() {
        artists.clone()
    } else {
        groups.clone()
    };

    let mut genre_set = BTreeSet::new();
    for c in &categories {
        genre_set.insert(c.clone());
    }
    for p in &parodies {
        genre_set.insert(p.clone());
    }

    let mut tags = Vec::new();
    let mut seen_tags = BTreeSet::new();
    for ch in &characters {
        if seen_tags.insert(ch.to_ascii_lowercase()) {
            tags.push(ch.clone());
        }
    }
    for (t, _) in raw_tags.into_iter().take(24) {
        if seen_tags.insert(t.to_ascii_lowercase()) {
            tags.push(t.to_string());
        }
    }

    let mut summary_parts = Vec::new();
    if let Some(pages) = gallery.num_pages.filter(|&p| p > 0) {
        summary_parts.push(format!("页数: {pages}"));
    }
    if !parodies.is_empty() {
        summary_parts.push(format!("原作: {}", parodies.join(", ")));
    }
    if !characters.is_empty() {
        summary_parts.push(format!("角色: {}", characters.join(", ")));
    }
    if let Some(scanlator) = gallery
        .scanlator
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        summary_parts.push(format!("汉化/扫描: {scanlator}"));
    }
    if let Some(favs) = gallery.num_favorites.filter(|&f| f > 0) {
        summary_parts.push(format!("收藏: {favs}"));
    }

    let id_str = gallery.id_string();
    let mut external_ids = std::collections::BTreeMap::new();
    if !id_str.is_empty() {
        external_ids.insert("nhentai".to_string(), id_str.clone());
    }

    ScrapedMetadata {
        provider_id: "nhentai".into(),
        external_id: id_str,
        target: Some(ScrapeTargetKind::Comic),
        title: Some(title),
        original_title,
        series_or_album: parodies.first().cloned(),
        creators,
        illustrators_or_directors: artists,
        publisher_or_studio: (!groups.is_empty()).then(|| groups.join(", ")),
        year,
        published_date,
        language,
        summary: (!summary_parts.is_empty()).then(|| summary_parts.join(" · ")),
        genres: genre_set.into_iter().collect(),
        tags,
        cover_url: build_nhentai_cover_url(gallery),
        external_ids,
        ..Default::default()
    }
}

fn check_cloudflare_challenge(status: reqwest::StatusCode, body: &str) -> DomainResult<()> {
    let lower = body.to_ascii_lowercase();
    let looks_cf = status.as_u16() == 403
        || status.as_u16() == 503
        || lower.contains("just a moment")
        || lower.contains("cf-browser-verification")
        || lower.contains("challenge-platform");
    if !status.is_success() {
        if looks_cf {
            return Err(DomainError::Storage(format!(
                "nhentai 触发了 Cloudflare 访问限制 (HTTP {})，请检查设置中的 HTTP 代理出口后重试",
                status.as_u16()
            )));
        }
        return Err(DomainError::Storage(format!(
            "nhentai API 返回 HTTP {}",
            status.as_u16()
        )));
    }
    if lower.contains("just a moment") && lower.contains("cloudflare") {
        return Err(DomainError::Storage(
            "nhentai 返回了 Cloudflare 人机校验页，请更换可用代理出口后重试".into(),
        ));
    }
    Ok(())
}

async fn fetch_gallery_by_id(
    gallery_id: &str,
    creds: &ProviderCredentials,
) -> DomainResult<NhentaiGallery> {
    let client = build_browser_http_client(creds, false)?;
    let url = format!("{NHENTAI_API_BASE}/gallery/{gallery_id}");
    let resp = client
        .get(&url)
        .header(reqwest::header::REFERER, format!("{NHENTAI_WEB_BASE}/"))
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(|e| DomainError::Storage(format!("请求 nhentai 画廊 #{gallery_id} 失败: {e}")))?;

    let status = resp.status();
    let body = read_response_text(resp, encoding_rs::UTF_8).await?;
    check_cloudflare_challenge(status, &body)?;

    serde_json::from_str::<NhentaiGallery>(&body)
        .map_err(|e| DomainError::Storage(format!("解析 nhentai 画廊 JSON 失败: {e}")))
}

#[async_trait]
impl ScraperProviderPort for NhentaiProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "nhentai".into(),
            name: "nhentai".into(),
            description:
                "成人向同人志/漫画元数据与封面（支持关键词或数字画廊 ID，需开启 R18 刮削源）".into(),
            website: NHENTAI_WEB_BASE.into(),
            supported_targets: vec![ScrapeTargetKind::Comic],
            requires_api_key: false,
        }
    }

    fn is_configured(&self, creds: &ProviderCredentials) -> bool {
        creds.adult_scrapers_enabled()
    }

    async fn search(
        &self,
        query: &ScrapeQuery,
        creds: &ProviderCredentials,
    ) -> DomainResult<Vec<ScrapeCandidate>> {
        if !creds.adult_scrapers_enabled() {
            return Ok(Vec::new());
        }
        let target = query.target.unwrap_or(ScrapeTargetKind::Comic);
        if target != ScrapeTargetKind::Comic {
            return Ok(Vec::new());
        }
        let kw = query.keyword.trim();
        if kw.is_empty() {
            return Ok(Vec::new());
        }

        if let Some(gid) = extract_nhentai_gallery_id(kw)
            && let Ok(gallery) = fetch_gallery_by_id(&gid, creds).await
        {
            return Ok(vec![map_nhentai_candidate(&gallery)]);
        }

        let client = build_browser_http_client(creds, false)?;
        let resp = client
            .get(format!("{NHENTAI_API_BASE}/galleries/search"))
            .query(&[("query", kw), ("sort", "popular"), ("page", "1")])
            .header(reqwest::header::REFERER, format!("{NHENTAI_WEB_BASE}/"))
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|e| DomainError::Storage(format!("请求 nhentai 搜索失败: {e}")))?;

        let status = resp.status();
        let body = read_response_text(resp, encoding_rs::UTF_8).await?;
        check_cloudflare_challenge(status, &body)?;

        let parsed: NhentaiSearchResponse = serde_json::from_str(&body)
            .map_err(|e| DomainError::Storage(format!("解析 nhentai 搜索结果失败: {e}")))?;

        let mut scored: Vec<(i32, ScrapeCandidate)> = parsed
            .result
            .iter()
            .take(16)
            .map(|g| {
                let cand = map_nhentai_candidate(g);
                let score = score_candidate_relevance(
                    kw,
                    query.year,
                    &cand.title,
                    cand.subtitle.as_deref(),
                    cand.year,
                    g.num_favorites.unwrap_or(0),
                );
                (score, cand)
            })
            .collect();

        scored.sort_by_key(|a| std::cmp::Reverse(a.0));
        Ok(scored.into_iter().take(12).map(|(_, c)| c).collect())
    }

    async fn fetch_detail(
        &self,
        external_id: &str,
        _target: ScrapeTargetKind,
        _locale: &str,
        creds: &ProviderCredentials,
    ) -> DomainResult<ScrapedMetadata> {
        if !creds.adult_scrapers_enabled() {
            return Err(DomainError::Validation(
                "尚未开启 R18 / 成人向刮削源，请在设置页启用后重试".into(),
            ));
        }
        let gid = extract_nhentai_gallery_id(external_id).ok_or_else(|| {
            DomainError::Validation(format!("无效的 nhentai 画廊 ID: '{external_id}'"))
        })?;
        let gallery = fetch_gallery_by_id(&gid, creds).await?;
        Ok(map_nhentai_detail(&gallery))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_gallery_ids_from_numbers_hashes_and_urls() {
        assert_eq!(
            extract_nhentai_gallery_id("177013").as_deref(),
            Some("177013")
        );
        assert_eq!(
            extract_nhentai_gallery_id("#123456").as_deref(),
            Some("123456")
        );
        assert_eq!(
            extract_nhentai_gallery_id("https://nhentai.net/g/456789/").as_deref(),
            Some("456789")
        );
        assert_eq!(extract_nhentai_gallery_id("Frieren doujin"), None);
    }

    #[test]
    fn maps_nhentai_gallery_payload_preferring_japanese_title() {
        let sample = r#"{
            "id": 456789,
            "media_id": "2345678",
            "title": {
                "english": "[CircleA (ArtistB)] Sample Title [Chinese]",
                "japanese": "[サークルA (作家B)] サンプルタイトル [中国翻訳]",
                "pretty": "Sample Title"
            },
            "images": {
                "cover": { "t": "p" },
                "thumbnail": { "t": "j" }
            },
            "scanlator": "DemoGroup",
            "upload_date": 1704067200,
            "num_pages": 36,
            "num_favorites": 1200,
            "tags": [
                { "type": "artist", "name": "artist b", "count": 50 },
                { "type": "group", "name": "circle a", "count": 30 },
                { "type": "parody", "name": "sample series", "count": 400 },
                { "type": "character", "name": "heroine", "count": 200 },
                { "type": "language", "name": "translated", "count": 9999 },
                { "type": "language", "name": "chinese", "count": 8888 },
                { "type": "category", "name": "doujinshi", "count": 7777 },
                { "type": "tag", "name": "full color", "count": 6000 }
            ]
        }"#;
        let gallery: NhentaiGallery = serde_json::from_str(sample).unwrap();
        let meta = map_nhentai_detail(&gallery);

        assert_eq!(meta.provider_id, "nhentai");
        assert_eq!(meta.external_id, "456789");
        assert_eq!(
            meta.title.as_deref(),
            Some("[サークルA (作家B)] サンプルタイトル [中国翻訳]")
        );
        assert_eq!(
            meta.original_title.as_deref(),
            Some("[CircleA (ArtistB)] Sample Title [Chinese]")
        );
        assert_eq!(meta.series_or_album.as_deref(), Some("sample series"));
        assert_eq!(meta.creators, vec!["artist b"]);
        assert_eq!(meta.publisher_or_studio.as_deref(), Some("circle a"));
        assert_eq!(meta.language.as_deref(), Some("zh"));
        assert_eq!(meta.year, Some(2024));
        assert_eq!(meta.published_date.as_deref(), Some("2024-01-01"));
        assert_eq!(
            meta.cover_url.as_deref(),
            Some("https://t.nhentai.net/galleries/2345678/cover.png")
        );
        assert!(meta.tags.contains(&"full color".to_string()));
        assert!(meta.tags.contains(&"heroine".to_string()));
    }
}

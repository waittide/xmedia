use std::collections::HashMap;

use async_trait::async_trait;
use serde::Deserialize;
use xmedia_core::domain::error::{DomainError, DomainResult};

use crate::domain::config::ProviderCredentials;
use crate::domain::ports::ScraperProviderPort;
use crate::domain::scraper::{
    ProviderDescriptor, ScrapeCandidate, ScrapeQuery, ScrapeTargetKind, ScrapedMetadata,
    score_candidate_relevance,
};
use crate::infra::scrapers::cn_music_common::read_json_response;
use crate::infra::scrapers::cn_novel_common::build_browser_http_client;

pub struct MangaDexProvider;

const MANGADEX_API_BASE: &str = "https://api.mangadex.org";

#[derive(Debug, Deserialize, Default)]
pub(crate) struct MdListResponse {
    #[serde(default)]
    pub data: Vec<MdMangaEntity>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct MdSingleResponse {
    pub data: Option<MdMangaEntity>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct MdMangaEntity {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub attributes: MdMangaAttributes,
    #[serde(default)]
    pub relationships: Vec<MdRelationship>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MdMangaAttributes {
    #[serde(default)]
    pub title: HashMap<String, String>,
    #[serde(default)]
    pub alt_titles: Vec<HashMap<String, String>>,
    #[serde(default)]
    pub description: HashMap<String, String>,
    #[serde(default)]
    pub original_language: Option<String>,
    #[serde(default)]
    pub year: Option<i32>,
    #[serde(default)]
    pub tags: Vec<MdTag>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct MdTag {
    #[serde(default)]
    pub attributes: MdTagAttributes,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct MdTagAttributes {
    #[serde(default)]
    pub name: HashMap<String, String>,
    #[serde(default)]
    pub group: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct MdRelationship {
    #[serde(rename = "type", default)]
    pub rel_type: String,
    #[serde(default)]
    pub attributes: Option<MdRelAttributes>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MdRelAttributes {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub file_name: Option<String>,
}

fn pick_localized(map: &HashMap<String, String>, preferred: &[&str]) -> Option<String> {
    for key in preferred {
        if let Some(val) = map.get(*key) {
            let trimmed = val.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    map.values()
        .map(|s| s.trim())
        .find(|s| !s.is_empty())
        .map(str::to_string)
}

fn pick_title_from_entity(attrs: &MdMangaAttributes) -> (Option<String>, Option<String>) {
    let mut merged: HashMap<String, String> = attrs.title.clone();
    for alt in &attrs.alt_titles {
        for (k, v) in alt {
            merged.entry(k.clone()).or_insert_with(|| v.clone());
        }
    }
    let display = pick_localized(&merged, &["zh", "zh-hk", "en", "ja-ro", "ja", "fr", "ko"]);
    let orig_lang = attrs.original_language.as_deref().unwrap_or("ja");
    let original = pick_localized(&merged, &[orig_lang, "ja", "ko", "fr", "en"])
        .filter(|o| display.as_deref() != Some(o.as_str()));
    (display, original)
}

fn cover_url_from_entity(entity: &MdMangaEntity) -> Option<String> {
    let file_name = entity.relationships.iter().find_map(|r| {
        if r.rel_type == "cover_art" {
            r.attributes.as_ref().and_then(|a| a.file_name.as_deref())
        } else {
            None
        }
    })?;
    if file_name.trim().is_empty() || entity.id.trim().is_empty() {
        return None;
    }
    Some(format!(
        "https://uploads.mangadex.org/covers/{}/{}.512.jpg",
        entity.id.trim(),
        file_name.trim()
    ))
}

fn authors_and_artists(entity: &MdMangaEntity) -> (Vec<String>, Vec<String>) {
    let mut authors = Vec::new();
    let mut artists = Vec::new();
    for rel in &entity.relationships {
        let Some(name) = rel
            .attributes
            .as_ref()
            .and_then(|a| a.name.as_deref())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        if rel.rel_type == "author" && !authors.iter().any(|x| x == name) {
            authors.push(name.to_string());
        } else if rel.rel_type == "artist" && !artists.iter().any(|x| x == name) {
            artists.push(name.to_string());
        }
    }
    if authors.is_empty() && !artists.is_empty() {
        authors = artists.clone();
    }
    (authors, artists)
}

pub(crate) fn map_mangadex_detail(entity: MdMangaEntity) -> ScrapedMetadata {
    let (title, original_title) = pick_title_from_entity(&entity.attributes);
    let (creators, illustrators_or_directors) = authors_and_artists(&entity);
    let cover_url = cover_url_from_entity(&entity);
    let summary = pick_localized(&entity.attributes.description, &["zh", "zh-hk", "en", "ja"]);

    let mut genres = Vec::new();
    let mut tags = Vec::new();
    for tag in &entity.attributes.tags {
        if let Some(name) = pick_localized(&tag.attributes.name, &["zh", "en"]) {
            if tag.attributes.group.as_deref() == Some("genre") {
                if !genres.contains(&name) {
                    genres.push(name);
                }
            } else if !tags.contains(&name) {
                tags.push(name);
            }
        }
    }

    ScrapedMetadata {
        provider_id: "mangadex".into(),
        external_id: entity.id,
        target: Some(ScrapeTargetKind::Comic),
        title: title.clone(),
        original_title,
        series_or_album: title,
        summary,
        creators,
        illustrators_or_directors,
        genres,
        tags,
        year: entity.attributes.year,
        language: entity.attributes.original_language,
        cover_url,
        ..Default::default()
    }
}

#[async_trait]
impl ScraperProviderPort for MangaDexProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "mangadex".into(),
            name: "MangaDex".into(),
            supported_targets: vec![ScrapeTargetKind::Comic],
            requires_api_key: false,
            website: "https://mangadex.org".into(),
            description: "全球开源漫画数据库，覆盖日漫/韩漫/欧美漫多语言标题、作者、标签与高清封面（无需 Key）".into(),
        }
    }

    fn is_configured(&self, _creds: &ProviderCredentials) -> bool {
        true
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
        let client = build_browser_http_client(creds, false)?;
        let resp = client
            .get(format!("{MANGADEX_API_BASE}/manga"))
            .query(&[
                ("title", kw),
                ("limit", "12"),
                ("includes[]", "cover_art"),
                ("includes[]", "author"),
                ("includes[]", "artist"),
                ("contentRating[]", "safe"),
                ("contentRating[]", "suggestive"),
                ("contentRating[]", "erotica"),
            ])
            .send()
            .await
            .map_err(|e| DomainError::Storage(format!("mangadex search failed: {e}")))?;

        if !resp.status().is_success() {
            return Err(DomainError::Storage(format!(
                "mangadex search returned HTTP {}",
                resp.status()
            )));
        }

        let body: MdListResponse = read_json_response(resp, "mangadex").await?;
        let mut scored: Vec<(i32, usize, ScrapeCandidate)> = Vec::new();
        for (idx, entity) in body.data.into_iter().enumerate() {
            let (title_opt, orig_opt) = pick_title_from_entity(&entity.attributes);
            let Some(title) = title_opt else {
                continue;
            };
            let (authors, artists) = authors_and_artists(&entity);
            let subtitle = if !authors.is_empty() {
                Some(authors.join(" / "))
            } else if !artists.is_empty() {
                Some(artists.join(" / "))
            } else {
                orig_opt.clone()
            };
            let summary =
                pick_localized(&entity.attributes.description, &["zh", "zh-hk", "en", "ja"]);
            let cover_url = cover_url_from_entity(&entity);
            let score = score_candidate_relevance(
                kw,
                query.year,
                &title,
                orig_opt.as_deref(),
                entity.attributes.year,
                0,
            );
            scored.push((
                score,
                idx,
                ScrapeCandidate {
                    provider_id: "mangadex".into(),
                    provider_name: "MangaDex".into(),
                    external_id: entity.id,
                    target: ScrapeTargetKind::Comic,
                    title,
                    subtitle,
                    year: entity.attributes.year,
                    summary,
                    cover_url,
                    synced_lyrics: None,
                    plain_lyrics: None,
                },
            ));
        }
        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        Ok(scored.into_iter().map(|(_, _, c)| c).collect())
    }

    async fn fetch_detail(
        &self,
        external_id: &str,
        _target: ScrapeTargetKind,
        _locale: &str,
        creds: &ProviderCredentials,
    ) -> DomainResult<ScrapedMetadata> {
        let id = external_id.trim();
        if id.is_empty() {
            return Err(DomainError::Validation("empty mangadex id".into()));
        }
        let client = build_browser_http_client(creds, false)?;
        let resp = client
            .get(format!("{MANGADEX_API_BASE}/manga/{id}"))
            .query(&[
                ("includes[]", "cover_art"),
                ("includes[]", "author"),
                ("includes[]", "artist"),
            ])
            .send()
            .await
            .map_err(|e| DomainError::Storage(format!("mangadex detail failed: {e}")))?;

        if !resp.status().is_success() {
            return Err(DomainError::Storage(format!(
                "mangadex detail returned HTTP {}",
                resp.status()
            )));
        }

        let body: MdSingleResponse = read_json_response(resp, "mangadex").await?;
        let entity = body
            .data
            .ok_or_else(|| DomainError::NotFound(format!("mangadex manga {id} not found")))?;
        Ok(map_mangadex_detail(entity))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mangadex_mapper_extracts_multilingual_title_creators_cover_and_tags() {
        let sample: MdSingleResponse = serde_json::from_str(
            r#"{
                "data": {
                    "id": "a1c7c817-4e59-43b7-9365-09675a149a6f",
                    "attributes": {
                        "title": { "en": "One Piece" },
                        "altTitles": [
                            { "zh": "海贼王" },
                            { "ja": "ワンピース" }
                        ],
                        "description": {
                            "zh": "寻找大秘宝的航海冒险故事。",
                            "en": "Gol D. Roger was known as the Pirate King."
                        },
                        "originalLanguage": "ja",
                        "year": 1997,
                        "tags": [
                            { "attributes": { "name": { "en": "Adventure" }, "group": "genre" } },
                            { "attributes": { "name": { "en": "Pirates" }, "group": "theme" } }
                        ]
                    },
                    "relationships": [
                        { "type": "author", "attributes": { "name": "Oda Eiichiro" } },
                        { "type": "artist", "attributes": { "name": "Oda Eiichiro" } },
                        { "type": "cover_art", "attributes": { "fileName": "vol1.jpg" } }
                    ]
                }
            }"#,
        )
        .unwrap();

        let meta = map_mangadex_detail(sample.data.unwrap());
        assert_eq!(meta.title.as_deref(), Some("海贼王"));
        assert_eq!(meta.original_title.as_deref(), Some("ワンピース"));
        assert_eq!(meta.creators, vec!["Oda Eiichiro"]);
        assert_eq!(meta.year, Some(1997));
        assert_eq!(meta.genres, vec!["Adventure"]);
        assert_eq!(meta.tags, vec!["Pirates"]);
        assert_eq!(
            meta.cover_url.as_deref(),
            Some(
                "https://uploads.mangadex.org/covers/a1c7c817-4e59-43b7-9365-09675a149a6f/vol1.jpg.512.jpg"
            )
        );
    }
}

use async_trait::async_trait;
use serde::Deserialize;
use xmedia_core::domain::error::{DomainError, DomainResult};

use crate::domain::config::ProviderCredentials;
use crate::domain::ports::ScraperProviderPort;
use crate::domain::scraper::{
    ProviderDescriptor, ScrapeCandidate, ScrapeQuery, ScrapeTargetKind, ScrapedMetadata,
};
use crate::infra::scrapers::http_client::build_http_client;

pub struct ItunesMusicProvider;

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ItunesItem {
    #[serde(default)]
    pub wrapper_type: String,
    #[serde(default)]
    pub track_id: Option<u64>,
    #[serde(default)]
    pub collection_id: Option<u64>,
    #[serde(default)]
    pub track_name: Option<String>,
    #[serde(default)]
    pub collection_name: Option<String>,
    #[serde(default)]
    pub artist_name: Option<String>,
    #[serde(default)]
    pub artwork_url100: Option<String>,
    #[serde(default)]
    pub release_date: Option<String>,
    #[serde(default)]
    pub track_number: Option<u32>,
    #[serde(default)]
    pub disc_number: Option<u32>,
    #[serde(default)]
    pub primary_genre_name: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct ItunesResponse {
    #[serde(default)]
    pub results: Vec<ItunesItem>,
}

fn upgrade_itunes_artwork(raw: Option<&str>, size: &str) -> Option<String> {
    let url = raw?.trim();
    if url.is_empty() {
        return None;
    }
    Some(url.replace("100x100bb", size).replace("100x100", size))
}

fn parse_year(date_opt: Option<&str>) -> Option<i32> {
    date_opt?
        .trim()
        .get(..4)?
        .parse::<i32>()
        .ok()
        .filter(|&y| y > 1850)
}

fn has_cjk(text: &str) -> bool {
    text.chars().any(|c| {
        matches!(
            c as u32,
            0x4E00..=0x9FFF | 0x3040..=0x30FF | 0x3400..=0x4DBF | 0xAC00..=0xD7AF
        )
    })
}

fn infer_storefront(text: &str, locale: Option<&str>) -> &'static str {
    if text.chars().any(|c| matches!(c as u32, 0x3040..=0x30FF)) {
        return "JP";
    }
    if has_cjk(text) {
        return "TW";
    }
    match locale.unwrap_or("zh-CN") {
        l if l.starts_with("ja") => "JP",
        l if l.starts_with("zh") => "TW",
        _ => "US",
    }
}

pub(crate) fn map_itunes_to_candidate(
    item: ItunesItem,
    target: ScrapeTargetKind,
    storefront: &str,
) -> Option<ScrapeCandidate> {
    let is_album = target == ScrapeTargetKind::MusicAlbum;
    let id_num = if is_album {
        item.collection_id.or(item.track_id)?
    } else {
        item.track_id.or(item.collection_id)?
    };
    let title = if is_album {
        item.collection_name.clone().or(item.track_name.clone())?
    } else {
        item.track_name.clone().or(item.collection_name.clone())?
    };

    let mut sub = Vec::new();
    if let Some(ref artist) = item.artist_name
        && !artist.trim().is_empty()
    {
        sub.push(artist.trim().to_string());
    }
    if !is_album
        && let Some(ref album) = item.collection_name
        && !album.trim().is_empty()
    {
        sub.push(album.trim().to_string());
    }

    Some(ScrapeCandidate {
        provider_id: "itunes".to_string(),
        provider_name: "Apple Music".to_string(),
        external_id: format!("{storefront}:{id_num}"),
        target,
        title,
        subtitle: (!sub.is_empty()).then(|| sub.join(" · ")),
        year: parse_year(item.release_date.as_deref()),
        summary: item.primary_genre_name,
        cover_url: upgrade_itunes_artwork(item.artwork_url100.as_deref(), "600x600bb"),
        synced_lyrics: None,
        plain_lyrics: None,
    })
}

pub(crate) fn map_itunes_detail(
    item: ItunesItem,
    external_id: &str,
    target: ScrapeTargetKind,
) -> ScrapedMetadata {
    let is_album = target == ScrapeTargetKind::MusicAlbum;
    let title = if is_album {
        item.collection_name.clone().or(item.track_name.clone())
    } else {
        item.track_name.clone().or(item.collection_name.clone())
    };
    let creators: Vec<String> = item
        .artist_name
        .filter(|s| !s.trim().is_empty())
        .into_iter()
        .collect();
    let genres: Vec<String> = item
        .primary_genre_name
        .filter(|s| !s.trim().is_empty())
        .into_iter()
        .collect();
    let year = parse_year(item.release_date.as_deref());

    ScrapedMetadata {
        provider_id: "itunes".to_string(),
        external_id: external_id.to_string(),
        target: Some(target),
        title,
        series_or_album: item.collection_name.filter(|s| !s.trim().is_empty()),
        creators,
        year,
        published_date: item.release_date,
        track_or_episode_number: item.track_number,
        disc_or_season_number: item.disc_number,
        genres,
        cover_url: upgrade_itunes_artwork(item.artwork_url100.as_deref(), "1000x1000bb"),
        ..Default::default()
    }
}

#[async_trait]
impl ScraperProviderPort for ItunesMusicProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "itunes".to_string(),
            name: "Apple Music".to_string(),
            description: "Apple Music / iTunes 官方曲库与高清专辑封面源（免密钥即开即用）"
                .to_string(),
            website: "https://music.apple.com".to_string(),
            supported_targets: vec![ScrapeTargetKind::MusicTrack, ScrapeTargetKind::MusicAlbum],
            requires_api_key: false,
        }
    }

    async fn search(
        &self,
        query: &ScrapeQuery,
        creds: &ProviderCredentials,
    ) -> DomainResult<Vec<ScrapeCandidate>> {
        let client = build_http_client(creds)?;
        let target = match query.target {
            Some(ScrapeTargetKind::MusicAlbum) => ScrapeTargetKind::MusicAlbum,
            _ => ScrapeTargetKind::MusicTrack,
        };
        let entity = if target == ScrapeTargetKind::MusicAlbum {
            "album"
        } else {
            "song"
        };

        let mut term = query.keyword.trim().to_string();
        if let Some(artist) = query
            .artist_or_author
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            term = format!("{artist} {term}");
        }

        let primary_store = infer_storefront(&term, query.locale.as_deref());
        let storefronts: &[&str] = if primary_store == "US" {
            &["US", "TW"]
        } else {
            &[primary_store, "CN", "US"]
        };

        let mut saw_valid_response = false;
        let mut last_err: Option<DomainError> = None;
        for &sf in storefronts {
            let resp = client
                .get("https://itunes.apple.com/search")
                .query(&[
                    ("term", term.as_str()),
                    ("media", "music"),
                    ("entity", entity),
                    ("country", sf),
                    ("limit", "12"),
                ])
                .send()
                .await
                .map_err(|e| DomainError::Storage(format!("iTunes search failed: {e}")))?;
            if !resp.status().is_success() {
                last_err = Some(DomainError::Storage(format!(
                    "iTunes search ({sf}) returned HTTP {}",
                    resp.status()
                )));
                continue;
            }
            let body: ItunesResponse = match resp.json().await {
                Ok(parsed) => {
                    saw_valid_response = true;
                    parsed
                }
                Err(e) => {
                    last_err = Some(DomainError::Storage(format!(
                        "invalid iTunes search ({sf}) JSON: {e}"
                    )));
                    continue;
                }
            };
            let mapped: Vec<ScrapeCandidate> = body
                .results
                .into_iter()
                .filter_map(|it| map_itunes_to_candidate(it, target, sf))
                .collect();
            if !mapped.is_empty() {
                return Ok(mapped);
            }
        }

        if !saw_valid_response && let Some(err) = last_err {
            return Err(err);
        }

        Ok(Vec::new())
    }

    async fn fetch_detail(
        &self,
        external_id: &str,
        target: ScrapeTargetKind,
        locale: &str,
        creds: &ProviderCredentials,
    ) -> DomainResult<ScrapedMetadata> {
        let client = build_http_client(creds)?;
        let (storefront, id_part) = external_id
            .trim()
            .split_once(':')
            .unwrap_or_else(|| (infer_storefront("", Some(locale)), external_id.trim()));

        let resp = client
            .get("https://itunes.apple.com/lookup")
            .query(&[("id", id_part), ("country", storefront)])
            .send()
            .await
            .map_err(|e| DomainError::Storage(format!("iTunes lookup failed: {e}")))?;
        if !resp.status().is_success() {
            return Err(DomainError::NotFound(format!(
                "iTunes item '{external_id}' not found"
            )));
        }
        let mut body: ItunesResponse = resp
            .json()
            .await
            .map_err(|e| DomainError::Storage(format!("iTunes JSON error: {e}")))?;
        let item = body
            .results
            .drain(..)
            .find(|r| r.wrapper_type == "track" || r.wrapper_type == "collection")
            .ok_or_else(|| DomainError::NotFound(format!("iTunes item '{external_id}' empty")))?;

        Ok(map_itunes_detail(item, external_id, target))
    }
}

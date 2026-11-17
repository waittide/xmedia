use async_trait::async_trait;
use serde::Deserialize;
use xmedia_core::domain::error::{DomainError, DomainResult};

use crate::domain::config::ProviderCredentials;
use crate::domain::ports::ScraperProviderPort;
use crate::domain::scraper::{
    ProviderDescriptor, ScrapeCandidate, ScrapeQuery, ScrapeTargetKind, ScrapedMetadata,
};
use crate::infra::scrapers::http_client::build_http_client;

pub struct LrclibProvider;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LrclibRecord {
    pub id: i64,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub track_name: Option<String>,
    #[serde(default)]
    pub artist_name: Option<String>,
    #[serde(default)]
    pub album_name: Option<String>,
    #[serde(default)]
    pub duration: Option<f64>,
    #[serde(default)]
    pub synced_lyrics: Option<String>,
    #[serde(default)]
    pub plain_lyrics: Option<String>,
}

impl LrclibRecord {
    pub(crate) fn into_candidate(self) -> ScrapeCandidate {
        let title = self
            .track_name
            .or(self.name)
            .unwrap_or_else(|| "Untitled".to_string());
        let artist = self.artist_name.unwrap_or_default();
        let album = self.album_name.unwrap_or_default();
        let dur_badge = self
            .duration
            .map(|d| {
                let secs = d.round() as u32;
                format!("{}:{:02}", secs / 60, secs % 60)
            })
            .unwrap_or_default();

        let mut parts = Vec::new();
        if !artist.trim().is_empty() {
            parts.push(artist);
        }
        if !album.trim().is_empty() {
            parts.push(album);
        }
        if !dur_badge.is_empty() {
            parts.push(dur_badge);
        }

        let preview = self
            .synced_lyrics
            .as_deref()
            .or(self.plain_lyrics.as_deref())
            .map(|s| s.lines().take(4).collect::<Vec<_>>().join(" / "));

        ScrapeCandidate {
            provider_id: "lrclib".to_string(),
            provider_name: "LRCLIB".to_string(),
            external_id: self.id.to_string(),
            target: ScrapeTargetKind::Lyrics,
            title,
            subtitle: (!parts.is_empty()).then(|| parts.join(" · ")),
            year: None,
            summary: preview,
            cover_url: None,
            synced_lyrics: self.synced_lyrics.filter(|s| !s.trim().is_empty()),
            plain_lyrics: self.plain_lyrics.filter(|s| !s.trim().is_empty()),
        }
    }

    pub(crate) fn into_metadata(self, target: ScrapeTargetKind) -> ScrapedMetadata {
        let title = self.track_name.or(self.name);
        let creators = self
            .artist_name
            .filter(|s| !s.trim().is_empty())
            .into_iter()
            .collect();
        ScrapedMetadata {
            provider_id: "lrclib".to_string(),
            external_id: self.id.to_string(),
            target: Some(target),
            title,
            series_or_album: self.album_name.filter(|s| !s.trim().is_empty()),
            creators,
            synced_lyrics: self.synced_lyrics.filter(|s| !s.trim().is_empty()),
            plain_lyrics: self.plain_lyrics.filter(|s| !s.trim().is_empty()),
            ..Default::default()
        }
    }
}

#[async_trait]
impl ScraperProviderPort for LrclibProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "lrclib".to_string(),
            name: "LRCLIB".to_string(),
            description: "开源免密钥双态歌词库（支持 LRC 时间轴逐行同步歌词与纯文本歌词）"
                .to_string(),
            website: "https://lrclib.net".to_string(),
            supported_targets: vec![ScrapeTargetKind::Lyrics, ScrapeTargetKind::MusicTrack],
            requires_api_key: false,
        }
    }

    async fn search(
        &self,
        query: &ScrapeQuery,
        creds: &ProviderCredentials,
    ) -> DomainResult<Vec<ScrapeCandidate>> {
        let client = build_http_client(creds)?;
        let kw = query.keyword.trim();
        let artist = query
            .artist_or_author
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let album = query
            .album_or_series
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());

        let mut records: Vec<LrclibRecord> = Vec::new();

        if let Some(artist_name) = artist {
            let mut params = vec![
                ("track_name", kw.to_string()),
                ("artist_name", artist_name.to_string()),
            ];
            if let Some(alb) = album {
                params.push(("album_name", alb.to_string()));
            }
            let resp = client
                .get("https://lrclib.net/api/search")
                .query(&params)
                .send()
                .await
                .map_err(|e| DomainError::Storage(format!("LRCLIB request failed: {e}")))?;
            if resp.status().is_success() {
                records = resp.json::<Vec<LrclibRecord>>().await.unwrap_or_default();
            }
        }

        if records.is_empty() {
            let q = match artist {
                Some(a) => format!("{kw} {a}"),
                None => kw.to_string(),
            };
            let resp = client
                .get("https://lrclib.net/api/search")
                .query(&[("q", q)])
                .send()
                .await
                .map_err(|e| DomainError::Storage(format!("LRCLIB search failed: {e}")))?;
            if resp.status().is_success() {
                records = resp.json::<Vec<LrclibRecord>>().await.unwrap_or_default();
            }
        }

        Ok(records
            .into_iter()
            .take(15)
            .map(LrclibRecord::into_candidate)
            .collect())
    }

    async fn fetch_detail(
        &self,
        external_id: &str,
        target: ScrapeTargetKind,
        _locale: &str,
        creds: &ProviderCredentials,
    ) -> DomainResult<ScrapedMetadata> {
        let client = build_http_client(creds)?;
        let url = format!("https://lrclib.net/api/get/{}", external_id.trim());
        let resp = client
            .get(&url)
            .send()
            .await
            .map_err(|e| DomainError::Storage(format!("LRCLIB detail failed: {e}")))?;
        if !resp.status().is_success() {
            return Err(DomainError::NotFound(format!(
                "LRCLIB entry '{external_id}' not found (HTTP {})",
                resp.status()
            )));
        }
        let rec: LrclibRecord = resp
            .json()
            .await
            .map_err(|e| DomainError::Storage(format!("LRCLIB JSON parse error: {e}")))?;
        Ok(rec.into_metadata(target))
    }
}

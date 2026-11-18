use async_trait::async_trait;
use serde::Deserialize;
use xmedia_core::domain::error::{DomainError, DomainResult};

use crate::domain::config::ProviderCredentials;
use crate::domain::ports::ScraperProviderPort;
use crate::domain::scraper::{
    ProviderDescriptor, ScrapeCandidate, ScrapeQuery, ScrapeTargetKind, ScrapedMetadata,
};
use crate::infra::scrapers::http_client::build_http_client;

pub struct MusicBrainzProvider;

#[derive(Debug, Deserialize, Default)]
pub(crate) struct MbArtistCredit {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    artist: Option<MbNamedObj>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct MbNamedObj {
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct MbTag {
    #[serde(default)]
    name: String,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct MbTrackPos {
    #[serde(default)]
    number: Option<String>,
    #[serde(default)]
    position: Option<u32>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct MbMedium {
    #[serde(default)]
    position: Option<u32>,
    #[serde(default)]
    track: Vec<MbTrackPos>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct MbLabelInfo {
    #[serde(default)]
    label: Option<MbNamedObj>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct MbReleaseGroup {
    pub id: String,
    #[serde(default, rename = "primary-type")]
    pub primary_type: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct MbReleaseItem {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default, rename = "release-group")]
    pub release_group: Option<MbReleaseGroup>,
    #[serde(default, rename = "artist-credit")]
    pub artist_credit: Vec<MbArtistCredit>,
    #[serde(default, rename = "label-info")]
    pub label_info: Vec<MbLabelInfo>,
    #[serde(default)]
    pub media: Vec<MbMedium>,
    #[serde(default)]
    pub genres: Vec<MbTag>,
    #[serde(default)]
    pub tags: Vec<MbTag>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct MbRecordingItem {
    pub id: String,
    pub title: String,
    #[serde(default, rename = "first-release-date")]
    pub first_release_date: Option<String>,
    #[serde(default, rename = "artist-credit")]
    pub artist_credit: Vec<MbArtistCredit>,
    #[serde(default)]
    pub releases: Vec<MbReleaseItem>,
    #[serde(default)]
    pub genres: Vec<MbTag>,
    #[serde(default)]
    pub tags: Vec<MbTag>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct MbRecordingSearchResp {
    #[serde(default)]
    pub recordings: Vec<MbRecordingItem>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct MbReleaseSearchResp {
    #[serde(default)]
    pub releases: Vec<MbReleaseItem>,
}

fn extract_artists(credits: &[MbArtistCredit]) -> Vec<String> {
    let mut out = Vec::new();
    for c in credits {
        let name = c
            .name
            .as_deref()
            .or_else(|| c.artist.as_ref().and_then(|a| a.name.as_deref()))
            .unwrap_or("")
            .trim();
        if !name.is_empty() && !out.iter().any(|existing| existing == name) {
            out.push(name.to_string());
        }
    }
    out
}

fn parse_year_prefix(date_opt: Option<&str>) -> Option<i32> {
    date_opt?
        .trim()
        .get(..4)?
        .parse::<i32>()
        .ok()
        .filter(|&y| y > 1000)
}

fn best_cover_for_release(rel: &MbReleaseItem) -> String {
    if let Some(ref rg) = rel.release_group
        && !rg.id.trim().is_empty()
    {
        return format!(
            "https://coverartarchive.org/release-group/{}/front-500",
            rg.id
        );
    }
    format!("https://coverartarchive.org/release/{}/front-500", rel.id)
}

fn select_best_release<'a>(
    releases: &'a [MbReleaseItem],
    preferred_album: Option<&str>,
) -> Option<&'a MbReleaseItem> {
    let pref_trimmed = preferred_album.map(str::trim).filter(|s| !s.is_empty());
    releases.iter().max_by_key(|r| {
        let exact_album = pref_trimmed.is_some_and(|p| r.title.trim().eq_ignore_ascii_case(p));
        let contains_album = pref_trimmed.is_some_and(|p| {
            let rt = r.title.trim().to_lowercase();
            let pt = p.to_lowercase();
            rt.contains(&pt) || pt.contains(&rt)
        });
        let official = r
            .status
            .as_deref()
            .is_some_and(|s| s.eq_ignore_ascii_case("official"));
        let is_album = r
            .release_group
            .as_ref()
            .and_then(|rg| rg.primary_type.as_deref())
            .is_some_and(|pt| pt.eq_ignore_ascii_case("album"));
        let has_rg = r.release_group.is_some();
        (exact_album, contains_album, official, is_album, has_rg)
    })
}

pub(crate) fn map_recording_to_candidate(
    rec: MbRecordingItem,
    preferred_album: Option<&str>,
) -> ScrapeCandidate {
    let artists = extract_artists(&rec.artist_credit);
    let best_release = select_best_release(&rec.releases, preferred_album);
    let album_title = best_release.map(|r| r.title.clone());
    let cover_url = best_release.map(best_cover_for_release);
    let year = parse_year_prefix(rec.first_release_date.as_deref())
        .or_else(|| best_release.and_then(|r| parse_year_prefix(r.date.as_deref())));

    let mut sub = Vec::new();
    if !artists.is_empty() {
        sub.push(artists.join(", "));
    }
    if let Some(alb) = album_title {
        sub.push(alb);
    }

    ScrapeCandidate {
        provider_id: "musicbrainz".to_string(),
        provider_name: "MusicBrainz".to_string(),
        external_id: rec.id,
        target: ScrapeTargetKind::MusicTrack,
        title: rec.title,
        subtitle: (!sub.is_empty()).then(|| sub.join(" · ")),
        year,
        summary: None,
        cover_url,
        synced_lyrics: None,
        plain_lyrics: None,
    }
}

pub(crate) fn map_release_to_candidate(rel: MbReleaseItem) -> ScrapeCandidate {
    let artists = extract_artists(&rel.artist_credit);
    let year = parse_year_prefix(rel.date.as_deref());
    let cover = best_cover_for_release(&rel);
    ScrapeCandidate {
        provider_id: "musicbrainz".to_string(),
        provider_name: "MusicBrainz".to_string(),
        external_id: rel.id,
        target: ScrapeTargetKind::MusicAlbum,
        title: rel.title,
        subtitle: (!artists.is_empty()).then(|| artists.join(", ")),
        year,
        summary: None,
        cover_url: Some(cover),
        synced_lyrics: None,
        plain_lyrics: None,
    }
}

pub(crate) fn map_recording_detail(rec: MbRecordingItem) -> ScrapedMetadata {
    let creators = extract_artists(&rec.artist_credit);
    let rel = select_best_release(&rec.releases, None);
    let album_title = rel.map(|r| r.title.clone());
    let cover_url = rel.map(best_cover_for_release);
    let disc_num = rel.and_then(|r| r.media.first()).and_then(|m| m.position);
    let track_num = rel
        .and_then(|r| r.media.first())
        .and_then(|m| m.track.first())
        .and_then(|t| {
            t.position.or_else(|| {
                t.number
                    .as_deref()
                    .and_then(|n| n.trim().parse::<u32>().ok())
            })
        });
    let year = parse_year_prefix(rec.first_release_date.as_deref())
        .or_else(|| rel.and_then(|r| parse_year_prefix(r.date.as_deref())));

    let genres: Vec<String> = rec
        .genres
        .into_iter()
        .map(|g| g.name)
        .filter(|s| !s.is_empty())
        .collect();
    let tags: Vec<String> = rec
        .tags
        .into_iter()
        .take(8)
        .map(|t| t.name)
        .filter(|s| !s.is_empty())
        .collect();

    ScrapedMetadata {
        provider_id: "musicbrainz".to_string(),
        external_id: rec.id,
        target: Some(ScrapeTargetKind::MusicTrack),
        title: Some(rec.title),
        series_or_album: album_title,
        creators,
        year,
        published_date: rec.first_release_date,
        track_or_episode_number: track_num,
        disc_or_season_number: disc_num,
        genres: if genres.is_empty() {
            tags.iter().take(2).cloned().collect()
        } else {
            genres
        },
        tags,
        cover_url,
        ..Default::default()
    }
}

pub(crate) fn map_release_detail(rel: MbReleaseItem) -> ScrapedMetadata {
    let creators = extract_artists(&rel.artist_credit);
    let year = parse_year_prefix(rel.date.as_deref());
    let cover = best_cover_for_release(&rel);
    let label = rel
        .label_info
        .iter()
        .find_map(|li| li.label.as_ref().and_then(|l| l.name.clone()));
    let genres: Vec<String> = rel
        .genres
        .into_iter()
        .map(|g| g.name)
        .filter(|s| !s.is_empty())
        .collect();
    let tags: Vec<String> = rel
        .tags
        .into_iter()
        .take(8)
        .map(|t| t.name)
        .filter(|s| !s.is_empty())
        .collect();

    ScrapedMetadata {
        provider_id: "musicbrainz".to_string(),
        external_id: rel.id,
        target: Some(ScrapeTargetKind::MusicAlbum),
        title: Some(rel.title.clone()),
        series_or_album: Some(rel.title),
        creators,
        publisher_or_studio: label,
        year,
        published_date: rel.date,
        genres,
        tags,
        cover_url: Some(cover),
        ..Default::default()
    }
}

fn escape_lucene(s: &str) -> String {
    s.replace('"', "\\\"")
}

#[async_trait]
impl ScraperProviderPort for MusicBrainzProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "musicbrainz".to_string(),
            name: "MusicBrainz + Cover Art".to_string(),
            description: "开放音乐百科与 Cover Art Archive 专辑封面源（免密钥即开即用）"
                .to_string(),
            website: "https://musicbrainz.org".to_string(),
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

        if query.target == Some(ScrapeTargetKind::MusicAlbum) {
            let mut q = format!("release:\"{}\"", escape_lucene(kw));
            if let Some(a) = artist {
                q.push_str(&format!(" AND artist:\"{}\"", escape_lucene(a)));
            }
            let resp = client
                .get("https://musicbrainz.org/ws/2/release")
                .query(&[("query", q.as_str()), ("fmt", "json"), ("limit", "12")])
                .send()
                .await
                .map_err(|e| {
                    DomainError::Storage(format!("MusicBrainz release search failed: {e}"))
                })?;
            if !resp.status().is_success() {
                return Ok(Vec::new());
            }
            let body: MbReleaseSearchResp = resp.json().await.unwrap_or_default();
            return Ok(body
                .releases
                .into_iter()
                .map(map_release_to_candidate)
                .collect());
        }

        let mut q = format!("recording:\"{}\"", escape_lucene(kw));
        if let Some(a) = artist {
            q.push_str(&format!(" AND artist:\"{}\"", escape_lucene(a)));
        }
        if let Some(alb) = album {
            q.push_str(&format!(" AND release:\"{}\"", escape_lucene(alb)));
        }

        let resp = client
            .get("https://musicbrainz.org/ws/2/recording")
            .query(&[("query", q.as_str()), ("fmt", "json"), ("limit", "12")])
            .send()
            .await
            .map_err(|e| {
                DomainError::Storage(format!("MusicBrainz recording search failed: {e}"))
            })?;
        if !resp.status().is_success() {
            return Ok(Vec::new());
        }
        let body: MbRecordingSearchResp = resp.json().await.unwrap_or_default();
        Ok(body
            .recordings
            .into_iter()
            .map(|rec| map_recording_to_candidate(rec, album))
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
        let id = external_id.trim();

        if target == ScrapeTargetKind::MusicAlbum {
            let url = format!("https://musicbrainz.org/ws/2/release/{id}");
            let resp = client
                .get(&url)
                .query(&[
                    ("fmt", "json"),
                    (
                        "inc",
                        "artist-credits+labels+release-groups+genres+tags+media",
                    ),
                ])
                .send()
                .await
                .map_err(|e| {
                    DomainError::Storage(format!("MusicBrainz release detail failed: {e}"))
                })?;
            if !resp.status().is_success() {
                return Err(DomainError::NotFound(format!(
                    "MusicBrainz release '{id}' not found"
                )));
            }
            let rel: MbReleaseItem = resp
                .json()
                .await
                .map_err(|e| DomainError::Storage(format!("MusicBrainz JSON error: {e}")))?;
            let mut meta = map_release_detail(rel);
            if let Some(ref cover) = meta.cover_url
                && !verify_cover_art_exists(&client, cover).await
            {
                meta.cover_url = None;
            }
            return Ok(meta);
        }

        let url = format!("https://musicbrainz.org/ws/2/recording/{id}");
        let resp = client
            .get(&url)
            .query(&[
                ("fmt", "json"),
                (
                    "inc",
                    "artist-credits+releases+release-groups+genres+tags+media",
                ),
            ])
            .send()
            .await
            .map_err(|e| {
                DomainError::Storage(format!("MusicBrainz recording detail failed: {e}"))
            })?;
        if !resp.status().is_success() {
            return Err(DomainError::NotFound(format!(
                "MusicBrainz recording '{id}' not found"
            )));
        }
        let rec: MbRecordingItem = resp
            .json()
            .await
            .map_err(|e| DomainError::Storage(format!("MusicBrainz JSON error: {e}")))?;
        let mut meta = map_recording_detail(rec);
        if let Some(ref cover) = meta.cover_url
            && !verify_cover_art_exists(&client, cover).await
        {
            meta.cover_url = None;
        }
        Ok(meta)
    }
}

async fn verify_cover_art_exists(client: &reqwest::Client, url: &str) -> bool {
    if !url.starts_with("https://coverartarchive.org/") {
        return true;
    }
    match client
        .head(url)
        .timeout(std::time::Duration::from_millis(2500))
        .send()
        .await
    {
        Ok(resp) => resp.status().is_success(),
        Err(_) => false,
    }
}

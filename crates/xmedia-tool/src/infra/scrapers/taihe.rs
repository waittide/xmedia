use std::collections::BTreeMap;

use async_trait::async_trait;
use encoding_rs::UTF_8;
use md5::{Digest, Md5};
use serde::Deserialize;
use xmedia_core::domain::error::{DomainError, DomainResult};

use crate::domain::config::ProviderCredentials;
use crate::domain::ports::ScraperProviderPort;
use crate::domain::scraper::{
    ProviderDescriptor, ScrapeCandidate, ScrapeQuery, ScrapeTargetKind, ScrapedMetadata,
};
use crate::infra::scrapers::cn_music_common::{
    build_music_search_keyword, format_duration_mmss, normalize_lrc_and_extract_plain,
    read_json_response, score_music_candidate, unescape_music_text,
};
use crate::infra::scrapers::cn_novel_common::{
    build_browser_http_client, normalize_cover_url, parse_year_and_date, read_response_text,
    strip_html_to_text, strip_inline_html,
};

pub struct TaiheMusicProvider;

const TAIHE_BASE_URL: &str = "https://music.91q.com";
const TAIHE_REFERER: &str = "https://music.91q.com/";
const TAIHE_APPID: &str = "16073360";
const TAIHE_SECRET: &str = "0b50b02fd0d73a9c4c8c3a781c30845f";

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaiheArtist {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub artist_type: i32,
    #[serde(default)]
    pub artist_type_name: String,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaiheTrackItem {
    #[serde(default, rename = "TSID")]
    pub tsid: String,
    #[serde(default)]
    pub asset_id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub artist: Vec<TaiheArtist>,
    #[serde(default)]
    pub album_title: String,
    #[serde(default)]
    pub release_date: String,
    #[serde(default)]
    pub lyric: String,
    #[serde(default)]
    pub pic: String,
    #[serde(default)]
    pub duration: i64,
    #[serde(default)]
    pub sort: Option<i64>,
    #[serde(default)]
    pub genre: String,
    #[serde(default)]
    pub lang: String,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaiheAlbumItem {
    #[serde(default)]
    pub album_asset_code: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub artist: Vec<TaiheArtist>,
    #[serde(default)]
    pub release_date: String,
    #[serde(default)]
    pub introduce: String,
    #[serde(default)]
    pub pic: String,
    #[serde(default)]
    pub genre: String,
    #[serde(default)]
    pub lang: String,
    #[serde(default)]
    pub track_list: Vec<TaiheTrackItem>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct TaiheSearchData {
    #[serde(default)]
    type_track: Vec<TaiheTrackItem>,
    #[serde(default)]
    type_album: Vec<TaiheAlbumItem>,
}

#[derive(Debug, Deserialize, Default)]
struct TaiheEnvelope<T> {
    #[serde(default)]
    state: bool,
    #[serde(default)]
    data: Option<T>,
}

pub(crate) fn build_taihe_signed_params(
    pairs: &[(&str, &str)],
    timestamp: i64,
) -> Vec<(String, String)> {
    let mut sorted = BTreeMap::new();
    for (k, v) in pairs {
        sorted.insert((*k).to_string(), (*v).to_string());
    }
    sorted.insert("appid".to_string(), TAIHE_APPID.to_string());
    sorted.insert("timestamp".to_string(), timestamp.to_string());

    let canonical: String = sorted
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&");

    let mut hasher = Md5::new();
    hasher.update(canonical.as_bytes());
    hasher.update(TAIHE_SECRET.as_bytes());
    let sign = hex::encode(hasher.finalize());

    let mut out: Vec<(String, String)> = sorted.into_iter().collect();
    out.push(("sign".to_string(), sign));
    out
}

fn current_unix_secs() -> i64 {
    jiff::Timestamp::now().as_second()
}

fn collect_taihe_singers(artists: &[TaiheArtist]) -> Vec<String> {
    let mut primary = Vec::new();
    let mut fallback = Vec::new();
    for a in artists {
        let n = strip_inline_html(&unescape_music_text(&a.name));
        if n.is_empty() {
            continue;
        }
        if (a.artist_type == 38 || a.artist_type_name.trim() == "歌手" || a.artist_type == 0)
            && !primary.iter().any(|e| e == &n)
        {
            primary.push(n.clone());
        }
        if !fallback.iter().any(|e| e == &n) {
            fallback.push(n);
        }
    }
    if !primary.is_empty() {
        primary
    } else {
        fallback
    }
}

fn extract_taihe_code(raw: &str, prefix: char, url_marker: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.len() >= 8
        && trimmed.starts_with(prefix)
        && trimmed[1..].bytes().all(|b| b.is_ascii_digit())
    {
        return Some(trimmed.to_string());
    }
    if let Some(idx) = trimmed.find(url_marker) {
        let after = &trimmed[idx + url_marker.len()..];
        let token: String = after
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect();
        if token.len() >= 8 && token.starts_with(prefix) {
            return Some(token);
        }
    }
    None
}

pub(crate) fn map_taihe_album_detail(alb: TaiheAlbumItem, external_id: &str) -> ScrapedMetadata {
    let title = {
        let t = strip_inline_html(&unescape_music_text(&alb.title));
        (!t.is_empty()).then_some(t)
    };
    let creators = collect_taihe_singers(&alb.artist);
    let (year, published_date) = parse_year_and_date(&alb.release_date);
    let summary = {
        let d = strip_html_to_text(&alb.introduce);
        (!d.is_empty()).then_some(d)
    };
    let genres: Vec<String> = {
        let g = strip_inline_html(&alb.genre);
        (!g.is_empty()).then_some(vec![g]).unwrap_or_default()
    };
    let language = {
        let l = strip_inline_html(&alb.lang);
        (!l.is_empty()).then_some(l)
    };
    let cover_url = normalize_cover_url(&alb.pic);
    let resolved_id = if !alb.album_asset_code.trim().is_empty() {
        alb.album_asset_code
    } else {
        external_id.to_string()
    };

    ScrapedMetadata {
        provider_id: "taihe".to_string(),
        external_id: resolved_id,
        target: Some(ScrapeTargetKind::MusicAlbum),
        title: title.clone(),
        series_or_album: title,
        creators,
        year,
        published_date,
        language,
        summary,
        genres,
        cover_url,
        ..Default::default()
    }
}

pub(crate) fn map_taihe_song_detail(
    song: TaiheTrackItem,
    external_id: &str,
    target: ScrapeTargetKind,
    raw_lrc: Option<&str>,
) -> ScrapedMetadata {
    let title = {
        let t = strip_inline_html(&unescape_music_text(&song.title));
        (!t.is_empty()).then_some(t)
    };
    let creators = collect_taihe_singers(&song.artist);
    let series_or_album = {
        let a = strip_inline_html(&unescape_music_text(&song.album_title));
        (!a.is_empty()).then_some(a)
    };
    let (year, published_date) = parse_year_and_date(&song.release_date);
    let track_no = song
        .sort
        .and_then(|n| u32::try_from(n).ok())
        .filter(|&n| n > 0);
    let genres: Vec<String> = {
        let g = strip_inline_html(&song.genre);
        (!g.is_empty()).then_some(vec![g]).unwrap_or_default()
    };
    let language = {
        let l = strip_inline_html(&song.lang);
        (!l.is_empty()).then_some(l)
    };
    let cover_url = normalize_cover_url(&song.pic);
    let (synced_lyrics, plain_lyrics) = raw_lrc
        .map(|l| normalize_lrc_and_extract_plain(l, None))
        .unwrap_or((None, None));
    let resolved_id = if !song.tsid.trim().is_empty() {
        song.tsid
    } else if !song.asset_id.trim().is_empty() {
        song.asset_id
    } else {
        external_id.to_string()
    };

    ScrapedMetadata {
        provider_id: "taihe".to_string(),
        external_id: resolved_id,
        target: Some(target),
        title,
        series_or_album,
        creators,
        year,
        published_date,
        language,
        track_or_episode_number: track_no,
        genres,
        cover_url,
        synced_lyrics,
        plain_lyrics,
        ..Default::default()
    }
}

#[async_trait]
impl ScraperProviderPort for TaiheMusicProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "taihe".to_string(),
            name: "千千音乐".to_string(),
            description: "太合千千音乐（原百度音乐）免登录公开曲库（提供海蝶/太合厂牌流行、民谣、OST 单曲专辑与标准 LRC 歌词）".to_string(),
            website: "https://music.91q.com".to_string(),
            supported_targets: vec![
                ScrapeTargetKind::MusicTrack,
                ScrapeTargetKind::MusicAlbum,
                ScrapeTargetKind::Lyrics,
            ],
            requires_api_key: false,
        }
    }

    async fn search(
        &self,
        query: &ScrapeQuery,
        creds: &ProviderCredentials,
    ) -> DomainResult<Vec<ScrapeCandidate>> {
        let target = query.target.unwrap_or(ScrapeTargetKind::MusicTrack);
        let is_album = target == ScrapeTargetKind::MusicAlbum;

        let direct_code = if is_album {
            extract_taihe_code(&query.keyword, 'P', "album/")
        } else {
            extract_taihe_code(&query.keyword, 'T', "song/")
        };
        if let Some(code) = direct_code
            && let Ok(detail) = self.fetch_detail(&code, target, "zh-CN", creds).await
            && let Some(title) = detail.title
        {
            let mut sub = Vec::new();
            if !detail.creators.is_empty() {
                sub.push(detail.creators.join(" / "));
            }
            if !is_album && let Some(alb) = detail.series_or_album {
                sub.push(alb);
            }
            return Ok(vec![ScrapeCandidate {
                provider_id: "taihe".to_string(),
                provider_name: "千千音乐".to_string(),
                external_id: detail.external_id,
                target,
                title,
                subtitle: (!sub.is_empty()).then(|| sub.join(" · ")),
                year: detail.year,
                summary: detail.summary,
                cover_url: detail.cover_url,
                synced_lyrics: detail.synced_lyrics,
                plain_lyrics: detail.plain_lyrics,
            }]);
        }

        let search_kw = build_music_search_keyword(query);
        if search_kw.is_empty() {
            return Ok(Vec::new());
        }

        let client = build_browser_http_client(creds, false)?;
        let search_type = if is_album { "0" } else { "1" };
        let signed = build_taihe_signed_params(
            &[
                ("pageNo", "1"),
                ("pageSize", "20"),
                ("type", search_type),
                ("word", search_kw.as_str()),
            ],
            current_unix_secs(),
        );

        let url = format!("{TAIHE_BASE_URL}/v1/search");
        let resp = client
            .get(&url)
            .header(reqwest::header::REFERER, TAIHE_REFERER)
            .query(&signed)
            .send()
            .await
            .map_err(|e| DomainError::Storage(format!("千千音乐 search failed: {e}")))?;

        let env: TaiheEnvelope<TaiheSearchData> =
            read_json_response(resp, "千千音乐 search").await?;
        if !env.state {
            return Ok(Vec::new());
        }
        let Some(data) = env.data else {
            return Ok(Vec::new());
        };

        if is_album {
            let mut scored: Vec<(i32, usize, ScrapeCandidate)> = Vec::new();
            for (idx, alb) in data.type_album.into_iter().enumerate() {
                let code = alb.album_asset_code.trim().to_string();
                if code.is_empty() {
                    continue;
                }
                let title = strip_inline_html(&unescape_music_text(&alb.title));
                if title.is_empty() {
                    continue;
                }
                let artists = collect_taihe_singers(&alb.artist);
                let (year, _) = parse_year_and_date(&alb.release_date);
                let score = score_music_candidate(
                    query,
                    &title,
                    &artists,
                    Some(&title),
                    year,
                    None,
                    (alb.track_list.len() as u32).saturating_mul(15),
                );
                let summary = {
                    let s = strip_inline_html(&alb.introduce);
                    (!s.is_empty()).then_some(s)
                };
                scored.push((
                    score,
                    idx,
                    ScrapeCandidate {
                        provider_id: "taihe".to_string(),
                        provider_name: "千千音乐".to_string(),
                        external_id: code,
                        target,
                        title,
                        subtitle: (!artists.is_empty()).then(|| artists.join(" / ")),
                        year,
                        summary,
                        cover_url: normalize_cover_url(&alb.pic),
                        synced_lyrics: None,
                        plain_lyrics: None,
                    },
                ));
            }
            scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
            return Ok(scored.into_iter().take(15).map(|(_, _, c)| c).collect());
        }

        let mut scored: Vec<(i32, usize, ScrapeCandidate)> = Vec::new();
        for (idx, item) in data.type_track.into_iter().enumerate() {
            let tsid = if !item.tsid.trim().is_empty() {
                item.tsid.trim().to_string()
            } else {
                item.asset_id.trim().to_string()
            };
            if tsid.is_empty() {
                continue;
            }
            let title = strip_inline_html(&unescape_music_text(&item.title));
            if title.is_empty() {
                continue;
            }
            let artists = collect_taihe_singers(&item.artist);
            let album_name = {
                let a = strip_inline_html(&unescape_music_text(&item.album_title));
                (!a.is_empty()).then_some(a)
            };
            let (year, _) = parse_year_and_date(&item.release_date);
            let dur_u32 = u32::try_from(item.duration).ok().filter(|&d| d > 0);
            let score = score_music_candidate(
                query,
                &title,
                &artists,
                album_name.as_deref(),
                year,
                dur_u32,
                0,
            );

            let mut sub = Vec::new();
            if !artists.is_empty() {
                sub.push(artists.join(" / "));
            }
            if let Some(ref alb) = album_name {
                sub.push(alb.clone());
            }
            if let Some(ds) = dur_u32.and_then(format_duration_mmss) {
                sub.push(ds);
            }

            scored.push((
                score,
                idx,
                ScrapeCandidate {
                    provider_id: "taihe".to_string(),
                    provider_name: "千千音乐".to_string(),
                    external_id: tsid,
                    target,
                    title,
                    subtitle: (!sub.is_empty()).then(|| sub.join(" · ")),
                    year,
                    summary: None,
                    cover_url: normalize_cover_url(&item.pic),
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
        let id = external_id.trim();
        if id.is_empty() {
            return Err(DomainError::Validation("empty 千千音乐 ID".into()));
        }
        let client = build_browser_http_client(creds, false)?;

        if target == ScrapeTargetKind::MusicAlbum {
            let signed = build_taihe_signed_params(&[("albumAssetCode", id)], current_unix_secs());
            let url = format!("{TAIHE_BASE_URL}/v1/album/info");
            let resp = client
                .get(&url)
                .header(reqwest::header::REFERER, TAIHE_REFERER)
                .query(&signed)
                .send()
                .await
                .map_err(|e| DomainError::Storage(format!("千千音乐 album detail failed: {e}")))?;
            let env: TaiheEnvelope<TaiheAlbumItem> =
                read_json_response(resp, "千千音乐 album").await?;
            let alb = env
                .data
                .ok_or_else(|| DomainError::NotFound(format!("千千音乐 album '{id}' not found")))?;
            return Ok(map_taihe_album_detail(alb, id));
        }

        let signed = build_taihe_signed_params(&[("TSID", id)], current_unix_secs());
        let url = format!("{TAIHE_BASE_URL}/v1/song/info");
        let resp = client
            .get(&url)
            .header(reqwest::header::REFERER, TAIHE_REFERER)
            .query(&signed)
            .send()
            .await
            .map_err(|e| DomainError::Storage(format!("千千音乐 song detail failed: {e}")))?;
        let env: TaiheEnvelope<Vec<TaiheTrackItem>> =
            read_json_response(resp, "千千音乐 song").await?;
        let song = env
            .data
            .unwrap_or_default()
            .into_iter()
            .next()
            .ok_or_else(|| DomainError::NotFound(format!("千千音乐 song '{id}' not found")))?;

        let mut lrc_raw: Option<String> = None;
        if let Some(lrc_url) = normalize_cover_url(&song.lyric)
            && let Ok(r) = client.get(&lrc_url).send().await
            && r.status().is_success()
        {
            lrc_raw = read_response_text(r, UTF_8).await.ok();
        }

        Ok(map_taihe_song_detail(song, id, target, lrc_raw.as_deref()))
    }
}

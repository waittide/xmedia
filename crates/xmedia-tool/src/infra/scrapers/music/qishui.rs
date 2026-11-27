use std::sync::LazyLock;

use async_trait::async_trait;
use encoding_rs::UTF_8;
use regex::Regex;
use serde::Deserialize;
use xmedia_core::domain::error::{DomainError, DomainResult};

use super::common::{
    MusicCandidateDraft, build_music_search_keyword, music_candidate_from_detail,
    normalize_lrc_and_extract_plain, parse_epoch_ms_year_and_date, rank_music_candidates,
    unescape_music_text,
};
use crate::application::ports::ScraperProviderPort;
use crate::domain::config::ProviderCredentials;
use crate::domain::scraper::{
    ProviderDescriptor, ScrapeCandidate, ScrapeQuery, ScrapeTargetKind, ScrapedMetadata,
};
use crate::infra::scrapers::html_util::{
    build_browser_http_client, extract_numeric_id_from_query, normalize_cover_url,
    read_json_response, read_response_text, strip_html_to_text, strip_inline_html,
};

pub struct QishuiMusicProvider;

const QS_SEO_TRACK_URL: &str = "https://beta-luna.douyin.com/luna/h5/seo_track";
const QS_SHARE_ALBUM_URL: &str = "https://music.douyin.com/qishui/share/album";
const QS_PC_SEARCH_TRACK_URL: &str = "https://api.qishui.com/luna/pc/search/track";
const QS_REFERER: &str = "https://music.douyin.com/";

static RE_QS_KRC_LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\[(\d+),(\d+)\](.*)$").unwrap());
static RE_QS_KRC_WORD_TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<-?\d+,-?\d+(?:,-?\d+)?>").unwrap());
static RE_QS_SHORT_URL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"https?://qishui\.douyin\.com/s/[A-Za-z0-9_-]+/?").unwrap());

#[derive(Debug, Deserialize, Default, Clone)]
pub(crate) struct QsUrlCover {
    #[serde(default)]
    pub uri: String,
    #[serde(default)]
    pub urls: Vec<String>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub(crate) struct QsArtist {
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub(crate) struct QsAlbumInfo {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub artists: Vec<QsArtist>,
    #[serde(default)]
    pub company: String,
    #[serde(default)]
    pub intro: String,
    #[serde(default)]
    pub release_date: i64,
    #[serde(default)]
    pub url_cover: Option<QsUrlCover>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub(crate) struct QsTrackCore {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub duration: i64,
    #[serde(default)]
    pub artists: Vec<QsArtist>,
    #[serde(default)]
    pub album: Option<QsAlbumInfo>,
    #[serde(default)]
    pub lang_codes: Vec<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct QsSeoTrackWrap {
    #[serde(default)]
    pub track: Option<QsTrackCore>,
    #[serde(default)]
    pub genre_tag: String,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct QsLyricPayload {
    #[serde(default)]
    pub content: String,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct QsAlbumTrackRef {
    #[serde(default)]
    pub track: Option<QsTrackIdOnly>,
    #[serde(default)]
    pub id: String,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct QsTrackIdOnly {
    #[serde(default)]
    pub id: String,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct QsSeoTrackEnvelope {
    #[serde(default)]
    pub seo_track: Option<QsSeoTrackWrap>,
    #[serde(default)]
    pub lyric: Option<QsLyricPayload>,
    #[serde(default)]
    pub album_tracks: Vec<QsAlbumTrackRef>,
}

#[derive(Debug, Deserialize, Default)]
struct QsRouterData {
    #[serde(default, rename = "loaderData")]
    loader_data: QsLoaderData,
}

#[derive(Debug, Deserialize, Default)]
struct QsLoaderData {
    #[serde(default)]
    album_page: Option<QsAlbumPage>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct QsAlbumPage {
    #[serde(default)]
    album_info: Option<QsAlbumInfo>,
}

#[derive(Debug, Deserialize, Default)]
struct QsSearchBlock {
    #[serde(default)]
    tracks: Vec<QsTrackCore>,
    #[serde(default)]
    result_groups: Vec<QsSearchGroup>,
}

#[derive(Debug, Deserialize, Default)]
struct QsSearchGroup {
    #[serde(default)]
    data: Vec<QsSearchItemWrap>,
}

#[derive(Debug, Deserialize, Default)]
struct QsSearchItemWrap {
    #[serde(default)]
    entity: Option<QsSearchEntity>,
}

#[derive(Debug, Deserialize, Default)]
struct QsSearchEntity {
    #[serde(default)]
    track: Option<QsTrackCore>,
}

pub(crate) fn build_qishui_cover_url(cover: Option<&QsUrlCover>, size: u32) -> Option<String> {
    let c = cover?;
    let uri = c.uri.trim();
    if uri.is_empty() {
        return None;
    }
    let host = c
        .urls
        .first()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or("https://p3-luna.douyinpic.com/img/");
    let sep = if host.ends_with('/') { "" } else { "/" };
    normalize_cover_url(&format!("{host}{sep}{uri}~c5_{size}x{size}.jpg"))
}

/// Converts Qishui's `[start_ms,dur_ms]<0,180,0>字...` KRC lines (or standard LRC) into `(synced_lrc, plain_lyrics)`.
pub(crate) fn parse_qishui_lyrics(raw: &str) -> (Option<String>, Option<String>) {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return (None, None);
    }

    let mut converted_lines = Vec::new();
    let mut saw_krc = false;

    for line in trimmed.lines() {
        let l = line.trim();
        if let Some(caps) = RE_QS_KRC_LINE.captures(l)
            && let Ok(ms) = caps[1].parse::<u64>()
        {
            saw_krc = true;
            let total_cs = (ms + 5) / 10;
            let mins = total_cs / 6000;
            let rem = total_cs % 6000;
            let secs = rem / 100;
            let cs = rem % 100;
            let text_body = RE_QS_KRC_WORD_TAG.replace_all(&caps[3], "");
            converted_lines.push(format!("[{mins:02}:{secs:02}.{cs:02}]{}", text_body.trim()));
        } else if !l.starts_with("[id:")
            && !l.starts_with("[ar:")
            && !l.starts_with("[ti:")
            && !l.starts_with("[al:")
            && !l.starts_with("[hash:")
            && !l.starts_with("[total:")
            && !l.starts_with("[offset:")
        {
            let cleaned = RE_QS_KRC_WORD_TAG.replace_all(l, "");
            converted_lines.push(cleaned.into_owned());
        }
    }

    let candidate = if saw_krc {
        converted_lines.join("\n")
    } else {
        RE_QS_KRC_WORD_TAG.replace_all(trimmed, "").into_owned()
    };
    normalize_lrc_and_extract_plain(&candidate, None)
}

fn extract_balanced_json_after(html: &str, marker: &str) -> Option<String> {
    let idx = html.find(marker)?;
    let rest = &html[idx + marker.len()..];
    let start = rest.find('{')?;
    let bytes = rest.as_bytes();
    let mut depth = 0i32;
    let mut in_str = false;
    let mut esc = false;

    for (i, &b) in bytes[start..].iter().enumerate() {
        if in_str {
            if esc {
                esc = false;
            } else if b == b'\\' {
                esc = true;
            } else if b == b'"' {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(rest[start..=start + i].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

async fn resolve_qishui_direct_id(
    creds: &ProviderCredentials,
    raw: &str,
    is_album: bool,
) -> Option<String> {
    let trimmed = raw.trim();
    if let Some(m) = RE_QS_SHORT_URL.find(trimmed)
        && crate::infra::scrapers::http_client::validate_target_url_ssrf(m.as_str(), false)
            .await
            .is_ok()
        && let Ok(safe_client) =
            crate::infra::scrapers::http_client::build_ssrf_safe_http_client(creds, false)
        && let Ok(resp) = safe_client
            .get(m.as_str())
            .header(reqwest::header::REFERER, QS_REFERER)
            .send()
            .await
    {
        let final_u = resp.url().as_str().to_string();
        let markers: &[&str] = if is_album {
            &["album_id=", "album/"]
        } else {
            &["track_id=", "song/"]
        };
        if let Some(id) = extract_numeric_id_from_query(&final_u, markers) {
            return Some(id);
        }
    }

    let markers: &[&str] = if is_album {
        &["album_id=", "qishui/album/", "share/album/"]
    } else {
        &["track_id=", "qishui/song/", "share/track/"]
    };
    let id = extract_numeric_id_from_query(trimmed, markers)?;
    if trimmed.contains("douyin.com") || trimmed.contains("qishui.com") || id.len() >= 16 {
        Some(id)
    } else {
        None
    }
}

pub(crate) fn map_qishui_seo_track(
    env: QsSeoTrackEnvelope,
    external_id: &str,
    target: ScrapeTargetKind,
) -> Option<ScrapedMetadata> {
    let wrap = env.seo_track?;
    let tr = wrap.track?;
    let title = strip_inline_html(&unescape_music_text(&tr.name));
    if title.is_empty() {
        return None;
    }

    let creators: Vec<String> = tr
        .artists
        .iter()
        .map(|a| strip_inline_html(&unescape_music_text(&a.name)))
        .filter(|s| !s.is_empty())
        .collect();

    let series_or_album = tr
        .album
        .as_ref()
        .map(|a| strip_inline_html(&unescape_music_text(&a.name)))
        .filter(|s| !s.is_empty());

    let (year, published_date) = tr
        .album
        .as_ref()
        .filter(|a| a.release_date > 0)
        .map(|a| parse_epoch_ms_year_and_date(a.release_date * 1000))
        .unwrap_or((None, None));

    let cover_url =
        build_qishui_cover_url(tr.album.as_ref().and_then(|a| a.url_cover.as_ref()), 1000);

    let resolved_id = if !tr.id.trim().is_empty() {
        tr.id.clone()
    } else {
        external_id.to_string()
    };

    let track_no = env
        .album_tracks
        .iter()
        .position(|at| {
            at.id.trim() == resolved_id
                || at
                    .track
                    .as_ref()
                    .is_some_and(|inner| inner.id.trim() == resolved_id)
        })
        .map(|idx| (idx + 1) as u32);

    let genres: Vec<String> = {
        let g = strip_inline_html(&wrap.genre_tag);
        (!g.is_empty()).then_some(vec![g]).unwrap_or_default()
    };
    let language = tr
        .lang_codes
        .first()
        .map(|s| strip_inline_html(s))
        .filter(|s| !s.is_empty());

    let (synced_lyrics, plain_lyrics) = env.lyric.as_deref_or_none().unwrap_or((None, None));

    Some(ScrapedMetadata {
        provider_id: "qishui".to_string(),
        external_id: resolved_id,
        target: Some(target),
        title: Some(title),
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
    })
}

trait LyricOptExt {
    fn as_deref_or_none(&self) -> Option<(Option<String>, Option<String>)>;
}

impl LyricOptExt for Option<QsLyricPayload> {
    fn as_deref_or_none(&self) -> Option<(Option<String>, Option<String>)> {
        let l = self.as_ref()?;
        Some(parse_qishui_lyrics(&l.content))
    }
}

pub(crate) fn map_qishui_album_info(alb: QsAlbumInfo, external_id: &str) -> ScrapedMetadata {
    let title = {
        let t = strip_inline_html(&unescape_music_text(&alb.name));
        (!t.is_empty()).then_some(t)
    };
    let creators: Vec<String> = alb
        .artists
        .iter()
        .map(|a| strip_inline_html(&unescape_music_text(&a.name)))
        .filter(|s| !s.is_empty())
        .collect();
    let publisher = {
        let c = strip_inline_html(&unescape_music_text(&alb.company));
        (!c.is_empty()).then_some(c)
    };
    let (year, published_date) = if alb.release_date > 0 {
        parse_epoch_ms_year_and_date(alb.release_date * 1000)
    } else {
        (None, None)
    };
    let summary = {
        let d = strip_html_to_text(&alb.intro);
        (!d.is_empty()).then_some(d)
    };
    let cover_url = build_qishui_cover_url(alb.url_cover.as_ref(), 1000);
    let resolved_id = if !alb.id.trim().is_empty() {
        alb.id
    } else {
        external_id.to_string()
    };

    ScrapedMetadata {
        provider_id: "qishui".to_string(),
        external_id: resolved_id,
        target: Some(ScrapeTargetKind::MusicAlbum),
        title: title.clone(),
        series_or_album: title,
        creators,
        publisher_or_studio: publisher,
        year,
        published_date,
        summary,
        cover_url,
        ..Default::default()
    }
}

#[async_trait]
impl ScraperProviderPort for QishuiMusicProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "qishui".to_string(),
            name: "汽水音乐".to_string(),
            description: "字节跳动/抖音汽水音乐公开曲库（支持抖音首发单曲/专辑 ID 与分享短链直解、1000px 封面及逐字转 LRC 歌词）".to_string(),
            website: "https://music.douyin.com".to_string(),
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
        let client = build_browser_http_client(creds, false)?;

        if let Some(direct_id) = resolve_qishui_direct_id(creds, &query.keyword, is_album).await
            && let Ok(detail) = self.fetch_detail(&direct_id, target, "zh-CN", creds).await
            && let Some(cand) = music_candidate_from_detail("qishui", "汽水音乐", target, detail)
        {
            return Ok(vec![cand]);
        }

        if is_album {
            return Ok(Vec::new());
        }

        let search_kw = build_music_search_keyword(query);
        if search_kw.is_empty() {
            return Ok(Vec::new());
        }

        let mut req = client
            .get(QS_PC_SEARCH_TRACK_URL)
            .header(reqwest::header::REFERER, QS_REFERER)
            .query(&[
                ("q", search_kw.as_str()),
                ("cursor", "0"),
                ("search_method", "input"),
                ("aid", "386088"),
                ("device_platform", "web"),
            ]);
        if let Some(cookie) = creds.get_extra("qishui_cookie") {
            req = req.header(reqwest::header::COOKIE, cookie);
        }

        let resp = req
            .send()
            .await
            .map_err(|e| DomainError::Storage(format!("汽水音乐 search failed: {e}")))?;
        if !resp.status().is_success() {
            return Err(DomainError::Storage(format!(
                "汽水音乐 search returned HTTP {}",
                resp.status()
            )));
        }
        let txt = read_response_text(resp, UTF_8).await?;
        if txt.trim().is_empty() {
            return Err(DomainError::Storage(
                "汽水音乐 search returned empty response".into(),
            ));
        }
        let block = serde_json::from_str::<QsSearchBlock>(txt.trim())
            .map_err(|e| DomainError::Storage(format!("invalid 汽水音乐 search JSON: {e}")))?;

        let mut tracks = block.tracks;
        for g in block.result_groups {
            for item in g.data {
                if let Some(ent) = item.entity
                    && let Some(t) = ent.track
                {
                    tracks.push(t);
                }
            }
        }

        let drafts = tracks.into_iter().map(|tr| {
            let title = strip_inline_html(&unescape_music_text(&tr.name));
            let artists: Vec<String> = tr
                .artists
                .iter()
                .map(|a| strip_inline_html(&unescape_music_text(&a.name)))
                .filter(|s| !s.is_empty())
                .collect();
            let album = tr
                .album
                .as_ref()
                .map(|a| strip_inline_html(&unescape_music_text(&a.name)))
                .filter(|s| !s.is_empty());
            let (year, _) = tr
                .album
                .as_ref()
                .filter(|a| a.release_date > 0)
                .map(|a| parse_epoch_ms_year_and_date(a.release_date * 1000))
                .unwrap_or((None, None));
            let duration_secs = u32::try_from(tr.duration / 1000).ok().filter(|&d| d > 0);
            MusicCandidateDraft {
                external_id: tr.id.trim().to_string(),
                title,
                artists,
                album,
                year,
                duration_secs,
                cover_url: build_qishui_cover_url(
                    tr.album.as_ref().and_then(|a| a.url_cover.as_ref()),
                    500,
                ),
                ..Default::default()
            }
        });

        Ok(rank_music_candidates(
            "qishui",
            "汽水音乐",
            target,
            query,
            drafts,
            15,
        ))
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
                "invalid 汽水音乐 ID '{id}'"
            )));
        }
        let client = build_browser_http_client(creds, false)?;

        if target == ScrapeTargetKind::MusicAlbum {
            let resp = client
                .get(QS_SHARE_ALBUM_URL)
                .header(reqwest::header::REFERER, QS_REFERER)
                .query(&[("album_id", id)])
                .send()
                .await
                .map_err(|e| DomainError::Storage(format!("汽水音乐 album page failed: {e}")))?;
            let html = read_response_text(resp, UTF_8).await?;
            let json_str = extract_balanced_json_after(&html, "_ROUTER_DATA")
                .ok_or_else(|| DomainError::NotFound(format!("汽水音乐 album '{id}' not found")))?;
            let router: QsRouterData = serde_json::from_str(&json_str)
                .map_err(|e| DomainError::Storage(format!("invalid 汽水音乐 album SSR: {e}")))?;
            let alb = router
                .loader_data
                .album_page
                .and_then(|p| p.album_info)
                .filter(|a| !a.name.trim().is_empty())
                .ok_or_else(|| DomainError::NotFound(format!("汽水音乐 album '{id}' not found")))?;
            return Ok(map_qishui_album_info(alb, id));
        }

        let resp = client
            .get(QS_SEO_TRACK_URL)
            .header(reqwest::header::REFERER, QS_REFERER)
            .query(&[("track_id", id)])
            .send()
            .await
            .map_err(|e| DomainError::Storage(format!("汽水音乐 seo_track failed: {e}")))?;

        let env: QsSeoTrackEnvelope = read_json_response(resp, "汽水音乐 seo_track").await?;
        map_qishui_seo_track(env, id, target)
            .ok_or_else(|| DomainError::NotFound(format!("汽水音乐 track '{id}' not found")))
    }
}

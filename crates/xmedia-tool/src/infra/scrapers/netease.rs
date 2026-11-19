use async_trait::async_trait;
use serde::Deserialize;
use xmedia_core::domain::error::{DomainError, DomainResult};

use crate::domain::config::ProviderCredentials;
use crate::domain::ports::ScraperProviderPort;
use crate::domain::scraper::{
    ProviderDescriptor, ScrapeCandidate, ScrapeQuery, ScrapeTargetKind, ScrapedMetadata,
};
use crate::infra::scrapers::cn_music_common::{
    build_music_search_keyword, format_duration_mmss, normalize_lrc_and_extract_plain,
    parse_epoch_ms_year_and_date, read_json_response, score_music_candidate, unescape_music_text,
};
use crate::infra::scrapers::cn_novel_common::{
    build_browser_http_client, extract_numeric_id_from_query, normalize_cover_url,
    strip_html_to_text, strip_inline_html,
};

pub struct NeteaseMusicProvider;

const SEARCH_URL: &str = "https://music.163.com/api/cloudsearch/pc";
const SONG_DETAIL_URL: &str = "https://music.163.com/api/song/detail/";
const LYRIC_URL: &str = "https://music.163.com/api/song/lyric";
const ALBUM_V1_PREFIX: &str = "https://interface.music.163.com/api/v1/album/";
const ALBUM_FALLBACK_PREFIX: &str = "https://music.163.com/api/album/";
const NE_REFERER: &str = "https://music.163.com/";
const NE_COOKIE: &str = "os=pc; appver=2.9.7";

#[derive(Debug, Deserialize, Default, Clone)]
pub(crate) struct NeArtist {
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NeAlbumInfo {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub pic_url: Option<String>,
    #[serde(default)]
    pub publish_time: Option<i64>,
    #[serde(default)]
    pub company: Option<String>,
    #[serde(default)]
    pub sub_type: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Option<String>,
    #[serde(default)]
    pub artist: Option<NeArtist>,
    #[serde(default)]
    pub artists: Vec<NeArtist>,
    #[serde(default)]
    pub size: Option<i64>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NeSearchSong {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub ar: Vec<NeArtist>,
    #[serde(default)]
    pub artists: Vec<NeArtist>,
    #[serde(default)]
    pub al: Option<NeAlbumInfo>,
    #[serde(default)]
    pub album: Option<NeAlbumInfo>,
    #[serde(default)]
    pub dt: Option<i64>,
    #[serde(default)]
    pub duration: Option<i64>,
    #[serde(default)]
    pub no: Option<i64>,
    #[serde(default)]
    pub cd: Option<String>,
    #[serde(default)]
    pub disc: Option<String>,
    #[serde(default)]
    pub publish_time: Option<i64>,
    #[serde(default)]
    pub pop: Option<f64>,
}

#[derive(Debug, Deserialize, Default)]
struct NeSearchResult {
    #[serde(default)]
    songs: Vec<NeSearchSong>,
    #[serde(default)]
    albums: Vec<NeAlbumInfo>,
}

#[derive(Debug, Deserialize, Default)]
struct NeSearchEnvelope {
    #[serde(default)]
    code: i64,
    #[serde(default)]
    result: Option<NeSearchResult>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct NeSongDetailEnvelope {
    #[serde(default)]
    pub songs: Vec<NeSearchSong>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct NeLyricBlock {
    #[serde(default)]
    pub lyric: String,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct NeLyricEnvelope {
    #[serde(default)]
    pub lrc: Option<NeLyricBlock>,
    #[serde(default)]
    pub tlyric: Option<NeLyricBlock>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct NeAlbumDetailEnvelope {
    #[serde(default)]
    pub code: i64,
    #[serde(default)]
    pub album: Option<NeAlbumInfo>,
}

pub(crate) fn upgrade_netease_cover(raw: Option<&str>, size: u32) -> Option<String> {
    let base = normalize_cover_url(raw?)?;
    let clean = base.split('?').next().unwrap_or(&base);
    Some(format!("{clean}?param={size}y{size}"))
}

fn collect_ne_artists(
    primary: &[NeArtist],
    fallback: &[NeArtist],
    single: Option<&NeArtist>,
) -> Vec<String> {
    let mut out = Vec::new();
    let src = if !primary.is_empty() {
        primary
    } else {
        fallback
    };
    for a in src {
        let n = strip_inline_html(&unescape_music_text(&a.name));
        if !n.is_empty() && !out.iter().any(|e| e == &n) {
            out.push(n);
        }
    }
    if out.is_empty()
        && let Some(s) = single
    {
        let n = strip_inline_html(&unescape_music_text(&s.name));
        if !n.is_empty() {
            out.push(n);
        }
    }
    out
}

fn parse_disc_number(cd_or_disc: Option<&str>) -> Option<u32> {
    let raw = cd_or_disc?.trim();
    if raw.is_empty() {
        return None;
    }
    raw.parse::<u32>().ok().filter(|&n| n > 0)
}

pub(crate) fn map_netease_song_detail(
    song: NeSearchSong,
    external_id: &str,
    target: ScrapeTargetKind,
    lyric_env: Option<NeLyricEnvelope>,
) -> ScrapedMetadata {
    let title = {
        let t = strip_inline_html(&unescape_music_text(&song.name));
        (!t.is_empty()).then_some(t)
    };
    let creators = collect_ne_artists(&song.artists, &song.ar, None);
    let alb = song.album.or(song.al);
    let series_or_album = alb
        .as_ref()
        .map(|a| strip_inline_html(&unescape_music_text(&a.name)))
        .filter(|s| !s.is_empty());
    let publisher = alb
        .as_ref()
        .and_then(|a| a.company.as_deref())
        .map(|c| strip_inline_html(&unescape_music_text(c)))
        .filter(|s| !s.is_empty());
    let pub_ms = song.publish_time.filter(|&t| t != 0).or_else(|| {
        alb.as_ref()
            .and_then(|a| a.publish_time)
            .filter(|&t| t != 0)
    });
    let (year, published_date) = pub_ms
        .map(parse_epoch_ms_year_and_date)
        .unwrap_or((None, None));
    let cover_url = upgrade_netease_cover(alb.as_ref().and_then(|a| a.pic_url.as_deref()), 1000);
    let track_no = song
        .no
        .and_then(|n| u32::try_from(n).ok())
        .filter(|&n| n > 0);
    let disc_no = parse_disc_number(song.disc.as_deref().or(song.cd.as_deref()));

    let (synced_lyrics, plain_lyrics) = lyric_env
        .and_then(|env| {
            let orig = env.lrc?.lyric;
            let trans = env.tlyric.map(|t| t.lyric);
            Some(normalize_lrc_and_extract_plain(&orig, trans.as_deref()))
        })
        .unwrap_or((None, None));

    let resolved_id = if song.id > 0 {
        song.id.to_string()
    } else {
        external_id.to_string()
    };

    ScrapedMetadata {
        provider_id: "netease".to_string(),
        external_id: resolved_id,
        target: Some(target),
        title,
        series_or_album,
        creators,
        publisher_or_studio: publisher,
        year,
        published_date,
        track_or_episode_number: track_no,
        disc_or_season_number: disc_no,
        cover_url,
        synced_lyrics,
        plain_lyrics,
        ..Default::default()
    }
}

pub(crate) fn map_netease_album_detail(alb: NeAlbumInfo, external_id: &str) -> ScrapedMetadata {
    let title = {
        let t = strip_inline_html(&unescape_music_text(&alb.name));
        (!t.is_empty()).then_some(t)
    };
    let creators = collect_ne_artists(&alb.artists, &[], alb.artist.as_ref());
    let publisher = alb
        .company
        .as_deref()
        .map(|c| strip_inline_html(&unescape_music_text(c)))
        .filter(|s| !s.is_empty());
    let (year, published_date) = alb
        .publish_time
        .map(parse_epoch_ms_year_and_date)
        .unwrap_or((None, None));
    let summary = alb
        .description
        .as_deref()
        .map(strip_html_to_text)
        .filter(|s| !s.is_empty());
    let tags: Vec<String> = alb
        .tags
        .as_deref()
        .unwrap_or("")
        .split([',', '，', ';', '；'])
        .map(strip_inline_html)
        .filter(|s| !s.is_empty())
        .collect();
    let genres: Vec<String> = alb
        .sub_type
        .as_deref()
        .map(strip_inline_html)
        .filter(|s| !s.is_empty())
        .into_iter()
        .collect();
    let cover_url = upgrade_netease_cover(alb.pic_url.as_deref(), 1000);
    let resolved_id = if alb.id > 0 {
        alb.id.to_string()
    } else {
        external_id.to_string()
    };

    ScrapedMetadata {
        provider_id: "netease".to_string(),
        external_id: resolved_id,
        target: Some(ScrapeTargetKind::MusicAlbum),
        title: title.clone(),
        series_or_album: title,
        creators,
        publisher_or_studio: publisher,
        year,
        published_date,
        summary,
        genres,
        tags,
        cover_url,
        ..Default::default()
    }
}

#[async_trait]
impl ScraperProviderPort for NeteaseMusicProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "netease".to_string(),
            name: "网易云音乐".to_string(),
            description: "网易云音乐免登录公开接口（支持独立音乐人/二次元/流行单曲、专辑、1000px 封面与双语 LRC 歌词）".to_string(),
            website: "https://music.163.com".to_string(),
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

        let url_markers: &[&str] = if is_album {
            &["album?id=", "album/"]
        } else {
            &["song?id=", "song/"]
        };
        if let Some(direct_id) = extract_numeric_id_from_query(&query.keyword, url_markers)
            && (query.keyword.contains("163.com") || query.keyword.trim().len() >= 6)
            && let Ok(detail) = self.fetch_detail(&direct_id, target, "zh-CN", creds).await
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
                provider_id: "netease".to_string(),
                provider_name: "网易云音乐".to_string(),
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
        let search_type = if is_album { "10" } else { "1" };
        let resp = client
            .post(SEARCH_URL)
            .header(reqwest::header::REFERER, NE_REFERER)
            .header(reqwest::header::COOKIE, NE_COOKIE)
            .form(&[
                ("s", search_kw.as_str()),
                ("type", search_type),
                ("offset", "0"),
                ("limit", "20"),
            ])
            .send()
            .await
            .map_err(|e| DomainError::Storage(format!("网易云音乐 search failed: {e}")))?;

        if !resp.status().is_success() {
            return Err(DomainError::Storage(format!(
                "网易云音乐 search returned HTTP {}",
                resp.status()
            )));
        }

        let env: NeSearchEnvelope = read_json_response(resp, "网易云音乐 search").await?;
        if env.code != 200 {
            return Ok(Vec::new());
        }
        let Some(result) = env.result else {
            return Ok(Vec::new());
        };

        if is_album {
            let mut scored: Vec<(i32, usize, ScrapeCandidate)> = Vec::new();
            for (idx, alb) in result.albums.into_iter().enumerate() {
                if alb.id <= 0 {
                    continue;
                }
                let title = strip_inline_html(&unescape_music_text(&alb.name));
                if title.is_empty() {
                    continue;
                }
                let artists = collect_ne_artists(&alb.artists, &[], alb.artist.as_ref());
                let (year, _) = alb
                    .publish_time
                    .map(parse_epoch_ms_year_and_date)
                    .unwrap_or((None, None));
                let score = score_music_candidate(
                    query,
                    &title,
                    &artists,
                    Some(&title),
                    year,
                    None,
                    u32::try_from(alb.size.unwrap_or(0))
                        .unwrap_or(0)
                        .saturating_mul(15),
                );
                let mut sub = Vec::new();
                if !artists.is_empty() {
                    sub.push(artists.join(" / "));
                }
                if let Some(sz) = alb.size.filter(|&n| n > 0) {
                    sub.push(format!("{sz} 首"));
                }
                scored.push((
                    score,
                    idx,
                    ScrapeCandidate {
                        provider_id: "netease".to_string(),
                        provider_name: "网易云音乐".to_string(),
                        external_id: alb.id.to_string(),
                        target,
                        title,
                        subtitle: (!sub.is_empty()).then(|| sub.join(" · ")),
                        year,
                        summary: alb
                            .company
                            .as_deref()
                            .map(strip_inline_html)
                            .filter(|s| !s.is_empty()),
                        cover_url: upgrade_netease_cover(alb.pic_url.as_deref(), 500),
                        synced_lyrics: None,
                        plain_lyrics: None,
                    },
                ));
            }
            scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
            return Ok(scored.into_iter().take(15).map(|(_, _, c)| c).collect());
        }

        let mut scored: Vec<(i32, usize, ScrapeCandidate)> = Vec::new();
        for (idx, song) in result.songs.into_iter().enumerate() {
            if song.id <= 0 {
                continue;
            }
            let title = strip_inline_html(&unescape_music_text(&song.name));
            if title.is_empty() {
                continue;
            }
            let artists = collect_ne_artists(&song.ar, &song.artists, None);
            let alb = song.al.as_ref().or(song.album.as_ref());
            let album_name = alb
                .map(|a| strip_inline_html(&unescape_music_text(&a.name)))
                .filter(|s| !s.is_empty());
            let (year, _) = song
                .publish_time
                .filter(|&t| t != 0)
                .or_else(|| alb.and_then(|a| a.publish_time).filter(|&t| t != 0))
                .map(parse_epoch_ms_year_and_date)
                .unwrap_or((None, None));
            let dur_secs = song
                .dt
                .or(song.duration)
                .filter(|&ms| ms > 0)
                .map(|ms| (ms / 1000) as u32);
            let pop_votes = song.pop.unwrap_or(0.0).max(0.0) as u32 * 25;

            let score = score_music_candidate(
                query,
                &title,
                &artists,
                album_name.as_deref(),
                year,
                dur_secs,
                pop_votes,
            );

            let mut sub = Vec::new();
            if !artists.is_empty() {
                sub.push(artists.join(" / "));
            }
            if let Some(ref a) = album_name {
                sub.push(a.clone());
            }
            if let Some(ds) = dur_secs.and_then(format_duration_mmss) {
                sub.push(ds);
            }

            scored.push((
                score,
                idx,
                ScrapeCandidate {
                    provider_id: "netease".to_string(),
                    provider_name: "网易云音乐".to_string(),
                    external_id: song.id.to_string(),
                    target,
                    title,
                    subtitle: (!sub.is_empty()).then(|| sub.join(" · ")),
                    year,
                    summary: None,
                    cover_url: upgrade_netease_cover(alb.and_then(|a| a.pic_url.as_deref()), 500),
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
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
            return Err(DomainError::Validation(format!(
                "invalid 网易云音乐 ID '{id}'"
            )));
        }
        let client = build_browser_http_client(creds, false)?;

        if target == ScrapeTargetKind::MusicAlbum {
            let url_v1 = format!("{ALBUM_V1_PREFIX}{id}");
            let resp = client
                .get(&url_v1)
                .header(reqwest::header::REFERER, NE_REFERER)
                .header(reqwest::header::COOKIE, NE_COOKIE)
                .send()
                .await
                .map_err(|e| {
                    DomainError::Storage(format!("网易云音乐 album detail failed: {e}"))
                })?;

            if let Ok(env) =
                read_json_response::<NeAlbumDetailEnvelope>(resp, "网易云音乐 album").await
                && env.code == 200
                && let Some(alb) = env.album
            {
                return Ok(map_netease_album_detail(alb, id));
            }

            let url_fb = format!("{ALBUM_FALLBACK_PREFIX}{id}");
            let resp_fb = client
                .get(&url_fb)
                .header(reqwest::header::REFERER, NE_REFERER)
                .header(reqwest::header::COOKIE, NE_COOKIE)
                .send()
                .await
                .map_err(|e| {
                    DomainError::Storage(format!("网易云音乐 album fallback failed: {e}"))
                })?;
            let env_fb: NeAlbumDetailEnvelope =
                read_json_response(resp_fb, "网易云音乐 album").await?;
            let alb = env_fb.album.ok_or_else(|| {
                DomainError::NotFound(format!("网易云音乐 album '{id}' not found"))
            })?;
            return Ok(map_netease_album_detail(alb, id));
        }

        let ids_param = format!("[{id}]");
        let detail_fut = client
            .get(SONG_DETAIL_URL)
            .header(reqwest::header::REFERER, NE_REFERER)
            .header(reqwest::header::COOKIE, NE_COOKIE)
            .query(&[("id", id), ("ids", ids_param.as_str())])
            .send();
        let lyric_fut = client
            .get(LYRIC_URL)
            .header(reqwest::header::REFERER, NE_REFERER)
            .header(reqwest::header::COOKIE, NE_COOKIE)
            .query(&[("id", id), ("lv", "1"), ("kv", "1"), ("tv", "-1")])
            .send();

        let (detail_res, lyric_res) = tokio::join!(detail_fut, lyric_fut);

        let detail_resp = detail_res
            .map_err(|e| DomainError::Storage(format!("网易云音乐 song detail failed: {e}")))?;
        let detail_env: NeSongDetailEnvelope =
            read_json_response(detail_resp, "网易云音乐 song").await?;
        let song =
            detail_env.songs.into_iter().next().ok_or_else(|| {
                DomainError::NotFound(format!("网易云音乐 song '{id}' not found"))
            })?;

        let lyric_env: Option<NeLyricEnvelope> = match lyric_res {
            Ok(r) if r.status().is_success() => {
                read_json_response::<NeLyricEnvelope>(r, "网易云音乐 lyric")
                    .await
                    .ok()
            }
            _ => None,
        };

        Ok(map_netease_song_detail(song, id, target, lyric_env))
    }
}

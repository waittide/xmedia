use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use xmedia_core::domain::error::{DomainError, DomainResult};

use crate::domain::config::ProviderCredentials;
use crate::domain::ports::ScraperProviderPort;
use crate::domain::scraper::{
    ProviderDescriptor, ScrapeCandidate, ScrapeQuery, ScrapeTargetKind, ScrapedMetadata,
};
use crate::infra::scrapers::cn_music_common::{
    build_music_search_keyword, decode_base64_utf8, format_duration_mmss,
    normalize_lrc_and_extract_plain, read_json_response, score_music_candidate,
    split_music_artists, unescape_music_text,
};
use crate::infra::scrapers::cn_novel_common::{
    build_browser_http_client, parse_year_and_date, strip_html_to_text, strip_inline_html,
};

pub struct QqMusicProvider;

const MUSICU_URL: &str = "https://u.y.qq.com/cgi-bin/musicu.fcg";
const LYRIC_URL: &str = "https://c.y.qq.com/lyric/fcgi-bin/fcg_query_lyric_new.fcg";
const QQ_REFERER: &str = "https://y.qq.com/";

#[derive(Debug, Deserialize, Default)]
pub(crate) struct QqSinger {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub title: String,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct QqAlbumBrief {
    #[serde(default)]
    pub mid: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub time_public: String,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct QqSongItem {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub mid: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub index_album: Option<i64>,
    #[serde(default)]
    pub index_cd: Option<i64>,
    #[serde(default)]
    pub time_public: String,
    #[serde(default)]
    pub interval: i64,
    #[serde(default)]
    pub singer: Vec<QqSinger>,
    #[serde(default)]
    pub album: QqAlbumBrief,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct QqAlbumSearchItem {
    #[serde(default, rename = "albumMID")]
    pub album_mid: String,
    #[serde(default, rename = "albumName")]
    pub album_name: String,
    #[serde(default, rename = "publicTime")]
    pub public_time: String,
    #[serde(default, rename = "singerName")]
    pub singer_name: String,
    #[serde(default)]
    pub singer_list: Vec<QqSinger>,
    #[serde(default)]
    pub song_count: i64,
}

#[derive(Debug, Deserialize, Default)]
struct QqSearchList<T> {
    #[serde(default)]
    list: Vec<T>,
}

#[derive(Debug, Deserialize, Default)]
struct QqSearchBody {
    #[serde(default)]
    song: QqSearchList<QqSongItem>,
    #[serde(default)]
    album: QqSearchList<QqAlbumSearchItem>,
}

#[derive(Debug, Deserialize, Default)]
struct QqSearchData {
    #[serde(default)]
    body: QqSearchBody,
}

#[derive(Debug, Deserialize, Default)]
struct QqSearchNode {
    #[serde(default)]
    code: i64,
    #[serde(default)]
    data: QqSearchData,
}

#[derive(Debug, Deserialize, Default)]
struct QqSearchEnvelope {
    #[serde(default)]
    req: QqSearchNode,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct QqInfoContentEntry {
    #[serde(default)]
    pub value: String,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct QqInfoField {
    #[serde(default)]
    pub content: Vec<QqInfoContentEntry>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct QqSongExtraInfo {
    #[serde(default)]
    pub company: Option<QqInfoField>,
    #[serde(default)]
    pub genre: Option<QqInfoField>,
    #[serde(default)]
    pub intro: Option<QqInfoField>,
    #[serde(default)]
    pub lan: Option<QqInfoField>,
    #[serde(default)]
    pub pub_time: Option<QqInfoField>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct QqSongDetailData {
    #[serde(default)]
    pub track_info: QqSongItem,
    #[serde(default)]
    pub info: QqSongExtraInfo,
}

#[derive(Debug, Deserialize, Default)]
struct QqSongDetailNode {
    #[serde(default)]
    code: i64,
    #[serde(default)]
    data: QqSongDetailData,
}

#[derive(Debug, Deserialize, Default)]
struct QqSongDetailEnvelope {
    #[serde(default)]
    songinfo: QqSongDetailNode,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct QqLyricResp {
    #[serde(default)]
    pub retcode: i64,
    #[serde(default)]
    pub lyric: String,
    #[serde(default)]
    pub trans: String,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct QqAlbumBasicInfo {
    #[serde(default)]
    pub album_mid: String,
    #[serde(default)]
    pub album_name: String,
    #[serde(default)]
    pub publish_date: String,
    #[serde(default)]
    pub desc: String,
    #[serde(default)]
    pub genre: String,
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub rec_labels: Vec<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct QqAlbumCompany {
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct QqAlbumSingerWrap {
    #[serde(default)]
    pub singer_list: Vec<QqSinger>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct QqAlbumDetailData {
    #[serde(default)]
    pub basic_info: QqAlbumBasicInfo,
    #[serde(default)]
    pub company: QqAlbumCompany,
    #[serde(default)]
    pub singer: QqAlbumSingerWrap,
}

#[derive(Debug, Deserialize, Default)]
struct QqAlbumDetailNode {
    #[serde(default)]
    code: i64,
    #[serde(default)]
    data: QqAlbumDetailData,
}

#[derive(Debug, Deserialize, Default)]
struct QqAlbumDetailEnvelope {
    #[serde(default)]
    album: QqAlbumDetailNode,
}

pub(crate) fn qq_album_cover_url(album_mid: &str, size: u32) -> Option<String> {
    let mid = album_mid.trim();
    if mid.is_empty() {
        return None;
    }
    Some(format!(
        "https://y.gtimg.cn/music/photo_new/T002R{size}x{size}M000{mid}.jpg"
    ))
}

fn extract_qq_direct_id(raw: &str, is_album: bool) -> Option<String> {
    let trimmed = raw.trim();
    let markers: &[&str] = if is_album {
        &["albumDetail/", "album/", "albummid="]
    } else {
        &["songDetail/", "songmid="]
    };
    for marker in markers {
        if let Some(idx) = trimmed.find(marker) {
            let after = &trimmed[idx + marker.len()..];
            let token: String = after
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric())
                .collect();
            if token.len() >= 5 {
                return Some(token);
            }
        }
    }
    if trimmed.len() == 14
        && trimmed.starts_with("00")
        && trimmed.bytes().all(|b| b.is_ascii_alphanumeric())
    {
        return Some(trimmed.to_string());
    }
    None
}

fn collect_qq_singers(singers: &[QqSinger]) -> Vec<String> {
    let mut out = Vec::new();
    for s in singers {
        let raw = if !s.name.trim().is_empty() {
            &s.name
        } else {
            &s.title
        };
        let cleaned = strip_inline_html(&unescape_music_text(raw));
        if !cleaned.is_empty() && !out.iter().any(|e| e == &cleaned) {
            out.push(cleaned);
        }
    }
    out
}

fn first_info_value(field: &Option<QqInfoField>) -> Option<String> {
    let v = field.as_ref()?.content.first()?.value.trim();
    let cleaned = strip_html_to_text(v);
    (!cleaned.is_empty()).then_some(cleaned)
}

pub(crate) fn map_qq_song_detail(
    data: QqSongDetailData,
    external_id: &str,
    target: ScrapeTargetKind,
    lyric_resp: Option<QqLyricResp>,
) -> ScrapedMetadata {
    let ti = data.track_info;
    let raw_title = if !ti.name.trim().is_empty() {
        &ti.name
    } else {
        &ti.title
    };
    let title = {
        let t = strip_inline_html(&unescape_music_text(raw_title));
        (!t.is_empty()).then_some(t)
    };
    let creators = collect_qq_singers(&ti.singer);
    let album_raw = if !ti.album.name.trim().is_empty() {
        &ti.album.name
    } else {
        &ti.album.title
    };
    let album_name = {
        let a = strip_inline_html(&unescape_music_text(album_raw));
        (!a.is_empty()).then_some(a)
    };

    let date_source = if !ti.time_public.trim().is_empty() {
        Some(ti.time_public.clone())
    } else if !ti.album.time_public.trim().is_empty() {
        Some(ti.album.time_public.clone())
    } else {
        first_info_value(&data.info.pub_time)
    };
    let (year, published_date) = date_source
        .as_deref()
        .map(parse_year_and_date)
        .unwrap_or((None, None));

    let track_no = ti
        .index_album
        .and_then(|n| u32::try_from(n).ok())
        .filter(|&n| n > 0);
    let disc_no = track_no.and(
        ti.index_cd
            .filter(|&c| c >= 0)
            .and_then(|c| u32::try_from(c + 1).ok()),
    );

    let publisher_or_studio = first_info_value(&data.info.company);
    let language = first_info_value(&data.info.lan);
    let genres: Vec<String> = first_info_value(&data.info.genre).into_iter().collect();
    let summary = first_info_value(&data.info.intro);
    let cover_url = qq_album_cover_url(&ti.album.mid, 800);

    let (synced_lyrics, plain_lyrics) = lyric_resp
        .filter(|r| r.retcode == 0 && !r.lyric.trim().is_empty())
        .and_then(|r| {
            let orig = decode_base64_utf8(&r.lyric)?;
            let trans = decode_base64_utf8(&r.trans);
            Some(normalize_lrc_and_extract_plain(&orig, trans.as_deref()))
        })
        .unwrap_or((None, None));

    let resolved_id = if !ti.mid.trim().is_empty() {
        ti.mid
    } else {
        external_id.to_string()
    };

    ScrapedMetadata {
        provider_id: "qq".to_string(),
        external_id: resolved_id,
        target: Some(target),
        title,
        series_or_album: album_name,
        creators,
        publisher_or_studio,
        year,
        published_date,
        language,
        track_or_episode_number: track_no,
        disc_or_season_number: disc_no,
        summary,
        genres,
        cover_url,
        synced_lyrics,
        plain_lyrics,
        ..Default::default()
    }
}

pub(crate) fn map_qq_album_detail(data: QqAlbumDetailData, external_id: &str) -> ScrapedMetadata {
    let b = data.basic_info;
    let title = {
        let t = strip_inline_html(&unescape_music_text(&b.album_name));
        (!t.is_empty()).then_some(t)
    };
    let creators = collect_qq_singers(&data.singer.singer_list);
    let (year, published_date) = parse_year_and_date(&b.publish_date);
    let publisher = {
        let c = strip_inline_html(&unescape_music_text(&data.company.name));
        (!c.is_empty()).then_some(c)
    };
    let language = {
        let l = strip_inline_html(&b.language);
        (!l.is_empty()).then_some(l)
    };
    let genres: Vec<String> = {
        let g = strip_inline_html(&b.genre);
        (!g.is_empty()).then_some(vec![g]).unwrap_or_default()
    };
    let tags: Vec<String> = b
        .rec_labels
        .into_iter()
        .map(|s| strip_inline_html(&s))
        .filter(|s| !s.is_empty())
        .collect();
    let summary = {
        let d = strip_html_to_text(&b.desc);
        (!d.is_empty()).then_some(d)
    };
    let mid = if !b.album_mid.trim().is_empty() {
        b.album_mid
    } else {
        external_id.to_string()
    };
    let cover_url = qq_album_cover_url(&mid, 800);

    ScrapedMetadata {
        provider_id: "qq".to_string(),
        external_id: mid,
        target: Some(ScrapeTargetKind::MusicAlbum),
        title: title.clone(),
        series_or_album: title,
        creators,
        publisher_or_studio: publisher,
        year,
        published_date,
        language,
        summary,
        genres,
        tags,
        cover_url,
        ..Default::default()
    }
}

#[async_trait]
impl ScraperProviderPort for QqMusicProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "qq".to_string(),
            name: "QQ音乐".to_string(),
            description: "腾讯 QQ 音乐免登录公开曲库（提供华语/日韩流行单曲、专辑、800px 封面与双语 LRC 同步歌词）".to_string(),
            website: "https://y.qq.com".to_string(),
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

        if let Some(direct_id) = extract_qq_direct_id(&query.keyword, is_album)
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
                provider_id: "qq".to_string(),
                provider_name: "QQ音乐".to_string(),
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
        let search_type = if is_album { 2 } else { 0 };
        let payload = json!({
            "comm": { "ct": 26, "cv": 2010101, "v": 2010101 },
            "req": {
                "module": "music.search.SearchCgiService",
                "method": "DoSearchForQQMusicDesktop",
                "param": {
                    "num_per_page": 20,
                    "page_num": 1,
                    "query": search_kw,
                    "search_type": search_type,
                }
            }
        });

        let resp = client
            .post(MUSICU_URL)
            .header(reqwest::header::REFERER, QQ_REFERER)
            .json(&payload)
            .send()
            .await
            .map_err(|e| DomainError::Storage(format!("QQ音乐 search failed: {e}")))?;

        if !resp.status().is_success() {
            return Err(DomainError::Storage(format!(
                "QQ音乐 search returned HTTP {}",
                resp.status()
            )));
        }

        let env: QqSearchEnvelope = read_json_response(resp, "QQ音乐 search").await?;
        if env.req.code != 0 {
            return Ok(Vec::new());
        }

        if is_album {
            let mut scored: Vec<(i32, usize, ScrapeCandidate)> = Vec::new();
            for (idx, item) in env.req.data.body.album.list.into_iter().enumerate() {
                if item.album_mid.trim().is_empty() {
                    continue;
                }
                let title = strip_inline_html(&unescape_music_text(&item.album_name));
                if title.is_empty() {
                    continue;
                }
                let mut artists = collect_qq_singers(&item.singer_list);
                if artists.is_empty() && !item.singer_name.trim().is_empty() {
                    artists = split_music_artists(&item.singer_name);
                }
                let (year, _) = parse_year_and_date(&item.public_time);
                let score = score_music_candidate(
                    query,
                    &title,
                    &artists,
                    Some(&title),
                    year,
                    None,
                    u32::try_from(item.song_count)
                        .unwrap_or(0)
                        .saturating_mul(20),
                );
                let mut sub = Vec::new();
                if !artists.is_empty() {
                    sub.push(artists.join(" / "));
                }
                if item.song_count > 0 {
                    sub.push(format!("{} 首", item.song_count));
                }
                scored.push((
                    score,
                    idx,
                    ScrapeCandidate {
                        provider_id: "qq".to_string(),
                        provider_name: "QQ音乐".to_string(),
                        external_id: item.album_mid.clone(),
                        target,
                        title,
                        subtitle: (!sub.is_empty()).then(|| sub.join(" · ")),
                        year,
                        summary: None,
                        cover_url: qq_album_cover_url(&item.album_mid, 500),
                        synced_lyrics: None,
                        plain_lyrics: None,
                    },
                ));
            }
            scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
            return Ok(scored.into_iter().take(15).map(|(_, _, c)| c).collect());
        }

        let mut scored: Vec<(i32, usize, ScrapeCandidate)> = Vec::new();
        for (idx, item) in env.req.data.body.song.list.into_iter().enumerate() {
            let ext_id = if !item.mid.trim().is_empty() {
                item.mid.clone()
            } else if item.id > 0 {
                item.id.to_string()
            } else {
                continue;
            };
            let raw_t = if !item.name.trim().is_empty() {
                &item.name
            } else {
                &item.title
            };
            let title = strip_inline_html(&unescape_music_text(raw_t));
            if title.is_empty() {
                continue;
            }
            let artists = collect_qq_singers(&item.singer);
            let album_raw = if !item.album.name.trim().is_empty() {
                &item.album.name
            } else {
                &item.album.title
            };
            let album = {
                let a = strip_inline_html(&unescape_music_text(album_raw));
                (!a.is_empty()).then_some(a)
            };
            let (year, _) = parse_year_and_date(&item.time_public);
            let dur_u32 = u32::try_from(item.interval).ok().filter(|&d| d > 0);
            let score =
                score_music_candidate(query, &title, &artists, album.as_deref(), year, dur_u32, 0);

            let mut sub = Vec::new();
            if !artists.is_empty() {
                sub.push(artists.join(" / "));
            }
            if let Some(ref alb) = album {
                sub.push(alb.clone());
            }
            if let Some(dur) = dur_u32.and_then(format_duration_mmss) {
                sub.push(dur);
            }

            scored.push((
                score,
                idx,
                ScrapeCandidate {
                    provider_id: "qq".to_string(),
                    provider_name: "QQ音乐".to_string(),
                    external_id: ext_id,
                    target,
                    title,
                    subtitle: (!sub.is_empty()).then(|| sub.join(" · ")),
                    year,
                    summary: None,
                    cover_url: qq_album_cover_url(&item.album.mid, 500),
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
            return Err(DomainError::Validation("empty QQ音乐 external_id".into()));
        }
        let client = build_browser_http_client(creds, false)?;

        if target == ScrapeTargetKind::MusicAlbum {
            let payload = json!({
                "comm": { "ct": 26, "cv": 2010101, "v": 2010101 },
                "album": {
                    "module": "music.musichallAlbum.AlbumInfoServer",
                    "method": "GetAlbumDetail",
                    "param": { "albumMid": id }
                }
            });
            let resp = client
                .post(MUSICU_URL)
                .header(reqwest::header::REFERER, QQ_REFERER)
                .json(&payload)
                .send()
                .await
                .map_err(|e| DomainError::Storage(format!("QQ音乐 album detail failed: {e}")))?;
            if !resp.status().is_success() {
                return Err(DomainError::Storage(format!(
                    "QQ音乐 album detail returned HTTP {}",
                    resp.status().as_u16()
                )));
            }
            let env: QqAlbumDetailEnvelope = read_json_response(resp, "QQ音乐 album").await?;
            if env.album.code != 0 {
                return Err(DomainError::NotFound(format!(
                    "QQ音乐 album '{id}' not found"
                )));
            }
            return Ok(map_qq_album_detail(env.album.data, id));
        }

        let is_numeric_id = id.bytes().all(|b| b.is_ascii_digit());
        let song_param = if is_numeric_id {
            json!({ "song_type": 0, "song_id": id.parse::<u64>().unwrap_or(0) })
        } else {
            json!({ "song_type": 0, "song_mid": id })
        };
        let detail_payload = json!({
            "comm": { "ct": 26, "cv": 2010101, "v": 2010101 },
            "songinfo": {
                "module": "music.pf_song_detail_svr",
                "method": "get_song_detail_yqq",
                "param": song_param
            }
        });

        let lyric_key = if is_numeric_id { "musicid" } else { "songmid" };
        let detail_fut = client
            .post(MUSICU_URL)
            .header(reqwest::header::REFERER, QQ_REFERER)
            .json(&detail_payload)
            .send();
        let lyric_fut = client
            .get(LYRIC_URL)
            .header(reqwest::header::REFERER, QQ_REFERER)
            .query(&[
                (lyric_key, id),
                ("format", "json"),
                ("nobase64", "0"),
                ("g_tk", "5381"),
            ])
            .send();

        let (detail_res, lyric_res) = tokio::join!(detail_fut, lyric_fut);

        let detail_resp = detail_res
            .map_err(|e| DomainError::Storage(format!("QQ音乐 song detail failed: {e}")))?;
        if !detail_resp.status().is_success() {
            return Err(DomainError::Storage(format!(
                "QQ音乐 song detail returned HTTP {}",
                detail_resp.status().as_u16()
            )));
        }
        let detail_env: QqSongDetailEnvelope =
            read_json_response(detail_resp, "QQ音乐 song detail").await?;
        if detail_env.songinfo.code != 0 {
            return Err(DomainError::NotFound(format!(
                "QQ音乐 song '{id}' not found"
            )));
        }

        let lyric_parsed: Option<QqLyricResp> = match lyric_res {
            Ok(r) if r.status().is_success() => {
                read_json_response::<QqLyricResp>(r, "QQ音乐 lyric")
                    .await
                    .ok()
            }
            _ => None,
        };

        Ok(map_qq_song_detail(
            detail_env.songinfo.data,
            id,
            target,
            lyric_parsed,
        ))
    }
}

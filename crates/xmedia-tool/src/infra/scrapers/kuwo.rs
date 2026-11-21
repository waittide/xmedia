use std::io::Read;
use std::sync::LazyLock;

use async_trait::async_trait;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use encoding_rs::GB18030;
use flate2::read::ZlibDecoder;
use regex::Regex;
use serde::Deserialize;
use xmedia_core::domain::error::{DomainError, DomainResult};

use crate::domain::config::ProviderCredentials;
use crate::domain::ports::ScraperProviderPort;
use crate::domain::scraper::{
    ProviderDescriptor, ScrapeCandidate, ScrapeQuery, ScrapeTargetKind, ScrapedMetadata,
};
use crate::infra::scrapers::cn_music_common::{
    build_music_search_keyword, format_duration_mmss, format_kuwo_lrclist,
    normalize_lrc_and_extract_plain, read_json_response, score_music_candidate,
    split_music_artists, unescape_music_text,
};
use crate::infra::scrapers::cn_novel_common::{
    build_browser_http_client, extract_numeric_id_from_query, normalize_cover_url,
    parse_year_and_date, strip_html_to_text, strip_inline_html,
};

pub struct KuwoMusicProvider;

const KW_SEARCH_URL: &str = "https://search.kuwo.cn/r.s";
const KW_SONG_LRC_URL: &str = "https://m.kuwo.cn/newh5/singles/songinfoandlrc";
const KW_NEWLYRIC_URL: &str = "https://newlyric.kuwo.cn/newlyric.lrc";
const KW_PIC_WEB_URL: &str = "https://artistpicserver.kuwo.cn/pic.web";
const YEELION_KEY: &[u8] = b"yeelion";

static RE_KW_WORD_TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<-?\d+,-?\d+(?:,-?\d+)?>").unwrap());

#[derive(Debug, Deserialize, Default)]
pub(crate) struct KwSongSearchItem {
    #[serde(default, rename = "DC_TARGETID")]
    pub dc_targetid: String,
    #[serde(default, rename = "MUSICRID")]
    pub musicrid: String,
    #[serde(default, rename = "SONGNAME")]
    pub songname: String,
    #[serde(default, rename = "NAME")]
    pub name: String,
    #[serde(default, rename = "ARTIST")]
    pub artist: String,
    #[serde(default, rename = "ALBUM")]
    pub album: String,
    #[serde(default, rename = "ALBUMID")]
    pub albumid: String,
    #[serde(default, rename = "DURATION")]
    pub duration: String,
    #[serde(default)]
    pub web_albumpic_short: String,
    #[serde(default, rename = "SCORE100")]
    pub score100: String,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct KwAlbumSearchItem {
    #[serde(default)]
    pub albumid: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub artist: String,
    #[serde(default, rename = "pub")]
    pub pub_date: String,
    #[serde(default)]
    pub pic: String,
    #[serde(default)]
    pub hts_img: String,
    #[serde(default)]
    pub company: String,
    #[serde(default)]
    pub info: String,
    #[serde(default)]
    pub lang: String,
    #[serde(default)]
    pub musiclist: Vec<KwAlbumTrackItem>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct KwAlbumTrackItem {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub track: String,
}

#[derive(Debug, Deserialize, Default)]
struct KwSearchEnvelope {
    #[serde(default)]
    abslist: Vec<KwSongSearchItem>,
    #[serde(default)]
    albumlist: Vec<KwAlbumSearchItem>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct KwSongDetailInfo {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub song_name: String,
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub album: String,
    #[serde(default)]
    pub album_id: String,
    #[serde(default)]
    pub pic: String,
    #[serde(default)]
    pub up_time: String,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct KwLrcItem {
    #[serde(default)]
    pub line_lyric: String,
    #[serde(default)]
    pub time: String,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct KwSongAndLrcData {
    #[serde(default)]
    pub songinfo: Option<KwSongDetailInfo>,
    #[serde(default)]
    pub lrclist: Option<Vec<KwLrcItem>>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct KwSongAndLrcEnvelope {
    #[serde(default)]
    pub data: Option<KwSongAndLrcData>,
}

#[derive(Debug, Default)]
struct KwParsedNewLyric {
    ti: Option<String>,
    ar: Option<String>,
    al: Option<String>,
    synced_lyrics: Option<String>,
    plain_lyrics: Option<String>,
}

fn xor_yeelion(input: &[u8]) -> Vec<u8> {
    input
        .iter()
        .enumerate()
        .map(|(i, b)| b ^ YEELION_KEY[i % YEELION_KEY.len()])
        .collect()
}

fn parse_kuwo_external_id(raw: &str) -> (String, Option<String>) {
    let trimmed = raw.trim().strip_prefix("MUSIC_").unwrap_or(raw.trim());
    if let Some((r, a)) = trimmed.split_once(':') {
        let alb = (!a.trim().is_empty() && a.trim() != "0").then(|| a.trim().to_string());
        (r.trim().to_string(), alb)
    } else {
        (trimmed.to_string(), None)
    }
}

async fn fetch_kuwo_newlyric_fallback(
    client: &reqwest::Client,
    rid: &str,
) -> Option<KwParsedNewLyric> {
    let query_plain = format!("user=12345,web,web,web&localhost&req=1&rid=MUSIC_{rid}&lrcx=0");
    let param = BASE64_STANDARD.encode(xor_yeelion(query_plain.as_bytes()));
    let url = format!("{KW_NEWLYRIC_URL}?{param}");
    let mut resp = client.get(&url).send().await.ok()?;
    let mut raw = Vec::new();
    while let Some(chunk) = resp.chunk().await.ok()? {
        if raw.len().saturating_add(chunk.len()) > 2 * 1024 * 1024 {
            return None;
        }
        raw.extend_from_slice(&chunk);
    }
    let sep = raw.windows(4).position(|w| w == b"\r\n\r\n")?;
    let header_str = String::from_utf8_lossy(&raw[..sep]);
    if header_str.contains("tp=none") {
        return None;
    }
    let is_lrcx = header_str.contains("lrcx=1");
    let compressed = &raw[sep + 4..];
    const MAX_KUWO_LYRIC_BYTES: u64 = 4 * 1024 * 1024;
    let z = ZlibDecoder::new(compressed);
    let mut inflated = Vec::new();
    z.take(MAX_KUWO_LYRIC_BYTES + 1)
        .read_to_end(&mut inflated)
        .ok()?;
    if (inflated.len() as u64) > MAX_KUWO_LYRIC_BYTES {
        return None;
    }

    let decoded_bytes = if is_lrcx {
        let compact: Vec<u8> = inflated
            .into_iter()
            .filter(|b| !b.is_ascii_whitespace())
            .collect();
        let inner = BASE64_STANDARD.decode(&compact).ok()?;
        xor_yeelion(&inner)
    } else {
        inflated
    };

    let (cow, _, _) = GB18030.decode(&decoded_bytes);
    let stripped = RE_KW_WORD_TAG.replace_all(&cow, "");

    let mut ti = None;
    let mut ar = None;
    let mut al = None;
    for line in stripped.lines() {
        let l = line.trim();
        if let Some(v) = l.strip_prefix("[ti:").and_then(|s| s.strip_suffix(']')) {
            let t = strip_inline_html(&unescape_music_text(v));
            if !t.is_empty() {
                ti = Some(t);
            }
        } else if let Some(v) = l.strip_prefix("[ar:").and_then(|s| s.strip_suffix(']')) {
            let a = strip_inline_html(&unescape_music_text(v));
            if !a.is_empty() {
                ar = Some(a);
            }
        } else if let Some(v) = l.strip_prefix("[al:").and_then(|s| s.strip_suffix(']')) {
            let a = strip_inline_html(&unescape_music_text(v));
            if !a.is_empty() {
                al = Some(a);
            }
        }
    }

    let (synced_lyrics, plain_lyrics) = normalize_lrc_and_extract_plain(&stripped, None);
    Some(KwParsedNewLyric {
        ti,
        ar,
        al,
        synced_lyrics,
        plain_lyrics,
    })
}

pub(crate) fn upgrade_kuwo_cover(raw: &str, target_size: u32) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.contains("default") {
        return None;
    }
    if trimmed.starts_with("http://")
        || trimmed.starts_with("https://")
        || trimmed.starts_with("//")
    {
        let https = normalize_cover_url(trimmed)?;
        for sz in ["/120/", "/240/", "/300/", "/500/"] {
            if https.contains(sz) {
                return Some(https.replacen(sz, &format!("/{target_size}/"), 1));
            }
        }
        return Some(https);
    }
    let rel = trimmed
        .strip_prefix("120/")
        .or_else(|| trimmed.strip_prefix("240/"))
        .or_else(|| trimmed.strip_prefix("300/"))
        .unwrap_or(trimmed);
    Some(format!(
        "https://img2.kuwo.cn/star/albumcover/{target_size}/{rel}"
    ))
}

fn extract_kuwo_rid(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if let Some(stripped) = trimmed.strip_prefix("MUSIC_")
        && stripped.bytes().all(|b| b.is_ascii_digit())
        && !stripped.is_empty()
    {
        return Some(stripped.to_string());
    }
    extract_numeric_id_from_query(trimmed, &["play_detail/", "yinyue/", "musicId="])
}

pub(crate) fn map_kuwo_album_detail(alb: KwAlbumSearchItem, external_id: &str) -> ScrapedMetadata {
    let title = {
        let t = strip_inline_html(&unescape_music_text(&alb.name));
        (!t.is_empty()).then_some(t)
    };
    let creators = split_music_artists(&alb.artist);
    let (year, published_date) = parse_year_and_date(&alb.pub_date);
    let publisher = {
        let c = strip_inline_html(&unescape_music_text(&alb.company));
        (!c.is_empty()).then_some(c)
    };
    let language = {
        let l = strip_inline_html(&alb.lang);
        (!l.is_empty()).then_some(l)
    };
    let summary = {
        let d = strip_html_to_text(&alb.info);
        (!d.is_empty()).then_some(d)
    };
    let cover_url =
        upgrade_kuwo_cover(&alb.hts_img, 1000).or_else(|| upgrade_kuwo_cover(&alb.pic, 1000));
    let resolved_id = if !alb.albumid.trim().is_empty() {
        alb.albumid
    } else {
        external_id.to_string()
    };

    ScrapedMetadata {
        provider_id: "kuwo".to_string(),
        external_id: resolved_id,
        target: Some(ScrapeTargetKind::MusicAlbum),
        title: title.clone(),
        series_or_album: title,
        creators,
        publisher_or_studio: publisher,
        year,
        published_date,
        language,
        summary,
        cover_url,
        ..Default::default()
    }
}

pub(crate) fn map_kuwo_song_detail(
    rid: &str,
    target: ScrapeTargetKind,
    data: KwSongAndLrcData,
    album_detail: Option<KwAlbumSearchItem>,
    fallback_cover: Option<String>,
) -> ScrapedMetadata {
    let info = data.songinfo.unwrap_or_default();
    let matched_track = album_detail
        .as_ref()
        .and_then(|ad| ad.musiclist.iter().find(|t| t.id.trim() == rid));

    let title = {
        let t = strip_inline_html(&unescape_music_text(&info.song_name));
        if !t.is_empty() {
            Some(t)
        } else {
            matched_track
                .map(|mt| strip_inline_html(&unescape_music_text(&mt.name)))
                .filter(|s| !s.is_empty())
        }
    };

    let mut creators = split_music_artists(&info.artist);
    if creators.is_empty()
        && let Some(mt) = matched_track
    {
        creators = split_music_artists(&mt.artist);
    }
    if creators.is_empty()
        && let Some(ref ad) = album_detail
    {
        creators = split_music_artists(&ad.artist);
    }

    let series_or_album = {
        let a = strip_inline_html(&unescape_music_text(&info.album));
        if !a.is_empty() {
            Some(a)
        } else {
            album_detail
                .as_ref()
                .map(|ad| strip_inline_html(&unescape_music_text(&ad.name)))
                .filter(|s| !s.is_empty())
        }
    };

    let (year, published_date) = album_detail
        .as_ref()
        .map(|ad| parse_year_and_date(&ad.pub_date))
        .filter(|(y, _)| y.is_some())
        .unwrap_or_else(|| parse_year_and_date(&info.up_time));

    let publisher = album_detail
        .as_ref()
        .map(|ad| strip_inline_html(&unescape_music_text(&ad.company)))
        .filter(|s| !s.is_empty());
    let language = album_detail
        .as_ref()
        .map(|ad| strip_inline_html(&ad.lang))
        .filter(|s| !s.is_empty());

    let track_number = matched_track
        .and_then(|t| t.track.trim().parse::<u32>().ok())
        .filter(|&n| n > 0);

    let cover_url = upgrade_kuwo_cover(&info.pic, 1000)
        .or_else(|| {
            album_detail.as_ref().and_then(|ad| {
                upgrade_kuwo_cover(&ad.hts_img, 1000).or_else(|| upgrade_kuwo_cover(&ad.pic, 1000))
            })
        })
        .or(fallback_cover);

    let lrc_pairs: Vec<(f64, String)> = data
        .lrclist
        .unwrap_or_default()
        .into_iter()
        .filter_map(|item| {
            let secs = item.time.trim().parse::<f64>().ok()?;
            Some((secs, item.line_lyric))
        })
        .collect();
    let (synced_lyrics, plain_lyrics) = format_kuwo_lrclist(&lrc_pairs);

    let resolved_id = if !info.id.trim().is_empty() {
        info.id
    } else {
        rid.to_string()
    };

    ScrapedMetadata {
        provider_id: "kuwo".to_string(),
        external_id: resolved_id,
        target: Some(target),
        title,
        series_or_album,
        creators,
        publisher_or_studio: publisher,
        year,
        published_date,
        language,
        track_or_episode_number: track_number,
        cover_url,
        synced_lyrics,
        plain_lyrics,
        ..Default::default()
    }
}

#[async_trait]
impl ScraperProviderPort for KuwoMusicProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "kuwo".to_string(),
            name: "酷我音乐".to_string(),
            description: "酷我音乐免登录公开曲库（提供华语经典/无损曲目元数据、1000px 封面与逐行时间轴 LRC 歌词）".to_string(),
            website: "https://www.kuwo.cn".to_string(),
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

        if is_album {
            if let Some(alb_id) = extract_numeric_id_from_query(&query.keyword, &["album_detail/"])
                && (query.keyword.contains("kuwo.cn") || query.keyword.trim().len() >= 4)
                && let Ok(detail) = self.fetch_detail(&alb_id, target, "zh-CN", creds).await
                && let Some(title) = detail.title
            {
                return Ok(vec![ScrapeCandidate {
                    provider_id: "kuwo".to_string(),
                    provider_name: "酷我音乐".to_string(),
                    external_id: detail.external_id,
                    target,
                    title,
                    subtitle: (!detail.creators.is_empty()).then(|| detail.creators.join(" / ")),
                    year: detail.year,
                    summary: detail.summary,
                    cover_url: detail.cover_url,
                    synced_lyrics: None,
                    plain_lyrics: None,
                }]);
            }
        } else if let Some(rid) = extract_kuwo_rid(&query.keyword)
            && (query.keyword.contains("kuwo.cn")
                || query.keyword.trim().starts_with("MUSIC_")
                || query.keyword.trim().len() >= 5)
            && let Ok(detail) = self.fetch_detail(&rid, target, "zh-CN", creds).await
            && let Some(title) = detail.title
        {
            let mut sub = Vec::new();
            if !detail.creators.is_empty() {
                sub.push(detail.creators.join(" / "));
            }
            if let Some(alb) = detail.series_or_album {
                sub.push(alb);
            }
            return Ok(vec![ScrapeCandidate {
                provider_id: "kuwo".to_string(),
                provider_name: "酷我音乐".to_string(),
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
        let ft = if is_album { "album" } else { "music" };
        let resp = client
            .get(KW_SEARCH_URL)
            .query(&[
                ("all", search_kw.as_str()),
                ("ft", ft),
                ("itemset", "web_2013"),
                ("client", "kt"),
                ("pn", "0"),
                ("rn", "20"),
                ("rformat", "json"),
                ("encoding", "utf8"),
                ("show_copyright_off", "1"),
                ("vipver", "1"),
                ("pcjson", "1"),
            ])
            .send()
            .await
            .map_err(|e| DomainError::Storage(format!("酷我音乐 search failed: {e}")))?;

        let env: KwSearchEnvelope = read_json_response(resp, "酷我音乐 search").await?;

        if is_album {
            let mut scored: Vec<(i32, usize, ScrapeCandidate)> = Vec::new();
            for (idx, alb) in env.albumlist.into_iter().enumerate() {
                let id = alb.albumid.trim().to_string();
                if id.is_empty() || id == "0" {
                    continue;
                }
                let title = strip_inline_html(&unescape_music_text(&alb.name));
                if title.is_empty() {
                    continue;
                }
                let artists = split_music_artists(&alb.artist);
                let (year, _) = parse_year_and_date(&alb.pub_date);
                let score =
                    score_music_candidate(query, &title, &artists, Some(&title), year, None, 0);
                scored.push((
                    score,
                    idx,
                    ScrapeCandidate {
                        provider_id: "kuwo".to_string(),
                        provider_name: "酷我音乐".to_string(),
                        external_id: id,
                        target,
                        title,
                        subtitle: (!artists.is_empty()).then(|| artists.join(" / ")),
                        year,
                        summary: None,
                        cover_url: upgrade_kuwo_cover(&alb.hts_img, 500)
                            .or_else(|| upgrade_kuwo_cover(&alb.pic, 500)),
                        synced_lyrics: None,
                        plain_lyrics: None,
                    },
                ));
            }
            scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
            return Ok(scored.into_iter().take(15).map(|(_, _, c)| c).collect());
        }

        let mut scored: Vec<(i32, usize, ScrapeCandidate)> = Vec::new();
        for (idx, item) in env.abslist.into_iter().enumerate() {
            let rid = if !item.dc_targetid.trim().is_empty() {
                item.dc_targetid.trim().to_string()
            } else {
                item.musicrid
                    .trim()
                    .strip_prefix("MUSIC_")
                    .unwrap_or("")
                    .to_string()
            };
            if rid.is_empty() || rid == "0" {
                continue;
            }
            let raw_t = if !item.songname.trim().is_empty() {
                &item.songname
            } else {
                &item.name
            };
            let title = strip_inline_html(&unescape_music_text(raw_t));
            if title.is_empty() {
                continue;
            }
            let artists = split_music_artists(&item.artist);
            let album_name = {
                let a = strip_inline_html(&unescape_music_text(&item.album));
                (!a.is_empty()).then_some(a)
            };
            let alb_id = item.albumid.trim();
            let ext_id = if !alb_id.is_empty() && alb_id != "0" {
                format!("{rid}:{alb_id}")
            } else {
                rid
            };
            let dur_secs = item.duration.trim().parse::<u32>().ok().filter(|&d| d > 0);
            let pop_votes = item
                .score100
                .trim()
                .parse::<u32>()
                .unwrap_or(0)
                .saturating_mul(25);

            let score = score_music_candidate(
                query,
                &title,
                &artists,
                album_name.as_deref(),
                None,
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
                    provider_id: "kuwo".to_string(),
                    provider_name: "酷我音乐".to_string(),
                    external_id: ext_id,
                    target,
                    title,
                    subtitle: (!sub.is_empty()).then(|| sub.join(" · ")),
                    year: None,
                    summary: None,
                    cover_url: upgrade_kuwo_cover(&item.web_albumpic_short, 500),
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
        let client = build_browser_http_client(creds, false)?;

        if target == ScrapeTargetKind::MusicAlbum {
            let id = external_id.trim();
            if id.is_empty() {
                return Err(DomainError::Validation("empty 酷我音乐 album ID".into()));
            }
            let resp = client
                .get(KW_SEARCH_URL)
                .query(&[
                    ("stype", "albuminfo"),
                    ("albumid", id),
                    ("show_copyright_off", "1"),
                    ("alflac", "1"),
                    ("pcmp4", "1"),
                    ("encoding", "utf8"),
                    ("plat", "pc"),
                    ("vipver", "NEW_8.7.6.0.BCS31"),
                    ("devid", "38668888"),
                    ("newver", "1"),
                    ("pcjson", "1"),
                ])
                .send()
                .await
                .map_err(|e| DomainError::Storage(format!("酷我音乐 album detail failed: {e}")))?;
            let alb: KwAlbumSearchItem = read_json_response(resp, "酷我音乐 album").await?;
            if alb.name.trim().is_empty() {
                return Err(DomainError::NotFound(format!(
                    "酷我音乐 album '{id}' not found"
                )));
            }
            return Ok(map_kuwo_album_detail(alb, id));
        }

        let (id, hinted_album_id) = parse_kuwo_external_id(external_id);
        if id.is_empty() {
            return Err(DomainError::Validation("empty 酷我音乐 ID".into()));
        }

        let referer = format!("https://m.kuwo.cn/yinyue/{id}");
        let mut h5_data: Option<KwSongAndLrcData> = None;
        for _ in 0..2 {
            if let Ok(resp) = client
                .get(KW_SONG_LRC_URL)
                .header(reqwest::header::REFERER, &referer)
                .query(&[("musicId", id.as_str()), ("httpsStatus", "1")])
                .send()
                .await
                && let Ok(env) =
                    read_json_response::<KwSongAndLrcEnvelope>(resp, "酷我音乐 song").await
                && let Some(d) = env.data
            {
                h5_data = Some(d);
                break;
            }
        }

        let album_id = h5_data
            .as_ref()
            .and_then(|d| d.songinfo.as_ref())
            .map(|s| s.album_id.trim().to_string())
            .filter(|a| !a.is_empty() && a != "0")
            .or(hinted_album_id);

        let has_pic = h5_data
            .as_ref()
            .and_then(|d| d.songinfo.as_ref())
            .is_some_and(|s| !s.pic.trim().is_empty());

        let mut album_detail: Option<KwAlbumSearchItem> = None;
        let mut fallback_cover: Option<String> = None;

        if target != ScrapeTargetKind::Lyrics {
            if let Some(ref aid) = album_id
                && let Ok(r) = client
                    .get(KW_SEARCH_URL)
                    .query(&[
                        ("stype", "albuminfo"),
                        ("albumid", aid.as_str()),
                        ("show_copyright_off", "1"),
                        ("alflac", "1"),
                        ("pcmp4", "1"),
                        ("encoding", "utf8"),
                        ("plat", "pc"),
                        ("vipver", "NEW_8.7.6.0.BCS31"),
                        ("devid", "38668888"),
                        ("newver", "1"),
                        ("pcjson", "1"),
                    ])
                    .send()
                    .await
                && let Ok(parsed) = read_json_response::<KwAlbumSearchItem>(r, "酷我专辑补充").await
                && !parsed.name.trim().is_empty()
            {
                album_detail = Some(parsed);
            }

            if !has_pic
                && album_detail.is_none()
                && let Ok(r) = client
                    .get(KW_PIC_WEB_URL)
                    .query(&[
                        ("corp", "kuwo"),
                        ("type", "rid_pic"),
                        ("pictype", "500"),
                        ("size", "500"),
                        ("rid", id.as_str()),
                    ])
                    .send()
                    .await
                && let Ok(u) = r.text().await
            {
                fallback_cover = upgrade_kuwo_cover(u.trim(), 1000);
            }
        }

        let mut meta = map_kuwo_song_detail(
            &id,
            target,
            h5_data.unwrap_or_default(),
            album_detail,
            fallback_cover,
        );
        meta.external_id = external_id.trim().to_string();

        if (meta.synced_lyrics.is_none() || meta.title.is_none())
            && let Some(nl) = fetch_kuwo_newlyric_fallback(&client, &id).await
        {
            if meta.title.is_none() {
                meta.title = nl.ti;
            }
            if meta.creators.is_empty()
                && let Some(ar) = nl.ar
            {
                meta.creators = split_music_artists(&ar);
            }
            if meta.series_or_album.is_none() {
                meta.series_or_album = nl.al;
            }
            if meta.synced_lyrics.is_none() {
                meta.synced_lyrics = nl.synced_lyrics;
            }
            if meta.plain_lyrics.is_none() {
                meta.plain_lyrics = nl.plain_lyrics;
            }
        }

        if meta.title.is_none() && meta.synced_lyrics.is_none() {
            return Err(DomainError::NotFound(format!(
                "酷我音乐 song '{id}' not found"
            )));
        }

        Ok(meta)
    }
}

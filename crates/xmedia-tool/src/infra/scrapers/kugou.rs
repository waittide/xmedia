use async_trait::async_trait;
use serde::Deserialize;
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
    build_browser_http_client, extract_numeric_id_from_query, normalize_cover_url,
    parse_year_and_date, strip_html_to_text, strip_inline_html,
};

pub struct KugouMusicProvider;

const KG_SONG_SEARCH_URL: &str = "https://mobilecdn.kugou.com/api/v3/search/song";
const KG_ALBUM_SEARCH_URL: &str = "https://mobilecdn.kugou.com/api/v3/search/album";
const KG_ALBUM_INFO_URL: &str = "https://mobilecdn.kugou.com/api/v3/album/info";
const KG_ALBUM_SONG_URL: &str = "https://mobilecdn.kugou.com/api/v3/album/song";
const KG_SONG_INFO_URL: &str = "https://m.kugou.com/app/i/getSongInfo.php";
const KG_LYRIC_SEARCH_URL: &str = "https://krcs.kugou.com/search";
const KG_LYRIC_DOWNLOAD_URL: &str = "https://lyrics.kugou.com/download";

#[derive(Debug, Deserialize, Default)]
pub(crate) struct KgSongSearchItem {
    #[serde(default)]
    pub hash: String,
    #[serde(default)]
    pub songname: String,
    #[serde(default)]
    pub singername: String,
    #[serde(default)]
    pub album_name: String,
    #[serde(default)]
    pub album_id: serde_json::Value,
    #[serde(default)]
    pub duration: i64,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct KgAlbumItem {
    #[serde(default)]
    pub albumid: serde_json::Value,
    #[serde(default)]
    pub albumname: String,
    #[serde(default)]
    pub singername: String,
    #[serde(default)]
    pub publishtime: String,
    #[serde(default)]
    pub imgurl: String,
    #[serde(default)]
    pub company: Option<String>,
    #[serde(default)]
    pub intro: Option<String>,
    #[serde(default)]
    pub songcount: Option<i64>,
}

#[derive(Debug, Deserialize, Default)]
struct KgListData<T> {
    #[serde(default)]
    info: Vec<T>,
}

#[derive(Debug, Deserialize, Default)]
struct KgListEnvelope<T> {
    #[serde(default)]
    data: Option<KgListData<T>>,
}

#[derive(Debug, Deserialize, Default)]
struct KgAlbumDetailEnvelope {
    #[serde(default)]
    data: Option<KgAlbumItem>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct KgPlaySongInfo {
    #[serde(default)]
    pub song_name: String,
    #[serde(default)]
    pub singer_name: String,
    #[serde(default, rename = "album_img")]
    pub album_img: String,
    #[serde(default)]
    pub img_url: String,
    #[serde(default)]
    pub albumid: serde_json::Value,
}

#[derive(Debug, Deserialize, Default)]
struct KgAlbumTrackRef {
    #[serde(default)]
    hash: String,
    #[serde(default, rename = "320hash")]
    hash_320: String,
    #[serde(default)]
    sqhash: String,
}

#[derive(Debug, Deserialize, Default)]
struct KgLyricCandidate {
    #[serde(default)]
    id: serde_json::Value,
    #[serde(default)]
    accesskey: String,
}

#[derive(Debug, Deserialize, Default)]
struct KgLyricSearchResp {
    #[serde(default)]
    candidates: Vec<KgLyricCandidate>,
}

#[derive(Debug, Deserialize, Default)]
struct KgLyricDownloadResp {
    #[serde(default)]
    content: String,
}

fn json_val_to_id(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::String(s) => {
            let t = s.trim();
            (!t.is_empty() && t != "0").then(|| t.to_string())
        }
        serde_json::Value::Number(n) => {
            let s = n.to_string();
            (s != "0").then_some(s)
        }
        _ => None,
    }
}

pub(crate) fn upgrade_kugou_cover(raw: &str, size: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let replaced = trimmed.replace("{size}", size);
    normalize_cover_url(&replaced)
}

fn parse_kugou_external_id(raw: &str) -> (String, Option<String>) {
    let trimmed = raw.trim();
    if let Some((h, a)) = trimmed.split_once(':') {
        let alb = (!a.trim().is_empty() && a.trim() != "0").then(|| a.trim().to_string());
        (h.trim().to_string(), alb)
    } else {
        (trimmed.to_string(), None)
    }
}

fn extract_kugou_direct_hash(raw: &str) -> Option<String> {
    let t = raw.trim();
    if t.len() == 32 && t.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Some(t.to_ascii_lowercase());
    }
    if let Some(idx) = t.to_ascii_lowercase().find("hash=") {
        let after = &t[idx + 5..];
        let hex_part: String = after
            .chars()
            .take_while(|c| c.is_ascii_hexdigit())
            .collect();
        if hex_part.len() == 32 {
            return Some(hex_part.to_ascii_lowercase());
        }
    }
    None
}

pub(crate) fn map_kugou_album_detail(alb: KgAlbumItem, external_id: &str) -> ScrapedMetadata {
    let title = {
        let t = strip_inline_html(&unescape_music_text(&alb.albumname));
        (!t.is_empty()).then_some(t)
    };
    let creators = split_music_artists(&alb.singername);
    let (year, published_date) = parse_year_and_date(&alb.publishtime);
    let publisher = alb
        .company
        .as_deref()
        .map(|c| strip_inline_html(&unescape_music_text(c)))
        .filter(|s| !s.is_empty());
    let summary = alb
        .intro
        .as_deref()
        .map(|s| s.trim().trim_matches('"'))
        .map(strip_html_to_text)
        .filter(|s| !s.is_empty());
    let cover_url = upgrade_kugou_cover(&alb.imgurl, "1000");
    let resolved_id = json_val_to_id(&alb.albumid).unwrap_or_else(|| external_id.to_string());

    ScrapedMetadata {
        provider_id: "kugou".to_string(),
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

pub(crate) fn map_kugou_song_detail(
    external_id: &str,
    target: ScrapeTargetKind,
    play_info: KgPlaySongInfo,
    album_info: Option<KgAlbumItem>,
    track_number: Option<u32>,
    lrc_content: Option<&str>,
) -> ScrapedMetadata {
    let title = {
        let t = strip_inline_html(&unescape_music_text(&play_info.song_name));
        (!t.is_empty()).then_some(t)
    };
    let mut creators = split_music_artists(&play_info.singer_name);
    if creators.is_empty()
        && let Some(ref alb) = album_info
    {
        creators = split_music_artists(&alb.singername);
    }

    let series_or_album = album_info
        .as_ref()
        .map(|a| strip_inline_html(&unescape_music_text(&a.albumname)))
        .filter(|s| !s.is_empty());
    let (year, published_date) = album_info
        .as_ref()
        .map(|a| parse_year_and_date(&a.publishtime))
        .unwrap_or((None, None));
    let publisher = album_info
        .as_ref()
        .and_then(|a| a.company.as_deref())
        .map(|c| strip_inline_html(&unescape_music_text(c)))
        .filter(|s| !s.is_empty());

    let cover_url = album_info
        .as_ref()
        .and_then(|a| upgrade_kugou_cover(&a.imgurl, "1000"))
        .or_else(|| upgrade_kugou_cover(&play_info.album_img, "1000"))
        .or_else(|| upgrade_kugou_cover(&play_info.img_url, "400"));

    let (synced_lyrics, plain_lyrics) = lrc_content
        .map(|raw| normalize_lrc_and_extract_plain(raw, None))
        .unwrap_or((None, None));

    ScrapedMetadata {
        provider_id: "kugou".to_string(),
        external_id: external_id.to_string(),
        target: Some(target),
        title,
        series_or_album,
        creators,
        publisher_or_studio: publisher,
        year,
        published_date,
        track_or_episode_number: track_number,
        cover_url,
        synced_lyrics,
        plain_lyrics,
        ..Default::default()
    }
}

async fn fetch_kugou_lyric_text(client: &reqwest::Client, hash: &str) -> Option<String> {
    let resp = client
        .get(KG_LYRIC_SEARCH_URL)
        .query(&[
            ("ver", "1"),
            ("man", "yes"),
            ("client", "mobi"),
            ("keyword", ""),
            ("duration", ""),
            ("hash", hash),
            ("album_audio_id", ""),
        ])
        .send()
        .await
        .ok()?;
    let search_data: KgLyricSearchResp = read_json_response(resp, "酷狗歌词检索").await.ok()?;
    let cand = search_data.candidates.into_iter().next()?;
    let cand_id = json_val_to_id(&cand.id)?;
    if cand.accesskey.trim().is_empty() {
        return None;
    }
    let dl_resp = client
        .get(KG_LYRIC_DOWNLOAD_URL)
        .query(&[
            ("ver", "1"),
            ("client", "pc"),
            ("id", cand_id.as_str()),
            ("accesskey", cand.accesskey.as_str()),
            ("fmt", "lrc"),
            ("charset", "utf8"),
        ])
        .send()
        .await
        .ok()?;
    let dl_data: KgLyricDownloadResp = read_json_response(dl_resp, "酷狗歌词下载").await.ok()?;
    decode_base64_utf8(&dl_data.content)
}

#[async_trait]
impl ScraperProviderPort for KugouMusicProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "kugou".to_string(),
            name: "酷狗音乐".to_string(),
            description: "酷狗音乐免登录公开曲库（提供华语海量单曲、专辑简介、1000px 专辑封面与精准时间轴 LRC 歌词）".to_string(),
            website: "https://www.kugou.com".to_string(),
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
            if let Some(alb_id) =
                extract_numeric_id_from_query(&query.keyword, &["album/info/", "album/"])
                && (query.keyword.contains("kugou.com") || query.keyword.trim().len() >= 5)
                && let Ok(detail) = self.fetch_detail(&alb_id, target, "zh-CN", creds).await
                && let Some(title) = detail.title
            {
                return Ok(vec![ScrapeCandidate {
                    provider_id: "kugou".to_string(),
                    provider_name: "酷狗音乐".to_string(),
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
        } else if let Some(hash) = extract_kugou_direct_hash(&query.keyword)
            && let Ok(detail) = self.fetch_detail(&hash, target, "zh-CN", creds).await
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
                provider_id: "kugou".to_string(),
                provider_name: "酷狗音乐".to_string(),
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

        if is_album {
            let resp = client
                .get(KG_ALBUM_SEARCH_URL)
                .query(&[
                    ("format", "json"),
                    ("keyword", search_kw.as_str()),
                    ("page", "1"),
                    ("pagesize", "15"),
                ])
                .send()
                .await
                .map_err(|e| DomainError::Storage(format!("酷狗音乐 album search failed: {e}")))?;
            let env: KgListEnvelope<KgAlbumItem> =
                read_json_response(resp, "酷狗音乐 album search").await?;
            let list = env.data.map(|d| d.info).unwrap_or_default();

            let mut scored: Vec<(i32, usize, ScrapeCandidate)> = Vec::new();
            for (idx, alb) in list.into_iter().enumerate() {
                let Some(alb_id) = json_val_to_id(&alb.albumid) else {
                    continue;
                };
                let title = strip_inline_html(&unescape_music_text(&alb.albumname));
                if title.is_empty() {
                    continue;
                }
                let artists = split_music_artists(&alb.singername);
                let (year, _) = parse_year_and_date(&alb.publishtime);
                let score = score_music_candidate(
                    query,
                    &title,
                    &artists,
                    Some(&title),
                    year,
                    None,
                    u32::try_from(alb.songcount.unwrap_or(0))
                        .unwrap_or(0)
                        .saturating_mul(15),
                );
                let mut sub = Vec::new();
                if !artists.is_empty() {
                    sub.push(artists.join(" / "));
                }
                if let Some(cnt) = alb.songcount.filter(|&c| c > 0) {
                    sub.push(format!("{cnt} 首"));
                }
                scored.push((
                    score,
                    idx,
                    ScrapeCandidate {
                        provider_id: "kugou".to_string(),
                        provider_name: "酷狗音乐".to_string(),
                        external_id: alb_id,
                        target,
                        title,
                        subtitle: (!sub.is_empty()).then(|| sub.join(" · ")),
                        year,
                        summary: None,
                        cover_url: upgrade_kugou_cover(&alb.imgurl, "400"),
                        synced_lyrics: None,
                        plain_lyrics: None,
                    },
                ));
            }
            scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
            return Ok(scored.into_iter().take(15).map(|(_, _, c)| c).collect());
        }

        let resp = client
            .get(KG_SONG_SEARCH_URL)
            .query(&[
                ("format", "json"),
                ("keyword", search_kw.as_str()),
                ("page", "1"),
                ("pagesize", "20"),
                ("showtype", "1"),
            ])
            .send()
            .await
            .map_err(|e| DomainError::Storage(format!("酷狗音乐 song search failed: {e}")))?;

        let env: KgListEnvelope<KgSongSearchItem> =
            read_json_response(resp, "酷狗音乐 song search").await?;
        let list = env.data.map(|d| d.info).unwrap_or_default();

        let mut scored: Vec<(i32, usize, ScrapeCandidate)> = Vec::new();
        for (idx, item) in list.into_iter().enumerate() {
            let hash = item.hash.trim().to_ascii_lowercase();
            if hash.is_empty() {
                continue;
            }
            let title = strip_inline_html(&unescape_music_text(&item.songname));
            if title.is_empty() {
                continue;
            }
            let artists = split_music_artists(&item.singername);
            let album_name = {
                let a = strip_inline_html(&unescape_music_text(&item.album_name));
                (!a.is_empty()).then_some(a)
            };
            let alb_id = json_val_to_id(&item.album_id);
            let ext_id = match &alb_id {
                Some(aid) => format!("{hash}:{aid}"),
                None => hash,
            };

            let dur_u32 = u32::try_from(item.duration).ok().filter(|&d| d > 0);
            let score = score_music_candidate(
                query,
                &title,
                &artists,
                album_name.as_deref(),
                None,
                dur_u32,
                0,
            );

            let mut sub = Vec::new();
            if !artists.is_empty() {
                sub.push(artists.join(" / "));
            }
            if let Some(ref a) = album_name {
                sub.push(a.clone());
            }
            if let Some(dur) = dur_u32.and_then(format_duration_mmss) {
                sub.push(dur);
            }

            scored.push((
                score,
                idx,
                ScrapeCandidate {
                    provider_id: "kugou".to_string(),
                    provider_name: "酷狗音乐".to_string(),
                    external_id: ext_id,
                    target,
                    title,
                    subtitle: (!sub.is_empty()).then(|| sub.join(" · ")),
                    year: None,
                    summary: None,
                    cover_url: None,
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
            let alb_id = external_id.trim();
            let resp = client
                .get(KG_ALBUM_INFO_URL)
                .query(&[("albumid", alb_id)])
                .send()
                .await
                .map_err(|e| DomainError::Storage(format!("酷狗音乐 album detail failed: {e}")))?;
            let env: KgAlbumDetailEnvelope =
                read_json_response(resp, "酷狗音乐 album detail").await?;
            let alb = env.data.ok_or_else(|| {
                DomainError::NotFound(format!("酷狗音乐 album '{alb_id}' not found"))
            })?;
            return Ok(map_kugou_album_detail(alb, alb_id));
        }

        let (hash, hinted_album_id) = parse_kugou_external_id(external_id);
        if hash.is_empty() {
            return Err(DomainError::Validation("empty 酷狗音乐 song hash".into()));
        }

        let play_fut = client
            .get(KG_SONG_INFO_URL)
            .header(reqwest::header::REFERER, "http://m.kugou.com/")
            .query(&[("cmd", "playInfo"), ("hash", hash.as_str())])
            .send();
        let lyric_fut = fetch_kugou_lyric_text(&client, &hash);

        let (play_res, lrc_text) = tokio::join!(play_fut, lyric_fut);

        let play_info: KgPlaySongInfo = match play_res {
            Ok(r) if r.status().is_success() => read_json_response(r, "酷狗音乐 playInfo")
                .await
                .unwrap_or_default(),
            _ => KgPlaySongInfo::default(),
        };

        let resolved_album_id = hinted_album_id.or_else(|| json_val_to_id(&play_info.albumid));

        let mut album_info: Option<KgAlbumItem> = None;
        let mut track_number: Option<u32> = None;

        if target != ScrapeTargetKind::Lyrics
            && let Some(ref aid) = resolved_album_id
        {
            let info_fut = client
                .get(KG_ALBUM_INFO_URL)
                .query(&[("albumid", aid.as_str())])
                .send();
            let tracks_fut = client
                .get(KG_ALBUM_SONG_URL)
                .query(&[
                    ("version", "9108"),
                    ("albumid", aid.as_str()),
                    ("plat", "0"),
                    ("pagesize", "100"),
                    ("area_code", "1"),
                    ("page", "1"),
                ])
                .send();

            let (info_res, tracks_res) = tokio::join!(info_fut, tracks_fut);
            if let Ok(r) = info_res
                && let Ok(env) = read_json_response::<KgAlbumDetailEnvelope>(r, "酷狗专辑").await
            {
                album_info = env.data;
            }
            if let Ok(r) = tracks_res
                && let Ok(env) =
                    read_json_response::<KgListEnvelope<KgAlbumTrackRef>>(r, "酷狗专辑曲目").await
                && let Some(data) = env.data
            {
                track_number = data
                    .info
                    .iter()
                    .position(|t| {
                        t.hash.eq_ignore_ascii_case(&hash)
                            || t.hash_320.eq_ignore_ascii_case(&hash)
                            || t.sqhash.eq_ignore_ascii_case(&hash)
                    })
                    .map(|idx| (idx + 1) as u32);
            }
        }

        if play_info.song_name.trim().is_empty() && lrc_text.is_none() && album_info.is_none() {
            return Err(DomainError::NotFound(format!(
                "酷狗音乐 song '{hash}' not found"
            )));
        }

        Ok(map_kugou_song_detail(
            external_id,
            target,
            play_info,
            album_info,
            track_number,
            lrc_text.as_deref(),
        ))
    }
}

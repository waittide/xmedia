use async_trait::async_trait;
use encoding_rs::UTF_8;
use serde::Deserialize;
use xmedia_core::domain::error::{DomainError, DomainResult};

use crate::domain::config::ProviderCredentials;
use crate::domain::ports::ScraperProviderPort;
use crate::domain::scraper::{
    ProviderDescriptor, ScrapeCandidate, ScrapeQuery, ScrapeTargetKind, ScrapedMetadata,
};
use crate::infra::scrapers::cn_music_common::{
    build_music_search_keyword, normalize_lrc_and_extract_plain, read_json_response,
    score_music_candidate, split_music_artists, unescape_music_text,
};
use crate::infra::scrapers::cn_novel_common::{
    build_browser_http_client, extract_numeric_id_from_query, normalize_cover_url,
    parse_year_and_date, read_response_text, strip_html_to_text, strip_inline_html,
};

pub struct MiguMusicProvider;

const MIGU_SEARCH_URL: &str = "https://pd.musicapp.migu.cn/MIGUM3.0/v1.0/content/search_all.do";
const MIGU_RESOURCE_URL: &str = "https://c.musicapp.migu.cn/MIGUM2.0/v1.0/content/resourceinfo.do";
const MIGU_UA: &str = "okhttp/3.12.0";

#[derive(Debug, Deserialize, Default, Clone)]
pub(crate) struct MiguNamedEntity {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MiguImgItem {
    #[serde(default)]
    pub img_size_type: String,
    #[serde(default)]
    pub img: String,
    #[serde(default)]
    pub webp_img: String,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MiguSearchSong {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub singers: Vec<MiguNamedEntity>,
    #[serde(default)]
    pub albums: Vec<MiguNamedEntity>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub img_items: Vec<MiguImgItem>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MiguSearchAlbum {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub singer: String,
    #[serde(default)]
    pub publish_date: String,
    #[serde(default)]
    pub img_items: Vec<MiguImgItem>,
}

#[derive(Debug, Deserialize, Default)]
struct MiguResultList<T> {
    #[serde(default)]
    result: Vec<T>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct MiguSearchEnvelope {
    #[serde(default)]
    code: String,
    #[serde(default)]
    song_result_data: Option<MiguResultList<MiguSearchSong>>,
    #[serde(default)]
    album_result_data: Option<MiguResultList<MiguSearchAlbum>>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MiguSongResource {
    #[serde(default)]
    pub song_name: String,
    #[serde(default)]
    pub singer: String,
    #[serde(default)]
    pub album_id: String,
    #[serde(default)]
    pub album: String,
    #[serde(default)]
    pub album_imgs: Vec<MiguImgItem>,
    #[serde(default)]
    pub lrc_url: String,
    #[serde(default)]
    pub track_number: Option<serde_json::Value>,
    #[serde(default)]
    pub disc: Option<serde_json::Value>,
    #[serde(default)]
    pub song_descs: String,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MiguAlbumResource {
    #[serde(default)]
    pub album_id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub singer: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub publish_date: String,
    #[serde(default)]
    pub publish_time: String,
    #[serde(default)]
    pub publish_company: String,
    #[serde(default)]
    pub publish_corp: String,
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub img_items: Vec<MiguImgItem>,
}

#[derive(Debug, Deserialize, Default)]
struct MiguResourceEnvelope<T> {
    #[serde(default)]
    resource: Vec<T>,
}

pub(crate) fn pick_best_migu_cover(items: &[MiguImgItem]) -> Option<String> {
    for pref in ["03", "02", "01"] {
        if let Some(found) = items.iter().find(|it| it.img_size_type.trim() == pref) {
            let raw = if !found.img.trim().is_empty() {
                &found.img
            } else {
                &found.webp_img
            };
            if let Some(u) = normalize_cover_url(raw) {
                return Some(u);
            }
        }
    }
    items.iter().find_map(|it| {
        let raw = if !it.img.trim().is_empty() {
            &it.img
        } else {
            &it.webp_img
        };
        normalize_cover_url(raw)
    })
}

fn parse_positive_u32(val: &Option<serde_json::Value>) -> Option<u32> {
    match val {
        Some(serde_json::Value::Number(n)) => n
            .as_u64()
            .and_then(|v| u32::try_from(v).ok())
            .filter(|&v| v > 0),
        Some(serde_json::Value::String(s)) => s.trim().parse::<u32>().ok().filter(|&v| v > 0),
        _ => None,
    }
}

fn parse_migu_external_id(raw: &str) -> (String, Option<String>) {
    let trimmed = raw.trim();
    if let Some((s, a)) = trimmed.split_once(':') {
        let alb = (!a.trim().is_empty() && a.trim() != "0").then(|| a.trim().to_string());
        (s.trim().to_string(), alb)
    } else {
        (trimmed.to_string(), None)
    }
}

pub(crate) fn map_migu_album_detail(alb: MiguAlbumResource, external_id: &str) -> ScrapedMetadata {
    let title = {
        let t = strip_inline_html(&unescape_music_text(&alb.title));
        (!t.is_empty()).then_some(t)
    };
    let creators = split_music_artists(&alb.singer);
    let date_raw = if !alb.publish_date.trim().is_empty() {
        &alb.publish_date
    } else {
        &alb.publish_time
    };
    let (year, published_date) = parse_year_and_date(date_raw);
    let comp_raw = if !alb.publish_company.trim().is_empty() {
        &alb.publish_company
    } else {
        &alb.publish_corp
    };
    let publisher = {
        let c = strip_inline_html(&unescape_music_text(comp_raw));
        (!c.is_empty()).then_some(c)
    };
    let language = {
        let l = strip_inline_html(&alb.language);
        (!l.is_empty()).then_some(l)
    };
    let summary = {
        let s = strip_html_to_text(&alb.summary);
        (!s.is_empty()).then_some(s)
    };
    let cover_url = pick_best_migu_cover(&alb.img_items);
    let resolved_id = if !alb.album_id.trim().is_empty() {
        alb.album_id
    } else {
        external_id.to_string()
    };

    ScrapedMetadata {
        provider_id: "migu".to_string(),
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

pub(crate) fn map_migu_song_detail(
    external_id: &str,
    target: ScrapeTargetKind,
    song: MiguSongResource,
    album_opt: Option<MiguAlbumResource>,
    raw_lrc: Option<&str>,
) -> ScrapedMetadata {
    let title = {
        let t = strip_inline_html(&unescape_music_text(&song.song_name));
        (!t.is_empty()).then_some(t)
    };
    let mut creators = split_music_artists(&song.singer);
    if creators.is_empty()
        && let Some(ref alb) = album_opt
    {
        creators = split_music_artists(&alb.singer);
    }

    let series_or_album = {
        let a = strip_inline_html(&unescape_music_text(&song.album));
        if !a.is_empty() {
            Some(a)
        } else {
            album_opt
                .as_ref()
                .map(|alb| strip_inline_html(&unescape_music_text(&alb.title)))
                .filter(|s| !s.is_empty())
        }
    };

    let (year, published_date) = album_opt
        .as_ref()
        .map(|alb| {
            let d = if !alb.publish_date.trim().is_empty() {
                &alb.publish_date
            } else {
                &alb.publish_time
            };
            parse_year_and_date(d)
        })
        .unwrap_or((None, None));

    let publisher = album_opt.as_ref().and_then(|alb| {
        let raw = if !alb.publish_company.trim().is_empty() {
            &alb.publish_company
        } else {
            &alb.publish_corp
        };
        let c = strip_inline_html(&unescape_music_text(raw));
        (!c.is_empty()).then_some(c)
    });

    let language = album_opt
        .as_ref()
        .map(|alb| strip_inline_html(&alb.language))
        .filter(|s| !s.is_empty());

    let track_no = parse_positive_u32(&song.track_number);
    let disc_no = parse_positive_u32(&song.disc);
    let summary = {
        let d = strip_html_to_text(&song.song_descs);
        (!d.is_empty()).then_some(d)
    };

    let cover_url = pick_best_migu_cover(&song.album_imgs).or_else(|| {
        album_opt
            .as_ref()
            .and_then(|alb| pick_best_migu_cover(&alb.img_items))
    });

    let (synced_lyrics, plain_lyrics) = raw_lrc
        .map(|l| normalize_lrc_and_extract_plain(l, None))
        .unwrap_or((None, None));

    ScrapedMetadata {
        provider_id: "migu".to_string(),
        external_id: external_id.to_string(),
        target: Some(target),
        title,
        series_or_album,
        creators,
        publisher_or_studio: publisher,
        year,
        published_date,
        language,
        track_or_episode_number: track_no,
        disc_or_season_number: disc_no,
        summary,
        cover_url,
        synced_lyrics,
        plain_lyrics,
        ..Default::default()
    }
}

#[async_trait]
impl ScraperProviderPort for MiguMusicProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "migu".to_string(),
            name: "咪咕音乐".to_string(),
            description: "中国移动咪咕音乐免登录公开曲库（擅长华语港台经典老唱片、原版专辑归属、高清 WebP 封面与标准 LRC 歌词）".to_string(),
            website: "https://music.migu.cn".to_string(),
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
            &["music/album/", "albumId="]
        } else {
            &["music/song/", "songId="]
        };
        if let Some(direct_id) = extract_numeric_id_from_query(&query.keyword, url_markers)
            && (query.keyword.contains("migu.cn") || query.keyword.trim().len() >= 6)
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
                provider_id: "migu".to_string(),
                provider_name: "咪咕音乐".to_string(),
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
        let switch_json = if is_album {
            r#"{"song":0,"album":1}"#
        } else {
            r#"{"song":1,"album":0}"#
        };

        let resp = client
            .get(MIGU_SEARCH_URL)
            .header(reqwest::header::USER_AGENT, MIGU_UA)
            .query(&[
                ("ua", "Android_migu"),
                ("version", "5.0.1"),
                ("text", search_kw.as_str()),
                ("pageNo", "1"),
                ("pageSize", "20"),
                ("searchSwitch", switch_json),
            ])
            .send()
            .await
            .map_err(|e| DomainError::Storage(format!("咪咕音乐 search failed: {e}")))?;

        let env: MiguSearchEnvelope = read_json_response(resp, "咪咕音乐 search").await?;
        if env.code != "000000" {
            return Ok(Vec::new());
        }

        if is_album {
            let list = env.album_result_data.map(|d| d.result).unwrap_or_default();
            let mut scored: Vec<(i32, usize, ScrapeCandidate)> = Vec::new();
            for (idx, alb) in list.into_iter().enumerate() {
                let id = alb.id.trim().to_string();
                if id.is_empty() {
                    continue;
                }
                let title = strip_inline_html(&unescape_music_text(&alb.name));
                if title.is_empty() {
                    continue;
                }
                let artists = split_music_artists(&alb.singer);
                let (year, _) = parse_year_and_date(&alb.publish_date);
                let score =
                    score_music_candidate(query, &title, &artists, Some(&title), year, None, 0);
                scored.push((
                    score,
                    idx,
                    ScrapeCandidate {
                        provider_id: "migu".to_string(),
                        provider_name: "咪咕音乐".to_string(),
                        external_id: id,
                        target,
                        title,
                        subtitle: (!artists.is_empty()).then(|| artists.join(" / ")),
                        year,
                        summary: None,
                        cover_url: pick_best_migu_cover(&alb.img_items),
                        synced_lyrics: None,
                        plain_lyrics: None,
                    },
                ));
            }
            scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
            return Ok(scored.into_iter().take(15).map(|(_, _, c)| c).collect());
        }

        let list = env.song_result_data.map(|d| d.result).unwrap_or_default();
        let mut scored: Vec<(i32, usize, ScrapeCandidate)> = Vec::new();
        for (idx, item) in list.into_iter().enumerate() {
            let sid = item.id.trim();
            if sid.is_empty() {
                continue;
            }
            let title = strip_inline_html(&unescape_music_text(&item.name));
            if title.is_empty() {
                continue;
            }
            let artists: Vec<String> = item
                .singers
                .iter()
                .map(|s| strip_inline_html(&unescape_music_text(&s.name)))
                .filter(|s| !s.is_empty())
                .collect();
            let first_album = item.albums.first();
            let album_name = first_album
                .map(|a| strip_inline_html(&unescape_music_text(&a.name)))
                .filter(|s| !s.is_empty());
            let ext_id = match first_album.map(|a| a.id.trim()).filter(|id| !id.is_empty()) {
                Some(aid) => format!("{sid}:{aid}"),
                None => sid.to_string(),
            };

            let score = score_music_candidate(
                query,
                &title,
                &artists,
                album_name.as_deref(),
                None,
                None,
                0,
            );

            let mut sub = Vec::new();
            if !artists.is_empty() {
                sub.push(artists.join(" / "));
            }
            if let Some(ref alb) = album_name {
                sub.push(alb.clone());
            }

            let tag_summary = (!item.tags.is_empty()).then(|| item.tags.join(" · "));

            scored.push((
                score,
                idx,
                ScrapeCandidate {
                    provider_id: "migu".to_string(),
                    provider_name: "咪咕音乐".to_string(),
                    external_id: ext_id,
                    target,
                    title,
                    subtitle: (!sub.is_empty()).then(|| sub.join(" · ")),
                    year: None,
                    summary: tag_summary,
                    cover_url: pick_best_migu_cover(&item.img_items),
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
                return Err(DomainError::Validation("empty 咪咕音乐 album ID".into()));
            }
            let resp = client
                .get(MIGU_RESOURCE_URL)
                .header(reqwest::header::USER_AGENT, MIGU_UA)
                .query(&[
                    ("needSimple", "00"),
                    ("resourceId", id),
                    ("resourceType", "2003"),
                ])
                .send()
                .await
                .map_err(|e| DomainError::Storage(format!("咪咕音乐 album detail failed: {e}")))?;
            let env: MiguResourceEnvelope<MiguAlbumResource> =
                read_json_response(resp, "咪咕音乐 album").await?;
            let alb =
                env.resource.into_iter().next().ok_or_else(|| {
                    DomainError::NotFound(format!("咪咕音乐 album '{id}' not found"))
                })?;
            return Ok(map_migu_album_detail(alb, id));
        }

        let (song_id, hinted_album_id) = parse_migu_external_id(external_id);
        if song_id.is_empty() {
            return Err(DomainError::Validation("empty 咪咕音乐 song ID".into()));
        }

        let resp = client
            .get(MIGU_RESOURCE_URL)
            .header(reqwest::header::USER_AGENT, MIGU_UA)
            .query(&[
                ("needSimple", "00"),
                ("resourceId", song_id.as_str()),
                ("resourceType", "2"),
            ])
            .send()
            .await
            .map_err(|e| DomainError::Storage(format!("咪咕音乐 song detail failed: {e}")))?;

        let env: MiguResourceEnvelope<MiguSongResource> =
            read_json_response(resp, "咪咕音乐 song").await?;
        let song =
            env.resource.into_iter().next().ok_or_else(|| {
                DomainError::NotFound(format!("咪咕音乐 song '{song_id}' not found"))
            })?;

        let resolved_album_id = if !song.album_id.trim().is_empty() && song.album_id.trim() != "0" {
            Some(song.album_id.trim().to_string())
        } else {
            hinted_album_id
        };

        let lrc_url_opt = normalize_cover_url(&song.lrc_url);

        let album_fut = async {
            if target == ScrapeTargetKind::Lyrics {
                return None;
            }
            let aid = resolved_album_id.as_deref()?;
            let r = client
                .get(MIGU_RESOURCE_URL)
                .header(reqwest::header::USER_AGENT, MIGU_UA)
                .query(&[
                    ("needSimple", "00"),
                    ("resourceId", aid),
                    ("resourceType", "2003"),
                ])
                .send()
                .await
                .ok()?;
            let alb_env: MiguResourceEnvelope<MiguAlbumResource> =
                read_json_response(r, "咪咕专辑补充").await.ok()?;
            alb_env.resource.into_iter().next()
        };

        let lrc_fut = async {
            let u = lrc_url_opt.as_deref()?;
            let r = client.get(u).send().await.ok()?;
            if !r.status().is_success() {
                return None;
            }
            read_response_text(r, UTF_8).await.ok()
        };

        let (album_opt, lrc_raw) = tokio::join!(album_fut, lrc_fut);

        Ok(map_migu_song_detail(
            external_id,
            target,
            song,
            album_opt,
            lrc_raw.as_deref(),
        ))
    }
}

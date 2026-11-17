use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use xmedia_core::domain::error::{DomainError, DomainResult};

use crate::domain::config::ProviderCredentials;
use crate::domain::ports::ScraperProviderPort;
use crate::domain::scraper::{
    ProviderDescriptor, ScrapeCandidate, ScrapeQuery, ScrapeTargetKind, ScrapedMetadata,
};
use crate::infra::scrapers::http_client::build_http_client;

pub struct BangumiProvider;

#[derive(Serialize)]
struct BgmSearchPayload<'a> {
    keyword: &'a str,
    filter: BgmSearchFilter,
}

#[derive(Serialize)]
struct BgmSearchFilter {
    #[serde(rename = "type")]
    subject_types: Vec<u8>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct BgmImages {
    #[serde(default)]
    pub large: Option<String>,
    #[serde(default)]
    pub common: Option<String>,
    #[serde(default)]
    pub medium: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct BgmRating {
    #[serde(default)]
    pub score: Option<f32>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct BgmTag {
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum BgmInfoboxValue {
    Text(String),
    Array(Vec<BgmInfoboxKv>),
}

#[derive(Debug, Deserialize)]
pub(crate) struct BgmInfoboxKv {
    pub v: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct BgmInfoboxItem {
    pub key: String,
    pub value: BgmInfoboxValue,
}

impl BgmInfoboxItem {
    fn values(&self) -> Vec<String> {
        match &self.value {
            BgmInfoboxValue::Text(s) => s
                .split(['、', '/', ',', '，'])
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(str::to_owned)
                .collect(),
            BgmInfoboxValue::Array(arr) => arr
                .iter()
                .map(|kv| kv.v.trim().to_string())
                .filter(|v| !v.is_empty())
                .collect(),
        }
    }

    fn first_raw(&self) -> Option<String> {
        match &self.value {
            BgmInfoboxValue::Text(s) => (!s.trim().is_empty()).then(|| s.trim().to_string()),
            BgmInfoboxValue::Array(arr) => arr
                .iter()
                .map(|kv| kv.v.trim())
                .find(|v| !v.is_empty())
                .map(str::to_owned),
        }
    }
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct BgmSubject {
    pub id: u64,
    #[serde(default, rename = "type")]
    pub subject_type: u8,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub name_cn: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default)]
    pub images: Option<BgmImages>,
    #[serde(default)]
    pub rating: Option<BgmRating>,
    #[serde(default)]
    pub tags: Vec<BgmTag>,
    #[serde(default)]
    pub infobox: Vec<BgmInfoboxItem>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct BgmSearchResp {
    #[serde(default)]
    pub data: Vec<BgmSubject>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(crate) struct BgmEpisodeItem {
    #[serde(default)]
    pub ep: Option<f64>,
    #[serde(default)]
    pub sort: f64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub name_cn: String,
    #[serde(default)]
    pub airdate: Option<String>,
    #[serde(default)]
    pub desc: String,
}

#[derive(Debug, Deserialize, Default)]
struct BgmEpisodesResp {
    #[serde(default)]
    data: Vec<BgmEpisodeItem>,
}

fn parse_bgm_external_id(raw: &str) -> (&str, Option<(u32, u32)>) {
    if let Some((base, se_part)) = raw.trim().split_once(":S")
        && let Some((s_str, e_str)) = se_part.split_once('E')
        && let (Ok(s), Ok(e)) = (s_str.parse::<u32>(), e_str.parse::<u32>())
    {
        return (base, Some((s, e)));
    }
    (raw.trim(), None)
}

fn subject_types_for_target(target: Option<ScrapeTargetKind>) -> Vec<u8> {
    match target {
        Some(ScrapeTargetKind::Book | ScrapeTargetKind::Comic) => vec![1],
        Some(ScrapeTargetKind::Movie | ScrapeTargetKind::TvShow) => vec![2, 6],
        Some(ScrapeTargetKind::MusicAlbum | ScrapeTargetKind::MusicTrack) => vec![3],
        _ => vec![1, 2, 3, 6],
    }
}

fn infer_target(subject_type: u8, requested: Option<ScrapeTargetKind>) -> ScrapeTargetKind {
    if let Some(req) = requested {
        return req;
    }
    match subject_type {
        1 => ScrapeTargetKind::Comic,
        3 => ScrapeTargetKind::MusicAlbum,
        _ => ScrapeTargetKind::TvShow,
    }
}

fn parse_year(date_opt: Option<&str>) -> Option<i32> {
    date_opt?
        .trim()
        .get(..4)?
        .parse::<i32>()
        .ok()
        .filter(|&y| y > 1800)
}

pub(crate) fn map_bgm_to_candidate(
    sub: BgmSubject,
    requested_target: Option<ScrapeTargetKind>,
    season_ep: Option<(u32, u32)>,
) -> ScrapeCandidate {
    let primary_title = if !sub.name_cn.trim().is_empty() {
        sub.name_cn.trim().to_string()
    } else {
        sub.name.trim().to_string()
    };
    let target = infer_target(sub.subject_type, requested_target);
    let mut subtitle_parts = Vec::new();
    if !sub.name.trim().is_empty() && sub.name.trim() != primary_title {
        subtitle_parts.push(sub.name.trim().to_string());
    }
    if target == ScrapeTargetKind::TvShow
        && let Some((s, e)) = season_ep
    {
        subtitle_parts.push(format!("S{s:02}E{e:02}"));
    }

    let external_id = match (target, season_ep) {
        (ScrapeTargetKind::TvShow, Some((s, e))) => format!("{}:S{s}E{e}", sub.id),
        _ => sub.id.to_string(),
    };
    let cover_url = sub.images.and_then(|img| {
        img.large
            .or(img.common)
            .or(img.medium)
            .filter(|u| !u.trim().is_empty())
    });
    let year = parse_year(sub.date.as_deref());

    ScrapeCandidate {
        provider_id: "bangumi".to_string(),
        provider_name: "Bangumi 番组计划".to_string(),
        external_id,
        target,
        title: primary_title,
        subtitle: (!subtitle_parts.is_empty()).then(|| subtitle_parts.join(" · ")),
        year,
        summary: (!sub.summary.trim().is_empty()).then(|| sub.summary.trim().to_string()),
        cover_url,
        synced_lyrics: None,
        plain_lyrics: None,
    }
}

pub(crate) fn map_bgm_detail(
    sub: BgmSubject,
    target: ScrapeTargetKind,
    season_ep: Option<(u32, u32)>,
    episode: Option<BgmEpisodeItem>,
) -> ScrapedMetadata {
    let subject_title = if !sub.name_cn.trim().is_empty() {
        Some(sub.name_cn.trim().to_string())
    } else {
        (!sub.name.trim().is_empty()).then(|| sub.name.trim().to_string())
    };
    let original_title = (!sub.name.trim().is_empty()).then(|| sub.name.trim().to_string());
    let subject_summary = (!sub.summary.trim().is_empty()).then(|| sub.summary.trim().to_string());

    let mut creators = Vec::new();
    let mut illustrators_or_directors = Vec::new();
    let mut publisher_or_studio: Option<String> = None;
    let mut isbn: Option<String> = None;

    for item in &sub.infobox {
        match item.key.trim() {
            "作者" | "原作" | "艺术家" | "作曲" | "演唱" | "脚本" => {
                for v in item.values() {
                    if !creators.contains(&v) {
                        creators.push(v);
                    }
                }
            }
            "作画" | "插图" | "人物原案" | "导演" | "监督" | "总导演" => {
                for v in item.values() {
                    if !illustrators_or_directors.contains(&v) {
                        illustrators_or_directors.push(v);
                    }
                }
            }
            "出版社" | "连载杂志" | "动画制作" | "制作" | "厂牌"
                if publisher_or_studio.is_none() =>
            {
                publisher_or_studio = item.first_raw();
            }
            "ISBN" | "isbn" if isbn.is_none() => {
                isbn = item.first_raw();
            }
            _ => {}
        }
    }

    let tags: Vec<String> = sub
        .tags
        .into_iter()
        .take(10)
        .map(|t| t.name)
        .filter(|n| !n.trim().is_empty())
        .collect();
    let genres: Vec<String> = tags.iter().take(3).cloned().collect();
    let cover_url = sub.images.and_then(|img| {
        img.large
            .or(img.common)
            .or(img.medium)
            .filter(|u| !u.trim().is_empty())
    });
    let year = parse_year(sub.date.as_deref());

    let (final_title, final_summary, pub_date) = if target == ScrapeTargetKind::TvShow
        && let Some(ep) = episode
    {
        let ep_title = if !ep.name_cn.trim().is_empty() {
            Some(ep.name_cn.trim().to_string())
        } else if !ep.name.trim().is_empty() {
            Some(ep.name.trim().to_string())
        } else {
            subject_title.clone()
        };
        let ep_desc = (!ep.desc.trim().is_empty())
            .then(|| ep.desc.trim().to_string())
            .or_else(|| subject_summary.clone());
        let ep_air = ep
            .airdate
            .filter(|d| !d.trim().is_empty())
            .or_else(|| sub.date.clone());
        (ep_title, ep_desc, ep_air)
    } else {
        (subject_title.clone(), subject_summary.clone(), sub.date)
    };

    let season_num =
        (target == ScrapeTargetKind::TvShow).then(|| season_ep.map(|(s, _)| s).unwrap_or(1));
    let ep_num = (target == ScrapeTargetKind::TvShow)
        .then(|| season_ep.map(|(_, e)| e))
        .flatten();

    ScrapedMetadata {
        provider_id: "bangumi".to_string(),
        external_id: sub.id.to_string(),
        target: Some(target),
        title: final_title,
        original_title,
        series_or_album: matches!(
            target,
            ScrapeTargetKind::Comic | ScrapeTargetKind::TvShow | ScrapeTargetKind::MusicAlbum
        )
        .then_some(subject_title)
        .flatten(),
        creators,
        illustrators_or_directors,
        publisher_or_studio,
        year,
        published_date: pub_date,
        language: Some("zh".to_string()),
        isbn,
        rating: sub.rating.and_then(|r| r.score),
        track_or_episode_number: ep_num,
        disc_or_season_number: season_num,
        summary: final_summary,
        show_summary: (target == ScrapeTargetKind::TvShow)
            .then(|| subject_summary.clone())
            .flatten(),
        season_title: season_num.map(|s| format!("第 {s} 季")),
        season_summary: (target == ScrapeTargetKind::TvShow)
            .then_some(subject_summary)
            .flatten(),
        season_poster_url: (target == ScrapeTargetKind::TvShow)
            .then(|| cover_url.clone())
            .flatten(),
        genres,
        tags,
        cover_url,
        ..Default::default()
    }
}

#[async_trait]
impl ScraperProviderPort for BangumiProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "bangumi".to_string(),
            name: "Bangumi 番组计划".to_string(),
            description: "中文 ACGN 权威资料库（支持漫画、轻小说/图书、番剧动画与音乐专辑刮削；免密钥可用，可选配置 Access Token）".to_string(),
            website: "https://bgm.tv".to_string(),
            supported_targets: vec![
                ScrapeTargetKind::Comic,
                ScrapeTargetKind::Book,
                ScrapeTargetKind::TvShow,
                ScrapeTargetKind::Movie,
                ScrapeTargetKind::MusicAlbum,
            ],
            requires_api_key: false,
        }
    }

    async fn search(
        &self,
        query: &ScrapeQuery,
        creds: &ProviderCredentials,
    ) -> DomainResult<Vec<ScrapeCandidate>> {
        let client = build_http_client(creds)?;
        let payload = BgmSearchPayload {
            keyword: query.keyword.trim(),
            filter: BgmSearchFilter {
                subject_types: subject_types_for_target(query.target),
            },
        };

        let mut req = client
            .post("https://api.bgm.tv/v0/search/subjects?limit=12")
            .json(&payload);
        if let Some(tok) = creds
            .bangumi_access_token
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
        {
            req = req.bearer_auth(tok);
        }

        let resp = req
            .send()
            .await
            .map_err(|e| DomainError::Storage(format!("Bangumi search failed: {e}")))?;
        if !resp.status().is_success() {
            return Ok(Vec::new());
        }
        let body: BgmSearchResp = resp.json().await.unwrap_or_default();
        let season_ep = match (query.season, query.episode) {
            (Some(s), Some(e)) => Some((s, e)),
            (None, Some(e)) => Some((1, e)),
            (Some(s), None) => Some((s, 1)),
            _ => None,
        };
        Ok(body
            .data
            .into_iter()
            .map(|s| map_bgm_to_candidate(s, query.target, season_ep))
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
        let (base_id, season_ep) = parse_bgm_external_id(external_id);
        let tok_opt = creds
            .bangumi_access_token
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty());

        let url = format!("https://api.bgm.tv/v0/subjects/{base_id}");
        let mut req = client.get(&url);
        if let Some(tok) = tok_opt {
            req = req.bearer_auth(tok);
        }

        let resp = req
            .send()
            .await
            .map_err(|e| DomainError::Storage(format!("Bangumi detail failed: {e}")))?;
        if !resp.status().is_success() {
            return Err(DomainError::NotFound(format!(
                "Bangumi subject '{base_id}' not found (HTTP {})",
                resp.status()
            )));
        }
        let sub: BgmSubject = resp
            .json()
            .await
            .map_err(|e| DomainError::Storage(format!("Bangumi JSON error: {e}")))?;

        let mut matched_ep: Option<BgmEpisodeItem> = None;
        if target == ScrapeTargetKind::TvShow
            && let Some((_, ep_num)) = season_ep
        {
            let offset = ep_num.saturating_sub(1) / 100 * 100;
            let ep_url = format!(
                "https://api.bgm.tv/v0/episodes?subject_id={base_id}&type=0&limit=100&offset={offset}"
            );
            let mut ep_req = client.get(&ep_url);
            if let Some(tok) = tok_opt {
                ep_req = ep_req.bearer_auth(tok);
            }
            if let Ok(ep_resp) = ep_req.send().await
                && ep_resp.status().is_success()
                && let Ok(ep_list) = ep_resp.json::<BgmEpisodesResp>().await
            {
                let want = f64::from(ep_num);
                matched_ep = ep_list.data.into_iter().find(|item| {
                    item.ep.is_some_and(|v| (v - want).abs() < f64::EPSILON)
                        || (item.sort - want).abs() < f64::EPSILON
                });
            }
        }

        Ok(map_bgm_detail(sub, target, season_ep, matched_ep))
    }
}

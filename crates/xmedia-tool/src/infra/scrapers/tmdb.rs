use async_trait::async_trait;
use reqwest::RequestBuilder;
use serde::Deserialize;
use xmedia_core::domain::error::{DomainError, DomainResult};

use crate::domain::config::ProviderCredentials;
use crate::domain::ports::ScraperProviderPort;
use crate::domain::scraper::{
    ProviderDescriptor, ScrapeCandidate, ScrapeQuery, ScrapeTargetKind, ScrapedMetadata,
    score_candidate_relevance,
};
use crate::infra::scrapers::http_client::build_http_client;

pub struct TmdbProvider;

#[derive(Debug, Deserialize, Default)]
pub(crate) struct TmdbSearchHit {
    pub id: u64,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub original_title: Option<String>,
    #[serde(default)]
    pub original_name: Option<String>,
    #[serde(default)]
    pub overview: Option<String>,
    #[serde(default)]
    pub release_date: Option<String>,
    #[serde(default)]
    pub first_air_date: Option<String>,
    #[serde(default)]
    pub poster_path: Option<String>,
    #[serde(default)]
    pub vote_count: u32,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct TmdbSearchResponse {
    #[serde(default)]
    pub results: Vec<TmdbSearchHit>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct TmdbNamed {
    #[serde(default)]
    name: String,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct TmdbCrewMember {
    #[serde(default)]
    name: String,
    #[serde(default)]
    job: String,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct TmdbCastMember {
    #[serde(default)]
    name: String,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct TmdbCredits {
    #[serde(default)]
    cast: Vec<TmdbCastMember>,
    #[serde(default)]
    crew: Vec<TmdbCrewMember>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct TmdbDetailBody {
    pub id: u64,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub original_title: Option<String>,
    #[serde(default)]
    pub original_name: Option<String>,
    #[serde(default)]
    pub overview: Option<String>,
    #[serde(default)]
    pub release_date: Option<String>,
    #[serde(default)]
    pub first_air_date: Option<String>,
    #[serde(default)]
    pub vote_average: Option<f32>,
    #[serde(default)]
    pub poster_path: Option<String>,
    #[serde(default)]
    pub backdrop_path: Option<String>,
    #[serde(default)]
    pub genres: Vec<TmdbNamed>,
    #[serde(default)]
    pub production_companies: Vec<TmdbNamed>,
    #[serde(default)]
    pub networks: Vec<TmdbNamed>,
    #[serde(default)]
    pub created_by: Vec<TmdbNamed>,
    #[serde(default)]
    pub credits: Option<TmdbCredits>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(crate) struct TmdbEpisodeBody {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub overview: Option<String>,
    #[serde(default)]
    pub air_date: Option<String>,
    #[serde(default)]
    pub vote_average: Option<f32>,
    #[serde(default)]
    pub season_number: Option<u32>,
    #[serde(default)]
    pub episode_number: Option<u32>,
    #[serde(default)]
    pub still_path: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(crate) struct TmdbSeasonBody {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub overview: Option<String>,
    #[serde(default)]
    pub air_date: Option<String>,
    #[serde(default)]
    pub poster_path: Option<String>,
    #[serde(default)]
    pub season_number: Option<u32>,
    #[serde(default)]
    pub episodes: Vec<TmdbEpisodeBody>,
}

fn attach_tmdb_auth(req: RequestBuilder, raw_key: &str) -> RequestBuilder {
    let key = raw_key.trim();
    if key.starts_with("eyJ") || key.contains('.') {
        req.bearer_auth(key)
    } else {
        req.query(&[("api_key", key)])
    }
}

fn poster_url(path: Option<&str>) -> Option<String> {
    let p = path?.trim();
    (!p.is_empty()).then(|| format!("https://image.tmdb.org/t/p/w780{p}"))
}

fn backdrop_url(path: Option<&str>) -> Option<String> {
    let p = path?.trim();
    (!p.is_empty()).then(|| format!("https://image.tmdb.org/t/p/w1280{p}"))
}

fn parse_year(date_opt: Option<&str>) -> Option<i32> {
    date_opt?
        .trim()
        .get(..4)?
        .parse::<i32>()
        .ok()
        .filter(|&y| y > 1880)
}

/// Parses `"209867:S1E3"` -> `("209867", Some((1, 3)))` or `"209867"` -> `("209867", None)`.
pub(crate) fn parse_tmdb_external_id(raw: &str) -> (&str, Option<(u32, u32)>) {
    if let Some((base, se_part)) = raw.trim().split_once(":S")
        && let Some((s_str, e_str)) = se_part.split_once('E')
        && let (Ok(s), Ok(e)) = (s_str.parse::<u32>(), e_str.parse::<u32>())
    {
        return (base, Some((s, e)));
    }
    (raw.trim(), None)
}

pub(crate) fn map_tmdb_hit_to_candidate(
    hit: TmdbSearchHit,
    target: ScrapeTargetKind,
    season_ep: Option<(u32, u32)>,
) -> ScrapeCandidate {
    let title = hit
        .title
        .or(hit.name)
        .unwrap_or_else(|| format!("TMDB #{}", hit.id));
    let orig = hit.original_title.or(hit.original_name);
    let mut subtitle_parts = Vec::new();
    if let Some(o) = orig.filter(|o| !o.trim().is_empty() && o.trim() != title) {
        subtitle_parts.push(o);
    }
    if let Some((s, e)) = season_ep
        && target == ScrapeTargetKind::TvShow
    {
        subtitle_parts.push(format!("S{s:02}E{e:02}"));
    }

    let external_id = match (target, season_ep) {
        (ScrapeTargetKind::TvShow, Some((s, e))) => format!("{}:S{s}E{e}", hit.id),
        _ => hit.id.to_string(),
    };
    let date_str = hit
        .release_date
        .as_deref()
        .or(hit.first_air_date.as_deref());

    ScrapeCandidate {
        provider_id: "tmdb".to_string(),
        provider_name: "TMDB".to_string(),
        external_id,
        target,
        title,
        subtitle: (!subtitle_parts.is_empty()).then(|| subtitle_parts.join(" · ")),
        year: parse_year(date_str),
        summary: hit.overview.filter(|s| !s.trim().is_empty()),
        cover_url: poster_url(hit.poster_path.as_deref()),
        synced_lyrics: None,
        plain_lyrics: None,
    }
}

pub(crate) fn map_tmdb_detail(
    detail: TmdbDetailBody,
    target: ScrapeTargetKind,
    season: Option<TmdbSeasonBody>,
    episode: Option<TmdbEpisodeBody>,
) -> ScrapedMetadata {
    let show_or_movie_title = detail.title.or(detail.name);
    let original_title = detail.original_title.or(detail.original_name);
    let date_raw = detail
        .release_date
        .or(detail.first_air_date)
        .or_else(|| season.as_ref().and_then(|s| s.air_date.clone()));
    let show_overview = detail.overview.filter(|s| !s.trim().is_empty());

    let mut directors: Vec<String> = detail
        .created_by
        .into_iter()
        .map(|c| c.name)
        .filter(|n| !n.is_empty())
        .collect();
    let mut cast = Vec::new();

    if let Some(credits) = detail.credits {
        for member in credits.crew {
            if member.job.eq_ignore_ascii_case("Director") && !directors.contains(&member.name) {
                directors.push(member.name);
            }
        }
        for actor in credits.cast.into_iter().take(12) {
            if !actor.name.is_empty() && !cast.contains(&actor.name) {
                cast.push(actor.name);
            }
        }
    }

    let studio = detail
        .networks
        .into_iter()
        .chain(detail.production_companies)
        .map(|c| c.name)
        .find(|n| !n.trim().is_empty());

    let genres: Vec<String> = detail
        .genres
        .into_iter()
        .map(|g| g.name)
        .filter(|n| !n.trim().is_empty())
        .collect();

    let season_title = season
        .as_ref()
        .and_then(|s| s.name.clone())
        .filter(|s| !s.trim().is_empty());
    let season_summary = season
        .as_ref()
        .and_then(|s| s.overview.clone())
        .filter(|s| !s.trim().is_empty());
    let season_poster_url = season
        .as_ref()
        .and_then(|s| poster_url(s.poster_path.as_deref()));
    let season_num_from_body = season.as_ref().and_then(|s| s.season_number);

    let (final_title, series_title, season_num, ep_num, summary, rating, pub_date, ep_still_url) =
        if let Some(ep) = episode {
            let still = poster_url(ep.still_path.as_deref());
            (
                ep.name
                    .filter(|s| !s.trim().is_empty())
                    .or_else(|| show_or_movie_title.clone()),
                show_or_movie_title,
                ep.season_number.or(season_num_from_body),
                ep.episode_number,
                ep.overview
                    .filter(|s| !s.trim().is_empty())
                    .or_else(|| show_overview.clone()),
                ep.vote_average.filter(|&v| v > 0.0).or(detail.vote_average),
                ep.air_date.or_else(|| date_raw.clone()),
                still,
            )
        } else {
            let s_title = (target == ScrapeTargetKind::TvShow)
                .then(|| show_or_movie_title.clone())
                .flatten();
            (
                show_or_movie_title,
                s_title,
                season_num_from_body,
                None,
                show_overview.clone(),
                detail.vote_average.filter(|&v| v > 0.0),
                date_raw.clone(),
                None,
            )
        };

    // Keep the series premiere year for TV shows so episodes across calendar years share one series folder
    let year = if target == ScrapeTargetKind::TvShow {
        parse_year(date_raw.as_deref()).or_else(|| parse_year(pub_date.as_deref()))
    } else {
        parse_year(pub_date.as_deref())
    };

    ScrapedMetadata {
        provider_id: "tmdb".to_string(),
        external_id: detail.id.to_string(),
        target: Some(target),
        title: final_title,
        original_title,
        series_or_album: series_title,
        illustrators_or_directors: directors,
        cast,
        publisher_or_studio: studio,
        year,
        published_date: pub_date,
        rating,
        track_or_episode_number: ep_num,
        disc_or_season_number: season_num,
        summary,
        show_summary: (target == ScrapeTargetKind::TvShow)
            .then_some(show_overview)
            .flatten(),
        season_title,
        season_summary,
        season_poster_url,
        episode_still_url: ep_still_url,
        genres,
        cover_url: poster_url(detail.poster_path.as_deref()),
        backdrop_url: backdrop_url(detail.backdrop_path.as_deref()),
        ..Default::default()
    }
}

#[async_trait]
impl ScraperProviderPort for TmdbProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "tmdb".to_string(),
            name: "TMDB".to_string(),
            description: "全球电影与剧集权威数据库（支持电影、剧集单集 SxxExx、演职员表、海报与背景图；需在设置中填写 API Key 或 Read Token）".to_string(),
            website: "https://www.themoviedb.org".to_string(),
            supported_targets: vec![ScrapeTargetKind::Movie, ScrapeTargetKind::TvShow],
            requires_api_key: true,
        }
    }

    fn is_configured(&self, creds: &ProviderCredentials) -> bool {
        creds.has_tmdb_key()
    }

    async fn search(
        &self,
        query: &ScrapeQuery,
        creds: &ProviderCredentials,
    ) -> DomainResult<Vec<ScrapeCandidate>> {
        let Some(api_key) = creds
            .tmdb_api_key
            .as_deref()
            .filter(|k| !k.trim().is_empty())
        else {
            return Ok(Vec::new());
        };
        let client = build_http_client(creds)?;
        let target = match query.target {
            Some(ScrapeTargetKind::Movie) => ScrapeTargetKind::Movie,
            Some(ScrapeTargetKind::TvShow) => ScrapeTargetKind::TvShow,
            _ if query.season.is_some() || query.episode.is_some() => ScrapeTargetKind::TvShow,
            _ => ScrapeTargetKind::Movie,
        };
        let endpoint = if target == ScrapeTargetKind::TvShow {
            "https://api.themoviedb.org/3/search/tv"
        } else {
            "https://api.themoviedb.org/3/search/movie"
        };
        let lang = query.locale.as_deref().unwrap_or("zh-CN");

        let mut req = attach_tmdb_auth(client.get(endpoint), api_key)
            .query(&[("query", query.keyword.trim()), ("language", lang)]);
        if let Some(y) = query.year {
            let ys = y.to_string();
            let year_param = if target == ScrapeTargetKind::TvShow {
                "first_air_date_year"
            } else {
                "year"
            };
            req = req.query(&[(year_param, ys.as_str())]);
        }

        let resp = req
            .send()
            .await
            .map_err(|e| DomainError::Storage(format!("TMDB search failed: {e}")))?;
        if !resp.status().is_success() {
            return Err(DomainError::Storage(format!(
                "TMDB search returned HTTP {}",
                resp.status()
            )));
        }

        let body: TmdbSearchResponse = resp.json().await.unwrap_or_default();
        let season_ep = match (query.season, query.episode) {
            (Some(s), Some(e)) => Some((s, e)),
            (None, Some(e)) => Some((1, e)),
            (Some(s), None) => Some((s, 1)),
            _ => None,
        };
        let mut scored: Vec<(i32, usize, ScrapeCandidate)> = body
            .results
            .into_iter()
            .take(15)
            .enumerate()
            .map(|(idx, h)| {
                let votes = h.vote_count;
                let raw_orig = h
                    .original_title
                    .as_deref()
                    .or(h.original_name.as_deref())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string);
                let cand = map_tmdb_hit_to_candidate(h, target, season_ep);
                let rel = score_candidate_relevance(
                    &query.keyword,
                    query.year,
                    &cand.title,
                    raw_orig.as_deref(),
                    cand.year,
                    votes,
                );
                (rel, idx, cand)
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        Ok(scored.into_iter().map(|(_, _, c)| c).collect())
    }

    async fn fetch_detail(
        &self,
        external_id: &str,
        target: ScrapeTargetKind,
        locale: &str,
        creds: &ProviderCredentials,
    ) -> DomainResult<ScrapedMetadata> {
        let api_key = creds
            .tmdb_api_key
            .as_deref()
            .filter(|k| !k.trim().is_empty())
            .ok_or_else(|| DomainError::Validation("TMDB API Key is not configured".into()))?;
        let client = build_http_client(creds)?;
        let (base_id, season_ep) = parse_tmdb_external_id(external_id);

        let url = if target == ScrapeTargetKind::TvShow {
            format!("https://api.themoviedb.org/3/tv/{base_id}")
        } else {
            format!("https://api.themoviedb.org/3/movie/{base_id}")
        };

        let resp = attach_tmdb_auth(client.get(&url), api_key)
            .query(&[("language", locale), ("append_to_response", "credits")])
            .send()
            .await
            .map_err(|e| DomainError::Storage(format!("TMDB detail failed: {e}")))?;
        if !resp.status().is_success() {
            return Err(DomainError::NotFound(format!(
                "TMDB item '{base_id}' not found (HTTP {})",
                resp.status()
            )));
        }
        let detail: TmdbDetailBody = resp
            .json()
            .await
            .map_err(|e| DomainError::Storage(format!("TMDB detail JSON error: {e}")))?;

        let mut season_body: Option<TmdbSeasonBody> = None;
        let mut ep_body: Option<TmdbEpisodeBody> = None;

        if target == ScrapeTargetKind::TvShow {
            let s = season_ep.map(|(s, _)| s).unwrap_or(1);
            let season_url = format!("https://api.themoviedb.org/3/tv/{base_id}/season/{s}");
            if let Ok(s_resp) = attach_tmdb_auth(client.get(&season_url), api_key)
                .query(&[("language", locale)])
                .send()
                .await
                && s_resp.status().is_success()
            {
                season_body = s_resp.json().await.ok();
            }

            if let Some((_, e)) = season_ep {
                if let Some(ref sb) = season_body {
                    ep_body = sb
                        .episodes
                        .iter()
                        .find(|ep| ep.episode_number == Some(e))
                        .cloned();
                }
                if ep_body.is_none() {
                    let ep_url =
                        format!("https://api.themoviedb.org/3/tv/{base_id}/season/{s}/episode/{e}");
                    if let Ok(ep_resp) = attach_tmdb_auth(client.get(&ep_url), api_key)
                        .query(&[("language", locale)])
                        .send()
                        .await
                        && ep_resp.status().is_success()
                    {
                        ep_body = ep_resp.json().await.ok();
                    }
                }
            }
        }

        Ok(map_tmdb_detail(detail, target, season_body, ep_body))
    }
}

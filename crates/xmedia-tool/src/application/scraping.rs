use std::collections::BTreeMap;
use std::sync::Arc;

use tokio::task::JoinSet;
use tracing::warn;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;
use xmedia_core::infra::thumbnail::sniff_image_mime;

use crate::application::settings::ToolSettingsService;
use crate::application::workbench::MediaWorkbenchService;
use crate::domain::config::ScraperPriorities;
use crate::domain::media::{
    BookMetaPatch, ComicMetaPatch, MediaFileDetail, MediaMetadataPatch, MusicMetaPatch,
    VideoMetaPatch,
};
use crate::domain::ports::ScraperProviderPort;
use crate::domain::scraper::{
    AggregateScrapeResponse, FieldSourceBadge, ProviderBriefDto, ProviderStatusDto,
    ScrapeCandidate, ScrapeQuery, ScrapeTargetKind, ScrapedMetadata, merge_scraped_by_priority,
};
use crate::domain::workspace::RootId;
use crate::infra::scrapers::http_client::build_http_client;

/// Application service orchestrating pluggable `ScraperProviderPort` adapters.
///
/// Providers are registered dynamically via `register_provider` or `new(...)`, making
/// new metadata/lyrics/artwork scrapers effortless to add without touching application logic.
#[derive(Clone)]
pub struct ScrapingOrchestratorService {
    settings: ToolSettingsService,
    workbench: MediaWorkbenchService,
    providers: Arc<Vec<Arc<dyn ScraperProviderPort>>>,
}

impl ScrapingOrchestratorService {
    pub fn new(
        settings: ToolSettingsService,
        workbench: MediaWorkbenchService,
        providers: Vec<Arc<dyn ScraperProviderPort>>,
    ) -> Self {
        Self {
            settings,
            workbench,
            providers: Arc::new(providers),
        }
    }

    fn sort_providers_by_priority(
        &self,
        domain: Option<MediaKind>,
        priorities: &ScraperPriorities,
        explicit_order: &[String],
    ) -> Vec<Arc<dyn ScraperProviderPort>> {
        let mut list: Vec<(usize, Arc<dyn ScraperProviderPort>)> =
            self.providers.iter().cloned().enumerate().collect();

        let ref_order: &[String] = if !explicit_order.is_empty() {
            explicit_order
        } else if let Some(dom) = domain {
            priorities.order_for(dom)
        } else {
            &[]
        };

        if !ref_order.is_empty() {
            list.sort_by_key(|(reg_idx, p)| {
                let id = p.descriptor().id;
                let prio_pos = ref_order
                    .iter()
                    .position(|x| x == &id)
                    .unwrap_or(usize::MAX);
                (prio_pos, *reg_idx)
            });
        }

        list.into_iter().map(|(_, p)| p).collect()
    }

    pub async fn list_providers(
        &self,
        target_filter: Option<ScrapeTargetKind>,
        domain_hint: Option<MediaKind>,
    ) -> DomainResult<Vec<ProviderStatusDto>> {
        let raw_settings = self.settings.get_raw().await?;
        let effective_domain =
            domain_hint.or_else(|| target_filter.map(ScrapeTargetKind::media_kind));
        let ordered = self.sort_providers_by_priority(
            effective_domain,
            &raw_settings.scraper_priorities,
            &[],
        );

        let mut out = Vec::new();
        for p in ordered {
            let desc = p.descriptor();
            if let Some(target) = target_filter
                && !desc.supported_targets.contains(&target)
            {
                continue;
            }
            if let Some(dom) = domain_hint
                && target_filter.is_none()
                && !desc.supported_targets.iter().any(|t| t.media_kind() == dom)
            {
                continue;
            }
            out.push(ProviderStatusDto {
                configured: p.is_configured(&raw_settings.credentials),
                descriptor: desc,
            });
        }
        Ok(out)
    }

    pub async fn search(&self, mut query: ScrapeQuery) -> DomainResult<Vec<ScrapeCandidate>> {
        if query.keyword.trim().is_empty() && query.isbn.as_deref().unwrap_or("").trim().is_empty()
        {
            return Ok(Vec::new());
        }
        let raw_settings = self.settings.get_raw().await?;
        if query.locale.is_none() {
            query.locale = Some(raw_settings.default_scrape_locale);
        }
        let creds = raw_settings.credentials;
        let domain = query.target.map(ScrapeTargetKind::media_kind);
        let ordered = self.sort_providers_by_priority(
            domain,
            &raw_settings.scraper_priorities,
            &query.providers,
        );

        let selected: Vec<Arc<dyn ScraperProviderPort>> = ordered
            .into_iter()
            .filter(|p| {
                let desc = p.descriptor();
                if let Some(target) = query.target
                    && !desc.supported_targets.contains(&target)
                {
                    return false;
                }
                if !query.providers.is_empty() && !query.providers.iter().any(|id| id == &desc.id) {
                    return false;
                }
                p.is_configured(&creds)
            })
            .collect();

        if selected.is_empty() {
            return Ok(Vec::new());
        }

        let provider_count = selected.len();
        let mut set = JoinSet::new();
        for (order_idx, provider) in selected.into_iter().enumerate() {
            let q = query.clone();
            let c = creds.clone();
            set.spawn(async move {
                let id = provider.descriptor().id;
                let items = match provider.search(&q, &c).await {
                    Ok(items) => items,
                    Err(err) => {
                        warn!("scraper provider '{id}' search failed: {err}");
                        Vec::new()
                    }
                };
                (order_idx, items)
            });
        }

        let mut per_provider: Vec<Vec<ScrapeCandidate>> = vec![Vec::new(); provider_count];
        while let Some(res) = set.join_next().await {
            if let Ok((idx, items)) = res
                && let Some(slot) = per_provider.get_mut(idx)
            {
                *slot = items;
            }
        }

        let max_len = per_provider.iter().map(Vec::len).max().unwrap_or(0);
        let total_cap: usize = per_provider.iter().map(Vec::len).sum();
        let mut combined = Vec::with_capacity(total_cap);
        for row in 0..max_len {
            for bucket in &per_provider {
                if let Some(item) = bucket.get(row) {
                    combined.push(item.clone());
                }
            }
        }
        Ok(combined)
    }

    /// Searches all enabled providers concurrently in user-configured priority order, fetches full
    /// details for each provider's top candidate in parallel, and merges all fields with provenance badges.
    pub async fn aggregate(&self, mut query: ScrapeQuery) -> DomainResult<AggregateScrapeResponse> {
        if query.keyword.trim().is_empty() && query.isbn.as_deref().unwrap_or("").trim().is_empty()
        {
            return Ok(AggregateScrapeResponse::default());
        }
        let raw_settings = self.settings.get_raw().await?;
        let effective_locale = query
            .locale
            .clone()
            .filter(|l| !l.trim().is_empty())
            .unwrap_or_else(|| raw_settings.default_scrape_locale.clone());
        query.locale = Some(effective_locale.clone());
        let creds = raw_settings.credentials;
        let domain = query.target.map(ScrapeTargetKind::media_kind);

        // Preserve user's configured priority order (or explicit ordered list when passed)
        let ordered = self.sort_providers_by_priority(
            domain,
            &raw_settings.scraper_priorities,
            &query.providers,
        );

        let selected: Vec<Arc<dyn ScraperProviderPort>> = ordered
            .into_iter()
            .filter(|p| {
                let desc = p.descriptor();
                if let Some(target) = query.target
                    && !desc.supported_targets.contains(&target)
                {
                    return false;
                }
                if !query.providers.is_empty() && !query.providers.iter().any(|id| id == &desc.id) {
                    return false;
                }
                p.is_configured(&creds)
            })
            .collect();

        if selected.is_empty() {
            return Ok(AggregateScrapeResponse::default());
        }

        let provider_order: Vec<ProviderBriefDto> = selected
            .iter()
            .map(|p| {
                let d = p.descriptor();
                ProviderBriefDto {
                    id: d.id,
                    name: d.name,
                }
            })
            .collect();

        // Phase 1: Parallel search across all selected providers
        let provider_count = selected.len();
        let mut search_set = JoinSet::new();
        for (order_idx, provider) in selected.iter().cloned().enumerate() {
            let q = query.clone();
            let c = creds.clone();
            search_set.spawn(async move {
                let id = provider.descriptor().id;
                let items = match provider.search(&q, &c).await {
                    Ok(items) => items,
                    Err(err) => {
                        warn!("scraper provider '{id}' search failed: {err}");
                        Vec::new()
                    }
                };
                (order_idx, items)
            });
        }

        let mut per_provider: Vec<Vec<ScrapeCandidate>> = vec![Vec::new(); provider_count];
        while let Some(res) = search_set.join_next().await {
            if let Ok((idx, items)) = res
                && let Some(slot) = per_provider.get_mut(idx)
            {
                *slot = items;
            }
        }

        // Phase 2: Parallel detail fetch for each provider's #1 candidate
        let mut selected_by_provider: BTreeMap<String, ScrapeCandidate> = BTreeMap::new();
        let mut detail_set = JoinSet::new();

        for (order_idx, items) in per_provider.iter().enumerate() {
            let Some(top_cand) = items.first().cloned() else {
                continue;
            };
            let provider = Arc::clone(&selected[order_idx]);
            let prov_desc = provider.descriptor();
            selected_by_provider.insert(prov_desc.id.clone(), top_cand.clone());

            let loc = effective_locale.clone();
            let c = creds.clone();
            detail_set.spawn(async move {
                let id = prov_desc.id.clone();
                let detail_res = provider
                    .fetch_detail(&top_cand.external_id, top_cand.target, &loc, &c)
                    .await;
                let detail = match detail_res {
                    Ok(d) => d,
                    Err(err) => {
                        warn!("scraper provider '{id}' top detail failed: {err}");
                        // Fallback to candidate fields if detail endpoint failed
                        ScrapedMetadata {
                            provider_id: top_cand.provider_id,
                            external_id: top_cand.external_id,
                            target: Some(top_cand.target),
                            title: Some(top_cand.title),
                            year: top_cand.year,
                            summary: top_cand.summary,
                            cover_url: top_cand.cover_url,
                            synced_lyrics: top_cand.synced_lyrics,
                            plain_lyrics: top_cand.plain_lyrics,
                            ..Default::default()
                        }
                    }
                };
                (order_idx, id, prov_desc.name, detail)
            });
        }

        let mut ordered_details: Vec<Option<(FieldSourceBadge, ScrapedMetadata)>> =
            vec![None; provider_count];
        let mut details_by_provider: BTreeMap<String, ScrapedMetadata> = BTreeMap::new();

        while let Some(res) = detail_set.join_next().await {
            if let Ok((idx, prov_id, prov_name, detail)) = res {
                details_by_provider.insert(prov_id.clone(), detail.clone());
                if let Some(slot) = ordered_details.get_mut(idx) {
                    *slot = Some((
                        FieldSourceBadge {
                            provider_id: prov_id,
                            provider_name: prov_name,
                        },
                        detail,
                    ));
                }
            }
        }

        let active_sources: Vec<(FieldSourceBadge, ScrapedMetadata)> =
            ordered_details.into_iter().flatten().collect();
        let merged = merge_scraped_by_priority(&active_sources, query.target);

        let max_len = per_provider.iter().map(Vec::len).max().unwrap_or(0);
        let total_cap: usize = per_provider.iter().map(Vec::len).sum();
        let mut combined = Vec::with_capacity(total_cap);
        for row in 0..max_len {
            for bucket in &per_provider {
                if let Some(item) = bucket.get(row) {
                    combined.push(item.clone());
                }
            }
        }

        Ok(AggregateScrapeResponse {
            provider_order,
            candidates: combined,
            selected_by_provider,
            details_by_provider,
            merged,
        })
    }

    pub async fn fetch_detail(
        &self,
        provider_id: &str,
        external_id: &str,
        target: ScrapeTargetKind,
        locale: Option<&str>,
    ) -> DomainResult<ScrapedMetadata> {
        let raw_settings = self.settings.get_raw().await?;
        let effective_locale = locale
            .filter(|l| !l.trim().is_empty())
            .unwrap_or(&raw_settings.default_scrape_locale);
        let creds = raw_settings.credentials;

        let provider = self
            .providers
            .iter()
            .find(|p| p.descriptor().id == provider_id)
            .ok_or_else(|| {
                DomainError::NotFound(format!("scraper provider '{provider_id}' not registered"))
            })?;

        if !provider.is_configured(&creds) {
            return Err(DomainError::Validation(format!(
                "provider '{}' requires API credentials in Settings",
                provider.descriptor().name
            )));
        }

        provider
            .fetch_detail(external_id, target, effective_locale, &creds)
            .await
    }

    /// Applies `ScrapedMetadata` directly onto a target media file (atomically writing tags/OPF/ComicInfo/NFO,
    /// lyrics, and downloading cover/backdrop artwork if requested).
    pub async fn apply_to_file(
        &self,
        root_id: RootId,
        rel_path: &str,
        media_kind: MediaKind,
        scraped: ScrapedMetadata,
        apply_cover: bool,
        write_sidecar_lrc: bool,
    ) -> DomainResult<MediaFileDetail> {
        let patch = map_scraped_to_patch(media_kind, &scraped, write_sidecar_lrc);
        let mut detail = self
            .workbench
            .write_metadata(root_id, rel_path, patch)
            .await?;

        if apply_cover {
            let creds = self.settings.get_credentials().await?;
            let client = build_http_client(&creds)?;

            if let Some(ref cover_url) = scraped.cover_url
                && !cover_url.trim().is_empty()
                && let Ok((bytes, mime)) = download_image_bytes(&client, cover_url).await
            {
                let slot = if media_kind == MediaKind::Video {
                    "poster"
                } else {
                    "cover"
                };
                detail = self
                    .workbench
                    .write_artwork(root_id, rel_path, &bytes, &mime, slot, Some(media_kind))
                    .await?;
            }

            if media_kind == MediaKind::Video
                && let Some(ref backdrop_url) = scraped.backdrop_url
                && !backdrop_url.trim().is_empty()
                && let Ok((bytes, mime)) = download_image_bytes(&client, backdrop_url).await
            {
                detail = self
                    .workbench
                    .write_artwork(root_id, rel_path, &bytes, &mime, "fanart", Some(media_kind))
                    .await?;
            }
        }

        Ok(detail)
    }
}

async fn download_image_bytes(
    client: &reqwest::Client,
    url: &str,
) -> DomainResult<(Vec<u8>, String)> {
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| DomainError::Storage(format!("failed downloading image '{url}': {e}")))?;
    if !resp.status().is_success() {
        return Err(DomainError::Storage(format!(
            "image download returned HTTP {}",
            resp.status()
        )));
    }
    let header_mime = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(';').next().unwrap_or(s).trim().to_string());
    let raw = resp
        .bytes()
        .await
        .map_err(|e| DomainError::Storage(format!("failed reading image body: {e}")))?
        .to_vec();
    let mime = header_mime
        .filter(|m| m.starts_with("image/"))
        .unwrap_or_else(|| sniff_image_mime(&raw).to_string());
    Ok((raw, mime))
}

fn map_scraped_to_patch(
    media_kind: MediaKind,
    s: &ScrapedMetadata,
    write_sidecar_lrc: bool,
) -> MediaMetadataPatch {
    match media_kind {
        MediaKind::Book => MediaMetadataPatch::Book(BookMetaPatch {
            title: s.title.clone(),
            authors: (!s.creators.is_empty()).then(|| s.creators.clone()),
            description: s.summary.clone(),
            publisher: s.publisher_or_studio.clone(),
            published_at: s
                .published_date
                .clone()
                .or_else(|| s.year.map(|y| y.to_string())),
            language: s.language.clone(),
            isbn: s.isbn.clone(),
            tags: {
                let mut merged = s.genres.clone();
                for t in &s.tags {
                    if !merged.contains(t) {
                        merged.push(t.clone());
                    }
                }
                (!merged.is_empty()).then_some(merged)
            },
        }),
        MediaKind::Comic => MediaMetadataPatch::Comic(ComicMetaPatch {
            series: s.series_or_album.clone().or_else(|| s.title.clone()),
            title: s.title.clone(),
            number: s.track_or_episode_number.map(|n| n.to_string()),
            volume: s.disc_or_season_number.and_then(|v| i32::try_from(v).ok()),
            summary: s.summary.clone(),
            writers: (!s.creators.is_empty()).then(|| s.creators.clone()),
            pencillers: (!s.illustrators_or_directors.is_empty())
                .then(|| s.illustrators_or_directors.clone()),
            publisher: s.publisher_or_studio.clone(),
            genres: (!s.genres.is_empty()).then(|| s.genres.clone()),
            tags: (!s.tags.is_empty()).then(|| s.tags.clone()),
            year: s.year,
            language_iso: s.language.clone(),
            reading_direction: None,
        }),
        MediaKind::Music => {
            let best_lyrics = s.synced_lyrics.clone().or_else(|| s.plain_lyrics.clone());
            MediaMetadataPatch::Music(MusicMetaPatch {
                title: s.title.clone(),
                artist: s.creators.first().cloned(),
                album_artist: (!s.creators.is_empty()).then(|| s.creators.join(", ")),
                album: s.series_or_album.clone(),
                track_number: s.track_or_episode_number,
                disc_number: s.disc_or_season_number,
                year: s.year,
                genre: s.genres.first().cloned(),
                comment: None,
                embedded_lyrics: best_lyrics.clone(),
                sidecar_lrc: if write_sidecar_lrc {
                    s.synced_lyrics.clone().or(best_lyrics)
                } else {
                    None
                },
            })
        }
        MediaKind::Video => {
            let is_ep = s.target == Some(ScrapeTargetKind::TvShow)
                || s.track_or_episode_number.is_some()
                || s.series_or_album.is_some();
            MediaMetadataPatch::Video(VideoMetaPatch {
                is_episode: Some(is_ep),
                title: s.title.clone(),
                original_title: s.original_title.clone(),
                show_title: s.series_or_album.clone(),
                season_number: s.disc_or_season_number,
                episode_number: s.track_or_episode_number,
                year: s.year,
                rating: s.rating,
                summary: s.summary.clone(),
                genres: (!s.genres.is_empty()).then(|| s.genres.clone()),
                tags: (!s.tags.is_empty()).then(|| s.tags.clone()),
                actors: (!s.cast.is_empty()).then(|| s.cast.clone()),
                directors: (!s.illustrators_or_directors.is_empty())
                    .then(|| s.illustrators_or_directors.clone()),
                studio: s.publisher_or_studio.clone(),
                tmdb_id: s
                    .external_ids
                    .get("tmdb")
                    .cloned()
                    .or_else(|| (s.provider_id == "tmdb").then(|| s.external_id.clone())),
                bangumi_id: s
                    .external_ids
                    .get("bangumi")
                    .cloned()
                    .or_else(|| (s.provider_id == "bangumi").then(|| s.external_id.clone())),
            })
        }
    }
}

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::Arc;

use tokio::task::JoinSet;
use tracing::warn;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;
use xmedia_core::infra::thumbnail::sniff_image_mime;

use crate::application::settings::ToolSettingsService;
use crate::application::webhook::notify_xmedia_rescan;
use crate::application::workbench::MediaWorkbenchService;
use crate::domain::config::ScraperPriorities;
use crate::domain::media::{
    BookMetaPatch, ComicMetaPatch, MediaFileDetail, MediaMetadataPatch, MediaMetadataPayload,
    MusicMetaPatch, VideoMetaPatch,
};
use crate::domain::ports::ScraperProviderPort;
use crate::domain::scraper::{
    AggregateScrapeResponse, FieldSourceBadge, ProviderBriefDto, ProviderStatusDto,
    ScrapeCandidate, ScrapeQuery, ScrapeTargetKind, ScrapedMetadata,
    is_compatible_secondary_source, merge_scraped_by_priority, promote_exact_match_anchor,
};
use crate::domain::template::render_template_target;
use crate::domain::workspace::RootId;
use crate::infra::atomic_fs::{
    atomic_write_bytes, cleanup_empty_parent_dirs, find_companion_sidecars,
};
use crate::infra::inspectors::book::{dir_has_single_book, is_generic_book_folder};
use crate::infra::inspectors::comic::is_generic_comic_folder;
use crate::infra::inspectors::music::is_generic_music_folder;
use crate::infra::inspectors::video::{
    apply_video_patch_onto, is_season_dir, read_base_video_meta, render_season_nfo_xml,
    render_tvshow_nfo_xml, render_video_nfo_xml, write_image_replacing_opposite_ext,
};
use crate::infra::scrape_cache::{ScrapeCacheStatsDto, ScrapeMetadataCache};
use crate::infra::scrapers::http_client::build_http_client;

#[derive(Debug, Clone, serde::Serialize)]
pub struct DryRunScrapeItemDto {
    pub rel_path: String,
    pub display_title: String,
    pub query_keyword: String,
    pub matched: bool,
    pub matched_title: Option<String>,
    pub matched_creator: Option<String>,
    pub matched_year: Option<i32>,
    pub matched_provider: Option<String>,
    pub confidence: u8,
    pub low_confidence: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AutoScrapeMatchMode {
    #[default]
    Standard,
    Strict,
    Loose,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AutoScrapeOptions {
    #[serde(default)]
    pub match_mode: AutoScrapeMatchMode,
    #[serde(default = "default_true")]
    pub only_fill_missing: bool,
    #[serde(default = "default_true")]
    pub apply_cover: bool,
    #[serde(default = "default_true")]
    pub write_sidecar_lrc: bool,
    #[serde(default)]
    pub organize_by_template: bool,
    #[serde(default)]
    pub providers: Vec<String>,
    /// Optional whitelist of field groups: "basic", "summary", "tags", "lyrics". Empty = all groups.
    #[serde(default)]
    pub field_groups: Vec<String>,
}

impl Default for AutoScrapeOptions {
    fn default() -> Self {
        Self {
            match_mode: AutoScrapeMatchMode::Standard,
            only_fill_missing: true,
            apply_cover: true,
            write_sidecar_lrc: true,
            organize_by_template: false,
            providers: Vec::new(),
            field_groups: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AutoScrapeResultDto {
    pub matched: bool,
    pub confidence: u8,
    pub detail: MediaFileDetail,
}

/// Application service orchestrating pluggable `ScraperProviderPort` adapters.
///
/// Providers are registered dynamically via `register_provider` or `new(...)`, making
/// new metadata/lyrics/artwork scrapers effortless to add without touching application logic.
#[derive(Clone)]
pub struct ScrapingOrchestratorService {
    settings: ToolSettingsService,
    workbench: MediaWorkbenchService,
    providers: Arc<Vec<Arc<dyn ScraperProviderPort>>>,
    cache: ScrapeMetadataCache,
}

impl ScrapingOrchestratorService {
    pub fn new(
        settings: ToolSettingsService,
        workbench: MediaWorkbenchService,
        providers: Vec<Arc<dyn ScraperProviderPort>>,
    ) -> Self {
        Self::with_cache_dir(settings, workbench, providers, None)
    }

    pub fn with_cache_dir(
        settings: ToolSettingsService,
        workbench: MediaWorkbenchService,
        providers: Vec<Arc<dyn ScraperProviderPort>>,
        cache_dir: Option<std::path::PathBuf>,
    ) -> Self {
        Self {
            settings,
            workbench,
            providers: Arc::new(providers),
            cache: ScrapeMetadataCache::new(cache_dir),
        }
    }

    pub async fn cache_stats(&self) -> DomainResult<ScrapeCacheStatsDto> {
        let ttl_ms = self.settings.get_raw().await?.scrape_cache_ttl_ms;
        Ok(self.cache.stats(ttl_ms))
    }

    pub fn clear_cache(&self) -> DomainResult<usize> {
        self.cache.clear()
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
            let enabled = raw_settings.is_provider_enabled(&desc.id);
            let prov_creds = raw_settings.credentials_for_provider(&desc.id);
            let credentials_ready = p.is_configured(&prov_creds);
            out.push(ProviderStatusDto {
                enabled,
                credentials_ready,
                configured: enabled && credentials_ready,
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
        let effective_locale = query
            .locale
            .clone()
            .filter(|l| !l.trim().is_empty())
            .unwrap_or_else(|| raw_settings.default_scrape_locale.clone());
        query.locale = Some(effective_locale.clone());
        let ttl_ms = raw_settings.scrape_cache_ttl_ms;
        let domain = query.target.map(ScrapeTargetKind::media_kind);
        let ordered = self.sort_providers_by_priority(
            domain,
            &raw_settings.scraper_priorities,
            &query.providers,
        );

        let has_isbn_hint = query.isbn.as_deref().is_some_and(|s| !s.trim().is_empty());
        let selected: Vec<Arc<dyn ScraperProviderPort>> = ordered
            .into_iter()
            .filter(|p| {
                let desc = p.descriptor();
                if let Some(target) = query.target
                    && !desc.supported_targets.contains(&target)
                {
                    return false;
                }
                if has_isbn_hint
                    && is_webnovel_serial_only_provider(&desc.id)
                    && query.providers.is_empty()
                {
                    return false;
                }
                if !query.providers.is_empty() && !query.providers.iter().any(|id| id == &desc.id) {
                    return false;
                }
                if !raw_settings.is_provider_enabled(&desc.id) {
                    return false;
                }
                let prov_creds = raw_settings.credentials_for_provider(&desc.id);
                p.is_configured(&prov_creds)
            })
            .collect();

        if selected.is_empty() {
            return Ok(Vec::new());
        }

        let provider_count = selected.len();
        let mut set = JoinSet::new();
        for (order_idx, provider) in selected.into_iter().enumerate() {
            let id = provider.descriptor().id;
            let loc = raw_settings.locale_for_provider(&id, &effective_locale);
            let mut q = query.clone();
            q.locale = Some(loc.clone());
            let c = raw_settings.credentials_for_provider(&id);
            let cache = self.cache.clone();
            set.spawn(async move {
                if let Some(cached) = cache.get_search(&id, &q, &loc, ttl_ms) {
                    return (order_idx, cached);
                }
                let items = match provider.search(&q, &c).await {
                    Ok(items) => {
                        cache.put_search(&id, &q, &loc, ttl_ms, &items);
                        items
                    }
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
        let ttl_ms = raw_settings.scrape_cache_ttl_ms;
        let domain = query.target.map(ScrapeTargetKind::media_kind);

        // Preserve user's configured priority order (or explicit ordered list when passed)
        let ordered = self.sort_providers_by_priority(
            domain,
            &raw_settings.scraper_priorities,
            &query.providers,
        );

        let has_isbn_hint = query.isbn.as_deref().is_some_and(|s| !s.trim().is_empty());
        let selected: Vec<Arc<dyn ScraperProviderPort>> = ordered
            .into_iter()
            .filter(|p| {
                let desc = p.descriptor();
                if let Some(target) = query.target
                    && !desc.supported_targets.contains(&target)
                {
                    return false;
                }
                if has_isbn_hint
                    && is_webnovel_serial_only_provider(&desc.id)
                    && query.providers.is_empty()
                {
                    return false;
                }
                if !query.providers.is_empty() && !query.providers.iter().any(|id| id == &desc.id) {
                    return false;
                }
                if !raw_settings.is_provider_enabled(&desc.id) {
                    return false;
                }
                let prov_creds = raw_settings.credentials_for_provider(&desc.id);
                p.is_configured(&prov_creds)
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
            let id = provider.descriptor().id;
            let loc = raw_settings.locale_for_provider(&id, &effective_locale);
            let mut q = query.clone();
            q.locale = Some(loc.clone());
            let c = raw_settings.credentials_for_provider(&id);
            let cache = self.cache.clone();
            search_set.spawn(async move {
                if let Some(cached) = cache.get_search(&id, &q, &loc, ttl_ms) {
                    return (order_idx, cached);
                }
                let items = match provider.search(&q, &c).await {
                    Ok(items) => {
                        cache.put_search(&id, &q, &loc, ttl_ms, &items);
                        items
                    }
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

            let loc = raw_settings.locale_for_provider(&prov_desc.id, &effective_locale);
            let c = raw_settings.credentials_for_provider(&prov_desc.id);
            let cache = self.cache.clone();
            detail_set.spawn(async move {
                let id = prov_desc.id.clone();
                if let Some(cached) =
                    cache.get_detail(&id, top_cand.target, &top_cand.external_id, &loc, ttl_ms)
                {
                    return (order_idx, id, prov_desc.name, cached);
                }
                let detail_res = provider
                    .fetch_detail(&top_cand.external_id, top_cand.target, &loc, &c)
                    .await;
                let detail = match detail_res {
                    Ok(d) => {
                        cache.put_detail(
                            &id,
                            top_cand.target,
                            &top_cand.external_id,
                            &loc,
                            ttl_ms,
                            &d,
                        );
                        d
                    }
                    Err(err) => {
                        warn!("scraper provider '{id}' top detail failed (falling back to search candidate): {err}");
                        fallback_metadata_from_candidate(&top_cand)
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

        let mut raw_active: Vec<(FieldSourceBadge, ScrapedMetadata)> =
            ordered_details.into_iter().flatten().collect();
        promote_exact_match_anchor(&mut raw_active, &query);
        let active_sources: Vec<(FieldSourceBadge, ScrapedMetadata)> = if raw_active.len() > 1 {
            let primary_meta = raw_active[0].1.clone();
            let target_hint = query.target;
            raw_active
                .into_iter()
                .enumerate()
                .filter(|(idx, (_, sec))| {
                    *idx == 0 || is_compatible_secondary_source(&primary_meta, sec, target_hint)
                })
                .map(|(_, pair)| pair)
                .collect()
        } else {
            raw_active
        };
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
        let base_locale = locale
            .filter(|l| !l.trim().is_empty())
            .unwrap_or(&raw_settings.default_scrape_locale);
        let effective_locale = raw_settings.locale_for_provider(provider_id, base_locale);

        let provider = self
            .providers
            .iter()
            .find(|p| p.descriptor().id == provider_id)
            .ok_or_else(|| {
                DomainError::NotFound(format!("scraper provider '{provider_id}' not registered"))
            })?;

        if !raw_settings.is_provider_enabled(provider_id) {
            return Err(DomainError::Validation(format!(
                "刮削源 '{}' 已禁用，请在设置中启用后重试",
                provider.descriptor().name
            )));
        }

        let creds = raw_settings.credentials_for_provider(provider_id);
        if !provider.is_configured(&creds) {
            return Err(DomainError::Validation(format!(
                "provider '{}' requires API credentials in Settings",
                provider.descriptor().name
            )));
        }

        let ttl_ms = raw_settings.scrape_cache_ttl_ms;
        if let Some(cached) =
            self.cache
                .get_detail(provider_id, target, external_id, &effective_locale, ttl_ms)
        {
            return Ok(cached);
        }

        let detail = provider
            .fetch_detail(external_id, target, &effective_locale, &creds)
            .await?;
        self.cache.put_detail(
            provider_id,
            target,
            external_id,
            &effective_locale,
            ttl_ms,
            &detail,
        );
        Ok(detail)
    }

    /// Automatic waterfall scrape for background task queue:
    /// Priority order: Local non-empty metadata -> #1 Priority Provider -> #2 Priority Provider -> ...
    pub async fn dry_run_files(
        &self,
        root_id: RootId,
        paths: Vec<String>,
        locale: Option<String>,
    ) -> DomainResult<Vec<DryRunScrapeItemDto>> {
        let mut results = Vec::with_capacity(paths.len());
        for rel_path in paths {
            let detail = match self.workbench.inspect_file(root_id, &rel_path, None).await {
                Ok(d) => d,
                Err(e) => {
                    results.push(DryRunScrapeItemDto {
                        rel_path: rel_path.clone(),
                        display_title: rel_path,
                        query_keyword: String::new(),
                        matched: false,
                        matched_title: None,
                        matched_creator: None,
                        matched_year: None,
                        matched_provider: None,
                        confidence: 0,
                        low_confidence: true,
                        error: Some(e.to_string()),
                    });
                    continue;
                }
            };
            let mut query = build_auto_query_from_detail(&detail);
            if locale.is_some() {
                query.locale = locale.clone();
            }
            let kw = query.keyword.clone();
            match self.aggregate(query.clone()).await {
                Ok(agg) => {
                    let confidence = compute_scrape_confidence(&query, &agg);
                    let meta = agg.merged.metadata;
                    let matched = meta.title.is_some();
                    let matched_provider = agg
                        .merged
                        .field_sources
                        .get("title")
                        .map(|b| b.provider_name.clone())
                        .or_else(|| agg.provider_order.first().map(|p| p.name.clone()));
                    results.push(DryRunScrapeItemDto {
                        rel_path,
                        display_title: detail.entry.display_title,
                        query_keyword: kw,
                        matched,
                        matched_title: meta.title,
                        matched_creator: meta.creators.first().cloned(),
                        matched_year: meta.year,
                        matched_provider,
                        confidence,
                        low_confidence: !matched || confidence < 65,
                        error: None,
                    });
                }
                Err(e) => {
                    results.push(DryRunScrapeItemDto {
                        rel_path,
                        display_title: detail.entry.display_title,
                        query_keyword: kw,
                        matched: false,
                        matched_title: None,
                        matched_creator: None,
                        matched_year: None,
                        matched_provider: None,
                        confidence: 0,
                        low_confidence: true,
                        error: Some(e.to_string()),
                    });
                }
            }
        }
        Ok(results)
    }

    pub async fn auto_scrape_file(
        &self,
        root_id: RootId,
        rel_path: &str,
        media_kind: MediaKind,
    ) -> DomainResult<AutoScrapeResultDto> {
        self.auto_scrape_file_with_options(
            root_id,
            rel_path,
            media_kind,
            AutoScrapeOptions::default(),
        )
        .await
    }

    pub async fn auto_scrape_file_with_options(
        &self,
        root_id: RootId,
        rel_path: &str,
        media_kind: MediaKind,
        opts: AutoScrapeOptions,
    ) -> DomainResult<AutoScrapeResultDto> {
        let local_detail = self
            .workbench
            .inspect_file(root_id, rel_path, Some(media_kind))
            .await?;
        let mut query = build_auto_query_from_detail(&local_detail);
        if !opts.providers.is_empty() {
            query.providers = opts.providers.clone();
        }
        if query.keyword.trim().is_empty() && query.isbn.as_deref().unwrap_or("").trim().is_empty()
        {
            return Ok(AutoScrapeResultDto {
                matched: false,
                confidence: 0,
                detail: local_detail,
            });
        }

        let agg = self.aggregate(query.clone()).await?;
        let confidence = compute_scrape_confidence(&query, &agg);
        let has_hit = agg
            .merged
            .metadata
            .title
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty())
            || agg.merged.metadata.cover_url.is_some()
            || agg.merged.metadata.synced_lyrics.is_some()
            || agg.merged.metadata.plain_lyrics.is_some()
            || agg.merged.metadata.summary.is_some();

        let passes_mode = match opts.match_mode {
            AutoScrapeMatchMode::Strict => has_hit && confidence >= 70,
            AutoScrapeMatchMode::Standard => has_hit && confidence >= 45,
            AutoScrapeMatchMode::Loose => has_hit,
        };

        if !passes_mode {
            return Ok(AutoScrapeResultDto {
                matched: false,
                confidence,
                detail: local_detail,
            });
        }

        let filtered_meta =
            filter_scraped_metadata_by_groups(agg.merged.metadata, &opts.field_groups);

        let updated = self
            .apply_to_file_ext(
                root_id,
                rel_path,
                media_kind,
                filtered_meta,
                opts.apply_cover,
                opts.write_sidecar_lrc,
                Some(opts.organize_by_template),
                opts.only_fill_missing,
                false,
            )
            .await?;

        Ok(AutoScrapeResultDto {
            matched: true,
            confidence,
            detail: updated,
        })
    }

    /// Applies `ScrapedMetadata` directly onto a target media file (atomically writing tags/OPF/ComicInfo/NFO,
    /// lyrics, downloading artwork, and optionally organizing into templates / Jellyfin folder layout).
    #[allow(clippy::too_many_arguments)]
    pub async fn apply_to_file(
        &self,
        root_id: RootId,
        rel_path: &str,
        media_kind: MediaKind,
        scraped: ScrapedMetadata,
        apply_cover: bool,
        write_sidecar_lrc: bool,
        organize_by_template: Option<bool>,
        only_fill_missing: bool,
    ) -> DomainResult<MediaFileDetail> {
        self.apply_to_file_ext(
            root_id,
            rel_path,
            media_kind,
            scraped,
            apply_cover,
            write_sidecar_lrc,
            organize_by_template,
            only_fill_missing,
            false,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn apply_to_file_ext(
        &self,
        root_id: RootId,
        rel_path: &str,
        media_kind: MediaKind,
        scraped: ScrapedMetadata,
        apply_cover: bool,
        write_sidecar_lrc: bool,
        organize_by_template: Option<bool>,
        only_fill_missing: bool,
        write_album_cover: bool,
    ) -> DomainResult<MediaFileDetail> {
        let should_organize = organize_by_template.unwrap_or(true);

        if media_kind == MediaKind::Video && should_organize {
            return self
                .apply_video_jellyfin_archive(
                    root_id,
                    rel_path,
                    scraped,
                    apply_cover,
                    only_fill_missing,
                )
                .await;
        }

        let need_existing = only_fill_missing
            || media_kind == MediaKind::Book
            || (media_kind == MediaKind::Music
                && scraped.target == Some(ScrapeTargetKind::MusicAlbum));
        let existing_detail = if need_existing {
            Some(
                self.workbench
                    .inspect_file(root_id, rel_path, Some(media_kind))
                    .await?,
            )
        } else {
            None
        };

        let existing_music_artist = existing_detail.as_ref().is_some_and(|d| {
            matches!(&d.metadata, MediaMetadataPayload::Music(m) if !m.artist.trim().is_empty())
        });

        let mut patch = map_scraped_to_patch(
            media_kind,
            &scraped,
            write_sidecar_lrc,
            existing_music_artist,
        );
        if only_fill_missing && let Some(ref cur) = existing_detail {
            let fallback_stem = Path::new(rel_path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            filter_patch_for_missing(&mut patch, &cur.metadata, fallback_stem);
        }

        let mut effective_apply_cover = apply_cover;
        if let Some(MediaMetadataPayload::Book(existing_book)) =
            existing_detail.as_ref().map(|d| &d.metadata)
            && !existing_book.locked_fields.is_empty()
        {
            let is_locked = |name: &str| {
                existing_book
                    .locked_fields
                    .iter()
                    .any(|f| f.eq_ignore_ascii_case(name))
            };
            if let MediaMetadataPatch::Book(ref mut bp) = patch {
                if is_locked("title") {
                    bp.title = None;
                }
                if is_locked("authors") {
                    bp.authors = None;
                }
                if is_locked("series") {
                    bp.series = None;
                }
                if is_locked("subseries") {
                    bp.subseries = None;
                }
                if is_locked("series_index") {
                    bp.series_index = None;
                }
                if is_locked("isbn") {
                    bp.isbn = None;
                }
                if is_locked("publisher") {
                    bp.publisher = None;
                }
                if is_locked("published_at") {
                    bp.published_at = None;
                }
                if is_locked("language") {
                    bp.language = None;
                }
                if is_locked("tags") {
                    bp.tags = None;
                }
                if is_locked("description") {
                    bp.description = None;
                }
            }
            if is_locked("cover") {
                effective_apply_cover = false;
            }
        }

        if let Some(MediaMetadataPayload::Video(existing_video)) =
            existing_detail.as_ref().map(|d| &d.metadata)
            && !existing_video.locked_fields.is_empty()
        {
            let is_locked = |name: &str| {
                existing_video
                    .locked_fields
                    .iter()
                    .any(|f| f.eq_ignore_ascii_case(name))
            };
            if let MediaMetadataPatch::Video(ref mut vp) = patch {
                if is_locked("title") {
                    vp.title = None;
                }
                if is_locked("original_title") {
                    vp.original_title = None;
                }
                if is_locked("show_title") {
                    vp.show_title = None;
                }
                if is_locked("summary") || is_locked("overview") {
                    vp.summary = None;
                }
                if is_locked("genres") {
                    vp.genres = None;
                }
                if is_locked("tags") {
                    vp.tags = None;
                }
                if is_locked("actors") || is_locked("cast") {
                    vp.actors = None;
                }
                if is_locked("directors") {
                    vp.directors = None;
                }
                if is_locked("studio") {
                    vp.studio = None;
                }
                if is_locked("year") {
                    vp.year = None;
                }
                if is_locked("rating") {
                    vp.rating = None;
                }
                if is_locked("collection_set") || is_locked("set") {
                    vp.collection_set = None;
                }
            }
            if is_locked("cover") || is_locked("poster") {
                effective_apply_cover = false;
            }
        }

        let mut detail = self
            .workbench
            .write_metadata(root_id, rel_path, patch)
            .await?;

        let defer_non_epub_book_cover = media_kind == MediaKind::Book
            && should_organize
            && !detail.entry.extension.eq_ignore_ascii_case("epub");
        let mut cached_cover_download: Option<(Vec<u8>, String)> = None;
        if effective_apply_cover && media_kind != MediaKind::Comic && !defer_non_epub_book_cover {
            let creds = self.settings.get_credentials().await?;
            let client = build_http_client(&creds)?;

            if media_kind == MediaKind::Video {
                let (has_poster, has_fanart, has_thumb) = match existing_detail
                    .as_ref()
                    .map(|d| &d.metadata)
                    .unwrap_or(&detail.metadata)
                {
                    MediaMetadataPayload::Video(v) => (v.has_poster, v.has_fanart, v.has_thumb),
                    _ => (false, false, false),
                };
                for (slot, url_opt, already_has) in [
                    ("poster", scraped.cover_url.as_deref(), has_poster),
                    ("fanart", scraped.backdrop_url.as_deref(), has_fanart),
                    ("thumb", scraped.episode_still_url.as_deref(), has_thumb),
                ] {
                    if only_fill_missing && already_has {
                        continue;
                    }
                    if let Some(u) = url_opt.map(str::trim).filter(|s| !s.is_empty())
                        && let Ok((bytes, mime)) = download_image_bytes(&client, u).await
                    {
                        detail = self
                            .workbench
                            .write_artwork(root_id, rel_path, &bytes, &mime, slot, Some(media_kind))
                            .await?;
                    }
                }
            } else {
                let already_has_cover = existing_detail
                    .as_ref()
                    .is_some_and(|d| existing_artwork_present(&d.metadata));
                if let Some(ref cover_url) = scraped.cover_url
                    && !cover_url.trim().is_empty()
                {
                    let need_embedded = !(only_fill_missing && already_has_cover);
                    if (need_embedded || (media_kind == MediaKind::Music && write_album_cover))
                        && let Ok((bytes, mime)) = download_image_bytes(&client, cover_url).await
                    {
                        if need_embedded {
                            detail = self
                                .workbench
                                .write_artwork(
                                    root_id,
                                    rel_path,
                                    &bytes,
                                    &mime,
                                    "cover",
                                    Some(media_kind),
                                )
                                .await?;
                        }
                        cached_cover_download = Some((bytes, mime));
                    }
                }
            }
        }

        if should_organize {
            let plans = self
                .workbench
                .preview_rename(
                    root_id,
                    &[detail.entry.rel_path.clone()],
                    None,
                    None,
                    Some(media_kind),
                )
                .await?;
            if let Some(plan) = plans.first()
                && !plan.unchanged
                && plan.conflict.is_none()
            {
                self.workbench.execute_rename(root_id, &plans).await?;
                if media_kind == MediaKind::Book
                    && !detail.entry.extension.eq_ignore_ascii_case("epub")
                    && let Ok(root) = self.settings.find_root(root_id).await
                    && let Ok(dst_sb) = root.resolve_existing(&plan.to_rel_path)
                    && let Some(dst_dir) = dst_sb.abs_path.parent()
                    && dst_dir != root.path
                    && dst_dir
                        .file_name()
                        .and_then(|s| s.to_str())
                        .is_some_and(|n| !is_generic_book_folder(n))
                    && dir_has_single_book(dst_dir)
                {
                    let stem_opf = dst_sb.abs_path.with_extension("opf");
                    let calibre_opf = dst_dir.join("metadata.opf");
                    if stem_opf.is_file() && !calibre_opf.exists() {
                        let _ = std::fs::rename(&stem_opf, &calibre_opf);
                    }
                    for ext in ["jpg", "jpeg", "png", "webp"] {
                        let stem_cov = dst_sb.abs_path.with_extension(ext);
                        let calibre_cov = dst_dir.join(format!("cover.{ext}"));
                        if stem_cov.is_file() && !calibre_cov.exists() {
                            let _ = std::fs::rename(&stem_cov, &calibre_cov);
                        }
                    }
                }
                detail = self
                    .workbench
                    .inspect_file(root_id, &plan.to_rel_path, Some(media_kind))
                    .await?;
            }
        }

        if effective_apply_cover && defer_non_epub_book_cover {
            let already_has_cover = existing_detail
                .as_ref()
                .is_some_and(|d| existing_artwork_present(&d.metadata));
            if !(only_fill_missing && already_has_cover)
                && let Some(ref cover_url) = scraped.cover_url
                && !cover_url.trim().is_empty()
            {
                let creds = self.settings.get_credentials().await?;
                let client = build_http_client(&creds)?;
                if let Ok((bytes, mime)) = download_image_bytes(&client, cover_url).await {
                    let cur_rel = detail.entry.rel_path.clone();
                    detail = self
                        .workbench
                        .write_artwork(
                            root_id,
                            &cur_rel,
                            &bytes,
                            &mime,
                            "cover",
                            Some(MediaKind::Book),
                        )
                        .await?;
                }
            }
        }

        // Optional Music album directory `cover.*` (user opt-in, default false)
        if effective_apply_cover
            && write_album_cover
            && media_kind == MediaKind::Music
            && let Some((bytes, mime)) = cached_cover_download
        {
            let parent_dir_name = Path::new(&detail.entry.rel_path)
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|s| s.to_str())
                .unwrap_or("");
            let in_album_dir =
                !parent_dir_name.is_empty() && !is_generic_music_folder(parent_dir_name);
            let already_has_dir_cover = matches!(
                &detail.metadata,
                MediaMetadataPayload::Music(m) if m.has_directory_cover
            );
            if in_album_dir && !(only_fill_missing && already_has_dir_cover) {
                let cur_rel = detail.entry.rel_path.clone();
                detail = self
                    .workbench
                    .write_artwork(
                        root_id,
                        &cur_rel,
                        &bytes,
                        &mime,
                        "album_cover",
                        Some(MediaKind::Music),
                    )
                    .await?;
            }
        }

        // For Comics (Komga/Kavita convention): write series-level `cover.*` in the dedicated
        // series directory AFTER organizing, without injecting an extra page into each chapter CBZ.
        if apply_cover && media_kind == MediaKind::Comic {
            let parent_dir_name = Path::new(&detail.entry.rel_path)
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|s| s.to_str())
                .unwrap_or("");
            let in_series_dir =
                !parent_dir_name.is_empty() && !is_generic_comic_folder(parent_dir_name);
            let already_has_series_cover = matches!(
                &detail.metadata,
                MediaMetadataPayload::Comic(c) if c.has_series_cover
            );
            if in_series_dir
                && !(only_fill_missing && already_has_series_cover)
                && let Some(ref cover_url) = scraped.cover_url
                && !cover_url.trim().is_empty()
            {
                let creds = self.settings.get_credentials().await?;
                let client = build_http_client(&creds)?;
                if let Ok((bytes, mime)) = download_image_bytes(&client, cover_url).await {
                    let cur_rel = detail.entry.rel_path.clone();
                    detail = self
                        .workbench
                        .write_artwork(
                            root_id,
                            &cur_rel,
                            &bytes,
                            &mime,
                            "series_cover",
                            Some(MediaKind::Comic),
                        )
                        .await?;
                }
            }
        }

        if let Ok(settings) = self.settings.get_raw().await
            && settings.xmedia_webhook.notify_on_scrape
            && let Ok(root) = self.settings.find_root(root_id).await
        {
            let webhook_cfg = settings.xmedia_webhook;
            let root_path = root.path;
            tokio::spawn(async move {
                notify_xmedia_rescan(&webhook_cfg, &root_path).await;
            });
        }

        Ok(detail)
    }

    async fn apply_video_jellyfin_archive(
        &self,
        root_id: RootId,
        rel_path: &str,
        scraped: ScrapedMetadata,
        apply_cover: bool,
        only_fill_missing: bool,
    ) -> DomainResult<MediaFileDetail> {
        let raw_settings = self.settings.get_raw().await?;
        let root = raw_settings
            .roots
            .iter()
            .find(|r| r.id == root_id)
            .cloned()
            .ok_or_else(|| DomainError::NotFound(format!("root '{root_id}' not found")))?;
        let src_sandbox = root.resolve_existing(rel_path)?;
        if !src_sandbox.abs_path.is_file() {
            return Err(DomainError::Validation(format!(
                "'{}' is not a regular video file",
                src_sandbox.rel_slash_string()
            )));
        }

        let patch = map_scraped_to_patch(MediaKind::Video, &scraped, false, false);
        let MediaMetadataPatch::Video(mut vp) = patch else {
            unreachable!();
        };

        // 1. Merge scraped metadata onto base local video metadata (preserving filename SxxExx if scraper omitted it)
        let src_abs_for_read = src_sandbox.abs_path.clone();
        let mut meta = tokio::task::spawn_blocking(move || read_base_video_meta(&src_abs_for_read))
            .await
            .map_err(|e| DomainError::Storage(format!("video base inspect failed: {e}")))?;
        let existing_poster = meta.has_poster;
        let existing_fanart = meta.has_fanart;
        let existing_thumb = meta.has_thumb;
        if only_fill_missing {
            let fallback_stem = src_sandbox
                .abs_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            filter_video_patch_for_missing(&mut vp, &meta, fallback_stem);
        }
        apply_video_patch_onto(&mut meta, vp);
        if meta.is_episode && meta.season_number.is_none() {
            meta.season_number = Some(1);
        }

        // 2. Compute Jellyfin folder hierarchy target anchored at `root`
        let configured_tpl = if meta.is_episode {
            raw_settings.rename_templates.video_episode.trim()
        } else {
            raw_settings.rename_templates.video_movie.trim()
        };
        let effective_tpl = if configured_tpl.contains('/') || configured_tpl.contains('\\') {
            configured_tpl.to_string()
        } else if meta.is_episode {
            format!("{{show}} ({{year}})/Season {{season:2}}/{configured_tpl}")
        } else {
            format!("{{title}} ({{year}})/{configured_tpl}")
        };

        let ext = src_sandbox
            .abs_path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("mp4");
        let (target_rel_path, target_stem) = render_template_target(
            &effective_tpl,
            "",
            &MediaMetadataPayload::Video(meta.clone()),
            &src_sandbox.abs_path,
            ext,
        );
        let dst_abs = root.resolve_target(&target_rel_path)?;
        if dst_abs != src_sandbox.abs_path && dst_abs.exists() {
            return Err(DomainError::Conflict(format!(
                "target '{}' already exists on disk",
                target_rel_path
            )));
        }

        let dst_dir = dst_abs
            .parent()
            .ok_or_else(|| DomainError::Validation("invalid target video folder".into()))?
            .to_path_buf();
        let show_dir = if meta.is_episode
            && is_season_dir(&dst_dir)
            && dst_dir != root.path
            && let Some(parent) = dst_dir.parent()
            && parent != root.path
            && parent.starts_with(&root.path)
        {
            parent.to_path_buf()
        } else {
            dst_dir.clone()
        };

        // 3. Pre-fetch requested scraped artwork bytes (with URL deduplication)
        let mut downloaded_images: HashMap<String, (Vec<u8>, &'static str)> = HashMap::new();
        let skip_poster = only_fill_missing && existing_poster;
        let skip_fanart = only_fill_missing && existing_fanart;
        let skip_thumb = only_fill_missing && existing_thumb;
        if apply_cover {
            let client = build_http_client(&raw_settings.credentials)?;
            let urls_to_fetch: [Option<&str>; 4] = [
                (!skip_poster)
                    .then_some(scraped.cover_url.as_deref())
                    .flatten(),
                (!skip_fanart)
                    .then_some(scraped.backdrop_url.as_deref())
                    .flatten(),
                (!skip_poster)
                    .then_some(scraped.season_poster_url.as_deref())
                    .flatten(),
                (!skip_thumb)
                    .then_some(scraped.episode_still_url.as_deref())
                    .flatten(),
            ];
            for raw_url in urls_to_fetch.into_iter().flatten() {
                let u = raw_url.trim();
                if u.is_empty() || downloaded_images.contains_key(u) {
                    continue;
                }
                match download_image_bytes(&client, u).await {
                    Ok((bytes, mime)) => {
                        let img_ext = if mime.contains("png") {
                            "png"
                        } else if mime.contains("webp") {
                            "webp"
                        } else {
                            "jpg"
                        };
                        downloaded_images.insert(u.to_string(), (bytes, img_ext));
                    }
                    Err(err) => {
                        warn!("skipping failed video artwork '{u}': {err}");
                    }
                }
            }
        }

        // 4. Perform filesystem move, subtitle migration, 3-tier NFO generation & Jellyfin artwork writes
        let root_path_buf = root.path.clone();
        let src_abs = src_sandbox.abs_path.clone();
        let dst_abs_clone = dst_abs.clone();
        let scraped_clone = scraped.clone();

        tokio::task::spawn_blocking(move || -> DomainResult<()> {
            std::fs::create_dir_all(&dst_dir)
                .map_err(|e| DomainError::Storage(format!("failed creating Jellyfin dir: {e}")))?;
            if show_dir != dst_dir {
                std::fs::create_dir_all(&show_dir).map_err(|e| {
                    DomainError::Storage(format!("failed creating Jellyfin show dir: {e}"))
                })?;
            }

            let old_parent = src_abs.parent().map(Path::to_path_buf);
            let companions = find_companion_sidecars(&src_abs);

            if dst_abs_clone != src_abs {
                std::fs::rename(&src_abs, &dst_abs_clone).map_err(|e| {
                    DomainError::Storage(format!(
                        "failed archiving video into '{}': {e}",
                        dst_abs_clone.display()
                    ))
                })?;
            }

            for (comp_abs, suffix) in companions {
                let lower = suffix.to_ascii_lowercase();
                let is_sub = lower.ends_with(".srt")
                    || lower.ends_with(".ass")
                    || lower.ends_with(".ssa")
                    || lower.ends_with(".vtt");
                if is_sub {
                    let sub_dst = dst_dir.join(format!("{target_stem}{suffix}"));
                    if sub_dst != comp_abs && !sub_dst.exists() {
                        let _ = std::fs::rename(&comp_abs, &sub_dst);
                    }
                } else if dst_abs_clone != src_abs {
                    // Preserve existing local poster/fanart/thumb when online download isn't replacing that slot
                    let image_ext = |p_lower: &str| -> &'static str {
                        if p_lower.ends_with(".png") {
                            "png"
                        } else if p_lower.ends_with(".webp") {
                            "webp"
                        } else {
                            "jpg"
                        }
                    };
                    if lower.contains("-poster.") && (!apply_cover || skip_poster) {
                        let target_poster_dir = if meta.is_episode { &show_dir } else { &dst_dir };
                        let migrated =
                            target_poster_dir.join(format!("poster.{}", image_ext(&lower)));
                        if !migrated.exists() {
                            let _ = std::fs::copy(&comp_abs, &migrated);
                        }
                    } else if lower.contains("-fanart.") && (!apply_cover || skip_fanart) {
                        let target_fan_dir = if meta.is_episode { &show_dir } else { &dst_dir };
                        let migrated = target_fan_dir.join(format!("fanart.{}", image_ext(&lower)));
                        if !migrated.exists() {
                            let _ = std::fs::copy(&comp_abs, &migrated);
                        }
                    } else if lower.contains("-thumb.") && (!apply_cover || skip_thumb) {
                        let migrated =
                            dst_dir.join(format!("{target_stem}-thumb.{}", image_ext(&lower)));
                        if !migrated.exists() {
                            let _ = std::fs::copy(&comp_abs, &migrated);
                        }
                    }
                    // Clean up flat legacy sidecars (`<old>.nfo`, `<old>-poster.*`, `<old>-fanart.*`, `<old>-thumb.*`)
                    let _ = std::fs::remove_file(&comp_abs);
                }
            }

            if !meta.is_episode {
                // Jellyfin Movie layout: `<Movie Dir>/movie.nfo`, `poster.jpg`, `fanart.jpg`
                let movie_nfo = dst_dir.join("movie.nfo");
                let xml = render_video_nfo_xml(&meta);
                atomic_write_bytes(&movie_nfo, xml.as_bytes())?;

                let same_stem_nfo = dst_dir.join(format!("{target_stem}.nfo"));
                if same_stem_nfo.is_file() && same_stem_nfo != movie_nfo {
                    let _ = std::fs::remove_file(same_stem_nfo);
                }

                if !skip_poster
                    && let Some(u) = scraped_clone.cover_url.as_deref().map(str::trim)
                    && let Some((bytes, img_ext)) = downloaded_images.get(u)
                {
                    write_image_replacing_opposite_ext(&dst_dir, "poster", img_ext, bytes)?;
                }
                if !skip_fanart
                    && let Some(u) = scraped_clone.backdrop_url.as_deref().map(str::trim)
                    && let Some((bytes, img_ext)) = downloaded_images.get(u)
                {
                    write_image_replacing_opposite_ext(&dst_dir, "fanart", img_ext, bytes)?;
                }
            } else {
                // Jellyfin 3-Tier TV Show layout:
                // 1) Series root (`show_dir`): `tvshow.nfo`, `poster.jpg`, `fanart.jpg`
                let tvshow_nfo_path = show_dir.join("tvshow.nfo");
                if !only_fill_missing || !tvshow_nfo_path.is_file() {
                    let show_plot = scraped_clone
                        .show_summary
                        .as_deref()
                        .or(meta.summary.as_deref());
                    let tvshow_xml = render_tvshow_nfo_xml(&meta, show_plot);
                    atomic_write_bytes(&tvshow_nfo_path, tvshow_xml.as_bytes())?;
                }

                if !skip_poster
                    && let Some(u) = scraped_clone.cover_url.as_deref().map(str::trim)
                    && let Some((bytes, img_ext)) = downloaded_images.get(u)
                {
                    write_image_replacing_opposite_ext(&show_dir, "poster", img_ext, bytes)?;
                }
                if !skip_fanart
                    && let Some(u) = scraped_clone.backdrop_url.as_deref().map(str::trim)
                    && let Some((bytes, img_ext)) = downloaded_images.get(u)
                {
                    write_image_replacing_opposite_ext(&show_dir, "fanart", img_ext, bytes)?;
                }

                // 2) Season folder (`dst_dir` = `Season XX`): `season.nfo`, `poster.jpg`, `fanart.jpg`
                let season_num = meta.season_number.unwrap_or(1);
                let season_nfo_path = dst_dir.join("season.nfo");
                if !only_fill_missing || !season_nfo_path.is_file() {
                    let season_plot = scraped_clone
                        .season_summary
                        .as_deref()
                        .or(scraped_clone.show_summary.as_deref())
                        .or(meta.summary.as_deref());
                    let season_xml = render_season_nfo_xml(
                        season_num,
                        scraped_clone.season_title.as_deref(),
                        meta.year,
                        season_plot,
                        meta.tmdb_id.as_deref(),
                        meta.bangumi_id.as_deref(),
                    );
                    atomic_write_bytes(&season_nfo_path, season_xml.as_bytes())?;
                }

                if dst_dir != show_dir {
                    let season_poster_u = scraped_clone
                        .season_poster_url
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .or_else(|| scraped_clone.cover_url.as_deref().map(str::trim));
                    if !skip_poster
                        && let Some(u) = season_poster_u
                        && let Some((bytes, img_ext)) = downloaded_images.get(u)
                    {
                        write_image_replacing_opposite_ext(&dst_dir, "poster", img_ext, bytes)?;
                    }
                    if !skip_fanart
                        && let Some(u) = scraped_clone.backdrop_url.as_deref().map(str::trim)
                        && let Some((bytes, img_ext)) = downloaded_images.get(u)
                    {
                        write_image_replacing_opposite_ext(&dst_dir, "fanart", img_ext, bytes)?;
                    }
                }

                // 3) Episode level (`Season XX/<target_stem>.nfo` + `<target_stem>-thumb.jpg`)
                let ep_nfo = dst_dir.join(format!("{target_stem}.nfo"));
                let ep_xml = render_video_nfo_xml(&meta);
                atomic_write_bytes(&ep_nfo, ep_xml.as_bytes())?;

                if !skip_thumb
                    && let Some(u) = scraped_clone.episode_still_url.as_deref().map(str::trim)
                    && let Some((bytes, img_ext)) = downloaded_images.get(u)
                {
                    let thumb_base = format!("{target_stem}-thumb");
                    write_image_replacing_opposite_ext(&dst_dir, &thumb_base, img_ext, bytes)?;
                }
            }

            if let Some(ref prev_dir) = old_parent {
                cleanup_empty_parent_dirs(prev_dir, &root_path_buf);
            }

            Ok(())
        })
        .await
        .map_err(|e| DomainError::Storage(format!("video Jellyfin archive task failed: {e}")))??;

        self.workbench
            .inspect_file(root_id, &target_rel_path, Some(MediaKind::Video))
            .await
    }

    pub async fn proxy_scraper_image(&self, raw_url: &str) -> DomainResult<(Vec<u8>, String)> {
        let parsed = reqwest::Url::parse(raw_url.trim())
            .map_err(|_| DomainError::Validation("invalid image URL".into()))?;
        if parsed.scheme() != "https" || !parsed.username().is_empty() || parsed.port().is_some() {
            return Err(DomainError::Validation(
                "disallowed image URL scheme or port".into(),
            ));
        }
        let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
        let allowed_domain = host.ends_with(".doubanio.com")
            || host == "xinhuashudian.com"
            || host.ends_with(".xinhuashudian.com")
            || host == "nhentai.net"
            || host.ends_with(".nhentai.net")
            || host == "hanime1.me"
            || host.ends_with(".hanime1.me")
            || host == "hembed.com"
            || host.ends_with(".hembed.com")
            || host.ends_with(".hanime1.com");
        let allowed = allowed_domain
            && host
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-');
        if !allowed {
            return Err(DomainError::Validation(format!(
                "host '{host}' is not on the scraper image proxy allowlist"
            )));
        }
        let creds = self.settings.get_credentials().await?;
        let client = build_http_client(&creds)?;
        download_image_bytes(&client, parsed.as_str()).await
    }

    /// Downloads a remote artwork image (e.g. from a scraper candidate) using configured proxy/UA
    /// and writes it into the target media file via the workbench.
    pub async fn apply_remote_artwork(
        &self,
        root_id: RootId,
        rel_path: &str,
        raw_url: &str,
        slot: &str,
        preferred_kind: Option<MediaKind>,
    ) -> DomainResult<MediaFileDetail> {
        let parsed = reqwest::Url::parse(raw_url.trim())
            .map_err(|_| DomainError::Validation("invalid artwork URL".into()))?;
        if !matches!(parsed.scheme(), "http" | "https") || !parsed.username().is_empty() {
            return Err(DomainError::Validation(
                "artwork URL must use http or https".into(),
            ));
        }
        let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
        if host.is_empty()
            || host == "localhost"
            || host.ends_with(".local")
            || host.starts_with("127.")
            || host.starts_with("10.")
            || host.starts_with("192.168.")
            || host.starts_with("169.254.")
            || host == "::1"
            || host == "[::1]"
        {
            return Err(DomainError::Validation(
                "disallowed local/private artwork host".into(),
            ));
        }
        let creds = self.settings.get_credentials().await?;
        let client = build_http_client(&creds)?;
        let (bytes, mime) = download_image_bytes(&client, parsed.as_str()).await?;
        self.workbench
            .write_artwork(root_id, rel_path, &bytes, &mime, slot, preferred_kind)
            .await
    }
}

pub(crate) fn compute_scrape_confidence(query: &ScrapeQuery, agg: &AggregateScrapeResponse) -> u8 {
    let meta = &agg.merged.metadata;
    let matched = meta.title.as_deref().is_some_and(|s| !s.trim().is_empty());
    let mut score: i32 = if matched { 45 } else { 0 };
    let kw = query.keyword.trim().to_lowercase();
    if let Some(ref mt) = meta.title {
        let b = mt.trim().to_lowercase();
        if !kw.is_empty() && !b.is_empty() {
            if kw == b {
                score += 35;
            } else if b.contains(&kw) || kw.contains(&b) {
                score += 22;
            }
        }
    }
    if let Some(qa) = query.artist_or_author.as_deref().map(str::trim)
        && !qa.is_empty()
    {
        let qa_low = qa.to_lowercase();
        let creator_match = meta.creators.iter().any(|c| {
            let cl = c.trim().to_lowercase();
            !cl.is_empty() && (cl == qa_low || cl.contains(&qa_low) || qa_low.contains(&cl))
        });
        if creator_match {
            score += 12;
        }
    }
    if let (Some(qy), Some(my)) = (query.year, meta.year)
        && (qy - my).abs() <= 1
    {
        score += 10;
    }
    if agg.details_by_provider.len() >= 2 {
        score += 10;
    }
    score.clamp(0, 100) as u8
}

pub(crate) fn filter_scraped_metadata_by_groups(
    mut meta: ScrapedMetadata,
    groups: &[String],
) -> ScrapedMetadata {
    if groups.is_empty() {
        return meta;
    }
    let has = |g: &str| groups.iter().any(|item| item.eq_ignore_ascii_case(g));
    if !has("basic") {
        meta.title = None;
        meta.original_title = None;
        meta.series_or_album = None;
        meta.creators.clear();
        meta.illustrators_or_directors.clear();
        meta.cast.clear();
        meta.publisher_or_studio = None;
        meta.year = None;
        meta.published_date = None;
        meta.language = None;
        meta.isbn = None;
        meta.rating = None;
        meta.track_or_episode_number = None;
        meta.disc_or_season_number = None;
    }
    if !has("summary") {
        meta.summary = None;
        meta.show_summary = None;
        meta.season_title = None;
        meta.season_summary = None;
    }
    if !has("tags") {
        meta.genres.clear();
        meta.tags.clear();
    }
    if !has("lyrics") {
        meta.synced_lyrics = None;
        meta.plain_lyrics = None;
    }
    meta
}

fn is_webnovel_serial_only_provider(id: &str) -> bool {
    matches!(
        id,
        "qidian" | "fanqie" | "qimao" | "17k" | "ciweimao" | "sfacg" | "faloo" | "jjwxc"
    )
}

async fn download_image_bytes(
    client: &reqwest::Client,
    url: &str,
) -> DomainResult<(Vec<u8>, String)> {
    let mut req = client.get(url);
    if url.contains(".doubanio.com/") {
        req = req.header(reqwest::header::REFERER, "https://book.douban.com/");
    } else if url.contains(".xinhuashudian.com/") || url.contains("://xinhuashudian.com/") {
        req = req.header(reqwest::header::REFERER, "https://www.xhsd.com/");
    } else if url.contains(".nhentai.net/") || url.contains("://nhentai.net/") {
        req = req.header(reqwest::header::REFERER, "https://nhentai.net/");
    } else if url.contains("hanime1.me/")
        || url.contains(".hembed.com/")
        || url.contains(".hanime1.com/")
    {
        req = req.header(reqwest::header::REFERER, "https://hanime1.me/");
    }
    let resp = req
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

fn has_lrc_timestamps(text: &str) -> bool {
    text.lines().any(|line| {
        let b = line.trim_start().as_bytes();
        b.len() >= 6
            && b[0] == b'['
            && b[1].is_ascii_digit()
            && b[2].is_ascii_digit()
            && b[3] == b':'
            && b[4].is_ascii_digit()
    })
}

fn existing_artwork_present(meta: &MediaMetadataPayload) -> bool {
    match meta {
        MediaMetadataPayload::Book(b) => b.has_cover,
        MediaMetadataPayload::Comic(c) => c.has_cover,
        MediaMetadataPayload::Music(m) => m.has_embedded_cover,
        MediaMetadataPayload::Video(v) => v.has_poster,
    }
}

fn non_empty_opt(s: &Option<String>) -> bool {
    s.as_ref().is_some_and(|v| !v.trim().is_empty())
}

fn filter_video_patch_for_missing(
    vp: &mut VideoMetaPatch,
    cur: &crate::domain::media::VideoFileMeta,
    fallback_stem: &str,
) {
    if cur.has_nfo && !cur.title.trim().is_empty() && cur.title.trim() != fallback_stem {
        vp.title = None;
    }
    if cur.has_nfo {
        vp.is_episode = None;
    }
    if non_empty_opt(&cur.original_title) {
        vp.original_title = None;
    }
    if non_empty_opt(&cur.show_title) {
        vp.show_title = None;
    }
    if cur.season_number.is_some() {
        vp.season_number = None;
    }
    if cur.episode_number.is_some() {
        vp.episode_number = None;
    }
    if cur.year.is_some() {
        vp.year = None;
    }
    if cur.rating.is_some() {
        vp.rating = None;
    }
    if non_empty_opt(&cur.summary) {
        vp.summary = None;
    }
    if !cur.genres.is_empty() {
        vp.genres = None;
    }
    if !cur.tags.is_empty() {
        vp.tags = None;
    }
    if !cur.actors.is_empty() {
        vp.actors = None;
    }
    if !cur.directors.is_empty() {
        vp.directors = None;
    }
    if non_empty_opt(&cur.studio) {
        vp.studio = None;
    }
    if non_empty_opt(&cur.tmdb_id) {
        vp.tmdb_id = None;
    }
    if non_empty_opt(&cur.bangumi_id) {
        vp.bangumi_id = None;
    }
}

fn filter_patch_for_missing(
    patch: &mut MediaMetadataPatch,
    current: &MediaMetadataPayload,
    fallback_stem: &str,
) {
    match (patch, current) {
        (MediaMetadataPatch::Book(bp), MediaMetadataPayload::Book(b)) => {
            let has_tag_block = !b.authors.is_empty()
                || b.publisher.is_some()
                || b.isbn.is_some()
                || !b.tags.is_empty()
                || b.description.is_some();
            if !b.title.trim().is_empty() && (b.title.trim() != fallback_stem || has_tag_block) {
                bp.title = None;
            }
            if !b.authors.is_empty() {
                bp.authors = None;
            }
            if non_empty_opt(&b.series) {
                bp.series = None;
            }
            if non_empty_opt(&b.subseries) {
                bp.subseries = None;
            }
            if non_empty_opt(&b.series_index) {
                bp.series_index = None;
            }
            if non_empty_opt(&b.description) {
                bp.description = None;
            }
            if non_empty_opt(&b.publisher) {
                bp.publisher = None;
            }
            if non_empty_opt(&b.published_at) {
                bp.published_at = None;
            }
            if non_empty_opt(&b.language) {
                bp.language = None;
            }
            if non_empty_opt(&b.isbn) {
                bp.isbn = None;
            }
            if !b.tags.is_empty() {
                bp.tags = None;
            }
        }
        (MediaMetadataPatch::Comic(cp), MediaMetadataPayload::Comic(c)) => {
            if non_empty_opt(&c.series) {
                cp.series = None;
            }
            if non_empty_opt(&c.series_group) {
                cp.series_group = None;
            }
            if non_empty_opt(&c.title) {
                cp.title = None;
            }
            if non_empty_opt(&c.number) {
                cp.number = None;
            }
            if c.volume.is_some() {
                cp.volume = None;
            }
            if non_empty_opt(&c.summary) {
                cp.summary = None;
            }
            if !c.writers.is_empty() {
                cp.writers = None;
            }
            if !c.pencillers.is_empty() {
                cp.pencillers = None;
            }
            if non_empty_opt(&c.publisher) {
                cp.publisher = None;
            }
            if !c.genres.is_empty() {
                cp.genres = None;
            }
            if !c.tags.is_empty() {
                cp.tags = None;
            }
            if c.year.is_some() {
                cp.year = None;
            }
            if non_empty_opt(&c.language_iso) {
                cp.language_iso = None;
            }
        }
        (MediaMetadataPatch::Music(mp), MediaMetadataPayload::Music(m)) => {
            let has_tags = !m.artist.trim().is_empty()
                || !m.album.trim().is_empty()
                || m.track_number.is_some()
                || m.year.is_some();
            if !m.title.trim().is_empty() && (m.title.trim() != fallback_stem || has_tags) {
                mp.title = None;
            }
            if !m.artist.trim().is_empty() {
                mp.artist = None;
            }
            if non_empty_opt(&m.album_artist) {
                mp.album_artist = None;
            }
            if !m.album.trim().is_empty() {
                mp.album = None;
            }
            if m.track_number.is_some() {
                mp.track_number = None;
            }
            if m.disc_number.is_some() {
                mp.disc_number = None;
            }
            if m.year.is_some() {
                mp.year = None;
            }
            if non_empty_opt(&m.genre) {
                mp.genre = None;
            }
            if non_empty_opt(&m.embedded_lyrics) {
                mp.embedded_lyrics = None;
            }
            if non_empty_opt(&m.sidecar_lrc) {
                mp.sidecar_lrc = None;
            }
        }
        (MediaMetadataPatch::Video(vp), MediaMetadataPayload::Video(v)) => {
            filter_video_patch_for_missing(vp, v, fallback_stem);
        }
        _ => {}
    }
}

fn map_scraped_to_patch(
    media_kind: MediaKind,
    s: &ScrapedMetadata,
    write_sidecar_lrc: bool,
    existing_music_artist_non_empty: bool,
) -> MediaMetadataPatch {
    match media_kind {
        MediaKind::Book => MediaMetadataPatch::Book(BookMetaPatch {
            title: s.title.clone(),
            authors: (!s.creators.is_empty()).then(|| s.creators.clone()),
            series: s.series_or_album.clone(),
            subseries: None,
            series_index: s.track_or_episode_number.map(|n| n.to_string()),
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
            locked_fields: None,
        }),
        MediaKind::Comic => {
            let is_chapter_scrape = s.track_or_episode_number.is_some();
            MediaMetadataPatch::Comic(ComicMetaPatch {
                series: s.series_or_album.clone().or_else(|| s.title.clone()),
                series_group: None,
                title: is_chapter_scrape.then(|| s.title.clone()).flatten(),
                number: s.track_or_episode_number.map(|n| n.to_string()),
                volume: is_chapter_scrape
                    .then(|| s.disc_or_season_number.and_then(|v| i32::try_from(v).ok()))
                    .flatten(),
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
            })
        }
        MediaKind::Music => {
            let best_lyrics = s.synced_lyrics.clone().or_else(|| s.plain_lyrics.clone());
            let sidecar_lrc = if write_sidecar_lrc {
                s.synced_lyrics
                    .as_deref()
                    .filter(|l| has_lrc_timestamps(l))
                    .map(str::to_string)
            } else {
                None
            };
            if s.target == Some(ScrapeTargetKind::Lyrics) {
                MediaMetadataPatch::Music(MusicMetaPatch {
                    embedded_lyrics: best_lyrics,
                    sidecar_lrc,
                    ..Default::default()
                })
            } else if s.target == Some(ScrapeTargetKind::MusicAlbum) {
                MediaMetadataPatch::Music(MusicMetaPatch {
                    title: None,
                    artist: if existing_music_artist_non_empty {
                        None
                    } else {
                        s.creators.first().cloned()
                    },
                    album_artist: (!s.creators.is_empty()).then(|| s.creators.join(", ")),
                    album: s.series_or_album.clone().or_else(|| s.title.clone()),
                    track_number: None,
                    disc_number: None,
                    year: s.year,
                    genre: s.genres.first().cloned(),
                    comment: None,
                    embedded_lyrics: None,
                    sidecar_lrc: None,
                })
            } else {
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
                    embedded_lyrics: best_lyrics,
                    sidecar_lrc,
                })
            }
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
                season_number: s.disc_or_season_number.and_then(|v| i32::try_from(v).ok()),
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
                ..Default::default()
            })
        }
    }
}

fn strip_trailing_year_tag(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some(idx) = trimmed.rfind(['(', '[']) {
        let suffix = trimmed[idx..].trim();
        if suffix.len() == 6
            && (suffix.ends_with(')') || suffix.ends_with(']'))
            && suffix[1..5]
                .parse::<i32>()
                .is_ok_and(|y| (1900..=2099).contains(&y))
        {
            return trimmed[..idx].trim().to_string();
        }
    }
    trimmed.to_string()
}

fn strip_comic_chapter_suffix(raw: &str) -> String {
    let cleaned = raw.replace('_', " ");
    let mut s = cleaned.trim();
    while s.starts_with('[') || s.starts_with('(') {
        let close = if s.starts_with('[') { ']' } else { ')' };
        if let Some(pos) = s.find(close) {
            s = s[pos + 1..].trim();
        } else {
            break;
        }
    }
    let lower = s.to_ascii_lowercase();
    for marker in [" vol.", " ch.", " chapter ", " 第"] {
        if let Some(pos) = lower.find(marker) {
            let head = s[..pos].trim();
            if !head.is_empty() {
                return head.to_string();
            }
        }
    }
    s.to_string()
}

fn build_auto_query_from_detail(d: &MediaFileDetail) -> ScrapeQuery {
    match &d.metadata {
        MediaMetadataPayload::Music(m) => ScrapeQuery {
            target: Some(ScrapeTargetKind::MusicTrack),
            keyword: if m.title.trim().is_empty() {
                d.entry.display_title.clone()
            } else {
                m.title.trim().to_string()
            },
            artist_or_author: (!m.artist.trim().is_empty()).then(|| m.artist.trim().to_string()),
            album_or_series: (!m.album.trim().is_empty()).then(|| m.album.trim().to_string()),
            year: m.year,
            duration_secs: (m.duration_secs > 0).then_some(m.duration_secs),
            ..Default::default()
        },
        MediaMetadataPayload::Video(v) => {
            let raw_kw = if v.is_episode {
                v.show_title
                    .as_deref()
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or_else(|| {
                        if v.title.trim().is_empty() {
                            d.entry.display_title.as_str()
                        } else {
                            v.title.as_str()
                        }
                    })
            } else if v.title.trim().is_empty() {
                d.entry.display_title.as_str()
            } else {
                v.title.as_str()
            };
            ScrapeQuery {
                target: Some(if v.is_episode {
                    ScrapeTargetKind::TvShow
                } else {
                    ScrapeTargetKind::Movie
                }),
                keyword: strip_trailing_year_tag(raw_kw),
                year: v.year,
                season: v.season_number,
                episode: v.episode_number,
                duration_secs: (v.duration_secs > 0).then_some(v.duration_secs),
                ..Default::default()
            }
        }
        MediaMetadataPayload::Comic(c) => {
            let title_or_disp = c
                .title
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or(&d.entry.display_title);
            let keyword = c
                .series
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| strip_comic_chapter_suffix(title_or_disp));
            ScrapeQuery {
                target: Some(ScrapeTargetKind::Comic),
                keyword,
                artist_or_author: c.writers.first().cloned(),
                year: c.year,
                ..Default::default()
            }
        }
        MediaMetadataPayload::Book(b) => {
            let parsed_year = b
                .published_at
                .as_deref()
                .and_then(|s| s.get(0..4))
                .and_then(|y| y.parse::<i32>().ok());
            ScrapeQuery {
                target: Some(ScrapeTargetKind::Book),
                keyword: if b.title.trim().is_empty() {
                    d.entry.display_title.clone()
                } else {
                    b.title.trim().to_string()
                },
                artist_or_author: b.authors.first().cloned(),
                album_or_series: b.series.clone(),
                isbn: b.isbn.clone(),
                year: parsed_year,
                ..Default::default()
            }
        }
    }
}

fn fallback_metadata_from_candidate(cand: &ScrapeCandidate) -> ScrapedMetadata {
    let mut creators: Vec<String> = Vec::new();
    let mut genres: Vec<String> = Vec::new();
    let mut series_or_album: Option<String> = None;
    let mut publisher_or_studio: Option<String> = None;
    let mut language: Option<String> = None;

    const CN_NOVEL_PROVIDERS: &[&str] = &[
        "qidian", "fanqie", "qimao", "zongheng", "jjwxc", "sfacg", "ciweimao", "faloo",
    ];
    const MUSIC_PROVIDERS: &[&str] = &[
        "qq_music",
        "netease",
        "kugou",
        "kuwo",
        "taihe",
        "qishui",
        "musicbrainz",
        "itunes",
        "lrclib",
    ];

    if let Some(sub) = cand
        .subtitle
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let parts: Vec<&str> = sub
            .split(" · ")
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        if CN_NOVEL_PROVIDERS.contains(&cand.provider_id.as_str()) {
            if let Some(first) = parts.first() {
                creators.push((*first).to_string());
            }
            if let Some(second) = parts.get(1) {
                genres.push((*second).to_string());
            }
        } else if MUSIC_PROVIDERS.contains(&cand.provider_id.as_str()) {
            if let Some(first) = parts.first() {
                for artist in first
                    .split(['/', '、'])
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    if !creators.iter().any(|c| c == artist) {
                        creators.push(artist.to_string());
                    }
                }
            }
            if let Some(second) = parts.get(1) {
                series_or_album = Some((*second).to_string());
            }
        } else if matches!(
            cand.target,
            ScrapeTargetKind::Book | ScrapeTargetKind::Comic
        ) {
            if let Some(first) = parts.first() {
                for author in first.split(" / ").map(str::trim).filter(|s| !s.is_empty()) {
                    if !creators.iter().any(|c| c == author) {
                        creators.push(author.to_string());
                    }
                }
            }
            if let Some(second) = parts.get(1) {
                publisher_or_studio = Some((*second).to_string());
            }
        }
    }

    if CN_NOVEL_PROVIDERS.contains(&cand.provider_id.as_str()) {
        if publisher_or_studio.is_none() && !cand.provider_name.trim().is_empty() {
            publisher_or_studio = Some(cand.provider_name.clone());
        }
        language = Some("zh-CN".to_string());
    }

    let mut external_ids = BTreeMap::new();
    if !cand.provider_id.trim().is_empty() && !cand.external_id.trim().is_empty() {
        external_ids.insert(cand.provider_id.clone(), cand.external_id.clone());
    }

    ScrapedMetadata {
        provider_id: cand.provider_id.clone(),
        external_id: cand.external_id.clone(),
        target: Some(cand.target),
        title: Some(cand.title.clone()),
        series_or_album,
        creators,
        publisher_or_studio,
        year: cand.year,
        language,
        summary: cand.summary.clone(),
        genres,
        cover_url: cand.cover_url.clone(),
        synced_lyrics: cand.synced_lyrics.clone(),
        plain_lyrics: cand.plain_lyrics.clone(),
        external_ids,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::scraper::MergedScrapedResult;

    #[test]
    fn compute_confidence_rewards_exact_title_creator_and_year() {
        let query = ScrapeQuery {
            keyword: "三体".into(),
            artist_or_author: Some("刘慈欣".into()),
            year: Some(2008),
            ..Default::default()
        };
        let agg = AggregateScrapeResponse {
            provider_order: vec![],
            candidates: vec![],
            selected_by_provider: BTreeMap::new(),
            details_by_provider: BTreeMap::new(),
            merged: MergedScrapedResult {
                metadata: ScrapedMetadata {
                    title: Some("三体".into()),
                    creators: vec!["刘慈欣".into()],
                    year: Some(2008),
                    ..Default::default()
                },
                field_sources: BTreeMap::new(),
            },
        };
        assert!(compute_scrape_confidence(&query, &agg) >= 90);

        let unrelated = AggregateScrapeResponse {
            merged: MergedScrapedResult {
                metadata: ScrapedMetadata {
                    title: Some("完全不同的书".into()),
                    ..Default::default()
                },
                field_sources: BTreeMap::new(),
            },
            ..agg
        };
        assert!(compute_scrape_confidence(&query, &unrelated) < 70);
    }

    #[test]
    fn filter_metadata_by_groups_preserves_selected_groups_only() {
        let raw = ScrapedMetadata {
            title: Some("Test".into()),
            creators: vec!["Author".into()],
            summary: Some("Plot".into()),
            tags: vec!["Sci-Fi".into()],
            synced_lyrics: Some("[00:01.00]Hello".into()),
            cover_url: Some("https://example.com/c.jpg".into()),
            ..Default::default()
        };
        let only_summary_and_tags =
            filter_scraped_metadata_by_groups(raw.clone(), &["summary".into(), "tags".into()]);
        assert!(only_summary_and_tags.title.is_none());
        assert!(only_summary_and_tags.creators.is_empty());
        assert_eq!(only_summary_and_tags.summary.as_deref(), Some("Plot"));
        assert_eq!(only_summary_and_tags.tags, vec!["Sci-Fi"]);
        assert!(only_summary_and_tags.synced_lyrics.is_none());
        assert_eq!(
            only_summary_and_tags.cover_url.as_deref(),
            Some("https://example.com/c.jpg")
        );

        let empty_means_all = filter_scraped_metadata_by_groups(raw, &[]);
        assert_eq!(empty_means_all.title.as_deref(), Some("Test"));
        assert!(empty_means_all.synced_lyrics.is_some());
    }

    #[test]
    fn fallback_metadata_from_candidate_preserves_novel_author_and_category() {
        let cand = ScrapeCandidate {
            provider_id: "fanqie".into(),
            provider_name: "番茄小说".into(),
            external_id: "7171641987538357284".into(),
            target: ScrapeTargetKind::Book,
            title: "测试小说".into(),
            subtitle: Some("测试作者 · 玄幻脑洞".into()),
            year: Some(2023),
            summary: Some("一段简介".into()),
            cover_url: Some("https://example.com/c.jpg".into()),
            synced_lyrics: None,
            plain_lyrics: None,
        };
        let meta = fallback_metadata_from_candidate(&cand);
        assert_eq!(meta.title.as_deref(), Some("测试小说"));
        assert_eq!(meta.creators, vec!["测试作者"]);
        assert_eq!(meta.genres, vec!["玄幻脑洞"]);
        assert_eq!(meta.publisher_or_studio.as_deref(), Some("番茄小说"));
        assert_eq!(meta.language.as_deref(), Some("zh-CN"));
        assert_eq!(
            meta.external_ids.get("fanqie").map(String::as_str),
            Some("7171641987538357284")
        );
    }
}

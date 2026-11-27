use std::collections::BTreeMap;
use std::sync::Arc;

use tokio::task::JoinSet;
use tracing::warn;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;

use super::mapping::{
    build_auto_query_from_detail, compute_scrape_confidence, fallback_metadata_from_candidate,
    filter_scraped_metadata_by_groups, is_webnovel_serial_only_provider,
};
use super::{
    ApplyScrapeInput, AutoScrapeMatchMode, AutoScrapeOptions, AutoScrapeResultDto,
    DryRunScrapeItemDto, ScrapeApplyService, ScraperCatalogService,
};
use crate::application::ports::{
    ScrapeCacheLookup, ScrapeCacheValue, ScrapeCacheWrite, ScraperProviderPort,
};
use crate::domain::config::{ScraperPriorities, ToolSettings};
use crate::domain::scraper::{
    AggregateScrapeResponse, FieldSourceBadge, ProviderBriefDto, ProviderStatusDto,
    ScrapeCandidate, ScrapeQuery, ScrapeTargetKind, ScrapedMetadata,
    is_compatible_secondary_source, merge_scraped_by_priority, promote_exact_match_anchor,
};
use crate::domain::workspace::RootId;

impl ScraperCatalogService {
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

    fn select_providers_for_query(
        &self,
        raw_settings: &ToolSettings,
        query: &ScrapeQuery,
    ) -> Vec<Arc<dyn ScraperProviderPort>> {
        let domain = query.target.map(ScrapeTargetKind::media_kind);
        let ordered = self.sort_providers_by_priority(
            domain,
            &raw_settings.scraper_priorities,
            &query.providers,
        );
        let has_isbn_hint = query.isbn.as_deref().is_some_and(|s| !s.trim().is_empty());

        ordered
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
            .collect()
    }

    async fn search_selected_providers(
        &self,
        selected: &[Arc<dyn ScraperProviderPort>],
        raw_settings: &ToolSettings,
        query: &ScrapeQuery,
        effective_locale: &str,
    ) -> Vec<Vec<ScrapeCandidate>> {
        let ttl_ms = raw_settings.scrape_cache_ttl_ms;
        let provider_count = selected.len();
        let mut set = JoinSet::new();
        for (order_idx, provider) in selected.iter().cloned().enumerate() {
            let id = provider.descriptor().id;
            let loc = raw_settings.locale_for_provider(&id, effective_locale);
            let mut q = query.clone();
            q.locale = Some(loc.clone());
            let c = raw_settings.credentials_for_provider(&id);
            let cache = self.cache.clone();
            let transport = self.transport.clone();
            set.spawn(async move {
                if let Some(ScrapeCacheValue::Search(cached)) = cache.get(
                    ScrapeCacheLookup::Search {
                        provider_id: &id,
                        query: &q,
                        locale: &loc,
                    },
                    ttl_ms,
                ) {
                    return (order_idx, cached);
                }
                transport.acquire_rate_slot(&id).await;
                let items = match provider.search(&q, &c).await {
                    Ok(items) => {
                        cache.put(
                            ScrapeCacheWrite::Search {
                                provider_id: &id,
                                query: &q,
                                locale: &loc,
                                items: &items,
                            },
                            ttl_ms,
                        );
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
        per_provider
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

        let selected = self.select_providers_for_query(&raw_settings, &query);
        if selected.is_empty() {
            return Ok(Vec::new());
        }

        let per_provider = self
            .search_selected_providers(&selected, &raw_settings, &query, &effective_locale)
            .await;
        Ok(interleave_provider_candidates(&per_provider))
    }

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

        let selected = self.select_providers_for_query(&raw_settings, &query);
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

        let provider_count = selected.len();
        let per_provider = self
            .search_selected_providers(&selected, &raw_settings, &query, &effective_locale)
            .await;

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
            let transport = self.transport.clone();
            detail_set.spawn(async move {
                let id = prov_desc.id.clone();
                if let Some(ScrapeCacheValue::Detail(cached)) = cache.get(
                    ScrapeCacheLookup::Detail {
                        provider_id: &id,
                        target: top_cand.target,
                        external_id: &top_cand.external_id,
                        locale: &loc,
                    },
                    ttl_ms,
                ) {
                    return (order_idx, id, prov_desc.name, *cached);
                }
                transport.acquire_rate_slot(&id).await;
                let detail = match provider
                    .fetch_detail(&top_cand.external_id, top_cand.target, &loc, &c)
                    .await
                {
                    Ok(d) => {
                        cache.put(
                            ScrapeCacheWrite::Detail {
                                provider_id: &id,
                                target: top_cand.target,
                                external_id: &top_cand.external_id,
                                locale: &loc,
                                detail: &d,
                            },
                            ttl_ms,
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
        let combined = interleave_provider_candidates(&per_provider);

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
        if let Some(ScrapeCacheValue::Detail(cached)) = self.cache.get(
            ScrapeCacheLookup::Detail {
                provider_id,
                target,
                external_id,
                locale: &effective_locale,
            },
            ttl_ms,
        ) {
            return Ok(*cached);
        }

        self.transport.acquire_rate_slot(provider_id).await;
        let detail = provider
            .fetch_detail(external_id, target, &effective_locale, &creds)
            .await?;
        self.cache.put(
            ScrapeCacheWrite::Detail {
                provider_id,
                target,
                external_id,
                locale: &effective_locale,
                detail: &detail,
            },
            ttl_ms,
        );
        Ok(detail)
    }
}

fn interleave_provider_candidates(per_provider: &[Vec<ScrapeCandidate>]) -> Vec<ScrapeCandidate> {
    let max_len = per_provider.iter().map(Vec::len).max().unwrap_or(0);
    let total_cap: usize = per_provider.iter().map(Vec::len).sum();
    let mut combined = Vec::with_capacity(total_cap);
    for row in 0..max_len {
        for bucket in per_provider {
            if let Some(item) = bucket.get(row) {
                combined.push(item.clone());
            }
        }
    }
    combined
}

impl ScrapeApplyService {
    pub async fn dry_run(
        &self,
        root_id: RootId,
        paths: Vec<String>,
        locale: Option<String>,
    ) -> DomainResult<Vec<DryRunScrapeItemDto>> {
        let mut results = Vec::with_capacity(paths.len());
        for rel_path in paths {
            let detail = match self.workspace.inspect(root_id, &rel_path, None).await {
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
            match self.catalog.aggregate(query.clone()).await {
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

    pub async fn auto_scrape(
        &self,
        root_id: RootId,
        rel_path: &str,
        media_kind: MediaKind,
        opts: AutoScrapeOptions,
    ) -> DomainResult<AutoScrapeResultDto> {
        let local_detail = self
            .workspace
            .inspect(root_id, rel_path, Some(media_kind))
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

        let agg = self.catalog.aggregate(query.clone()).await?;
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
            .apply(ApplyScrapeInput {
                root_id,
                rel_path: rel_path.to_string(),
                media_kind,
                scraped: filtered_meta,
                apply_cover: opts.apply_cover,
                write_sidecar_lrc: opts.write_sidecar_lrc,
                organize_by_template: Some(opts.organize_by_template),
                only_fill_missing: opts.only_fill_missing,
                write_album_cover: false,
            })
            .await?;

        Ok(AutoScrapeResultDto {
            matched: true,
            confidence,
            detail: updated,
        })
    }
}

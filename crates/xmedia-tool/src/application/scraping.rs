use std::collections::{BTreeMap, HashMap};
use std::path::Path;
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
    BookMetaPatch, ComicMetaPatch, MediaFileDetail, MediaMetadataPatch, MediaMetadataPayload,
    MusicMetaPatch, VideoMetaPatch,
};
use crate::domain::ports::ScraperProviderPort;
use crate::domain::scraper::{
    AggregateScrapeResponse, FieldSourceBadge, ProviderBriefDto, ProviderStatusDto,
    ScrapeCandidate, ScrapeQuery, ScrapeTargetKind, ScrapedMetadata, merge_scraped_by_priority,
};
use crate::domain::template::render_template_target;
use crate::domain::workspace::RootId;
use crate::infra::atomic_fs::{
    atomic_write_bytes, cleanup_empty_parent_dirs, find_companion_sidecars,
};
use crate::infra::inspectors::video::{
    apply_video_patch_onto, is_season_dir, read_base_video_meta, render_season_nfo_xml,
    render_tvshow_nfo_xml, render_video_nfo_xml, write_image_replacing_opposite_ext,
};
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
    /// lyrics, downloading artwork, and—for Video—directly archiving into Jellyfin folder hierarchy).
    pub async fn apply_to_file(
        &self,
        root_id: RootId,
        rel_path: &str,
        media_kind: MediaKind,
        scraped: ScrapedMetadata,
        apply_cover: bool,
        write_sidecar_lrc: bool,
    ) -> DomainResult<MediaFileDetail> {
        if media_kind == MediaKind::Video {
            return self
                .apply_video_jellyfin_archive(root_id, rel_path, scraped, apply_cover)
                .await;
        }

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
                detail = self
                    .workbench
                    .write_artwork(root_id, rel_path, &bytes, &mime, "cover", Some(media_kind))
                    .await?;
            }
        }

        Ok(detail)
    }

    async fn apply_video_jellyfin_archive(
        &self,
        root_id: RootId,
        rel_path: &str,
        scraped: ScrapedMetadata,
        apply_cover: bool,
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

        let patch = map_scraped_to_patch(MediaKind::Video, &scraped, false);
        let MediaMetadataPatch::Video(vp) = patch else {
            unreachable!();
        };

        // 1. Merge scraped metadata onto base local video metadata (preserving filename SxxExx if scraper omitted it)
        let src_abs_for_read = src_sandbox.abs_path.clone();
        let mut meta = tokio::task::spawn_blocking(move || read_base_video_meta(&src_abs_for_read))
            .await
            .map_err(|e| DomainError::Storage(format!("video base inspect failed: {e}")))?;
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
        if apply_cover {
            let client = build_http_client(&raw_settings.credentials)?;
            let urls_to_fetch: [Option<&str>; 4] = [
                scraped.cover_url.as_deref(),
                scraped.backdrop_url.as_deref(),
                scraped.season_poster_url.as_deref(),
                scraped.episode_still_url.as_deref(),
            ];
            for raw_url in urls_to_fetch.into_iter().flatten() {
                let u = raw_url.trim();
                if u.is_empty() || downloaded_images.contains_key(u) {
                    continue;
                }
                match download_image_bytes(&client, u).await {
                    Ok((bytes, mime)) => {
                        let img_ext = if mime.contains("png") { "png" } else { "jpg" };
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
                    // Preserve existing local poster/fanart if online cover download wasn't used
                    if !apply_cover {
                        if lower.ends_with("-poster.jpg") || lower.ends_with("-poster.png") {
                            let target_poster_dir =
                                if meta.is_episode { &show_dir } else { &dst_dir };
                            let ext_p = if lower.ends_with(".png") {
                                "png"
                            } else {
                                "jpg"
                            };
                            let migrated = target_poster_dir.join(format!("poster.{ext_p}"));
                            if !migrated.exists() {
                                let _ = std::fs::copy(&comp_abs, &migrated);
                            }
                        } else if lower.ends_with("-fanart.jpg") || lower.ends_with("-fanart.png") {
                            let target_fan_dir = if meta.is_episode { &show_dir } else { &dst_dir };
                            let ext_f = if lower.ends_with(".png") {
                                "png"
                            } else {
                                "jpg"
                            };
                            let migrated = target_fan_dir.join(format!("fanart.{ext_f}"));
                            if !migrated.exists() {
                                let _ = std::fs::copy(&comp_abs, &migrated);
                            }
                        }
                    }
                    // Clean up flat legacy sidecars (`<old>.nfo`, `<old>-poster.*`, `<old>-fanart.*`)
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

                if let Some(u) = scraped_clone.cover_url.as_deref().map(str::trim)
                    && let Some((bytes, img_ext)) = downloaded_images.get(u)
                {
                    write_image_replacing_opposite_ext(&dst_dir, "poster", img_ext, bytes)?;
                }
                if let Some(u) = scraped_clone.backdrop_url.as_deref().map(str::trim)
                    && let Some((bytes, img_ext)) = downloaded_images.get(u)
                {
                    write_image_replacing_opposite_ext(&dst_dir, "fanart", img_ext, bytes)?;
                }
            } else {
                // Jellyfin 3-Tier TV Show layout:
                // 1) Series root (`show_dir`): `tvshow.nfo`, `poster.jpg`, `fanart.jpg`
                let show_plot = scraped_clone
                    .show_summary
                    .as_deref()
                    .or(meta.summary.as_deref());
                let tvshow_xml = render_tvshow_nfo_xml(&meta, show_plot);
                atomic_write_bytes(&show_dir.join("tvshow.nfo"), tvshow_xml.as_bytes())?;

                if let Some(u) = scraped_clone.cover_url.as_deref().map(str::trim)
                    && let Some((bytes, img_ext)) = downloaded_images.get(u)
                {
                    write_image_replacing_opposite_ext(&show_dir, "poster", img_ext, bytes)?;
                }
                if let Some(u) = scraped_clone.backdrop_url.as_deref().map(str::trim)
                    && let Some((bytes, img_ext)) = downloaded_images.get(u)
                {
                    write_image_replacing_opposite_ext(&show_dir, "fanart", img_ext, bytes)?;
                }

                // 2) Season folder (`dst_dir` = `Season XX`): `season.nfo`, `poster.jpg`, `fanart.jpg`
                let season_num = meta.season_number.unwrap_or(1);
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
                atomic_write_bytes(&dst_dir.join("season.nfo"), season_xml.as_bytes())?;

                if dst_dir != show_dir {
                    let season_poster_u = scraped_clone
                        .season_poster_url
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .or_else(|| scraped_clone.cover_url.as_deref().map(str::trim));
                    if let Some(u) = season_poster_u
                        && let Some((bytes, img_ext)) = downloaded_images.get(u)
                    {
                        write_image_replacing_opposite_ext(&dst_dir, "poster", img_ext, bytes)?;
                    }
                    if let Some(u) = scraped_clone.backdrop_url.as_deref().map(str::trim)
                        && let Some((bytes, img_ext)) = downloaded_images.get(u)
                    {
                        write_image_replacing_opposite_ext(&dst_dir, "fanart", img_ext, bytes)?;
                    }
                }

                // 3) Episode level (`Season XX/<target_stem>.nfo` + `<target_stem>-thumb.jpg`)
                let ep_nfo = dst_dir.join(format!("{target_stem}.nfo"));
                let ep_xml = render_video_nfo_xml(&meta);
                atomic_write_bytes(&ep_nfo, ep_xml.as_bytes())?;

                if let Some(u) = scraped_clone.episode_still_url.as_deref().map(str::trim)
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

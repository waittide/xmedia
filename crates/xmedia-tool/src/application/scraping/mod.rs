mod apply;
mod apply_video;
mod artwork;
mod mapping;
mod search;
#[cfg(test)]
mod tests;

use std::sync::Arc;

use xmedia_core::domain::error::DomainResult;

use crate::application::ops::ScrapeCacheStatsDto;
use crate::application::ports::{
    HttpWebhookPort, MediaToolboxPort, ScrapeCachePort, ScraperProviderPort, ScraperTransportPort,
    WorkspaceFsPort,
};
use crate::application::settings::ToolSettingsService;
use crate::application::workbench::{
    MetadataService, RenameService, RootLockRegistry, WorkspaceBrowserService,
};
use crate::domain::scraper::ScrapedMetadata;
use crate::domain::workspace::RootId;
use xmedia_core::domain::library::MediaKind;

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
    pub detail: crate::domain::media::MediaFileDetail,
}

pub enum CacheManagementResult {
    Stats(ScrapeCacheStatsDto),
    Cleared(usize),
}

/// Application service querying pluggable `ScraperProviderPort` adapters, response cache, and image proxy.
#[derive(Clone)]
pub struct ScraperCatalogService {
    pub(super) settings: ToolSettingsService,
    pub(super) providers: Arc<Vec<Arc<dyn ScraperProviderPort>>>,
    pub(super) cache: Arc<dyn ScrapeCachePort>,
    pub(super) transport: Arc<dyn ScraperTransportPort>,
}

impl ScraperCatalogService {
    pub fn new(
        settings: ToolSettingsService,
        providers: Vec<Arc<dyn ScraperProviderPort>>,
        cache: Arc<dyn ScrapeCachePort>,
        transport: Arc<dyn ScraperTransportPort>,
    ) -> Self {
        Self {
            settings,
            providers: Arc::new(providers),
            cache,
            transport,
        }
    }

    pub async fn manage_cache(&self, clear: bool) -> DomainResult<CacheManagementResult> {
        if clear {
            Ok(CacheManagementResult::Cleared(self.cache.clear()?))
        } else {
            let ttl_ms = self.settings.get_raw().await?.scrape_cache_ttl_ms;
            Ok(CacheManagementResult::Stats(self.cache.stats(ttl_ms)))
        }
    }
}

#[derive(Debug, Clone)]
pub struct ApplyScrapeInput {
    pub root_id: RootId,
    pub rel_path: String,
    pub media_kind: MediaKind,
    pub scraped: ScrapedMetadata,
    pub apply_cover: bool,
    pub write_sidecar_lrc: bool,
    pub organize_by_template: Option<bool>,
    pub only_fill_missing: bool,
    pub write_album_cover: bool,
}

pub struct ScrapeApplyDeps {
    pub settings: ToolSettingsService,
    pub catalog: ScraperCatalogService,
    pub workspace: WorkspaceBrowserService,
    pub metadata: MetadataService,
    pub rename: RenameService,
    pub fs: Arc<dyn WorkspaceFsPort>,
    pub toolbox: Arc<dyn MediaToolboxPort>,
    pub transport: Arc<dyn ScraperTransportPort>,
    pub webhook: Arc<dyn HttpWebhookPort>,
    pub root_locks: RootLockRegistry,
}

/// Application service applying scraped metadata & artwork onto workspace files and Jellyfin archives.
#[derive(Clone)]
pub struct ScrapeApplyService {
    pub(super) settings: ToolSettingsService,
    pub(super) catalog: ScraperCatalogService,
    pub(super) workspace: WorkspaceBrowserService,
    pub(super) metadata: MetadataService,
    pub(super) rename: RenameService,
    pub(super) fs: Arc<dyn WorkspaceFsPort>,
    pub(super) toolbox: Arc<dyn MediaToolboxPort>,
    pub(super) transport: Arc<dyn ScraperTransportPort>,
    pub(super) webhook: Arc<dyn HttpWebhookPort>,
    pub(super) root_locks: RootLockRegistry,
    pub(super) allow_private_artwork_ips: bool,
}

impl ScrapeApplyService {
    pub fn new(deps: ScrapeApplyDeps) -> Self {
        Self {
            settings: deps.settings,
            catalog: deps.catalog,
            workspace: deps.workspace,
            metadata: deps.metadata,
            rename: deps.rename,
            fs: deps.fs,
            toolbox: deps.toolbox,
            transport: deps.transport,
            webhook: deps.webhook,
            root_locks: deps.root_locks,
            allow_private_artwork_ips: false,
        }
    }

    #[doc(hidden)]
    pub fn set_allow_private_artwork_ips(&mut self, allow: bool) {
        self.allow_private_artwork_ips = allow;
    }
}

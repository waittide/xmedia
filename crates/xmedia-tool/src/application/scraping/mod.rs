mod apply;
mod apply_video;
mod artwork;
mod mapping;
mod search;
#[cfg(test)]
mod tests;

use std::sync::Arc;

use xmedia_core::domain::error::DomainResult;

use crate::application::settings::ToolSettingsService;
use crate::application::workbench::MediaWorkbenchService;
use crate::domain::ops::ScrapeCacheStatsDto;
use crate::domain::ports::{ScrapeCachePort, ScraperProviderPort, ScraperTransportPort};

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

/// Application service orchestrating pluggable `ScraperProviderPort` adapters.
#[derive(Clone)]
pub struct ScrapingOrchestratorService {
    pub(super) settings: ToolSettingsService,
    pub(super) workbench: MediaWorkbenchService,
    pub(super) providers: Arc<Vec<Arc<dyn ScraperProviderPort>>>,
    pub(super) cache: Arc<dyn ScrapeCachePort>,
    pub(super) transport: Arc<dyn ScraperTransportPort>,
    pub(super) allow_private_artwork_ips: bool,
}

impl ScrapingOrchestratorService {
    pub fn new(
        settings: ToolSettingsService,
        workbench: MediaWorkbenchService,
        providers: Vec<Arc<dyn ScraperProviderPort>>,
        cache: Arc<dyn ScrapeCachePort>,
        transport: Arc<dyn ScraperTransportPort>,
    ) -> Self {
        Self {
            settings,
            workbench,
            providers: Arc::new(providers),
            cache,
            transport,
            allow_private_artwork_ips: false,
        }
    }

    #[doc(hidden)]
    pub fn set_allow_private_artwork_ips(&mut self, allow: bool) {
        self.allow_private_artwork_ips = allow;
    }

    pub async fn cache_stats(&self) -> DomainResult<ScrapeCacheStatsDto> {
        let ttl_ms = self.settings.get_raw().await?.scrape_cache_ttl_ms;
        Ok(self.cache.stats(ttl_ms))
    }

    pub fn clear_cache(&self) -> DomainResult<usize> {
        self.cache.clear()
    }
}

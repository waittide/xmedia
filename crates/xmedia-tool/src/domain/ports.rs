use std::path::Path;

use async_trait::async_trait;
use xmedia_core::application::ports::BinaryPayload;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::domain::library::MediaKind;

use crate::domain::config::{ProviderCredentials, ToolSettings};
use crate::domain::media::{CompletenessFlags, MediaMetadataPatch, MediaMetadataPayload};
use crate::domain::scraper::{
    ProviderDescriptor, ScrapeCandidate, ScrapeQuery, ScrapeTargetKind, ScrapedMetadata,
};

/// Port for persisting `ToolSettings` (roots, provider API keys, rename templates).
#[async_trait]
pub trait ConfigRepositoryPort: Send + Sync {
    async fn load(&self) -> DomainResult<ToolSettings>;
    async fn save(&self, settings: &ToolSettings) -> DomainResult<()>;
}

/// Quick summary returned when scanning a directory listing so folder views remain fast.
#[derive(Debug, Clone)]
pub struct QuickFileSummary {
    pub display_title: String,
    pub display_subtitle: String,
    pub year: Option<i32>,
    pub completeness: CompletenessFlags,
    pub metadata: Option<MediaMetadataPayload>,
}

/// Port for inspecting and atomically writing metadata/artwork of local media files for one `MediaKind`.
#[async_trait]
pub trait MediaInspectorPort: Send + Sync {
    fn media_kind(&self) -> MediaKind;

    fn supports_extension(&self, ext_lower: &str) -> bool;

    /// Lightweight summary used when listing a directory.
    async fn summarize_file(&self, path: &Path, fallback_stem: &str) -> QuickFileSummary;

    /// Full metadata read (including streams, lyrics, ComicInfo.xml, OPF, NFO).
    async fn inspect_file(&self, path: &Path) -> DomainResult<MediaMetadataPayload>;

    /// Atomically applies a metadata patch to the file (or sidecar `.nfo` / `.lrc`).
    async fn write_metadata(
        &self,
        path: &Path,
        patch: &MediaMetadataPatch,
    ) -> DomainResult<MediaMetadataPayload>;

    /// Reads embedded or sidecar cover/poster/fanart/thumb image if available.
    async fn read_artwork(
        &self,
        path: &Path,
        slot: Option<&str>,
    ) -> DomainResult<Option<BinaryPayload>>;

    /// Writes/replaces embedded or sidecar cover/poster image (`slot`: `"cover"` / `"poster"` / `"fanart"`).
    async fn write_artwork(
        &self,
        path: &Path,
        bytes: &[u8],
        mime_type: &str,
        slot: &str,
    ) -> DomainResult<()>;
}

/// Pluggable Scraper Provider Port (Hexagonal Architecture).
///
/// Adding a new online metadata/lyrics/artwork source only requires implementing this trait
/// and registering the instance in `ScrapingOrchestratorService`!
#[async_trait]
pub trait ScraperProviderPort: Send + Sync {
    /// Static capability descriptor (id, display name, supported target kinds, whether API key is required).
    fn descriptor(&self) -> ProviderDescriptor;

    /// Checks whether required credentials (if any) are currently configured.
    fn is_configured(&self, creds: &ProviderCredentials) -> bool {
        let _ = creds;
        !self.descriptor().requires_api_key
    }

    /// Searches external source for candidates matching `query`.
    async fn search(
        &self,
        query: &ScrapeQuery,
        creds: &ProviderCredentials,
    ) -> DomainResult<Vec<ScrapeCandidate>>;

    /// Fetches normalized rich metadata for a specific `external_id` returned by `search`.
    async fn fetch_detail(
        &self,
        external_id: &str,
        target: ScrapeTargetKind,
        locale: &str,
        creds: &ProviderCredentials,
    ) -> DomainResult<ScrapedMetadata>;
}

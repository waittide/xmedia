use std::path::{Path, PathBuf};

use async_trait::async_trait;
use xmedia_core::application::ports::BinaryPayload;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::domain::library::MediaKind;

use crate::application::ops::{
    ScrapeCacheStatsDto, ToolboxCommand, ToolboxOutput, WebhookTestReportDto,
};
use crate::domain::config::{ProviderCredentials, ToolSettings, XmediaWebhookConfig};
use crate::domain::media::{CompletenessFlags, MediaMetadataPatch, MediaMetadataPayload};
use crate::domain::scraper::{
    ProviderDescriptor, ScrapeCandidate, ScrapeQuery, ScrapeTargetKind, ScrapedMetadata,
};
use crate::domain::workspace::{ManagedRoot, SandboxPath};

/// Port for persisting `ToolSettings` (roots, provider API keys, rename templates).
#[async_trait]
pub trait ConfigRepositoryPort: Send + Sync {
    async fn load(&self) -> DomainResult<ToolSettings>;
    async fn save(&self, settings: &ToolSettings) -> DomainResult<()>;
}

/// Sandbox-aware filesystem & sidecar operations port.
pub trait WorkspaceFsPort: Send + Sync {
    fn validate_safe_host_directory(&self, canonical: &Path, label: &str) -> DomainResult<()>;

    fn open_managed_root(
        &self,
        name: String,
        raw_path: &Path,
        default_kind: Option<MediaKind>,
    ) -> DomainResult<ManagedRoot>;

    fn resolve_existing_in_root(
        &self,
        root: &ManagedRoot,
        rel_path: &str,
    ) -> DomainResult<SandboxPath>;

    fn resolve_target_in_root(&self, root: &ManagedRoot, rel_path: &str) -> DomainResult<PathBuf>;

    fn find_companion_sidecars(&self, media_file: &Path) -> Vec<(PathBuf, String)>;

    fn collect_relocated_companions(
        &self,
        _root_path: &Path,
        _media_abs: &Path,
        _rendered_stem: &str,
        _parent_rel: &str,
        _target_parent_rel: &str,
        _meta: &MediaMetadataPayload,
    ) -> Vec<(String, String)> {
        Vec::new()
    }

    fn cleanup_empty_parent_dirs(&self, start_dir: &Path, stop_at_root: &Path);
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

/// Read or write operation on embedded or sidecar media artwork.
#[derive(Debug, Clone, Copy)]
pub enum ArtworkIo<'a> {
    Read {
        slot: Option<&'a str>,
    },
    Write {
        slot: &'a str,
        bytes: &'a [u8],
        mime_type: &'a str,
    },
}

/// Port for inspecting and atomically writing metadata/artwork of local media files for one `MediaKind`.
#[async_trait]
pub trait MediaInspectorPort: Send + Sync {
    fn media_kind(&self) -> MediaKind;

    /// Returns `true` when `path` is a supported media file or standalone directory-backed media item.
    fn supports_path(&self, path: &Path) -> bool;

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

    /// Reads or writes embedded or sidecar artwork (`"cover"` / `"poster"` / `"fanart"` / `"thumb"`).
    async fn artwork(&self, path: &Path, op: ArtworkIo<'_>) -> DomainResult<Option<BinaryPayload>>;
}

/// Narrow command-dispatched port encapsulating format-specific content editing, packaging, and optimization.
pub trait MediaToolboxPort: Send + Sync {
    fn execute(&self, cmd: ToolboxCommand) -> DomainResult<ToolboxOutput>;
}

pub enum ScrapeCacheLookup<'a> {
    Search {
        provider_id: &'a str,
        query: &'a ScrapeQuery,
        locale: &'a str,
    },
    Detail {
        provider_id: &'a str,
        target: ScrapeTargetKind,
        external_id: &'a str,
        locale: &'a str,
    },
}

pub enum ScrapeCacheValue {
    Search(Vec<ScrapeCandidate>),
    Detail(Box<ScrapedMetadata>),
}

pub enum ScrapeCacheWrite<'a> {
    Search {
        provider_id: &'a str,
        query: &'a ScrapeQuery,
        locale: &'a str,
        items: &'a [ScrapeCandidate],
    },
    Detail {
        provider_id: &'a str,
        target: ScrapeTargetKind,
        external_id: &'a str,
        locale: &'a str,
        detail: &'a ScrapedMetadata,
    },
}

/// Port for persistent/in-memory scraper response caching.
pub trait ScrapeCachePort: Send + Sync {
    fn get(&self, key: ScrapeCacheLookup<'_>, ttl_ms: u64) -> Option<ScrapeCacheValue>;
    fn put(&self, entry: ScrapeCacheWrite<'_>, ttl_ms: u64);
    fn stats(&self, ttl_ms: u64) -> ScrapeCacheStatsDto;
    fn clear(&self) -> DomainResult<usize>;
}

/// Port for rate-limited outbound scraper HTTP transport & SSRF-guarded artwork downloads.
#[async_trait]
pub trait ScraperTransportPort: Send + Sync {
    async fn acquire_rate_slot(&self, provider_id: &str);

    async fn download_artwork(
        &self,
        creds: &ProviderCredentials,
        url: &str,
        allow_private_ips: bool,
    ) -> DomainResult<(Vec<u8>, String)>;
}

/// Port for testing connectivity and triggering incremental library scans on a linked `xmedia` server.
#[async_trait]
pub trait HttpWebhookPort: Send + Sync {
    async fn notify_rescan(&self, cfg: &XmediaWebhookConfig, changed_root_path: &Path);

    async fn dispatch_rescan(
        &self,
        cfg: &XmediaWebhookConfig,
        changed_root_path: Option<&Path>,
        trigger_scans: bool,
    ) -> DomainResult<WebhookTestReportDto>;
}

/// Pluggable Scraper Provider Port (Hexagonal Architecture).
#[async_trait]
pub trait ScraperProviderPort: Send + Sync {
    fn descriptor(&self) -> ProviderDescriptor;

    fn is_configured(&self, creds: &ProviderCredentials) -> bool {
        let _ = creds;
        !self.descriptor().requires_api_key
    }

    async fn search(
        &self,
        query: &ScrapeQuery,
        creds: &ProviderCredentials,
    ) -> DomainResult<Vec<ScrapeCandidate>>;

    async fn fetch_detail(
        &self,
        external_id: &str,
        target: ScrapeTargetKind,
        locale: &str,
        creds: &ProviderCredentials,
    ) -> DomainResult<ScrapedMetadata>;
}

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use xmedia_core::application::ports::BinaryPayload;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::domain::library::MediaKind;

use crate::domain::config::{ProviderCredentials, ToolSettings, XmediaWebhookConfig};
use crate::domain::media::{
    CompletenessFlags, MediaMetadataPatch, MediaMetadataPayload, VideoFileMeta,
};
use crate::domain::ops::{
    BookContentDocumentDto, ComicImageReencodeFormat, ComicOptimizeReportDto,
    ComicPagesSnapshotDto, CueSplitReportDto, EditableBookChapterDto, ExtractSubtitleReportDto,
    InsertedEpubImageDto, MergeComicsReportDto, MusicCoverOptimizeReportDto, MusicLyricsToolInput,
    MusicLyricsToolReportDto, NormalizeBookOutcome, NormalizeBookToEpubOptions,
    PolishBookReportDto, ProcessSubtitleInput, ProcessSubtitleReportDto, ReplayGainReportDto,
    SaveBookChapterInput, SaveComicPageInput, ScrapeCacheStatsDto, TransformBookContentResultDto,
    VideoSubtitleInfoDto, WebhookTestReportDto,
};
use crate::domain::scraper::{
    ProviderDescriptor, ScrapeCandidate, ScrapeQuery, ScrapeTargetKind, ScrapedMetadata,
};
use crate::domain::workspace::{ManagedRoot, SandboxPath};
use crate::domain::zh_text::BookTextTransformOptions;

/// Port for persisting `ToolSettings` (roots, provider API keys, rename templates).
#[async_trait]
pub trait ConfigRepositoryPort: Send + Sync {
    async fn load(&self) -> DomainResult<ToolSettings>;
    async fn save(&self, settings: &ToolSettings) -> DomainResult<()>;
}

/// Alias aligning with tactical DDD naming (`ConfigStorePort`).
pub use ConfigRepositoryPort as ConfigStorePort;

/// Sandbox-aware filesystem & atomic sidecar operations port.
pub trait WorkspaceFsPort: Send + Sync {
    fn paths_overlap(&self, a: &Path, b: &Path) -> bool {
        a == b || a.starts_with(b) || b.starts_with(a)
    }

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

    fn atomic_write_bytes(&self, target: &Path, bytes: &[u8]) -> DomainResult<()>;

    fn cleanup_empty_parent_dirs(&self, start_dir: &Path, stop_at_root: &Path);

    fn find_companion_sidecars(&self, media_file: &Path) -> Vec<(PathBuf, String)>;

    fn is_shared_dir_sidecar_name(&self, file_name: &str) -> bool;

    fn write_image_replacing_opposite_ext(
        &self,
        dir: &Path,
        base_name: &str,
        ext: &str,
        bytes: &[u8],
    ) -> DomainResult<()>;
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

    /// Returns `true` when `dir_path` is a standalone directory-backed media item (e.g. a comic image folder).
    fn supports_directory(&self, dir_path: &Path) -> bool {
        let _ = dir_path;
        false
    }

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

/// Port encapsulating format-specific content editing, packaging, optimization, and folder conventions.
pub trait MediaToolboxPort: Send + Sync {
    // Book operations
    fn read_epub_internal_asset(
        &self,
        path: &Path,
        asset_path: &str,
    ) -> DomainResult<BinaryPayload>;
    fn read_book_content(&self, path: &Path) -> DomainResult<BookContentDocumentDto>;
    fn resegment_txt_file(
        &self,
        path: &Path,
        chapter_pattern: Option<&str>,
        volume_pattern: Option<&str>,
    ) -> DomainResult<Vec<EditableBookChapterDto>>;
    fn save_book_content(
        &self,
        path: &Path,
        chapters: &[SaveBookChapterInput],
        stylesheets: Option<&[String]>,
        force_horizontal_spine: bool,
    ) -> DomainResult<()>;
    fn convert_txt_to_epub(
        &self,
        path: &Path,
        chapters: Option<&[SaveBookChapterInput]>,
        stylesheets: Option<&[String]>,
        delete_source_txt: bool,
    ) -> DomainResult<PathBuf>;
    fn transform_book_content_batch(
        &self,
        chapters: Vec<EditableBookChapterDto>,
        stylesheets: Vec<String>,
        options: &BookTextTransformOptions,
    ) -> DomainResult<TransformBookContentResultDto>;
    fn insert_epub_image(
        &self,
        path: &Path,
        chapter_source_href: Option<&str>,
        bytes: &[u8],
        mime_hint: Option<&str>,
    ) -> DomainResult<InsertedEpubImageDto>;
    fn polish_epub_book(&self, path: &Path) -> DomainResult<PolishBookReportDto>;
    fn normalize_book_to_epub(
        &self,
        path: &Path,
        opts: &NormalizeBookToEpubOptions,
    ) -> DomainResult<NormalizeBookOutcome>;

    // Comic operations
    fn list_comic_pages(&self, path: &Path) -> DomainResult<ComicPagesSnapshotDto>;
    fn read_comic_page_image(
        &self,
        path: &Path,
        entry_name: &str,
        max_width: Option<u32>,
    ) -> DomainResult<BinaryPayload>;
    fn save_comic_pages(
        &self,
        path: &Path,
        pages: &[SaveComicPageInput],
        reencode: ComicImageReencodeFormat,
        jpeg_quality: u8,
        max_width: Option<u32>,
    ) -> DomainResult<ComicOptimizeReportDto>;
    fn optimize_comic_cbz(
        &self,
        path: &Path,
        reencode: ComicImageReencodeFormat,
        jpeg_quality: u8,
        max_width: Option<u32>,
    ) -> DomainResult<ComicOptimizeReportDto>;
    fn merge_comics_to_cbz(
        &self,
        sources: &[PathBuf],
        target_abs: &Path,
        output_rel_path: String,
        series: Option<String>,
        title: Option<String>,
        volume: Option<f64>,
    ) -> DomainResult<MergeComicsReportDto>;

    // Music operations
    fn process_music_lyrics(
        &self,
        path: &Path,
        input: &MusicLyricsToolInput,
    ) -> DomainResult<MusicLyricsToolReportDto>;
    fn optimize_music_cover(
        &self,
        path: &Path,
        max_dimension: u32,
        jpeg_quality: u8,
    ) -> DomainResult<MusicCoverOptimizeReportDto>;
    fn apply_replaygain_batch(
        &self,
        items: &[(String, PathBuf)],
        ffmpeg_bin: &str,
    ) -> DomainResult<ReplayGainReportDto>;
    fn split_cue_album(&self, path: &Path, ffmpeg_bin: &str) -> DomainResult<CueSplitReportDto>;

    // Video operations & NFO helpers
    fn list_video_subtitles(&self, path: &Path) -> DomainResult<Vec<VideoSubtitleInfoDto>>;
    fn process_video_subtitle(
        &self,
        path: &Path,
        input: &ProcessSubtitleInput,
    ) -> DomainResult<ProcessSubtitleReportDto>;
    fn extract_embedded_subtitle(
        &self,
        path: &Path,
        stream_index: u32,
        language_tag: Option<&str>,
        output_ext: &str,
        ffmpeg_bin: &str,
    ) -> DomainResult<ExtractSubtitleReportDto>;
    fn read_base_video_meta(&self, path: &Path) -> VideoFileMeta;
    fn find_existing_video_nfo(&self, path: &Path, is_episode: bool) -> Option<PathBuf>;
    fn find_show_nfo_for_episode(&self, path: &Path) -> Option<PathBuf>;
    fn render_video_nfo_xml(&self, meta: &VideoFileMeta, existing_xml: Option<&[u8]>) -> String;
    fn render_tvshow_nfo_xml(
        &self,
        meta: &VideoFileMeta,
        show_plot: Option<&str>,
        existing_xml: Option<&[u8]>,
    ) -> String;
    #[allow(clippy::too_many_arguments)]
    fn render_season_nfo_xml(
        &self,
        season_num: u32,
        title: Option<&str>,
        year: Option<i32>,
        plot: Option<&str>,
        tmdb_id: Option<&str>,
        bangumi_id: Option<&str>,
        existing_xml: Option<&[u8]>,
    ) -> String;

    // Folder structure classification helpers
    fn dir_has_single_book(&self, dir: &Path) -> bool;
    fn is_generic_book_folder(&self, name: &str) -> bool;
    fn is_generic_comic_folder(&self, name: &str) -> bool;
    fn is_generic_music_folder(&self, name: &str) -> bool;
    fn dir_has_single_video(&self, dir: &Path) -> bool;
    fn is_season_dir(&self, dir: &Path) -> bool;
}

/// Port for persistent/in-memory scraper response caching.
pub trait ScrapeCachePort: Send + Sync {
    fn get_search(
        &self,
        provider_id: &str,
        query: &ScrapeQuery,
        locale: &str,
        ttl_ms: u64,
    ) -> Option<Vec<ScrapeCandidate>>;
    fn put_search(
        &self,
        provider_id: &str,
        query: &ScrapeQuery,
        locale: &str,
        ttl_ms: u64,
        items: &[ScrapeCandidate],
    );
    fn get_detail(
        &self,
        provider_id: &str,
        target: ScrapeTargetKind,
        external_id: &str,
        locale: &str,
        ttl_ms: u64,
    ) -> Option<ScrapedMetadata>;
    fn put_detail(
        &self,
        provider_id: &str,
        target: ScrapeTargetKind,
        external_id: &str,
        locale: &str,
        ttl_ms: u64,
        detail: &ScrapedMetadata,
    );
    fn stats(&self, ttl_ms: u64) -> ScrapeCacheStatsDto;
    fn clear(&self) -> DomainResult<usize>;
}

/// Port for rate-limited outbound scraper HTTP transport & SSRF-guarded artwork downloads.
#[async_trait]
pub trait ScraperTransportPort: Send + Sync {
    async fn acquire_rate_slot(&self, provider_id: &str);

    async fn validate_public_http_url(&self, raw_url: &str) -> DomainResult<String>;

    async fn download_artwork(
        &self,
        creds: &ProviderCredentials,
        url: &str,
        allow_private_ips: bool,
    ) -> DomainResult<(Vec<u8>, String)>;
}

/// Port for triggering incremental library scans on a linked `xmedia` main server.
#[async_trait]
pub trait HttpWebhookPort: Send + Sync {
    async fn test_webhook(&self, cfg: &XmediaWebhookConfig) -> DomainResult<WebhookTestReportDto>;

    async fn notify_rescan(&self, cfg: &XmediaWebhookConfig, changed_root_path: &Path);

    async fn trigger_rescan(
        &self,
        cfg: &XmediaWebhookConfig,
        changed_root_path: Option<&Path>,
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

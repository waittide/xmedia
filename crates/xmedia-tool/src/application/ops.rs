use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use xmedia_core::application::ports::BinaryPayload;

use crate::domain::media::VideoFileMeta;
use crate::domain::scraper::ScrapedMetadata;
use crate::domain::zh_text::{BookTextTransformOptions, ZhConvertDirection};

fn default_true() -> bool {
    true
}

fn default_max_image_dim() -> Option<u32> {
    Some(1600)
}

fn default_image_quality() -> u8 {
    82
}

fn default_page_type() -> String {
    "Story".to_string()
}

fn default_start_index() -> i32 {
    1
}

fn default_pad_width() -> usize {
    2
}

fn default_sync_mode() -> String {
    "both".to_string()
}

fn default_track_start() -> u32 {
    1
}

fn default_remap_mode() -> String {
    "sequential".to_string()
}

fn default_one() -> u32 {
    1
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScrapeCacheStatsDto {
    pub total_entries: usize,
    pub valid_entries: usize,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookTestReportDto {
    pub ok: bool,
    pub matched_libraries: Vec<String>,
    pub total_libraries: usize,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ChapterEditMode {
    #[default]
    Plain,
    Xhtml,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditableBookChapterDto {
    pub id: String,
    pub title: String,
    /// `0` = Volume / Top-level chapter, `1` = Sub-chapter nested under preceding `0`
    pub level: u8,
    pub plain_text: String,
    pub xhtml_body: String,
    pub has_rich_media: bool,
    pub source_href: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookContentDocumentDto {
    pub format: String,
    pub title: String,
    pub authors: Vec<String>,
    #[serde(default)]
    pub stylesheets: Vec<String>,
    pub chapters: Vec<EditableBookChapterDto>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SaveBookChapterInput {
    pub title: String,
    #[serde(default)]
    pub level: u8,
    #[serde(default)]
    pub mode: ChapterEditMode,
    #[serde(default)]
    pub plain_text: String,
    #[serde(default)]
    pub xhtml_body: String,
    #[serde(default)]
    pub source_href: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformBookContentResultDto {
    pub chapters: Vec<EditableBookChapterDto>,
    pub stylesheets: Vec<String>,
    pub changed_chapters: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InsertedEpubImageDto {
    pub asset_path: String,
    pub chapter_rel_href: String,
    pub mime_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolishBookReportDto {
    pub jacket_updated: bool,
    pub removed_orphan_files: usize,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BookImageCompressFormat {
    #[default]
    Webp,
    Jpeg,
    Keep,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NormalizeBookToEpubOptions {
    #[serde(default = "default_true")]
    pub keep_source: bool,
    #[serde(default = "default_true")]
    pub compress_images: bool,
    #[serde(default)]
    pub image_format: BookImageCompressFormat,
    #[serde(default = "default_max_image_dim")]
    pub max_image_dimension: Option<u32>,
    #[serde(default = "default_image_quality")]
    pub image_quality: u8,
    #[serde(default = "default_true")]
    pub generate_jacket: bool,
    #[serde(default = "default_true")]
    pub remove_orphans: bool,
    #[serde(default = "default_true")]
    pub pdf_scanned_fallback: bool,
    #[serde(default = "default_true")]
    pub skip_normalized: bool,
}

impl Default for NormalizeBookToEpubOptions {
    fn default() -> Self {
        Self {
            keep_source: true,
            compress_images: true,
            image_format: BookImageCompressFormat::Webp,
            max_image_dimension: default_max_image_dim(),
            image_quality: default_image_quality(),
            generate_jacket: true,
            remove_orphans: true,
            pdf_scanned_fallback: true,
            skip_normalized: true,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct NormalizeBookOutcome {
    pub output_path: PathBuf,
    pub backup_path: Option<PathBuf>,
    pub source_format: String,
    pub before_bytes: u64,
    pub after_bytes: u64,
    pub chapter_count: usize,
    pub compressed_images: usize,
    pub removed_orphans: usize,
    pub deleted_source: bool,
    pub skipped: bool,
    pub skip_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NormalizeBookItemReportDto {
    pub source_rel_path: String,
    pub output_rel_path: Option<String>,
    pub backup_rel_path: Option<String>,
    pub source_format: String,
    pub before_bytes: u64,
    pub after_bytes: u64,
    pub chapter_count: usize,
    pub compressed_images: usize,
    pub removed_orphans: usize,
    pub deleted_source: bool,
    pub skipped: bool,
    pub skip_reason: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NormalizeBooksBatchReportDto {
    pub total: usize,
    pub succeeded: usize,
    pub skipped: usize,
    pub failed: usize,
    pub total_before_bytes: u64,
    pub total_after_bytes: u64,
    pub items: Vec<NormalizeBookItemReportDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComicPageInfoDto {
    pub index: usize,
    pub entry_name: String,
    pub file_size: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub page_type: String,
    pub double_page: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComicPagesSnapshotDto {
    pub format: String,
    pub total_pages: usize,
    pub total_size_bytes: u64,
    pub pages: Vec<ComicPageInfoDto>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SaveComicPageInput {
    pub entry_name: String,
    #[serde(default = "default_page_type")]
    pub page_type: String,
    #[serde(default)]
    pub double_page: bool,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ComicImageReencodeFormat {
    #[default]
    Keep,
    Jpeg,
    Webp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComicOptimizeReportDto {
    pub before_bytes: u64,
    pub after_bytes: u64,
    pub page_count: usize,
    pub reencoded_pages: usize,
    pub removed_junk_entries: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeComicsReportDto {
    pub output_rel_path: String,
    pub source_count: usize,
    pub total_pages: usize,
    pub output_bytes: u64,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct BatchComicSeriesSyncInput {
    pub series: Option<String>,
    pub series_group: Option<String>,
    pub writers: Option<Vec<String>>,
    pub pencillers: Option<Vec<String>>,
    pub publisher: Option<String>,
    pub genres: Option<Vec<String>>,
    pub tags: Option<Vec<String>>,
    pub reading_direction: Option<String>,
    #[serde(default)]
    pub numbering_mode: String,
    #[serde(default = "default_start_index")]
    pub start_index: i32,
    #[serde(default = "default_pad_width")]
    pub pad_width: usize,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct MusicLyricsToolInput {
    #[serde(default)]
    pub offset_ms: i64,
    #[serde(default)]
    pub strip_credit_noise: bool,
    #[serde(default)]
    pub zh_direction: Option<ZhConvertDirection>,
    #[serde(default = "default_sync_mode")]
    pub sync_mode: String,
    pub custom_text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MusicLyricsToolReportDto {
    pub updated_embedded: bool,
    pub updated_sidecar: bool,
    pub removed_lines: usize,
    pub final_lyrics: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MusicCoverOptimizeReportDto {
    pub had_cover: bool,
    pub optimized: bool,
    pub before_bytes: u64,
    pub after_bytes: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayGainTrackResultDto {
    pub rel_path: String,
    pub track_gain_db: f64,
    pub track_peak: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayGainReportDto {
    pub processed: usize,
    pub album_gain_db: Option<f64>,
    pub album_peak: Option<f64>,
    pub tracks: Vec<ReplayGainTrackResultDto>,
    pub failed_paths: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct BatchMusicAlbumSyncInput {
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub artist: Option<String>,
    pub year: Option<i32>,
    pub genre: Option<String>,
    pub disc_number: Option<u32>,
    #[serde(default)]
    pub auto_number_tracks: bool,
    #[serde(default = "default_track_start")]
    pub start_track: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CueSplitReportDto {
    pub created_files: Vec<String>,
    pub album_title: Option<String>,
    pub album_artist: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoSubtitleInfoDto {
    pub filename: String,
    pub extension: String,
    pub file_size: u64,
    pub detected_encoding: String,
    pub is_utf8: bool,
    pub language_tag: Option<String>,
    pub forced: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ProcessSubtitleInput {
    pub subtitle_filename: String,
    #[serde(default)]
    pub convert_utf8: bool,
    #[serde(default)]
    pub offset_ms: i64,
    #[serde(default)]
    pub zh_direction: Option<ZhConvertDirection>,
    pub rename_language: Option<String>,
    #[serde(default)]
    pub forced: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessSubtitleReportDto {
    pub original_filename: String,
    pub final_filename: String,
    pub original_encoding: String,
    pub shifted_cues: usize,
    pub converted_utf8: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractSubtitleReportDto {
    pub created_filename: String,
    pub file_size: u64,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct BatchRemapEpisodesInput {
    pub show_title: Option<String>,
    pub season_number: Option<u32>,
    #[serde(default = "default_remap_mode")]
    pub mode: String,
    #[serde(default = "default_one")]
    pub start_episode: u32,
    #[serde(default)]
    pub episode_offset: i32,
    #[serde(default)]
    pub write_season_nfo: bool,
}

#[derive(Debug, Clone)]
pub enum BookToolCommand {
    ReadEpubAsset {
        path: PathBuf,
        asset_path: String,
    },
    ReadContent {
        path: PathBuf,
    },
    ResegmentTxt {
        path: PathBuf,
        chapter_pattern: Option<String>,
        volume_pattern: Option<String>,
    },
    SaveContent {
        path: PathBuf,
        chapters: Vec<SaveBookChapterInput>,
        stylesheets: Option<Vec<String>>,
        force_horizontal_spine: bool,
    },
    ConvertTxtToEpub {
        path: PathBuf,
        chapters: Option<Vec<SaveBookChapterInput>>,
        stylesheets: Option<Vec<String>>,
        delete_source_txt: bool,
    },
    TransformContent {
        chapters: Vec<EditableBookChapterDto>,
        stylesheets: Vec<String>,
        options: BookTextTransformOptions,
    },
    InsertEpubImage {
        path: PathBuf,
        chapter_source_href: Option<String>,
        bytes: Vec<u8>,
        mime_hint: Option<String>,
    },
    PolishEpub {
        path: PathBuf,
    },
    NormalizeToEpub {
        path: PathBuf,
        options: NormalizeBookToEpubOptions,
    },
    PromoteCalibreSidecars {
        root_path: PathBuf,
        book_path: PathBuf,
    },
}

#[derive(Debug)]
pub enum BookToolOutput {
    Binary(BinaryPayload),
    Document(BookContentDocumentDto),
    Chapters(Vec<EditableBookChapterDto>),
    Transform(TransformBookContentResultDto),
    InsertedImage(InsertedEpubImageDto),
    Polished(PolishBookReportDto),
    Normalized(NormalizeBookOutcome),
    Path(PathBuf),
    Unit,
}

#[derive(Debug, Clone)]
pub enum ComicToolCommand {
    ListPages {
        path: PathBuf,
    },
    ReadPageImage {
        path: PathBuf,
        entry_name: String,
        max_width: Option<u32>,
    },
    SavePages {
        path: PathBuf,
        pages: Vec<SaveComicPageInput>,
        reencode: ComicImageReencodeFormat,
        jpeg_quality: u8,
        max_width: Option<u32>,
    },
    OptimizeCbz {
        path: PathBuf,
        reencode: ComicImageReencodeFormat,
        jpeg_quality: u8,
        max_width: Option<u32>,
    },
    MergeToCbz {
        sources: Vec<PathBuf>,
        target_abs: PathBuf,
        output_rel_path: String,
        series: Option<String>,
        title: Option<String>,
        volume: Option<f64>,
    },
}

#[derive(Debug)]
pub enum ComicToolOutput {
    Pages(ComicPagesSnapshotDto),
    Binary(BinaryPayload),
    Optimized(ComicOptimizeReportDto),
    Merged(MergeComicsReportDto),
}

#[derive(Debug, Clone)]
pub enum MusicToolCommand {
    ProcessLyrics {
        path: PathBuf,
        input: MusicLyricsToolInput,
    },
    OptimizeCover {
        path: PathBuf,
        max_dimension: u32,
        jpeg_quality: u8,
    },
    ApplyReplayGain {
        items: Vec<(String, PathBuf)>,
        ffmpeg_bin: String,
    },
    SplitCue {
        path: PathBuf,
        ffmpeg_bin: String,
    },
}

#[derive(Debug)]
pub enum MusicToolOutput {
    Lyrics(MusicLyricsToolReportDto),
    Cover(MusicCoverOptimizeReportDto),
    ReplayGain(ReplayGainReportDto),
    CueSplit(CueSplitReportDto),
}

#[derive(Debug, Clone)]
pub struct JellyfinArchiveInput {
    pub root_path: PathBuf,
    pub src_abs: PathBuf,
    pub dst_abs: PathBuf,
    pub target_stem: String,
    pub meta: VideoFileMeta,
    pub scraped: ScrapedMetadata,
    pub downloaded_images: HashMap<String, (Vec<u8>, &'static str)>,
    pub skip_poster: bool,
    pub skip_fanart: bool,
    pub skip_thumb: bool,
    pub only_fill_missing: bool,
}

#[derive(Debug, Clone)]
pub enum VideoToolCommand {
    ListSubtitles {
        path: PathBuf,
    },
    ProcessSubtitle {
        path: PathBuf,
        input: ProcessSubtitleInput,
    },
    ExtractEmbeddedSubtitle {
        path: PathBuf,
        stream_index: u32,
        language_tag: Option<String>,
        output_ext: String,
        ffmpeg_bin: String,
    },
    ReadBaseMeta {
        path: PathBuf,
    },
    WriteSeasonNfo {
        season_dir: PathBuf,
        season_num: u32,
        year: Option<i32>,
        tmdb_id: Option<String>,
        bangumi_id: Option<String>,
    },
    ArchiveJellyfin(Box<JellyfinArchiveInput>),
}

#[derive(Debug)]
pub enum VideoToolOutput {
    Subtitles(Vec<VideoSubtitleInfoDto>),
    ProcessedSubtitle(ProcessSubtitleReportDto),
    ExtractedSubtitle(ExtractSubtitleReportDto),
    BaseMeta(Box<VideoFileMeta>),
    Unit,
}

#[derive(Debug, Clone)]
pub enum ToolboxCommand {
    Book(BookToolCommand),
    Comic(ComicToolCommand),
    Music(MusicToolCommand),
    Video(VideoToolCommand),
}

#[derive(Debug)]
pub enum ToolboxOutput {
    Book(BookToolOutput),
    Comic(ComicToolOutput),
    Music(MusicToolOutput),
    Video(VideoToolOutput),
}

use serde::{Deserialize, Serialize};
use xmedia_core::domain::library::MediaKind;
use xmedia_core::infra::ffmpeg::{ProbedAudioStream, ProbedSubtitleStream};

use crate::domain::workspace::RootId;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompletenessFlags {
    pub has_title: bool,
    pub has_creator: bool,
    pub has_cover: bool,
    pub has_summary_or_lyrics: bool,
    pub has_nfo_or_tag_block: bool,
    #[serde(default)]
    pub is_normalized: bool,
    #[serde(default)]
    pub is_organized: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FolderEntryDto {
    pub name: String,
    pub rel_path: String,
    pub child_dir_count: usize,
    pub media_file_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaFileEntry {
    pub root_id: RootId,
    pub rel_path: String,
    pub file_name: String,
    pub extension: String,
    pub media_kind: MediaKind,
    pub file_size: u64,
    pub modified_unix_secs: i64,
    pub display_title: String,
    pub display_subtitle: String,
    pub year: Option<i32>,
    pub completeness: CompletenessFlags,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawFileEntryDto {
    pub rel_path: String,
    pub file_name: String,
    pub extension: String,
    pub file_size: u64,
    pub modified_unix_secs: i64,
    pub is_media: bool,
    pub media_kind: Option<MediaKind>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectorySnapshot {
    pub root_id: RootId,
    pub root_name: String,
    pub current_rel_path: String,
    pub parent_rel_path: Option<String>,
    pub folders: Vec<FolderEntryDto>,
    pub files: Vec<MediaFileEntry>,
    #[serde(default)]
    pub raw_files: Vec<RawFileEntryDto>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BookFileMeta {
    pub format: String,
    pub title: String,
    pub authors: Vec<String>,
    #[serde(default)]
    pub series: Option<String>,
    #[serde(default)]
    pub subseries: Option<String>,
    #[serde(default)]
    pub series_index: Option<String>,
    pub description: Option<String>,
    pub publisher: Option<String>,
    pub published_at: Option<String>,
    pub language: Option<String>,
    pub isbn: Option<String>,
    pub tags: Vec<String>,
    #[serde(default)]
    pub locked_fields: Vec<String>,
    #[serde(default)]
    pub is_normalized: bool,
    pub has_cover: bool,
    pub writable: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ComicFileMeta {
    pub format: String,
    pub series: Option<String>,
    #[serde(default)]
    pub series_group: Option<String>,
    pub title: Option<String>,
    pub number: Option<String>,
    pub volume: Option<i32>,
    pub summary: Option<String>,
    pub writers: Vec<String>,
    pub pencillers: Vec<String>,
    pub publisher: Option<String>,
    pub genres: Vec<String>,
    pub tags: Vec<String>,
    pub year: Option<i32>,
    pub language_iso: Option<String>,
    pub reading_direction: String,
    pub page_count: u32,
    pub has_comic_info_xml: bool,
    pub has_cover: bool,
    #[serde(default)]
    pub has_series_cover: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MusicFileMeta {
    pub format: String,
    pub title: String,
    pub artist: String,
    pub album_artist: Option<String>,
    pub album: String,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub year: Option<i32>,
    pub genre: Option<String>,
    pub comment: Option<String>,
    pub duration_secs: u32,
    pub bit_rate_kbps: Option<u32>,
    pub sample_rate_hz: Option<u32>,
    pub bit_depth: Option<u16>,
    pub channels: Option<u8>,
    pub has_embedded_cover: bool,
    #[serde(default)]
    pub has_directory_cover: bool,
    pub embedded_lyrics: Option<String>,
    pub sidecar_lrc: Option<String>,
    pub sidecar_lrc_filename: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VideoFileMeta {
    pub container: String,
    pub is_episode: bool,
    pub title: String,
    pub original_title: Option<String>,
    pub show_title: Option<String>,
    pub season_number: Option<u32>,
    pub episode_number: Option<u32>,
    pub year: Option<i32>,
    pub rating: Option<f32>,
    pub summary: Option<String>,
    pub genres: Vec<String>,
    pub tags: Vec<String>,
    pub actors: Vec<String>,
    pub directors: Vec<String>,
    pub studio: Option<String>,
    #[serde(default)]
    pub collection_set: Option<String>,
    pub tmdb_id: Option<String>,
    pub bangumi_id: Option<String>,
    #[serde(default)]
    pub locked_fields: Vec<String>,
    pub has_nfo: bool,
    pub nfo_filename: String,
    pub has_poster: bool,
    pub has_fanart: bool,
    pub has_thumb: bool,
    #[serde(default)]
    pub has_clearlogo: bool,
    pub duration_secs: u32,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub video_codec: Option<String>,
    pub bit_rate_kbps: Option<u32>,
    pub audio_streams: Vec<ProbedAudioStream>,
    pub subtitle_streams: Vec<ProbedSubtitleStream>,
    pub sidecar_subtitles: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum MediaMetadataPayload {
    Book(BookFileMeta),
    Comic(ComicFileMeta),
    Music(MusicFileMeta),
    Video(VideoFileMeta),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaFileDetail {
    pub entry: MediaFileEntry,
    pub metadata: MediaMetadataPayload,
}

/// Patch payload for writing metadata back to the underlying file (or sidecar `.nfo` / `.lrc`).
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", content = "patch", rename_all = "snake_case")]
pub enum MediaMetadataPatch {
    Book(BookMetaPatch),
    Comic(ComicMetaPatch),
    Music(MusicMetaPatch),
    Video(VideoMetaPatch),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BookMetaPatch {
    pub title: Option<String>,
    pub authors: Option<Vec<String>>,
    #[serde(default)]
    pub series: Option<String>,
    #[serde(default)]
    pub subseries: Option<String>,
    #[serde(default)]
    pub series_index: Option<String>,
    pub description: Option<String>,
    pub publisher: Option<String>,
    pub published_at: Option<String>,
    pub language: Option<String>,
    pub isbn: Option<String>,
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub locked_fields: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ComicMetaPatch {
    pub series: Option<String>,
    #[serde(default)]
    pub series_group: Option<String>,
    pub title: Option<String>,
    pub number: Option<String>,
    pub volume: Option<i32>,
    pub summary: Option<String>,
    pub writers: Option<Vec<String>>,
    pub pencillers: Option<Vec<String>>,
    pub publisher: Option<String>,
    pub genres: Option<Vec<String>>,
    pub tags: Option<Vec<String>>,
    pub year: Option<i32>,
    pub language_iso: Option<String>,
    pub reading_direction: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MusicMetaPatch {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album_artist: Option<String>,
    pub album: Option<String>,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub year: Option<i32>,
    pub genre: Option<String>,
    pub comment: Option<String>,
    /// When `Some(...)`, writes embedded USLT/lyrics tag (empty string removes embedded lyrics).
    pub embedded_lyrics: Option<String>,
    /// When `Some(...)`, writes or deletes (`""`) the sidecar `<stem>.lrc` file.
    pub sidecar_lrc: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VideoMetaPatch {
    pub is_episode: Option<bool>,
    pub title: Option<String>,
    pub original_title: Option<String>,
    pub show_title: Option<String>,
    /// `< 0` clears `season_number` to `None`; `>= 0` (including `0` for Specials) sets `Some(s as u32)`.
    pub season_number: Option<i32>,
    pub episode_number: Option<u32>,
    pub year: Option<i32>,
    pub rating: Option<f32>,
    pub summary: Option<String>,
    pub genres: Option<Vec<String>>,
    pub tags: Option<Vec<String>>,
    pub actors: Option<Vec<String>>,
    pub directors: Option<Vec<String>>,
    pub studio: Option<String>,
    #[serde(default)]
    pub collection_set: Option<String>,
    pub tmdb_id: Option<String>,
    pub bangumi_id: Option<String>,
    #[serde(default)]
    pub locked_fields: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenameConflictPolicy {
    #[default]
    Skip,
    AutoSuffix,
    Overwrite,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenamePlanItem {
    pub from_rel_path: String,
    pub to_rel_path: String,
    pub companion_moves: Vec<(String, String)>,
    pub conflict: Option<String>,
    pub unchanged: bool,
    #[serde(default)]
    pub will_overwrite: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteFileErrorDto {
    pub rel_path: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteFilesReportDto {
    pub deleted_count: usize,
    pub deleted_paths: Vec<String>,
    pub deleted_companions: Vec<String>,
    pub failed: Vec<DeleteFileErrorDto>,
}

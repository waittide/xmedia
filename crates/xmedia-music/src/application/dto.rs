use std::path::PathBuf;

use bytes::Bytes;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use xmedia_core::domain::library::LibraryId;
use xmedia_core::domain::user::UserId;

use crate::domain::catalog::{AlbumId, ArtistId, AudioFormat, ReplayGain, TrackId};
use crate::domain::collection::PlaylistId;

#[derive(Debug, Clone)]
pub struct UploadFileItem {
    pub filename: String,
    pub bytes: Bytes,
}

#[derive(Debug, Clone)]
pub struct UploadFilePathItem {
    pub filename: String,
    pub temp_path: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
pub struct UploadBatchResultDto {
    pub uploaded_tracks: Vec<TrackDto>,
    pub attached_lyrics_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct GlobalScanStatusDto {
    pub scanning: bool,
    pub count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackDto {
    pub id: TrackId,
    pub library_id: LibraryId,
    pub album_id: AlbumId,
    pub artist_id: ArtistId,
    pub album_artist_id: ArtistId,
    pub title: String,
    pub artist_name: String,
    pub album_artist_name: String,
    pub album_title: String,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub year: Option<i32>,
    pub genre: Option<String>,
    pub duration_secs: u32,
    pub bit_rate_kbps: Option<u32>,
    pub sample_rate_hz: Option<u32>,
    pub bit_depth: Option<u16>,
    pub channels: Option<u8>,
    pub format: AudioFormat,
    pub mime_type: String,
    pub is_lossless: bool,
    pub file_size: u64,
    pub replay_gain: ReplayGain,
    pub has_lyrics: bool,
    pub has_embedded_lyrics: bool,
    pub has_sidecar_lyrics: bool,
    pub is_compilation: bool,
    pub cover_key: Option<String>,
    pub path: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub starred_at: Option<Timestamp>,
    pub user_rating: Option<u8>,
    pub play_count: u64,
    pub last_played_at: Option<Timestamp>,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlbumDto {
    pub id: AlbumId,
    pub library_id: LibraryId,
    pub title: String,
    pub album_artist_id: ArtistId,
    pub album_artist_name: String,
    pub year: Option<i32>,
    pub genre: Option<String>,
    pub disc_count: u32,
    pub track_count: u32,
    pub duration_secs: u32,
    pub is_compilation: bool,
    pub cover_key: Option<String>,
    pub starred_at: Option<Timestamp>,
    pub user_rating: Option<u8>,
    pub play_count: u64,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtistDto {
    pub id: ArtistId,
    pub name: String,
    pub is_album_artist: bool,
    pub is_track_artist: bool,
    pub album_count: u32,
    pub track_count: u32,
    pub cover_key: Option<String>,
    pub starred_at: Option<Timestamp>,
    pub user_rating: Option<u8>,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlbumDetailDto {
    pub album: AlbumDto,
    pub tracks: Vec<TrackDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtistDetailDto {
    pub artist: ArtistDto,
    pub albums: Vec<AlbumDto>,
    pub appears_on_tracks: Vec<TrackDto>,
    pub top_tracks: Vec<TrackDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResultDto {
    pub artists: Vec<ArtistDto>,
    pub albums: Vec<AlbumDto>,
    pub tracks: Vec<TrackDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogStatsDto {
    pub total_tracks: u64,
    pub total_albums: u64,
    pub total_artists: u64,
    pub starred_tracks: u64,
    pub playlists_count: u64,
    pub listening_count: u64,
    pub bookmarks_count: u64,
    pub genres_count: u64,
    pub tags_count: u64,
    pub libraries_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HomeOverviewDto {
    pub stats: CatalogStatsDto,
    pub recent_added_albums: Vec<AlbumDto>,
    pub frequent_albums: Vec<AlbumDto>,
    pub recent_played_tracks: Vec<TrackDto>,
    pub random_tracks: Vec<TrackDto>,
    pub bookmarks: Vec<BookmarkDto>,
    pub play_queue: Option<PlayQueueDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistSummaryDto {
    pub id: PlaylistId,
    pub owner_id: UserId,
    pub owner_username: String,
    pub name: String,
    pub comment: Option<String>,
    pub is_public: bool,
    pub track_count: u32,
    pub duration_secs: u32,
    pub cover_key: Option<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistDetailDto {
    pub playlist: PlaylistSummaryDto,
    pub tracks: Vec<TrackDto>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UpdatePlaylistInput {
    pub name: Option<String>,
    pub comment: Option<String>,
    pub is_public: Option<bool>,
    pub replace_track_ids: Option<Vec<TrackId>>,
    #[serde(default)]
    pub add_track_ids: Vec<TrackId>,
    #[serde(default)]
    pub remove_indices: Vec<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StarredBundleDto {
    pub artists: Vec<ArtistDto>,
    pub albums: Vec<AlbumDto>,
    pub tracks: Vec<TrackDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayQueueDto {
    pub tracks: Vec<TrackDto>,
    pub current_track_id: Option<TrackId>,
    pub position_ms: u64,
    pub changed_by: String,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookmarkDto {
    pub track: TrackDto,
    pub position_ms: u64,
    pub comment: Option<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NowPlayingDto {
    pub username: String,
    pub client_name: String,
    pub minutes_ago: u32,
    pub track: TrackDto,
}

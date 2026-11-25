use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::LibraryId;
pub use xmedia_core::domain::media::{PagedResult, SortOrder};

xmedia_core::define_uuid_id!(TrackId, AlbumId, ArtistId);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AudioFormat {
    Flac,
    Mp3,
    M4a,
    Alac,
    Aac,
    Ogg,
    Opus,
    Wav,
    Aiff,
    Ape,
    Wv,
}

impl AudioFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Flac => "flac",
            Self::Mp3 => "mp3",
            Self::M4a => "m4a",
            Self::Alac => "alac",
            Self::Aac => "aac",
            Self::Ogg => "ogg",
            Self::Opus => "opus",
            Self::Wav => "wav",
            Self::Aiff => "aiff",
            Self::Ape => "ape",
            Self::Wv => "wv",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Flac => "flac",
            Self::Mp3 => "mp3",
            Self::M4a | Self::Alac => "m4a",
            Self::Aac => "aac",
            Self::Ogg => "ogg",
            Self::Opus => "opus",
            Self::Wav => "wav",
            Self::Aiff => "aiff",
            Self::Ape => "ape",
            Self::Wv => "wv",
        }
    }

    pub fn mime_type(self) -> &'static str {
        match self {
            Self::Flac => "audio/flac",
            Self::Mp3 => "audio/mpeg",
            Self::M4a | Self::Alac => "audio/mp4",
            Self::Aac => "audio/aac",
            Self::Ogg => "audio/ogg",
            Self::Opus => "audio/opus",
            Self::Wav => "audio/wav",
            Self::Aiff => "audio/aiff",
            Self::Ape => "audio/x-ape",
            Self::Wv => "audio/x-wavpack",
        }
    }

    pub fn is_lossless(self) -> bool {
        matches!(
            self,
            Self::Flac | Self::Alac | Self::Wav | Self::Aiff | Self::Ape | Self::Wv
        )
    }

    pub fn parse(raw: &str) -> DomainResult<Self> {
        match raw
            .trim()
            .trim_start_matches('.')
            .to_ascii_lowercase()
            .as_str()
        {
            "flac" => Ok(Self::Flac),
            "mp3" => Ok(Self::Mp3),
            "m4a" | "mp4" => Ok(Self::M4a),
            "alac" => Ok(Self::Alac),
            "aac" => Ok(Self::Aac),
            "ogg" | "oga" => Ok(Self::Ogg),
            "opus" => Ok(Self::Opus),
            "wav" | "wave" => Ok(Self::Wav),
            "aiff" | "aif" | "aifc" => Ok(Self::Aiff),
            "ape" => Ok(Self::Ape),
            "wv" | "wavpack" => Ok(Self::Wv),
            other => Err(DomainError::UnsupportedFormat(format!(
                "unsupported audio format: {other}"
            ))),
        }
    }

    pub fn from_extension(ext: &str) -> Option<Self> {
        Self::parse(ext).ok()
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ReplayGain {
    pub track_gain_db: Option<f32>,
    pub track_peak: Option<f32>,
    pub album_gain_db: Option<f32>,
    pub album_peak: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OnlineTrackedTrackSource {
    #[serde(default)]
    pub storage_key: Option<String>,
    pub source_id: Uuid,
    pub item_id: String,
    #[serde(default)]
    pub detail_url: Option<String>,
    #[serde(default)]
    pub play_url: Option<String>,
    #[serde(default)]
    pub url_id: Option<String>,
    #[serde(default)]
    pub cover_url: Option<String>,
    #[serde(default)]
    pub cover_id: Option<String>,
    #[serde(default)]
    pub lyric_id: Option<String>,
    #[serde(default)]
    pub quality: Option<String>,
    #[serde(default)]
    pub cached: bool,
    #[serde(default)]
    pub sidecar_lrc_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TrackSource {
    Managed {
        storage_key: String,
        #[serde(default)]
        sidecar_lrc_key: Option<String>,
    },
    LocalPath {
        relative_path: String,
        file_mtime: i64,
        #[serde(default)]
        sidecar_lrc_path: Option<String>,
    },
    OnlineTracked(Box<OnlineTrackedTrackSource>),
}

impl TrackSource {
    pub fn display_path(&self) -> &str {
        match self {
            Self::Managed { storage_key, .. } => storage_key.as_str(),
            Self::LocalPath { relative_path, .. } => relative_path.as_str(),
            Self::OnlineTracked(info) => {
                info.storage_key.as_deref().unwrap_or(info.item_id.as_str())
            }
        }
    }

    pub fn is_online_tracked(&self) -> bool {
        matches!(self, Self::OnlineTracked(_))
    }

    pub fn is_cached(&self) -> bool {
        match self {
            Self::Managed { .. } | Self::LocalPath { .. } => true,
            Self::OnlineTracked(info) => info.cached && info.storage_key.is_some(),
        }
    }

    pub fn online_tracking_info(&self) -> Option<(Uuid, &str, bool)> {
        match self {
            Self::OnlineTracked(info) => Some((
                info.source_id,
                info.item_id.as_str(),
                info.cached && info.storage_key.is_some(),
            )),
            _ => None,
        }
    }

    pub fn managed_file_keys(&self) -> Vec<&str> {
        let mut keys = Vec::new();
        match self {
            Self::Managed {
                storage_key,
                sidecar_lrc_key,
            } => {
                keys.push(storage_key.as_str());
                if let Some(lrc) = sidecar_lrc_key {
                    keys.push(lrc.as_str());
                }
            }
            Self::OnlineTracked(info) => {
                if let Some(ref key) = info.storage_key {
                    keys.push(key.as_str());
                }
                if let Some(ref lrc) = info.sidecar_lrc_key {
                    keys.push(lrc.as_str());
                }
            }
            Self::LocalPath { .. } => {}
        }
        keys
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
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
    pub file_size: u64,
    pub file_hash: String,
    pub replay_gain: ReplayGain,
    pub has_embedded_lyrics: bool,
    pub has_sidecar_lyrics: bool,
    pub is_compilation: bool,
    pub cover_key: Option<String>,
    pub source: TrackSource,
    pub is_missing: bool,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl Track {
    pub fn has_lyrics(&self) -> bool {
        self.has_embedded_lyrics || self.has_sidecar_lyrics
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Album {
    pub id: AlbumId,
    pub library_id: LibraryId,
    pub album_key: String,
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
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl Album {
    pub fn compute_key(
        library_id: LibraryId,
        album_title: &str,
        album_artist_name: &str,
        is_compilation: bool,
    ) -> String {
        let norm_title = album_title.trim().to_lowercase();
        let norm_artist = if is_compilation {
            "__compilation__".to_string()
        } else {
            album_artist_name.trim().to_lowercase()
        };
        format!("{}:{norm_artist}:{norm_title}", library_id.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artist {
    pub id: ArtistId,
    pub name: String,
    pub is_album_artist: bool,
    pub is_track_artist: bool,
    pub album_count: u32,
    pub track_count: u32,
    pub cover_key: Option<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LyricLine {
    pub start_ms: u64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuredLyrics {
    pub lang: String,
    pub synced: bool,
    pub display_artist: Option<String>,
    pub display_title: Option<String>,
    pub offset_ms: i64,
    pub lines: Vec<LyricLine>,
    pub plain_text: String,
}

impl StructuredLyrics {
    pub fn empty() -> Self {
        Self {
            lang: "und".to_string(),
            synced: false,
            display_artist: None,
            display_title: None,
            offset_ms: 0,
            lines: Vec::new(),
            plain_text: String::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty() && self.plain_text.trim().is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TrackSortField {
    Title,
    Artist,
    Album,
    #[default]
    CreatedAt,
    Year,
    Duration,
    TrackOrder,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackQuery {
    pub library_id: Option<LibraryId>,
    pub album_id: Option<AlbumId>,
    pub artist_id: Option<ArtistId>,
    pub genre: Option<String>,
    #[serde(default)]
    pub tag: Option<String>,
    pub year_from: Option<i32>,
    pub year_to: Option<i32>,
    pub search: Option<String>,
    pub sort_by: TrackSortField,
    pub sort_order: SortOrder,
    pub page: u32,
    pub page_size: u32,
    #[serde(skip)]
    pub exclude_online_tracked: bool,
}

impl Default for TrackQuery {
    fn default() -> Self {
        Self {
            library_id: None,
            album_id: None,
            artist_id: None,
            genre: None,
            tag: None,
            year_from: None,
            year_to: None,
            search: None,
            sort_by: TrackSortField::CreatedAt,
            sort_order: SortOrder::Desc,
            page: 1,
            page_size: 50,
            exclude_online_tracked: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum AlbumListType {
    #[default]
    Newest,
    Recent,
    Frequent,
    Random,
    Starred,
    AlphabeticalByName,
    AlphabeticalByArtist,
    ByYear,
    ByGenre,
}

impl AlbumListType {
    pub fn parse(raw: &str) -> DomainResult<Self> {
        match raw.trim() {
            "newest" => Ok(Self::Newest),
            "recent" => Ok(Self::Recent),
            "frequent" => Ok(Self::Frequent),
            "random" => Ok(Self::Random),
            "starred" => Ok(Self::Starred),
            "alphabeticalByName" | "alphabetical_by_name" => Ok(Self::AlphabeticalByName),
            "alphabeticalByArtist" | "alphabetical_by_artist" => Ok(Self::AlphabeticalByArtist),
            "byYear" | "by_year" => Ok(Self::ByYear),
            "byGenre" | "by_genre" => Ok(Self::ByGenre),
            other => Err(DomainError::Validation(format!(
                "unsupported album list type: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlbumListQuery {
    pub list_type: AlbumListType,
    pub library_id: Option<LibraryId>,
    pub genre: Option<String>,
    pub from_year: Option<i32>,
    pub to_year: Option<i32>,
    pub search: Option<String>,
    pub offset: u32,
    pub size: u32,
}

impl Default for AlbumListQuery {
    fn default() -> Self {
        Self {
            list_type: AlbumListType::Newest,
            library_id: None,
            genre: None,
            from_year: None,
            to_year: None,
            search: None,
            offset: 0,
            size: 30,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ArtistRoleFilter {
    #[default]
    All,
    AlbumArtist,
    TrackArtist,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtistQuery {
    pub library_id: Option<LibraryId>,
    pub search: Option<String>,
    pub role_filter: ArtistRoleFilter,
    pub offset: u32,
    pub size: u32,
}

impl Default for ArtistQuery {
    fn default() -> Self {
        Self {
            library_id: None,
            search: None,
            role_filter: ArtistRoleFilter::All,
            offset: 0,
            size: 100,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenreStat {
    pub name: String,
    pub song_count: u64,
    pub album_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MusicTagStat {
    pub name: String,
    pub song_count: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BoundCatalogCounts {
    pub active_track_ids: std::collections::HashSet<TrackId>,
    pub active_album_ids: std::collections::HashSet<AlbumId>,
    pub total_tracks: u64,
    pub total_albums: u64,
    pub total_artists: u64,
    pub genres_count: u64,
    pub libraries_count: u64,
    pub tags_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchQuery {
    pub query: String,
    pub library_id: Option<LibraryId>,
    pub artist_count: u32,
    pub artist_offset: u32,
    pub album_count: u32,
    pub album_offset: u32,
    pub song_count: u32,
    pub song_offset: u32,
}

impl Default for SearchQuery {
    fn default() -> Self {
        Self {
            query: String::new(),
            library_id: None,
            artist_count: 20,
            artist_offset: 0,
            album_count: 20,
            album_offset: 0,
            song_count: 20,
            song_offset: 0,
        }
    }
}

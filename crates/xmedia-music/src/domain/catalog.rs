mod queries;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::LibraryId;
pub use xmedia_core::domain::media::{PagedResult, SortOrder};

pub use queries::{
    AlbumListQuery, AlbumListType, ArtistQuery, ArtistRoleFilter, BoundCatalogCounts, GenreStat,
    MusicTagStat, SearchQuery, TrackQuery, TrackSortField,
};

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

    pub fn attach_managed_sidecar_lrc(&mut self, lrc_key: String, now: Timestamp) -> bool {
        if let TrackSource::Managed {
            ref storage_key, ..
        } = self.source
        {
            self.source = TrackSource::Managed {
                storage_key: storage_key.clone(),
                sidecar_lrc_key: Some(lrc_key),
            };
            self.has_sidecar_lyrics = true;
            self.updated_at = now;
            true
        } else {
            false
        }
    }

    pub fn attach_online_sidecar_lrc(&mut self, lrc_key: String, now: Timestamp) {
        if let TrackSource::OnlineTracked(ref mut info) = self.source {
            info.sidecar_lrc_key = Some(lrc_key);
        }
        self.has_sidecar_lyrics = true;
        self.updated_at = now;
    }

    pub fn mark_present(&mut self, now: Timestamp) -> bool {
        if self.is_missing {
            self.is_missing = false;
            self.updated_at = now;
            true
        } else {
            false
        }
    }

    pub fn mark_missing(&mut self, now: Timestamp) -> bool {
        if !self.is_missing {
            self.is_missing = true;
            self.updated_at = now;
            true
        } else {
            false
        }
    }

    pub fn set_cover_if_absent(&mut self, cover_key: String) -> bool {
        if self.cover_key.is_none() {
            self.cover_key = Some(cover_key);
            true
        } else {
            false
        }
    }

    pub fn set_bit_rate_kbps(&mut self, bit_rate_kbps: u32) {
        self.bit_rate_kbps = Some(bit_rate_kbps);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn apply_probed_metadata(
        &mut self,
        duration_secs: u32,
        bit_rate_kbps: Option<u32>,
        sample_rate_hz: Option<u32>,
        bit_depth: Option<u16>,
        channels: Option<u8>,
        year: Option<i32>,
        genre: Option<String>,
        replay_gain: ReplayGain,
        has_embedded_lyrics: bool,
    ) {
        if duration_secs > 0 {
            self.duration_secs = duration_secs;
        }
        if bit_rate_kbps.is_some() {
            self.bit_rate_kbps = bit_rate_kbps;
        }
        if sample_rate_hz.is_some() {
            self.sample_rate_hz = sample_rate_hz;
        }
        if bit_depth.is_some() {
            self.bit_depth = bit_depth;
        }
        if channels.is_some() {
            self.channels = channels;
        }
        if year.is_some() && self.year.is_none() {
            self.year = year;
        }
        if genre.is_some() && self.genre.is_none() {
            self.genre = genre;
        }
        self.replay_gain = replay_gain;
        self.has_embedded_lyrics = has_embedded_lyrics;
    }

    pub fn finalize_online_cached(
        &mut self,
        format: AudioFormat,
        file_size: u64,
        file_hash: String,
        cached_source: OnlineTrackedTrackSource,
        has_sidecar_lyrics: bool,
        now: Timestamp,
    ) {
        if has_sidecar_lyrics {
            self.has_sidecar_lyrics = true;
        }
        self.format = format;
        self.file_size = file_size;
        self.file_hash = file_hash;
        self.source = TrackSource::OnlineTracked(Box::new(cached_source));
        self.updated_at = now;
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
    pub fn normalize_title(raw_title: &str) -> String {
        let trimmed = raw_title.trim();
        if trimmed.is_empty() {
            "Unknown Album".to_string()
        } else {
            trimmed.to_string()
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        library_id: LibraryId,
        raw_title: &str,
        album_artist_id: ArtistId,
        album_artist_name: &str,
        year: Option<i32>,
        genre: Option<&str>,
        is_compilation: bool,
        cover_hint: Option<&str>,
        now: Timestamp,
    ) -> Self {
        let title = Self::normalize_title(raw_title);
        let album_key = Self::compute_key(library_id, &title, album_artist_name, is_compilation);
        Self {
            id: AlbumId::new(),
            library_id,
            album_key,
            title,
            album_artist_id,
            album_artist_name: album_artist_name.to_string(),
            year,
            genre: genre.map(str::to_string),
            disc_count: 1,
            track_count: 0,
            duration_secs: 0,
            is_compilation,
            cover_key: cover_hint.map(str::to_string),
            created_at: now,
            updated_at: now,
        }
    }

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

    pub fn merge_metadata(
        &mut self,
        year: Option<i32>,
        genre: Option<&str>,
        cover_hint: Option<&str>,
        now: Timestamp,
    ) -> bool {
        let mut dirty = false;
        if year.is_some() && self.year != year {
            self.year = year;
            dirty = true;
        }
        if genre.is_some() && self.genre.as_deref() != genre {
            self.genre = genre.map(str::to_string);
            dirty = true;
        }
        if self.cover_key.is_none()
            && let Some(cov) = cover_hint
        {
            self.cover_key = Some(cov.to_string());
            dirty = true;
        }
        if dirty {
            self.updated_at = now;
        }
        dirty
    }

    pub fn set_cover_if_absent(&mut self, cover_key: &str, now: Timestamp) -> bool {
        if self.cover_key.is_none() {
            self.cover_key = Some(cover_key.to_string());
            self.updated_at = now;
            true
        } else {
            false
        }
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

impl Artist {
    pub fn normalize_name(raw_name: &str) -> String {
        let trimmed = raw_name.trim();
        if trimmed.is_empty() {
            "Unknown Artist".to_string()
        } else {
            trimmed.to_string()
        }
    }

    pub fn new(
        raw_name: &str,
        is_album_artist: bool,
        is_track_artist: bool,
        cover_hint: Option<&str>,
        now: Timestamp,
    ) -> Self {
        Self {
            id: ArtistId::new(),
            name: Self::normalize_name(raw_name),
            is_album_artist,
            is_track_artist,
            album_count: 0,
            track_count: 0,
            cover_key: cover_hint.map(str::to_string),
            created_at: now,
            updated_at: now,
        }
    }

    pub fn mark_album_artist(&mut self, now: Timestamp) -> bool {
        if !self.is_album_artist {
            self.is_album_artist = true;
            self.updated_at = now;
            true
        } else {
            false
        }
    }

    pub fn merge_roles_and_cover(
        &mut self,
        as_album_artist: bool,
        as_track_artist: bool,
        cover_hint: Option<&str>,
        now: Timestamp,
    ) -> bool {
        let mut dirty = false;
        if as_album_artist && !self.is_album_artist {
            self.is_album_artist = true;
            dirty = true;
        }
        if as_track_artist && !self.is_track_artist {
            self.is_track_artist = true;
            dirty = true;
        }
        if self.cover_key.is_none()
            && let Some(cov) = cover_hint
        {
            self.cover_key = Some(cov.to_string());
            dirty = true;
        }
        if dirty {
            self.updated_at = now;
        }
        dirty
    }

    pub fn set_cover_if_absent(&mut self, cover_key: &str, now: Timestamp) -> bool {
        if self.cover_key.is_none() {
            self.cover_key = Some(cover_key.to_string());
            self.updated_at = now;
            true
        } else {
            false
        }
    }
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

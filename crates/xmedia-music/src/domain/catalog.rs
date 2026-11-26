mod queries;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::LibraryId;
pub use xmedia_core::domain::media::{PagedResult, SortOrder};
use xmedia_formats::music::{
    StructuredLyrics as FormatStructuredLyrics, parse_lyrics_str as format_parse_lyrics_str,
};

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

impl AsRef<str> for AudioFormat {
    fn as_ref(&self) -> &str {
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
}

impl std::fmt::Display for AudioFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_ref())
    }
}

impl std::str::FromStr for AudioFormat {
    type Err = DomainError;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        let token = normalize_audio_token(raw);
        match_audio_format(&token)
    }
}

fn normalize_audio_token(raw: &str) -> String {
    raw.trim().trim_start_matches('.').to_ascii_lowercase()
}

fn match_audio_format(token: &str) -> DomainResult<AudioFormat> {
    match token {
        "flac" => Ok(AudioFormat::Flac),
        "mp3" => Ok(AudioFormat::Mp3),
        "m4a" | "mp4" => Ok(AudioFormat::M4a),
        "alac" => Ok(AudioFormat::Alac),
        "aac" => Ok(AudioFormat::Aac),
        "ogg" | "oga" => Ok(AudioFormat::Ogg),
        "opus" => Ok(AudioFormat::Opus),
        "wav" | "wave" => Ok(AudioFormat::Wav),
        "aiff" | "aif" | "aifc" => Ok(AudioFormat::Aiff),
        "ape" => Ok(AudioFormat::Ape),
        "wv" | "wavpack" => Ok(AudioFormat::Wv),
        other => Err(DomainError::UnsupportedFormat(format!(
            "unsupported audio format: {other}"
        ))),
    }
}

impl AudioFormat {
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

#[derive(Debug, Clone)]
pub struct ProbedTrackInit {
    pub id: TrackId,
    pub library_id: LibraryId,
    pub album_id: AlbumId,
    pub album_title: String,
    pub artist_id: ArtistId,
    pub artist_name: String,
    pub album_artist_id: ArtistId,
    pub album_artist_name: String,
    pub title: String,
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
    pub existing_created_at: Option<Timestamp>,
}

#[derive(Debug, Clone)]
pub struct OnlineTrackedInit {
    pub id: TrackId,
    pub library_id: LibraryId,
    pub album_id: AlbumId,
    pub album_title: String,
    pub artist_id: ArtistId,
    pub artist_name: String,
    pub title: String,
    pub duration_secs: u32,
    pub format: AudioFormat,
    pub cover_key: Option<String>,
    pub tracked: OnlineTrackedTrackSource,
}

#[derive(Debug, Clone)]
pub struct ProbedCacheFields {
    pub duration_secs: u32,
    pub bit_rate_kbps: Option<u32>,
    pub sample_rate_hz: Option<u32>,
    pub bit_depth: Option<u16>,
    pub channels: Option<u8>,
    pub year: Option<i32>,
    pub genre: Option<String>,
    pub replay_gain: ReplayGain,
    pub has_embedded_lyrics: bool,
}

#[derive(Debug, Clone)]
pub struct OnlineAudioCacheUpdate {
    pub probed: Option<ProbedCacheFields>,
    pub fallback_bitrate_kbps: Option<u32>,
    pub cover_key: Option<String>,
    pub format: AudioFormat,
    pub file_size: u64,
    pub file_hash: String,
    pub cached_source: OnlineTrackedTrackSource,
    pub added_sidecar_lyrics: bool,
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
    pub fn from_probed(init: ProbedTrackInit, now: Timestamp) -> Self {
        Self {
            id: init.id,
            library_id: init.library_id,
            album_id: init.album_id,
            artist_id: init.artist_id,
            album_artist_id: init.album_artist_id,
            title: init.title,
            artist_name: init.artist_name,
            album_artist_name: init.album_artist_name,
            album_title: init.album_title,
            track_number: init.track_number,
            disc_number: init.disc_number,
            year: init.year,
            genre: init.genre,
            duration_secs: init.duration_secs,
            bit_rate_kbps: init.bit_rate_kbps,
            sample_rate_hz: init.sample_rate_hz,
            bit_depth: init.bit_depth,
            channels: init.channels,
            format: init.format,
            file_size: init.file_size,
            file_hash: init.file_hash,
            replay_gain: init.replay_gain,
            has_embedded_lyrics: init.has_embedded_lyrics,
            has_sidecar_lyrics: init.has_sidecar_lyrics,
            is_compilation: init.is_compilation,
            cover_key: init.cover_key,
            source: init.source,
            is_missing: false,
            created_at: init.existing_created_at.unwrap_or(now),
            updated_at: now,
        }
    }

    pub fn new_online_tracked(init: OnlineTrackedInit, now: Timestamp) -> Self {
        let file_hash = online_track_hash(init.tracked.source_id, &init.tracked.item_id);
        Self {
            id: init.id,
            library_id: init.library_id,
            album_id: init.album_id,
            artist_id: init.artist_id,
            album_artist_id: init.artist_id,
            title: init.title,
            artist_name: init.artist_name.clone(),
            album_artist_name: init.artist_name,
            album_title: init.album_title,
            track_number: None,
            disc_number: None,
            year: None,
            genre: None,
            duration_secs: init.duration_secs,
            bit_rate_kbps: None,
            sample_rate_hz: None,
            bit_depth: None,
            channels: None,
            format: init.format,
            file_size: 0,
            file_hash,
            replay_gain: ReplayGain::default(),
            has_embedded_lyrics: false,
            has_sidecar_lyrics: false,
            is_compilation: false,
            cover_key: init.cover_key,
            source: TrackSource::OnlineTracked(Box::new(init.tracked)),
            is_missing: false,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn has_lyrics(&self) -> bool {
        self.has_embedded_lyrics || self.has_sidecar_lyrics
    }

    pub fn is_cached(&self) -> bool {
        match &self.source {
            TrackSource::Managed { .. } | TrackSource::LocalPath { .. } => true,
            TrackSource::OnlineTracked(info) => info.cached && info.storage_key.is_some(),
        }
    }

    pub fn attach_sidecar_lrc(&mut self, lrc_key: String, now: Timestamp) -> bool {
        match self.source {
            TrackSource::Managed { .. } => self.attach_managed_lrc(lrc_key, now),
            TrackSource::OnlineTracked(_) => self.attach_online_lrc(lrc_key, now),
            TrackSource::LocalPath { .. } => false,
        }
    }

    pub fn set_missing(&mut self, missing: bool, now: Timestamp) -> bool {
        if self.is_missing == missing {
            return false;
        }
        self.is_missing = missing;
        self.updated_at = now;
        true
    }

    pub fn cache_online_audio(&mut self, update: OnlineAudioCacheUpdate, now: Timestamp) {
        if let Some(probed) = update.probed {
            self.apply_probed_fields(probed);
        } else if let Some(br) = update.fallback_bitrate_kbps {
            self.bit_rate_kbps = Some(br);
        }
        if let Some(cov) = update.cover_key {
            self.ensure_cover_key(cov);
        }
        self.update_online_cached_source(
            update.format,
            update.file_size,
            update.file_hash,
            update.cached_source,
            update.added_sidecar_lyrics,
            now,
        );
    }

    fn attach_managed_lrc(&mut self, lrc_key: String, now: Timestamp) -> bool {
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

    fn attach_online_lrc(&mut self, lrc_key: String, now: Timestamp) -> bool {
        if let TrackSource::OnlineTracked(ref mut info) = self.source {
            info.sidecar_lrc_key = Some(lrc_key);
            self.has_sidecar_lyrics = true;
            self.updated_at = now;
            true
        } else {
            false
        }
    }

    fn apply_probed_fields(&mut self, probed: ProbedCacheFields) {
        if probed.duration_secs > 0 {
            self.duration_secs = probed.duration_secs;
        }
        if probed.bit_rate_kbps.is_some() {
            self.bit_rate_kbps = probed.bit_rate_kbps;
        }
        if probed.sample_rate_hz.is_some() {
            self.sample_rate_hz = probed.sample_rate_hz;
        }
        if probed.bit_depth.is_some() {
            self.bit_depth = probed.bit_depth;
        }
        if probed.channels.is_some() {
            self.channels = probed.channels;
        }
        self.apply_optional_year_genre(probed.year, probed.genre);
        self.replay_gain = probed.replay_gain;
        self.has_embedded_lyrics = probed.has_embedded_lyrics;
    }

    fn apply_optional_year_genre(&mut self, year: Option<i32>, genre: Option<String>) {
        if year.is_some() && self.year.is_none() {
            self.year = year;
        }
        if genre.is_some() && self.genre.is_none() {
            self.genre = genre;
        }
    }

    fn ensure_cover_key(&mut self, cover_key: String) {
        if self.cover_key.is_none() {
            self.cover_key = Some(cover_key);
        }
    }

    fn update_online_cached_source(
        &mut self,
        format: AudioFormat,
        file_size: u64,
        file_hash: String,
        cached_source: OnlineTrackedTrackSource,
        added_sidecar_lyrics: bool,
        now: Timestamp,
    ) {
        if added_sidecar_lyrics {
            self.has_sidecar_lyrics = true;
        }
        self.format = format;
        self.file_size = file_size;
        self.file_hash = file_hash;
        self.source = TrackSource::OnlineTracked(Box::new(cached_source));
        self.updated_at = now;
    }
}

fn online_track_hash(source_id: Uuid, item_id: &str) -> String {
    format!("online:{source_id}:{item_id}")
}

#[derive(Debug, Clone)]
pub struct AlbumSpec<'a> {
    pub library_id: LibraryId,
    pub raw_title: &'a str,
    pub album_artist_id: ArtistId,
    pub album_artist_name: &'a str,
    pub year: Option<i32>,
    pub genre: Option<&'a str>,
    pub is_compilation: bool,
    pub cover_hint: Option<&'a str>,
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
    pub fn lookup_key(
        library_id: LibraryId,
        raw_title: &str,
        album_artist_name: &str,
        is_compilation: bool,
    ) -> (String, String) {
        let title = normalize_album_title(raw_title);
        let key = compute_album_key(library_id, &title, album_artist_name, is_compilation);
        (title, key)
    }

    pub fn reconcile(existing: Option<Self>, spec: AlbumSpec<'_>, now: Timestamp) -> (Self, bool) {
        match existing {
            Some(mut album) => {
                let dirty = album.merge_album_metadata(spec.year, spec.genre, spec.cover_hint, now);
                (album, dirty)
            }
            None => (Self::create_album(spec, now), true),
        }
    }

    pub fn set_cover_if_absent(&mut self, cover_key: &str, now: Timestamp) -> bool {
        self.apply_optional_album_cover(Some(cover_key), now)
    }

    fn create_album(spec: AlbumSpec<'_>, now: Timestamp) -> Self {
        let (title, album_key) = Self::lookup_key(
            spec.library_id,
            spec.raw_title,
            spec.album_artist_name,
            spec.is_compilation,
        );
        Self {
            id: AlbumId::new(),
            library_id: spec.library_id,
            album_key,
            title,
            album_artist_id: spec.album_artist_id,
            album_artist_name: spec.album_artist_name.to_string(),
            year: spec.year,
            genre: spec.genre.map(str::to_string),
            disc_count: 1,
            track_count: 0,
            duration_secs: 0,
            is_compilation: spec.is_compilation,
            cover_key: spec.cover_hint.map(str::to_string),
            created_at: now,
            updated_at: now,
        }
    }

    fn merge_album_metadata(
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
        if self.apply_optional_album_cover(cover_hint, now) {
            return true;
        }
        if dirty {
            self.updated_at = now;
        }
        dirty
    }

    fn apply_optional_album_cover(&mut self, cover_hint: Option<&str>, now: Timestamp) -> bool {
        if self.cover_key.is_none()
            && let Some(cov) = cover_hint
        {
            self.cover_key = Some(cov.to_string());
            self.updated_at = now;
            true
        } else {
            false
        }
    }
}

fn normalize_album_title(raw_title: &str) -> String {
    let trimmed = raw_title.trim();
    if trimmed.is_empty() {
        "Unknown Album".to_string()
    } else {
        trimmed.to_string()
    }
}

fn compute_album_key(
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

    pub fn reconcile(
        existing: Option<Self>,
        raw_name: &str,
        as_album_artist: bool,
        as_track_artist: bool,
        cover_hint: Option<&str>,
        now: Timestamp,
    ) -> (Self, bool) {
        match existing {
            Some(mut artist) => {
                let dirty =
                    artist.merge_artist_roles(as_album_artist, as_track_artist, cover_hint, now);
                (artist, dirty)
            }
            None => (
                Self::create_artist(raw_name, as_album_artist, as_track_artist, cover_hint, now),
                true,
            ),
        }
    }

    pub fn set_cover_if_absent(&mut self, cover_key: &str, now: Timestamp) -> bool {
        self.apply_optional_artist_cover(Some(cover_key), now)
    }

    fn create_artist(
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

    fn merge_artist_roles(
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
        if self.apply_optional_artist_cover(cover_hint, now) {
            return true;
        }
        if dirty {
            self.updated_at = now;
        }
        dirty
    }

    fn apply_optional_artist_cover(&mut self, cover_hint: Option<&str>, now: Timestamp) -> bool {
        if self.cover_key.is_none()
            && let Some(cov) = cover_hint
        {
            self.cover_key = Some(cov.to_string());
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

impl Default for StructuredLyrics {
    fn default() -> Self {
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
}

impl From<FormatStructuredLyrics> for StructuredLyrics {
    fn from(src: FormatStructuredLyrics) -> Self {
        convert_format_lyrics(src)
    }
}

impl From<&str> for StructuredLyrics {
    fn from(raw: &str) -> Self {
        convert_format_lyrics(format_parse_lyrics_str(raw))
    }
}

fn convert_format_lyrics(src: FormatStructuredLyrics) -> StructuredLyrics {
    StructuredLyrics {
        lang: src.lang,
        synced: src.synced,
        display_artist: src.display_artist,
        display_title: src.display_title,
        offset_ms: src.offset_ms,
        lines: src.lines.into_iter().map(convert_lyric_line).collect(),
        plain_text: src.plain_text,
    }
}

fn convert_lyric_line(line: xmedia_formats::music::LyricLine) -> LyricLine {
    LyricLine {
        start_ms: line.start_ms,
        text: line.text,
    }
}

impl StructuredLyrics {
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty() && self.plain_text.trim().is_empty()
    }

    pub fn to_lrc(&self, raw_hint: &str) -> Option<String> {
        if !raw_hint.trim().is_empty() {
            return Some(raw_hint.to_string());
        }
        if self.synced && !self.lines.is_empty() {
            let formatted = self.format_synced_lines();
            if !formatted.trim().is_empty() {
                return Some(formatted);
            }
        }
        if !self.plain_text.trim().is_empty() {
            return Some(self.plain_text.clone());
        }
        None
    }

    fn format_synced_lines(&self) -> String {
        let mut out = String::new();
        for line in &self.lines {
            let total_secs = line.start_ms / 1000;
            let mins = total_secs / 60;
            let secs = total_secs % 60;
            let cs = (line.start_ms % 1000) / 10;
            out.push_str(&format!("[{mins:02}:{secs:02}.{cs:02}]{}\n", line.text));
        }
        out
    }
}

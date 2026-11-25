pub mod lyrics;
pub mod tags;

use std::fmt;
use std::fs::File;
use std::io::{BufReader, Cursor, Read, Seek};
use std::path::Path;

use lofty::config::{ParseOptions, ParsingMode};
use lofty::file::{AudioFile, FileType, TaggedFile, TaggedFileExt};
use lofty::id3::v2::{Frame, SynchronizedTextFrame};
use lofty::mp4::{Mp4Codec, Mp4File};
use lofty::mpeg::MpegFile;
use lofty::picture::PictureType;
use lofty::probe::Probe;
use lofty::tag::{Accessor, ItemKey, Tag, TagType};
use serde::{Deserialize, Serialize};

use crate::common::{BinaryAsset, sniff_image_mime};
use crate::error::{FormatError, FormatResult};

pub use lyrics::{
    LyricLine, StructuredLyrics, extract_embedded_lyrics_from_path,
    extract_embedded_lyrics_from_reader, parse_lrc_bytes, parse_lyrics_str, resolve_hybrid_lyrics,
    shift_and_clean_lrc_text,
};
pub use tags::{
    apply_audio_patch_in_place, ensure_mutable_primary_tag, find_album_cover_path,
    is_generic_music_folder, read_embedded_cover_from_path, read_sidecar_lrc, sidecar_lrc_path,
    write_embedded_cover_in_place, write_replaygain_in_place,
};

/// Canonical audio file formats supported across `xmedia-music` and `xmedia-tool`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AudioFormatExt {
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

impl AudioFormatExt {
    pub const ALL: &'static [Self] = &[
        Self::Flac,
        Self::Mp3,
        Self::M4a,
        Self::Alac,
        Self::Aac,
        Self::Ogg,
        Self::Opus,
        Self::Wav,
        Self::Aiff,
        Self::Ape,
        Self::Wv,
    ];

    pub const ALL_EXTENSIONS: &'static [&'static str] = &[
        "flac", "mp3", "m4a", "alac", "aac", "ogg", "opus", "wav", "aiff", "aif", "ape", "wv",
    ];

    pub fn from_extension(ext: &str) -> Option<Self> {
        let clean = ext.trim().trim_start_matches('.').to_ascii_lowercase();
        match clean.as_str() {
            "flac" => Some(Self::Flac),
            "mp3" => Some(Self::Mp3),
            "m4a" | "mp4" => Some(Self::M4a),
            "alac" => Some(Self::Alac),
            "aac" => Some(Self::Aac),
            "ogg" | "oga" => Some(Self::Ogg),
            "opus" => Some(Self::Opus),
            "wav" => Some(Self::Wav),
            "aiff" | "aif" | "aifc" => Some(Self::Aiff),
            "ape" => Some(Self::Ape),
            "wv" | "wavpack" => Some(Self::Wv),
            _ => None,
        }
    }

    pub const fn as_str(self) -> &'static str {
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

    pub const fn mime_type(self) -> &'static str {
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

    pub const fn is_lossless(self) -> bool {
        matches!(
            self,
            Self::Flac | Self::Alac | Self::Wav | Self::Aiff | Self::Ape | Self::Wv
        )
    }
}

impl fmt::Display for AudioFormatExt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// ReplayGain 2.0 loudness normalization tags.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct ReplayGainInfo {
    pub track_gain_db: Option<f32>,
    pub track_peak: Option<f32>,
    pub album_gain_db: Option<f32>,
    pub album_peak: Option<f32>,
}

impl ReplayGainInfo {
    pub fn is_empty(&self) -> bool {
        self.track_gain_db.is_none()
            && self.track_peak.is_none()
            && self.album_gain_db.is_none()
            && self.album_peak.is_none()
    }
}

/// Unified parsed audio metadata, technical stream properties, ReplayGain, lyrics, and artwork.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedAudioMeta {
    pub format: AudioFormatExt,
    pub title: String,
    pub artist: Option<String>,
    pub track_artist: String,
    pub album_artist: Option<String>,
    pub effective_album_artist: String,
    pub album: Option<String>,
    pub album_title: String,
    pub track_number: Option<u32>,
    pub total_tracks: Option<u32>,
    pub disc_number: Option<u32>,
    pub total_discs: Option<u32>,
    pub year: Option<i32>,
    pub genre: Option<String>,
    pub comment: Option<String>,
    pub is_compilation: bool,
    pub duration_secs: u32,
    pub bit_rate_kbps: Option<u32>,
    pub sample_rate_hz: Option<u32>,
    pub bit_depth: Option<u16>,
    pub channels: Option<u8>,
    pub replay_gain: ReplayGainInfo,
    pub has_embedded_lyrics: bool,
    pub embedded_lyrics: Option<String>,
    pub cover: Option<BinaryAsset>,
}

/// Partial update payload for writing embedded audio tags.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AudioMetaPatch {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album_artist: Option<String>,
    pub album: Option<String>,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub year: Option<i32>,
    pub genre: Option<String>,
    pub comment: Option<String>,
    pub is_compilation: Option<bool>,
    pub embedded_lyrics: Option<String>,
    pub replay_gain: Option<ReplayGainInfo>,
}

/// Opens a `lofty::file::TaggedFile` from a `Read + Seek` stream using relaxed parsing.
pub fn open_tagged_reader<R: Read + Seek>(
    reader: &mut R,
    ext_hint: &str,
) -> FormatResult<TaggedFile> {
    let opts = ParseOptions::new().parsing_mode(ParsingMode::Relaxed);
    let clean_ext = ext_hint.trim().trim_start_matches('.');
    let probe = match Probe::new(&mut *reader).options(opts).guess_file_type() {
        Ok(guessed) if guessed.file_type().is_some() => guessed,
        _ => {
            let _ = reader.rewind();
            let ft = FileType::from_ext(clean_ext).ok_or_else(|| {
                FormatError::UnsupportedFormat("failed to detect audio container format".into())
            })?;
            Probe::new(&mut *reader).options(opts).set_file_type(ft)
        }
    };
    probe
        .read()
        .map_err(|e| FormatError::Validation(format!("failed to parse audio tags: {e}")))
}

/// Opens a `lofty::file::TaggedFile` from disk.
pub fn open_tagged_path(path: &Path) -> FormatResult<TaggedFile> {
    let file = File::open(path)
        .map_err(|e| FormatError::Io(format!("failed opening '{}': {e}", path.display())))?;
    let mut reader = BufReader::new(file);
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    open_tagged_reader(&mut reader, ext)
}

/// Probes audio metadata, technical properties, ReplayGain, compilation flag, and embedded cover/lyrics from disk.
pub fn probe_audio_path(
    path: &Path,
    ext_hint: &str,
    fallback_title: &str,
) -> FormatResult<ParsedAudioMeta> {
    let file = File::open(path)
        .map_err(|e| FormatError::Io(format!("failed opening '{}': {e}", path.display())))?;
    let mut reader = BufReader::new(file);
    let effective_ext = if !ext_hint.trim().is_empty() {
        ext_hint
    } else {
        path.extension().and_then(|s| s.to_str()).unwrap_or("")
    };
    let effective_fallback = if !fallback_title.trim().is_empty() {
        fallback_title
    } else {
        path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Untitled")
    };
    probe_audio_reader(&mut reader, effective_ext, effective_fallback)
}

/// Probes audio metadata from an in-memory byte slice.
pub fn probe_audio_bytes(
    bytes: &[u8],
    ext_hint: &str,
    fallback_title: &str,
) -> FormatResult<ParsedAudioMeta> {
    let mut cursor = Cursor::new(bytes);
    probe_audio_reader(&mut cursor, ext_hint, fallback_title)
}

/// Probes audio metadata from any `Read + Seek` reader.
pub fn probe_audio_reader<R: Read + Seek>(
    reader: &mut R,
    ext_hint: &str,
    fallback_title: &str,
) -> FormatResult<ParsedAudioMeta> {
    let opts = ParseOptions::new().parsing_mode(ParsingMode::Relaxed);
    let tagged = open_tagged_reader(reader, ext_hint)?;
    let mut format = map_lofty_file_type(tagged.file_type(), ext_hint)?;
    let mut sylt_lrc: Option<String> = None;

    if tagged.file_type() == FileType::Mp4 {
        if reader.rewind().is_ok()
            && let Ok(mp4) = Mp4File::read_from(&mut *reader, opts)
            && mp4.properties().codec() == Some(Mp4Codec::ALAC)
        {
            format = AudioFormatExt::Alac;
        }
    } else if tagged.file_type() == FileType::Mpeg
        && reader.rewind().is_ok()
        && let Ok(mp3) = MpegFile::read_from(&mut *reader, opts)
        && let Some(id3v2) = mp3.id3v2()
    {
        for frame in id3v2 {
            if frame.id_str() == "SYLT"
                && let Frame::Binary(bin) = frame
            {
                if let Ok(sylt) = SynchronizedTextFrame::parse(&bin.data, frame.flags())
                    && !sylt.content.is_empty()
                {
                    let mut entries = sylt.content;
                    entries.sort_by_key(|(ts, _)| *ts);
                    let rendered = entries
                        .into_iter()
                        .map(|(ts, txt)| {
                            let mins = ts / 60_000;
                            let secs = (ts % 60_000) / 1_000;
                            let cs = (ts % 1_000) / 10;
                            format!("[{mins:02}:{secs:02}.{cs:02}]{}", txt.trim())
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    sylt_lrc = Some(rendered);
                } else {
                    sylt_lrc = Some(String::new());
                }
                break;
            }
        }
    }

    Ok(extract_audio_metadata(
        &tagged,
        format,
        fallback_title,
        sylt_lrc,
    ))
}

fn map_lofty_file_type(ft: FileType, ext_hint: &str) -> FormatResult<AudioFormatExt> {
    let ext_clean = ext_hint.trim().trim_start_matches('.').to_ascii_lowercase();
    match ft {
        FileType::Flac => Ok(AudioFormatExt::Flac),
        FileType::Mpeg => Ok(AudioFormatExt::Mp3),
        FileType::Mp4 => {
            if ext_clean == "alac" {
                Ok(AudioFormatExt::Alac)
            } else {
                Ok(AudioFormatExt::M4a)
            }
        }
        FileType::Aac => {
            if ext_clean == "aac" {
                Ok(AudioFormatExt::Aac)
            } else {
                Ok(AudioFormatExt::M4a)
            }
        }
        FileType::Vorbis => Ok(AudioFormatExt::Ogg),
        FileType::Opus => Ok(AudioFormatExt::Opus),
        FileType::Wav => Ok(AudioFormatExt::Wav),
        FileType::Aiff => Ok(AudioFormatExt::Aiff),
        FileType::Ape => Ok(AudioFormatExt::Ape),
        FileType::WavPack => Ok(AudioFormatExt::Wv),
        _ => AudioFormatExt::from_extension(&ext_clean).ok_or_else(|| {
            FormatError::UnsupportedFormat(format!("unsupported audio format: '{ext_hint}'"))
        }),
    }
}

pub(crate) fn collect_ordered_tags(tagged: &TaggedFile) -> Vec<&Tag> {
    let mut ordered: Vec<&Tag> = Vec::new();
    if matches!(tagged.file_type(), FileType::Wav | FileType::Aiff)
        && let Some(id3) = tagged.tag(TagType::Id3v2)
    {
        ordered.push(id3);
    }
    if let Some(primary) = tagged.primary_tag()
        && !ordered
            .iter()
            .any(|existing| std::ptr::eq(*existing, primary))
    {
        ordered.push(primary);
    }
    for t in tagged.tags() {
        if !ordered.iter().any(|existing| std::ptr::eq(*existing, t)) {
            ordered.push(t);
        }
    }
    ordered
}

fn extract_audio_metadata(
    tagged: &TaggedFile,
    format: AudioFormatExt,
    fallback_title: &str,
    sylt_lrc: Option<String>,
) -> ParsedAudioMeta {
    let props = tagged.properties();
    let duration_secs = props.duration().as_secs() as u32;
    let bit_rate_kbps = props
        .audio_bitrate()
        .or_else(|| props.overall_bitrate())
        .filter(|&b| b > 0);
    let sample_rate_hz = props.sample_rate().filter(|&s| s > 0);
    let bit_depth = props.bit_depth().filter(|&d| d > 0).map(u16::from);
    let channels = props.channels().filter(|&c| c > 0);

    let ordered_tags = collect_ordered_tags(tagged);
    let get_str = |key: ItemKey| -> Option<String> {
        for tag in &ordered_tags {
            if let Some(val) = tag.get_string(key) {
                let trimmed = val.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
        }
        None
    };

    let title = get_str(ItemKey::TrackTitle).unwrap_or_else(|| {
        let trimmed = fallback_title.trim();
        if trimmed.is_empty() {
            "Untitled".to_string()
        } else {
            trimmed.to_string()
        }
    });

    let is_compilation = get_str(ItemKey::FlagCompilation)
        .as_deref()
        .is_some_and(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"));

    let artist = get_str(ItemKey::TrackArtist);
    let album_artist = get_str(ItemKey::AlbumArtist);
    let track_artist = artist
        .clone()
        .or_else(|| album_artist.clone())
        .unwrap_or_else(|| "Unknown Artist".to_string());
    let effective_album_artist = album_artist.clone().unwrap_or_else(|| {
        if is_compilation {
            "Various Artists".to_string()
        } else {
            track_artist.clone()
        }
    });

    let album = get_str(ItemKey::AlbumTitle);
    let album_title = album.clone().unwrap_or_else(|| "Unknown Album".to_string());

    let track_number = ordered_tags
        .iter()
        .find_map(|t| t.track())
        .or_else(|| get_str(ItemKey::TrackNumber).and_then(|s| parse_leading_u32(&s)))
        .filter(|&n| n > 0);
    let total_tracks = ordered_tags
        .iter()
        .find_map(|t| t.track_total())
        .or_else(|| get_str(ItemKey::TrackTotal).and_then(|s| s.trim().parse::<u32>().ok()))
        .filter(|&n| n > 0);

    let disc_number = ordered_tags
        .iter()
        .find_map(|t| t.disk())
        .or_else(|| get_str(ItemKey::DiscNumber).and_then(|s| parse_leading_u32(&s)))
        .filter(|&n| n > 0);
    let total_discs = ordered_tags
        .iter()
        .find_map(|t| t.disk_total())
        .or_else(|| get_str(ItemKey::DiscTotal).and_then(|s| s.trim().parse::<u32>().ok()))
        .filter(|&n| n > 0);

    let year = ordered_tags
        .iter()
        .find_map(|t| t.date().map(|d| i32::from(d.year)))
        .or_else(|| get_str(ItemKey::Year).and_then(|s| parse_year(&s)))
        .or_else(|| get_str(ItemKey::RecordingDate).and_then(|s| parse_year(&s)));

    let genre = get_str(ItemKey::Genre);
    let comment = ordered_tags
        .iter()
        .find_map(|t| t.comment().map(|c| c.into_owned()))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let replay_gain = ReplayGainInfo {
        track_gain_db: get_str(ItemKey::ReplayGainTrackGain).and_then(|s| parse_gain_float(&s)),
        track_peak: get_str(ItemKey::ReplayGainTrackPeak).and_then(|s| parse_gain_float(&s)),
        album_gain_db: get_str(ItemKey::ReplayGainAlbumGain).and_then(|s| parse_gain_float(&s)),
        album_peak: get_str(ItemKey::ReplayGainAlbumPeak).and_then(|s| parse_gain_float(&s)),
    };

    let text_lyrics = get_str(ItemKey::Lyrics).or_else(|| get_str(ItemKey::UnsyncLyrics));
    let has_sylt = sylt_lrc.is_some();
    let embedded_lyrics = text_lyrics.or_else(|| sylt_lrc.filter(|s| !s.trim().is_empty()));
    let has_embedded_lyrics = has_sylt || embedded_lyrics.is_some();
    let cover = extract_best_cover(&ordered_tags);

    ParsedAudioMeta {
        format,
        title,
        artist,
        track_artist,
        album_artist,
        effective_album_artist,
        album,
        album_title,
        track_number,
        total_tracks,
        disc_number,
        total_discs,
        year,
        genre,
        comment,
        is_compilation,
        duration_secs,
        bit_rate_kbps,
        sample_rate_hz,
        bit_depth,
        channels,
        replay_gain,
        has_embedded_lyrics,
        embedded_lyrics,
        cover,
    }
}

pub(crate) fn extract_best_cover(tags: &[&Tag]) -> Option<BinaryAsset> {
    let mut fallback_pic = None;
    for tag in tags {
        for pic in tag.pictures() {
            if pic.data().is_empty() {
                continue;
            }
            if pic.pic_type() == PictureType::CoverFront {
                let mime = pic
                    .mime_type()
                    .map(|m| m.as_str().to_string())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| sniff_image_mime(pic.data()).to_string());
                return Some(BinaryAsset::new(pic.data().to_vec(), mime));
            }
            if fallback_pic.is_none() {
                fallback_pic = Some(pic);
            }
        }
    }
    let pic = fallback_pic?;
    let mime = pic
        .mime_type()
        .map(|m| m.as_str().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| sniff_image_mime(pic.data()).to_string());
    Some(BinaryAsset::new(pic.data().to_vec(), mime))
}

fn parse_leading_u32(s: &str) -> Option<u32> {
    let head = s.split('/').next()?.trim();
    head.parse::<u32>().ok()
}

fn parse_year(s: &str) -> Option<i32> {
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.len() >= 4 {
        digits[..4].parse::<i32>().ok()
    } else {
        None
    }
}

fn parse_gain_float(s: &str) -> Option<f32> {
    let cleaned = s
        .trim()
        .trim_end_matches("dB")
        .trim_end_matches("db")
        .trim_end_matches("DB")
        .trim()
        .trim_start_matches('+');
    cleaned.parse::<f32>().ok().filter(|v| v.is_finite())
}

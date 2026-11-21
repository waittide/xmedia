use std::fs::File;
use std::io::{BufReader, Cursor, Read, Seek};

use lofty::config::{ParseOptions, ParsingMode};
use lofty::file::{AudioFile, FileType, TaggedFile, TaggedFileExt};
use lofty::id3::v2::Frame;
use lofty::mp4::{Mp4Codec, Mp4File};
use lofty::mpeg::MpegFile;
use lofty::picture::PictureType;
use lofty::probe::Probe;
use lofty::tag::{Accessor, ItemKey, Tag};
use xmedia_core::application::ports::BinaryPayload;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::infra::thumbnail::sniff_image_mime;

use crate::application::ports::{AudioMetadataProbePort, ProbedAudioMetadata};
use crate::domain::catalog::{AudioFormat, ReplayGain};

#[derive(Default)]
pub struct LoftyAudioProbe;

impl AudioMetadataProbePort for LoftyAudioProbe {
    fn probe_audio_file(
        &self,
        path: &str,
        ext_hint: &str,
        fallback_title: &str,
    ) -> DomainResult<ProbedAudioMetadata> {
        let file = File::open(path)
            .map_err(|e| DomainError::Storage(format!("failed to open {path}: {e}")))?;
        let mut reader = BufReader::new(file);
        probe_from_reader(&mut reader, ext_hint, fallback_title)
    }

    fn probe_audio_bytes(
        &self,
        bytes: &[u8],
        ext_hint: &str,
        fallback_title: &str,
    ) -> DomainResult<ProbedAudioMetadata> {
        let mut cursor = Cursor::new(bytes);
        probe_from_reader(&mut cursor, ext_hint, fallback_title)
    }
}

fn probe_from_reader<R: Read + Seek>(
    reader: &mut R,
    ext_hint: &str,
    fallback_title: &str,
) -> DomainResult<ProbedAudioMetadata> {
    let opts = ParseOptions::new().parsing_mode(ParsingMode::Relaxed);

    let mut probe = Probe::new(&mut *reader).options(opts);
    if let Some(ft) = FileType::from_ext(ext_hint) {
        probe = probe.set_file_type(ft);
    }
    if let Ok(guessed) = probe.guess_file_type() {
        probe = guessed;
    } else {
        return Err(DomainError::UnsupportedFormat(
            "failed to detect audio format".into(),
        ));
    }

    let tagged = probe
        .read()
        .map_err(|e| DomainError::Validation(format!("failed to parse audio tags: {e}")))?;

    let mut format = map_file_type(tagged.file_type(), ext_hint)?;
    let mut has_sylt = false;

    if tagged.file_type() == FileType::Mp4 {
        if reader.rewind().is_ok()
            && let Ok(mp4) = Mp4File::read_from(&mut *reader, opts)
            && mp4.properties().codec() == Some(Mp4Codec::ALAC)
        {
            format = AudioFormat::Alac;
        }
    } else if tagged.file_type() == FileType::Mpeg
        && reader.rewind().is_ok()
        && let Ok(mp3) = MpegFile::read_from(&mut *reader, opts)
        && let Some(id3v2) = mp3.id3v2()
    {
        has_sylt = id3v2
            .into_iter()
            .any(|f| f.id_str() == "SYLT" && matches!(f, Frame::Binary(_)));
    }

    Ok(extract_metadata(&tagged, format, fallback_title, has_sylt))
}

fn map_file_type(ft: FileType, ext_hint: &str) -> DomainResult<AudioFormat> {
    match ft {
        FileType::Flac => Ok(AudioFormat::Flac),
        FileType::Mpeg => Ok(AudioFormat::Mp3),
        FileType::Mp4 | FileType::Aac => Ok(AudioFormat::M4a),
        FileType::Vorbis => Ok(AudioFormat::Ogg),
        FileType::Opus => Ok(AudioFormat::Opus),
        FileType::Wav => Ok(AudioFormat::Wav),
        _ => AudioFormat::parse(ext_hint),
    }
}

fn extract_metadata(
    tagged: &TaggedFile,
    format: AudioFormat,
    fallback_title: &str,
    has_sylt: bool,
) -> ProbedAudioMetadata {
    let props = tagged.properties();
    let duration_secs = props.duration().as_secs() as u32;
    let bit_rate_kbps = props
        .audio_bitrate()
        .or_else(|| props.overall_bitrate())
        .filter(|&b| b > 0);
    let sample_rate_hz = props.sample_rate().filter(|&s| s > 0);
    let bit_depth = props.bit_depth().filter(|&d| d > 0).map(u16::from);
    let channels = props.channels().filter(|&c| c > 0);

    let mut ordered_tags: Vec<&Tag> = Vec::new();
    if let Some(primary) = tagged.primary_tag() {
        ordered_tags.push(primary);
    }
    for t in tagged.tags() {
        if !ordered_tags
            .iter()
            .any(|existing| std::ptr::eq(*existing, t))
        {
            ordered_tags.push(t);
        }
    }

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

    let raw_compilation = get_str(ItemKey::FlagCompilation);
    let is_compilation = raw_compilation
        .as_deref()
        .is_some_and(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"));

    let raw_track_artist = get_str(ItemKey::TrackArtist);
    let raw_album_artist = get_str(ItemKey::AlbumArtist);

    let track_artist = raw_track_artist
        .clone()
        .or_else(|| raw_album_artist.clone())
        .unwrap_or_else(|| "Unknown Artist".to_string());

    let album_artist = raw_album_artist.unwrap_or_else(|| {
        if is_compilation {
            "Various Artists".to_string()
        } else {
            track_artist.clone()
        }
    });

    let album_title = get_str(ItemKey::AlbumTitle).unwrap_or_else(|| "Unknown Album".to_string());

    let track_number = ordered_tags
        .iter()
        .find_map(|t| t.track())
        .or_else(|| get_str(ItemKey::TrackNumber).and_then(|s| parse_leading_u32(&s)))
        .filter(|&n| n > 0);

    let disc_number = ordered_tags
        .iter()
        .find_map(|t| t.disk())
        .or_else(|| get_str(ItemKey::DiscNumber).and_then(|s| parse_leading_u32(&s)))
        .filter(|&n| n > 0);

    let year = ordered_tags
        .iter()
        .find_map(|t| t.date().map(|d| i32::from(d.year)))
        .or_else(|| get_str(ItemKey::Year).and_then(|s| parse_year(&s)))
        .or_else(|| get_str(ItemKey::RecordingDate).and_then(|s| parse_year(&s)));

    let genre = get_str(ItemKey::Genre);

    let replay_gain = ReplayGain {
        track_gain_db: get_str(ItemKey::ReplayGainTrackGain).and_then(|s| parse_gain_float(&s)),
        track_peak: get_str(ItemKey::ReplayGainTrackPeak).and_then(|s| parse_gain_float(&s)),
        album_gain_db: get_str(ItemKey::ReplayGainAlbumGain).and_then(|s| parse_gain_float(&s)),
        album_peak: get_str(ItemKey::ReplayGainAlbumPeak).and_then(|s| parse_gain_float(&s)),
    };

    let has_embedded_lyrics =
        has_sylt || get_str(ItemKey::Lyrics).is_some() || get_str(ItemKey::UnsyncLyrics).is_some();

    let cover = extract_best_cover(&ordered_tags);

    ProbedAudioMetadata {
        format,
        title,
        track_artist,
        album_artist,
        album_title,
        track_number,
        disc_number,
        year,
        genre,
        is_compilation,
        duration_secs,
        bit_rate_kbps,
        sample_rate_hz,
        bit_depth,
        channels,
        replay_gain,
        has_embedded_lyrics,
        cover,
    }
}

fn extract_best_cover(tags: &[&Tag]) -> Option<BinaryPayload> {
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
                return Some(BinaryPayload {
                    bytes: pic.data().to_vec(),
                    mime_type: mime,
                });
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
    Some(BinaryPayload {
        bytes: pic.data().to_vec(),
        mime_type: mime,
    })
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
    cleaned.parse::<f32>().ok()
}

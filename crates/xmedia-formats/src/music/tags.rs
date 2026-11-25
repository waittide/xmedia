use std::path::{Path, PathBuf};

use lofty::config::WriteOptions;
use lofty::file::{AudioFile, FileType, TaggedFile, TaggedFileExt};
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::tag::items::Timestamp;
use lofty::tag::{Accessor, ItemKey, ItemValue, Tag, TagItem, TagType};

use super::{
    AudioMetaPatch, ReplayGainInfo, collect_ordered_tags, extract_best_cover, open_tagged_path,
};
use crate::common::{BinaryAsset, decode_text_smart, sniff_image_mime};
use crate::error::{FormatError, FormatResult};

const GENERIC_MUSIC_FOLDER_NAMES: &[&str] = &[
    "music",
    "musics",
    "audio",
    "songs",
    "tracks",
    "sample",
    "samples",
    "download",
    "downloads",
    "inbox",
    "library",
    "media",
    "音乐",
    "歌曲",
];

/// Returns `true` when `name` is a generic root/downloads folder rather than a dedicated album directory.
pub fn is_generic_music_folder(name: &str) -> bool {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return true;
    }
    let lower = trimmed.to_ascii_lowercase();
    GENERIC_MUSIC_FOLDER_NAMES
        .iter()
        .any(|g| lower == *g || trimmed == *g)
}

/// Finds a sidecar album cover (`cover.*`, `folder.*`, `front.*`) in `audio_path`'s directory
/// when that directory is not a generic music/downloads folder.
pub fn find_album_cover_path(audio_path: &Path) -> Option<PathBuf> {
    let parent = audio_path.parent()?;
    let dir_name = parent.file_name()?.to_str()?;
    if is_generic_music_folder(dir_name) {
        return None;
    }
    const CANDIDATES: &[&str] = &[
        "cover.jpg",
        "cover.jpeg",
        "cover.png",
        "cover.webp",
        "folder.jpg",
        "folder.jpeg",
        "folder.png",
        "folder.webp",
        "front.jpg",
        "front.jpeg",
        "front.png",
        "front.webp",
    ];
    CANDIDATES
        .iter()
        .map(|name| parent.join(name))
        .find(|p| p.is_file())
}

/// Computes the canonical sidecar `.lrc` path for `audio_path`.
pub fn sidecar_lrc_path(audio_path: &Path) -> PathBuf {
    audio_path.with_extension("lrc")
}

/// Reads and smart-decodes (UTF-8 / UTF-16 / GB18030) a sidecar `.lrc` file if non-empty.
pub fn read_sidecar_lrc(audio_path: &Path) -> Option<String> {
    let lrc_path = sidecar_lrc_path(audio_path);
    let raw = std::fs::read(&lrc_path).ok()?;
    let (decoded, _) = decode_text_smart(&raw);
    (!decoded.trim().is_empty()).then_some(decoded)
}

/// Returns a mutable reference to the preferred primary tag for `tagged`,
/// selecting `TagType::Id3v2` on WAV/AIFF containers so Unicode tags, `USLT` lyrics, and artwork persist cleanly.
pub fn ensure_mutable_primary_tag(tagged: &mut TaggedFile) -> &mut Tag {
    let preferred_type = if matches!(tagged.file_type(), FileType::Wav | FileType::Aiff) {
        TagType::Id3v2
    } else {
        tagged.primary_tag_type()
    };
    if tagged.tag(preferred_type).is_none() {
        tagged.insert_tag(Tag::new(preferred_type));
    }
    tagged
        .tag_mut(preferred_type)
        .expect("preferred audio tag just inserted")
}

/// Applies `patch` directly onto `path` (which may be a staging file in an atomic write workflow).
/// Properly writes `ItemKey::UnsyncLyrics` (`USLT`) on ID3v2 containers (MP3/WAV/AIFF) and `ItemKey::Lyrics`
/// on Vorbis/MP4/APE containers.
pub fn apply_audio_patch_in_place(path: &Path, patch: &AudioMetaPatch) -> FormatResult<()> {
    let mut tagged = open_tagged_path(path)?;
    let tag = ensure_mutable_primary_tag(&mut tagged);

    if let Some(ref v) = patch.title {
        let clean = v.trim();
        if clean.is_empty() {
            tag.remove_title();
        } else {
            tag.set_title(clean.to_string());
        }
    }
    if let Some(ref v) = patch.artist {
        let clean = v.trim();
        if clean.is_empty() {
            tag.remove_artist();
        } else {
            tag.set_artist(clean.to_string());
        }
    }
    if let Some(ref v) = patch.album_artist {
        let clean = v.trim();
        if clean.is_empty() {
            tag.remove_key(ItemKey::AlbumArtist);
        } else {
            tag.insert_text(ItemKey::AlbumArtist, clean.to_string());
        }
    }
    if let Some(ref v) = patch.album {
        let clean = v.trim();
        if clean.is_empty() {
            tag.remove_album();
        } else {
            tag.set_album(clean.to_string());
        }
    }
    if let Some(n) = patch.track_number {
        if n == 0 {
            tag.remove_track();
        } else {
            tag.set_track(n);
        }
    }
    if let Some(n) = patch.disc_number {
        if n == 0 {
            tag.remove_disk();
        } else {
            tag.set_disk(n);
        }
    }
    if let Some(y) = patch.year {
        tag.remove_date();
        tag.remove_key(ItemKey::Year);
        if y > 0 {
            if let Ok(year_u16) = u16::try_from(y) {
                tag.set_date(Timestamp {
                    year: year_u16,
                    ..Default::default()
                });
            }
            if matches!(tag.tag_type(), TagType::Id3v1 | TagType::Ape) {
                tag.insert_text(ItemKey::Year, y.to_string());
            }
        }
    }
    if let Some(ref g) = patch.genre {
        let clean = g.trim();
        if clean.is_empty() {
            tag.remove_genre();
        } else {
            tag.set_genre(clean.to_string());
        }
    }
    if let Some(ref c) = patch.comment {
        let clean = c.trim();
        if clean.is_empty() {
            tag.remove_comment();
        } else {
            tag.set_comment(clean.to_string());
        }
    }
    if let Some(comp) = patch.is_compilation {
        if comp {
            tag.insert_text(ItemKey::FlagCompilation, "1".to_string());
        } else {
            tag.remove_key(ItemKey::FlagCompilation);
        }
    }
    if let Some(ref lyr) = patch.embedded_lyrics {
        tag.remove_key(ItemKey::Lyrics);
        tag.remove_key(ItemKey::UnsyncLyrics);
        if !lyr.trim().is_empty() {
            // ID3v2 (MP3/WAV/AIFF) maps USLT exclusively to `ItemKey::UnsyncLyrics`
            let lyrics_key = if tag.tag_type() == TagType::Id3v2 {
                ItemKey::UnsyncLyrics
            } else {
                ItemKey::Lyrics
            };
            tag.insert_text(lyrics_key, lyr.clone());
        }
    }
    if let Some(ref rg) = patch.replay_gain {
        apply_replaygain_to_tag(tag, rg);
    }

    tagged
        .save_to_path(path, WriteOptions::default())
        .map_err(|e| FormatError::Io(format!("failed encoding audio tags via lofty: {e}")))
}

fn apply_replaygain_to_tag(tag: &mut Tag, rg: &ReplayGainInfo) {
    if let Some(tg) = rg.track_gain_db {
        tag.insert(TagItem::new(
            ItemKey::ReplayGainTrackGain,
            ItemValue::Text(format!("{tg:+.2} dB")),
        ));
    }
    if let Some(tp) = rg.track_peak {
        tag.insert(TagItem::new(
            ItemKey::ReplayGainTrackPeak,
            ItemValue::Text(format!("{tp:.6}")),
        ));
    }
    if let Some(ag) = rg.album_gain_db {
        tag.insert(TagItem::new(
            ItemKey::ReplayGainAlbumGain,
            ItemValue::Text(format!("{ag:+.2} dB")),
        ));
    }
    if let Some(ap) = rg.album_peak {
        tag.insert(TagItem::new(
            ItemKey::ReplayGainAlbumPeak,
            ItemValue::Text(format!("{ap:.6}")),
        ));
    }
}

/// Writes ReplayGain 2.0 tags in-place onto `path`.
pub fn write_replaygain_in_place(
    path: &Path,
    track_gain_db: f64,
    track_peak: f64,
    album_gain_db: Option<f64>,
    album_peak: Option<f64>,
) -> FormatResult<()> {
    let mut tagged = open_tagged_path(path)?;
    let tag = ensure_mutable_primary_tag(&mut tagged);
    let rg = ReplayGainInfo {
        track_gain_db: Some(track_gain_db as f32),
        track_peak: Some(track_peak as f32),
        album_gain_db: album_gain_db.map(|v| v as f32),
        album_peak: album_peak.map(|v| v as f32),
    };
    apply_replaygain_to_tag(tag, &rg);
    tagged
        .save_to_path(path, WriteOptions::default())
        .map_err(|e| FormatError::Io(format!("failed saving ReplayGain tags: {e}")))
}

/// Reads the best embedded cover (`PictureType::CoverFront` preferred, first picture fallback) from `path`.
pub fn read_embedded_cover_from_path(path: &Path) -> FormatResult<Option<BinaryAsset>> {
    let tagged = open_tagged_path(path)?;
    let ordered = collect_ordered_tags(&tagged);
    Ok(extract_best_cover(&ordered))
}

/// Writes `CoverFront` artwork in-place onto `path` while preserving any non-`CoverFront` pictures
/// (such as `CoverBack`, `Artist`, or `LeafletPage`).
pub fn write_embedded_cover_in_place(
    path: &Path,
    bytes: &[u8],
    mime_hint: &str,
) -> FormatResult<()> {
    let effective_mime = if mime_hint.trim().is_empty() {
        sniff_image_mime(bytes).to_string()
    } else {
        mime_hint.trim().to_string()
    };
    let mut tagged = open_tagged_path(path)?;
    let tag = ensure_mutable_primary_tag(&mut tagged);
    let pic = Picture::unchecked(bytes.to_vec())
        .pic_type(PictureType::CoverFront)
        .mime_type(MimeType::from_str(&effective_mime))
        .build();

    let preserved: Vec<Picture> = tag
        .pictures()
        .iter()
        .filter(|existing| existing.pic_type() != PictureType::CoverFront)
        .cloned()
        .collect();
    while !tag.pictures().is_empty() {
        tag.remove_picture(0);
    }
    tag.push_picture(pic);
    for other in preserved {
        tag.push_picture(other);
    }
    tagged
        .save_to_path(path, WriteOptions::default())
        .map_err(|e| FormatError::Io(format!("failed saving embedded audio cover: {e}")))
}

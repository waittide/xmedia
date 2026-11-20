use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use lofty::config::{ParseOptions, ParsingMode, WriteOptions};
use lofty::file::{AudioFile, FileType, TaggedFile, TaggedFileExt};
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::probe::Probe;
use lofty::tag::items::Timestamp;
use lofty::tag::{Accessor, ItemKey, Tag, TagType};
use xmedia_core::application::ports::BinaryPayload;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;
use xmedia_core::infra::stream::decode_text_smart;
use xmedia_core::infra::thumbnail::sniff_image_mime;

use crate::domain::media::{
    CompletenessFlags, MediaMetadataPatch, MediaMetadataPayload, MusicFileMeta, MusicMetaPatch,
};
use crate::domain::ports::{MediaInspectorPort, QuickFileSummary};
use crate::infra::atomic_fs::{atomic_mutate_copy, atomic_write_bytes};
use crate::infra::inspectors::video::write_image_replacing_opposite_ext;

pub struct LoftyMusicInspector;

const MUSIC_EXTENSIONS: &[&str] = &["mp3", "flac", "m4a", "aac", "alac", "ogg", "opus", "wav"];

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

pub(crate) fn is_generic_music_folder(name: &str) -> bool {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return true;
    }
    let lower = trimmed.to_ascii_lowercase();
    GENERIC_MUSIC_FOLDER_NAMES
        .iter()
        .any(|g| lower == *g || trimmed == *g)
}

fn find_album_cover_path(audio_path: &Path) -> Option<PathBuf> {
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

pub(crate) fn sidecar_lrc_path(audio_path: &Path) -> PathBuf {
    audio_path.with_extension("lrc")
}

pub(crate) fn read_sidecar_lrc(audio_path: &Path) -> Option<String> {
    let lrc_path = sidecar_lrc_path(audio_path);
    let raw = std::fs::read(&lrc_path).ok()?;
    let (decoded, _) = decode_text_smart(&raw);
    let trimmed = decoded.trim();
    (!trimmed.is_empty()).then_some(decoded)
}

pub(crate) fn open_tagged(path: &Path) -> DomainResult<TaggedFile> {
    let file = File::open(path)
        .map_err(|e| DomainError::Storage(format!("failed opening '{}': {e}", path.display())))?;
    let reader = BufReader::new(file);
    let opts = ParseOptions::new().parsing_mode(ParsingMode::Relaxed);
    let mut probe = Probe::new(reader).options(opts);
    if let Some(ext) = path.extension().and_then(|s| s.to_str())
        && let Some(ft) = FileType::from_ext(ext)
    {
        probe = probe.set_file_type(ft);
    }
    let probe = probe.guess_file_type().map_err(|e| {
        DomainError::UnsupportedFormat(format!("failed detecting audio format: {e}"))
    })?;
    probe
        .read()
        .map_err(|e| DomainError::Validation(format!("failed reading audio metadata: {e}")))
}

fn parse_music_meta(path: &Path) -> DomainResult<MusicFileMeta> {
    let fallback_stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Untitled");
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("mp3")
        .to_ascii_lowercase();
    let tagged = open_tagged(path)?;
    let props = tagged.properties();

    let tag_ref = if tagged.file_type() == FileType::Wav {
        tagged
            .tag(TagType::Id3v2)
            .or_else(|| tagged.primary_tag())
            .or_else(|| tagged.first_tag())
    } else {
        tagged.primary_tag().or_else(|| tagged.first_tag())
    };
    let title = tag_ref
        .and_then(|t| t.title().map(|c| c.into_owned()))
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| fallback_stem.to_string());
    let artist = tag_ref
        .and_then(|t| t.artist().map(|c| c.into_owned()))
        .unwrap_or_default();
    let album_artist = tag_ref
        .and_then(|t| t.get_string(ItemKey::AlbumArtist).map(str::to_owned))
        .filter(|s| !s.trim().is_empty());
    let album = tag_ref
        .and_then(|t| t.album().map(|c| c.into_owned()))
        .unwrap_or_default();
    let track_number = tag_ref.and_then(|t| t.track());
    let disc_number = tag_ref.and_then(|t| t.disk());
    let year = tag_ref.and_then(|t| {
        t.date()
            .map(|d| i32::from(d.year))
            .or_else(|| {
                t.get_string(ItemKey::Year)
                    .and_then(|s| s.trim().get(..4))
                    .and_then(|y| y.parse::<i32>().ok())
            })
            .or_else(|| {
                t.get_string(ItemKey::RecordingDate)
                    .and_then(|s| s.trim().get(..4))
                    .and_then(|y| y.parse::<i32>().ok())
            })
    });
    let genre = tag_ref
        .and_then(|t| t.genre().map(|c| c.into_owned()))
        .filter(|s| !s.trim().is_empty());
    let comment = tag_ref
        .and_then(|t| t.comment().map(|c| c.into_owned()))
        .filter(|s| !s.trim().is_empty());
    let embedded_lyrics = tagged
        .tags()
        .iter()
        .find_map(|t| {
            t.get_string(ItemKey::Lyrics)
                .or_else(|| t.get_string(ItemKey::UnsyncLyrics))
                .map(str::to_owned)
        })
        .filter(|s| !s.trim().is_empty());
    let has_embedded_cover = tagged.tags().iter().any(|t| !t.pictures().is_empty());
    let has_directory_cover = find_album_cover_path(path).is_some();

    let lrc_path = sidecar_lrc_path(path);
    let sidecar_lrc_filename = lrc_path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let sidecar_lrc = read_sidecar_lrc(path);

    Ok(MusicFileMeta {
        format: ext,
        title,
        artist,
        album_artist,
        album,
        track_number,
        disc_number,
        year,
        genre,
        comment,
        duration_secs: props.duration().as_secs() as u32,
        bit_rate_kbps: props
            .audio_bitrate()
            .or_else(|| props.overall_bitrate())
            .filter(|&b| b > 0),
        sample_rate_hz: props.sample_rate().filter(|&s| s > 0),
        bit_depth: props.bit_depth().filter(|&d| d > 0).map(u16::from),
        channels: props.channels().filter(|&c| c > 0),
        has_embedded_cover,
        has_directory_cover,
        embedded_lyrics,
        sidecar_lrc,
        sidecar_lrc_filename,
    })
}

pub(crate) fn ensure_mutable_primary_tag(tagged: &mut TaggedFile) -> &mut Tag {
    let preferred_type = if tagged.file_type() == FileType::Wav {
        TagType::Id3v2
    } else {
        tagged.primary_tag_type()
    };
    if tagged.tag(preferred_type).is_none() {
        tagged.insert_tag(Tag::new(preferred_type));
    }
    tagged
        .tag_mut(preferred_type)
        .expect("preferred tag just inserted")
}

fn apply_music_patch_to_file(path: &Path, patch: &MusicMetaPatch) -> DomainResult<()> {
    let touches_embedded = patch.title.is_some()
        || patch.artist.is_some()
        || patch.album_artist.is_some()
        || patch.album.is_some()
        || patch.track_number.is_some()
        || patch.disc_number.is_some()
        || patch.year.is_some()
        || patch.genre.is_some()
        || patch.comment.is_some()
        || patch.embedded_lyrics.is_some();

    if touches_embedded {
        atomic_mutate_copy(path, |staging| {
            let mut tagged = open_tagged(staging)?;
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
            if let Some(ref lyr) = patch.embedded_lyrics {
                let clean = lyr.trim();
                tag.remove_key(ItemKey::Lyrics);
                tag.remove_key(ItemKey::UnsyncLyrics);
                if !clean.is_empty() {
                    // ID3v2 (MP3/WAV) maps USLT exclusively to `ItemKey::UnsyncLyrics`
                    let lyrics_key = if tag.tag_type() == TagType::Id3v2 {
                        ItemKey::UnsyncLyrics
                    } else {
                        ItemKey::Lyrics
                    };
                    tag.insert_text(lyrics_key, lyr.clone());
                }
            }

            tagged
                .save_to_path(staging, WriteOptions::default())
                .map_err(|e| {
                    DomainError::Storage(format!("failed encoding audio tags via lofty: {e}"))
                })
        })?;
    }

    if let Some(ref sidecar) = patch.sidecar_lrc {
        let lrc_path = sidecar_lrc_path(path);
        if sidecar.trim().is_empty() {
            if lrc_path.exists() {
                let _ = std::fs::remove_file(&lrc_path);
            }
        } else {
            atomic_write_bytes(&lrc_path, sidecar.as_bytes())?;
        }
    }

    Ok(())
}

#[async_trait]
impl MediaInspectorPort for LoftyMusicInspector {
    fn media_kind(&self) -> MediaKind {
        MediaKind::Music
    }

    fn supports_extension(&self, ext_lower: &str) -> bool {
        MUSIC_EXTENSIONS.contains(&ext_lower)
    }

    async fn summarize_file(&self, path: &Path, fallback_stem: &str) -> QuickFileSummary {
        let p = path.to_path_buf();
        let stem = fallback_stem.to_string();
        tokio::task::spawn_blocking(move || match parse_music_meta(&p) {
            Ok(m) => {
                let sub = if !m.artist.is_empty() && !m.album.is_empty() {
                    format!("{} · {}", m.artist, m.album)
                } else if !m.artist.is_empty() {
                    m.artist.clone()
                } else {
                    m.album.clone()
                };
                QuickFileSummary {
                    display_title: m.title.clone(),
                    display_subtitle: sub,
                    year: m.year,
                    completeness: CompletenessFlags {
                        has_title: !m.title.trim().is_empty(),
                        has_creator: !m.artist.trim().is_empty(),
                        has_cover: m.has_embedded_cover || m.has_directory_cover,
                        has_summary_or_lyrics: m.sidecar_lrc.is_some()
                            || m.embedded_lyrics.is_some(),
                        has_nfo_or_tag_block: !m.album.trim().is_empty(),
                        is_normalized: false,
                    },
                }
            }
            Err(_) => QuickFileSummary {
                display_title: stem,
                display_subtitle: String::new(),
                year: None,
                completeness: CompletenessFlags::default(),
            },
        })
        .await
        .unwrap_or_else(|_| QuickFileSummary {
            display_title: fallback_stem.to_string(),
            display_subtitle: String::new(),
            year: None,
            completeness: CompletenessFlags::default(),
        })
    }

    async fn inspect_file(&self, path: &Path) -> DomainResult<MediaMetadataPayload> {
        let p = path.to_path_buf();
        let meta = tokio::task::spawn_blocking(move || parse_music_meta(&p))
            .await
            .map_err(|e| DomainError::Storage(format!("music inspect task failed: {e}")))??;
        Ok(MediaMetadataPayload::Music(meta))
    }

    async fn write_metadata(
        &self,
        path: &Path,
        patch: &MediaMetadataPatch,
    ) -> DomainResult<MediaMetadataPayload> {
        let MediaMetadataPatch::Music(mp) = patch else {
            return Err(DomainError::Validation(
                "expected music metadata patch for audio file".into(),
            ));
        };
        let p = path.to_path_buf();
        let mp_cloned = mp.clone();
        let updated = tokio::task::spawn_blocking(move || {
            apply_music_patch_to_file(&p, &mp_cloned)?;
            parse_music_meta(&p)
        })
        .await
        .map_err(|e| DomainError::Storage(format!("music write task failed: {e}")))??;
        Ok(MediaMetadataPayload::Music(updated))
    }

    async fn read_artwork(
        &self,
        path: &Path,
        slot: Option<&str>,
    ) -> DomainResult<Option<BinaryPayload>> {
        let p = path.to_path_buf();
        let slot_owned = slot.map(str::to_ascii_lowercase);
        tokio::task::spawn_blocking(move || {
            if slot_owned.as_deref() != Some("album_cover") {
                let tagged = open_tagged(&p)?;
                for tag in tagged.tags() {
                    if let Some(pic) = tag
                        .pictures()
                        .iter()
                        .find(|pic| pic.pic_type() == PictureType::CoverFront)
                        .or_else(|| tag.pictures().first())
                    {
                        let data = pic.data().to_vec();
                        let mime = pic
                            .mime_type()
                            .map(|m| m.as_str().to_string())
                            .unwrap_or_else(|| sniff_image_mime(&data).to_string());
                        return Ok(Some(BinaryPayload {
                            bytes: data,
                            mime_type: mime,
                        }));
                    }
                }
            }
            if let Some(cov_path) = find_album_cover_path(&p)
                && let Ok(bytes) = std::fs::read(&cov_path)
                && !bytes.is_empty()
            {
                let mime = sniff_image_mime(&bytes).to_string();
                return Ok(Some(BinaryPayload {
                    bytes,
                    mime_type: mime,
                }));
            }
            Ok(None)
        })
        .await
        .map_err(|e| DomainError::Storage(format!("artwork read task failed: {e}")))?
    }

    async fn write_artwork(
        &self,
        path: &Path,
        bytes: &[u8],
        mime_type: &str,
        slot: &str,
    ) -> DomainResult<()> {
        let p = path.to_path_buf();
        let raw = bytes.to_vec();
        let mime_hint = if mime_type.trim().is_empty() {
            sniff_image_mime(&raw).to_string()
        } else {
            mime_type.trim().to_string()
        };
        if slot.eq_ignore_ascii_case("album_cover") {
            let ext = if mime_hint.contains("png") {
                "png"
            } else if mime_hint.contains("webp") {
                "webp"
            } else {
                "jpg"
            };
            let parent = p
                .parent()
                .ok_or_else(|| {
                    DomainError::Validation("audio file has no parent directory".into())
                })?
                .to_path_buf();
            return tokio::task::spawn_blocking(move || {
                write_image_replacing_opposite_ext(&parent, "cover", ext, &raw)
            })
            .await
            .map_err(|e| DomainError::Storage(format!("album cover write failed: {e}")))?;
        }
        tokio::task::spawn_blocking(move || {
            atomic_mutate_copy(&p, |staging| {
                let mut tagged = open_tagged(staging)?;
                let tag = ensure_mutable_primary_tag(&mut tagged);
                let lofty_mime = MimeType::from_str(&mime_hint);
                let pic = Picture::unchecked(raw)
                    .pic_type(PictureType::CoverFront)
                    .mime_type(lofty_mime)
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
                    .save_to_path(staging, WriteOptions::default())
                    .map_err(|e| DomainError::Storage(format!("failed saving embedded cover: {e}")))
            })
        })
        .await
        .map_err(|e| DomainError::Storage(format!("artwork write task failed: {e}")))?
    }
}

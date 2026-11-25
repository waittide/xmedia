use std::path::Path;

use async_trait::async_trait;
use xmedia_core::application::ports::BinaryPayload;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;
use xmedia_formats::music::{
    AudioFormatExt, AudioMetaPatch, apply_audio_patch_in_place, find_album_cover_path,
    probe_audio_path, read_embedded_cover_from_path, write_embedded_cover_in_place,
};
pub(crate) use xmedia_formats::music::{
    is_generic_music_folder, read_sidecar_lrc, sidecar_lrc_path,
};
use xmedia_formats::sniff_image_mime;

use crate::domain::media::{
    CompletenessFlags, MediaMetadataPatch, MediaMetadataPayload, MusicFileMeta, MusicMetaPatch,
};
use crate::domain::ports::{MediaInspectorPort, QuickFileSummary};
use crate::infra::atomic_fs::{atomic_mutate_copy, atomic_write_bytes};
use crate::infra::inspectors::map_format_err;
use crate::infra::inspectors::video::write_image_replacing_opposite_ext;

pub struct LoftyMusicInspector;

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
    let parsed = probe_audio_path(path, &ext, fallback_stem).map_err(map_format_err)?;

    let lrc_path = sidecar_lrc_path(path);
    let sidecar_lrc_filename = lrc_path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let sidecar_lrc = read_sidecar_lrc(path);

    Ok(MusicFileMeta {
        format: ext,
        title: parsed.title,
        artist: parsed.artist.unwrap_or_default(),
        album_artist: parsed.album_artist,
        album: parsed.album.unwrap_or_default(),
        track_number: parsed.track_number,
        disc_number: parsed.disc_number,
        year: parsed.year,
        genre: parsed.genre,
        comment: parsed.comment,
        duration_secs: parsed.duration_secs,
        bit_rate_kbps: parsed.bit_rate_kbps,
        sample_rate_hz: parsed.sample_rate_hz,
        bit_depth: parsed.bit_depth,
        channels: parsed.channels,
        has_embedded_cover: parsed.cover.is_some(),
        has_directory_cover: find_album_cover_path(path).is_some(),
        embedded_lyrics: parsed.embedded_lyrics,
        sidecar_lrc,
        sidecar_lrc_filename,
    })
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
        let fmt_patch = AudioMetaPatch {
            title: patch.title.clone(),
            artist: patch.artist.clone(),
            album_artist: patch.album_artist.clone(),
            album: patch.album.clone(),
            track_number: patch.track_number,
            disc_number: patch.disc_number,
            year: patch.year,
            genre: patch.genre.clone(),
            comment: patch.comment.clone(),
            is_compilation: None,
            embedded_lyrics: patch.embedded_lyrics.clone(),
            replay_gain: None,
        };
        atomic_mutate_copy(path, |staging| {
            apply_audio_patch_in_place(staging, &fmt_patch).map_err(map_format_err)
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
        AudioFormatExt::from_extension(ext_lower).is_some()
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
                        is_organized: false,
                    },
                    metadata: Some(MediaMetadataPayload::Music(m)),
                }
            }
            Err(_) => QuickFileSummary {
                display_title: stem,
                display_subtitle: String::new(),
                year: None,
                completeness: CompletenessFlags::default(),
                metadata: None,
            },
        })
        .await
        .unwrap_or_else(|_| QuickFileSummary {
            display_title: fallback_stem.to_string(),
            display_subtitle: String::new(),
            year: None,
            completeness: CompletenessFlags::default(),
            metadata: None,
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
            if slot_owned.as_deref() != Some("album_cover")
                && let Some(asset) = read_embedded_cover_from_path(&p).map_err(map_format_err)?
            {
                return Ok(Some(BinaryPayload {
                    bytes: asset.bytes,
                    mime_type: asset.mime_type,
                }));
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
                write_embedded_cover_in_place(staging, &raw, &mime_hint).map_err(map_format_err)
            })
        })
        .await
        .map_err(|e| DomainError::Storage(format!("artwork write task failed: {e}")))?
    }
}

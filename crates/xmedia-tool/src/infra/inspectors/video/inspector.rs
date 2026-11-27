use std::path::Path;

use async_trait::async_trait;
use xmedia_core::application::ports::BinaryPayload;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;
use xmedia_core::infra::ffmpeg::FfmpegToolkit;
use xmedia_core::infra::thumbnail::sniff_image_mime;

use super::artwork::{
    find_clearlogo_path, find_fanart_path, find_poster_path, find_thumb_path,
    write_image_replacing_opposite_ext,
};
use super::filename::{VIDEO_EXTENSIONS, dir_has_single_video, is_season_dir};
use super::nfo::{
    find_existing_nfo, find_show_nfo_for_episode, parse_nfo_into, read_base_video_meta,
    render_season_nfo_xml_preserving, render_tvshow_nfo_xml_preserving,
    render_video_nfo_xml_preserving,
};
use crate::application::ports::{ArtworkIo, MediaInspectorPort, QuickFileSummary};
use crate::domain::media::{
    CompletenessFlags, MediaMetadataPatch, MediaMetadataPayload, VideoFileMeta, VideoMetaPatch,
};
use crate::infra::atomic_fs::atomic_write_bytes;

pub struct VideoNfoInspector {
    ffmpeg: FfmpegToolkit,
}

impl VideoNfoInspector {
    pub fn new(ffmpeg: FfmpegToolkit) -> Self {
        Self { ffmpeg }
    }
}

#[async_trait]
impl MediaInspectorPort for VideoNfoInspector {
    fn media_kind(&self) -> MediaKind {
        MediaKind::Video
    }

    fn supports_path(&self, path: &Path) -> bool {
        !path.is_dir()
            && path
                .extension()
                .and_then(|s| s.to_str())
                .map(|s| s.to_ascii_lowercase())
                .is_some_and(|ext| VIDEO_EXTENSIONS.contains(&ext.as_str()))
    }

    async fn summarize_file(&self, path: &Path, fallback_stem: &str) -> QuickFileSummary {
        let p = path.to_path_buf();
        let stem = fallback_stem.to_string();
        tokio::task::spawn_blocking(move || {
            let meta = read_base_video_meta(&p);
            let sub = if meta.is_episode {
                match (&meta.show_title, meta.season_number, meta.episode_number) {
                    (Some(sh), Some(s), Some(e)) => format!("{sh} · S{s:02}E{e:02}"),
                    (Some(sh), _, _) => sh.clone(),
                    _ => String::new(),
                }
            } else {
                meta.genres.join(" / ")
            };
            QuickFileSummary {
                display_title: if meta.title.is_empty() {
                    stem
                } else {
                    meta.title.clone()
                },
                display_subtitle: sub,
                year: meta.year,
                completeness: CompletenessFlags {
                    has_title: !meta.title.trim().is_empty(),
                    has_creator: !meta.directors.is_empty()
                        || !meta.actors.is_empty()
                        || meta.show_title.is_some(),
                    has_cover: meta.has_poster || meta.has_thumb,
                    has_summary_or_lyrics: meta
                        .summary
                        .as_ref()
                        .is_some_and(|s| !s.trim().is_empty()),
                    has_nfo_or_tag_block: meta.has_nfo,
                    is_normalized: false,
                    is_organized: false,
                },
                metadata: Some(MediaMetadataPayload::Video(meta)),
            }
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
        let mut meta = tokio::task::spawn_blocking(move || read_base_video_meta(&p))
            .await
            .map_err(|e| DomainError::Storage(format!("video inspect task failed: {e}")))?;

        if let Ok(probed) = self.ffmpeg.probe_media(path).await {
            meta.duration_secs = probed.duration_secs;
            meta.bit_rate_kbps = probed.bit_rate_kbps;
            meta.width = probed.width;
            meta.height = probed.height;
            meta.video_codec = probed.video_codec;
            meta.audio_streams = probed.audio_streams;
            meta.subtitle_streams = probed.subtitle_streams;
        }

        Ok(MediaMetadataPayload::Video(meta))
    }

    async fn write_metadata(
        &self,
        path: &Path,
        patch: &MediaMetadataPatch,
    ) -> DomainResult<MediaMetadataPayload> {
        let MediaMetadataPatch::Video(vp) = patch else {
            return Err(DomainError::Validation(
                "expected video metadata patch for video file".into(),
            ));
        };
        let p = path.to_path_buf();
        let vp_cloned: VideoMetaPatch = vp.clone();

        tokio::task::spawn_blocking(move || -> DomainResult<()> {
            let mut meta = read_base_video_meta(&p);
            meta.apply_patch(vp_cloned);

            let existing_source_nfo = find_existing_nfo(&p, meta.is_episode)
                .or_else(|| find_existing_nfo(&p, !meta.is_episode));
            let nfo_target = if !meta.is_episode
                && let Some(parent) = p.parent()
                && parent.join("movie.nfo").is_file()
                && dir_has_single_video(parent)
                && !p.with_extension("nfo").is_file()
            {
                parent.join("movie.nfo")
            } else {
                p.with_extension("nfo")
            };
            let existing_nfo_bytes = std::fs::read(&nfo_target).ok().or_else(|| {
                existing_source_nfo
                    .as_ref()
                    .and_then(|src| std::fs::read(src).ok())
            });
            let xml = render_video_nfo_xml_preserving(&meta, existing_nfo_bytes.as_deref());
            atomic_write_bytes(&nfo_target, xml.as_bytes())?;

            // Keep Jellyfin `season.nfo` and `tvshow.nfo` synchronized when editing an archived episode,
            // preserving existing show/season-level fields rather than clobbering them with single-episode data.
            if meta.is_episode {
                if let Some(parent) = p.parent() {
                    let season_nfo = parent.join("season.nfo");
                    if season_nfo.is_file() {
                        let mut existing_season = VideoFileMeta::default();
                        let existing_season_raw = std::fs::read(&season_nfo).ok();
                        if let Some(ref raw) = existing_season_raw {
                            parse_nfo_into(raw, &mut existing_season);
                        }
                        let s_num = meta.season_number.unwrap_or(1);
                        let s_title = (!existing_season.title.trim().is_empty())
                            .then_some(existing_season.title.as_str());
                        let s_xml = render_season_nfo_xml_preserving(
                            s_num,
                            s_title,
                            existing_season.year.or(meta.year),
                            existing_season.summary.as_deref(),
                            existing_season
                                .tmdb_id
                                .as_deref()
                                .or(meta.tmdb_id.as_deref()),
                            existing_season
                                .bangumi_id
                                .as_deref()
                                .or(meta.bangumi_id.as_deref()),
                            existing_season_raw.as_deref(),
                        );
                        let _ = atomic_write_bytes(&season_nfo, s_xml.as_bytes());
                    }
                }
                if let Some(show_nfo) = find_show_nfo_for_episode(&p) {
                    let mut merged_show = VideoFileMeta::default();
                    let existing_show_raw = std::fs::read(&show_nfo).ok();
                    if let Some(ref raw) = existing_show_raw {
                        parse_nfo_into(raw, &mut merged_show);
                    }
                    if let Some(ref st) = meta.show_title
                        && !st.trim().is_empty()
                    {
                        merged_show.show_title = Some(st.clone());
                        merged_show.title = st.clone();
                    }
                    if merged_show.tmdb_id.is_none() {
                        merged_show.tmdb_id = meta.tmdb_id.clone();
                    }
                    if merged_show.bangumi_id.is_none() {
                        merged_show.bangumi_id = meta.bangumi_id.clone();
                    }
                    if merged_show.year.is_none() {
                        merged_show.year = meta.year;
                    }
                    if merged_show.studio.is_none() {
                        merged_show.studio = meta.studio.clone();
                    }
                    if merged_show.genres.is_empty() {
                        merged_show.genres = meta.genres.clone();
                    }
                    let show_summary = merged_show.summary.clone();
                    let show_xml = render_tvshow_nfo_xml_preserving(
                        &merged_show,
                        show_summary.as_deref(),
                        existing_show_raw.as_deref(),
                    );
                    let _ = atomic_write_bytes(&show_nfo, show_xml.as_bytes());
                }
            }

            Ok(())
        })
        .await
        .map_err(|e| DomainError::Storage(format!("video nfo write failed: {e}")))??;

        self.inspect_file(path).await
    }

    async fn artwork(&self, path: &Path, op: ArtworkIo<'_>) -> DomainResult<Option<BinaryPayload>> {
        match op {
            ArtworkIo::Read { slot } => read_video_artwork(path, slot).await,
            ArtworkIo::Write {
                slot,
                bytes,
                mime_type,
            } => {
                write_video_artwork(path, bytes, mime_type, slot).await?;
                Ok(None)
            }
        }
    }
}

async fn read_video_artwork(
    path: &Path,
    slot: Option<&str>,
) -> DomainResult<Option<BinaryPayload>> {
    let p = path.to_path_buf();
    let slot_owned = slot.map(|s| s.trim().to_ascii_lowercase());
    tokio::task::spawn_blocking(move || {
        let resolved = match slot_owned.as_deref() {
            Some("fanart") | Some("backdrop") => find_fanart_path(&p),
            Some("thumb") => find_thumb_path(&p),
            Some("clearlogo") | Some("logo") => find_clearlogo_path(&p),
            Some("poster") | Some("cover") => find_poster_path(&p),
            _ => find_poster_path(&p)
                .or_else(|| find_fanart_path(&p))
                .or_else(|| find_thumb_path(&p)),
        };
        let Some(img_path) = resolved else {
            return Ok(None);
        };
        let raw = std::fs::read(&img_path)
            .map_err(|e| DomainError::Storage(format!("failed reading poster: {e}")))?;
        let mime = sniff_image_mime(&raw).to_string();
        Ok(Some(BinaryPayload {
            bytes: raw,
            mime_type: mime,
        }))
    })
    .await
    .map_err(|e| DomainError::Storage(format!("video poster read failed: {e}")))?
}

async fn write_video_artwork(
    path: &Path,
    bytes: &[u8],
    mime_type: &str,
    slot: &str,
) -> DomainResult<()> {
    let parent = path
        .parent()
        .ok_or_else(|| DomainError::Validation("video file has no parent folder".into()))?
        .to_path_buf();
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("video")
        .to_string();
    let sniffed = sniff_image_mime(bytes);
    let detected = if sniffed != "image/jpeg" {
        sniffed
    } else if !mime_type.trim().is_empty() {
        mime_type.trim()
    } else {
        sniffed
    };
    let ext = if detected.contains("png") {
        "png"
    } else if detected.contains("webp") {
        "webp"
    } else {
        "jpg"
    };
    let is_jellyfin_movie_dir = parent.join("movie.nfo").is_file() && dir_has_single_video(&parent);
    let is_jellyfin_season = is_season_dir(&parent) || parent.join("season.nfo").is_file();
    let is_jellyfin_show = parent.join("tvshow.nfo").is_file();
    let use_dir_artwork = is_jellyfin_movie_dir || is_jellyfin_season || is_jellyfin_show;

    let is_thumb = slot.eq_ignore_ascii_case("thumb");
    let is_clearlogo = slot.eq_ignore_ascii_case("clearlogo") || slot.eq_ignore_ascii_case("logo");
    let is_fanart = slot.eq_ignore_ascii_case("fanart") || slot.eq_ignore_ascii_case("backdrop");
    let stem_base = if is_thumb {
        format!("{stem}-thumb")
    } else if is_clearlogo {
        if use_dir_artwork {
            "clearlogo".to_string()
        } else {
            format!("{stem}-clearlogo")
        }
    } else if is_fanart {
        if use_dir_artwork {
            "fanart".to_string()
        } else {
            format!("{stem}-fanart")
        }
    } else if use_dir_artwork {
        "poster".to_string()
    } else {
        format!("{stem}-poster")
    };
    let raw = bytes.to_vec();

    tokio::task::spawn_blocking(move || -> DomainResult<()> {
        write_image_replacing_opposite_ext(&parent, &stem_base, ext, &raw)?;
        if !is_thumb
            && is_jellyfin_season
            && let Some(show_dir) = parent.parent()
            && show_dir.join("tvshow.nfo").is_file()
        {
            write_image_replacing_opposite_ext(show_dir, &stem_base, ext, &raw)?;
        }
        Ok(())
    })
    .await
    .map_err(|e| DomainError::Storage(format!("video artwork write failed: {e}")))??;
    Ok(())
}

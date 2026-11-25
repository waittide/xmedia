pub mod filename;
pub mod nfo;
pub(crate) mod nfo_preserve;
pub mod nfo_write;
pub mod subtitle;

use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub use filename::{
    ParsedVideoFilename, dir_has_single_video, extract_video_resolution, guess_video_from_path,
    is_season_dir, list_sidecar_subtitles, parse_season_dir_number,
    parse_sidecar_subtitle_lang_and_label, parse_video_path,
};
pub use nfo::{
    NfoActor, NfoAudioStream, NfoRootKind, NfoSubtitleStream, NfoUniqueId, VideoNfoMeta,
    find_existing_nfo, find_season_nfo_for_episode, find_show_nfo_for_episode, parse_nfo_into,
    parse_nfo_xml,
};
pub use nfo_write::{
    render_season_nfo_xml_preserving, render_tvshow_nfo_xml_preserving,
    render_video_nfo_xml_preserving,
};
pub use subtitle::{
    convert_ass_to_vtt, convert_srt_to_vtt, convert_subtitle_bytes_to_vtt, shift_webvtt_timestamps,
};

/// Canonical video container formats supported across `xmedia-video` and `xmedia-tool`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VideoFormatExt {
    Mp4,
    Mkv,
    Webm,
    Mov,
    Avi,
    Ts,
    M2ts,
    Mts,
    M4v,
    Flv,
}

impl VideoFormatExt {
    pub const ALL: &'static [Self] = &[
        Self::Mp4,
        Self::Mkv,
        Self::Webm,
        Self::Mov,
        Self::Avi,
        Self::Ts,
        Self::M2ts,
        Self::Mts,
        Self::M4v,
        Self::Flv,
    ];

    pub const ALL_EXTENSIONS: &'static [&'static str] = &[
        "mp4", "mkv", "webm", "mov", "avi", "ts", "m2ts", "mts", "m4v", "flv",
    ];

    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext
            .trim()
            .trim_start_matches('.')
            .to_ascii_lowercase()
            .as_str()
        {
            "mp4" => Some(Self::Mp4),
            "mkv" => Some(Self::Mkv),
            "webm" => Some(Self::Webm),
            "mov" => Some(Self::Mov),
            "avi" => Some(Self::Avi),
            "ts" => Some(Self::Ts),
            "m2ts" => Some(Self::M2ts),
            "mts" => Some(Self::Mts),
            "m4v" => Some(Self::M4v),
            "flv" => Some(Self::Flv),
            _ => None,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Mp4 => "mp4",
            Self::Mkv => "mkv",
            Self::Webm => "webm",
            Self::Mov => "mov",
            Self::Avi => "avi",
            Self::Ts => "ts",
            Self::M2ts => "m2ts",
            Self::Mts => "mts",
            Self::M4v => "m4v",
            Self::Flv => "flv",
        }
    }

    pub const fn mime_type(self) -> &'static str {
        match self {
            Self::Mp4 | Self::M4v => "video/mp4",
            Self::Mkv => "video/x-matroska",
            Self::Webm => "video/webm",
            Self::Mov => "video/quicktime",
            Self::Avi => "video/x-msvideo",
            Self::Ts | Self::M2ts | Self::Mts => "video/mp2t",
            Self::Flv => "video/x-flv",
        }
    }
}

impl fmt::Display for VideoFormatExt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Canonical external/sidecar subtitle extensions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SubtitleFormatExt {
    Srt,
    Ass,
    Ssa,
    Vtt,
}

impl SubtitleFormatExt {
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext
            .trim()
            .trim_start_matches('.')
            .to_ascii_lowercase()
            .as_str()
        {
            "srt" => Some(Self::Srt),
            "ass" => Some(Self::Ass),
            "ssa" => Some(Self::Ssa),
            "vtt" | "webvtt" => Some(Self::Vtt),
            _ => None,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Srt => "srt",
            Self::Ass => "ass",
            Self::Ssa => "ssa",
            Self::Vtt => "vtt",
        }
    }
}

/// Returns `true` if `parent` is a dedicated movie/season/show directory eligible for shared artwork.
pub fn allow_dir_level_artwork(parent: &Path) -> bool {
    is_season_dir(parent) || parent.join("tvshow.nfo").is_file() || dir_has_single_video(parent)
}

pub fn find_thumb_path(video_path: &Path) -> Option<PathBuf> {
    let parent = video_path.parent()?;
    let stem = video_path.file_stem()?.to_str()?;
    ["jpg", "jpeg", "png", "webp"]
        .into_iter()
        .map(|ext| parent.join(format!("{stem}-thumb.{ext}")))
        .find(|p| p.is_file())
}

pub fn find_poster_path(video_path: &Path) -> Option<PathBuf> {
    let parent = video_path.parent()?;
    let stem = video_path.file_stem()?.to_str()?;
    let mut candidates = vec![
        parent.join(format!("{stem}-poster.jpg")),
        parent.join(format!("{stem}-poster.jpeg")),
        parent.join(format!("{stem}-poster.png")),
        parent.join(format!("{stem}-poster.webp")),
    ];
    if allow_dir_level_artwork(parent) {
        for name in [
            "poster.jpg",
            "poster.jpeg",
            "poster.png",
            "poster.webp",
            "folder.jpg",
            "folder.png",
            "folder.webp",
        ] {
            candidates.push(parent.join(name));
        }
    }
    if is_season_dir(parent)
        && let Some(show_dir) = parent.parent()
    {
        for name in [
            "poster.jpg",
            "poster.jpeg",
            "poster.png",
            "poster.webp",
            "folder.jpg",
            "folder.png",
            "folder.webp",
        ] {
            candidates.push(show_dir.join(name));
        }
    }
    candidates.into_iter().find(|p| p.is_file())
}

pub fn find_fanart_path(video_path: &Path) -> Option<PathBuf> {
    let parent = video_path.parent()?;
    let stem = video_path.file_stem()?.to_str()?;
    let mut candidates = vec![
        parent.join(format!("{stem}-fanart.jpg")),
        parent.join(format!("{stem}-fanart.jpeg")),
        parent.join(format!("{stem}-fanart.png")),
        parent.join(format!("{stem}-fanart.webp")),
    ];
    if allow_dir_level_artwork(parent) {
        for name in [
            "fanart.jpg",
            "fanart.jpeg",
            "fanart.png",
            "fanart.webp",
            "backdrop.jpg",
            "backdrop.jpeg",
            "backdrop.png",
            "backdrop.webp",
        ] {
            candidates.push(parent.join(name));
        }
    }
    if is_season_dir(parent)
        && let Some(show_dir) = parent.parent()
    {
        for name in [
            "fanart.jpg",
            "fanart.jpeg",
            "fanart.png",
            "fanart.webp",
            "backdrop.jpg",
            "backdrop.jpeg",
            "backdrop.png",
            "backdrop.webp",
        ] {
            candidates.push(show_dir.join(name));
        }
    }
    candidates.into_iter().find(|p| p.is_file())
}

pub fn find_clearlogo_path(video_path: &Path) -> Option<PathBuf> {
    let parent = video_path.parent()?;
    let stem = video_path.file_stem()?.to_str()?;
    for ext in ["png", "webp", "jpg"] {
        let p = parent.join(format!("{stem}-clearlogo.{ext}"));
        if p.is_file() {
            return Some(p);
        }
    }
    if allow_dir_level_artwork(parent) {
        for ext in ["png", "webp", "jpg"] {
            let p = parent.join(format!("clearlogo.{ext}"));
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

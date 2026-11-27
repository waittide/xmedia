use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RootId(pub Uuid);

impl RootId {
    fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for RootId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for RootId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A whitelisted root directory on disk that `xmedia-tool` is permitted to inspect and modify.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagedRoot {
    pub id: RootId,
    pub name: String,
    pub path: PathBuf,
    /// Optional domain hint (when `None`, all 4 domains can browse files inside this root).
    #[serde(default)]
    pub default_kind: Option<MediaKind>,
    /// When true, blocks all metadata writes, renames, artwork mutations, and archive edits.
    #[serde(default)]
    pub read_only: bool,
}

impl ManagedRoot {
    /// Constructs a `ManagedRoot` from an already-canonicalized directory path (pure value logic,
    /// no OS filesystem I/O). Infrastructure callers should validate/canonicalize via
    /// `crate::infra::workspace_fs::open_managed_root`.
    pub fn from_canonical(
        name: impl Into<String>,
        canonical_path: PathBuf,
        default_kind: Option<MediaKind>,
    ) -> Self {
        let clean_name = {
            let trimmed = name.into().trim().to_string();
            if trimmed.is_empty() {
                canonical_path
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| canonical_path.display().to_string())
            } else {
                trimmed
            }
        };
        Self {
            id: RootId::new(),
            name: clean_name,
            path: canonical_path,
            default_kind,
            read_only: false,
        }
    }

    pub fn ensure_writable(&self) -> DomainResult<()> {
        if self.read_only {
            return Err(DomainError::Forbidden(format!(
                "root '{}' is mounted read-only; disable read-only mode in settings to modify files",
                self.name
            )));
        }
        Ok(())
    }

    /// Pure boundary check verifying that a canonicalized filesystem path resides within `self.path`.
    pub fn verify_canonical_child(&self, canonical_abs: PathBuf) -> DomainResult<SandboxPath> {
        if !canonical_abs.starts_with(&self.path) {
            return Err(DomainError::Forbidden(
                "path escapes whitelisted root directory".into(),
            ));
        }
        let normalized_rel = canonical_abs
            .strip_prefix(&self.path)
            .unwrap_or(Path::new(""))
            .to_path_buf();
        Ok(SandboxPath {
            root_id: self.id,
            root_path: self.path.clone(),
            rel_path: normalized_rel,
            abs_path: canonical_abs,
        })
    }

    /// Pure boundary check verifying that a canonicalized ancestor directory stays within `self.path`
    /// before joining the non-existent relative `tail`.
    pub fn verify_canonical_target(
        &self,
        canonical_ancestor: PathBuf,
        tail: &Path,
    ) -> DomainResult<PathBuf> {
        if !canonical_ancestor.starts_with(&self.path) {
            return Err(DomainError::Forbidden(
                "target path escapes whitelisted root directory".into(),
            ));
        }
        Ok(canonical_ancestor.join(tail))
    }
}

/// Verified path guaranteed to reside inside a `ManagedRoot`.
#[derive(Debug, Clone)]
pub struct SandboxPath {
    pub root_id: RootId,
    pub root_path: PathBuf,
    pub rel_path: PathBuf,
    pub abs_path: PathBuf,
}

impl SandboxPath {
    pub fn rel_slash_string(&self) -> String {
        path_to_slash(&self.rel_path)
    }
}

pub fn path_to_slash(path: &Path) -> String {
    path.components()
        .filter_map(|c| match c {
            Component::Normal(os) => Some(os.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

pub fn sanitize_relative_path(raw: &str) -> DomainResult<PathBuf> {
    let normalized = raw.trim().replace('\\', "/");
    let trimmed = normalized.trim_start_matches('/');
    if trimmed.is_empty() {
        return Ok(PathBuf::new());
    }
    let mut out = PathBuf::new();
    for comp in Path::new(trimmed).components() {
        match comp {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(DomainError::Forbidden(
                    "relative path must not contain '..' or root prefixes".into(),
                ));
            }
        }
    }
    Ok(out)
}

pub(in crate::domain) fn sanitize_filename_component(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => ' ',
            c if c.is_control() => ' ',
            other => other,
        })
        .collect();
    cleaned
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_matches('.')
        .to_string()
}

pub fn paths_overlap(a: &Path, b: &Path) -> bool {
    a == b || a.starts_with(b) || b.starts_with(a)
}

pub const SHARED_DIR_SIDECAR_FILES: &[&str] = &[
    "metadata.opf",
    "movie.nfo",
    "season.nfo",
    "tvshow.nfo",
    "poster.jpg",
    "poster.jpeg",
    "poster.png",
    "poster.webp",
    "fanart.jpg",
    "fanart.jpeg",
    "fanart.png",
    "fanart.webp",
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

pub fn is_shared_dir_sidecar_name(file_name: &str) -> bool {
    let lower = file_name.to_ascii_lowercase();
    SHARED_DIR_SIDECAR_FILES.contains(&lower.as_str())
}

pub fn is_dedicated_subfolder_for(kind: MediaKind, dir_name: &str) -> bool {
    let trimmed = dir_name.trim();
    if trimmed.is_empty() {
        return false;
    }
    match kind {
        MediaKind::Book => !xmedia_formats::book::sidecar::is_generic_book_folder(trimmed),
        MediaKind::Comic => !xmedia_formats::comic::filename::is_generic_comic_folder(trimmed),
        MediaKind::Music => !xmedia_formats::music::is_generic_music_folder(trimmed),
        MediaKind::Video => true,
    }
}

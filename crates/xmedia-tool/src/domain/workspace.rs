use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RootId(pub Uuid);

impl RootId {
    pub fn new() -> Self {
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
    pub fn new(
        name: impl Into<String>,
        raw_path: impl AsRef<Path>,
        default_kind: Option<MediaKind>,
    ) -> DomainResult<Self> {
        let canonical = std::fs::canonicalize(raw_path.as_ref()).map_err(|e| {
            DomainError::Validation(format!(
                "root directory '{}' does not exist or cannot be accessed: {e}",
                raw_path.as_ref().display()
            ))
        })?;
        if !canonical.is_dir() {
            return Err(DomainError::Validation(format!(
                "path '{}' is not a directory",
                canonical.display()
            )));
        }
        let clean_name = {
            let trimmed = name.into().trim().to_string();
            if trimmed.is_empty() {
                canonical
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| canonical.display().to_string())
            } else {
                trimmed
            }
        };
        Ok(Self {
            id: RootId::new(),
            name: clean_name,
            path: canonical,
            default_kind,
            read_only: false,
        })
    }

    pub fn with_read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
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

    /// Resolves a relative path within this root, strictly preventing `..` or symlink traversal
    /// outside the canonicalized root boundary.
    pub fn resolve_existing(&self, rel_path: &str) -> DomainResult<SandboxPath> {
        let cleaned_rel = sanitize_relative_path(rel_path)?;
        let candidate = self.path.join(&cleaned_rel);
        let canonical = std::fs::canonicalize(&candidate).map_err(|e| {
            DomainError::NotFound(format!("path '{}' not found: {e}", cleaned_rel.display()))
        })?;
        if !canonical.starts_with(&self.path) {
            return Err(DomainError::Forbidden(
                "path escapes whitelisted root directory".into(),
            ));
        }
        let normalized_rel = canonical
            .strip_prefix(&self.path)
            .unwrap_or(Path::new(""))
            .to_path_buf();
        Ok(SandboxPath {
            root_id: self.id,
            root_path: self.path.clone(),
            rel_path: normalized_rel,
            abs_path: canonical,
        })
    }

    /// Resolves a target path for creating/renaming a file inside this root, verifying its parent
    /// directory stays strictly within the canonicalized root boundary.
    pub fn resolve_target(&self, rel_path: &str) -> DomainResult<PathBuf> {
        let cleaned_rel = sanitize_relative_path(rel_path)?;
        if cleaned_rel.as_os_str().is_empty() {
            return Err(DomainError::Validation(
                "target relative path cannot be empty".into(),
            ));
        }
        let candidate = self.path.join(&cleaned_rel);
        // Walk up to the nearest existing ancestor to canonicalize and check boundary
        let mut ancestor = candidate.parent().unwrap_or(&self.path);
        while !ancestor.exists() {
            ancestor = ancestor
                .parent()
                .ok_or_else(|| DomainError::Forbidden("invalid target path outside root".into()))?;
        }
        let canon_ancestor = std::fs::canonicalize(ancestor)
            .map_err(|e| DomainError::Storage(format!("failed to verify target directory: {e}")))?;
        if !canon_ancestor.starts_with(&self.path) {
            return Err(DomainError::Forbidden(
                "target path escapes whitelisted root directory".into(),
            ));
        }
        let tail = candidate.strip_prefix(ancestor).unwrap_or(Path::new(""));
        Ok(canon_ancestor.join(tail))
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
    let trimmed = raw.trim().trim_start_matches(['/', '\\']);
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

pub fn sanitize_filename_component(raw: &str) -> String {
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

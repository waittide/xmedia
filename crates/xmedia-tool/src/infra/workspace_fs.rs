use std::path::{Path, PathBuf};

use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;

use crate::domain::workspace::{ManagedRoot, SandboxPath, sanitize_relative_path};

/// Validates and canonicalizes a directory on disk, constructing a pure `ManagedRoot` domain value.
pub fn open_managed_root(
    name: impl Into<String>,
    raw_path: impl AsRef<Path>,
    default_kind: Option<MediaKind>,
) -> DomainResult<ManagedRoot> {
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
    Ok(ManagedRoot::from_canonical(name, canonical, default_kind))
}

/// Resolves an existing relative path inside `root`, strictly preventing `..` or symlink traversal
/// outside the canonicalized root boundary.
pub fn resolve_existing_in_root(root: &ManagedRoot, rel_path: &str) -> DomainResult<SandboxPath> {
    let cleaned_rel = sanitize_relative_path(rel_path)?;
    let candidate = root.path.join(&cleaned_rel);
    let canonical = std::fs::canonicalize(&candidate).map_err(|e| {
        DomainError::NotFound(format!("path '{}' not found: {e}", cleaned_rel.display()))
    })?;
    root.verify_canonical_child(canonical)
}

/// Resolves a target path for creating/renaming a file inside `root`, verifying its nearest
/// existing ancestor directory stays strictly within the canonicalized root boundary.
pub fn resolve_target_in_root(root: &ManagedRoot, rel_path: &str) -> DomainResult<PathBuf> {
    let cleaned_rel = sanitize_relative_path(rel_path)?;
    if cleaned_rel.as_os_str().is_empty() {
        return Err(DomainError::Validation(
            "target relative path cannot be empty".into(),
        ));
    }
    let candidate = root.path.join(&cleaned_rel);
    if let Ok(leaf_meta) = std::fs::symlink_metadata(&candidate)
        && leaf_meta.file_type().is_symlink()
    {
        return Err(DomainError::Forbidden(
            "target path cannot be a symbolic link".into(),
        ));
    }
    let mut ancestor = candidate.parent().unwrap_or(&root.path);
    while !ancestor.exists() {
        ancestor = ancestor
            .parent()
            .ok_or_else(|| DomainError::Forbidden("invalid target path outside root".into()))?;
    }
    let canon_ancestor = std::fs::canonicalize(ancestor)
        .map_err(|e| DomainError::Storage(format!("failed to verify target directory: {e}")))?;
    let tail = candidate.strip_prefix(ancestor).unwrap_or(Path::new(""));
    root.verify_canonical_target(canon_ancestor, tail)
}

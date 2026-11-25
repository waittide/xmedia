use std::path::{Component, Path, PathBuf};

use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;

use crate::domain::workspace::{ManagedRoot, SandboxPath, sanitize_relative_path};

pub fn paths_overlap(a: &Path, b: &Path) -> bool {
    a == b || a.starts_with(b) || b.starts_with(a)
}

/// Rejects filesystem roots, critical OS/system directories (`/etc`, `/proc`, `/sys`, `/dev`,
/// `/boot`, `/usr`, `/bin`, `/sbin`, `/lib`, etc.), and sensitive dot-directories (`.ssh`,
/// `.gnupg`, `.aws`, `.kube`) from being mounted as a `ManagedRoot` or `inbox_path`.
pub fn validate_safe_host_directory(canonical: &Path, label: &str) -> DomainResult<()> {
    if canonical.parent().is_none() {
        return Err(DomainError::Validation(format!(
            "{label} directory cannot be the filesystem root '{}'",
            canonical.display()
        )));
    }

    const BLOCKED_SUBTREES: &[&str] = &[
        "/etc",
        "/proc",
        "/sys",
        "/dev",
        "/boot",
        "/bin",
        "/sbin",
        "/lib",
        "/lib64",
        "/usr/bin",
        "/usr/sbin",
        "/usr/lib",
        "/usr/lib64",
        "/usr/libexec",
        "/usr/include",
        "/var/log",
        "/var/spool",
        "/var/run",
        "/var/lib/docker",
        "/var/lib/systemd",
        "/run/systemd",
        "/run/udev",
        "/private/etc",
        "/System",
    ];
    for blocked in BLOCKED_SUBTREES {
        let bp = Path::new(blocked);
        if canonical == bp || canonical.starts_with(bp) {
            return Err(DomainError::Validation(format!(
                "{label} directory '{}' points to a protected system path",
                canonical.display()
            )));
        }
    }

    const BLOCKED_EXACT_DIRS: &[&str] = &[
        "/usr",
        "/var",
        "/run",
        "/root",
        "/home",
        "/Users",
        "/tmp",
        "/var/tmp",
        "/private",
        "/private/tmp",
    ];
    for exact in BLOCKED_EXACT_DIRS {
        if canonical == Path::new(exact) {
            return Err(DomainError::Validation(format!(
                "{label} directory cannot be the shared system top-level path '{}'",
                canonical.display()
            )));
        }
    }

    const BLOCKED_SECRET_COMPONENTS: &[&str] =
        &[".ssh", ".gnupg", ".aws", ".kube", ".docker", ".pki"];
    for comp in canonical.components() {
        if let Component::Normal(name) = comp
            && let Some(s) = name.to_str()
            && BLOCKED_SECRET_COMPONENTS
                .iter()
                .any(|b| s.eq_ignore_ascii_case(b))
        {
            return Err(DomainError::Validation(format!(
                "{label} directory '{}' cannot reside inside a credential directory",
                canonical.display()
            )));
        }
    }

    Ok(())
}

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
    validate_safe_host_directory(&canonical, "root")?;
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

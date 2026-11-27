use std::path::{Component, Path, PathBuf};

use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;

use crate::application::ports::WorkspaceFsPort;
use crate::domain::media::MediaMetadataPayload;
use crate::domain::workspace::{ManagedRoot, SandboxPath, path_to_slash, sanitize_relative_path};
use crate::infra::atomic_fs::{cleanup_empty_parent_dirs, find_companion_sidecars};
use crate::infra::inspectors::{book, comic, music, video};

#[derive(Debug, Default, Clone, Copy)]
pub struct LocalWorkspaceFs;

impl WorkspaceFsPort for LocalWorkspaceFs {
    fn validate_safe_host_directory(&self, canonical: &Path, label: &str) -> DomainResult<()> {
        validate_safe_host_directory(canonical, label)
    }

    fn open_managed_root(
        &self,
        name: String,
        raw_path: &Path,
        default_kind: Option<MediaKind>,
    ) -> DomainResult<ManagedRoot> {
        open_managed_root(name, raw_path, default_kind)
    }

    fn resolve_existing_in_root(
        &self,
        root: &ManagedRoot,
        rel_path: &str,
    ) -> DomainResult<SandboxPath> {
        resolve_existing_in_root(root, rel_path)
    }

    fn resolve_target_in_root(&self, root: &ManagedRoot, rel_path: &str) -> DomainResult<PathBuf> {
        resolve_target_in_root(root, rel_path)
    }

    fn find_companion_sidecars(&self, media_file: &Path) -> Vec<(PathBuf, String)> {
        find_companion_sidecars(media_file)
    }

    fn collect_relocated_companions(
        &self,
        root_path: &Path,
        media_abs: &Path,
        rendered_stem: &str,
        parent_rel: &str,
        target_parent_rel: &str,
        meta: &MediaMetadataPayload,
    ) -> Vec<(String, String)> {
        let mut companion_moves = Vec::new();
        for (comp_abs, suffix) in find_companion_sidecars(media_abs) {
            if let Ok(comp_rel) = comp_abs.strip_prefix(root_path) {
                let comp_from = path_to_slash(comp_rel);
                let next_comp_name = format!("{rendered_stem}{suffix}");
                let comp_to = if target_parent_rel.is_empty() {
                    next_comp_name
                } else {
                    format!("{target_parent_rel}/{next_comp_name}")
                };
                companion_moves.push((comp_from, comp_to));
            }
        }

        if parent_rel == target_parent_rel || target_parent_rel.is_empty() {
            return companion_moves;
        }
        let Some(old_dir) = media_abs.parent().filter(|d| *d != root_path) else {
            return companion_moves;
        };

        let append_dir_files =
            |moves: &mut Vec<(String, String)>, dir: &Path, dst_rel: &str, names: &[&str]| {
                for fname in names {
                    let cand = dir.join(fname);
                    if cand.is_file()
                        && let Ok(rel_c) = cand.strip_prefix(root_path)
                    {
                        moves.push((path_to_slash(rel_c), format!("{dst_rel}/{fname}")));
                    }
                }
            };

        match meta {
            MediaMetadataPayload::Book(_) if book::dir_has_single_book(old_dir) => {
                append_dir_files(
                    &mut companion_moves,
                    old_dir,
                    target_parent_rel,
                    &[
                        "metadata.opf",
                        "cover.jpg",
                        "cover.jpeg",
                        "cover.png",
                        "cover.webp",
                    ],
                );
            }
            MediaMetadataPayload::Music(_)
                if old_dir
                    .file_name()
                    .and_then(|s| s.to_str())
                    .is_some_and(|n| !music::is_generic_music_folder(n)) =>
            {
                append_dir_files(
                    &mut companion_moves,
                    old_dir,
                    target_parent_rel,
                    &[
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
                    ],
                );
            }
            MediaMetadataPayload::Comic(_)
                if old_dir
                    .file_name()
                    .and_then(|s| s.to_str())
                    .is_some_and(|n| !comic::is_generic_comic_folder(n)) =>
            {
                append_dir_files(
                    &mut companion_moves,
                    old_dir,
                    target_parent_rel,
                    &[
                        "cover.jpg",
                        "cover.jpeg",
                        "cover.png",
                        "cover.webp",
                        "folder.jpg",
                        "folder.jpeg",
                        "folder.png",
                        "folder.webp",
                    ],
                );
            }
            MediaMetadataPayload::Video(v) => {
                let dir_files: &[&str] = if v.is_episode {
                    &[
                        "season.nfo",
                        "poster.jpg",
                        "poster.jpeg",
                        "poster.png",
                        "poster.webp",
                        "fanart.jpg",
                        "fanart.jpeg",
                        "fanart.png",
                        "fanart.webp",
                    ]
                } else if video::dir_has_single_video(old_dir) {
                    &[
                        "movie.nfo",
                        "poster.jpg",
                        "poster.jpeg",
                        "poster.png",
                        "poster.webp",
                        "fanart.jpg",
                        "fanart.jpeg",
                        "fanart.png",
                        "fanart.webp",
                    ]
                } else {
                    &[]
                };
                append_dir_files(&mut companion_moves, old_dir, target_parent_rel, dir_files);

                if v.is_episode
                    && let Some(old_show_dir) = old_dir.parent()
                    && old_show_dir != root_path
                    && old_show_dir.starts_with(root_path)
                    && let Some(new_show_parent) = Path::new(target_parent_rel).parent()
                {
                    let new_show_rel = path_to_slash(new_show_parent);
                    if !new_show_rel.is_empty() {
                        append_dir_files(
                            &mut companion_moves,
                            old_show_dir,
                            &new_show_rel,
                            &[
                                "tvshow.nfo",
                                "poster.jpg",
                                "poster.jpeg",
                                "poster.png",
                                "poster.webp",
                                "fanart.jpg",
                                "fanart.jpeg",
                                "fanart.png",
                                "fanart.webp",
                            ],
                        );
                    }
                }
            }
            _ => {}
        }

        companion_moves
    }

    fn cleanup_empty_parent_dirs(&self, start_dir: &Path, stop_at_root: &Path) {
        cleanup_empty_parent_dirs(start_dir, stop_at_root);
    }
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

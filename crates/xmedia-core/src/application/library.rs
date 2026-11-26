use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use tracing::{error, info, warn};

use crate::application::dto::{CreateLibraryInput, LibraryDto, RenameLibraryInput};
use crate::application::ports::{MediaDomainLifecycle, RealtimeEventPort, SystemEvent};
use crate::domain::error::{DomainError, DomainResult};
use crate::domain::library::{Library, LibraryId, LibraryKind, MediaKind, ScanStatus};
use crate::domain::repository::{LibraryMutation, LibraryRepository};
use crate::domain::user::User;

pub use crate::application::scan_reporter::ScanProgressReporter;

struct ActiveScanEntry {
    scan_seq: u64,
    abort_handle: Option<tokio::task::AbortHandle>,
    cancelled_by_delete: Arc<AtomicBool>,
    done: Arc<tokio::sync::Notify>,
}

struct ScanSettlementGuard {
    library_id: LibraryId,
    media_kind: MediaKind,
    scan_seq: u64,
    active_scans: Arc<Mutex<HashMap<LibraryId, ActiveScanEntry>>>,
    libraries: Arc<dyn LibraryRepository>,
    events: Arc<dyn RealtimeEventPort>,
    cancelled_by_delete: Arc<AtomicBool>,
    done: Arc<tokio::sync::Notify>,
    settled: bool,
}

impl Drop for ScanSettlementGuard {
    fn drop(&mut self) {
        {
            let mut active = self.active_scans.lock().unwrap_or_else(|e| e.into_inner());
            if active
                .get(&self.library_id)
                .is_some_and(|entry| entry.scan_seq == self.scan_seq)
            {
                active.remove(&self.library_id);
            }
        }
        self.done.notify_waiters();
        if !self.settled && !self.cancelled_by_delete.load(Ordering::Acquire) {
            let library_id = self.library_id;
            let media_kind = self.media_kind;
            let libraries = self.libraries.clone();
            let events = self.events.clone();
            tokio::spawn(async move {
                settle_scan_failure(
                    &libraries,
                    &events,
                    library_id,
                    media_kind,
                    "library scan aborted or terminated unexpectedly".to_string(),
                )
                .await;
            });
        }
    }
}

#[derive(Clone)]
pub struct LibraryService {
    libraries: Arc<dyn LibraryRepository>,
    events: Arc<dyn RealtimeEventPort>,
    lifecycles: Arc<RwLock<HashMap<MediaKind, Arc<dyn MediaDomainLifecycle>>>>,
    data_dir: Option<PathBuf>,
    allowed_mount_prefixes: Vec<PathBuf>,
    active_scans: Arc<Mutex<HashMap<LibraryId, ActiveScanEntry>>>,
    next_scan_seq: Arc<AtomicU64>,
    mount_lock: Arc<tokio::sync::Mutex<()>>,
}

impl LibraryService {
    pub fn new(
        libraries: Arc<dyn LibraryRepository>,
        events: Arc<dyn RealtimeEventPort>,
        lifecycles: Arc<RwLock<HashMap<MediaKind, Arc<dyn MediaDomainLifecycle>>>>,
    ) -> Self {
        let allowed_mount_prefixes = std::env::var("XMEDIA_ALLOWED_MOUNT_PREFIXES")
            .ok()
            .map(|raw| parse_allowed_mount_prefixes(&raw))
            .unwrap_or_default();
        Self {
            libraries,
            events,
            lifecycles,
            data_dir: None,
            allowed_mount_prefixes,
            active_scans: Arc::new(Mutex::new(HashMap::new())),
            next_scan_seq: Arc::new(AtomicU64::new(1)),
            mount_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    pub fn with_mount_policy(
        mut self,
        data_dir: Option<PathBuf>,
        allowed_prefixes: Option<Vec<PathBuf>>,
    ) -> Self {
        if let Some(dir) = data_dir {
            self.data_dir = Some(dir);
        }
        if let Some(prefixes) = allowed_prefixes {
            self.allowed_mount_prefixes = prefixes
                .into_iter()
                .filter(|p| !p.as_os_str().is_empty())
                .map(|pb| pb.canonicalize().unwrap_or(pb))
                .collect();
        }
        self
    }

    pub async fn recover_interrupted_scans(&self) -> DomainResult<()> {
        let libs = self.libraries.list(None).await?;
        for mut lib in libs {
            if lib.recover_interrupted_scan("scan interrupted by server restart") {
                warn!(library_id = %lib.id, name = %lib.name, "resetting interrupted library scan state from previous run");
                let _ = self
                    .libraries
                    .update(
                        lib.id,
                        LibraryMutation::ScanState {
                            status: &lib.scan_status,
                            last_scanned_at: lib.last_scanned_at,
                        },
                        lib.updated_at,
                    )
                    .await?;
            }
        }
        Ok(())
    }

    pub fn register_lifecycle(&self, handler: Arc<dyn MediaDomainLifecycle>) {
        self.lifecycles
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(handler.media_kind(), handler);
    }

    pub async fn ensure_managed_library(&self, media_kind: MediaKind) -> DomainResult<Library> {
        if let Some(existing) = self.libraries.find_managed_by_media(media_kind).await? {
            return Ok(existing);
        }
        let lib = Library::new_managed(media_kind)?;
        self.libraries.save(&lib).await?;
        Ok(lib)
    }

    pub async fn list_libraries(
        &self,
        media_kind: Option<MediaKind>,
    ) -> DomainResult<Vec<LibraryDto>> {
        let libs = self.libraries.list(media_kind).await?;
        let mut out = Vec::with_capacity(libs.len());
        for lib in libs {
            let count = self.count_items(&lib).await?;
            out.push(LibraryDto::from_domain(lib, count));
        }
        Ok(out)
    }

    pub async fn get_library(&self, id: LibraryId) -> DomainResult<LibraryDto> {
        let lib = self
            .libraries
            .find_by_id(id)
            .await?
            .ok_or_else(|| DomainError::NotFound("library not found".into()))?;
        let count = self.count_items(&lib).await?;
        Ok(LibraryDto::from_domain(lib, count))
    }

    pub async fn create_local_mount(
        &self,
        actor: &User,
        input: CreateLibraryInput,
    ) -> DomainResult<LibraryDto> {
        actor.ensure_admin()?;
        self.ensure_media_enabled(input.media_kind)?;

        let _mount_guard = self.mount_lock.lock().await;

        let raw_root = input.root_path.trim().to_string();
        let name = input.name;
        let media_kind = input.media_kind;
        let data_dir = self.data_dir.clone();
        let allowed_prefixes = self.allowed_mount_prefixes.clone();
        let existing_mounts: Vec<(String, String)> = self
            .libraries
            .list(None)
            .await?
            .into_iter()
            .filter_map(|existing| match existing.kind {
                LibraryKind::LocalMount { root_path } => Some((existing.name, root_path)),
                LibraryKind::Managed => None,
            })
            .collect();

        let lib = tokio::task::spawn_blocking(move || {
            build_canonical_local_mount(
                media_kind,
                name,
                &raw_root,
                &allowed_prefixes,
                data_dir.as_deref(),
                &existing_mounts,
            )
        })
        .await
        .map_err(|e| DomainError::Storage(format!("mount validation task failed: {e}")))??;
        self.libraries.save(&lib).await?;
        Ok(LibraryDto::from_domain(lib, 0))
    }

    pub async fn rename_library(
        &self,
        actor: &User,
        id: LibraryId,
        input: RenameLibraryInput,
    ) -> DomainResult<LibraryDto> {
        actor.ensure_admin()?;
        let mut lib = self
            .libraries
            .find_by_id(id)
            .await?
            .ok_or_else(|| DomainError::NotFound("library not found".into()))?;
        lib.rename(input.name)?;
        if !self
            .libraries
            .update(lib.id, LibraryMutation::Name(&lib.name), lib.updated_at)
            .await?
        {
            return Err(DomainError::NotFound("library not found".into()));
        }
        let count = self.count_items(&lib).await?;
        Ok(LibraryDto::from_domain(lib, count))
    }

    pub async fn trigger_scan(&self, actor: &User, id: LibraryId) -> DomainResult<LibraryDto> {
        actor.ensure_admin()?;
        let mut lib = self
            .libraries
            .find_by_id(id)
            .await?
            .ok_or_else(|| DomainError::NotFound("library not found".into()))?;

        lib.ensure_scannable()?;

        let handler = self.get_handler(lib.media_kind).ok_or_else(|| {
            DomainError::UnsupportedFormat(format!(
                "media module '{}' is not enabled in this build",
                lib.media_kind.as_str()
            ))
        })?;

        if let LibraryKind::LocalMount { ref root_path } = lib.kind {
            let existing_count = self.count_items(&lib).await.unwrap_or(0);
            let root = PathBuf::from(root_path);
            tokio::task::spawn_blocking(move || verify_mount_root_alive(&root, existing_count))
                .await
                .map_err(|e| DomainError::Storage(format!("mount health check failed: {e}")))??;
        }

        let scan_seq = self.next_scan_seq.fetch_add(1, Ordering::Relaxed);
        let cancelled_by_delete = Arc::new(AtomicBool::new(false));
        let done = Arc::new(tokio::sync::Notify::new());
        {
            let mut active = self.active_scans.lock().unwrap_or_else(|e| e.into_inner());
            if active.contains_key(&lib.id) {
                return Err(DomainError::Conflict(
                    "library scan is already in progress".into(),
                ));
            }
            active.insert(
                lib.id,
                ActiveScanEntry {
                    scan_seq,
                    abort_handle: None,
                    cancelled_by_delete: cancelled_by_delete.clone(),
                    done: done.clone(),
                },
            );
        }

        lib.set_scan_status(ScanStatus::Scanning {
            scanned: 0,
            total: None,
        });
        match self
            .libraries
            .update(
                lib.id,
                LibraryMutation::ScanState {
                    status: &lib.scan_status,
                    last_scanned_at: lib.last_scanned_at,
                },
                lib.updated_at,
            )
            .await
        {
            Ok(true) => {}
            Ok(false) => {
                self.remove_active_scan_if_seq(lib.id, scan_seq);
                return Err(DomainError::NotFound("library not found".into()));
            }
            Err(err) => {
                self.remove_active_scan_if_seq(lib.id, scan_seq);
                return Err(err);
            }
        }
        let count = match self.count_items(&lib).await {
            Ok(c) => c,
            Err(err) => {
                self.remove_active_scan_if_seq(lib.id, scan_seq);
                settle_scan_failure(
                    &self.libraries,
                    &self.events,
                    lib.id,
                    lib.media_kind,
                    err.to_string(),
                )
                .await;
                return Err(err);
            }
        };
        let dto = LibraryDto::from_domain(lib.clone(), count);

        self.events.publish(SystemEvent::LibraryScanStarted {
            library_id: lib.id,
            library_name: lib.name.clone(),
            media_kind: lib.media_kind,
        });

        let libraries_repo = self.libraries.clone();
        let events = self.events.clone();
        let active_scans = self.active_scans.clone();
        let lib_id = lib.id;
        let media_kind = lib.media_kind;
        let join_handle = tokio::spawn(async move {
            let mut guard = ScanSettlementGuard {
                library_id: lib_id,
                media_kind,
                scan_seq,
                active_scans,
                libraries: libraries_repo.clone(),
                events: events.clone(),
                cancelled_by_delete: cancelled_by_delete.clone(),
                done,
                settled: false,
            };
            info!(library_id = %lib.id, media = %lib.media_kind.as_str(), "starting background library scan");
            let res = handler.scan_library(lib).await;
            guard.settled = true;
            if cancelled_by_delete.load(Ordering::Acquire) {
                return;
            }
            match res {
                Ok(()) => {
                    settle_scan_failure(
                        &libraries_repo,
                        &events,
                        lib_id,
                        media_kind,
                        "library scan finished without reporting completion".to_string(),
                    )
                    .await;
                }
                Err(err) => {
                    error!(library_id = %lib_id, error = %err, "library scan failed");
                    settle_scan_failure(
                        &libraries_repo,
                        &events,
                        lib_id,
                        media_kind,
                        err.to_string(),
                    )
                    .await;
                }
            }
        });

        {
            let mut active = self.active_scans.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(entry) = active.get_mut(&lib_id)
                && entry.scan_seq == scan_seq
            {
                entry.abort_handle = Some(join_handle.abort_handle());
            } else {
                join_handle.abort();
            }
        }

        Ok(dto)
    }

    pub async fn delete_library(&self, actor: &User, id: LibraryId) -> DomainResult<()> {
        actor.ensure_admin()?;
        let lib = self
            .libraries
            .find_by_id(id)
            .await?
            .ok_or_else(|| DomainError::NotFound("library not found".into()))?;

        lib.ensure_deletable()?;

        let removed_scan = {
            let mut active = self.active_scans.lock().unwrap_or_else(|e| e.into_inner());
            active.remove(&id)
        };
        let was_scanning = lib.scan_status.is_scanning() || removed_scan.is_some();
        if let Some(entry) = removed_scan {
            entry.cancelled_by_delete.store(true, Ordering::Release);
            let notified = entry.done.notified();
            if let Some(handle) = entry.abort_handle {
                handle.abort();
                let _ = tokio::time::timeout(std::time::Duration::from_secs(2), notified).await;
            } else {
                tokio::task::yield_now().await;
            }
        }
        if was_scanning {
            self.events.publish(SystemEvent::LibraryScanFailed {
                library_id: id,
                library_name: lib.name.clone(),
                media_kind: lib.media_kind,
                message: "library deleted".to_string(),
            });
        }

        if let Some(handler) = self.get_handler(lib.media_kind) {
            handler.on_library_deleted(id).await?;
        }
        self.libraries.delete(id).await
    }

    fn remove_active_scan_if_seq(&self, id: LibraryId, scan_seq: u64) {
        let mut active = self.active_scans.lock().unwrap_or_else(|e| e.into_inner());
        if active
            .get(&id)
            .is_some_and(|entry| entry.scan_seq == scan_seq)
        {
            active.remove(&id);
        }
    }

    async fn count_items(&self, lib: &Library) -> DomainResult<u64> {
        if let Some(handler) = self.get_handler(lib.media_kind) {
            handler.count_library_items(lib.id).await
        } else {
            Ok(0)
        }
    }

    fn get_handler(&self, media_kind: MediaKind) -> Option<Arc<dyn MediaDomainLifecycle>> {
        self.lifecycles
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(&media_kind)
            .cloned()
    }

    fn ensure_media_enabled(&self, media_kind: MediaKind) -> DomainResult<()> {
        if self.get_handler(media_kind).is_none() {
            return Err(DomainError::UnsupportedFormat(format!(
                "media module '{}' is not enabled in this build",
                media_kind.as_str()
            )));
        }
        Ok(())
    }
}

fn build_canonical_local_mount(
    media_kind: MediaKind,
    name: String,
    raw_root: &str,
    allowed_prefixes: &[PathBuf],
    data_dir: Option<&Path>,
    existing_mounts: &[(String, String)],
) -> DomainResult<Library> {
    let path = Path::new(raw_root);
    if !path.exists() || !path.is_dir() {
        return Err(DomainError::Validation(format!(
            "mount directory does not exist or is not a directory: {raw_root}"
        )));
    }
    let canonical_path = path
        .canonicalize()
        .map_err(|e| DomainError::Storage(format!("failed to canonicalize path: {e}")))?;
    let data_canonical = data_dir.map(|d| d.canonicalize().unwrap_or_else(|_| d.to_path_buf()));
    let canonical_existing: Vec<(String, PathBuf)> = existing_mounts
        .iter()
        .map(|(existing_name, root)| {
            let p = Path::new(root);
            (
                existing_name.clone(),
                p.canonicalize().unwrap_or_else(|_| p.to_path_buf()),
            )
        })
        .collect();
    Library::new_local_mount(
        media_kind,
        name,
        &canonical_path,
        allowed_prefixes,
        data_canonical.as_deref(),
        &canonical_existing,
    )
}

fn parse_allowed_mount_prefixes(raw_allowlist: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for part in raw_allowlist.split(',') {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }
        for pb in std::env::split_paths(trimmed) {
            if pb.as_os_str().is_empty() {
                continue;
            }
            out.push(pb.canonicalize().unwrap_or(pb));
        }
    }
    out
}

fn verify_mount_root_alive(root_path: &Path, existing_item_count: u64) -> DomainResult<()> {
    if !root_path.exists() || !root_path.is_dir() {
        return Err(DomainError::Storage(format!(
            "mount directory '{}' is offline or missing; scan aborted to protect existing index",
            root_path.display()
        )));
    }
    if existing_item_count > 0 {
        let mut rd = std::fs::read_dir(root_path).map_err(|e| {
            DomainError::Storage(format!(
                "mount directory '{}' is unreadable ({e}); scan aborted",
                root_path.display()
            ))
        })?;
        if rd.next().is_none() {
            return Err(DomainError::Storage(format!(
                "mount directory '{}' appears empty while {} items are indexed (suspected unmounted NAS/USB); scan aborted to protect library data",
                root_path.display(),
                existing_item_count
            )));
        }
    }
    Ok(())
}

async fn settle_scan_failure(
    libraries_repo: &Arc<dyn LibraryRepository>,
    events: &Arc<dyn RealtimeEventPort>,
    library_id: LibraryId,
    media_kind: MediaKind,
    message: String,
) {
    if let Ok(Some(mut current)) = libraries_repo.find_by_id(library_id).await
        && current.scan_status.is_scanning()
    {
        current.set_scan_status(ScanStatus::Failed {
            message: message.clone(),
        });
        let existed = libraries_repo
            .update(
                current.id,
                LibraryMutation::ScanState {
                    status: &current.scan_status,
                    last_scanned_at: current.last_scanned_at,
                },
                current.updated_at,
            )
            .await
            .unwrap_or(false);
        if existed {
            events.publish(SystemEvent::LibraryScanFailed {
                library_id,
                library_name: current.name,
                media_kind,
                message,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mount_allowlist_and_unmount_guard_work() {
        let tmp = tempfile::tempdir().unwrap();
        let allowed = tmp.path().join("media");
        let sub = allowed.join("movies");
        let outside = tmp.path().join("etc");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::create_dir_all(&outside).unwrap();

        let prefixes = parse_allowed_mount_prefixes(&allowed.to_string_lossy());
        assert!(
            Library::new_local_mount(
                MediaKind::Video,
                "Movies",
                &sub.canonicalize().unwrap(),
                &prefixes,
                None,
                &[],
            )
            .is_ok()
        );
        assert!(
            Library::new_local_mount(
                MediaKind::Video,
                "Outside",
                &outside.canonicalize().unwrap(),
                &prefixes,
                None,
                &[],
            )
            .is_err()
        );

        assert!(verify_mount_root_alive(&sub, 0).is_ok());
        assert!(verify_mount_root_alive(&sub, 12).is_err());
        std::fs::write(sub.join("sample.mkv"), b"ok").unwrap();
        assert!(verify_mount_root_alive(&sub, 12).is_ok());
        assert!(verify_mount_root_alive(&tmp.path().join("missing"), 0).is_err());
    }
}

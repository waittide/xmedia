use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use tracing::{error, info, warn};

use crate::application::dto::{CreateLibraryInput, LibraryDto, RenameLibraryInput};
use crate::application::ports::{MediaDomainLifecycle, RealtimeEventPort, SystemEvent};
use crate::domain::error::{DomainError, DomainResult};
use crate::domain::library::{Library, LibraryId, LibraryKind, MediaKind};
use crate::domain::repository::LibraryRepository;
use crate::domain::user::User;

struct ActiveScanEntry {
    scan_seq: u64,
    abort_handle: Option<tokio::task::AbortHandle>,
}

struct ScanSettlementGuard {
    library_id: LibraryId,
    media_kind: MediaKind,
    scan_seq: u64,
    active_scans: Arc<Mutex<HashMap<LibraryId, ActiveScanEntry>>>,
    libraries: Arc<dyn LibraryRepository>,
    events: Arc<dyn RealtimeEventPort>,
    settled: bool,
}

impl Drop for ScanSettlementGuard {
    fn drop(&mut self) {
        {
            let mut active = self
                .active_scans
                .lock()
                .expect("active scans mutex poisoned");
            if active
                .get(&self.library_id)
                .is_some_and(|entry| entry.scan_seq == self.scan_seq)
            {
                active.remove(&self.library_id);
            }
        }
        if !self.settled {
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
    active_scans: Arc<Mutex<HashMap<LibraryId, ActiveScanEntry>>>,
    next_scan_seq: Arc<AtomicU64>,
}

impl LibraryService {
    pub fn new(
        libraries: Arc<dyn LibraryRepository>,
        events: Arc<dyn RealtimeEventPort>,
        lifecycles: Arc<RwLock<HashMap<MediaKind, Arc<dyn MediaDomainLifecycle>>>>,
    ) -> Self {
        Self::with_data_dir(libraries, events, lifecycles, None)
    }

    pub fn with_data_dir(
        libraries: Arc<dyn LibraryRepository>,
        events: Arc<dyn RealtimeEventPort>,
        lifecycles: Arc<RwLock<HashMap<MediaKind, Arc<dyn MediaDomainLifecycle>>>>,
        data_dir: Option<PathBuf>,
    ) -> Self {
        Self {
            libraries,
            events,
            lifecycles,
            data_dir,
            active_scans: Arc::new(Mutex::new(HashMap::new())),
            next_scan_seq: Arc::new(AtomicU64::new(1)),
        }
    }

    pub async fn recover_interrupted_scans(&self) -> DomainResult<()> {
        let libs = self.libraries.list_all().await?;
        for mut lib in libs {
            if lib.scan_status.is_scanning() {
                warn!(library_id = %lib.id, name = %lib.name, "resetting interrupted library scan state from previous run");
                lib.mark_scan_failed("scan interrupted by server restart");
                self.libraries.save(&lib).await?;
            }
        }
        Ok(())
    }

    pub fn register_lifecycle(&self, handler: Arc<dyn MediaDomainLifecycle>) {
        self.lifecycles
            .write()
            .expect("lifecycle registry lock poisoned")
            .insert(handler.media_kind(), handler);
    }

    pub async fn ensure_managed_library(&self, media_kind: MediaKind) -> DomainResult<Library> {
        if let Some(existing) = self.libraries.find_managed_by_media(media_kind).await? {
            return Ok(existing);
        }
        let lib = Library::new_managed(media_kind, media_kind.default_managed_name())?;
        self.libraries.save(&lib).await?;
        Ok(lib)
    }

    pub async fn list_libraries(
        &self,
        media_kind: Option<MediaKind>,
    ) -> DomainResult<Vec<LibraryDto>> {
        let libs = match media_kind {
            Some(kind) => self.libraries.list_by_media(kind).await?,
            None => self.libraries.list_all().await?,
        };
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
        Self::require_admin(actor)?;
        self.ensure_media_enabled(input.media_kind)?;

        let path = Path::new(input.root_path.trim());
        if !path.exists() || !path.is_dir() {
            return Err(DomainError::Validation(format!(
                "mount directory does not exist or is not a directory: {}",
                input.root_path.trim()
            )));
        }

        let canonical_path = path
            .canonicalize()
            .map_err(|e| DomainError::Storage(format!("failed to canonicalize path: {e}")))?;

        if let Ok(raw_allowlist) = std::env::var("XMEDIA_ALLOWED_MOUNT_PREFIXES") {
            verify_mount_allowlist(&canonical_path, &raw_allowlist)?;
        }

        if let Some(ref data_dir) = self.data_dir {
            let data_canonical = data_dir.canonicalize().unwrap_or_else(|_| data_dir.clone());
            if paths_overlap(&canonical_path, &data_canonical) {
                return Err(DomainError::Validation(
                    "mount path cannot overlap with xmedia data directory".into(),
                ));
            }
        }

        for existing in self.libraries.list_all().await? {
            if let LibraryKind::LocalMount { ref root_path } = existing.kind {
                let existing_path = Path::new(root_path);
                let existing_canonical = existing_path
                    .canonicalize()
                    .unwrap_or_else(|_| existing_path.to_path_buf());
                if paths_overlap(&canonical_path, &existing_canonical) {
                    return Err(DomainError::Validation(format!(
                        "mount path '{}' overlaps with existing library '{}' ({root_path})",
                        canonical_path.display(),
                        existing.name
                    )));
                }
            }
        }

        let canonical = canonical_path.to_string_lossy().to_string();

        let lib = Library::new_local_mount(input.media_kind, input.name, canonical)?;
        self.libraries.save(&lib).await?;
        Ok(LibraryDto::from_domain(lib, 0))
    }

    pub async fn rename_library(
        &self,
        actor: &User,
        id: LibraryId,
        input: RenameLibraryInput,
    ) -> DomainResult<LibraryDto> {
        Self::require_admin(actor)?;
        let mut lib = self
            .libraries
            .find_by_id(id)
            .await?
            .ok_or_else(|| DomainError::NotFound("library not found".into()))?;
        lib.rename(input.name)?;
        self.libraries.save(&lib).await?;
        let count = self.count_items(&lib).await?;
        Ok(LibraryDto::from_domain(lib, count))
    }

    pub async fn trigger_scan(&self, actor: &User, id: LibraryId) -> DomainResult<LibraryDto> {
        Self::require_admin(actor)?;
        let mut lib = self
            .libraries
            .find_by_id(id)
            .await?
            .ok_or_else(|| DomainError::NotFound("library not found".into()))?;

        if lib.kind.is_managed() {
            return Err(DomainError::Validation(
                "managed upload library does not require directory scanning".into(),
            ));
        }
        if lib.scan_status.is_scanning() {
            return Err(DomainError::Conflict(
                "library scan is already in progress".into(),
            ));
        }

        let handler = self.get_handler(lib.media_kind).ok_or_else(|| {
            DomainError::UnsupportedFormat(format!(
                "media module '{}' is not enabled in this build",
                lib.media_kind.as_str()
            ))
        })?;

        if let LibraryKind::LocalMount { ref root_path } = lib.kind {
            let existing_count = self.count_items(&lib).await.unwrap_or(0);
            verify_mount_root_alive(Path::new(root_path), existing_count)?;
        }

        let scan_seq = self.next_scan_seq.fetch_add(1, Ordering::Relaxed);
        {
            let mut active = self
                .active_scans
                .lock()
                .expect("active scans mutex poisoned");
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
                },
            );
        }

        lib.mark_scanning(0, None);
        if let Err(err) = self.libraries.save(&lib).await {
            self.remove_active_scan_if_seq(lib.id, scan_seq);
            return Err(err);
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
                settled: false,
            };
            info!(library_id = %lib.id, media = %lib.media_kind.as_str(), "starting background library scan");
            let res = handler.scan_library(lib).await;
            guard.settled = true;
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
            let mut active = self
                .active_scans
                .lock()
                .expect("active scans mutex poisoned");
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
        Self::require_admin(actor)?;
        let lib = self
            .libraries
            .find_by_id(id)
            .await?
            .ok_or_else(|| DomainError::NotFound("library not found".into()))?;

        if lib.kind.is_managed() {
            return Err(DomainError::Validation(
                "default managed library cannot be deleted".into(),
            ));
        }

        let removed_scan = {
            let mut active = self
                .active_scans
                .lock()
                .expect("active scans mutex poisoned");
            active.remove(&id)
        };
        let was_scanning = lib.scan_status.is_scanning() || removed_scan.is_some();
        if let Some(entry) = removed_scan {
            if let Some(handle) = entry.abort_handle {
                handle.abort();
            }
            tokio::task::yield_now().await;
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
        let mut active = self
            .active_scans
            .lock()
            .expect("active scans mutex poisoned");
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
            .expect("lifecycle registry lock poisoned")
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

    fn require_admin(actor: &User) -> DomainResult<()> {
        if !actor.role.is_admin() {
            return Err(DomainError::Forbidden(
                "administrator privileges required".into(),
            ));
        }
        Ok(())
    }
}

fn paths_overlap(a: &Path, b: &Path) -> bool {
    a == b || a.starts_with(b) || b.starts_with(a)
}

/// Validates that `canonical_path` falls under at least one allowed prefix when
/// `XMEDIA_ALLOWED_MOUNT_PREFIXES` is configured (colon or comma separated).
fn verify_mount_allowlist(canonical_path: &Path, raw_allowlist: &str) -> DomainResult<()> {
    let prefixes: Vec<PathBuf> = raw_allowlist
        .split([':', ','])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|p| {
            let pb = PathBuf::from(p);
            pb.canonicalize().unwrap_or(pb)
        })
        .collect();
    if prefixes.is_empty() {
        return Ok(());
    }
    if prefixes.iter().any(|p| canonical_path.starts_with(p)) {
        return Ok(());
    }
    Err(DomainError::Forbidden(format!(
        "mount path '{}' is outside XMEDIA_ALLOWED_MOUNT_PREFIXES",
        canonical_path.display()
    )))
}

/// Unmount / offline-NAS protection: aborts scan before mutating index if the mount
/// directory vanished or became completely empty when previously populated.
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
        current.mark_scan_failed(message.clone());
        let _ = libraries_repo.save(&current).await;
        events.publish(SystemEvent::LibraryScanFailed {
            library_id,
            library_name: current.name,
            media_kind,
            message,
        });
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

        let allowlist = allowed.to_string_lossy().to_string();
        assert!(verify_mount_allowlist(&sub.canonicalize().unwrap(), &allowlist).is_ok());
        assert!(verify_mount_allowlist(&outside.canonicalize().unwrap(), &allowlist).is_err());

        // Empty directory is allowed when library has 0 indexed items
        assert!(verify_mount_root_alive(&sub, 0).is_ok());
        // Empty directory is rejected when library already has indexed items (unmount protection)
        assert!(verify_mount_root_alive(&sub, 12).is_err());
        // Non-empty directory passes
        std::fs::write(sub.join("sample.mkv"), b"ok").unwrap();
        assert!(verify_mount_root_alive(&sub, 12).is_ok());
        // Missing directory always fails
        assert!(verify_mount_root_alive(&tmp.path().join("missing"), 0).is_err());
    }
}

use std::sync::Arc;

use crate::application::ports::{RealtimeEventPort, SystemEvent};
use crate::domain::error::{DomainError, DomainResult};
use crate::domain::library::Library;
use crate::domain::repository::LibraryRepository;

/// Reusable throttled scan progress reporter shared across media domain lifecycle scanners.
/// Handles persisting `ScanStatus` updates and broadcasting `SystemEvent` notifications.
pub struct ScanProgressReporter {
    library: Library,
    libraries: Arc<dyn LibraryRepository>,
    events: Arc<dyn RealtimeEventPort>,
    total: Option<u64>,
    progress_step: u64,
    scanned: u64,
}

impl ScanProgressReporter {
    pub fn new(
        library: Library,
        libraries: Arc<dyn LibraryRepository>,
        events: Arc<dyn RealtimeEventPort>,
        total: Option<u64>,
    ) -> Self {
        let progress_step = match total {
            Some(t) if t <= 100 => 1,
            Some(t) if t <= 1000 => 5,
            Some(_) => 20,
            None => 10,
        };
        Self {
            library,
            libraries,
            events,
            total,
            progress_step,
            scanned: 0,
        }
    }

    pub fn with_step(mut self, step: u64) -> Self {
        self.progress_step = step.max(1);
        self
    }

    pub fn library(&self) -> &Library {
        &self.library
    }

    pub fn scanned(&self) -> u64 {
        self.scanned
    }

    /// Marks the library as scanning at 0 items and broadcasts the initial progress event.
    pub async fn start(&mut self) {
        self.scanned = 0;
        self.library.mark_scanning(0, self.total);
        let updated = self
            .libraries
            .update_scan_state(
                self.library.id,
                &self.library.scan_status,
                self.library.last_scanned_at,
                self.library.updated_at,
            )
            .await
            .unwrap_or(false);
        if updated {
            self.events.publish(SystemEvent::LibraryScanProgress {
                library_id: self.library.id,
                library_name: self.library.name.clone(),
                media_kind: self.library.media_kind,
                scanned: 0,
                total: self.total,
            });
        }
    }

    /// Increments the scanned item counter by 1 and emits a throttled progress update.
    pub async fn advance(&mut self) {
        self.scanned = self.scanned.saturating_add(1);
        self.report(self.scanned).await;
    }

    /// Records the current scanned count and persists/broadcasts when hitting the throttle step or total.
    pub async fn report(&mut self, scanned: u64) {
        self.scanned = scanned;
        let reached_total = self.total.is_some_and(|t| scanned >= t);
        if scanned.is_multiple_of(self.progress_step) || reached_total {
            self.library.mark_scanning(scanned, self.total);
            let updated = self
                .libraries
                .update_scan_state(
                    self.library.id,
                    &self.library.scan_status,
                    self.library.last_scanned_at,
                    self.library.updated_at,
                )
                .await
                .unwrap_or(false);
            if updated {
                self.events.publish(SystemEvent::LibraryScanProgress {
                    library_id: self.library.id,
                    library_name: self.library.name.clone(),
                    media_kind: self.library.media_kind,
                    scanned,
                    total: self.total,
                });
            }
        }
    }

    /// Finalizes the scan using the internally tracked `scanned` count, persists completion,
    /// broadcasts `LibraryScanCompleted`, and emits `MediaChanged` if any items changed.
    pub async fn complete(self, added: u64, updated: u64, missing: u64) -> DomainResult<Library> {
        let scanned = self.scanned;
        self.complete_with_scanned(scanned, added, updated, missing)
            .await
    }

    /// Finalizes the scan with an explicit `scanned` count.
    pub async fn complete_with_scanned(
        mut self,
        scanned: u64,
        added: u64,
        updated: u64,
        missing: u64,
    ) -> DomainResult<Library> {
        self.library
            .mark_scan_completed(scanned, added, updated, missing);
        let existed = self
            .libraries
            .update_scan_state(
                self.library.id,
                &self.library.scan_status,
                self.library.last_scanned_at,
                self.library.updated_at,
            )
            .await?;
        if !existed {
            return Err(DomainError::NotFound(
                "library was deleted while scan was in progress".into(),
            ));
        }
        self.events.publish(SystemEvent::LibraryScanCompleted {
            library_id: self.library.id,
            library_name: self.library.name.clone(),
            media_kind: self.library.media_kind,
            scanned,
            added,
            updated,
            missing,
        });
        if added > 0 || updated > 0 || missing > 0 {
            self.events.publish(SystemEvent::MediaChanged {
                media_kind: self.library.media_kind,
            });
        }
        Ok(self.library)
    }

    /// Marks the scan as failed, persists best-effort, and broadcasts `LibraryScanFailed`.
    pub async fn fail(mut self, message: impl Into<String>) {
        let msg = message.into();
        self.library.mark_scan_failed(msg.clone());
        let existed = self
            .libraries
            .update_scan_state(
                self.library.id,
                &self.library.scan_status,
                self.library.last_scanned_at,
                self.library.updated_at,
            )
            .await
            .unwrap_or(false);
        if existed {
            self.events.publish(SystemEvent::LibraryScanFailed {
                library_id: self.library.id,
                library_name: self.library.name.clone(),
                media_kind: self.library.media_kind,
                message: msg,
            });
        }
    }
}

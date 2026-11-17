use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, RwLock};

use tracing::{error, info};

use crate::application::dto::{CreateLibraryInput, LibraryDto, RenameLibraryInput};
use crate::application::ports::{MediaDomainLifecycle, RealtimeEventPort, SystemEvent};
use crate::domain::error::{DomainError, DomainResult};
use crate::domain::library::{Library, LibraryId, MediaKind};
use crate::domain::repository::LibraryRepository;
use crate::domain::user::User;

#[derive(Clone)]
pub struct LibraryService {
    libraries: Arc<dyn LibraryRepository>,
    events: Arc<dyn RealtimeEventPort>,
    lifecycles: Arc<RwLock<HashMap<MediaKind, Arc<dyn MediaDomainLifecycle>>>>,
}

impl LibraryService {
    pub fn new(
        libraries: Arc<dyn LibraryRepository>,
        events: Arc<dyn RealtimeEventPort>,
        lifecycles: Arc<RwLock<HashMap<MediaKind, Arc<dyn MediaDomainLifecycle>>>>,
    ) -> Self {
        Self {
            libraries,
            events,
            lifecycles,
        }
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

        let canonical = path
            .canonicalize()
            .map_err(|e| DomainError::Storage(format!("failed to canonicalize path: {e}")))?
            .to_string_lossy()
            .to_string();

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

        lib.mark_scanning(0);
        self.libraries.save(&lib).await?;
        self.events.publish(SystemEvent::LibraryScanStarted {
            library_id: lib.id,
            media_kind: lib.media_kind,
        });

        let count = self.count_items(&lib).await?;
        let dto = LibraryDto::from_domain(lib.clone(), count);

        let libraries_repo = self.libraries.clone();
        let events = self.events.clone();
        tokio::spawn(async move {
            info!(library_id = %lib.id, media = %lib.media_kind.as_str(), "starting background library scan");
            if let Err(err) = handler.scan_library(lib.clone()).await {
                error!(library_id = %lib.id, error = %err, "library scan failed");
                let mut failed = lib;
                failed.mark_scan_failed(err.to_string());
                let _ = libraries_repo.save(&failed).await;
                events.publish(SystemEvent::LibraryScanFailed {
                    library_id: failed.id,
                    media_kind: failed.media_kind,
                    message: err.to_string(),
                });
            }
        });

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

        if let Some(handler) = self.get_handler(lib.media_kind) {
            handler.on_library_deleted(id).await?;
        }
        self.libraries.delete(id).await
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

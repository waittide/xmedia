pub mod shows;
pub mod sources;
pub mod videos;
pub mod watch;

use toasty::Db;
use xmedia_core::domain::error::DomainError;

pub use sources::{ToastyVideoSourceRepository, migrate_legacy_video_sources};
pub use watch::ToastyVideoWatchRepository;

pub(super) fn db_err(err: impl std::fmt::Display) -> DomainError {
    DomainError::Persistence(err.to_string())
}

#[derive(Clone)]
pub struct ToastyVideoCatalogRepository {
    pub(super) db: Db,
}

impl ToastyVideoCatalogRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

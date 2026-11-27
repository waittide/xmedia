mod catalog;
mod collections;
mod sources;

use xmedia_core::domain::error::DomainError;

pub use catalog::ToastyMusicCatalogRepository;
pub use collections::ToastyMusicCollectionRepository;
pub use sources::{ToastyMusicSourceRepository, migrate_legacy_music_sources};

pub(super) fn db_err(err: impl std::fmt::Display) -> DomainError {
    DomainError::Persistence(err.to_string())
}

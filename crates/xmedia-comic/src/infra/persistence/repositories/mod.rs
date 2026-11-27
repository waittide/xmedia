mod chapters;
mod reading;
mod series;

use xmedia_core::domain::error::DomainError;

pub use super::source_repository::{ToastyComicSourceRepository, migrate_legacy_comic_sources};
pub use chapters::ToastyChapterRepository;
pub use reading::ToastyComicReadingRepository;
pub use series::ToastySeriesRepository;

pub(super) fn db_err(err: impl std::fmt::Display) -> DomainError {
    DomainError::Persistence(err.to_string())
}

mod books;
mod online_shelf;
mod reading;
mod sources;

use xmedia_core::domain::error::DomainError;

pub use books::ToastyBookRepository;
pub use online_shelf::ToastyOnlineShelfRepository;
pub use reading::ToastyBookReadingRepository;
pub use sources::ToastyBookSourceRepository;

pub(super) fn db_err(err: impl std::fmt::Display) -> DomainError {
    DomainError::Persistence(err.to_string())
}

pub mod application;
pub mod domain;
pub mod http;
pub mod infra;
pub mod source;

#[cfg(test)]
mod tests;

use std::path::Path;
use std::sync::Arc;

use toasty::Db;
use xmedia_core::application::library::LibraryService;
use xmedia_core::application::ports::RealtimeEventPort;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::domain::repository::LibraryRepository;

use application::BookServices;
use application::catalog::BookCatalogService;
use application::lifecycle::BookDomainLifecycle;
use application::online::BookOnlineService;
use application::reading::BookReadingService;
use infra::parser::CompositeBookParser;
use infra::persistence::repositories::{
    ToastyBookReadingRepository, ToastyBookRepository, ToastyBookSourceRepository,
    ToastyOnlineShelfRepository,
};
use infra::storage::{LocalBookScanner, LocalBookStorage};

pub use infra::persistence::{ensure_book_schema, register_book_models};

pub fn init_book_module(
    db: Db,
    data_dir: &Path,
    libraries_repo: Arc<dyn LibraryRepository>,
    library_service: &LibraryService,
    events: Arc<dyn RealtimeEventPort>,
) -> DomainResult<BookServices> {
    let books = Arc::new(ToastyBookRepository::new(db.clone()));
    let readings = Arc::new(ToastyBookReadingRepository::new(db.clone()));
    let sources = Arc::new(ToastyBookSourceRepository::new(db.clone()));
    let online_shelves = Arc::new(ToastyOnlineShelfRepository::new(db));
    let parser = Arc::new(CompositeBookParser);
    let storage = Arc::new(LocalBookStorage::new(data_dir)?);
    let scanner = Arc::new(LocalBookScanner);

    let lifecycle = Arc::new(BookDomainLifecycle::new(
        libraries_repo.clone(),
        books.clone(),
        readings.clone(),
        online_shelves.clone(),
        scanner,
        parser.clone(),
        storage.clone(),
        events,
    ));
    library_service.register_lifecycle(lifecycle);

    let catalog = Arc::new(BookCatalogService::new(
        books.clone(),
        libraries_repo,
        readings.clone(),
        parser,
        storage,
    ));
    let reading = Arc::new(BookReadingService::new(readings, books));
    let online = Arc::new(BookOnlineService::new(
        sources,
        online_shelves,
        data_dir,
        catalog.clone(),
    )?);

    Ok(BookServices::new(catalog, reading, online))
}

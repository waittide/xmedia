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
use xmedia_core::application::bindings::MediaBindingService;
use xmedia_core::application::library::LibraryService;
use xmedia_core::application::online_items::OnlineLibraryService;
use xmedia_core::application::ports::RealtimeEventPort;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::domain::library::MediaKind;
use xmedia_core::domain::repository::LibraryRepository;
use xmedia_core::infra::persistence::repositories::{
    ToastyOnlineLibraryItemRepository, ToastyUserMediaBindingRepository,
};

use application::BookServices;
use application::catalog::BookCatalogService;
use application::lifecycle::BookDomainLifecycle;
use application::online::{BookOnlineService, BookOnlineSourceDirectory};
use application::reading::BookReadingService;
use infra::parser::CompositeBookParser;
use infra::persistence::repositories::{
    ToastyBookReadingRepository, ToastyBookRepository, ToastyBookSourceRepository,
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
    init_book_module_with_allow_private_ips(
        db,
        data_dir,
        libraries_repo,
        library_service,
        events,
        false,
    )
}

pub fn init_book_module_with_allow_private_ips(
    db: Db,
    data_dir: &Path,
    libraries_repo: Arc<dyn LibraryRepository>,
    library_service: &LibraryService,
    events: Arc<dyn RealtimeEventPort>,
    allow_private_ips: bool,
) -> DomainResult<BookServices> {
    let bindings = Arc::new(MediaBindingService::new(
        Arc::new(ToastyUserMediaBindingRepository::new(db.clone())),
        Some(events.clone()),
    ));
    init_book_module_with_bindings(
        db,
        data_dir,
        libraries_repo,
        library_service,
        bindings,
        events,
        allow_private_ips,
    )
}

pub fn init_book_module_with_bindings(
    db: Db,
    data_dir: &Path,
    libraries_repo: Arc<dyn LibraryRepository>,
    library_service: &LibraryService,
    bindings: Arc<MediaBindingService>,
    events: Arc<dyn RealtimeEventPort>,
    allow_private_ips: bool,
) -> DomainResult<BookServices> {
    let books = Arc::new(ToastyBookRepository::new(db.clone()));
    let readings = Arc::new(ToastyBookReadingRepository::new(db.clone()));
    let sources = Arc::new(ToastyBookSourceRepository::new(db.clone()));
    let online_items_repo = Arc::new(ToastyOnlineLibraryItemRepository::new(db));
    let parser = Arc::new(CompositeBookParser);
    let storage = Arc::new(LocalBookStorage::new(data_dir)?);
    let scanner = Arc::new(LocalBookScanner);

    let catalog = Arc::new(BookCatalogService::new(
        books.clone(),
        libraries_repo.clone(),
        readings.clone(),
        bindings.clone(),
        parser.clone(),
        storage.clone(),
    ));

    let reading = Arc::new(BookReadingService::new(
        readings.clone(),
        books.clone(),
        bindings.clone(),
    ));
    let engine = source::BookSourceEngine::new(
        source::http::HttpClientConfig {
            allow_private_ips,
            ..Default::default()
        },
        std::time::Duration::from_secs(45),
    );
    let online = Arc::new(BookOnlineService::with_engine(
        sources.clone(),
        data_dir,
        catalog.clone(),
        engine,
        allow_private_ips,
    )?);
    let online_items = Arc::new(OnlineLibraryService::new(
        MediaKind::Book,
        online_items_repo.clone(),
        Arc::new(BookOnlineSourceDirectory::new(sources)),
    ));

    let lifecycle = Arc::new(BookDomainLifecycle::new(
        libraries_repo,
        books,
        readings,
        online_items_repo,
        bindings,
        catalog.clone(),
        online.clone(),
        scanner,
        parser,
        storage,
        events,
    ));
    library_service.register_lifecycle(lifecycle);

    Ok(BookServices::new(catalog, reading, online, online_items))
}

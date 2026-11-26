pub mod application;
pub mod domain;
pub mod http;
pub mod infra;

#[cfg(test)]
mod tests;

use std::path::Path;
use std::sync::Arc;

use toasty::Db;
use xmedia_core::application::bindings::MediaBindingService;
use xmedia_core::application::library::LibraryService;
use xmedia_core::application::online_items::OnlineLibraryService;
use xmedia_core::application::ports::RealtimeEventPort;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;
use xmedia_core::domain::repository::LibraryRepository;
use xmedia_core::infra::persistence::repositories::{
    ToastyOnlineLibraryItemRepository, ToastyUserMediaBindingRepository,
};

use application::BookServices;
use application::catalog::BookCatalogService;
use application::lifecycle::BookDomainLifecycle;
use application::online::BookOnlineService;
use application::reading::BookReadingService;
use domain::online::LegadoBookSource;
use infra::parser::CompositeBookParser;
use infra::persistence::repositories::{
    ToastyBookReadingRepository, ToastyBookRepository, ToastyBookSourceRepository,
};
use infra::source::BookSourceEngine;
use infra::source::packager::BookFormatPackager;
use infra::source::tracked_store::TrackedBookStore;
use infra::storage::{LocalBookScanner, LocalBookStorage};
use xmedia_source::crud::SourceCrudService;

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
    let sources_repo = Arc::new(ToastyBookSourceRepository::with_cache_dir(
        db.clone(),
        data_dir.join("online_book_cache"),
    ));
    let sources = Arc::new(SourceCrudService::<LegadoBookSource>::new(sources_repo));
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
        libraries_repo.clone(),
        bindings.clone(),
        catalog.clone(),
    ));
    let engine = Arc::new(BookSourceEngine::new(
        infra::source::http::HttpClientConfig {
            allow_private_ips,
            ..Default::default()
        },
        std::time::Duration::from_secs(45),
    ));
    let packager = Arc::new(BookFormatPackager);
    let tracked_store = Arc::new(TrackedBookStore::new(data_dir).map_err(DomainError::Storage)?);
    let online = Arc::new(BookOnlineService::new(
        sources.clone(),
        data_dir,
        catalog.clone(),
        engine,
        packager,
        tracked_store,
        allow_private_ips,
    )?);
    let online_items = Arc::new(OnlineLibraryService::new(
        MediaKind::Book,
        online_items_repo.clone(),
        sources.clone(),
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

    Ok(BookServices::new(
        catalog,
        reading,
        sources,
        online,
        online_items,
    ))
}

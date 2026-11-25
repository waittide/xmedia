pub mod application;
pub mod domain;
pub mod http;
pub mod infra;

#[cfg(test)]
mod online_tests;
#[cfg(test)]
mod tests;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

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
use xmedia_source::crud::SourceCrudService;

use application::ComicServices;
use application::catalog::ComicCatalogService;
use application::lifecycle::ComicDomainLifecycle;
use application::online::ComicOnlineService;
use application::opds::ComicOpdsService;
use application::reading::ComicReadingService;
use domain::online::ComicSourceSpec;
use infra::archive::PureRustArchiveReader;
use infra::persistence::repositories::{
    ToastyChapterRepository, ToastyComicReadingRepository, ToastyComicSourceRepository,
    ToastySeriesRepository,
};
use infra::scanner::SmartComicScanner;
use infra::source::{ComicSourceEngine, http::HttpClientConfig};
use infra::storage::LocalComicFileStorage;
use infra::thumbnail::WebpThumbnailGenerator;

pub use infra::persistence::{ensure_comic_schema, register_comic_models};

pub fn init_comic_module(
    db: Db,
    data_dir: &Path,
    libraries_repo: Arc<dyn LibraryRepository>,
    library_service: &LibraryService,
    events: Arc<dyn RealtimeEventPort>,
) -> DomainResult<ComicServices> {
    init_comic_module_with_allow_private_ips(
        db,
        data_dir,
        libraries_repo,
        library_service,
        events,
        false,
    )
}

pub fn init_comic_module_with_allow_private_ips(
    db: Db,
    data_dir: &Path,
    libraries_repo: Arc<dyn LibraryRepository>,
    library_service: &LibraryService,
    events: Arc<dyn RealtimeEventPort>,
    allow_private_ips: bool,
) -> DomainResult<ComicServices> {
    let bindings_repo = Arc::new(ToastyUserMediaBindingRepository::new(db.clone()));
    let bindings = Arc::new(MediaBindingService::new(
        bindings_repo,
        Some(events.clone()),
    ));
    init_comic_module_with_bindings(
        db,
        data_dir,
        libraries_repo,
        library_service,
        events,
        bindings,
        allow_private_ips,
    )
}

pub fn init_comic_module_with_bindings(
    db: Db,
    data_dir: &Path,
    libraries_repo: Arc<dyn LibraryRepository>,
    library_service: &LibraryService,
    events: Arc<dyn RealtimeEventPort>,
    bindings: Arc<MediaBindingService>,
    allow_private_ips: bool,
) -> DomainResult<ComicServices> {
    let series = Arc::new(ToastySeriesRepository::new(db.clone()));
    let chapters = Arc::new(ToastyChapterRepository::new(db.clone()));
    let sources = Arc::new(ToastyComicSourceRepository::new(db.clone()));
    let online_items_repo = Arc::new(ToastyOnlineLibraryItemRepository::new(db.clone()));
    let reading_repo = Arc::new(ToastyComicReadingRepository::new(db));

    let archives = Arc::new(PureRustArchiveReader::new());
    let scanner = Arc::new(SmartComicScanner::new(archives.clone()));
    let thumbnails = Arc::new(WebpThumbnailGenerator::new(data_dir)?);
    let storage = Arc::new(LocalComicFileStorage::new(data_dir)?);

    let lifecycle = Arc::new(ComicDomainLifecycle::new(
        libraries_repo.clone(),
        series.clone(),
        chapters.clone(),
        reading_repo.clone(),
        bindings.clone(),
        scanner,
        archives.clone(),
        thumbnails.clone(),
        storage.clone(),
        events.clone(),
    ));
    library_service.register_lifecycle(lifecycle);

    let catalog = Arc::new(ComicCatalogService::new(
        libraries_repo.clone(),
        series.clone(),
        chapters.clone(),
        reading_repo.clone(),
        bindings.clone(),
        archives,
        thumbnails.clone(),
        storage.clone(),
        events.clone(),
    ));
    let reading = Arc::new(ComicReadingService::new(
        series.clone(),
        chapters.clone(),
        reading_repo.clone(),
        bindings.clone(),
    ));
    let opds = Arc::new(ComicOpdsService::new(
        libraries_repo.clone(),
        series.clone(),
        chapters.clone(),
        reading_repo.clone(),
        bindings,
    ));
    let engine = Arc::new(ComicSourceEngine::new(
        HttpClientConfig {
            timeout_secs: 20,
            allow_private_ips,
            ..Default::default()
        },
        Duration::from_secs(45),
    ));
    let source_crud = Arc::new(SourceCrudService::<ComicSourceSpec>::new(sources));
    let online = Arc::new(ComicOnlineService::with_engine(
        source_crud.clone(),
        libraries_repo,
        series,
        chapters,
        reading_repo,
        catalog.clone(),
        thumbnails,
        storage,
        events,
        engine,
        allow_private_ips,
    ));
    let online_items = Arc::new(OnlineLibraryService::new(
        MediaKind::Comic,
        online_items_repo,
        source_crud,
    ));

    Ok(ComicServices::new(
        catalog,
        reading,
        opds,
        online,
        online_items,
    ))
}

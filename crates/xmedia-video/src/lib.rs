pub mod application;
pub mod domain;
pub mod http;
pub mod infra;

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
use xmedia_core::infra::ffmpeg::FfmpegToolkit;
use xmedia_core::infra::persistence::repositories::{
    ToastyOnlineLibraryItemRepository, ToastyUserMediaBindingRepository,
};
use xmedia_source::crud::SourceCrudService;
use xmedia_source::http::HttpClientConfig;

use application::{VideoAppDeps, VideoServices, build_video_services};
use domain::online::VideoSourceSpec;
use infra::media_engine::FfmpegVideoEngine;
use infra::persistence::repositories::{
    ToastyVideoCatalogRepository, ToastyVideoSourceRepository, ToastyVideoWatchRepository,
};
use infra::source::{HttpVideoStreamTransport, VideoSourceEngine};
use infra::storage::{LocalVideoScanner, LocalVideoStorage};

pub use infra::persistence::{ensure_video_indexes, ensure_video_schema, register_video_models};

pub fn init_video_module(
    db: Db,
    data_dir: &Path,
    ffmpeg: FfmpegToolkit,
    libraries_repo: Arc<dyn LibraryRepository>,
    library_service: &LibraryService,
    events: Arc<dyn RealtimeEventPort>,
) -> DomainResult<VideoServices> {
    init_video_module_with_allow_private_ips(
        db,
        data_dir,
        ffmpeg,
        libraries_repo,
        library_service,
        events,
        false,
    )
}

pub fn init_video_module_with_allow_private_ips(
    db: Db,
    data_dir: &Path,
    ffmpeg: FfmpegToolkit,
    libraries_repo: Arc<dyn LibraryRepository>,
    library_service: &LibraryService,
    events: Arc<dyn RealtimeEventPort>,
    allow_private_ips: bool,
) -> DomainResult<VideoServices> {
    let binding_repo = Arc::new(ToastyUserMediaBindingRepository::new(db.clone()));
    let bindings = Arc::new(MediaBindingService::new(binding_repo, Some(events.clone())));
    init_video_module_with_bindings(
        db,
        data_dir,
        ffmpeg,
        libraries_repo,
        library_service,
        bindings,
        events,
        allow_private_ips,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn init_video_module_with_bindings(
    db: Db,
    data_dir: &Path,
    ffmpeg: FfmpegToolkit,
    libraries_repo: Arc<dyn LibraryRepository>,
    library_service: &LibraryService,
    bindings: Arc<MediaBindingService>,
    events: Arc<dyn RealtimeEventPort>,
    allow_private_ips: bool,
) -> DomainResult<VideoServices> {
    let catalog_repo = Arc::new(ToastyVideoCatalogRepository::new(db.clone()));
    let watch_repo = Arc::new(ToastyVideoWatchRepository::new(db.clone()));
    let online_items_repo = Arc::new(ToastyOnlineLibraryItemRepository::new(db.clone()));
    let source_repo = Arc::new(ToastyVideoSourceRepository::new(db));

    let storage = Arc::new(LocalVideoStorage::new(data_dir)?);
    let scanner = Arc::new(LocalVideoScanner);
    let engine = Arc::new(FfmpegVideoEngine::new(ffmpeg));

    let source_crud = Arc::new(SourceCrudService::<VideoSourceSpec>::new(source_repo));
    let http_config = HttpClientConfig {
        allow_private_ips,
        ..Default::default()
    };
    let source_engine = Arc::new(VideoSourceEngine::new(
        http_config.clone(),
        Duration::from_secs(45),
    ));
    let stream_transport = Arc::new(HttpVideoStreamTransport::new(http_config));
    let online_items = Arc::new(OnlineLibraryService::new(
        MediaKind::Video,
        online_items_repo,
        source_crud.clone(),
    ));

    let (services, lifecycle) = build_video_services(VideoAppDeps {
        libraries_repo,
        catalog_repo,
        watch_repo,
        bindings,
        storage,
        scanner,
        engine,
        events,
        source_crud,
        source_engine,
        stream_transport,
        online_items,
    });
    library_service.register_lifecycle(lifecycle);

    Ok(services)
}

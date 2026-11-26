pub mod catalog;
pub mod dto;
pub mod ingest;
pub mod lifecycle;
pub mod online;
pub mod ports;
mod projector;
pub mod shows;
pub mod streaming;
pub mod watch;

use std::sync::Arc;

use catalog::VideoCatalogService;
use ingest::VideoIngestService;
use lifecycle::VideoDomainLifecycle;
use online::{VideoOnlineService, VideoTrackingDeps};
use ports::{
    VideoMediaEnginePort, VideoScannerPort, VideoSourceEnginePort, VideoStoragePort,
    VideoStreamTransportPort,
};
use projector::VideoProjector;
use shows::VideoShowService;
use streaming::VideoStreamingService;
use watch::VideoWatchService;
use xmedia_core::application::bindings::MediaBindingService;
use xmedia_core::application::online_items::OnlineLibraryService;
use xmedia_core::application::ports::RealtimeEventPort;
use xmedia_core::domain::repository::LibraryRepository;
use xmedia_source::crud::SourceCrudService;

use crate::domain::online::VideoSourceSpec;
use crate::domain::repository::{VideoCatalogRepository, VideoWatchRepository};

#[derive(Clone)]
pub struct VideoServices {
    pub catalog: Arc<VideoCatalogService>,
    pub shows: Arc<VideoShowService>,
    pub ingest: Arc<VideoIngestService>,
    pub watch: Arc<VideoWatchService>,
    pub streaming: Arc<VideoStreamingService>,
    pub online: Arc<VideoOnlineService>,
    /// Per-user favorites / episode watch progress of online videos.
    pub online_items: Arc<OnlineLibraryService>,
}

pub(crate) struct VideoAppDeps {
    pub libraries_repo: Arc<dyn LibraryRepository>,
    pub catalog_repo: Arc<dyn VideoCatalogRepository>,
    pub watch_repo: Arc<dyn VideoWatchRepository>,
    pub bindings: Arc<MediaBindingService>,
    pub storage: Arc<dyn VideoStoragePort>,
    pub scanner: Arc<dyn VideoScannerPort>,
    pub engine: Arc<dyn VideoMediaEnginePort>,
    pub events: Arc<dyn RealtimeEventPort>,
    pub source_crud: Arc<SourceCrudService<VideoSourceSpec>>,
    pub source_engine: Arc<dyn VideoSourceEnginePort>,
    pub stream_transport: Arc<dyn VideoStreamTransportPort>,
    pub online_items: Arc<OnlineLibraryService>,
}

pub(crate) fn build_video_services(
    deps: VideoAppDeps,
) -> (VideoServices, Arc<VideoDomainLifecycle>) {
    let projector = VideoProjector::new(
        deps.catalog_repo.clone(),
        deps.watch_repo.clone(),
        deps.bindings.clone(),
    );
    let catalog = Arc::new(VideoCatalogService::new(
        deps.libraries_repo.clone(),
        deps.catalog_repo.clone(),
        deps.watch_repo.clone(),
        deps.bindings.clone(),
        deps.engine.clone(),
        projector.clone(),
    ));
    let shows = Arc::new(VideoShowService::new(
        deps.catalog_repo.clone(),
        deps.watch_repo.clone(),
        deps.bindings.clone(),
        deps.storage.clone(),
        projector.clone(),
    ));
    let ingest = Arc::new(VideoIngestService::new(
        deps.libraries_repo.clone(),
        deps.catalog_repo.clone(),
        deps.watch_repo.clone(),
        deps.bindings.clone(),
        deps.storage.clone(),
        deps.engine.clone(),
        deps.events.clone(),
        projector.clone(),
    ));
    let watch = Arc::new(VideoWatchService::new(
        deps.catalog_repo.clone(),
        deps.watch_repo.clone(),
        deps.bindings.clone(),
        projector.clone(),
    ));
    let streaming = Arc::new(VideoStreamingService::new(
        deps.libraries_repo.clone(),
        deps.catalog_repo.clone(),
        deps.bindings.clone(),
        deps.storage.clone(),
        deps.engine.clone(),
        projector.clone(),
    ));
    let lifecycle = Arc::new(VideoDomainLifecycle::new(
        deps.libraries_repo.clone(),
        deps.catalog_repo.clone(),
        deps.watch_repo,
        deps.bindings.clone(),
        ingest.clone(),
        deps.storage.clone(),
        deps.scanner,
        deps.engine.clone(),
        deps.events.clone(),
    ));
    let online = Arc::new(
        VideoOnlineService::new(deps.source_crud, deps.source_engine, deps.stream_transport)
            .with_tracking(VideoTrackingDeps {
                libraries: deps.libraries_repo,
                catalog_repo: deps.catalog_repo,
                bindings: deps.bindings,
                shows: shows.clone(),
                ingest: ingest.clone(),
                projector,
                storage: deps.storage,
                media_engine: deps.engine,
                events: deps.events,
            }),
    );

    (
        VideoServices {
            catalog,
            shows,
            ingest,
            watch,
            streaming,
            online,
            online_items: deps.online_items,
        },
        lifecycle,
    )
}

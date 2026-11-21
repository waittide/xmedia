pub mod catalog;
pub mod dto;
pub mod lifecycle;
pub mod ports;
pub mod streaming;
pub mod watch;

use std::sync::Arc;

use catalog::VideoCatalogService;
use lifecycle::VideoDomainLifecycle;
use streaming::VideoStreamingService;
use watch::VideoWatchService;

#[derive(Clone)]
pub struct VideoServices {
    pub catalog: Arc<VideoCatalogService>,
    pub watch: Arc<VideoWatchService>,
    pub streaming: Arc<VideoStreamingService>,
    pub ingest: Arc<VideoDomainLifecycle>,
}

impl VideoServices {
    pub fn new(
        catalog: Arc<VideoCatalogService>,
        watch: Arc<VideoWatchService>,
        streaming: Arc<VideoStreamingService>,
        ingest: Arc<VideoDomainLifecycle>,
    ) -> Self {
        Self {
            catalog,
            watch,
            streaming,
            ingest,
        }
    }
}

pub mod catalog;
pub mod dto;
pub mod lifecycle;
pub mod online;
pub mod ports;
pub mod streaming;
pub mod watch;

use std::sync::Arc;

use catalog::VideoCatalogService;
use lifecycle::VideoDomainLifecycle;
use online::VideoOnlineService;
use streaming::VideoStreamingService;
use watch::VideoWatchService;
use xmedia_core::application::online_items::OnlineLibraryService;

#[derive(Clone)]
pub struct VideoServices {
    pub catalog: Arc<VideoCatalogService>,
    pub watch: Arc<VideoWatchService>,
    pub streaming: Arc<VideoStreamingService>,
    pub ingest: Arc<VideoDomainLifecycle>,
    pub online: Arc<VideoOnlineService>,
    /// Per-user favorites / episode watch progress of online videos.
    pub online_items: Arc<OnlineLibraryService>,
}

impl VideoServices {
    pub fn new(
        catalog: Arc<VideoCatalogService>,
        watch: Arc<VideoWatchService>,
        streaming: Arc<VideoStreamingService>,
        ingest: Arc<VideoDomainLifecycle>,
        online: Arc<VideoOnlineService>,
        online_items: Arc<OnlineLibraryService>,
    ) -> Self {
        Self {
            catalog,
            watch,
            streaming,
            ingest,
            online,
            online_items,
        }
    }
}

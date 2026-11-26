pub mod browse;
pub mod catalog;
pub mod collection;
pub mod dto;
pub mod ingest;
pub mod lifecycle;
pub mod online;
pub mod ports;
pub(crate) mod projector;
pub mod streaming;
pub mod tracks;

use std::sync::Arc;

use browse::MusicBrowseService;
use catalog::MusicCatalogService;
use collection::MusicCollectionService;
use ingest::MusicIngestService;
use online::MusicOnlineService;
use streaming::MusicStreamingService;
use tracks::MusicTrackService;
use xmedia_core::application::online_items::OnlineLibraryService;

#[derive(Clone)]
pub struct MusicServices {
    pub catalog: Arc<MusicCatalogService>,
    pub browse: Arc<MusicBrowseService>,
    pub tracks: Arc<MusicTrackService>,
    pub collection: Arc<MusicCollectionService>,
    pub ingest: Arc<MusicIngestService>,
    pub streaming: Arc<MusicStreamingService>,
    pub online: Arc<MusicOnlineService>,
    /// Per-user favorites / playback progress of online or remote tracks.
    pub online_items: Arc<OnlineLibraryService>,
}

impl MusicServices {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        catalog: Arc<MusicCatalogService>,
        browse: Arc<MusicBrowseService>,
        tracks: Arc<MusicTrackService>,
        collection: Arc<MusicCollectionService>,
        ingest: Arc<MusicIngestService>,
        streaming: Arc<MusicStreamingService>,
        online: Arc<MusicOnlineService>,
        online_items: Arc<OnlineLibraryService>,
    ) -> Self {
        Self {
            catalog,
            browse,
            tracks,
            collection,
            ingest,
            streaming,
            online,
            online_items,
        }
    }
}

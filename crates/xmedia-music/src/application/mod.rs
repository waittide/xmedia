pub mod catalog;
pub mod collection;
pub mod dto;
pub mod ingest;
pub mod online;
pub mod ports;
pub mod streaming;

use std::sync::Arc;

use catalog::MusicCatalogService;
use collection::MusicCollectionService;
use ingest::MusicIngestService;
use online::MusicOnlineService;
use streaming::MusicStreamingService;

#[derive(Clone)]
pub struct MusicServices {
    pub catalog: Arc<MusicCatalogService>,
    pub collection: Arc<MusicCollectionService>,
    pub ingest: Arc<MusicIngestService>,
    pub streaming: Arc<MusicStreamingService>,
    pub online: Arc<MusicOnlineService>,
}

impl MusicServices {
    pub fn new(
        catalog: Arc<MusicCatalogService>,
        collection: Arc<MusicCollectionService>,
        ingest: Arc<MusicIngestService>,
        streaming: Arc<MusicStreamingService>,
        online: Arc<MusicOnlineService>,
    ) -> Self {
        Self {
            catalog,
            collection,
            ingest,
            streaming,
            online,
        }
    }
}

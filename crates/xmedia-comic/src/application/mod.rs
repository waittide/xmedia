pub mod catalog;
pub mod dto;
pub mod filename_parser;
pub mod lifecycle;
pub mod online;
pub mod opds;
pub mod ports;
pub mod reading;

use std::sync::Arc;

use catalog::ComicCatalogService;
use online::ComicOnlineService;
use opds::ComicOpdsService;
use reading::ComicReadingService;

#[derive(Clone)]
pub struct ComicServices {
    pub catalog: Arc<ComicCatalogService>,
    pub reading: Arc<ComicReadingService>,
    pub opds: Arc<ComicOpdsService>,
    pub online: Arc<ComicOnlineService>,
}

impl ComicServices {
    pub fn new(
        catalog: Arc<ComicCatalogService>,
        reading: Arc<ComicReadingService>,
        opds: Arc<ComicOpdsService>,
        online: Arc<ComicOnlineService>,
    ) -> Self {
        Self {
            catalog,
            reading,
            opds,
            online,
        }
    }
}

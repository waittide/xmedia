pub mod catalog;
pub mod dto;
pub mod lifecycle;
pub mod online;
pub mod ports;
pub mod reading;

use std::sync::Arc;

use xmedia_core::application::online_items::OnlineLibraryService;
use xmedia_source::crud::SourceCrudService;

use crate::domain::online::LegadoBookSource;
use catalog::BookCatalogService;
use online::BookOnlineService;
use reading::BookReadingService;

#[derive(Clone)]
pub struct BookServices {
    pub catalog: Arc<BookCatalogService>,
    pub reading: Arc<BookReadingService>,
    pub sources: Arc<SourceCrudService<LegadoBookSource>>,
    pub online: Arc<BookOnlineService>,
    pub online_items: Arc<OnlineLibraryService>,
}

impl BookServices {
    pub fn new(
        catalog: Arc<BookCatalogService>,
        reading: Arc<BookReadingService>,
        sources: Arc<SourceCrudService<LegadoBookSource>>,
        online: Arc<BookOnlineService>,
        online_items: Arc<OnlineLibraryService>,
    ) -> Self {
        Self {
            catalog,
            reading,
            sources,
            online,
            online_items,
        }
    }
}

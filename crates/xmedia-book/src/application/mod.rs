pub mod catalog;
pub mod dto;
pub mod lifecycle;
pub mod online;
pub mod ports;
pub mod reading;

use std::sync::Arc;

use catalog::BookCatalogService;
use online::BookOnlineService;
use reading::BookReadingService;

#[derive(Clone)]
pub struct BookServices {
    pub catalog: Arc<BookCatalogService>,
    pub reading: Arc<BookReadingService>,
    pub online: Arc<BookOnlineService>,
}

impl BookServices {
    pub fn new(
        catalog: Arc<BookCatalogService>,
        reading: Arc<BookReadingService>,
        online: Arc<BookOnlineService>,
    ) -> Self {
        Self {
            catalog,
            reading,
            online,
        }
    }
}

pub mod catalog;
pub mod dto;
pub mod lifecycle;
pub mod ports;
pub mod reading;

use std::sync::Arc;

use catalog::BookCatalogService;
use reading::BookReadingService;

#[derive(Clone)]
pub struct BookServices {
    pub catalog: Arc<BookCatalogService>,
    pub reading: Arc<BookReadingService>,
}

impl BookServices {
    pub fn new(catalog: Arc<BookCatalogService>, reading: Arc<BookReadingService>) -> Self {
        Self { catalog, reading }
    }
}

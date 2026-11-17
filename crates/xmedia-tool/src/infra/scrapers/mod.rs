pub mod http_client;

use std::sync::Arc;

use crate::domain::ports::ScraperProviderPort;

pub fn build_default_scraper_providers() -> Vec<Arc<dyn ScraperProviderPort>> {
    Vec::new()
}

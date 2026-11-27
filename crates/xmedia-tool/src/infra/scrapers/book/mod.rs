mod douban;
mod neodb;
mod openlibrary;
mod xinhua;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use crate::application::ports::ScraperProviderPort;

pub(super) fn providers() -> Vec<Arc<dyn ScraperProviderPort>> {
    vec![
        Arc::new(douban::DoubanBookProvider),
        Arc::new(neodb::NeoDbProvider),
        Arc::new(xinhua::XinhuaBookProvider),
        Arc::new(openlibrary::OpenLibraryProvider),
    ]
}

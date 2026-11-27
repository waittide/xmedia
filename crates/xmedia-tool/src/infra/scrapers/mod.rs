mod acgn;
pub mod artwork_downloader;
mod book;
mod html_util;
pub mod http_client;
mod music;
mod webnovel;

use std::sync::Arc;

use crate::application::ports::ScraperProviderPort;

/// Builds the default registry of pluggable `ScraperProviderPort` implementations.
pub fn build_default_scraper_providers() -> Vec<Arc<dyn ScraperProviderPort>> {
    let mut out = Vec::with_capacity(28);
    out.extend(book::providers());
    out.extend(webnovel::providers());
    out.extend(music::providers());
    out.extend(acgn::providers());
    out
}

#[cfg(test)]
mod tests;

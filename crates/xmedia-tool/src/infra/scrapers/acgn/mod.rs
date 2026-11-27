mod anilist;
mod bangumi;
mod hanime1;
mod mangadex;
mod nhentai;
mod tmdb;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use crate::application::ports::ScraperProviderPort;

pub(super) fn providers() -> Vec<Arc<dyn ScraperProviderPort>> {
    vec![
        Arc::new(bangumi::BangumiProvider),
        Arc::new(anilist::AniListProvider),
        Arc::new(mangadex::MangaDexProvider),
        Arc::new(nhentai::NhentaiProvider),
        Arc::new(tmdb::TmdbProvider),
        Arc::new(hanime1::Hanime1Provider),
    ]
}

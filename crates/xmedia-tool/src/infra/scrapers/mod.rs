pub mod anilist;
pub mod bangumi;
pub mod ciweimao;
pub mod cn_music_common;
pub mod cn_novel_common;
pub mod douban;
pub mod faloo;
pub mod fanqie;
pub mod hanime1;
pub mod http_client;
pub mod itunes;
pub mod jjwxc;
pub mod k17;
pub mod kugou;
pub mod kuwo;
pub mod lrclib;
pub mod mangadex;
pub mod migu;
pub mod musicbrainz;
pub mod neodb;
pub mod netease;
pub mod nhentai;
pub mod openlibrary;
pub mod qidian;
pub mod qimao;
pub mod qishui;
pub mod qq_music;
pub mod sfacg;
pub mod taihe;
pub mod tmdb;
pub mod xinhua;

use std::sync::Arc;

use crate::domain::ports::ScraperProviderPort;

/// Builds the default registry of pluggable `ScraperProviderPort` implementations.
///
/// Extending `xmedia-tool` with a new scraper source requires only:
/// 1. Implementing `ScraperProviderPort` in `infra/scrapers/<new_source>.rs`;
/// 2. Appending `Arc::new(...)` here.
///
/// Both the HTTP API (`/api/v1/tool/scrapers/providers`) and the Web UI discover new sources automatically.
pub fn build_default_scraper_providers() -> Vec<Arc<dyn ScraperProviderPort>> {
    vec![
        Arc::new(douban::DoubanBookProvider),
        Arc::new(neodb::NeoDbProvider),
        Arc::new(xinhua::XinhuaBookProvider),
        Arc::new(qidian::QidianProvider),
        Arc::new(fanqie::FanqieProvider),
        Arc::new(qimao::QimaoProvider),
        Arc::new(k17::K17Provider),
        Arc::new(ciweimao::CiweimaoProvider),
        Arc::new(sfacg::SfacgProvider),
        Arc::new(faloo::FalooProvider),
        Arc::new(jjwxc::JjwxcProvider),
        Arc::new(qq_music::QqMusicProvider),
        Arc::new(netease::NeteaseMusicProvider),
        Arc::new(kugou::KugouMusicProvider),
        Arc::new(kuwo::KuwoMusicProvider),
        Arc::new(migu::MiguMusicProvider),
        Arc::new(taihe::TaiheMusicProvider),
        Arc::new(qishui::QishuiMusicProvider),
        Arc::new(itunes::ItunesMusicProvider),
        Arc::new(musicbrainz::MusicBrainzProvider),
        Arc::new(lrclib::LrclibProvider),
        Arc::new(openlibrary::OpenLibraryProvider),
        Arc::new(bangumi::BangumiProvider),
        Arc::new(anilist::AniListProvider),
        Arc::new(mangadex::MangaDexProvider),
        Arc::new(nhentai::NhentaiProvider),
        Arc::new(tmdb::TmdbProvider),
        Arc::new(hanime1::Hanime1Provider),
    ]
}

#[cfg(test)]
mod tests;

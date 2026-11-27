mod common;
mod itunes;
mod kugou;
mod kuwo;
mod lrclib;
mod migu;
mod musicbrainz;
mod netease;
mod qishui;
mod qq_music;
mod taihe;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use crate::application::ports::ScraperProviderPort;

pub(super) fn providers() -> Vec<Arc<dyn ScraperProviderPort>> {
    vec![
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
    ]
}

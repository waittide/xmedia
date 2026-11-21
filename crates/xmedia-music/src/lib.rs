pub mod application;
pub mod domain;
pub mod http;
pub mod infra;

#[cfg(test)]
mod tests;

use std::path::Path;
use std::sync::Arc;

use toasty::Db;
use xmedia_core::application::library::LibraryService;
use xmedia_core::application::ports::RealtimeEventPort;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::domain::repository::LibraryRepository;
use xmedia_core::infra::ffmpeg::FfmpegToolkit;

use application::MusicServices;
use application::catalog::MusicCatalogService;
use application::collection::MusicCollectionService;
use application::ingest::MusicIngestService;
use application::streaming::MusicStreamingService;
use infra::audio_probe::LoftyAudioProbe;
use infra::lyrics::HybridLyricsResolver;
use infra::persistence::repositories::{
    ToastyMusicCatalogRepository, ToastyMusicCollectionRepository,
};
use infra::storage::{LocalMusicScanner, LocalMusicStorage};
use infra::transcoder::FfmpegAudioTranscoder;

pub use infra::persistence::{ensure_music_schema, register_music_models};

pub fn init_music_module(
    db: Db,
    data_dir: &Path,
    ffmpeg: FfmpegToolkit,
    libraries_repo: Arc<dyn LibraryRepository>,
    library_service: &LibraryService,
    events: Arc<dyn RealtimeEventPort>,
) -> DomainResult<MusicServices> {
    let catalog_repo = Arc::new(ToastyMusicCatalogRepository::new(db.clone()));
    let collections_repo = Arc::new(ToastyMusicCollectionRepository::new(db));

    let storage = Arc::new(LocalMusicStorage::new(data_dir)?);
    let scanner = Arc::new(LocalMusicScanner);
    let probe = Arc::new(LoftyAudioProbe);
    let lyrics = Arc::new(HybridLyricsResolver);
    let transcoder = Arc::new(FfmpegAudioTranscoder::new(ffmpeg));

    let catalog = MusicCatalogService::new(
        libraries_repo.clone(),
        catalog_repo.clone(),
        collections_repo.clone(),
        storage.clone(),
        lyrics,
    );
    let collection = Arc::new(MusicCollectionService::new(
        catalog_repo.clone(),
        collections_repo.clone(),
        catalog.clone(),
    ));
    let ingest = Arc::new(MusicIngestService::new(
        libraries_repo.clone(),
        catalog_repo.clone(),
        collections_repo,
        catalog.clone(),
        storage.clone(),
        scanner,
        probe,
        events,
    ));
    library_service.register_lifecycle(ingest.clone());

    let streaming = Arc::new(MusicStreamingService::new(
        libraries_repo,
        catalog_repo,
        storage,
        transcoder,
    ));

    Ok(MusicServices::new(
        Arc::new(catalog),
        collection,
        ingest,
        streaming,
    ))
}

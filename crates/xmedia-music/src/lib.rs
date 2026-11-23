pub mod application;
pub mod domain;
pub mod http;
pub mod infra;
pub mod source;

#[cfg(test)]
mod tests;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use toasty::Db;
use xmedia_core::application::library::LibraryService;
use xmedia_core::application::online_items::OnlineLibraryService;
use xmedia_core::application::ports::RealtimeEventPort;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::domain::library::MediaKind;
use xmedia_core::domain::repository::LibraryRepository;
use xmedia_core::infra::ffmpeg::FfmpegToolkit;
use xmedia_core::infra::persistence::repositories::ToastyOnlineLibraryItemRepository;
use xmedia_source::http::HttpClientConfig;

use application::MusicServices;
use application::catalog::MusicCatalogService;
use application::collection::MusicCollectionService;
use application::ingest::MusicIngestService;
use application::online::{MusicOnlineService, MusicOnlineSourceDirectory, MusicTrackingDeps};
use application::streaming::MusicStreamingService;
use infra::audio_probe::LoftyAudioProbe;
use infra::lyrics::HybridLyricsResolver;
use infra::persistence::repositories::{
    ToastyMusicCatalogRepository, ToastyMusicCollectionRepository, ToastyMusicSourceRepository,
};
use infra::storage::{LocalMusicScanner, LocalMusicStorage};
use infra::transcoder::FfmpegAudioTranscoder;
use source::MusicSourceEngine;

pub use infra::persistence::{ensure_music_schema, register_music_models};

pub fn init_music_module(
    db: Db,
    data_dir: &Path,
    ffmpeg: FfmpegToolkit,
    libraries_repo: Arc<dyn LibraryRepository>,
    library_service: &LibraryService,
    events: Arc<dyn RealtimeEventPort>,
) -> DomainResult<MusicServices> {
    init_music_module_with_allow_private_ips(
        db,
        data_dir,
        ffmpeg,
        libraries_repo,
        library_service,
        events,
        false,
    )
}

pub fn init_music_module_with_allow_private_ips(
    db: Db,
    data_dir: &Path,
    ffmpeg: FfmpegToolkit,
    libraries_repo: Arc<dyn LibraryRepository>,
    library_service: &LibraryService,
    events: Arc<dyn RealtimeEventPort>,
    allow_private_ips: bool,
) -> DomainResult<MusicServices> {
    let catalog_repo = Arc::new(ToastyMusicCatalogRepository::new(db.clone()));
    let collections_repo = Arc::new(ToastyMusicCollectionRepository::new(db.clone()));
    let online_items_repo = Arc::new(ToastyOnlineLibraryItemRepository::new(db.clone()));
    let sources_repo = Arc::new(ToastyMusicSourceRepository::new(db));

    let storage = Arc::new(LocalMusicStorage::new(data_dir)?);
    let scanner = Arc::new(LocalMusicScanner);
    let probe = Arc::new(LoftyAudioProbe);
    let lyrics = Arc::new(HybridLyricsResolver);
    let transcoder = Arc::new(FfmpegAudioTranscoder::new(ffmpeg));

    let catalog = Arc::new(MusicCatalogService::new(
        libraries_repo.clone(),
        catalog_repo.clone(),
        collections_repo.clone(),
        storage.clone(),
        lyrics,
    ));
    let collection = Arc::new(MusicCollectionService::new(
        catalog_repo.clone(),
        collections_repo.clone(),
        (*catalog).clone(),
    ));
    let ingest = Arc::new(MusicIngestService::new(
        libraries_repo.clone(),
        catalog_repo.clone(),
        collections_repo,
        (*catalog).clone(),
        storage.clone(),
        scanner,
        probe.clone(),
        events.clone(),
    ));
    library_service.register_lifecycle(ingest.clone());

    let streaming = Arc::new(MusicStreamingService::new(
        libraries_repo.clone(),
        catalog_repo.clone(),
        storage.clone(),
        transcoder,
    ));

    let engine = MusicSourceEngine::new(
        HttpClientConfig {
            allow_private_ips,
            ..Default::default()
        },
        Duration::from_secs(45),
    );
    let online = Arc::new(
        MusicOnlineService::with_engine(sources_repo.clone(), engine, allow_private_ips)
            .with_tracking(MusicTrackingDeps {
                libraries: libraries_repo,
                catalog_repo,
                catalog_service: catalog.clone(),
                ingest_service: ingest.clone(),
                storage,
                probe,
                events,
            }),
    );
    let online_items = Arc::new(OnlineLibraryService::new(
        MediaKind::Music,
        online_items_repo,
        Arc::new(MusicOnlineSourceDirectory::new(sources_repo)),
    ));

    Ok(MusicServices::new(
        catalog,
        collection,
        ingest,
        streaming,
        online,
        online_items,
    ))
}

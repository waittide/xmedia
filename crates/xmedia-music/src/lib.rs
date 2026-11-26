pub mod application;
pub mod domain;
pub mod http;
pub mod infra;

#[cfg(test)]
mod tests;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use toasty::Db;
use xmedia_core::application::bindings::MediaBindingService;
use xmedia_core::application::library::LibraryService;
use xmedia_core::application::online_items::OnlineLibraryService;
use xmedia_core::application::ports::RealtimeEventPort;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::domain::library::MediaKind;
use xmedia_core::domain::repository::LibraryRepository;
use xmedia_core::infra::ffmpeg::FfmpegToolkit;
use xmedia_core::infra::persistence::repositories::{
    ToastyOnlineLibraryItemRepository, ToastyUserMediaBindingRepository,
};
use xmedia_source::crud::{SourceCrudService, SourceRepository};
use xmedia_source::http::HttpClientConfig;

use application::MusicServices;
use application::browse::MusicBrowseService;
use application::catalog::MusicCatalogService;
use application::collection::MusicCollectionService;
use application::ingest::MusicIngestService;
use application::lifecycle::MusicDomainLifecycle;
use application::online::{MusicOnlineService, MusicTrackingDeps};
use application::projector::MusicProjector;
use application::streaming::MusicStreamingService;
use application::tracks::MusicTrackService;
use domain::online::MusicSourceSpec;
use infra::audio_probe::LoftyAudioProbe;
use infra::lyrics::HybridLyricsResolver;
use infra::persistence::repositories::{
    ToastyMusicCatalogRepository, ToastyMusicCollectionRepository, ToastyMusicSourceRepository,
};
use infra::source::{HttpMusicMediaProxy, MusicSourceEngine};
use infra::storage::{LocalMusicScanner, LocalMusicStorage};
use infra::transcoder::FfmpegAudioTranscoder;

pub use infra::persistence::{ensure_music_indexes, ensure_music_schema, register_music_models};

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
    let bindings_repo = Arc::new(ToastyUserMediaBindingRepository::new(db.clone()));
    let bindings = Arc::new(MediaBindingService::new(
        bindings_repo,
        Some(events.clone()),
    ));
    init_music_module_with_bindings(
        db,
        data_dir,
        ffmpeg,
        libraries_repo,
        library_service,
        events,
        bindings,
        allow_private_ips,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn init_music_module_with_bindings(
    db: Db,
    data_dir: &Path,
    ffmpeg: FfmpegToolkit,
    libraries_repo: Arc<dyn LibraryRepository>,
    library_service: &LibraryService,
    events: Arc<dyn RealtimeEventPort>,
    bindings: Arc<MediaBindingService>,
    allow_private_ips: bool,
) -> DomainResult<MusicServices> {
    let catalog_repo = Arc::new(ToastyMusicCatalogRepository::new(db.clone()));
    let collections_repo = Arc::new(ToastyMusicCollectionRepository::new(db.clone()));
    let online_items_repo = Arc::new(ToastyOnlineLibraryItemRepository::new(db.clone()));
    let sources_repo: Arc<dyn SourceRepository> = Arc::new(ToastyMusicSourceRepository::new(db));
    let sources_crud = Arc::new(SourceCrudService::<MusicSourceSpec>::new(sources_repo));

    let storage = Arc::new(LocalMusicStorage::new(data_dir)?);
    let scanner = Arc::new(LocalMusicScanner);
    let probe = Arc::new(LoftyAudioProbe);
    let lyrics = Arc::new(HybridLyricsResolver);
    let transcoder = Arc::new(FfmpegAudioTranscoder::new(ffmpeg));

    let projector = MusicProjector::new(
        catalog_repo.clone(),
        collections_repo.clone(),
        bindings.clone(),
    );
    let catalog = Arc::new(MusicCatalogService::new(
        catalog_repo.clone(),
        collections_repo.clone(),
        projector.clone(),
    ));
    let browse = Arc::new(MusicBrowseService::new(
        libraries_repo.clone(),
        catalog_repo.clone(),
        collections_repo.clone(),
        projector.clone(),
    ));
    let tracks = Arc::new(MusicTrackService::new(
        catalog_repo.clone(),
        collections_repo.clone(),
        bindings.clone(),
        projector.clone(),
    ));
    let collection = Arc::new(MusicCollectionService::new(
        catalog_repo.clone(),
        collections_repo.clone(),
        bindings.clone(),
        projector.clone(),
    ));
    let ingest = Arc::new(MusicIngestService::new(
        libraries_repo.clone(),
        catalog_repo.clone(),
        collections_repo.clone(),
        bindings.clone(),
        projector.clone(),
        storage.clone(),
        probe.clone(),
        events.clone(),
    ));
    let lifecycle = Arc::new(MusicDomainLifecycle::new(
        libraries_repo.clone(),
        catalog_repo.clone(),
        collections_repo.clone(),
        bindings.clone(),
        ingest.clone(),
        storage.clone(),
        scanner,
        probe.clone(),
        events.clone(),
    ));
    library_service.register_lifecycle(lifecycle);

    let streaming = Arc::new(MusicStreamingService::new(
        libraries_repo.clone(),
        catalog_repo.clone(),
        collections_repo,
        bindings.clone(),
        storage.clone(),
        lyrics,
        transcoder,
    ));

    let engine = Arc::new(MusicSourceEngine::new(
        HttpClientConfig {
            allow_private_ips,
            ..Default::default()
        },
        Duration::from_secs(45),
    ));
    let media_proxy = Arc::new(HttpMusicMediaProxy::new(allow_private_ips));
    let online = Arc::new(
        MusicOnlineService::with_ports(sources_crud.clone(), engine, media_proxy).with_tracking(
            MusicTrackingDeps {
                libraries: libraries_repo,
                catalog_repo,
                bindings,
                projector,
                tracks_service: tracks.clone(),
                ingest_service: ingest.clone(),
                storage,
                probe,
                events,
            },
        ),
    );
    let online_items = Arc::new(OnlineLibraryService::new(
        MediaKind::Music,
        online_items_repo,
        sources_crud,
    ));

    Ok(MusicServices::new(
        catalog,
        browse,
        tracks,
        collection,
        ingest,
        streaming,
        online,
        online_items,
    ))
}

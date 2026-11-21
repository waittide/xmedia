mod collections_and_events;
mod rest_and_subsonic;

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use tempfile::TempDir;
use toasty::Db;
use toasty::schema::ModelSet;
use xmedia_core::application::auth::AuthService;
use xmedia_core::application::library::LibraryService;
use xmedia_core::infra::events::BroadcastEventHub;
use xmedia_core::infra::ffmpeg::FfmpegToolkit;
use xmedia_core::infra::persistence::repositories::{
    ToastyLibraryRepository, ToastyUserRepository,
};
use xmedia_core::infra::security::{JwtTokenService, SecurityManager};
use xmedia_core::{ensure_core_schema, register_core_models};

use crate::{ensure_music_schema, init_music_module, register_music_models};

pub(super) fn make_wav(samples: u32) -> Vec<u8> {
    let sample_rate: u32 = 8000;
    let channels: u16 = 1;
    let bits: u16 = 16;
    let byte_rate = sample_rate * 2;
    let block_align: u16 = 2;
    let data_size = samples * 2;
    let riff_size = 36 + data_size;

    let mut buf = Vec::with_capacity((riff_size + 8) as usize);
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&riff_size.to_le_bytes());
    buf.extend_from_slice(b"WAVEfmt ");
    buf.extend_from_slice(&16u32.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes());
    buf.extend_from_slice(&channels.to_le_bytes());
    buf.extend_from_slice(&sample_rate.to_le_bytes());
    buf.extend_from_slice(&byte_rate.to_le_bytes());
    buf.extend_from_slice(&block_align.to_le_bytes());
    buf.extend_from_slice(&bits.to_le_bytes());
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_size.to_le_bytes());
    buf.resize((riff_size + 8) as usize, 0);
    buf
}

pub(super) struct TestEnv {
    pub(super) tmp: TempDir,
    pub(super) db: Db,
    pub(super) auth_svc: Arc<AuthService>,
    pub(super) library_svc: Arc<LibraryService>,
    pub(super) music_services: crate::application::MusicServices,
    pub(super) events: Arc<BroadcastEventHub>,
}

pub(super) async fn setup_test_env(db_name: &str) -> TestEnv {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join(db_name);
    let db_url = format!("sqlite:{}", db_path.display());

    let mut models = ModelSet::new();
    register_core_models(&mut models);
    register_music_models(&mut models);
    let mut db = Db::builder().models(models).connect(&db_url).await.unwrap();
    ensure_core_schema(&mut db, &db_url).await.unwrap();
    ensure_music_schema(&mut db, &db_url).await.unwrap();

    let users_repo = Arc::new(ToastyUserRepository::new(db.clone()));
    let libraries_repo = Arc::new(ToastyLibraryRepository::new(db.clone()));
    let lifecycles = Arc::new(RwLock::new(HashMap::new()));
    let events = Arc::new(BroadcastEventHub::default());
    let security = Arc::new(SecurityManager::new("music-unit-secret"));
    let tokens = Arc::new(JwtTokenService::new("music-unit-secret", 3600));

    let auth_svc = Arc::new(AuthService::new(
        users_repo,
        security,
        tokens,
        lifecycles.clone(),
    ));
    let library_svc = Arc::new(LibraryService::new(
        libraries_repo.clone(),
        events.clone(),
        lifecycles,
    ));
    let music_services = init_music_module(
        db.clone(),
        tmp.path(),
        FfmpegToolkit::default(),
        libraries_repo,
        &library_svc,
        events.clone(),
    )
    .unwrap();

    TestEnv {
        tmp,
        db,
        auth_svc,
        library_svc,
        music_services,
        events,
    }
}

pub mod application;
pub mod domain;
pub mod http;
pub mod infra;

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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, RwLock};

    use axum::Router;
    use axum::body::{Body, to_bytes};
    use axum::http::{Request, StatusCode, header};
    use bytes::Bytes;
    use encoding_rs::GB18030;
    use md5::{Digest, Md5};
    use tempfile::TempDir;
    use toasty::Db;
    use toasty::schema::ModelSet;
    use tower::ServiceExt;
    use xmedia_core::application::auth::AuthService;
    use xmedia_core::application::dto::{CreateLibraryInput, CreateUserInput, SetupAdminInput};
    use xmedia_core::application::library::LibraryService;
    use xmedia_core::domain::error::DomainError;
    use xmedia_core::domain::library::{MediaKind, ScanStatus};
    use xmedia_core::domain::user::UserRole;
    use xmedia_core::infra::events::BroadcastEventHub;
    use xmedia_core::infra::ffmpeg::FfmpegToolkit;
    use xmedia_core::infra::persistence::repositories::{
        ToastyLibraryRepository, ToastyUserRepository,
    };
    use xmedia_core::infra::security::{JwtTokenService, SecurityManager};
    use xmedia_core::{ensure_core_schema, register_core_models};

    use crate::application::dto::UploadFileItem;
    use crate::http::state::MusicHttpState;
    use crate::http::{rest_router, subsonic_router};
    use crate::{ensure_music_schema, init_music_module, register_music_models};

    fn make_wav(samples: u32) -> Vec<u8> {
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

    #[tokio::test]
    async fn music_rest_opensubsonic_and_incremental_scan_e2e() {
        let tmp = TempDir::new().unwrap();
        let db_path = tmp.path().join("xmedia_music.db");
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
        let security = Arc::new(SecurityManager::new("music-test-secret"));
        let tokens = Arc::new(JwtTokenService::new("music-test-secret", 3600));

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
            db,
            tmp.path(),
            FfmpegToolkit::default(),
            libraries_repo,
            &library_svc,
            events,
        )
        .unwrap();

        let session = auth_svc
            .setup_initial_admin(SetupAdminInput {
                username: "alice".into(),
                display_name: Some("Alice".into()),
                password: "secretpassword".into(),
            })
            .await
            .unwrap();
        let admin = auth_svc.authenticate_jwt(&session.token).await.unwrap();
        let bob = auth_svc
            .create_user(
                &admin,
                CreateUserInput {
                    username: "bob".into(),
                    display_name: Some("Bob".into()),
                    password: "bobpassword".into(),
                    role: UserRole::Member,
                },
            )
            .await
            .unwrap();

        let state = MusicHttpState::new(
            auth_svc.clone(),
            library_svc.clone(),
            music_services.clone(),
        );
        let app = Router::new()
            .nest("/api/v1/music", rest_router(state.clone()))
            .nest("/rest", subsonic_router(state));

        // 1. Upload WAV + GB18030 LRC sidecar
        let (gbk_lrc, _, _) =
            GB18030.encode("[ti:夜曲]\n[ar:周杰伦]\n[00:01.50]为你弹奏肖邦的夜曲\n");
        let uploaded = music_services
            .ingest
            .upload_files(
                &admin,
                vec![
                    UploadFileItem {
                        filename: "Nocturne.wav".into(),
                        bytes: Bytes::from(make_wav(16_000)),
                    },
                    UploadFileItem {
                        filename: "Nocturne.lrc".into(),
                        bytes: Bytes::copy_from_slice(&gbk_lrc),
                    },
                ],
            )
            .await
            .unwrap();
        assert_eq!(uploaded.uploaded_tracks.len(), 1);
        assert_eq!(uploaded.attached_lyrics_count, 1);
        let track_id = uploaded.uploaded_tracks[0].id;

        // 2. Verify HTTP Range 206 partial audio streaming
        let range_res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/music/tracks/{track_id}/stream?token={}",
                        session.token
                    ))
                    .header(header::RANGE, "bytes=0-99")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(range_res.status(), StatusCode::PARTIAL_CONTENT);
        let range_bytes = to_bytes(range_res.into_body(), usize::MAX).await.unwrap();
        assert_eq!(range_bytes.len(), 100);
        assert_eq!(&range_bytes[0..4], b"RIFF");

        // 3. Verify OpenSubsonic unauthenticated getOpenSubsonicExtensions + token/salt auth + formPost lyrics
        let ext_res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/rest/getOpenSubsonicExtensions.view?f=json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(ext_res.status(), StatusCode::OK);
        let ext_json: serde_json::Value =
            serde_json::from_slice(&to_bytes(ext_res.into_body(), usize::MAX).await.unwrap())
                .unwrap();
        assert_eq!(ext_json["subsonic-response"]["openSubsonic"], true);

        let salt = "salty99";
        let mut md5 = Md5::new();
        md5.update(b"secretpassword");
        md5.update(salt.as_bytes());
        let token_hex = hex::encode(md5.finalize());

        let lyr_res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/rest/getLyricsBySongId.view")
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .body(Body::from(format!(
                        "u=alice&t={token_hex}&s={salt}&v=1.16.1&c=symfonium&f=json&id={track_id}"
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(lyr_res.status(), StatusCode::OK);
        let lyr_json: serde_json::Value =
            serde_json::from_slice(&to_bytes(lyr_res.into_body(), usize::MAX).await.unwrap())
                .unwrap();
        assert_eq!(lyr_json["subsonic-response"]["status"], "ok");
        assert_eq!(
            lyr_json["subsonic-response"]["lyricsList"]["structuredLyrics"][0]["line"][0]["value"],
            "为你弹奏肖邦的夜曲"
        );

        // 4. Verify OpenSubsonic apiKeyAuthentication + multi-user star isolation
        let star_res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/rest/star?apiKey={}&f=json&id={track_id}",
                        session.user.api_key
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(star_res.status(), StatusCode::OK);
        assert_eq!(
            music_services
                .collection
                .get_starred(admin.id, None)
                .await
                .unwrap()
                .tracks
                .len(),
            1
        );
        assert_eq!(
            music_services
                .collection
                .get_starred(bob.id, None)
                .await
                .unwrap()
                .tracks
                .len(),
            0
        );

        // 5. Verify local mount incremental scan via LibraryService
        let mount_dir = tmp.path().join("mounted_music");
        std::fs::create_dir_all(&mount_dir).unwrap();
        let m1 = mount_dir.join("TrackA.wav");
        let m2 = mount_dir.join("TrackB.wav");
        std::fs::write(&m1, make_wav(8_000)).unwrap();
        std::fs::write(&m2, make_wav(8_000)).unwrap();

        let created_lib = library_svc
            .create_local_mount(
                &admin,
                CreateLibraryInput {
                    name: "Lossless Mount".into(),
                    media_kind: MediaKind::Music,
                    root_path: mount_dir.to_string_lossy().into_owned(),
                },
            )
            .await
            .unwrap();
        library_svc
            .trigger_scan(&admin, created_lib.id)
            .await
            .unwrap();

        for _ in 0..50 {
            let lib_now = library_svc.get_library(created_lib.id).await.unwrap();
            if matches!(lib_now.scan_status, ScanStatus::Completed { .. }) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        assert_eq!(
            library_svc
                .get_library(created_lib.id)
                .await
                .unwrap()
                .scan_status,
            ScanStatus::Completed {
                scanned: 2,
                added: 2,
                updated: 0,
                missing: 0,
            }
        );

        // Remove TrackB.wav -> incremental rescan marks 1 missing
        std::fs::remove_file(&m2).unwrap();
        library_svc
            .trigger_scan(&admin, created_lib.id)
            .await
            .unwrap();
        for _ in 0..50 {
            let lib_now = library_svc.get_library(created_lib.id).await.unwrap();
            if matches!(
                lib_now.scan_status,
                ScanStatus::Completed { missing: 1, .. }
            ) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        assert_eq!(
            library_svc
                .get_library(created_lib.id)
                .await
                .unwrap()
                .item_count,
            1
        );

        // 6. Verify empty/orphan LRC upload rejection, non-admin tag denial, cover auth,
        //    private playlist isolation, lyrics artist fallback, and delete_track cleanup
        let orphan_res = music_services
            .ingest
            .upload_files(
                &admin,
                vec![UploadFileItem {
                    filename: "orphan.lrc".into(),
                    bytes: Bytes::from_static(b"[00:01.00]orphan line\n"),
                }],
            )
            .await;
        assert!(matches!(orphan_res, Err(DomainError::Validation(_))));

        let bob_user = auth_svc.authenticate_api_key(&bob.api_key).await.unwrap();
        let bob_tag_res = music_services
            .catalog
            .set_track_tags(&bob_user, track_id, vec!["night".into()])
            .await;
        assert!(matches!(bob_tag_res, Err(DomainError::Forbidden(_))));

        let unauth_cover_res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/music/covers/nonexistent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauth_cover_res.status(), StatusCode::UNAUTHORIZED);

        let artist_name = uploaded.uploaded_tracks[0].artist_name.clone();
        let fallback_lyrics = music_services
            .catalog
            .get_lyrics_by_artist_title(Some(&artist_name), None)
            .await
            .unwrap();
        assert!(!fallback_lyrics.is_empty());

        let pl = music_services
            .collection
            .create_playlist(&admin, "Admin Private", None, false, vec![track_id])
            .await
            .unwrap();
        assert_eq!(pl.tracks.len(), 1);
        assert!(matches!(
            music_services
                .collection
                .get_playlist(&bob_user, pl.playlist.id)
                .await,
            Err(DomainError::Forbidden(_))
        ));

        music_services
            .ingest
            .delete_track(&admin, track_id)
            .await
            .unwrap();
        assert!(matches!(
            music_services.catalog.get_track(admin.id, track_id).await,
            Err(DomainError::NotFound(_))
        ));
        let pl_after = music_services
            .collection
            .get_playlist(&admin, pl.playlist.id)
            .await
            .unwrap();
        assert!(pl_after.tracks.is_empty());
        assert!(
            music_services
                .collection
                .get_starred(admin.id, None)
                .await
                .unwrap()
                .tracks
                .is_empty()
        );
    }

    #[tokio::test]
    async fn subsonic_album_list_by_year_supports_descending_from_greater_than_to() {
        use crate::domain::catalog::{Album, AlbumId, AlbumListQuery, AlbumListType, ArtistId};
        use crate::domain::repository::MusicCatalogRepository;
        use crate::infra::persistence::repositories::ToastyMusicCatalogRepository;
        use xmedia_core::domain::library::LibraryId;

        let tmp = TempDir::new().unwrap();
        let db_path = tmp.path().join("by_year.db");
        let db_url = format!("sqlite:{}", db_path.display());

        let mut models = ModelSet::new();
        register_core_models(&mut models);
        register_music_models(&mut models);
        let mut db = Db::builder().models(models).connect(&db_url).await.unwrap();
        ensure_core_schema(&mut db, &db_url).await.unwrap();
        ensure_music_schema(&mut db, &db_url).await.unwrap();

        let catalog_repo = Arc::new(ToastyMusicCatalogRepository::new(db));
        let lib_id = LibraryId::new();
        let artist_id = ArtistId::new();
        let now = jiff::Timestamp::now();

        for (title, year) in [
            ("Old Album", 2005),
            ("Mid Album", 2015),
            ("New Album", 2023),
        ] {
            let album = Album {
                id: AlbumId::new(),
                library_id: lib_id,
                album_key: format!("key-{year}"),
                title: title.into(),
                album_artist_id: artist_id,
                album_artist_name: "Artist".into(),
                year: Some(year),
                genre: None,
                disc_count: 1,
                track_count: 1,
                duration_secs: 180,
                is_compilation: false,
                cover_key: None,
                created_at: now,
                updated_at: now,
            };
            catalog_repo.save_album(&album).await.unwrap();
        }

        // Subsonic spec: fromYear > toYear returns albums in [toYear, fromYear] in descending year order
        let desc = catalog_repo
            .list_albums(
                &AlbumListQuery {
                    list_type: AlbumListType::ByYear,
                    from_year: Some(2024),
                    to_year: Some(2010),
                    size: 10,
                    ..Default::default()
                },
                None,
                None,
            )
            .await
            .unwrap();
        let desc_years: Vec<Option<i32>> = desc.items.iter().map(|a| a.year).collect();
        assert_eq!(desc_years, vec![Some(2023), Some(2015)]);

        let asc = catalog_repo
            .list_albums(
                &AlbumListQuery {
                    list_type: AlbumListType::ByYear,
                    from_year: Some(2010),
                    to_year: Some(2024),
                    size: 10,
                    ..Default::default()
                },
                None,
                None,
            )
            .await
            .unwrap();
        let asc_years: Vec<Option<i32>> = asc.items.iter().map(|a| a.year).collect();
        assert_eq!(asc_years, vec![Some(2015), Some(2023)]);
    }

    struct TestEnv {
        tmp: TempDir,
        db: Db,
        auth_svc: Arc<AuthService>,
        library_svc: Arc<LibraryService>,
        music_services: crate::application::MusicServices,
        events: Arc<BroadcastEventHub>,
    }

    async fn setup_test_env(db_name: &str) -> TestEnv {
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

    #[tokio::test]
    async fn upload_files_publishes_media_changed_event() {
        use xmedia_core::application::ports::SystemEvent;

        use crate::application::dto::UploadFilePathItem;

        let env = setup_test_env("upload_event.db").await;
        let session = env
            .auth_svc
            .setup_initial_admin(SetupAdminInput {
                username: "admin".into(),
                display_name: None,
                password: "password123".into(),
            })
            .await
            .unwrap();
        let admin = env.auth_svc.authenticate_jwt(&session.token).await.unwrap();
        let member_dto = env
            .auth_svc
            .create_user(
                &admin,
                CreateUserInput {
                    username: "member".into(),
                    display_name: None,
                    password: "password123".into(),
                    role: UserRole::Member,
                },
            )
            .await
            .unwrap();
        let member = env
            .auth_svc
            .authenticate_api_key(&member_dto.api_key)
            .await
            .unwrap();

        let mut rx = env.events.subscribe();

        // Non-admin upload must be rejected without emitting any event
        assert!(matches!(
            env.music_services
                .ingest
                .upload_files(
                    &member,
                    vec![UploadFileItem {
                        filename: "Blocked.wav".into(),
                        bytes: Bytes::from(make_wav(8_000)),
                    }],
                )
                .await,
            Err(DomainError::Forbidden(_))
        ));
        assert!(rx.try_recv().is_err());

        // upload_files emits MediaChanged(Music)
        env.music_services
            .ingest
            .upload_files(
                &admin,
                vec![UploadFileItem {
                    filename: "SongA.wav".into(),
                    bytes: Bytes::from(make_wav(8_000)),
                }],
            )
            .await
            .unwrap();
        assert!(matches!(
            rx.try_recv(),
            Ok(SystemEvent::MediaChanged {
                media_kind: MediaKind::Music
            })
        ));

        // upload_files_from_paths also emits MediaChanged(Music)
        let temp_wav = env.tmp.path().join("SongB.wav");
        std::fs::write(&temp_wav, make_wav(8_000)).unwrap();
        env.music_services
            .ingest
            .upload_files_from_paths(
                &admin,
                vec![UploadFilePathItem {
                    filename: "SongB.wav".into(),
                    temp_path: temp_wav,
                }],
            )
            .await
            .unwrap();
        assert!(matches!(
            rx.try_recv(),
            Ok(SystemEvent::MediaChanged {
                media_kind: MediaKind::Music
            })
        ));
    }

    #[tokio::test]
    async fn delete_library_cascades_all_collection_references() {
        use xmedia_core::application::ports::SystemEvent;

        use crate::domain::catalog::{
            Album, AlbumId, Artist, ArtistId, AudioFormat, ReplayGain, Track, TrackId, TrackSource,
        };
        use crate::domain::repository::{MusicCatalogRepository, MusicCollectionRepository};
        use crate::infra::persistence::repositories::{
            ToastyMusicCatalogRepository, ToastyMusicCollectionRepository,
        };

        let env = setup_test_env("delete_cascade.db").await;
        let session = env
            .auth_svc
            .setup_initial_admin(SetupAdminInput {
                username: "admin".into(),
                display_name: None,
                password: "password123".into(),
            })
            .await
            .unwrap();
        let admin = env.auth_svc.authenticate_jwt(&session.token).await.unwrap();

        let managed_lib = env
            .library_svc
            .ensure_managed_library(MediaKind::Music)
            .await
            .unwrap();
        let mount_dir = env.tmp.path().join("mount_cascade");
        std::fs::create_dir_all(&mount_dir).unwrap();
        let mounted_lib = env
            .library_svc
            .create_local_mount(
                &admin,
                CreateLibraryInput {
                    name: "ToDelete".into(),
                    media_kind: MediaKind::Music,
                    root_path: mount_dir.to_string_lossy().into_owned(),
                },
            )
            .await
            .unwrap();

        let catalog_repo = ToastyMusicCatalogRepository::new(env.db.clone());
        let collections_repo = ToastyMusicCollectionRepository::new(env.db.clone());
        let now = jiff::Timestamp::now();

        let shared_artist = Artist {
            id: ArtistId::new(),
            name: "Shared Artist".into(),
            is_album_artist: true,
            is_track_artist: true,
            album_count: 1,
            track_count: 2,
            cover_key: None,
            created_at: now,
            updated_at: now,
        };
        let exclusive_artist = Artist {
            id: ArtistId::new(),
            name: "Exclusive Artist".into(),
            is_album_artist: true,
            is_track_artist: true,
            album_count: 1,
            track_count: 1,
            cover_key: None,
            created_at: now,
            updated_at: now,
        };
        catalog_repo.save_artist(&shared_artist).await.unwrap();
        catalog_repo.save_artist(&exclusive_artist).await.unwrap();

        let keep_album = Album {
            id: AlbumId::new(),
            library_id: managed_lib.id,
            album_key: Album::compute_key(managed_lib.id, "Keep Album", &shared_artist.name, false),
            title: "Keep Album".into(),
            album_artist_id: shared_artist.id,
            album_artist_name: shared_artist.name.clone(),
            year: Some(2024),
            genre: Some("Jazz".into()),
            disc_count: 1,
            track_count: 1,
            duration_secs: 180,
            is_compilation: false,
            cover_key: None,
            created_at: now,
            updated_at: now,
        };
        let del_album = Album {
            id: AlbumId::new(),
            library_id: mounted_lib.id,
            album_key: Album::compute_key(
                mounted_lib.id,
                "Delete Album",
                &exclusive_artist.name,
                false,
            ),
            title: "Delete Album".into(),
            album_artist_id: exclusive_artist.id,
            album_artist_name: exclusive_artist.name.clone(),
            year: Some(2023),
            genre: Some("Rock".into()),
            disc_count: 1,
            track_count: 1,
            duration_secs: 200,
            is_compilation: false,
            cover_key: None,
            created_at: now,
            updated_at: now,
        };
        catalog_repo.save_album(&keep_album).await.unwrap();
        catalog_repo.save_album(&del_album).await.unwrap();

        let keep_track = Track {
            id: TrackId::new(),
            library_id: managed_lib.id,
            album_id: keep_album.id,
            artist_id: shared_artist.id,
            album_artist_id: shared_artist.id,
            title: "Keep Track".into(),
            artist_name: shared_artist.name.clone(),
            album_artist_name: shared_artist.name.clone(),
            album_title: keep_album.title.clone(),
            track_number: Some(1),
            disc_number: Some(1),
            year: Some(2024),
            genre: Some("Jazz".into()),
            duration_secs: 180,
            bit_rate_kbps: Some(320),
            sample_rate_hz: Some(44_100),
            bit_depth: None,
            channels: Some(2),
            format: AudioFormat::Mp3,
            file_size: 1024,
            file_hash: "hash-keep".into(),
            replay_gain: ReplayGain::default(),
            has_embedded_lyrics: false,
            has_sidecar_lyrics: false,
            is_compilation: false,
            cover_key: None,
            source: TrackSource::Managed {
                storage_key: "keep.mp3".into(),
                sidecar_lrc_key: None,
            },
            is_missing: false,
            created_at: now,
            updated_at: now,
        };
        let del_track = Track {
            id: TrackId::new(),
            library_id: mounted_lib.id,
            album_id: del_album.id,
            artist_id: shared_artist.id,
            album_artist_id: exclusive_artist.id,
            title: "Delete Track".into(),
            artist_name: shared_artist.name.clone(),
            album_artist_name: exclusive_artist.name.clone(),
            album_title: del_album.title.clone(),
            track_number: Some(1),
            disc_number: Some(1),
            year: Some(2023),
            genre: Some("Rock".into()),
            duration_secs: 200,
            bit_rate_kbps: Some(320),
            sample_rate_hz: Some(44_100),
            bit_depth: None,
            channels: Some(2),
            format: AudioFormat::Flac,
            file_size: 2048,
            file_hash: "hash-del".into(),
            replay_gain: ReplayGain::default(),
            has_embedded_lyrics: false,
            has_sidecar_lyrics: false,
            is_compilation: false,
            cover_key: None,
            source: TrackSource::LocalPath {
                relative_path: "del.flac".into(),
                file_mtime: 1,
                sidecar_lrc_path: None,
            },
            is_missing: false,
            created_at: now,
            updated_at: now,
        };
        catalog_repo.save_track(&keep_track).await.unwrap();
        catalog_repo.save_track(&del_track).await.unwrap();
        catalog_repo.recompute_catalog_aggregates().await.unwrap();

        // Populate stars, ratings, playlist, bookmarks, play queue, scrobbles/stats, and tags
        env.music_services
            .collection
            .star_items(
                admin.id,
                &[del_track.id.to_string(), keep_track.id.to_string()],
                &[del_album.id, keep_album.id],
                &[exclusive_artist.id, shared_artist.id],
            )
            .await
            .unwrap();
        for target in [
            del_track.id.to_string(),
            keep_track.id.to_string(),
            del_album.id.to_string(),
            keep_album.id.to_string(),
            exclusive_artist.id.to_string(),
            shared_artist.id.to_string(),
        ] {
            env.music_services
                .collection
                .set_rating(admin.id, &target, 5)
                .await
                .unwrap();
        }
        let playlist = env
            .music_services
            .collection
            .create_playlist(
                &admin,
                "Mixed Playlist",
                None,
                true,
                vec![del_track.id, keep_track.id],
            )
            .await
            .unwrap();
        env.music_services
            .collection
            .upsert_bookmark(admin.id, del_track.id, 12_000, Some("del bm".into()))
            .await
            .unwrap();
        env.music_services
            .collection
            .upsert_bookmark(admin.id, keep_track.id, 24_000, Some("keep bm".into()))
            .await
            .unwrap();
        env.music_services
            .collection
            .save_play_queue(
                admin.id,
                vec![del_track.id, keep_track.id],
                Some(del_track.id),
                42_000,
                "unit-test",
            )
            .await
            .unwrap();
        env.music_services
            .collection
            .scrobble(
                &admin,
                vec![(del_track.id, None), (keep_track.id, None)],
                true,
                "unit-test",
            )
            .await
            .unwrap();
        env.music_services
            .catalog
            .set_track_tags(&admin, del_track.id, vec!["ephemeral".into()])
            .await
            .unwrap();

        let mut rx = env.events.subscribe();
        env.library_svc
            .delete_library(&admin, mounted_lib.id)
            .await
            .unwrap();

        // MediaChanged event emitted
        assert!(matches!(
            rx.try_recv(),
            Ok(SystemEvent::MediaChanged {
                media_kind: MediaKind::Music
            })
        ));

        // Catalog entities deleted (shared artist survives)
        assert!(
            catalog_repo
                .find_track_by_id(del_track.id)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            catalog_repo
                .find_album_by_id(del_album.id)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            catalog_repo
                .find_artist_by_id(exclusive_artist.id)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            catalog_repo
                .find_artist_by_id(shared_artist.id)
                .await
                .unwrap()
                .is_some()
        );

        // Stars cleaned for deleted track/album/exclusive artist; surviving stars intact
        let stars = collections_repo.list_starred(admin.id, None).await.unwrap();
        let starred_ids: Vec<_> = stars.iter().map(|s| s.target_id).collect();
        assert!(!starred_ids.contains(&del_track.id.0));
        assert!(!starred_ids.contains(&del_album.id.0));
        assert!(!starred_ids.contains(&exclusive_artist.id.0));
        assert!(starred_ids.contains(&keep_track.id.0));
        assert!(starred_ids.contains(&keep_album.id.0));
        assert!(starred_ids.contains(&shared_artist.id.0));

        // Ratings cleaned for deleted track/album/exclusive artist; surviving ratings intact
        let ratings = collections_repo.list_user_ratings(admin.id).await.unwrap();
        let rated_ids: Vec<_> = ratings.iter().map(|r| r.target_id).collect();
        assert!(!rated_ids.contains(&del_track.id.0));
        assert!(!rated_ids.contains(&del_album.id.0));
        assert!(!rated_ids.contains(&exclusive_artist.id.0));
        assert!(rated_ids.contains(&keep_track.id.0));
        assert!(rated_ids.contains(&keep_album.id.0));
        assert!(rated_ids.contains(&shared_artist.id.0));

        // Playlist tracks stripped of deleted track, surviving track retained
        let pl_after = collections_repo
            .find_playlist_by_id(playlist.playlist.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(pl_after.track_ids, vec![keep_track.id]);

        // Bookmarks cleaned
        let bms = collections_repo.list_bookmarks(admin.id).await.unwrap();
        assert_eq!(bms.len(), 1);
        assert_eq!(bms[0].track_id, keep_track.id);

        // Play queue cleaned and current track advanced to surviving track with reset position
        let queue = collections_repo
            .get_play_queue(admin.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(queue.track_ids, vec![keep_track.id]);
        assert_eq!(queue.current_track_id, Some(keep_track.id));
        assert_eq!(queue.position_ms, 0);

        // Play stats and scrobbles cleaned
        let stats = collections_repo
            .list_user_play_stats(admin.id)
            .await
            .unwrap();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].track_id, keep_track.id);
        let scrobbles = collections_repo
            .list_recent_scrobbles(admin.id, 20)
            .await
            .unwrap();
        assert_eq!(scrobbles.len(), 1);
        assert_eq!(scrobbles[0].track_id, keep_track.id);

        // Tags cleaned
        assert!(catalog_repo.list_tags().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn playlist_visibility_rules_for_public_and_private_playlists() {
        use crate::application::dto::UpdatePlaylistInput;

        let env = setup_test_env("playlist_visibility.db").await;
        let admin_session = env
            .auth_svc
            .setup_initial_admin(SetupAdminInput {
                username: "admin".into(),
                display_name: None,
                password: "password123".into(),
            })
            .await
            .unwrap();
        let admin = env
            .auth_svc
            .authenticate_jwt(&admin_session.token)
            .await
            .unwrap();
        let alice_dto = env
            .auth_svc
            .create_user(
                &admin,
                CreateUserInput {
                    username: "alice".into(),
                    display_name: None,
                    password: "password123".into(),
                    role: UserRole::Member,
                },
            )
            .await
            .unwrap();
        let bob_dto = env
            .auth_svc
            .create_user(
                &admin,
                CreateUserInput {
                    username: "bob".into(),
                    display_name: None,
                    password: "password123".into(),
                    role: UserRole::Member,
                },
            )
            .await
            .unwrap();
        let alice = env
            .auth_svc
            .authenticate_api_key(&alice_dto.api_key)
            .await
            .unwrap();
        let bob = env
            .auth_svc
            .authenticate_api_key(&bob_dto.api_key)
            .await
            .unwrap();

        let alice_private = env
            .music_services
            .collection
            .create_playlist(&alice, "Alice Private", None, false, vec![])
            .await
            .unwrap();
        let alice_public = env
            .music_services
            .collection
            .create_playlist(&alice, "Alice Public", None, true, vec![])
            .await
            .unwrap();
        let bob_private = env
            .music_services
            .collection
            .create_playlist(&bob, "Bob Private", None, false, vec![])
            .await
            .unwrap();

        // Alice sees her own private + public playlists, not Bob's private
        let alice_list: Vec<_> = env
            .music_services
            .collection
            .list_playlists(alice.id)
            .await
            .unwrap()
            .into_iter()
            .map(|p| p.id)
            .collect();
        assert_eq!(alice_list.len(), 2);
        assert!(alice_list.contains(&alice_private.playlist.id));
        assert!(alice_list.contains(&alice_public.playlist.id));

        // Bob sees his own private playlist + Alice's public playlist, not Alice's private
        let bob_list: Vec<_> = env
            .music_services
            .collection
            .list_playlists(bob.id)
            .await
            .unwrap()
            .into_iter()
            .map(|p| p.id)
            .collect();
        assert_eq!(bob_list.len(), 2);
        assert!(bob_list.contains(&bob_private.playlist.id));
        assert!(bob_list.contains(&alice_public.playlist.id));
        assert!(!bob_list.contains(&alice_private.playlist.id));

        // Home overview playlist count aligns with visible playlists
        let bob_overview = env
            .music_services
            .catalog
            .get_home_overview(bob.id)
            .await
            .unwrap();
        assert_eq!(bob_overview.stats.playlists_count, 2);

        // Reading: Bob can read Alice's public playlist, cannot read Alice's private playlist
        assert!(
            env.music_services
                .collection
                .get_playlist(&bob, alice_public.playlist.id)
                .await
                .is_ok()
        );
        assert!(matches!(
            env.music_services
                .collection
                .get_playlist(&bob, alice_private.playlist.id)
                .await,
            Err(DomainError::Forbidden(_))
        ));
        // Admin can read private playlist
        assert!(
            env.music_services
                .collection
                .get_playlist(&admin, alice_private.playlist.id)
                .await
                .is_ok()
        );

        // Mutation & deletion: only owner or admin allowed, even for public playlists
        assert!(matches!(
            env.music_services
                .collection
                .update_playlist(
                    &bob,
                    alice_public.playlist.id,
                    UpdatePlaylistInput {
                        name: Some("Hijacked".into()),
                        ..Default::default()
                    },
                )
                .await,
            Err(DomainError::Forbidden(_))
        ));
        assert!(matches!(
            env.music_services
                .collection
                .delete_playlist(&bob, alice_public.playlist.id)
                .await,
            Err(DomainError::Forbidden(_))
        ));

        // Owner toggles private -> public, making it visible and readable to Bob
        env.music_services
            .collection
            .update_playlist(
                &alice,
                alice_private.playlist.id,
                UpdatePlaylistInput {
                    is_public: Some(true),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let bob_list_after = env
            .music_services
            .collection
            .list_playlists(bob.id)
            .await
            .unwrap();
        assert_eq!(bob_list_after.len(), 3);
        assert!(
            env.music_services
                .collection
                .get_playlist(&bob, alice_private.playlist.id)
                .await
                .is_ok()
        );

        // Admin can modify and delete another user's playlist
        env.music_services
            .collection
            .update_playlist(
                &admin,
                alice_public.playlist.id,
                UpdatePlaylistInput {
                    name: Some("Moderated".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        env.music_services
            .collection
            .delete_playlist(&admin, alice_public.playlist.id)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn rest_status_response_preserves_frontend_ffmpeg_fields() {
        let env = setup_test_env("status_compat.db").await;
        let session = env
            .auth_svc
            .setup_initial_admin(SetupAdminInput {
                username: "admin".into(),
                display_name: None,
                password: "password123".into(),
            })
            .await
            .unwrap();

        let state = MusicHttpState::new(
            env.auth_svc.clone(),
            env.library_svc.clone(),
            env.music_services.clone(),
        );
        let app = Router::new().nest("/api/v1/music", rest_router(state));

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/music/status")
                    .header(header::AUTHORIZATION, format!("Bearer {}", session.token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json: serde_json::Value =
            serde_json::from_slice(&to_bytes(resp.into_body(), usize::MAX).await.unwrap()).unwrap();
        let ffmpeg = &json["ffmpeg"];
        assert!(ffmpeg["available"].is_boolean());
        assert!(ffmpeg["version"].is_null() || ffmpeg["version"].is_string());
        assert_eq!(ffmpeg["available"], ffmpeg["ffmpeg_available"]);
        assert_eq!(ffmpeg["version"], ffmpeg["ffmpeg_version"]);
        assert!(ffmpeg["ffprobe_available"].is_boolean());
        assert_eq!(json["open_subsonic"], true);
    }
}

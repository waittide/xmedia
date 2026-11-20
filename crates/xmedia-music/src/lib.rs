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
}

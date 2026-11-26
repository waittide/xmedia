use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
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
use xmedia_core::domain::library::{LibraryId, MediaKind, ScanStatus};
use xmedia_core::domain::user::UserRole;
use xmedia_core::infra::events::BroadcastEventHub;
use xmedia_core::infra::ffmpeg::FfmpegToolkit;
use xmedia_core::infra::persistence::repositories::{
    ToastyLibraryRepository, ToastyUserRepository,
};
use xmedia_core::infra::security::{JwtTokenService, SecurityManager};
use xmedia_core::{ensure_core_schema, register_core_models};

use super::{make_wav, setup_test_env};
use crate::application::dto::{BindMusicItemsInput, UploadFilePathItem};
use crate::domain::catalog::{
    Album, AlbumId, AlbumListQuery, AlbumListType, ArtistId, AudioFormat, ReplayGain, Track,
    TrackId, TrackSource,
};
use crate::domain::repository::{CatalogMutation, MusicCatalogRepository};
use crate::http::state::MusicHttpState;
use crate::http::{rest_router, subsonic_router};
use crate::infra::persistence::repositories::ToastyMusicCatalogRepository;
use crate::{ensure_music_schema, init_music_module, register_music_models};

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
    let admin = auth_svc
        .authenticate(
            xmedia_core::application::dto::AuthCredential::Jwt(&session.token),
            None,
        )
        .await
        .unwrap();
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
    let (gbk_lrc, _, _) = GB18030.encode("[ti:夜曲]\n[ar:周杰伦]\n[00:01.50]为你弹奏肖邦的夜曲\n");
    let nocturne_wav = tmp.path().join("Nocturne.wav");
    let nocturne_lrc = tmp.path().join("Nocturne.lrc");
    std::fs::write(&nocturne_wav, make_wav(16_000)).unwrap();
    std::fs::write(&nocturne_lrc, &gbk_lrc[..]).unwrap();
    let uploaded = music_services
        .ingest
        .upload_files_from_paths(
            &admin,
            vec![
                UploadFilePathItem {
                    filename: "Nocturne.wav".into(),
                    temp_path: nocturne_wav,
                },
                UploadFilePathItem {
                    filename: "Nocturne.lrc".into(),
                    temp_path: nocturne_lrc,
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
        serde_json::from_slice(&to_bytes(ext_res.into_body(), usize::MAX).await.unwrap()).unwrap();
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
        serde_json::from_slice(&to_bytes(lyr_res.into_body(), usize::MAX).await.unwrap()).unwrap();
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
    let orphan_lrc = tmp.path().join("orphan.lrc");
    std::fs::write(&orphan_lrc, b"[00:01.00]orphan line\n").unwrap();
    let orphan_res = music_services
        .ingest
        .upload_files_from_paths(
            &admin,
            vec![UploadFilePathItem {
                filename: "orphan.lrc".into(),
                temp_path: orphan_lrc,
            }],
        )
        .await;
    assert!(matches!(orphan_res, Err(DomainError::Validation(_))));

    let bob_session = auth_svc.login("bob", "bobpassword", None).await.unwrap();
    let bob_user = auth_svc
        .authenticate(
            xmedia_core::application::dto::AuthCredential::Jwt(&bob_session.token),
            None,
        )
        .await
        .unwrap();
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
        .streaming
        .get_lyrics_by_artist_title(admin.id, Some(&artist_name), None)
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
        music_services.tracks.get_track(admin.id, track_id).await,
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
    let env = setup_test_env("by_year.db").await;
    let session = env
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
        .authenticate(
            xmedia_core::application::dto::AuthCredential::Jwt(&session.token),
            None,
        )
        .await
        .unwrap();

    let catalog_repo = Arc::new(ToastyMusicCatalogRepository::new(env.db.clone()));
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
        catalog_repo
            .save(CatalogMutation::Album(&album))
            .await
            .unwrap();

        let track = Track {
            id: TrackId::new(),
            library_id: lib_id,
            album_id: album.id,
            artist_id,
            album_artist_id: artist_id,
            title: format!("{title} Track"),
            artist_name: "Artist".into(),
            album_artist_name: "Artist".into(),
            album_title: title.into(),
            track_number: Some(1),
            disc_number: Some(1),
            year: Some(year),
            genre: None,
            duration_secs: 180,
            bit_rate_kbps: None,
            sample_rate_hz: None,
            bit_depth: None,
            channels: None,
            format: AudioFormat::Mp3,
            file_size: 1024,
            file_hash: format!("hash-{year}"),
            replay_gain: ReplayGain::default(),
            has_embedded_lyrics: false,
            has_sidecar_lyrics: false,
            is_compilation: false,
            cover_key: None,
            source: TrackSource::Managed {
                storage_key: format!("{year}.mp3"),
                sidecar_lrc_key: None,
            },
            is_missing: false,
            created_at: now,
            updated_at: now,
        };
        catalog_repo
            .save(CatalogMutation::Track(&track))
            .await
            .unwrap();
        env.music_services
            .ingest
            .bind_tracks(
                &admin,
                BindMusicItemsInput {
                    item_ids: vec![track.id.0],
                    ..Default::default()
                },
            )
            .await
            .unwrap();
    }

    // Subsonic spec: fromYear > toYear returns albums in [toYear, fromYear] in descending year order
    let desc = env
        .music_services
        .browse
        .list_albums(
            admin.id,
            AlbumListQuery {
                list_type: AlbumListType::ByYear,
                from_year: Some(2024),
                to_year: Some(2010),
                size: 10,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let desc_years: Vec<Option<i32>> = desc.items.iter().map(|a| a.year).collect();
    assert_eq!(desc_years, vec![Some(2023), Some(2015)]);

    let asc = env
        .music_services
        .browse
        .list_albums(
            admin.id,
            AlbumListQuery {
                list_type: AlbumListType::ByYear,
                from_year: Some(2010),
                to_year: Some(2024),
                size: 10,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let asc_years: Vec<Option<i32>> = asc.items.iter().map(|a| a.year).collect();
    assert_eq!(asc_years, vec![Some(2015), Some(2023)]);
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

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use tempfile::TempDir;
use toasty::Db;
use toasty::schema::ModelSet;
use tower::ServiceExt;
use xmedia_core::application::auth::AuthService;
use xmedia_core::application::bindings::BindMediaItemsInput;
use xmedia_core::application::dto::{CreateLibraryInput, CreateUserInput, SetupAdminInput};
use xmedia_core::application::library::LibraryService;
use xmedia_core::application::ports::SystemEvent;
use xmedia_core::domain::library::{MediaKind, ScanStatus};
use xmedia_core::domain::user::UserRole;
use xmedia_core::infra::events::BroadcastEventHub;
use xmedia_core::infra::ffmpeg::FfmpegToolkit;
use xmedia_core::infra::persistence::repositories::{
    ToastyLibraryRepository, ToastyUserRepository,
};
use xmedia_core::infra::security::{JwtTokenService, SecurityManager};
use xmedia_core::{ensure_core_schema, register_core_models};

use super::{fake_mp4_bytes, stage_upload_files};
use crate::application::dto::{SaveWatchProgressInput, UpsertVideoShelfInput};
use crate::domain::repository::{ProgressScope, VideoShelfScope, VideoWatchRepository};
use crate::domain::video::ShowMetadataPatch;
use crate::domain::watch::VideoFavoriteKind;
use crate::http::state::VideoHttpState;
use crate::http::{jellyfin_router, rest_router};
use crate::infra::persistence::repositories::ToastyVideoWatchRepository;
use crate::{ensure_video_schema, init_video_module, register_video_models};

#[tokio::test]
async fn delete_video_emits_media_changed_and_subtitles_list_endpoint_works() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("xmedia_video_ev.db");
    let db_url = format!("sqlite:{}", db_path.display());

    let mut models = ModelSet::new();
    register_core_models(&mut models);
    register_video_models(&mut models);
    let mut db = Db::builder().models(models).connect(&db_url).await.unwrap();
    ensure_core_schema(&mut db, &db_url).await.unwrap();
    ensure_video_schema(&mut db, &db_url).await.unwrap();

    let users_repo = Arc::new(ToastyUserRepository::new(db.clone()));
    let libraries_repo = Arc::new(ToastyLibraryRepository::new(db.clone()));
    let lifecycles = Arc::new(RwLock::new(HashMap::new()));
    let events = Arc::new(BroadcastEventHub::default());
    let security = Arc::new(SecurityManager::new("video-test-secret"));
    let tokens = Arc::new(JwtTokenService::new("video-test-secret", 3600));

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
    let video_services = init_video_module(
        db,
        tmp.path(),
        FfmpegToolkit::new("__missing_ffmpeg__", "__missing_ffprobe__"),
        libraries_repo,
        &library_svc,
        events.clone(),
    )
    .unwrap();

    let session = auth_svc
        .setup_initial_admin(SetupAdminInput {
            username: "admin".into(),
            display_name: None,
            password: "password123".into(),
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

    let state = VideoHttpState::new(auth_svc, library_svc, video_services.clone());
    let app = Router::new().nest("/api/v1/video", rest_router(state));

    let dune_mp4 = fake_mp4_bytes(256);
    let uploaded = video_services
        .ingest
        .upload_files_from_paths(
            &admin,
            stage_upload_files(
                tmp.path(),
                &[
                    ("Dune (2021).mp4", &dune_mp4),
                    (
                        "Dune (2021).en.srt",
                        b"1\n00:00:01,000 --> 00:00:03,000\nArrakis.\n",
                    ),
                ],
            ),
        )
        .await
        .unwrap();
    let movie = &uploaded.uploaded_videos[0];

    // Verify lightweight subtitles endpoint returns expected subtitle track
    let sub_list_res = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/v1/video/videos/{}/subtitles?token={}",
                    movie.id, session.token
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(sub_list_res.status(), StatusCode::OK);
    let sub_json: serde_json::Value = serde_json::from_slice(
        &to_bytes(sub_list_res.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    let tracks = sub_json.as_array().unwrap();
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0]["id"], "ext-s0");
    assert_eq!(tracks[0]["language"], "en");

    // Subscribe before deleting video and verify MediaChanged(Video) is published
    let mut rx = events.subscribe();
    video_services
        .ingest
        .delete_video(&admin, movie.id)
        .await
        .unwrap();

    let mut saw_video_changed = false;
    while let Ok(ev) = rx.try_recv() {
        if matches!(
            ev,
            SystemEvent::MediaChanged {
                media_kind: MediaKind::Video
            }
        ) {
            saw_video_changed = true;
        }
    }
    assert!(
        saw_video_changed,
        "delete_video should emit SystemEvent::MediaChanged for MediaKind::Video"
    );
}

#[tokio::test]
async fn delete_library_cascades_progress_favorites_and_shelf_items() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("xmedia_video_cascade.db");
    let db_url = format!("sqlite:{}", db_path.display());

    let mut models = ModelSet::new();
    register_core_models(&mut models);
    register_video_models(&mut models);
    let mut db = Db::builder().models(models).connect(&db_url).await.unwrap();
    ensure_core_schema(&mut db, &db_url).await.unwrap();
    ensure_video_schema(&mut db, &db_url).await.unwrap();

    let users_repo = Arc::new(ToastyUserRepository::new(db.clone()));
    let libraries_repo = Arc::new(ToastyLibraryRepository::new(db.clone()));
    let lifecycles = Arc::new(RwLock::new(HashMap::new()));
    let events = Arc::new(BroadcastEventHub::default());
    let security = Arc::new(SecurityManager::new("video-test-secret"));
    let tokens = Arc::new(JwtTokenService::new("video-test-secret", 3600));

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
    let video_services = init_video_module(
        db.clone(),
        tmp.path(),
        FfmpegToolkit::new("__missing_ffmpeg__", "__missing_ffprobe__"),
        libraries_repo,
        &library_svc,
        events,
    )
    .unwrap();

    let session = auth_svc
        .setup_initial_admin(SetupAdminInput {
            username: "admin".into(),
            display_name: None,
            password: "password123".into(),
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

    // Upload a managed movie (in another library) that must survive cascade deletion
    let arrival_mp4 = fake_mp4_bytes(256);
    let managed = video_services
        .ingest
        .upload_files_from_paths(
            &admin,
            stage_upload_files(tmp.path(), &[("Arrival (2016).mp4", &arrival_mp4)]),
        )
        .await
        .unwrap();
    let survivor = &managed.uploaded_videos[0];

    // Mount and scan a local library with a TV show
    let show_dir = tmp.path().join("mount_a").join("Andor").join("Season 1");
    std::fs::create_dir_all(&show_dir).unwrap();
    std::fs::write(show_dir.join("01 - Kassa.mp4"), fake_mp4_bytes(256)).unwrap();
    std::fs::write(
        show_dir.join("02 - That Would Be Me.mp4"),
        fake_mp4_bytes(256),
    )
    .unwrap();

    let mount_lib = library_svc
        .create_local_mount(
            &admin,
            CreateLibraryInput {
                name: "Mount A".into(),
                media_kind: MediaKind::Video,
                root_path: tmp.path().join("mount_a").to_string_lossy().into_owned(),
            },
        )
        .await
        .unwrap();
    library_svc
        .trigger_scan(&admin, mount_lib.id)
        .await
        .unwrap();
    for _ in 0..50 {
        let st = library_svc.get_library(mount_lib.id).await.unwrap();
        if matches!(st.scan_status, ScanStatus::Completed { .. }) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }

    video_services
        .ingest
        .bind_items(
            &admin,
            BindMediaItemsInput {
                item_ids: Vec::new(),
                library_id: Some(mount_lib.id.0),
                user_id: None,
            },
        )
        .await
        .unwrap();

    let shows = video_services
        .shows
        .list_shows(&admin, Default::default())
        .await
        .unwrap();
    assert_eq!(shows.len(), 1);
    let show_id = shows[0].id;
    let show_detail = video_services
        .shows
        .get_show_detail(&admin, show_id)
        .await
        .unwrap();
    let ep1 = &show_detail.seasons[0].episodes[0];
    let ep2 = &show_detail.seasons[0].episodes[1];

    // Create progress, favorites (video + show), and a shelf spanning both libraries
    video_services
        .watch
        .save_progress(
            &admin,
            survivor.id,
            SaveWatchProgressInput {
                position_secs: 100.0,
                duration_secs: Some(600),
                completed: None,
                selected_audio_stream: None,
                selected_subtitle: None,
            },
        )
        .await
        .unwrap();
    video_services
        .watch
        .save_progress(
            &admin,
            ep1.id,
            SaveWatchProgressInput {
                position_secs: 200.0,
                duration_secs: Some(1200),
                completed: None,
                selected_audio_stream: None,
                selected_subtitle: None,
            },
        )
        .await
        .unwrap();

    video_services
        .watch
        .set_favorite(&admin, VideoFavoriteKind::Video, survivor.id.0, true)
        .await
        .unwrap();
    video_services
        .watch
        .set_favorite(&admin, VideoFavoriteKind::Video, ep1.id.0, true)
        .await
        .unwrap();
    video_services
        .watch
        .set_favorite(&admin, VideoFavoriteKind::Show, show_id.0, true)
        .await
        .unwrap();

    let shelf = video_services
        .watch
        .create_shelf(
            &admin,
            UpsertVideoShelfInput {
                name: "Mixed Watchlist".into(),
                description: String::new(),
                video_ids: vec![survivor.id, ep1.id, ep2.id],
            },
        )
        .await
        .unwrap();
    assert_eq!(shelf.videos.len(), 3);

    // Delete the mounted library -> must cascade-clean progress, favorites, and shelf items
    library_svc
        .delete_library(&admin, mount_lib.id)
        .await
        .unwrap();

    assert!(
        video_services
            .shows
            .list_shows(&admin, Default::default())
            .await
            .unwrap()
            .is_empty()
    );

    let watch_repo = ToastyVideoWatchRepository::new(db);

    // Progress for deleted episode must be gone; survivor progress must remain
    assert!(
        watch_repo
            .query_progress(ProgressScope::One {
                user_id: admin.id,
                video_id: ep1.id,
            })
            .await
            .unwrap()
            .into_iter()
            .next()
            .is_none()
    );
    assert!(
        watch_repo
            .query_progress(ProgressScope::One {
                user_id: admin.id,
                video_id: survivor.id,
            })
            .await
            .unwrap()
            .into_iter()
            .next()
            .is_some()
    );

    // Favorites for deleted episode and deleted show must be removed from persistence
    let raw_favs = watch_repo.list_favorites(admin.id, None).await.unwrap();
    assert_eq!(raw_favs.len(), 1);
    assert_eq!(raw_favs[0].kind, VideoFavoriteKind::Video);
    assert_eq!(raw_favs[0].target_id, survivor.id.0);

    // Shelf must only retain the surviving managed movie in persistence
    let persisted_shelf = watch_repo
        .query_shelves(VideoShelfScope::Id(shelf.shelf.id))
        .await
        .unwrap()
        .into_iter()
        .next()
        .unwrap();
    assert_eq!(persisted_shelf.video_ids, vec![survivor.id]);
}

#[tokio::test]
async fn taxonomy_counts_show_genres_tags_and_jellyfin_enforces_user_scope_and_binding_visibility()
{
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("xmedia_video_jf_scope.db");
    let db_url = format!("sqlite:{}", db_path.display());

    let mut models = ModelSet::new();
    register_core_models(&mut models);
    register_video_models(&mut models);
    let mut db = Db::builder().models(models).connect(&db_url).await.unwrap();
    ensure_core_schema(&mut db, &db_url).await.unwrap();
    ensure_video_schema(&mut db, &db_url).await.unwrap();

    let users_repo = Arc::new(ToastyUserRepository::new(db.clone()));
    let libraries_repo = Arc::new(ToastyLibraryRepository::new(db.clone()));
    let lifecycles = Arc::new(RwLock::new(HashMap::new()));
    let events = Arc::new(BroadcastEventHub::default());
    let security = Arc::new(SecurityManager::new("video-test-secret"));
    let tokens = Arc::new(JwtTokenService::new("video-test-secret", 3600));

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
    let video_services = init_video_module(
        db,
        tmp.path(),
        FfmpegToolkit::new("__missing_ffmpeg__", "__missing_ffprobe__"),
        libraries_repo,
        &library_svc,
        events,
    )
    .unwrap();

    let admin_session = auth_svc
        .setup_initial_admin(SetupAdminInput {
            username: "admin".into(),
            display_name: None,
            password: "password123".into(),
        })
        .await
        .unwrap();
    let admin = auth_svc
        .authenticate(
            xmedia_core::application::dto::AuthCredential::Jwt(&admin_session.token),
            None,
        )
        .await
        .unwrap();

    let member_dto = auth_svc
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
    let member_session = auth_svc.login("member", "password123", None).await.unwrap();

    // Mount a library with 2 shows sharing genre "Fantasy" and tag "magic"
    let mount_root = tmp.path().join("shows_mount");
    let show1_dir = mount_root.join("Show Alpha").join("Season 1");
    let show2_dir = mount_root.join("Show Beta").join("Season 1");
    std::fs::create_dir_all(&show1_dir).unwrap();
    std::fs::create_dir_all(&show2_dir).unwrap();
    std::fs::write(show1_dir.join("01 - Pilot.mp4"), fake_mp4_bytes(256)).unwrap();
    std::fs::write(
        mount_root.join("Show Alpha").join("tvshow.nfo"),
        "<tvshow><title>Show Alpha</title><genre>Fantasy</genre><tag>magic</tag></tvshow>",
    )
    .unwrap();
    std::fs::write(show2_dir.join("01 - Start.mp4"), fake_mp4_bytes(256)).unwrap();
    std::fs::write(
        mount_root.join("Show Beta").join("tvshow.nfo"),
        "<tvshow><title>Show Beta</title><genre>Fantasy</genre><tag>magic</tag></tvshow>",
    )
    .unwrap();
    // Add a local poster image for Show Alpha
    let png_1x1: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];
    std::fs::write(mount_root.join("Show Alpha").join("poster.png"), png_1x1).unwrap();

    let lib = library_svc
        .create_local_mount(
            &admin,
            CreateLibraryInput {
                name: "Private Shows".into(),
                media_kind: MediaKind::Video,
                root_path: mount_root.to_string_lossy().into_owned(),
            },
        )
        .await
        .unwrap();
    library_svc.trigger_scan(&admin, lib.id).await.unwrap();
    for _ in 0..50 {
        let st = library_svc.get_library(lib.id).await.unwrap();
        if matches!(st.scan_status, ScanStatus::Completed { .. }) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }

    // Bind ONLY to admin, NOT to member
    video_services
        .ingest
        .bind_items(
            &admin,
            BindMediaItemsInput {
                item_ids: Vec::new(),
                library_id: Some(lib.id.0),
                user_id: None,
            },
        )
        .await
        .unwrap();

    // 1. Verify get_taxonomy increments show genre and tag counts across multiple shows
    let all_shows = video_services
        .shows
        .list_shows(&admin, Default::default())
        .await
        .unwrap();
    assert_eq!(all_shows.len(), 2);
    for s in &all_shows {
        video_services
            .shows
            .update_show_metadata(
                &admin,
                s.id,
                ShowMetadataPatch {
                    genres: Some(vec!["Drama".into()]),
                    tags: Some(vec!["featured".into()]),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
    }
    let tax = video_services.catalog.get_taxonomy(admin.id).await.unwrap();
    let drama = tax.genres.iter().find(|g| g.name == "Drama").unwrap();
    assert_eq!(drama.count, 2);
    let featured = tax.tags.iter().find(|t| t.name == "featured").unwrap();
    assert_eq!(featured.count, 2);

    // 2. Verify update_shelf accepts multi-byte CJK names up to 120 chars
    let shelf = video_services
        .watch
        .create_shelf(
            &admin,
            UpsertVideoShelfInput {
                name: "Init".into(),
                description: String::new(),
                video_ids: Vec::new(),
            },
        )
        .await
        .unwrap();
    let cjk_80 = "剧".repeat(80);
    let updated_shelf = video_services
        .watch
        .update_shelf(
            &admin,
            shelf.shelf.id,
            UpsertVideoShelfInput {
                name: cjk_80.clone(),
                description: "CJK".into(),
                video_ids: Vec::new(),
            },
        )
        .await
        .unwrap();
    assert_eq!(updated_shelf.shelf.name, cjk_80);

    let shows = video_services
        .shows
        .list_shows(&admin, Default::default())
        .await
        .unwrap();
    let alpha_show = shows.iter().find(|s| s.title == "Show Alpha").unwrap();

    let state = VideoHttpState::new(auth_svc, library_svc, video_services);
    let app = Router::new().nest("/jellyfin", jellyfin_router(state));

    // 3. Admin querying another user's Jellyfin {userId} routes must be rejected (403)
    let cross_views = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/jellyfin/Users/{}/Views", member_dto.id))
                .header("X-Emby-Token", &admin_session.token)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(cross_views.status(), StatusCode::FORBIDDEN);

    let cross_special = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/jellyfin/Users/{}/Items/{}/SpecialFeatures",
                    member_dto.id, alpha_show.id
                ))
                .header("X-Emby-Token", &admin_session.token)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(cross_special.status(), StatusCode::FORBIDDEN);

    // 4. Member with 0 bound items in `lib` cannot see library item detail or local show image
    let unbound_lib_detail = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/jellyfin/Items/{}", lib.id))
                .header("X-Emby-Token", &member_session.token)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unbound_lib_detail.status(), StatusCode::NOT_FOUND);

    let unbound_img = app
        .oneshot(
            Request::builder()
                .uri(format!("/jellyfin/Items/{}/Images/Primary", alpha_show.id))
                .header("X-Emby-Token", &member_session.token)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unbound_img.status(), StatusCode::NOT_FOUND);
}

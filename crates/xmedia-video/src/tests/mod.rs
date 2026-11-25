mod lifecycle_and_scope;
mod online_e2e;

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use tempfile::TempDir;
use toasty::Db;
use toasty::schema::ModelSet;
use tower::ServiceExt;
use xmedia_core::application::auth::AuthService;
use xmedia_core::application::bindings::BindMediaItemsInput;
use xmedia_core::application::dto::{CreateLibraryInput, SetupAdminInput};
use xmedia_core::application::library::LibraryService;
use xmedia_core::domain::library::{MediaKind, ScanStatus};
use xmedia_core::infra::events::BroadcastEventHub;
use xmedia_core::infra::ffmpeg::FfmpegToolkit;
use xmedia_core::infra::persistence::repositories::{
    ToastyLibraryRepository, ToastyUserRepository,
};
use xmedia_core::infra::security::{JwtTokenService, SecurityManager};
use xmedia_core::{ensure_core_schema, register_core_models};

use crate::application::dto::{SaveWatchProgressInput, UpsertVideoShelfInput, VideoUploadPathItem};
use crate::domain::video::{ShowMetadataPatch, ShowQueryFilter};
use crate::domain::watch::VideoFavoriteKind;
use crate::http::rest_router;
use crate::http::state::VideoHttpState;
use crate::{ensure_video_schema, init_video_module, register_video_models};

pub(super) fn fake_mp4_bytes(size: usize) -> Vec<u8> {
    let mut buf = vec![0u8; size.max(64)];
    buf[0..12].copy_from_slice(b"\x00\x00\x00\x18ftypmp42");
    buf
}

pub(super) fn stage_upload_files(
    base_dir: &std::path::Path,
    files: &[(&str, &[u8])],
) -> Vec<VideoUploadPathItem> {
    let stage_dir = base_dir.join(format!("upload_stage_{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&stage_dir).unwrap();
    files
        .iter()
        .enumerate()
        .map(|(idx, (filename, bytes))| {
            let temp_path = stage_dir.join(format!("{idx}_{filename}"));
            std::fs::write(&temp_path, bytes).unwrap();
            VideoUploadPathItem {
                filename: (*filename).to_string(),
                temp_path,
            }
        })
        .collect()
}

#[tokio::test]
async fn video_upload_subtitles_range_show_scan_and_watch_progress_e2e() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("xmedia_video.db");
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

    let session = auth_svc
        .setup_initial_admin(SetupAdminInput {
            username: "alice".into(),
            display_name: Some("Alice".into()),
            password: "secretpassword".into(),
        })
        .await
        .unwrap();
    let admin = auth_svc.authenticate_jwt(&session.token).await.unwrap();

    let state = VideoHttpState::new(auth_svc, library_svc.clone(), video_services.clone());
    let app = Router::new().nest("/api/v1/video", rest_router(state));

    // 1. Upload standalone movie + sidecar SRT subtitle
    let mp4_512 = fake_mp4_bytes(512);
    let uploaded = video_services
        .ingest
        .upload_files_from_paths(
            &admin,
            stage_upload_files(
                tmp.path(),
                &[
                    ("Interstellar (2014).mp4", &mp4_512),
                    (
                        "Interstellar (2014).zh.srt",
                        b"1\r\n00:00:01,250 --> 00:00:04,000\r\nDo not go gentle into that good night.\r\n",
                    ),
                ],
            ),
        )
        .await
        .unwrap();
    assert_eq!(uploaded.uploaded_videos.len(), 1);
    assert_eq!(uploaded.attached_subtitles_count, 1);
    let movie = &uploaded.uploaded_videos[0];
    assert_eq!(movie.title, "Interstellar");
    assert_eq!(movie.year, Some(2014));
    assert!(movie.poster_key.is_some());

    // 2. Verify HTTP 206 Range Direct Play + WebVTT subtitle conversion
    let stream_res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/v1/video/videos/{}/stream?token={}",
                    movie.id, session.token
                ))
                .header(header::RANGE, "bytes=0-11")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(stream_res.status(), StatusCode::PARTIAL_CONTENT);
    let chunk = to_bytes(stream_res.into_body(), usize::MAX).await.unwrap();
    assert_eq!(&chunk[4..12], b"ftypmp42");

    let vtt_res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/v1/video/videos/{}/subtitles/ext-s0.vtt?token={}",
                    movie.id, session.token
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(vtt_res.status(), StatusCode::OK);
    let vtt_text = String::from_utf8(
        to_bytes(vtt_res.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(vtt_text.starts_with("WEBVTT"));
    assert!(vtt_text.contains("00:00:01.250 --> 00:00:04.000"));

    // 3. Verify TV Show mount scan (Season dir + ASS subtitle + NFO) & episode progression
    let show_dir = tmp
        .path()
        .join("mounted_videos")
        .join("Frieren")
        .join("Season 1");
    std::fs::create_dir_all(&show_dir).unwrap();
    std::fs::write(show_dir.join("01 - Journey End.mp4"), fake_mp4_bytes(256)).unwrap();
    std::fs::write(show_dir.join("02 - Magic Trick.mp4"), fake_mp4_bytes(256)).unwrap();
    std::fs::write(
        show_dir.join("01 - Journey End.ja.ass"),
        "[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:02.50,0:00:05.00,Default,,0,0,0,,{\\i1}勇者ヒンメル{\\i0}\n",
    )
    .unwrap();

    let mount_lib = library_svc
        .create_local_mount(
            &admin,
            CreateLibraryInput {
                name: "Anime Videos".into(),
                media_kind: MediaKind::Video,
                root_path: tmp
                    .path()
                    .join("mounted_videos")
                    .to_string_lossy()
                    .into_owned(),
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
    assert_eq!(
        library_svc
            .get_library(mount_lib.id)
            .await
            .unwrap()
            .item_count,
        2
    );

    // Scanned local library stays in the pool until claimed
    assert!(
        video_services
            .catalog
            .list_shows(&admin, Default::default())
            .await
            .unwrap()
            .is_empty()
    );
    let pool_before = video_services
        .catalog
        .list_pool(&admin, Some(mount_lib.id), None)
        .await
        .unwrap();
    assert_eq!(pool_before.shows.len(), 1);
    assert!(!pool_before.shows[0].bound);

    video_services
        .catalog
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
        .catalog
        .list_shows(&admin, Default::default())
        .await
        .unwrap();
    assert_eq!(shows.len(), 1);
    assert_eq!(shows[0].title, "Frieren");
    assert_eq!(shows[0].episode_count, 2);

    let show_detail = video_services
        .catalog
        .get_show_detail(&admin, shows[0].id)
        .await
        .unwrap();
    let ep1 = &show_detail.seasons[0].episodes[0];
    let ep2 = &show_detail.seasons[0].episodes[1];
    assert_eq!(ep1.episode_number, Some(1));
    assert_eq!(ep2.episode_number, Some(2));

    let ep1_detail = video_services
        .catalog
        .get_video_detail(&admin, ep1.id)
        .await
        .unwrap();
    assert_eq!(ep1_detail.next_episode_id, Some(ep2.id));

    let ass_vtt = video_services
        .catalog
        .get_subtitle_webvtt(admin.id, ep1.id, "ext-s0")
        .await
        .unwrap();
    assert!(ass_vtt.contains("00:00:02.500 --> 00:00:05.000"));
    assert!(ass_vtt.contains("勇者ヒンメル"));
    assert!(!ass_vtt.contains("{\\i1}"));

    // Mark Ep.1 >= 92% watched -> auto-completes Ep.1 -> show continue_episode advances to Ep.2!
    let prog = video_services
        .watch
        .save_progress(
            &admin,
            ep1.id,
            SaveWatchProgressInput {
                position_secs: 1380.0,
                duration_secs: Some(1440),
                completed: None,
                selected_audio_stream: None,
                selected_subtitle: Some("ext-s0".into()),
            },
        )
        .await
        .unwrap();
    assert!(prog.completed);

    let updated_show = video_services
        .catalog
        .get_show_detail(&admin, shows[0].id)
        .await
        .unwrap();
    assert_eq!(updated_show.show.watched_episode_count, 1);
    assert_eq!(updated_show.continue_episode.unwrap().id, ep2.id);

    // 4. Rename & organize ep1 into Jellyfin layout with `tvshow.nfo` -> VideoId & watched progress survive,
    //    and `tvshow.nfo` <title> populates `show_title` without overwriting episode <title>.
    let jf_show_dir = tmp
        .path()
        .join("mounted_videos")
        .join("葬送的芙莉莲 (2023)");
    let jf_season_dir = jf_show_dir.join("Season 01");
    std::fs::create_dir_all(&jf_season_dir).unwrap();
    std::fs::write(
        jf_show_dir.join("tvshow.nfo"),
        "<tvshow><title>葬送的芙莉莲</title><year>2023</year></tvshow>",
    )
    .unwrap();
    std::fs::rename(
        show_dir.join("01 - Journey End.mp4"),
        jf_season_dir.join("葬送的芙莉莲 - S01E01 - 旅程的终点.mp4"),
    )
    .unwrap();
    std::fs::rename(
        show_dir.join("01 - Journey End.ja.ass"),
        jf_season_dir.join("葬送的芙莉莲 - S01E01 - 旅程的终点.ja.ass"),
    )
    .unwrap();
    std::fs::write(
        jf_season_dir.join("葬送的芙莉莲 - S01E01 - 旅程的终点.nfo"),
        "<episodedetails><title>旅程的终点</title><season>1</season><episode>1</episode></episodedetails>",
    )
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
        .catalog
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

    let moved_ep1 = video_services
        .catalog
        .get_video_detail(&admin, ep1.id)
        .await
        .unwrap();
    assert_eq!(moved_ep1.video.title, "旅程的终点");
    assert_eq!(moved_ep1.video.show_title.as_deref(), Some("葬送的芙莉莲"));
    assert!(
        moved_ep1
            .path
            .contains("葬送的芙莉莲 - S01E01 - 旅程的终点.mp4")
    );
    assert!(
        moved_ep1
            .video
            .progress
            .as_ref()
            .is_some_and(|p| p.completed)
    );

    // 5. Verify WebVTT timestamp shift for transcoded seeks & poster auth enforcement
    let shifted_vtt_res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/v1/video/videos/{}/subtitles/ext-s0.vtt?token={}&offset_secs=2.0",
                    movie.id, session.token
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(shifted_vtt_res.status(), StatusCode::OK);
    let shifted_vtt = String::from_utf8(
        to_bytes(shifted_vtt_res.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(shifted_vtt.contains("00:00:00.000 --> 00:00:02.000"));

    let dropped_vtt_res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/v1/video/videos/{}/subtitles/ext-s0.vtt?token={}&offset_secs=5.0",
                    movie.id, session.token
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(dropped_vtt_res.status(), StatusCode::OK);
    let dropped_vtt = String::from_utf8(
        to_bytes(dropped_vtt_res.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(!dropped_vtt.contains("Do not go gentle"));

    let poster_key = movie.poster_key.as_deref().unwrap();
    let unauth_poster_res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/video/posters/{poster_key}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauth_poster_res.status(), StatusCode::UNAUTHORIZED);

    let auth_poster_res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/v1/video/posters/{poster_key}?token={}",
                    session.token
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(auth_poster_res.status(), StatusCode::OK);

    // 6. Verify ShowQueryFilter.tag filtering & update_show_metadata episode show_title sync
    let moved_show_id = moved_ep1.video.show_id.unwrap();
    video_services
        .catalog
        .update_show_metadata(
            &admin,
            moved_show_id,
            ShowMetadataPatch {
                title: Some("Frieren Beyond Journey".into()),
                tags: Some(vec!["fantasy".into()]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let tagged_shows = video_services
        .catalog
        .list_shows(
            &admin,
            ShowQueryFilter {
                tag: Some("FANTASY".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(tagged_shows.len(), 1);
    assert!(
        video_services
            .catalog
            .list_shows(
                &admin,
                ShowQueryFilter {
                    tag: Some("scifi".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap()
            .is_empty()
    );
    let ep1_after_show_rename = video_services
        .catalog
        .get_video_detail(&admin, ep1.id)
        .await
        .unwrap();
    assert_eq!(
        ep1_after_show_rename.video.show_title.as_deref(),
        Some("Frieren Beyond Journey")
    );

    // 7. Verify delete_video removes favorite and shelf references
    video_services
        .watch
        .set_favorite(&admin, VideoFavoriteKind::Video, movie.id.0, true)
        .await
        .unwrap();
    let shelf = video_services
        .watch
        .create_shelf(
            &admin,
            UpsertVideoShelfInput {
                name: "Favorites".into(),
                description: String::new(),
                video_ids: vec![movie.id],
            },
        )
        .await
        .unwrap();
    video_services
        .catalog
        .delete_video(&admin, movie.id)
        .await
        .unwrap();
    let favs_after = video_services.watch.list_favorites(&admin).await.unwrap();
    assert!(favs_after.videos.is_empty());
    let shelf_after = video_services
        .watch
        .get_shelf(&admin, shelf.shelf.id)
        .await
        .unwrap();
    assert!(shelf_after.videos.is_empty());
}

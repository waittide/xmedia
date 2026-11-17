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

use application::VideoServices;
use application::catalog::VideoCatalogService;
use application::lifecycle::VideoDomainLifecycle;
use application::streaming::VideoStreamingService;
use application::watch::VideoWatchService;
use infra::media_engine::FfmpegVideoEngine;
use infra::persistence::repositories::{ToastyVideoCatalogRepository, ToastyVideoWatchRepository};
use infra::storage::{LocalVideoScanner, LocalVideoStorage};

pub use infra::persistence::{ensure_video_schema, register_video_models};

pub fn init_video_module(
    db: Db,
    data_dir: &Path,
    ffmpeg: FfmpegToolkit,
    libraries_repo: Arc<dyn LibraryRepository>,
    library_service: &LibraryService,
    events: Arc<dyn RealtimeEventPort>,
) -> DomainResult<VideoServices> {
    let catalog_repo = Arc::new(ToastyVideoCatalogRepository::new(db.clone()));
    let watch_repo = Arc::new(ToastyVideoWatchRepository::new(db));

    let storage = Arc::new(LocalVideoStorage::new(data_dir)?);
    let scanner = Arc::new(LocalVideoScanner);
    let engine = Arc::new(FfmpegVideoEngine::new(ffmpeg));

    let catalog = VideoCatalogService::new(
        libraries_repo.clone(),
        catalog_repo.clone(),
        watch_repo.clone(),
        storage.clone(),
        engine.clone(),
    );
    let watch = Arc::new(VideoWatchService::new(
        catalog_repo.clone(),
        watch_repo.clone(),
        catalog.clone(),
    ));
    let streaming = Arc::new(VideoStreamingService::new(
        libraries_repo.clone(),
        catalog_repo.clone(),
        storage.clone(),
        engine.clone(),
    ));
    let ingest = Arc::new(VideoDomainLifecycle::new(
        libraries_repo,
        catalog_repo,
        watch_repo,
        catalog.clone(),
        storage,
        scanner,
        engine,
        events,
    ));
    library_service.register_lifecycle(ingest.clone());

    Ok(VideoServices::new(
        Arc::new(catalog),
        watch,
        streaming,
        ingest,
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
    use tempfile::TempDir;
    use toasty::Db;
    use toasty::schema::ModelSet;
    use tower::ServiceExt;
    use xmedia_core::application::auth::AuthService;
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

    use crate::application::dto::{SaveWatchProgressInput, VideoUploadItem};
    use crate::http::rest_router;
    use crate::http::state::VideoHttpState;
    use crate::{ensure_video_schema, init_video_module, register_video_models};

    fn fake_mp4_bytes(size: usize) -> Vec<u8> {
        let mut buf = vec![0u8; size.max(64)];
        buf[0..12].copy_from_slice(b"\x00\x00\x00\x18ftypmp42");
        buf
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
        let uploaded = video_services
            .ingest
            .upload_files(
                &admin,
                vec![
                    VideoUploadItem {
                        filename: "Interstellar (2014).mp4".into(),
                        bytes: Bytes::from(fake_mp4_bytes(512)),
                    },
                    VideoUploadItem {
                        filename: "Interstellar (2014).zh.srt".into(),
                        bytes: Bytes::from_static(
                            b"1\r\n00:00:01,250 --> 00:00:04,000\r\nDo not go gentle into that good night.\r\n",
                        ),
                    },
                ],
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
            .get_subtitle_webvtt(ep1.id, "ext-s0")
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
    }
}

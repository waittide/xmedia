use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::Query;
use axum::http::{Request, StatusCode, header};
use axum::routing::get;
use serde::Deserialize;
use tempfile::TempDir;
use toasty::Db;
use toasty::schema::ModelSet;
use tower::ServiceExt;
use xmedia_core::application::auth::AuthService;
use xmedia_core::application::dto::SetupAdminInput;
use xmedia_core::application::library::LibraryService;
use xmedia_core::infra::events::BroadcastEventHub;
use xmedia_core::infra::ffmpeg::FfmpegToolkit;
use xmedia_core::infra::persistence::repositories::{
    ToastyLibraryRepository, ToastyUserRepository,
};
use xmedia_core::infra::security::{JwtTokenService, SecurityManager};
use xmedia_core::{ensure_core_schema, register_core_models};

use crate::http::rest_router;
use crate::http::state::VideoHttpState;
use crate::{ensure_video_schema, init_video_module_with_allow_private_ips, register_video_models};

use super::{
    PNG_1X1, fake_mp4_payload, fake_ts_segment, patch_json, post_json, send_json_with_method,
    setup_test_app,
};

#[derive(Debug, Default, Deserialize)]
struct VodIdsQuery {
    ids: Option<String>,
}

#[tokio::test]
async fn online_video_items_favorites_episode_progress_and_availability_e2e() {
    let (_tmp, app, admin_tok, member_tok) = setup_test_app(true).await;

    let src_json = serde_json::json!({
        "sourceName": "Test Video Source",
        "sourceUrl": "https://video-item.test",
        "sourceGroup": "Test",
        "sourceType": 1,
        "apiUrl": "https://video-item.test/api.php/provide/vod"
    });
    let (st_adm, adm_val) = post_json(
        &app,
        "/api/v1/plugins/import?domain=video",
        &admin_tok,
        serde_json::json!({ "raw_json": src_json.to_string() }),
    )
    .await;
    assert_eq!(st_adm, StatusCode::OK);
    let admin_source_id = adm_val["sources"][0]["id"].as_str().unwrap().to_string();

    let (st_imp, imp_val) = post_json(
        &app,
        "/api/v1/plugins/import?domain=video",
        &member_tok,
        serde_json::json!({ "raw_json": src_json.to_string() }),
    )
    .await;
    assert_eq!(st_imp, StatusCode::OK);
    let source_id = imp_val["sources"][0]["id"].as_str().unwrap().to_string();
    assert_ne!(source_id, admin_source_id);

    let (st_put, item) = send_json_with_method(
        &app,
        "PUT",
        "/api/v1/video/online/items",
        &member_tok,
        Some(serde_json::json!({
            "source_id": source_id,
            "item_key": "https://video-item.test/vod/101",
            "title": "Spirited Away Series",
            "subtitle": "2024 · Anime",
            "cover_url": "https://video-item.test/poster.jpg",
            "kind": "anime",
            "extra": { "vod_id": "101", "remarks": "12 Episodes" },
            "favorite": true,
            "progress": {
                "unit_index": 1,
                "unit_key": "https://video-item.test/play/101-2.m3u8",
                "unit_title": "EP 02",
                "unit_group": "HD Line 1",
                "position": 600.0,
                "duration": 1200.0,
                "total_units": 12,
                "completed": false
            }
        })),
    )
    .await;
    assert_eq!(st_put, StatusCode::OK);
    let item_id = item["id"].as_str().unwrap().to_string();
    assert_eq!(item["media_kind"], "video");
    assert_eq!(item["source_name"], "Test Video Source");
    assert_eq!(item["availability"], "ok");
    assert_eq!(item["progress"]["unit_group"], "HD Line 1");
    assert!((item["progress_ratio"].as_f64().unwrap() - (1.5 / 12.0)).abs() < 1e-6);

    let (st_lk, lk) = post_json(
        &app,
        "/api/v1/video/online/items/lookup",
        &member_tok,
        serde_json::json!({
            "items": [
                { "source_id": source_id, "item_key": "https://video-item.test/vod/101" }
            ]
        }),
    )
    .await;
    assert_eq!(st_lk, StatusCode::OK);
    assert_eq!(lk["total"], 1);

    let (st_cont, cont) = send_json_with_method(
        &app,
        "GET",
        "/api/v1/video/online/items?view=continue",
        &member_tok,
        None,
    )
    .await;
    assert_eq!(st_cont, StatusCode::OK);
    assert_eq!(cont["total"], 1);
    assert_eq!(cont["items"][0]["id"], item_id);

    let (st_prog, prog) = send_json_with_method(
        &app,
        "PUT",
        &format!("/api/v1/video/online/items/{item_id}/progress"),
        &member_tok,
        Some(serde_json::json!({
            "unit_index": 2,
            "unit_key": "https://video-item.test/play/101-3.m3u8",
            "unit_title": "EP 03",
            "unit_group": "Backup Line",
            "position": 300.0,
            "duration": 1200.0,
            "total_units": 12,
            "completed": false
        })),
    )
    .await;
    assert_eq!(st_prog, StatusCode::OK);
    assert_eq!(prog["progress"]["unit_group"], "Backup Line");
    assert_eq!(prog["progress"]["unit_index"], 2);

    // Admin cannot mutate member's plugin (404 Not Found), and disabling admin's own plugin does not affect member
    let (st_adm_patch_member, _) = patch_json(
        &app,
        &format!("/api/v1/plugins/{source_id}"),
        &admin_tok,
        serde_json::json!({ "enabled": false }),
    )
    .await;
    assert_eq!(st_adm_patch_member, StatusCode::NOT_FOUND);

    let (st_dis, _) = patch_json(
        &app,
        &format!("/api/v1/plugins/{source_id}"),
        &member_tok,
        serde_json::json!({ "enabled": false }),
    )
    .await;
    assert_eq!(st_dis, StatusCode::OK);

    let (st_get, got) = send_json_with_method(
        &app,
        "GET",
        &format!("/api/v1/video/online/items/{item_id}"),
        &member_tok,
        None,
    )
    .await;
    assert_eq!(st_get, StatusCode::OK);
    assert_eq!(got["availability"], "source_disabled");

    let (st_clr, _) = send_json_with_method(
        &app,
        "DELETE",
        &format!("/api/v1/video/online/items/{item_id}/progress"),
        &member_tok,
        None,
    )
    .await;
    assert_eq!(st_clr, StatusCode::OK);

    let (st_cont2, cont2) = send_json_with_method(
        &app,
        "GET",
        "/api/v1/video/online/items?view=continue",
        &member_tok,
        None,
    )
    .await;
    assert_eq!(st_cont2, StatusCode::OK);
    assert_eq!(cont2["total"], 0);

    let (st_del, _) = send_json_with_method(
        &app,
        "DELETE",
        &format!("/api/v1/video/online/items/{item_id}"),
        &member_tok,
        None,
    )
    .await;
    assert_eq!(st_del, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn online_tracked_video_movie_show_play_cache_and_refresh_e2e() {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    let (_tmp, app, admin_tok, member_tok) = setup_test_app(true).await;

    let include_ep3 = Arc::new(AtomicBool::new(false));
    let include_ep3_srv = Arc::clone(&include_ep3);
    let mp4_hits = Arc::new(AtomicUsize::new(0));
    let mp4_hits_srv = Arc::clone(&mp4_hits);
    let mp4_bytes = Arc::new(fake_mp4_payload());
    let mp4_for_srv = Arc::clone(&mp4_bytes);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");
    let base_srv = base_url.clone();

    let mock_router = Router::new()
        .route(
            "/api.php/provide/vod/",
            get(move |Query(q): Query<VodIdsQuery>| {
                let b = base_srv.clone();
                let ep3 = Arc::clone(&include_ep3_srv);
                async move {
                    let ids = q.ids.unwrap_or_default();
                    let list = if ids == "101" {
                        serde_json::json!([{
                            "vod_id": 101,
                            "vod_name": "星际迷航：测试电影",
                            "vod_pic": format!("{b}/poster.png"),
                            "vod_year": "2025",
                            "vod_area": "科幻",
                            "vod_content": "一部用于测试在线追更和离线缓存的电影。",
                            "vod_play_from": "mp4line",
                            "vod_play_url": format!("正片${b}/media/movie.mp4")
                        }])
                    } else if ids == "202" {
                        let mut eps = format!("第01集${b}/media/ep1.mp4#第02集${b}/media/ep2.mp4");
                        if ep3.load(Ordering::SeqCst) {
                            eps.push_str(&format!("#第03集${b}/media/ep3.mp4"));
                        }
                        serde_json::json!([{
                            "vod_id": 202,
                            "vod_name": "星海纪元",
                            "vod_pic": format!("{b}/poster.png"),
                            "vod_year": "2026",
                            "vod_area": "动画",
                            "vod_content": "一部连载中的在线追更剧集。",
                            "vod_play_from": "mp4line",
                            "vod_play_url": eps
                        }])
                    } else {
                        serde_json::json!([])
                    };
                    let body = serde_json::json!({
                        "code": 1,
                        "page": 1,
                        "pagecount": 1,
                        "total": list.as_array().map(|a| a.len()).unwrap_or(0),
                        "list": list
                    });
                    (
                        StatusCode::OK,
                        [(header::CONTENT_TYPE, "application/json")],
                        body.to_string(),
                    )
                }
            }),
        )
        .route(
            "/media/{name}",
            get(move || {
                let hits = Arc::clone(&mp4_hits_srv);
                let bytes = Arc::clone(&mp4_for_srv);
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    (
                        StatusCode::OK,
                        [(header::CONTENT_TYPE, "video/mp4")],
                        (*bytes).clone(),
                    )
                }
            }),
        )
        .route(
            "/poster.png",
            get(|| async {
                (
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, "image/png")],
                    PNG_1X1.to_vec(),
                )
            }),
        );

    let srv_handle = tokio::spawn(async move {
        axum::serve(listener, mock_router).await.unwrap();
    });

    let source_spec = serde_json::json!({
        "sourceName": "Mock Tracked Video Source",
        "sourceUrl": base_url,
        "apiUrl": format!("{base_url}/api.php/provide/vod/"),
        "sourceFormat": "applecms_v10",
        "enabled": true
    });
    let (st_imp, imp_val) = send_json_with_method(
        &app,
        "POST",
        "/api/v1/plugins/import?domain=video",
        &admin_tok,
        Some(serde_json::json!({ "raw_json": source_spec.to_string() })),
    )
    .await;
    assert_eq!(st_imp, StatusCode::OK, "{imp_val}");
    let source_id = imp_val["sources"][0]["id"].as_str().unwrap().to_string();

    let (st_mv, mv_val) = send_json_with_method(
        &app,
        "POST",
        "/api/v1/video/online/track",
        &admin_tok,
        Some(serde_json::json!({
            "source_id": source_id,
            "video_id": "101"
        })),
    )
    .await;
    assert_eq!(st_mv, StatusCode::OK, "{mv_val}");
    assert_eq!(mv_val["kind"], "movie");
    assert_eq!(mv_val["added_episodes"], 1);
    let movie_id = mv_val["video"]["id"].as_str().unwrap().to_string();
    assert_eq!(mv_val["video"]["title"], "星际迷航：测试电影");
    assert_eq!(mv_val["video"]["online_tracking"]["cached"], false);
    assert!(mv_val["video"]["poster_key"].as_str().is_some());
    assert_eq!(mp4_hits.load(Ordering::SeqCst), 0);

    let (st_pool, pool_val) =
        send_json_with_method(&app, "GET", "/api/v1/video/pool", &member_tok, None).await;
    assert_eq!(st_pool, StatusCode::OK, "{pool_val}");
    assert!(pool_val["movies"].as_array().unwrap().is_empty());

    let (st_iso, _) = send_json_with_method(
        &app,
        "GET",
        &format!("/api/v1/video/videos/{movie_id}/online/play?token={member_tok}"),
        &member_tok,
        None,
    )
    .await;
    assert_eq!(st_iso, StatusCode::NOT_FOUND);

    // Member cannot track using admin's source_id (404) and cannot bind admin's OnlineTracked item
    let (st_foreign_track, _) = send_json_with_method(
        &app,
        "POST",
        "/api/v1/video/online/track",
        &member_tok,
        Some(serde_json::json!({
            "source_id": source_id,
            "video_id": "101"
        })),
    )
    .await;
    assert_eq!(st_foreign_track, StatusCode::NOT_FOUND);

    let (st_bind_foreign, bind_foreign_val) = send_json_with_method(
        &app,
        "POST",
        "/api/v1/video/bindings",
        &member_tok,
        Some(serde_json::json!({ "item_ids": [movie_id] })),
    )
    .await;
    assert_eq!(st_bind_foreign, StatusCode::OK, "{bind_foreign_val}");
    assert_eq!(bind_foreign_val["bound_count"], 0);
    assert_eq!(bind_foreign_val["total_requested"], 0);

    // Member imports their own isolated plugin and tracks their own OnlineTracked movie
    let (st_m_imp, m_imp_val) = send_json_with_method(
        &app,
        "POST",
        "/api/v1/plugins/import?domain=video",
        &member_tok,
        Some(serde_json::json!({ "raw_json": source_spec.to_string() })),
    )
    .await;
    assert_eq!(st_m_imp, StatusCode::OK, "{m_imp_val}");
    let member_src_id = m_imp_val["sources"][0]["id"].as_str().unwrap().to_string();
    assert_ne!(member_src_id, source_id);

    let (st_member_mv, member_mv_val) = send_json_with_method(
        &app,
        "POST",
        "/api/v1/video/online/track",
        &member_tok,
        Some(serde_json::json!({
            "source_id": member_src_id,
            "video_id": "101"
        })),
    )
    .await;
    assert_eq!(st_member_mv, StatusCode::OK, "{member_mv_val}");
    let member_movie_id = member_mv_val["video"]["id"].as_str().unwrap().to_string();
    assert_ne!(member_movie_id, movie_id);
    assert_eq!(member_mv_val["added_episodes"], 1);

    let (st_play, play_val) = send_json_with_method(
        &app,
        "GET",
        &format!("/api/v1/video/videos/{member_movie_id}/online/play?token={member_tok}"),
        &member_tok,
        None,
    )
    .await;
    assert_eq!(st_play, StatusCode::OK, "{play_val}");
    assert_eq!(play_val["cached"], false);
    assert_eq!(play_val["stream_type"], "mp4");
    assert!(
        play_val["proxy_stream_url"]
            .as_str()
            .unwrap()
            .contains("/api/v1/video/online/stream-proxy")
    );

    let (st_cache, cached_mv) = send_json_with_method(
        &app,
        "POST",
        &format!("/api/v1/video/videos/{movie_id}/online/cache"),
        &admin_tok,
        None,
    )
    .await;
    assert_eq!(st_cache, StatusCode::OK, "{cached_mv}");
    assert_eq!(cached_mv["online_tracking"]["cached"], true);
    assert_eq!(cached_mv["file_size"], mp4_bytes.len() as u64);
    assert_eq!(mp4_hits.load(Ordering::SeqCst), 1);

    let range_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/v1/video/videos/{movie_id}/stream?quality=direct&token={admin_tok}"
                ))
                .header(header::RANGE, "bytes=0-15")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(range_resp.status(), StatusCode::PARTIAL_CONTENT);
    let range_bytes = to_bytes(range_resp.into_body(), 1024 * 1024).await.unwrap();
    assert_eq!(&range_bytes[..16], &mp4_bytes[..16]);

    let (st_show, show_val) = send_json_with_method(
        &app,
        "POST",
        "/api/v1/video/online/track",
        &admin_tok,
        Some(serde_json::json!({
            "source_id": source_id,
            "video_id": "202"
        })),
    )
    .await;
    assert_eq!(st_show, StatusCode::OK, "{show_val}");
    assert_eq!(show_val["kind"], "show");
    assert_eq!(show_val["added_episodes"], 2);
    let show_id = show_val["show"]["show"]["id"].as_str().unwrap().to_string();
    assert_eq!(show_val["show"]["show"]["episode_count"], 2);
    assert_eq!(
        show_val["show"]["show"]["online_tracking"]["source_id"],
        source_id
    );

    include_ep3.store(true, Ordering::SeqCst);
    let (st_ref, ref_val) = send_json_with_method(
        &app,
        "POST",
        &format!("/api/v1/video/shows/{show_id}/online/refresh"),
        &admin_tok,
        None,
    )
    .await;
    assert_eq!(st_ref, StatusCode::OK, "{ref_val}");
    assert_eq!(ref_val["show"]["episode_count"], 3);
    assert_eq!(
        ref_val["seasons"][0]["episodes"].as_array().unwrap().len(),
        3
    );

    let (st_sc, sc_val) = send_json_with_method(
        &app,
        "POST",
        &format!("/api/v1/video/shows/{show_id}/online/cache"),
        &admin_tok,
        Some(serde_json::json!({})),
    )
    .await;
    assert_eq!(st_sc, StatusCode::OK, "{sc_val}");
    assert_eq!(sc_val["cached_count"], 3);
    assert_eq!(sc_val["failed_count"], 0);
    assert_eq!(mp4_hits.load(Ordering::SeqCst), 4);

    let (st_sc2, sc_val2) = send_json_with_method(
        &app,
        "POST",
        &format!("/api/v1/video/shows/{show_id}/online/cache"),
        &admin_tok,
        Some(serde_json::json!({})),
    )
    .await;
    assert_eq!(st_sc2, StatusCode::OK);
    assert_eq!(sc_val2["cached_count"], 0);
    assert_eq!(sc_val2["skipped_count"], 3);
    assert_eq!(mp4_hits.load(Ordering::SeqCst), 4);

    let (st_ub1, ub1_val) = send_json_with_method(
        &app,
        "DELETE",
        &format!("/api/v1/video/bindings/{movie_id}"),
        &admin_tok,
        None,
    )
    .await;
    assert_eq!(st_ub1, StatusCode::OK, "{ub1_val}");
    assert_eq!(ub1_val["unbound"], true);
    assert_eq!(ub1_val["remaining_bindings"], 0);
    assert_eq!(ub1_val["deleted_from_pool"], true);

    let (st_ub2, ub2_val) = send_json_with_method(
        &app,
        "DELETE",
        &format!("/api/v1/video/bindings/{member_movie_id}"),
        &member_tok,
        None,
    )
    .await;
    assert_eq!(st_ub2, StatusCode::OK, "{ub2_val}");
    assert_eq!(ub2_val["unbound"], true);
    assert_eq!(ub2_val["remaining_bindings"], 0);
    assert_eq!(ub2_val["deleted_from_pool"], true);

    let (st_ubs, ubs_val) = send_json_with_method(
        &app,
        "DELETE",
        &format!("/api/v1/video/bindings/{show_id}"),
        &admin_tok,
        None,
    )
    .await;
    assert_eq!(st_ubs, StatusCode::OK, "{ubs_val}");
    assert_eq!(ubs_val["unbound"], true);
    assert_eq!(ubs_val["remaining_bindings"], 0);
    assert_eq!(ubs_val["deleted_from_pool"], true);

    srv_handle.abort();
}

#[tokio::test]
async fn jellyfin_api_local_and_online_tracked_video_e2e() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("xmedia_video_jellyfin.db");
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
    let security = Arc::new(SecurityManager::new("video-jf-secret"));
    let tokens = Arc::new(JwtTokenService::new("video-jf-secret", 3600));

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
    let video_services = init_video_module_with_allow_private_ips(
        db,
        tmp.path(),
        FfmpegToolkit::new("__missing_ffmpeg__", "__missing_ffprobe__"),
        libraries_repo,
        &library_svc,
        events,
        true,
    )
    .unwrap();

    let admin_session = auth_svc
        .setup_initial_admin(SetupAdminInput {
            username: "admin".into(),
            display_name: Some("Admin".into()),
            password: "adminpassword123".into(),
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

    let local_mp4 = fake_mp4_payload();
    let uploaded = video_services
        .ingest
        .upload_files_from_paths(
            &admin,
            super::super::stage_upload_files(
                tmp.path(),
                &[("Local Classic (2020).mp4", &local_mp4)],
            ),
        )
        .await
        .unwrap();
    let local_movie_id = uploaded.uploaded_videos[0].id;

    let state = VideoHttpState::new(auth_svc, library_svc, video_services);
    let app = Router::new()
        .nest("/api/v1/plugins", crate::http::plugins_router(&state))
        .nest("/api/v1/video", rest_router(state.clone()))
        .nest("/jellyfin", crate::http::jellyfin_router(state));

    let ts_seg = Arc::new(fake_ts_segment());
    let ts_for_srv = Arc::clone(&ts_seg);
    let online_mp4 = Arc::new(fake_mp4_payload());
    let online_mp4_srv = Arc::clone(&online_mp4);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");
    let base_srv = base_url.clone();

    let mock_router = Router::new()
        .route(
            "/api.php/provide/vod/",
            get(move |Query(q): Query<VodIdsQuery>| {
                let b = base_srv.clone();
                async move {
                    let ids = q.ids.unwrap_or_default();
                    let list = if ids == "m1" {
                        serde_json::json!([{
                            "vod_id": "m1",
                            "vod_name": "在线电影MP4",
                            "vod_pic": format!("{b}/poster.png"),
                            "vod_year": "2025",
                            "vod_area": "动作",
                            "vod_play_from": "mp4",
                            "vod_play_url": format!("正片${b}/direct.mp4")
                        }])
                    } else if ids == "s1" {
                        serde_json::json!([{
                            "vod_id": "s1",
                            "vod_name": "在线追更番剧",
                            "vod_pic": format!("{b}/poster.png"),
                            "vod_year": "2026",
                            "vod_area": "动画",
                            "vod_play_from": "hls",
                            "vod_play_url": format!("第01集${b}/hls/ep1.m3u8#第02集${b}/hls/ep2.m3u8")
                        }])
                    } else {
                        serde_json::json!([])
                    };
                    (
                        StatusCode::OK,
                        [(header::CONTENT_TYPE, "application/json")],
                        serde_json::json!({ "code": 1, "list": list }).to_string(),
                    )
                }
            }),
        )
        .route(
            "/direct.mp4",
            get(move || {
                let d = Arc::clone(&online_mp4_srv);
                async move {
                    (
                        StatusCode::OK,
                        [(header::CONTENT_TYPE, "video/mp4")],
                        (*d).clone(),
                    )
                }
            }),
        )
        .route(
            "/hls/ep1.m3u8",
            get(|| async {
                let m3u8 = "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:6\n#EXTINF:6.0,\nseg0.ts\n#EXT-X-ENDLIST\n";
                (
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, "application/vnd.apple.mpegurl")],
                    m3u8,
                )
            }),
        )
        .route(
            "/hls/seg0.ts",
            get(move || {
                let s = Arc::clone(&ts_for_srv);
                async move {
                    (
                        StatusCode::OK,
                        [(header::CONTENT_TYPE, "video/mp2t")],
                        (*s).clone(),
                    )
                }
            }),
        )
        .route(
            "/poster.png",
            get(|| async {
                (
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, "image/png")],
                    PNG_1X1.to_vec(),
                )
            }),
        );
    let srv_handle = tokio::spawn(async move {
        axum::serve(listener, mock_router).await.unwrap();
    });

    let src_spec = serde_json::json!({
        "sourceName": "JF Mock Source",
        "sourceUrl": base_url,
        "apiUrl": format!("{base_url}/api.php/provide/vod/"),
        "sourceFormat": "applecms_v10",
        "enabled": true
    });
    let (st_imp, imp_val) = send_json_with_method(
        &app,
        "POST",
        "/api/v1/plugins/import?domain=video",
        &admin_session.token,
        Some(serde_json::json!({ "raw_json": src_spec.to_string() })),
    )
    .await;
    assert_eq!(st_imp, StatusCode::OK);
    let source_id = imp_val["sources"][0]["id"].as_str().unwrap().to_string();

    let (st_m1, m1_val) = send_json_with_method(
        &app,
        "POST",
        "/api/v1/video/online/track",
        &admin_session.token,
        Some(serde_json::json!({ "source_id": source_id, "video_id": "m1" })),
    )
    .await;
    assert_eq!(st_m1, StatusCode::OK);
    let tracked_movie_id = m1_val["video"]["id"].as_str().unwrap().to_string();
    let tracked_movie_poster = m1_val["video"]["poster_key"].as_str().unwrap().to_string();

    let (st_s1, s1_val) = send_json_with_method(
        &app,
        "POST",
        "/api/v1/video/online/track",
        &admin_session.token,
        Some(serde_json::json!({ "source_id": source_id, "video_id": "s1" })),
    )
    .await;
    assert_eq!(st_s1, StatusCode::OK);
    let tracked_show_id = s1_val["show"]["show"]["id"].as_str().unwrap().to_string();
    let tracked_ep1_id = s1_val["show"]["seasons"][0]["episodes"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let pub_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/jellyfin/System/Info/Public")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(pub_resp.status(), StatusCode::OK);
    let pub_json: serde_json::Value =
        serde_json::from_slice(&to_bytes(pub_resp.into_body(), 1024 * 1024).await.unwrap())
            .unwrap();
    assert_eq!(pub_json["ServerName"], "XMedia");
    assert_eq!(pub_json["StartupWizardCompleted"], true);

    let auth_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/jellyfin/Users/AuthenticateByName")
                .header(header::CONTENT_TYPE, "application/json")
                .header(
                    "X-Emby-Authorization",
                    r#"MediaBrowser Client="Infuse", Device="AppleTV", DeviceId="tv-1", Version="8.0""#,
                )
                .body(Body::from(
                    serde_json::json!({
                        "Username": "admin",
                        "Pw": "adminpassword123"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(auth_resp.status(), StatusCode::OK);
    let auth_json: serde_json::Value =
        serde_json::from_slice(&to_bytes(auth_resp.into_body(), 1024 * 1024).await.unwrap())
            .unwrap();
    let jf_token = auth_json["AccessToken"].as_str().unwrap().to_string();
    let jf_user_id = auth_json["User"]["Id"].as_str().unwrap().to_string();
    assert!(!jf_token.is_empty());

    let views_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/jellyfin/Users/{jf_user_id}/Views"))
                .header("X-Emby-Token", &jf_token)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(views_resp.status(), StatusCode::OK);
    let views_json: serde_json::Value =
        serde_json::from_slice(&to_bytes(views_resp.into_body(), 1024 * 1024).await.unwrap())
            .unwrap();
    let lib_id = views_json["Items"][0]["Id"].as_str().unwrap().to_string();

    let items_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/jellyfin/Users/{jf_user_id}/Items?ParentId={lib_id}"
                ))
                .header(
                    header::AUTHORIZATION,
                    format!(r#"MediaBrowser Client="Swiftfin", Token="{jf_token}""#),
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(items_resp.status(), StatusCode::OK);
    let items_json: serde_json::Value =
        serde_json::from_slice(&to_bytes(items_resp.into_body(), 1024 * 1024).await.unwrap())
            .unwrap();
    assert_eq!(items_json["TotalRecordCount"], 3);

    let seasons_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/jellyfin/Shows/{tracked_show_id}/Seasons"))
                .header("X-Emby-Token", &jf_token)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(seasons_resp.status(), StatusCode::OK);
    let seasons_json: serde_json::Value = serde_json::from_slice(
        &to_bytes(seasons_resp.into_body(), 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(seasons_json["TotalRecordCount"], 1);
    let season1_id = seasons_json["Items"][0]["Id"].as_str().unwrap().to_string();

    let eps_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/jellyfin/Shows/{tracked_show_id}/Episodes?SeasonId={season1_id}"
                ))
                .header("X-Emby-Token", &jf_token)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(eps_resp.status(), StatusCode::OK);
    let eps_json: serde_json::Value =
        serde_json::from_slice(&to_bytes(eps_resp.into_body(), 1024 * 1024).await.unwrap())
            .unwrap();
    assert_eq!(eps_json["TotalRecordCount"], 2);

    let local_pb_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/jellyfin/Items/{local_movie_id}/PlaybackInfo"))
                .header("X-Emby-Token", &jf_token)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(local_pb_resp.status(), StatusCode::OK);
    let local_pb: serde_json::Value = serde_json::from_slice(
        &to_bytes(local_pb_resp.into_body(), 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    let local_stream_url = local_pb["MediaSources"][0]["DirectStreamUrl"]
        .as_str()
        .unwrap();
    let local_stream_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/jellyfin{local_stream_url}"))
                .header(header::RANGE, "bytes=0-15")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(local_stream_resp.status(), StatusCode::PARTIAL_CONTENT);
    let local_chunk = to_bytes(local_stream_resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    assert_eq!(&local_chunk[..16], &local_mp4[..16]);

    let mp4_pb_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/jellyfin/Items/{tracked_movie_id}/PlaybackInfo"))
                .header("X-Emby-Token", &jf_token)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(mp4_pb_resp.status(), StatusCode::OK);
    let mp4_pb: serde_json::Value = serde_json::from_slice(
        &to_bytes(mp4_pb_resp.into_body(), 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    let mp4_stream_url = mp4_pb["MediaSources"][0]["DirectStreamUrl"]
        .as_str()
        .unwrap();
    let mp4_stream_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/jellyfin{mp4_stream_url}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(mp4_stream_resp.status(), StatusCode::OK);
    let mp4_chunk = to_bytes(mp4_stream_resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    assert_eq!(mp4_chunk.as_ref(), online_mp4.as_slice());

    let hls_pb_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/jellyfin/Items/{tracked_ep1_id}/PlaybackInfo"))
                .header("X-Emby-Token", &jf_token)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(hls_pb_resp.status(), StatusCode::OK);
    let hls_pb: serde_json::Value = serde_json::from_slice(
        &to_bytes(hls_pb_resp.into_body(), 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    let hls_url = hls_pb["MediaSources"][0]["TranscodingUrl"]
        .as_str()
        .unwrap();
    assert!(hls_url.contains("/hls/master.m3u8"));

    let m3u8_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/jellyfin{hls_url}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(m3u8_resp.status(), StatusCode::OK);
    let m3u8_text = String::from_utf8(
        to_bytes(m3u8_resp.into_body(), 1024 * 1024)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    let seg_rel_line = m3u8_text
        .lines()
        .find(|l| l.starts_with("proxy?url="))
        .expect("rewritten relative proxy segment URL");

    let seg_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/jellyfin/Videos/{}/hls/{seg_rel_line}",
                    tracked_ep1_id.replace('-', "")
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(seg_resp.status(), StatusCode::OK);
    let seg_bytes = to_bytes(seg_resp.into_body(), 1024 * 1024).await.unwrap();
    assert_eq!(seg_bytes.as_ref(), ts_seg.as_slice());

    let unauth_img = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/jellyfin/Items/{tracked_movie_id}/Images/Primary"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauth_img.status(), StatusCode::UNAUTHORIZED);

    let tagged_img = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/jellyfin/Items/{tracked_movie_id}/Images/Primary?tag={tracked_movie_poster}"
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(tagged_img.status(), StatusCode::OK);

    let prog_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/jellyfin/Sessions/Playing/Progress")
                .header("X-Emby-Token", &jf_token)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "ItemId": tracked_ep1_id,
                        "PositionTicks": 1_250_000_000i64
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(prog_resp.status(), StatusCode::NO_CONTENT);

    let fav_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/jellyfin/Users/{jf_user_id}/FavoriteItems/{tracked_show_id}"
                ))
                .header("X-Emby-Token", &jf_token)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(fav_resp.status(), StatusCode::OK);

    let resume_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/jellyfin/Users/{jf_user_id}/Items/Resume"))
                .header("X-Emby-Token", &jf_token)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resume_resp.status(), StatusCode::OK);
    let resume_json: serde_json::Value = serde_json::from_slice(
        &to_bytes(resume_resp.into_body(), 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(resume_json["TotalRecordCount"], 1);
    assert_eq!(
        resume_json["Items"][0]["UserData"]["PlaybackPositionTicks"],
        1_250_000_000i64
    );

    let (st_native, native_detail) = send_json_with_method(
        &app,
        "GET",
        &format!("/api/v1/video/videos/{tracked_ep1_id}"),
        &admin_session.token,
        None,
    )
    .await;
    assert_eq!(st_native, StatusCode::OK);
    assert_eq!(native_detail["video"]["progress"]["position_secs"], 125.0);

    let (st_native_show, native_show) = send_json_with_method(
        &app,
        "GET",
        &format!("/api/v1/video/shows/{tracked_show_id}"),
        &admin_session.token,
        None,
    )
    .await;
    assert_eq!(st_native_show, StatusCode::OK);
    assert_eq!(native_show["show"]["is_favorite"], true);

    srv_handle.abort();
}

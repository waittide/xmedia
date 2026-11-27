use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use axum::routing::get;
use tempfile::TempDir;
use toasty::Db;
use toasty::schema::ModelSet;
use tower::ServiceExt;
use xmedia_core::application::auth::AuthService;
use xmedia_core::application::dto::{CreateUserInput, SetupAdminInput};
use xmedia_core::application::library::LibraryService;
use xmedia_core::domain::user::UserRole;
use xmedia_core::infra::events::BroadcastEventHub;
use xmedia_core::infra::ffmpeg::FfmpegToolkit;
use xmedia_core::infra::persistence::repositories::{
    ToastyLibraryRepository, ToastyUserRepository,
};
use xmedia_core::infra::security::{JwtTokenService, SecurityManager};
use xmedia_core::{ensure_core_schema, register_core_models};

use super::{PNG_1X1, send_json, setup_online_test_harness};
use crate::http::rest_router;
use crate::http::state::MusicHttpState;
use crate::{ensure_music_schema, init_music_module_with_allow_private_ips, register_music_models};

#[tokio::test]
async fn online_music_items_favorites_progress_and_remote_e2e() {
    let h = setup_online_test_harness(true).await;

    let src_json = serde_json::json!({
        "sourceName": "Test Music Source",
        "sourceUrl": "https://music-item.test",
        "sourceGroup": "Test",
        "ruleSearch": { "trackList": "$.items[*]", "title": "$.t", "trackId": "$.id" }
    });
    let (st_imp, _, b_imp) = send_json(
        &h.app,
        "POST",
        "/sources/import",
        &h.admin_token,
        Some(serde_json::json!({ "raw_json": src_json.to_string() })),
    )
    .await;
    assert_eq!(st_imp, StatusCode::OK);
    let imp_val: serde_json::Value = serde_json::from_slice(&b_imp).unwrap();
    let source_id = imp_val["sources"][0]["id"].as_str().unwrap().to_string();

    let (st_put, _, b_put) = send_json(
        &h.app,
        "PUT",
        "/online/items",
        &h.member_token,
        Some(serde_json::json!({
            "source_id": source_id,
            "item_key": "track-77",
            "title": "Nocturne Op.9 No.2",
            "subtitle": "Chopin",
            "cover_url": "https://music-item.test/cover.jpg",
            "extra": {
                "album": "Nocturnes",
                "play_url": "https://music-item.test/stream/77",
                "lyric_id": "lrc-77"
            },
            "favorite": true,
            "progress": {
                "position": 65.5,
                "duration": 262.0,
                "completed": false
            }
        })),
    )
    .await;
    assert_eq!(st_put, StatusCode::OK);
    let track: serde_json::Value = serde_json::from_slice(&b_put).unwrap();
    let track_item_id = track["id"].as_str().unwrap().to_string();
    assert_eq!(track["media_kind"], "music");
    assert_eq!(track["source_name"], "Test Music Source");
    assert_eq!(track["availability"], "ok");
    assert_eq!(track["extra"]["album"], "Nocturnes");
    assert!((track["progress_ratio"].as_f64().unwrap() - 0.25).abs() < 1e-6);

    let (st_sub, _, b_sub) = send_json(
        &h.app,
        "PUT",
        "/online/items",
        &h.member_token,
        Some(serde_json::json!({
            "source_kind": "remote",
            "source_id": "subsonic-nas",
            "item_key": "song-900",
            "title": "Clair de Lune",
            "subtitle": "Debussy",
            "cover_url": "/_app/subsonic/subsonic-nas/cover/al-9",
            "favorite": true
        })),
    )
    .await;
    assert_eq!(st_sub, StatusCode::OK);
    let sub_item: serde_json::Value = serde_json::from_slice(&b_sub).unwrap();
    assert_eq!(sub_item["availability"], "unverified");
    assert!(sub_item["source_name"].is_null());

    let (st_lk, _, b_lk) = send_json(
        &h.app,
        "POST",
        "/online/items/lookup",
        &h.member_token,
        Some(serde_json::json!({
            "items": [
                { "source_id": source_id, "item_key": "track-77" },
                { "source_kind": "remote", "source_id": "subsonic-nas", "item_key": "song-900" }
            ]
        })),
    )
    .await;
    assert_eq!(st_lk, StatusCode::OK);
    let lk: serde_json::Value = serde_json::from_slice(&b_lk).unwrap();
    assert_eq!(lk["total"], 2);

    let (st_cont, _, b_cont) = send_json(
        &h.app,
        "GET",
        "/online/items?view=continue",
        &h.member_token,
        None,
    )
    .await;
    assert_eq!(st_cont, StatusCode::OK);
    let cont: serde_json::Value = serde_json::from_slice(&b_cont).unwrap();
    assert_eq!(cont["total"], 1);
    assert_eq!(cont["items"][0]["id"], track_item_id);

    let (st_rem_list, _, b_rem_list) = send_json(
        &h.app,
        "GET",
        "/online/items?source_kind=remote&view=favorites",
        &h.member_token,
        None,
    )
    .await;
    assert_eq!(st_rem_list, StatusCode::OK);
    let rem_list: serde_json::Value = serde_json::from_slice(&b_rem_list).unwrap();
    assert_eq!(rem_list["total"], 1);
    assert_eq!(rem_list["items"][0]["source_id"], "subsonic-nas");

    let (st_prog, _, b_prog) = send_json(
        &h.app,
        "PUT",
        &format!("/online/items/{track_item_id}/progress"),
        &h.member_token,
        Some(serde_json::json!({
            "position": 262.0,
            "duration": 262.0,
            "completed": true
        })),
    )
    .await;
    assert_eq!(st_prog, StatusCode::OK);
    let p_val: serde_json::Value = serde_json::from_slice(&b_prog).unwrap();
    assert_eq!(p_val["progress"]["completed"], true);

    let (st_cont2, _, b_cont2) = send_json(
        &h.app,
        "GET",
        "/online/items?view=continue",
        &h.member_token,
        None,
    )
    .await;
    assert_eq!(st_cont2, StatusCode::OK);
    let cont2: serde_json::Value = serde_json::from_slice(&b_cont2).unwrap();
    assert_eq!(cont2["total"], 0);

    let (st_del, _, _) = send_json(
        &h.app,
        "DELETE",
        &format!("/online/items/{track_item_id}"),
        &h.member_token,
        None,
    )
    .await;
    assert_eq!(st_del, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn online_tracked_music_hybrid_cache_and_opensubsonic_e2e() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("xmedia_music_tracked.db");
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
    let security = Arc::new(SecurityManager::new("music-tracked-secret"));
    let tokens = Arc::new(JwtTokenService::new("music-tracked-secret", 3600));

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
    let music_services = init_music_module_with_allow_private_ips(
        db,
        tmp.path(),
        FfmpegToolkit::default(),
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
    auth_svc
        .create_user(
            &admin,
            CreateUserInput {
                username: "member".into(),
                display_name: Some("Member".into()),
                password: "memberpassword123".into(),
                role: UserRole::Member,
            },
        )
        .await
        .unwrap();
    let member_session = auth_svc
        .login("member", "memberpassword123", None)
        .await
        .unwrap();
    let api_key = admin.api_key.clone();

    let state = MusicHttpState::new(auth_svc, library_svc, music_services);
    let app = Router::new()
        .nest("/api/v1/music", rest_router(state.clone()))
        .nest("/rest", crate::http::subsonic_router(state));

    let audio_hits = Arc::new(AtomicUsize::new(0));
    let audio_hits_srv = Arc::clone(&audio_hits);
    let wav_bytes = Arc::new(super::super::make_wav(16_000));
    let wav_for_srv = Arc::clone(&wav_bytes);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");
    let base_url_srv = base_url.clone();

    let mock_router = Router::new()
        .route(
            "/stream/{id}",
            get(
                move |axum::extract::Path(id): axum::extract::Path<String>| {
                    let b = base_url_srv.clone();
                    async move {
                        let payload = serde_json::json!({
                            "url": format!("{b}/audio/{id}.wav"),
                            "format": "wav",
                            "br": 256
                        });
                        (
                            StatusCode::OK,
                            [(header::CONTENT_TYPE, "application/json")],
                            payload.to_string(),
                        )
                    }
                },
            ),
        )
        .route(
            "/audio/{file}",
            get(move || {
                let hits = Arc::clone(&audio_hits_srv);
                let data = Arc::clone(&wav_for_srv);
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    (
                        StatusCode::OK,
                        [(header::CONTENT_TYPE, "audio/wav")],
                        (*data).clone(),
                    )
                }
            }),
        )
        .route(
            "/lyric/{id}",
            get(|| async {
                let payload = serde_json::json!({
                    "lyric": "[00:00.50]海风吹过\n[00:03.20]星辰坠落\n"
                });
                (
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, "application/json")],
                    payload.to_string(),
                )
            }),
        )
        .route(
            "/cover.png",
            get(|| async {
                (
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, "image/png")],
                    PNG_1X1.to_vec(),
                )
            }),
        );

    let server_handle = tokio::spawn(async move {
        axum::serve(listener, mock_router).await.unwrap();
    });

    let source_spec = serde_json::json!({
        "sourceName": "Mock Tracked Music Source",
        "sourceUrl": base_url,
        "enabled": true,
        "ruleStream": {
            "requestUrl": format!("{base_url}/stream/{{{{track.id}}}}"),
            "streamUrl": "$.url",
            "format": "$.format",
            "bitrate": "$.br"
        },
        "ruleLyric": {
            "requestUrl": format!("{base_url}/lyric/{{{{track.id}}}}"),
            "lyric": "$.lyric"
        }
    });
    let (st_imp, _, b_imp) = send_json(
        &app,
        "POST",
        "/api/v1/music/sources/import",
        &admin_session.token,
        Some(serde_json::json!({ "raw_json": source_spec.to_string() })),
    )
    .await;
    assert_eq!(
        st_imp,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&b_imp)
    );
    let imp_val: serde_json::Value = serde_json::from_slice(&b_imp).unwrap();
    let source_id = imp_val["sources"][0]["id"].as_str().unwrap().to_string();

    let track_payload = serde_json::json!({
        "source_id": source_id,
        "track_id": "song-001",
        "title": "星海航行",
        "artist": "银河乐队",
        "album": "深空旅人",
        "duration_secs": 180,
        "cover_url": format!("{base_url}/cover.png"),
        "format": "wav"
    });
    let (st_track, _, b_track) = send_json(
        &app,
        "POST",
        "/api/v1/music/online/track",
        &admin_session.token,
        Some(track_payload.clone()),
    )
    .await;
    assert_eq!(
        st_track,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&b_track)
    );
    let tracked: serde_json::Value = serde_json::from_slice(&b_track).unwrap();
    let track_id = tracked["id"].as_str().unwrap().to_string();
    assert_eq!(tracked["title"], "星海航行");
    assert_eq!(tracked["artist_name"], "银河乐队");
    assert_eq!(tracked["album_title"], "深空旅人");
    assert_eq!(tracked["online_tracking"]["cached"], false);
    assert!(tracked["cover_key"].as_str().is_some());
    assert_eq!(audio_hits.load(Ordering::SeqCst), 0);

    let (st_mem_404, _, _) = send_json(
        &app,
        "GET",
        &format!("/api/v1/music/tracks/{track_id}"),
        &member_session.token,
        None,
    )
    .await;
    assert_eq!(st_mem_404, StatusCode::NOT_FOUND);

    let (st_mem_cache_404, _, _) = send_json(
        &app,
        "POST",
        &format!("/api/v1/music/tracks/{track_id}/online/cache"),
        &member_session.token,
        None,
    )
    .await;
    assert_eq!(st_mem_cache_404, StatusCode::NOT_FOUND);

    let (st_pool, _, b_pool) = send_json(
        &app,
        "GET",
        "/api/v1/music/pool",
        &member_session.token,
        None,
    )
    .await;
    assert_eq!(st_pool, StatusCode::OK);
    let pool_val: serde_json::Value = serde_json::from_slice(&b_pool).unwrap();
    assert_eq!(pool_val["total"], 0);

    let (st_mem_track, _, b_mem_track) = send_json(
        &app,
        "POST",
        "/api/v1/music/online/track",
        &member_session.token,
        Some(track_payload.clone()),
    )
    .await;
    assert_eq!(st_mem_track, StatusCode::OK);
    let mem_tracked: serde_json::Value = serde_json::from_slice(&b_mem_track).unwrap();
    assert_eq!(mem_tracked["id"], track_id);

    let (st_dup, _, b_dup) = send_json(
        &app,
        "POST",
        "/api/v1/music/online/track",
        &admin_session.token,
        Some(track_payload),
    )
    .await;
    assert_eq!(st_dup, StatusCode::OK);
    let dup: serde_json::Value = serde_json::from_slice(&b_dup).unwrap();
    assert_eq!(dup["id"], track_id);

    let search_req = Request::builder()
        .method("GET")
        .uri(format!(
            "/rest/search3.view?f=json&v=1.16.1&c=test&apiKey={api_key}&query=%E6%98%9F%E6%B5%B7"
        ))
        .body(Body::empty())
        .unwrap();
    let search_resp = app.clone().oneshot(search_req).await.unwrap();
    assert_eq!(search_resp.status(), StatusCode::OK);
    let search_bytes = to_bytes(search_resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let search_json: serde_json::Value = serde_json::from_slice(&search_bytes).unwrap();
    let songs = search_json["subsonic-response"]["searchResult3"]["song"]
        .as_array()
        .unwrap();
    assert_eq!(songs.len(), 1);
    assert_eq!(songs[0]["id"], track_id);
    assert_eq!(songs[0]["title"], "星海航行");

    let lyr_req = Request::builder()
        .method("GET")
        .uri(format!(
            "/rest/getLyricsBySongId.view?f=json&v=1.16.1&c=test&apiKey={api_key}&id={track_id}"
        ))
        .body(Body::empty())
        .unwrap();
    let lyr_resp = app.clone().oneshot(lyr_req).await.unwrap();
    assert_eq!(lyr_resp.status(), StatusCode::OK);
    let lyr_bytes = to_bytes(lyr_resp.into_body(), 1024 * 1024).await.unwrap();
    let lyr_json: serde_json::Value = serde_json::from_slice(&lyr_bytes).unwrap();
    let structured = lyr_json["subsonic-response"]["lyricsList"]["structuredLyrics"]
        .as_array()
        .unwrap();
    assert_eq!(structured.len(), 1);
    assert_eq!(structured[0]["synced"], true);
    assert_eq!(audio_hits.load(Ordering::SeqCst), 0);

    let stream_req = Request::builder()
        .method("GET")
        .uri(format!(
            "/rest/stream.view?v=1.16.1&c=test&apiKey={api_key}&id={track_id}&format=raw"
        ))
        .body(Body::empty())
        .unwrap();
    let stream_resp = app.clone().oneshot(stream_req).await.unwrap();
    assert_eq!(stream_resp.status(), StatusCode::OK);
    let stream_bytes = to_bytes(stream_resp.into_body(), 10 * 1024 * 1024)
        .await
        .unwrap();
    assert_eq!(stream_bytes.as_ref(), wav_bytes.as_slice());
    assert_eq!(audio_hits.load(Ordering::SeqCst), 1);

    let (st_get, _, b_get) = send_json(
        &app,
        "GET",
        &format!("/api/v1/music/tracks/{track_id}"),
        &admin_session.token,
        None,
    )
    .await;
    assert_eq!(st_get, StatusCode::OK);
    let refreshed: serde_json::Value = serde_json::from_slice(&b_get).unwrap();
    assert_eq!(refreshed["online_tracking"]["cached"], true);
    assert_eq!(refreshed["file_size"], wav_bytes.len() as u64);

    let stream_req2 = Request::builder()
        .method("GET")
        .uri(format!(
            "/rest/stream.view?v=1.16.1&c=test&apiKey={api_key}&id={track_id}&format=raw"
        ))
        .body(Body::empty())
        .unwrap();
    let stream_resp2 = app.clone().oneshot(stream_req2).await.unwrap();
    assert_eq!(stream_resp2.status(), StatusCode::OK);
    let stream_bytes2 = to_bytes(stream_resp2.into_body(), 10 * 1024 * 1024)
        .await
        .unwrap();
    assert_eq!(stream_bytes2.as_ref(), wav_bytes.as_slice());
    assert_eq!(audio_hits.load(Ordering::SeqCst), 1);

    let (st_batch, _, b_batch) = send_json(
        &app,
        "POST",
        "/api/v1/music/online/track-batch",
        &admin_session.token,
        Some(serde_json::json!({
            "tracks": [{
                "source_id": source_id,
                "track_id": "song-002",
                "title": "星际漫游",
                "artist": "银河乐队",
                "album": "深空旅人",
                "format": "wav"
            }]
        })),
    )
    .await;
    assert_eq!(st_batch, StatusCode::OK);
    let batch_list: serde_json::Value = serde_json::from_slice(&b_batch).unwrap();
    let track2_id = batch_list[0]["id"].as_str().unwrap().to_string();
    assert_eq!(batch_list[0]["online_tracking"]["cached"], false);

    let (st_cache, _, b_cache) = send_json(
        &app,
        "POST",
        &format!("/api/v1/music/tracks/{track2_id}/online/cache"),
        &admin_session.token,
        None,
    )
    .await;
    assert_eq!(st_cache, StatusCode::OK);
    let cached2: serde_json::Value = serde_json::from_slice(&b_cache).unwrap();
    assert_eq!(cached2["online_tracking"]["cached"], true);
    assert_eq!(audio_hits.load(Ordering::SeqCst), 2);

    let (st_unb_m, _, b_unb_m) = send_json(
        &app,
        "DELETE",
        &format!("/api/v1/music/bindings/{track_id}"),
        &member_session.token,
        None,
    )
    .await;
    assert_eq!(st_unb_m, StatusCode::OK);
    let unb_m: serde_json::Value = serde_json::from_slice(&b_unb_m).unwrap();
    assert_eq!(unb_m["unbound"], true);
    assert_eq!(unb_m["remaining_bindings"], 1);
    assert_eq!(unb_m["deleted_from_pool"], false);

    let (st_unb_a, _, b_unb_a) = send_json(
        &app,
        "DELETE",
        &format!("/api/v1/music/bindings/{track_id}"),
        &admin_session.token,
        None,
    )
    .await;
    assert_eq!(st_unb_a, StatusCode::OK);
    let unb_a: serde_json::Value = serde_json::from_slice(&b_unb_a).unwrap();
    assert_eq!(unb_a["unbound"], true);
    assert_eq!(unb_a["remaining_bindings"], 0);
    assert_eq!(unb_a["deleted_from_pool"], true);

    server_handle.abort();
}

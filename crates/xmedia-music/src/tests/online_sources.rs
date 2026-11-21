use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::{Query, Request as AxumRequest};
use axum::http::{HeaderMap, Request, StatusCode, header};
use axum::response::Response;
use axum::routing::get;
use bytes::Bytes;
use serde::Deserialize;
use tempfile::TempDir;
use toasty::Db;
use toasty::schema::ModelSet;
use tower::ServiceExt;
use uuid::Uuid;
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
use xmedia_source::crud::MASKED_SECRET_PLACEHOLDER;
use xmedia_source::http::HttpClientConfig;
use xmedia_source::model::SourceRuntimeState;
use xmedia_source::proxy::IMAGE_PROXY_CSP;

use crate::domain::catalog::StructuredLyrics;
use crate::http::rest_router;
use crate::http::state::MusicHttpState;
use crate::infra::lyrics::parse_lyrics_str;
use crate::source::MusicSourceEngine;
use crate::source::engine::{
    infer_audio_format, normalize_artist, parse_duration_secs, url_encode_component,
};
use crate::source::model::{MusicSourceSpec, MusicTrackContext};
use crate::{
    ensure_music_schema, init_music_module, init_music_module_with_allow_private_ips,
    register_music_models,
};

const SEEDS_MUSIC_JSON: &str = include_str!("../../../../seeds/sources/music.json");

const PNG_1X1: &[u8] = &[
    0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, b'I', b'H', b'D', b'R',
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0A, b'I', b'D', b'A', b'T', 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, b'I', b'E', b'N', b'D', 0xAE,
    0x42, 0x60, 0x82,
];

struct OnlineTestHarness {
    _tmp: TempDir,
    app: Router,
    admin_token: String,
    member_token: String,
}

async fn setup_online_test_harness(allow_private_ips: bool) -> OnlineTestHarness {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("xmedia_music_online.db");
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
    let security = Arc::new(SecurityManager::new("music-online-secret"));
    let tokens = Arc::new(JwtTokenService::new("music-online-secret", 3600));

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

    let music_services = if allow_private_ips {
        init_music_module_with_allow_private_ips(
            db,
            tmp.path(),
            FfmpegToolkit::default(),
            libraries_repo,
            &library_svc,
            events,
            true,
        )
        .unwrap()
    } else {
        init_music_module(
            db,
            tmp.path(),
            FfmpegToolkit::default(),
            libraries_repo,
            &library_svc,
            events,
        )
        .unwrap()
    };

    let admin_session = auth_svc
        .setup_initial_admin(SetupAdminInput {
            username: "admin".into(),
            display_name: Some("Admin".into()),
            password: "adminpassword123".into(),
        })
        .await
        .unwrap();
    let admin = auth_svc
        .authenticate_jwt(&admin_session.token)
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
    let member_session = auth_svc.login("member", "memberpassword123").await.unwrap();

    let state = MusicHttpState::new(auth_svc, library_svc, music_services);
    let app = rest_router(state);

    OnlineTestHarness {
        _tmp: tmp,
        app,
        admin_token: admin_session.token,
        member_token: member_session.token,
    }
}

async fn send_json(
    app: &Router,
    method: &str,
    uri: &str,
    token: &str,
    body: Option<serde_json::Value>,
) -> (StatusCode, HeaderMap, Vec<u8>) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"));
    let req_body = if let Some(val) = body {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
        Body::from(serde_json::to_vec(&val).unwrap())
    } else {
        Body::empty()
    };
    let resp = app
        .clone()
        .oneshot(builder.body(req_body).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let headers = resp.headers().clone();
    let bytes = to_bytes(resp.into_body(), 64 * 1024 * 1024).await.unwrap();
    (status, headers, bytes.to_vec())
}

#[test]
fn seeds_music_json_parses_cleanly_and_helpers_behave_correctly() {
    let parsed = MusicSourceSpec::parse_json_batch(SEEDS_MUSIC_JSON).unwrap();
    assert_eq!(parsed.len(), 4);
    assert_eq!(parsed[0].0.source_name, "GDStudio Music API (网易云)");
    assert_eq!(
        parsed[0].0.supported_qualities,
        vec!["128", "192", "320", "740", "999"]
    );
    assert_eq!(parsed[1].0.source_name, "Audius Public API");
    assert_eq!(parsed[2].0.source_name, "Internet Archive Audio");
    assert_eq!(parsed[3].0.source_name, "iTunes Search + LRCLIB");

    // Lenient deserialization: capitalized Header, object vs string, CSV supportedQualities, bool/int coercion
    let custom_raw = r#"{
        "name": "Custom",
        "url": "https://example.test",
        "enabled": "false",
        "weight": "42",
        "Header": {"User-Agent": "CustomUA"},
        "mediaHeader": {"Referer": "https://media.test/"},
        "supportedQualities": "128, 320 | flac"
    }"#;
    let custom_batch = MusicSourceSpec::parse_json_batch(custom_raw).unwrap();
    let spec = &custom_batch[0].0;
    assert!(!spec.enabled);
    assert_eq!(spec.weight, 42);
    assert_eq!(spec.supported_qualities, vec!["128", "320", "flac"]);
    let merged_hdr = spec.parse_media_headers();
    assert_eq!(
        merged_hdr.get("User-Agent").map(String::as_str),
        Some("CustomUA")
    );
    assert_eq!(
        merged_hdr.get("Referer").map(String::as_str),
        Some("https://media.test/")
    );

    // Helper functions
    assert_eq!(
        url_encode_component("R&B #1 中文"),
        "R%26B%20%231%20%E4%B8%AD%E6%96%87"
    );
    assert_eq!(
        normalize_artist("  Artist A \n\n Artist B  "),
        "Artist A / Artist B"
    );
    assert_eq!(parse_duration_secs("35:57", None), Some(2157));
    assert_eq!(parse_duration_secs("1:02:03", None), Some(3723));
    assert_eq!(parse_duration_secs("225868", Some("ms")), Some(226));
    assert_eq!(parse_duration_secs("212", Some("s")), Some(212));
    assert_eq!(parse_duration_secs("3433.45", Some("auto")), Some(3433));
    assert_eq!(parse_duration_secs("180000", Some("auto")), Some(180));

    assert_eq!(
        infer_audio_format("https://cdn.example.com/track/song.MP3?token=abc#v1"),
        Some("mp3".into())
    );
    assert_eq!(
        infer_audio_format("https://cdn.example.com/track/song.m4a"),
        Some("m4a".into())
    );
    assert_eq!(
        infer_audio_format("https://cdn.example.com/track/noext"),
        None
    );

    let lrc = parse_lyrics_str("[ti:Mojito]\n[ar:周杰伦]\n[00:01.20]麻烦给我的爱人来一杯Mojito\n");
    assert!(lrc.synced);
    assert_eq!(lrc.display_title.as_deref(), Some("Mojito"));
    assert_eq!(lrc.display_artist.as_deref(), Some("周杰伦"));
    assert_eq!(lrc.lines.len(), 1);
    assert_eq!(lrc.lines[0].start_ms, 1200);
}

#[tokio::test]
async fn engine_replays_all_four_seed_protocols_login_and_error_handling() {
    #[derive(Deserialize)]
    struct GdQuery {
        types: Option<String>,
        id: Option<String>,
        br: Option<String>,
    }

    let app = Router::new()
        // GDStudio mock
        .route(
            "/gd/api.php",
            get(|Query(q): Query<GdQuery>| async move {
                match q.types.as_deref() {
                    Some("search") => axum::Json(serde_json::json!([
                        {
                            "id": "2736327941",
                            "name": "Mojito (Live)",
                            "artist": ["孙焱彬", "周杰伦"],
                            "album": "Live Collection",
                            "pic_id": "109951171867660113",
                            "url_id": "2736327941",
                            "lyric_id": "2736327941"
                        }
                    ])),
                    Some("url") => axum::Json(serde_json::json!({
                        "url": format!("http://127.0.0.1:9/audio/{}.mp3?br={}", q.id.unwrap_or_default(), q.br.as_deref().unwrap_or("320")),
                        "br": 320,
                        "size": 21222966
                    })),
                    Some("pic") => axum::Json(serde_json::json!({
                        "url": "https://p2.music.126.net/demo.jpg?param=500y500"
                    })),
                    Some("lyric") => axum::Json(serde_json::json!({
                        "lyric": "[00:00.00]Mojito\n[00:12.50]麻烦给我的爱人来一杯Mojito",
                        "tlyric": "[00:12.50]Please get my lover a Mojito"
                    })),
                    _ => axum::Json(serde_json::json!({})),
                }
            }),
        )
        // Audius mock
        .route(
            "/v1/tracks/search",
            get(|| async {
                axum::Json(serde_json::json!({
                    "data": [{
                        "id": "V66vyPP",
                        "title": "Xsetra - Fragments",
                        "duration": 212,
                        "genre": "Drum & Bass",
                        "user": { "name": "Xsetra" },
                        "artwork": { "480x480": "https://cdn.audius.test/480.jpg" }
                    }]
                }))
            }),
        )
        .route(
            "/v1/tracks/trending",
            get(|| async {
                axum::Json(serde_json::json!({
                    "data": [{
                        "id": "TREND1",
                        "title": "Trending Beat",
                        "duration": 180,
                        "genre": "Electronic",
                        "user": { "name": "DJ Demo" },
                        "artwork": { "150x150": "https://cdn.audius.test/150.jpg" }
                    }]
                }))
            }),
        )
        .route(
            "/v1/tracks/{id}",
            get(|| async {
                axum::Json(serde_json::json!({
                    "data": {
                        "id": "V66vyPP",
                        "title": "Xsetra - Fragments (Full)",
                        "duration": 212,
                        "genre": "Drum & Bass",
                        "user": { "name": "Xsetra" },
                        "artwork": { "480x480": "https://cdn.audius.test/480.jpg" }
                    }
                }))
            }),
        )
        // Internet Archive mock
        .route(
            "/advancedsearch.php",
            get(|| async {
                axum::Json(serde_json::json!({
                    "response": {
                        "docs": [
                            {
                                "identifier": "bndf_20250623",
                                "title": "BEETHOVEN Klavierkonzert Nr. 3"
                            },
                            {
                                "identifier": "danutz-22",
                                "title": "BEETHOVEN IN 22 CENTURY",
                                "creator": "DANUTZ"
                            }
                        ]
                    }
                }))
            }),
        )
        .route(
            "/metadata/bndf_20250623",
            get(|| async {
                axum::Json(serde_json::json!({
                    "metadata": {
                        "identifier": "bndf_20250623",
                        "title": "BEETHOVEN Klavierkonzert Nr. 3",
                        "creator": ["Ludwig van Beethoven", "Orchestra"]
                    },
                    "files": [
                        { "name": "secret.mp3", "private": "true", "length": "01:00", "bitrate": "128" },
                        { "name": "subdir/track 8.mp3", "format": "VBR MP3", "bitrate": "136", "length": "35:57" }
                    ]
                }))
            }),
        )
        // iTunes + LRCLIB mock
        .route(
            "/search",
            get(|| async {
                axum::Json(serde_json::json!({
                    "results": [{
                        "trackId": 6814997425i64,
                        "trackName": "Shake It Off",
                        "artistName": "Taylor Swift",
                        "collectionName": "1989",
                        "previewUrl": "https://audio-ssl.itunes.apple.com/preview.m4a",
                        "artworkUrl100": "https://is1-ssl.mzstatic.com/image/100x100bb.jpg",
                        "trackTimeMillis": 225868
                    }]
                }))
            }),
        )
        .route(
            "/lookup",
            get(|| async {
                axum::Json(serde_json::json!({
                    "results": [{
                        "trackId": 6814997425i64,
                        "trackName": "Shake It Off",
                        "artistName": "Taylor Swift",
                        "collectionName": "1989",
                        "previewUrl": "https://audio-ssl.itunes.apple.com/lookup-preview.m4a",
                        "artworkUrl100": "https://is1-ssl.mzstatic.com/image/100x100bb.jpg",
                        "trackTimeMillis": 225868
                    }]
                }))
            }),
        )
        .route(
            "/api/search",
            get(|| async {
                axum::Json(serde_json::json!([
                    {
                        "id": 1,
                        "syncedLyrics": "[00:00.98] I stay out too late\n[00:04.20] Got nothing in my brain",
                        "plainLyrics": "I stay out too late"
                    }
                ]))
            }),
        )
        .route(
            "/error-500",
            get(|| async { (StatusCode::INTERNAL_SERVER_ERROR, "upstream error") }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let base = format!("http://{addr}");

    let engine = MusicSourceEngine::new(
        HttpClientConfig {
            timeout_secs: 5,
            allow_private_ips: true,
            proxy_url: None,
        },
        Duration::from_secs(10),
    );

    // Prepare rewritten seed specs pointing to local mock server
    let mut specs: Vec<MusicSourceSpec> = MusicSourceSpec::parse_json_batch(SEEDS_MUSIC_JSON)
        .unwrap()
        .into_iter()
        .map(|(s, _)| s)
        .collect();

    tokio::task::spawn_blocking(move || {
        // 1. GDStudio replay
    {
        let gd = &mut specs[0];
        gd.source_url = base.clone();
        gd.search_url = Some(format!(
            "{base}/gd/api.php?types=search&source=netease&name={{{{key}}}}&count={{{{limit}}}}&pages={{{{page}}}}"
        ));
        gd.explore_url = Some(format!(
            "热歌榜::{base}/gd/api.php?types=search&source=netease&name=hot&count={{{{limit}}}}&pages={{{{page}}}}"
        ));
        gd.rule_stream.as_mut().unwrap().request_url = Some(format!(
            "{base}/gd/api.php?types=url&source=netease&id={{{{urlId}}}}&br={{{{quality}}}}"
        ));
        gd.rule_cover.as_mut().unwrap().request_url = Some(format!(
            "{base}/gd/api.php?types=pic&source=netease&id={{{{coverId}}}}&size=500"
        ));
        gd.rule_lyric.as_mut().unwrap().request_url = Some(format!(
            "{base}/gd/api.php?types=lyric&source=netease&id={{{{lyricId}}}}"
        ));

        let s_out = engine
            .search_sync(gd, SourceRuntimeState::default(), "Mojito", 1, 10)
            .unwrap();
        assert_eq!(s_out.data.len(), 1);
        assert_eq!(s_out.data[0].artist, "孙焱彬 / 周杰伦");
        let ctx = s_out.data[0].to_context(Some("320".into()));

        let stream_out = engine
            .resolve_stream_sync(gd, SourceRuntimeState::default(), &ctx, Some("320"))
            .unwrap();
        assert!(stream_out.data.stream_url.contains("br=320"));
        assert_eq!(stream_out.data.format, "mp3");
        assert_eq!(stream_out.data.bitrate, Some(320));
        assert_eq!(stream_out.data.expires_in_secs, Some(1200));

        let cover_out = engine
            .resolve_cover_sync(gd, SourceRuntimeState::default(), &ctx)
            .unwrap();
        assert_eq!(
            cover_out.data.as_deref(),
            Some("https://p2.music.126.net/demo.jpg?param=500y500")
        );

        let lrc_out = engine
            .resolve_lyrics_sync(gd, SourceRuntimeState::default(), &ctx)
            .unwrap()
            .data
            .unwrap();
        assert!(lrc_out.lyrics.synced);
        assert_eq!(lrc_out.lyrics.lines.len(), 2);
        assert!(lrc_out.translated_lyrics.is_some());
    }

    // 2. Audius replay (requestUrl: None stream evaluation)
    {
        let audius = &mut specs[1];
        audius.source_url = base.clone();
        audius.search_url = Some(format!(
            "{base}/v1/tracks/search?query={{{{key}}}}&limit={{{{limit}}}}"
        ));
        audius.explore_url = Some(format!(
            "热门::{base}/v1/tracks/trending?limit={{{{limit}}}}"
        ));
        audius.rule_track_detail.as_mut().unwrap().request_url =
            Some(format!("{base}/v1/tracks/{{{{trackId}}}}"));

        let s_out = engine
            .search_sync(audius, SourceRuntimeState::default(), "fragments", 1, 10)
            .unwrap();
        assert_eq!(s_out.data[0].duration_secs, Some(212));
        let exp_out = engine
            .explore_sync(audius, SourceRuntimeState::default(), None, 1, 10)
            .unwrap();
        assert_eq!(exp_out.data.tracks[0].track_id, "TREND1");

        let detail_out = engine
            .fetch_track_detail_sync(
                audius,
                SourceRuntimeState::default(),
                &s_out.data[0].to_context(None),
            )
            .unwrap();
        assert_eq!(detail_out.data.title, "Xsetra - Fragments (Full)");

        let stream_out = engine
            .resolve_stream_sync(
                audius,
                SourceRuntimeState::default(),
                &detail_out.data.to_context(None),
                None,
            )
            .unwrap();
        assert_eq!(
            stream_out.data.stream_url,
            "https://api.audius.co/v1/tracks/V66vyPP/stream?app_name=xmedia"
        );
        assert_eq!(stream_out.data.format, "mp3");
        assert_eq!(stream_out.data.bitrate, Some(320));
    }

    // 3. Internet Archive Audio replay (@js: private filtering + encoded path + mm:ss duration)
    {
        let ia = &mut specs[2];
        ia.source_url = base.clone();
        ia.search_url = Some(format!("{base}/advancedsearch.php?q={{{{key}}}}"));
        ia.rule_track_detail.as_mut().unwrap().request_url =
            Some(format!("{base}/metadata/{{{{trackId}}}}"));
        ia.rule_stream.as_mut().unwrap().request_url =
            Some(format!("{base}/metadata/{{{{urlId}}}}"));

        let s_out = engine
            .search_sync(ia, SourceRuntimeState::default(), "beethoven", 1, 10)
            .unwrap();
        assert_eq!(s_out.data.len(), 2);
        assert_eq!(s_out.data[0].artist, "");
        assert_eq!(s_out.data[1].artist, "DANUTZ");

        let detail_out = engine
            .fetch_track_detail_sync(
                ia,
                SourceRuntimeState::default(),
                &s_out.data[0].to_context(None),
            )
            .unwrap();
        assert_eq!(detail_out.data.duration_secs, Some(2157));
        assert_eq!(detail_out.data.artist, "Ludwig van Beethoven / Orchestra");

        let stream_out = engine
            .resolve_stream_sync(
                ia,
                SourceRuntimeState::default(),
                &detail_out.data.to_context(None),
                None,
            )
            .unwrap();
        assert_eq!(
            stream_out.data.stream_url,
            "https://archive.org/download/bndf_20250623/subdir/track%208.mp3"
        );
        assert_eq!(stream_out.data.bitrate, Some(136));
    }

    // 4. iTunes + LRCLIB replay
    {
        let itunes = &mut specs[3];
        itunes.source_url = base.clone();
        itunes.search_url = Some(format!("{base}/search?term={{{{key}}}}"));
        itunes.rule_track_detail.as_mut().unwrap().request_url =
            Some(format!("{base}/lookup?id={{{{trackId}}}}"));
        itunes.rule_stream.as_mut().unwrap().request_url =
            Some(format!("{base}/lookup?id={{{{trackId}}}}"));
        itunes.rule_lyric.as_mut().unwrap().request_url =
            Some(format!("{base}/api/search?q={{{{title}}}} {{{{artist}}}}"));

        let s_out = engine
            .search_sync(itunes, SourceRuntimeState::default(), "Taylor Swift", 1, 10)
            .unwrap();
        assert_eq!(s_out.data[0].duration_secs, Some(226));
        assert_eq!(
            s_out.data[0].cover_url.as_deref(),
            Some("https://is1-ssl.mzstatic.com/image/600x600bb.jpg")
        );

        // When play_url is None, falls back to /lookup
        let mut ctx_no_play = s_out.data[0].to_context(None);
        ctx_no_play.play_url = None;
        let stream_out = engine
            .resolve_stream_sync(itunes, SourceRuntimeState::default(), &ctx_no_play, None)
            .unwrap();
        assert_eq!(
            stream_out.data.stream_url,
            "https://audio-ssl.itunes.apple.com/lookup-preview.m4a"
        );
        assert_eq!(stream_out.data.format, "m4a");

        let lrc_out = engine
            .resolve_lyrics_sync(
                itunes,
                SourceRuntimeState::default(),
                &s_out.data[0].to_context(None),
            )
            .unwrap()
            .data
            .unwrap();
        assert!(lrc_out.lyrics.synced);
        assert_eq!(lrc_out.lyrics.lines.len(), 2);
    }

    // 5. Login action & upstream error handling
    {
        let mut login_spec = MusicSourceSpec {
            source_name: "Login Test".into(),
            source_url: base.clone(),
            search_url: Some(format!("{base}/error-500")),
            login_url: Some(
                "function login() { source.setVariable('vip_' + result.user); java.toast('ok'); return 'signed_in'; }"
                    .into(),
            ),
            ..Default::default()
        };
        login_spec.rule_search = specs[0].rule_search.clone();
        let login_res = engine
            .run_login_action_sync(
                &login_spec,
                SourceRuntimeState::default(),
                HashMap::from([("user".into(), "alice".into())]),
                Some("login()"),
            )
            .unwrap();
        assert_eq!(login_res.data, "signed_in");
        assert_eq!(login_res.state.variable, "vip_alice");
        assert_eq!(login_res.diagnostics.toasts, vec!["ok"]);

        let err_res = engine.search_sync(&login_spec, SourceRuntimeState::default(), "test", 1, 5);
        assert!(err_res.is_err());

        // Missing track_id/play_url for stream fails gracefully
        let empty_ctx = MusicTrackContext::default();
        let missing_stream = engine.resolve_stream_sync(
            &login_spec,
            SourceRuntimeState::default(),
            &empty_ctx,
            None,
        );
        assert!(missing_stream.is_err());
    }
    })
    .await
    .unwrap();

    server.abort();
}

#[tokio::test]
async fn music_online_crud_masking_rbac_and_proxy_streaming_e2e() {
    let captured_headers: Arc<Mutex<HashMap<String, String>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let cap_clone = captured_headers.clone();

    let audio_payload: Vec<u8> = (0u8..120).collect();
    let audio_for_range = audio_payload.clone();
    let audio_for_chunked = audio_payload.clone();

    let mock_app = Router::new()
        .route(
            "/gd/search",
            get(|| async {
                axum::Json(serde_json::json!([{
                    "id": "track-1",
                    "name": "Test Song",
                    "artist": ["Artist One", "Artist Two"],
                    "album": "Test Album",
                    "pic_id": "pic-1",
                    "url_id": "url-1",
                    "lyric_id": "lrc-1"
                }]))
            }),
        )
        .route(
            "/gd/explore",
            get(|| async {
                axum::Json(serde_json::json!([{
                    "id": "exp-1",
                    "name": "Top Hit",
                    "artist": ["Chart Artist"],
                    "album": "Charts",
                    "pic_id": "pic-1",
                    "url_id": "url-1",
                    "lyric_id": "lrc-1"
                }]))
            }),
        )
        .route(
            "/gd/url",
            get(|headers: HeaderMap| async move {
                let host = headers
                    .get(header::HOST)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("127.0.0.1");
                axum::Json(serde_json::json!({
                    "url": format!("http://{host}/audio/song.mp3"),
                    "br": 320
                }))
            }),
        )
        .route(
            "/gd/pic",
            get(|headers: HeaderMap| async move {
                let host = headers
                    .get(header::HOST)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("127.0.0.1");
                axum::Json(serde_json::json!({
                    "url": format!("http://{host}/cover/art.png")
                }))
            }),
        )
        .route(
            "/gd/lyric",
            get(|| async {
                axum::Json(serde_json::json!({
                    "lyric": "[ti:Test Song]\n[00:01.00]Line One\n[00:02.50]Line Two"
                }))
            }),
        )
        .route(
            "/cover/art.png",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "image/png")
                    .body(Body::from(PNG_1X1.to_vec()))
                    .unwrap()
            }),
        )
        .route(
            "/cover/evil.svg",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "image/svg+xml")
                    .body(Body::from("<svg></svg>"))
                    .unwrap()
            }),
        )
        .route(
            "/audio/song.mp3",
            get(move |req: AxumRequest| {
                let cap = cap_clone.clone();
                let data_range = audio_for_range.clone();
                let data_full = audio_for_chunked.clone();
                async move {
                    {
                        let mut g = cap.lock().unwrap();
                        g.clear();
                        for (k, v) in req.headers() {
                            if let Ok(s) = v.to_str() {
                                g.insert(k.as_str().to_ascii_lowercase(), s.to_string());
                            }
                        }
                    }
                    if let Some(range_hdr) =
                        req.headers().get("range").and_then(|v| v.to_str().ok())
                        && let Some(spec) = range_hdr.strip_prefix("bytes=")
                        && let Some((s_str, e_str)) = spec.split_once('-')
                        && let (Ok(s), Ok(e)) = (s_str.parse::<usize>(), e_str.parse::<usize>())
                        && s <= e
                        && e < data_range.len()
                    {
                        let slice = data_range[s..=e].to_vec();
                        let len = slice.len();
                        let total = data_range.len();
                        return Response::builder()
                            .status(StatusCode::PARTIAL_CONTENT)
                            .header("content-type", "audio/mpeg")
                            .header("content-length", len.to_string())
                            .header("content-range", format!("bytes {s}-{e}/{total}"))
                            .header("accept-ranges", "bytes")
                            .body(Body::from(slice))
                            .unwrap();
                    }

                    // Stream in multiple chunks to verify chunked pump
                    let (tx, rx) = tokio::sync::mpsc::channel::<
                        xmedia_core::domain::error::DomainResult<Bytes>,
                    >(4);
                    tokio::spawn(async move {
                        for chunk in data_full.chunks(30) {
                            let _ = tx.send(Ok(Bytes::copy_from_slice(chunk))).await;
                        }
                    });
                    let stream = xmedia_core::infra::stream::MpscByteStream::into_boxed(rx);
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "audio/mpeg")
                        .header("content-length", audio_for_chunked.len().to_string())
                        .header("accept-ranges", "bytes")
                        .body(Body::from_stream(stream))
                        .unwrap()
                }
            }),
        )
        .route(
            "/audio/evil.html",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/html")
                    .body(Body::from("<html>nope</html>"))
                    .unwrap()
            }),
        )
        .route(
            "/audio/missing.mp3",
            get(|| async { (StatusCode::NOT_FOUND, "not found") }),
        )
        .route(
            "/audio/ratelimit.mp3",
            get(|| async { (StatusCode::TOO_MANY_REQUESTS, "slow down") }),
        )
        .route(
            "/audio/oob.mp3",
            get(|| async {
                Response::builder()
                    .status(StatusCode::RANGE_NOT_SATISFIABLE)
                    .header("content-range", "bytes */120")
                    .body(Body::empty())
                    .unwrap()
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, mock_app).await.unwrap();
    });
    let base = format!("http://{addr}");

    let h = setup_online_test_harness(true).await;

    let source_one = serde_json::json!({
        "sourceName": "Mock Music A",
        "sourceUrl": base,
        "sourceGroup": "  测试分组  ",
        "sourceComment": "mock source",
        "enabled": true,
        "weight": 50,
        "header": {
            "User-Agent": "HeaderUA",
            "X-Shared": "from-header",
            "Host": "spoofed.local"
        },
        "mediaHeader": {
            "X-Shared": "from-media-header",
            "Referer": "https://music.example.test/",
            "Range": "bytes=0-1",
            "Connection": "close"
        },
        "searchUrl": format!("{base}/gd/search?q={{{{key}}}}"),
        "exploreUrl": format!("热歌::{base}/gd/explore"),
        "supportedQualities": ["128", "320"],
        "loginUrl": "function login() { cookie.setCookie(source.sourceUrl, 'session=abc123'); source.setVariable('tok_' + result.password); return 'logged_in'; }",
        "loginUi": serde_json::json!([
            {"name": "username", "type": "text"},
            {"name": "password", "type": "password"}
        ]).to_string(),
        "enabledCookieJar": true,
        "ruleSearch": {
            "trackList": "$",
            "trackId": "$.id",
            "title": "$.name",
            "artist": "$.artist[*]||$.artist",
            "album": "$.album",
            "coverId": "$.pic_id",
            "urlId": "$.url_id",
            "lyricId": "$.lyric_id",
            "checkKeyWord": "hello"
        },
        "ruleStream": {
            "requestUrl": format!("{base}/gd/url?id={{{{urlId}}}}&br={{{{quality}}}}"),
            "streamUrl": "$.url",
            "bitrate": "$.br"
        },
        "ruleCover": {
            "requestUrl": format!("{base}/gd/pic?id={{{{coverId}}}}"),
            "coverUrl": "$.url"
        },
        "ruleLyric": {
            "requestUrl": format!("{base}/gd/lyric?id={{{{lyricId}}}}"),
            "lyric": "$.lyric"
        }
    });

    let source_two_disabled = serde_json::json!({
        "sourceName": "Disabled Source B",
        "sourceUrl": "https://disabled.example.test",
        "enabled": false,
        "weight": 10
    });

    // 1. Non-admin cannot import (403)
    let (st, _, _) = send_json(
        &h.app,
        "POST",
        "/sources/import",
        &h.member_token,
        Some(serde_json::json!({ "raw_json": source_one.to_string() })),
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);

    // 2. Admin imports with UTF-8 BOM + array
    let bom_array = format!(
        "\u{feff}{}",
        serde_json::to_string(&vec![&source_one, &source_two_disabled]).unwrap()
    );
    let (st, _, body) = send_json(
        &h.app,
        "POST",
        "/sources/import",
        &h.admin_token,
        Some(serde_json::json!({ "raw_json": bom_array })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let import_rep: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(import_rep["imported_count"], 2);
    assert_eq!(import_rep["sources"][0]["group_name"], "测试分组");
    let id_a = import_rep["sources"][0]["id"].as_str().unwrap().to_string();
    let id_b = import_rep["sources"][1]["id"].as_str().unwrap().to_string();

    // 3. Large import (> 2 MiB default Axum limit, under 32 MiB limit)
    let big_comment = "x".repeat(2_500_000);
    let mut big_source = source_two_disabled.clone();
    big_source["sourceComment"] = serde_json::Value::String(big_comment);
    let (st, _, _) = send_json(
        &h.app,
        "POST",
        "/sources/import",
        &h.admin_token,
        Some(serde_json::json!({ "raw_json": big_source.to_string() })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    // 4. Login action sets secret + variable; masking & preservation verified
    let (st, _, body) = send_json(
        &h.app,
        "POST",
        &format!("/sources/{id_a}/login-action"),
        &h.admin_token,
        Some(serde_json::json!({
            "login_info": { "username": "alice", "password": "topsecret" },
            "action": "login()"
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let login_dto: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(login_dto["result"], "logged_in");
    assert_eq!(
        login_dto["source"]["login_info"]["password"],
        MASKED_SECRET_PLACEHOLDER
    );
    assert_eq!(login_dto["source"]["login_info"]["username"], "alice");
    assert_eq!(login_dto["source"]["variable_preview"], "tok_topsecret");

    // Re-submitting masked secret preserves existing secret
    let (st, _, body) = send_json(
        &h.app,
        "POST",
        &format!("/sources/{id_a}/login-action"),
        &h.admin_token,
        Some(serde_json::json!({
            "form_data": { "username": "alice", "password": MASKED_SECRET_PLACEHOLDER },
            "action": "login()"
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let login_dto2: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(login_dto2["source"]["variable_preview"], "tok_topsecret");

    // Re-importing same (sourceUrl, sourceName) deduplicates and preserves runtime_state
    let mut updated_one = source_one.clone();
    updated_one["weight"] = serde_json::json!(60);
    let (st, _, body) = send_json(
        &h.app,
        "POST",
        "/sources/import",
        &h.admin_token,
        Some(serde_json::json!({ "raw_json": updated_one.to_string() })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let reimport_rep: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(reimport_rep["imported_count"], 0);
    assert_eq!(reimport_rep["updated_count"], 1);
    assert_eq!(reimport_rep["sources"][0]["id"], id_a);
    assert_eq!(
        reimport_rep["sources"][0]["variable_preview"],
        "tok_topsecret"
    );
    assert_eq!(
        reimport_rep["sources"][0]["cookie_hosts"],
        serde_json::json!(["127.0.0.1"])
    );

    // 5. Non-admin GET /sources strips disabled sources + sensitive fields
    let (st, _, body) = send_json(&h.app, "GET", "/sources", &h.member_token, None).await;
    assert_eq!(st, StatusCode::OK);
    let member_list: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
    assert_eq!(member_list.len(), 1);
    assert_eq!(member_list[0]["id"], id_a);
    assert!(member_list[0]["login_info"].as_object().unwrap().is_empty());
    assert!(member_list[0]["login_ui"].as_array().unwrap().is_empty());
    assert_eq!(member_list[0]["variable_preview"], "");
    assert!(
        member_list[0]["cookie_hosts"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    // Non-admin receives 403 on export, variable, batch, reorder, patch, delete, test, login-action
    for (m, p, payload) in [
        ("GET", format!("/sources/{id_a}/export"), None),
        ("GET", format!("/sources/{id_a}/variable"), None),
        (
            "POST",
            "/sources/reorder".into(),
            Some(serde_json::json!({"ordered_ids": [id_a]})),
        ),
        (
            "POST",
            "/sources/batch".into(),
            Some(serde_json::json!({"ids": [id_a], "action": "disable"})),
        ),
        (
            "PATCH",
            format!("/sources/{id_a}"),
            Some(serde_json::json!({"weight": 1})),
        ),
        ("DELETE", format!("/sources/{id_a}"), None),
        (
            "POST",
            format!("/sources/{id_a}/test"),
            Some(serde_json::json!({"mode": "search"})),
        ),
    ] {
        let (st, _, _) = send_json(&h.app, m, &p, &h.member_token, payload).await;
        assert_eq!(st, StatusCode::FORBIDDEN, "expected 403 on {m} {p}");
    }

    // 6. Non-admin cannot access disabled source B via /online/* or proxies (403)
    let (st, _, _) = send_json(
        &h.app,
        "POST",
        "/online/explore",
        &h.member_token,
        Some(serde_json::json!({"source_id": id_b})),
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, _, _) = send_json(
        &h.app,
        "GET",
        &format!(
            "/online/audio-proxy?url={}&source_id={id_b}",
            url_encode_component(&format!("{base}/audio/song.mp3"))
        ),
        &h.member_token,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);

    // 7. Search / Explore / TrackInfo / Stream / Lyrics / Test Source
    let (st, _, body) = send_json(
        &h.app,
        "POST",
        "/online/search",
        &h.member_token,
        Some(serde_json::json!({"keyword": "test"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let search_dto: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(search_dto["results"].as_array().unwrap().len(), 1);
    assert_eq!(
        search_dto["results"][0]["artist"],
        "Artist One / Artist Two"
    );

    let (st, _, body) = send_json(
        &h.app,
        "POST",
        "/online/explore",
        &h.member_token,
        Some(serde_json::json!({"source_id": id_a})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let exp_dto: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(exp_dto["results"][0]["track_id"], "exp-1");
    assert!(!exp_dto["categories"].as_array().unwrap().is_empty());

    let (st, _, body) = send_json(
        &h.app,
        "POST",
        "/online/track-info",
        &h.member_token,
        Some(serde_json::json!({
            "source_id": id_a,
            "track_id": "track-1",
            "title": "Test Song",
            "artist": "Artist One",
            "cover_id": "pic-1",
            "url_id": "url-1",
            "lyric_id": "lrc-1",
            "quality": "320"
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let resolved_dto: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let proxy_stream_url = resolved_dto["proxy_stream_url"]
        .as_str()
        .unwrap()
        .to_string();
    let proxy_cover_url = resolved_dto["proxy_cover_url"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(proxy_stream_url.starts_with("/api/v1/music/online/audio-proxy?url="));
    assert!(proxy_cover_url.starts_with("/api/v1/music/online/cover-proxy?url="));
    assert_eq!(resolved_dto["lyrics"]["synced"], true);
    assert_eq!(resolved_dto["lyrics"]["lines"].as_array().unwrap().len(), 2);

    let (st, _, body) = send_json(
        &h.app,
        "POST",
        "/online/stream",
        &h.member_token,
        Some(serde_json::json!({
            "source_id": id_a,
            "track_id": "track-1",
            "url_id": "url-1",
            "quality": "320"
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let stream_dto: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(stream_dto["headers_applied"], true);
    assert_eq!(stream_dto["bitrate"], 320);

    let (st, _, body) = send_json(
        &h.app,
        "POST",
        "/online/lyrics",
        &h.member_token,
        Some(serde_json::json!({
            "source_id": id_a,
            "track_id": "track-1",
            "lyric_id": "lrc-1"
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let lrc_dto: StructuredLyrics = serde_json::from_slice(&body).unwrap();
    assert!(lrc_dto.synced);
    assert_eq!(lrc_dto.lines.len(), 2);

    // Admin source test (Full)
    let (st, _, body) = send_json(
        &h.app,
        "POST",
        &format!("/sources/{id_a}/test"),
        &h.admin_token,
        Some(serde_json::json!({"mode": "full"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let test_rep: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(test_rep["ok"], true);
    assert_eq!(test_rep["steps"].as_array().unwrap().len(), 4);

    let (st, _, body) = send_json(
        &h.app,
        "POST",
        &format!("/sources/{id_a}/test"),
        &h.admin_token,
        Some(serde_json::json!({"mode": "stream", "url_id": "url-1", "quality": "320"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let stream_test_rep: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(stream_test_rep["ok"], true);

    let (st, _, body) = send_json(
        &h.app,
        "POST",
        &format!("/sources/{id_a}/test"),
        &h.admin_token,
        Some(serde_json::json!({"mode": "lyric", "lyric_id": "lrc-1"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let lyric_test_rep: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(lyric_test_rep["ok"], true);

    // 8. Cover proxy: valid PNG + SVG rejection
    let rel_cover = proxy_cover_url.strip_prefix("/api/v1/music").unwrap();
    let (st, hdrs, bytes) = send_json(&h.app, "GET", rel_cover, &h.member_token, None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(bytes, PNG_1X1);
    assert_eq!(hdrs.get(header::CONTENT_TYPE).unwrap(), "image/png");
    assert_eq!(hdrs.get(header::X_CONTENT_TYPE_OPTIONS).unwrap(), "nosniff");
    assert_eq!(
        hdrs.get(header::CONTENT_SECURITY_POLICY).unwrap(),
        IMAGE_PROXY_CSP
    );

    let (st, _, _) = send_json(
        &h.app,
        "GET",
        &format!(
            "/online/cover-proxy?url={}&source_id={id_a}",
            url_encode_component(&format!("{base}/cover/evil.svg"))
        ),
        &h.member_token,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    // 9. Audio proxy: 206 Range forwarding + header injection (mediaHeader overrides header; static Range & Host stripped)
    let rel_audio = proxy_stream_url.strip_prefix("/api/v1/music").unwrap();
    let range_req = Request::builder()
        .method("GET")
        .uri(rel_audio)
        .header(header::AUTHORIZATION, format!("Bearer {}", h.member_token))
        .header(header::RANGE, "bytes=10-29")
        .body(Body::empty())
        .unwrap();
    let range_resp = h.app.clone().oneshot(range_req).await.unwrap();
    assert_eq!(range_resp.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        range_resp.headers().get(header::CONTENT_RANGE).unwrap(),
        "bytes 10-29/120"
    );
    assert_eq!(
        range_resp.headers().get(header::CONTENT_LENGTH).unwrap(),
        "20"
    );
    assert_eq!(
        range_resp.headers().get(header::ACCEPT_RANGES).unwrap(),
        "bytes"
    );
    assert_eq!(
        range_resp.headers().get(header::CONTENT_TYPE).unwrap(),
        "audio/mpeg"
    );
    assert_eq!(
        range_resp
            .headers()
            .get(header::X_CONTENT_TYPE_OPTIONS)
            .unwrap(),
        "nosniff"
    );
    let range_body = to_bytes(range_resp.into_body(), 1024).await.unwrap();
    assert_eq!(range_body.as_ref(), &(10u8..30).collect::<Vec<u8>>()[..]);

    {
        let cap = captured_headers.lock().unwrap();
        assert_eq!(
            cap.get("x-shared").map(String::as_str),
            Some("from-media-header")
        );
        assert_eq!(
            cap.get("referer").map(String::as_str),
            Some("https://music.example.test/")
        );
        assert_eq!(
            cap.get("cookie").map(String::as_str),
            Some("session=abc123")
        );
        assert_eq!(cap.get("range").map(String::as_str), Some("bytes=10-29"));
        assert_ne!(cap.get("host").map(String::as_str), Some("spoofed.local"));
    }

    // 10. Audio proxy: 200 OK full multi-chunk stream
    let (st, hdrs, full_bytes) = send_json(&h.app, "GET", rel_audio, &h.member_token, None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(hdrs.get(header::CONTENT_LENGTH).unwrap(), "120");
    assert_eq!(full_bytes, (0u8..120).collect::<Vec<u8>>());

    // 11. Audio proxy content-type & error mapping + malformed range
    let (st, _, _) = send_json(
        &h.app,
        "GET",
        &format!(
            "/online/audio-proxy?url={}&source_id={id_a}",
            url_encode_component(&format!("{base}/audio/evil.html"))
        ),
        &h.member_token,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::UNSUPPORTED_MEDIA_TYPE);

    let (st, _, _) = send_json(
        &h.app,
        "GET",
        &format!(
            "/online/audio-proxy?url={}&source_id={id_a}",
            url_encode_component(&format!("{base}/audio/missing.mp3"))
        ),
        &h.member_token,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);

    let (st, _, _) = send_json(
        &h.app,
        "GET",
        &format!(
            "/online/audio-proxy?url={}&source_id={id_a}",
            url_encode_component(&format!("{base}/audio/ratelimit.mp3"))
        ),
        &h.member_token,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::TOO_MANY_REQUESTS);

    let (st, _, _) = send_json(
        &h.app,
        "GET",
        &format!(
            "/online/audio-proxy?url={}&source_id={id_a}",
            url_encode_component(&format!("{base}/audio/oob.mp3"))
        ),
        &h.member_token,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::RANGE_NOT_SATISFIABLE);

    let bad_range_req = Request::builder()
        .method("GET")
        .uri(rel_audio)
        .header(header::AUTHORIZATION, format!("Bearer {}", h.member_token))
        .header(header::RANGE, "bytes=invalid-range")
        .body(Body::empty())
        .unwrap();
    let bad_range_resp = h.app.clone().oneshot(bad_range_req).await.unwrap();
    assert_eq!(bad_range_resp.status(), StatusCode::BAD_REQUEST);

    // 12. Reorder, Export, Batch, Delete
    let id_a_uuid = Uuid::parse_str(&id_a).unwrap();
    let id_b_uuid = Uuid::parse_str(&id_b).unwrap();
    let (st, _, body) = send_json(
        &h.app,
        "POST",
        "/sources/reorder",
        &h.admin_token,
        Some(serde_json::json!({"ordered_ids": [id_b_uuid, id_a_uuid]})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let reordered: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
    assert_eq!(reordered[0]["id"], id_b);

    let (st, _, _) = send_json(
        &h.app,
        "GET",
        &format!("/sources/{id_a}/export"),
        &h.admin_token,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    let (st, _, body) = send_json(
        &h.app,
        "POST",
        "/sources/export-batch",
        &h.admin_token,
        Some(serde_json::json!({"ids": [id_a_uuid]})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let exported_arr: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
    assert_eq!(exported_arr.len(), 1);

    let (st, _, body) = send_json(
        &h.app,
        "PATCH",
        &format!("/sources/{id_a}"),
        &h.admin_token,
        Some(serde_json::json!({
            "group_name": "  新分组  ",
            "weight": 88,
            "variable": "updated_var"
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let patched_dto: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(patched_dto["group_name"], "新分组");

    let (st, _, body) = send_json(
        &h.app,
        "GET",
        &format!("/sources/{id_a}/variable"),
        &h.admin_token,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let var_json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(var_json["variable"], "updated_var");

    let (st, _, body) = send_json(
        &h.app,
        "POST",
        "/sources/batch",
        &h.admin_token,
        Some(serde_json::json!({
            "ids": [id_a_uuid],
            "action": "disable"
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let batch_rep: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(batch_rep["affected_count"], 1);

    let (st, _, _) = send_json(
        &h.app,
        "DELETE",
        &format!("/sources/{id_b}"),
        &h.admin_token,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::NO_CONTENT);

    server.abort();
}

#[tokio::test]
async fn ssrf_hardened_default_rejects_loopback_and_metadata_urls() {
    let h = setup_online_test_harness(false).await;

    for target in [
        "http://127.0.0.1:8080/song.mp3",
        "http://localhost:8080/song.mp3",
        "http://169.254.169.254/latest/meta-data/",
    ] {
        let (st, _, _) = send_json(
            &h.app,
            "GET",
            &format!("/online/audio-proxy?url={}", url_encode_component(target)),
            &h.member_token,
            None,
        )
        .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "audio-proxy must reject SSRF target {target}"
        );

        let (st, _, _) = send_json(
            &h.app,
            "GET",
            &format!("/online/cover-proxy?url={}", url_encode_component(target)),
            &h.member_token,
            None,
        )
        .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "cover-proxy must reject SSRF target {target}"
        );
    }
}

#[tokio::test]
async fn engine_blocks_template_js_injection_and_fixes_short_circuit_headers_ia_login_cover() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use crate::source::model::{MusicCoverRule, MusicSearchRule, MusicStreamRule};

    let lookup_hits = Arc::new(AtomicUsize::new(0));
    let lookup_hits_srv = lookup_hits.clone();
    let seen_queries: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let seen_queries_srv = seen_queries.clone();

    #[derive(Deserialize)]
    struct LookupQuery {
        id: Option<String>,
        q: Option<String>,
    }

    let app = Router::new()
        .route(
            "/echo-search",
            get(move |axum::extract::RawQuery(raw_q): axum::extract::RawQuery| {
                let sq = seen_queries_srv.clone();
                async move {
                    if let Some(q) = raw_q {
                        sq.lock().unwrap().push(q);
                    }
                    axum::Json(serde_json::json!([
                        { "id": "t1", "name": "Safe Song", "artist": "Safe Artist" }
                    ]))
                }
            }),
        )
        .route(
            "/stream-lookup",
            get(move |Query(q): Query<LookupQuery>| {
                let hits = lookup_hits_srv.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    axum::Json(serde_json::json!({
                        "url": format!(
                            "https://cdn.example.test/audio/{}/{}.m4a",
                            q.id.as_deref().unwrap_or("none"),
                            q.q.as_deref().unwrap_or("default")
                        ),
                        "fmt": "flac",
                        "br": 999,
                        "ttl": 600,
                        "dynHeaders": { "X-Dyn": "from-jsonpath", "Referer": "https://dyn.test/" }
                    }))
                }
            }),
        )
        .route(
            "/metadata/ia-bool-priv",
            get(|| async {
                axum::Json(serde_json::json!({
                    "metadata": {
                        "identifier": "ia-bool-priv",
                        "title": "Public OGG Item",
                        "creator": "Archivist"
                    },
                    "files": [
                        { "name": "secret_bool.mp3", "private": true, "length": "01:00", "bitrate": "320" },
                        { "name": "secret_str.mp3", "private": "true", "length": "02:00", "bitrate": "256" },
                        { "name": "public_track.ogg", "length": "03:30", "bitrate": "192.4" }
                    ]
                }))
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let base = format!("http://{addr}");

    let engine = MusicSourceEngine::new(
        HttpClientConfig {
            timeout_secs: 5,
            allow_private_ips: true,
            proxy_url: None,
        },
        Duration::from_secs(10),
    );

    let seeds: Vec<MusicSourceSpec> = MusicSourceSpec::parse_json_batch(SEEDS_MUSIC_JSON)
        .unwrap()
        .into_iter()
        .map(|(s, _)| s)
        .collect();

    tokio::task::spawn_blocking(move || {
        // 1. Template / JS code injection prevention across placeholders and @js: rules
        let inj_spec = MusicSourceSpec {
            source_name: "Injection Guard".into(),
            source_url: base.clone(),
            search_url: Some(format!(
                "{base}/echo-search?k={{{{key}}}}&extra={{{{1+1}}}}"
            )),
            rule_search: Some(MusicSearchRule {
                track_list: Some("$".into()),
                track_id: Some("$.id".into()),
                title: Some("$.name".into()),
                artist: Some("$.artist".into()),
                ..Default::default()
            }),
            rule_stream: Some(MusicStreamRule {
                stream_url: Some(
                    "{{playUrl}}||https://cdn.example.test/tracks/{{urlId}}/{{trackId}}.mp3?q={{1+1}}"
                        .into(),
                ),
                format: Some("mp3".into()),
                ..Default::default()
            }),
            ..Default::default()
        };

        // 1a. Untrusted keyword containing {{java.toast(...)}} must NOT execute JS
        let search_out = engine
            .search_sync(
                &inj_spec,
                SourceRuntimeState::default(),
                "{{java.toast('INJECTED_KEY')}}",
                1,
                10,
            )
            .unwrap();
        assert!(search_out.diagnostics.toasts.is_empty());
        {
            let qs = seen_queries.lock().unwrap();
            assert!(qs[0].contains("extra=2"));
            assert!(!qs[0].contains("INJECTED_KEY") || qs[0].contains("%7B%7B"));
        }

        // 1b. Untrusted playUrl containing {{java.toast(...)}} in || combinator must NOT execute JS
        let mut ctx_play = MusicTrackContext {
            track_id: "{{1+1}}".into(),
            url_id: Some("{{java.toast('INJECTED_URLID')}}".into()),
            play_url: Some("https://cdn.example.test/{{java.toast('INJECTED_PLAY')}}.mp3".into()),
            ..Default::default()
        };
        let play_out = engine
            .resolve_stream_sync(&inj_spec, SourceRuntimeState::default(), &ctx_play, None)
            .unwrap();
        assert!(play_out.diagnostics.toasts.is_empty());
        assert_eq!(
            play_out.data.stream_url,
            "https://cdn.example.test/{{java.toast('INJECTED_PLAY')}}.mp3"
        );

        // 1c. Fallback branch with urlId="{{java.toast(...)}}" & trackId="{{1+1}}" does NOT execute JS,
        // while the template's own {{1+1}} token DOES evaluate to 2.
        ctx_play.play_url = None;
        let stream_out = engine
            .resolve_stream_sync(&inj_spec, SourceRuntimeState::default(), &ctx_play, None)
            .unwrap();
        assert!(stream_out.diagnostics.toasts.is_empty());
        assert_eq!(
            stream_out.data.stream_url,
            "https://cdn.example.test/tracks/{{java.toast('INJECTED_URLID')}}/{{1+1}}.mp3?q=2"
        );

        // 1d. @js: rule reads track/key/quality via JS globals without placeholder interpolation
        let js_spec = MusicSourceSpec {
            source_name: "JS Global Guard".into(),
            source_url: base.clone(),
            rule_stream: Some(MusicStreamRule {
                stream_url: Some(
                    "@js:'https://cdn.example.test/' + encodeURIComponent(track.urlId) + '?q=' + quality"
                        .into(),
                ),
                ..Default::default()
            }),
            ..Default::default()
        };
        let js_ctx = MusicTrackContext {
            track_id: "1".into(),
            url_id: Some("';java.toast('PWN');'".into()),
            ..Default::default()
        };
        let js_out = engine
            .resolve_stream_sync(&js_spec, SourceRuntimeState::default(), &js_ctx, Some("320"))
            .unwrap();
        assert!(js_out.diagnostics.toasts.is_empty());
        assert_eq!(
            js_out.data.stream_url,
            "https://cdn.example.test/'%3Bjava.toast('PWN')%3B'?q=320"
        );

        // 2. resolve_stream_sync short-circuit behavior + {{ quality }} + headers (JSONPath & static JSON)
        let mut sc_spec = MusicSourceSpec {
            source_name: "ShortCircuit Spec".into(),
            source_url: base.clone(),
            supported_qualities: vec!["256".into()],
            rule_stream: Some(MusicStreamRule {
                request_url: Some(format!("{base}/stream-lookup?id={{{{trackId}}}}&q={{{{ quality }}}}")),
                stream_url: Some("$.url".into()),
                format: Some("m4a".into()),
                bitrate: Some("256".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let sc_ctx = MusicTrackContext {
            track_id: "trk1".into(),
            play_url: Some("https://cdn.example.test/preview.m4a".into()),
            ..Default::default()
        };
        // 2a. Whitespace {{ quality }} MUST bypass short-circuit and hit /stream-lookup
        let out_q = engine
            .resolve_stream_sync(&sc_spec, SourceRuntimeState::default(), &sc_ctx, Some("256"))
            .unwrap();
        assert_eq!(lookup_hits.load(Ordering::SeqCst), 1);
        assert!(out_q.data.stream_url.ends_with("/trk1/256.m4a"));

        // 2b. Static JSON object ruleStream.headers MUST bypass short-circuit and apply header
        sc_spec.rule_stream.as_mut().unwrap().request_url =
            Some(format!("{base}/stream-lookup?id={{{{trackId}}}}"));
        sc_spec.rule_stream.as_mut().unwrap().headers = Some(r#"{"X-Static":"1"}"#.into());
        let out_static_hdr = engine
            .resolve_stream_sync(&sc_spec, SourceRuntimeState::default(), &sc_ctx, None)
            .unwrap();
        assert_eq!(lookup_hits.load(Ordering::SeqCst), 2);
        assert_eq!(
            out_static_hdr.data.headers.get("X-Static").map(String::as_str),
            Some("1")
        );

        // 2c. JSONPath ruleStream.headers + dynamic format/bitrate/expiresInSecs MUST bypass short-circuit
        sc_spec.rule_stream.as_mut().unwrap().headers = Some("$.dynHeaders".into());
        sc_spec.rule_stream.as_mut().unwrap().format = Some("$.fmt".into());
        sc_spec.rule_stream.as_mut().unwrap().bitrate = Some("$.br".into());
        sc_spec.rule_stream.as_mut().unwrap().expires_in_secs = Some("$.ttl".into());
        let out_dyn = engine
            .resolve_stream_sync(&sc_spec, SourceRuntimeState::default(), &sc_ctx, None)
            .unwrap();
        assert_eq!(lookup_hits.load(Ordering::SeqCst), 3);
        assert_eq!(out_dyn.data.format, "flac");
        assert_eq!(out_dyn.data.bitrate, Some(999));
        assert_eq!(out_dyn.data.expires_in_secs, Some(600));
        assert_eq!(
            out_dyn.data.headers.get("X-Dyn").map(String::as_str),
            Some("from-jsonpath")
        );

        // 2d. When headers is None, format/bitrate/expires static, no quality in req_tpl, and quality matches sole supported_quality -> short-circuits!
        sc_spec.rule_stream.as_mut().unwrap().headers = None;
        sc_spec.rule_stream.as_mut().unwrap().format = Some("m4a".into());
        sc_spec.rule_stream.as_mut().unwrap().bitrate = Some("256".into());
        sc_spec.rule_stream.as_mut().unwrap().expires_in_secs = None;
        let out_fast = engine
            .resolve_stream_sync(&sc_spec, SourceRuntimeState::default(), &sc_ctx, Some("256"))
            .unwrap();
        assert_eq!(lookup_hits.load(Ordering::SeqCst), 3);
        assert_eq!(out_fast.data.stream_url, "https://cdn.example.test/preview.m4a");

        // 3. Internet Archive boolean private:true exclusion & .ogg fallback for duration/bitrate
        let mut ia = seeds[2].clone();
        ia.source_url = base.clone();
        ia.rule_track_detail.as_mut().unwrap().request_url =
            Some(format!("{base}/metadata/{{{{trackId}}}}"));
        ia.rule_stream.as_mut().unwrap().request_url =
            Some(format!("{base}/metadata/{{{{urlId}}}}"));
        let ia_ctx = MusicTrackContext {
            track_id: "ia-bool-priv".into(),
            url_id: Some("ia-bool-priv".into()),
            ..Default::default()
        };
        let ia_detail = engine
            .fetch_track_detail_sync(&ia, SourceRuntimeState::default(), &ia_ctx)
            .unwrap();
        assert_eq!(ia_detail.data.duration_secs, Some(210));
        let ia_stream = engine
            .resolve_stream_sync(&ia, SourceRuntimeState::default(), &ia_ctx, None)
            .unwrap();
        assert_eq!(
            ia_stream.data.stream_url,
            "https://archive.org/download/ia-bool-priv/public_track.ogg"
        );
        assert_eq!(ia_stream.data.format, "ogg");
        assert_eq!(ia_stream.data.bitrate, Some(192));

        // 4. Case-insensitive @JS: login prefix + merge_login_info_preserving_masked in run_login_action_sync
        let login_spec = MusicSourceSpec {
            source_name: "Case-insensitive Login".into(),
            source_url: base.clone(),
            login_url: Some("@JS: function login() { return result.password; }".into()),
            ..Default::default()
        };
        let mut init_rt = SourceRuntimeState::default();
        init_rt
            .login_info
            .insert("password".into(), "real_secret_pw".into());
        let login_out = engine
            .run_login_action_sync(
                &login_spec,
                init_rt,
                HashMap::from([(
                    "password".into(),
                    MASKED_SECRET_PLACEHOLDER.to_string(),
                )]),
                Some("login()"),
            )
            .unwrap();
        assert_eq!(login_out.data, "real_secret_pw");
        assert_eq!(
            login_out.state.login_info.get("password").map(String::as_str),
            Some("real_secret_pw")
        );

        // 5. resolve_cover preserves ctx.cover_url when ruleCover.requestUrl is whitespace-empty
        let cover_spec = MusicSourceSpec {
            source_name: "Whitespace Cover Req".into(),
            source_url: base,
            rule_cover: Some(MusicCoverRule {
                request_url: Some("   ".into()),
                cover_url: Some("https://override.test/wrong.jpg".into()),
            }),
            ..Default::default()
        };
        let cover_ctx = MusicTrackContext {
            track_id: "c1".into(),
            cover_url: Some("https://cdn.example.test/existing.jpg".into()),
            ..Default::default()
        };
        let cover_out = engine
            .resolve_cover_sync(&cover_spec, SourceRuntimeState::default(), &cover_ctx)
            .unwrap();
        assert_eq!(
            cover_out.data.as_deref(),
            Some("https://cdn.example.test/existing.jpg")
        );
    })
    .await
    .unwrap();

    server.abort();
}

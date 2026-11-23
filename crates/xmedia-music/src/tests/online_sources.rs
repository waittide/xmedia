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
            "https://cdn.example.test/%7B%7Bjava.toast('INJECTED_PLAY')%7D%7D.mp3"
        );

        // 1c. Fallback branch with urlId="{{java.toast(...)}}" & trackId="{{1+1}}" does NOT execute JS,
        // and percent-encodes urlId/trackId while the template's own {{1+1}} token DOES evaluate to 2.
        ctx_play.play_url = None;
        let stream_out = engine
            .resolve_stream_sync(&inj_spec, SourceRuntimeState::default(), &ctx_play, None)
            .unwrap();
        assert!(stream_out.diagnostics.toasts.is_empty());
        assert_eq!(
            stream_out.data.stream_url,
            format!(
                "https://cdn.example.test/tracks/{}/{}.mp3?q=2",
                url_encode_component("{{java.toast('INJECTED_URLID')}}"),
                url_encode_component("{{1+1}}"),
            )
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

#[tokio::test]
async fn regression_explore_detail_url_js_injection_and_uppercase_quality() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use crate::source::model::{MusicStreamRule, MusicTrackDetailRule};

    #[derive(Deserialize)]
    struct QueryCapture {
        q: Option<String>,
        p: Option<String>,
        l: Option<String>,
        calc: Option<String>,
        leak: Option<String>,
        x: Option<String>,
    }

    let seen_stream_q: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let seen_stream_q_srv = seen_stream_q.clone();
    let seen_detail_x: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let seen_detail_x_srv = seen_detail_x.clone();
    let seen_explore: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let seen_explore_srv = seen_explore.clone();
    let seen_explore_srv2 = seen_explore.clone();
    let stream_hits = Arc::new(AtomicUsize::new(0));
    let stream_hits_srv = stream_hits.clone();

    let mock_app = Router::new()
        .route(
            "/cat-js",
            get(move |Query(q): Query<QueryCapture>| {
                let se = seen_explore_srv.clone();
                async move {
                    se.lock().unwrap().push(format!(
                        "cat:p={}:l={}:calc={}",
                        q.p.as_deref().unwrap_or(""),
                        q.l.as_deref().unwrap_or(""),
                        q.calc.as_deref().unwrap_or("")
                    ));
                    axum::Json(serde_json::json!([
                        { "id": "c1", "name": "Cat Track", "artist": "Cat Artist" }
                    ]))
                }
            }),
        )
        .route(
            "/custom-explore",
            get(move |Query(q): Query<QueryCapture>| {
                let se = seen_explore_srv2.clone();
                async move {
                    se.lock().unwrap().push(format!(
                        "custom:p={}:l={}:leak={}",
                        q.p.as_deref().unwrap_or(""),
                        q.l.as_deref().unwrap_or(""),
                        q.leak.as_deref().unwrap_or("")
                    ));
                    axum::Json(serde_json::json!([
                        { "id": "c2", "name": "Custom Track", "artist": "Custom Artist" }
                    ]))
                }
            }),
        )
        .route(
            "/detail-ok",
            get(move |Query(q): Query<QueryCapture>| {
                let sd = seen_detail_x_srv.clone();
                async move {
                    sd.lock().unwrap().push(q.x.unwrap_or_default());
                    axum::Json(serde_json::json!({
                        "id": "d1",
                        "name": "Detail Track",
                        "artist": "Detail Artist",
                        "lrc": "[00:01.00]Safe Lyric Line"
                    }))
                }
            }),
        )
        .route(
            "/stream-q",
            get(move |Query(q): Query<QueryCapture>| {
                let hits = stream_hits_srv.clone();
                let sq = seen_stream_q_srv.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    sq.lock().unwrap().push(q.q.unwrap_or_default());
                    axum::Json(serde_json::json!({
                        "url": "https://cdn.example.test/hq.mp3"
                    }))
                }
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, mock_app).await.unwrap();
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

    let base_clone = base.clone();
    let seen_detail_x_check = seen_detail_x.clone();
    let seen_stream_q_check = seen_stream_q.clone();
    tokio::task::spawn_blocking(move || {
        // 1. {{ QUALITY }} uppercase wire substitution
        let q_spec = MusicSourceSpec {
            source_name: "Uppercase Quality".into(),
            source_url: base_clone.clone(),
            supported_qualities: vec!["256".into()],
            rule_stream: Some(MusicStreamRule {
                request_url: Some(format!("{base_clone}/stream-q?q={{{{ QUALITY }}}}")),
                stream_url: Some("$.url".into()),
                format: Some("mp3".into()),
                bitrate: Some("256".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let q_ctx = MusicTrackContext {
            track_id: "t1".into(),
            play_url: Some("https://cdn.example.test/low.mp3".into()),
            ..Default::default()
        };
        let q_out = engine
            .resolve_stream_sync(&q_spec, SourceRuntimeState::default(), &q_ctx, Some("256"))
            .unwrap();
        assert_eq!(stream_hits.load(Ordering::SeqCst), 1);
        assert_eq!(q_out.data.stream_url, "https://cdn.example.test/hq.mp3");
        assert_eq!(seen_stream_q_check.lock().unwrap().as_slice(), &["256"]);

        // 2. fetch_track_detail_sync & resolve_lyrics_sync block JS/template injection on ctx.detail_url fallback
        let detail_spec = MusicSourceSpec {
            source_name: "Detail Fallback Guard".into(),
            source_url: base_clone.clone(),
            rule_track_detail: Some(MusicTrackDetailRule {
                request_url: None,
                track_id: Some("$.id".into()),
                title: Some("$.name".into()),
                artist: Some("$.artist".into()),
                lyric: Some("$.lrc".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let mut rt_secret = SourceRuntimeState {
            variable: "ADMIN_VAR".into(),
            ..Default::default()
        };
        rt_secret
            .login_info
            .insert("api_secret".into(), "TOP_SECRET".into());

        // 2a. @js: in ctx.detail_url is rejected without executing QuickJS
        let js_ctx = MusicTrackContext {
            track_id: "t1".into(),
            detail_url: Some(format!(
                "@js:java.toast(source.getLoginInfo()); source.setVariable('PWN'); '{base_clone}/detail-ok'"
            )),
            ..Default::default()
        };
        let detail_js_err = engine
            .fetch_track_detail_sync(&detail_spec, rt_secret.clone(), &js_ctx)
            .unwrap_err();
        assert!(detail_js_err.diagnostics.toasts.is_empty());

        let lyric_js_err = engine
            .resolve_lyrics_sync(&detail_spec, rt_secret.clone(), &js_ctx)
            .unwrap_err();
        assert!(lyric_js_err.diagnostics.toasts.is_empty());

        // 2b. {{java.toast(...)}} in ctx.detail_url is NOT evaluated as Mustache/JS
        let mustache_ctx = MusicTrackContext {
            track_id: "t1".into(),
            detail_url: Some(format!(
                "{base_clone}/detail-ok?x={{{{java.toast(source.getLoginInfo())}}}}"
            )),
            ..Default::default()
        };
        let detail_ok = engine
            .fetch_track_detail_sync(&detail_spec, rt_secret.clone(), &mustache_ctx)
            .unwrap();
        assert!(detail_ok.diagnostics.toasts.is_empty());
        assert_eq!(detail_ok.state.variable, "ADMIN_VAR");
        assert_eq!(detail_ok.data.title, "Detail Track");

        let lyric_ok = engine
            .resolve_lyrics_sync(&detail_spec, rt_secret, &mustache_ctx)
            .unwrap();
        assert!(lyric_ok.diagnostics.toasts.is_empty());
        assert_eq!(lyric_ok.state.variable, "ADMIN_VAR");
        assert_eq!(
            lyric_ok.data.unwrap().lyrics.lines[0].text,
            "Safe Lyric Line"
        );
        {
            let xs = seen_detail_x_check.lock().unwrap();
            assert_eq!(xs.len(), 2);
            assert_eq!(xs[0], "{{java.toast(source.getLoginInfo())}}");
        }
    })
    .await
    .unwrap();

    // 3. REST API non-admin POST /online/explore, /online/track-info, /online/lyrics hardening
    let h = setup_online_test_harness(true).await;
    let source_json = serde_json::json!({
        "sourceName": "Explore Guard Source",
        "sourceUrl": base,
        "enabled": true,
        "exploreUrl": format!("分类JS::{base}/cat-js?p={{{{page}}}}&l={{{{limit}}}}&calc={{{{1+1}}}}"),
        "loginUrl": "function login() { source.setVariable('ADMIN_SECRET_VAR'); return 'ok'; }",
        "ruleSearch": {
            "trackList": "$",
            "trackId": "$.id",
            "title": "$.name",
            "artist": "$.artist"
        },
        "ruleTrackDetail": {
            "trackId": "$.id",
            "title": "$.name",
            "artist": "$.artist",
            "lyric": "$.lrc"
        }
    });
    let (st, _, body) = send_json(
        &h.app,
        "POST",
        "/sources/import",
        &h.admin_token,
        Some(serde_json::json!({ "raw_json": source_json.to_string() })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let imp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let sid = imp["sources"][0]["id"].as_str().unwrap().to_string();

    let (st, _, _) = send_json(
        &h.app,
        "POST",
        &format!("/sources/{sid}/login-action"),
        &h.admin_token,
        Some(serde_json::json!({
            "login_info": { "api_secret": "TOP_SECRET_999" },
            "action": "login()"
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    // 3a. Matching category URL from exploreUrl still evaluates admin-authored {{1+1}}
    let (st, _, body) = send_json(
        &h.app,
        "POST",
        "/online/explore",
        &h.member_token,
        Some(serde_json::json!({
            "source_id": sid,
            "explore_url": format!("{base}/cat-js?p={{{{page}}}}&l={{{{limit}}}}&calc={{{{1+1}}}}"),
            "page": 2,
            "limit": 5
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let cat_resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(cat_resp["results"][0]["title"], "Cat Track");

    // 3b. Non-matching @js: explore_url from non-admin is rejected and never mutates state
    let (st, _, _) = send_json(
        &h.app,
        "POST",
        "/online/explore",
        &h.member_token,
        Some(serde_json::json!({
            "source_id": sid,
            "explore_url": format!("@js:java.toast(source.getLoginInfo()); source.setVariable('PWNED'); '{base}/custom-explore'")
        })),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    // 3c. Non-matching custom HTTP URL substitutes ONLY {{page}}/{{limit}} without QuickJS
    let (st, _, body) = send_json(
        &h.app,
        "POST",
        "/online/explore",
        &h.member_token,
        Some(serde_json::json!({
            "source_id": sid,
            "explore_url": format!("{base}/custom-explore?p={{{{page}}}}&l={{{{limit}}}}&leak={{{{java.toast(source.getLoginInfo())}}}}"),
            "page": 3,
            "limit": 7
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let custom_resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(custom_resp["results"][0]["title"], "Custom Track");
    assert!(
        custom_resp["diagnostics"]["toasts"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    // 3d. Non-admin /online/track-info & /online/lyrics with tainted detail_url do NOT execute QuickJS
    let (st, _, body) = send_json(
        &h.app,
        "POST",
        "/online/track-info",
        &h.member_token,
        Some(serde_json::json!({
            "source_id": sid,
            "track_id": "d1",
            "detail_url": format!("{base}/detail-ok?x={{{{java.toast(source.getLoginInfo())}}}}")
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let ti_resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(ti_resp["track"]["title"], "Detail Track");
    assert!(
        ti_resp["diagnostics"]["toasts"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    let (st, _, _) = send_json(
        &h.app,
        "POST",
        "/online/lyrics",
        &h.member_token,
        Some(serde_json::json!({
            "source_id": sid,
            "track_id": "d1",
            "detail_url": format!("@js:java.toast('PWN'); '{base}/detail-ok'")
        })),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    let (st, _, body) = send_json(
        &h.app,
        "POST",
        "/online/lyrics",
        &h.member_token,
        Some(serde_json::json!({
            "source_id": sid,
            "track_id": "d1",
            "detail_url": format!("{base}/detail-ok?x={{{{java.toast('PWN')}}}}")
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let lrc_resp: StructuredLyrics = serde_json::from_slice(&body).unwrap();
    assert_eq!(lrc_resp.lines[0].text, "Safe Lyric Line");

    // Admin variable must remain intact
    let (st, _, body) = send_json(
        &h.app,
        "GET",
        &format!("/sources/{sid}/variable"),
        &h.admin_token,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let var_resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(var_resp["variable"], "ADMIN_SECRET_VAR");

    {
        let ex = seen_explore.lock().unwrap();
        assert_eq!(ex[0], "cat:p=2:l=5:calc=2");
        assert_eq!(
            ex[1],
            "custom:p=3:l=7:leak={{java.toast(source.getLoginInfo())}}"
        );
    }

    server.abort();
}

#[tokio::test]
async fn regression_raw_json_source_group_sync_and_concurrent_test_source_state_preservation() {
    use tokio::sync::Notify;

    let search_started = Arc::new(Notify::new());
    let search_release = Arc::new(Notify::new());
    let started_srv = search_started.clone();
    let release_srv = search_release.clone();

    let mock_app = Router::new().route(
        "/slow-search",
        get(move || {
            let st = started_srv.clone();
            let rel = release_srv.clone();
            async move {
                st.notify_one();
                rel.notified().await;
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .header("set-cookie", "test_cookie=1; Path=/")
                    .body(Body::from(
                        serde_json::json!([
                            { "id": "s1", "name": "Slow Song", "artist": "Slow Artist" }
                        ])
                        .to_string(),
                    ))
                    .unwrap()
            }
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, mock_app).await.unwrap();
    });
    let base = format!("http://{addr}");

    let h = setup_online_test_harness(true).await;

    // 1. import_sources & update_source(raw_json) trim and sync sourceGroup in exported raw_json
    let batch = serde_json::json!([
        {
            "sourceName": "Sync Group A",
            "sourceUrl": base,
            "sourceGroup": "  Indie Rock  ",
            "enabled": true,
            "enabledCookieJar": true,
            "loginUrl": "function login() { source.setVariable('var_' + (result.secret || '')); return 'ok'; }",
            "loginUi": serde_json::json!([{"name": "secret", "type": "password"}]).to_string(),
            "searchUrl": format!("{base}/slow-search?k={{{{key}}}}"),
            "ruleSearch": {
                "trackList": "$",
                "trackId": "$.id",
                "title": "$.name",
                "artist": "$.artist"
            }
        },
        {
            "sourceName": "Sync Group B",
            "sourceUrl": "https://sync-b.example.test",
            "sourceGroup": "   ",
            "enabled": true
        }
    ]);
    let (st, _, body) = send_json(
        &h.app,
        "POST",
        "/sources/import",
        &h.admin_token,
        Some(serde_json::json!({ "raw_json": batch.to_string() })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let imp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let id_a = imp["sources"][0]["id"].as_str().unwrap().to_string();
    let id_b = imp["sources"][1]["id"].as_str().unwrap().to_string();
    assert_eq!(imp["sources"][0]["group_name"], "Indie Rock");
    assert_eq!(imp["sources"][1]["group_name"], "");

    let (st, _, body) = send_json(
        &h.app,
        "GET",
        &format!("/sources/{id_a}/export"),
        &h.admin_token,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let exp_a: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(exp_a["sourceGroup"], "Indie Rock");

    let (st, _, body) = send_json(
        &h.app,
        "GET",
        &format!("/sources/{id_b}/export"),
        &h.admin_token,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let exp_b: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(exp_b.get("sourceGroup").is_none());

    // update_source(raw_json) with padded sourceGroup, then whitespace-only sourceGroup
    let mut updated_raw = exp_a.clone();
    updated_raw["sourceGroup"] = serde_json::json!("  Jazz Fusion  ");
    let (st, _, body) = send_json(
        &h.app,
        "PATCH",
        &format!("/sources/{id_a}"),
        &h.admin_token,
        Some(serde_json::json!({ "raw_json": updated_raw.to_string() })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let upd_dto: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(upd_dto["group_name"], "Jazz Fusion");

    let (st, _, body) = send_json(
        &h.app,
        "GET",
        &format!("/sources/{id_a}/export"),
        &h.admin_token,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let exp_a2: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(exp_a2["sourceGroup"], "Jazz Fusion");

    updated_raw["sourceGroup"] = serde_json::json!("   ");
    let (st, _, body) = send_json(
        &h.app,
        "PATCH",
        &format!("/sources/{id_a}"),
        &h.admin_token,
        Some(serde_json::json!({ "raw_json": updated_raw.to_string() })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let upd_dto2: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(upd_dto2["group_name"], "");

    let (st, _, body) = send_json(
        &h.app,
        "GET",
        &format!("/sources/{id_a}/export"),
        &h.admin_token,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let exp_a3: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(exp_a3.get("sourceGroup").is_none());

    // 2. Concurrent test_source + login_action preserves pre-existing & newly-updated variable/login_info
    let (st, _, _) = send_json(
        &h.app,
        "POST",
        &format!("/sources/{id_a}/login-action"),
        &h.admin_token,
        Some(serde_json::json!({
            "login_info": { "secret": "s1" },
            "action": "login()"
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    let app_clone = h.app.clone();
    let token_clone = h.admin_token.clone();
    let id_clone = id_a.clone();
    let test_task = tokio::spawn(async move {
        send_json(
            &app_clone,
            "POST",
            &format!("/sources/{id_clone}/test"),
            &token_clone,
            Some(serde_json::json!({ "mode": "search" })),
        )
        .await
    });

    search_started.notified().await;

    // Concurrent login_action while test_source is in flight
    let (st, _, _) = send_json(
        &h.app,
        "POST",
        &format!("/sources/{id_a}/login-action"),
        &h.admin_token,
        Some(serde_json::json!({
            "login_info": { "secret": "newer_s3" },
            "action": "login()"
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    search_release.notify_one();
    let (test_st, _, test_body) = test_task.await.unwrap();
    assert_eq!(test_st, StatusCode::OK);
    let test_rep: serde_json::Value = serde_json::from_slice(&test_body).unwrap();
    assert_eq!(test_rep["ok"], true);

    let (st, _, body) = send_json(
        &h.app,
        "GET",
        &format!("/sources/{id_a}/variable"),
        &h.admin_token,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let var_after: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(var_after["variable"], "var_newer_s3");

    // Re-running login with MASKED_SECRET_PLACEHOLDER confirms login_info["secret"] == "newer_s3"
    let (st, _, body) = send_json(
        &h.app,
        "POST",
        &format!("/sources/{id_a}/login-action"),
        &h.admin_token,
        Some(serde_json::json!({
            "login_info": { "secret": MASKED_SECRET_PLACEHOLDER },
            "action": "login()"
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let relogin: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(relogin["source"]["variable_preview"], "var_newer_s3");
    assert!(
        relogin["source"]["cookie_hosts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "127.0.0.1")
    );

    // 3. Concurrent test_source + PATCH(weight, variable) preserves both PATCH updates
    let app_clone2 = h.app.clone();
    let token_clone2 = h.admin_token.clone();
    let id_clone2 = id_a.clone();
    let test_task2 = tokio::spawn(async move {
        send_json(
            &app_clone2,
            "POST",
            &format!("/sources/{id_clone2}/test"),
            &token_clone2,
            Some(serde_json::json!({ "mode": "search" })),
        )
        .await
    });

    search_started.notified().await;

    let (st, _, body) = send_json(
        &h.app,
        "PATCH",
        &format!("/sources/{id_a}"),
        &h.admin_token,
        Some(serde_json::json!({
            "weight": 77,
            "variable": "patched_concurrent"
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let patch_dto: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(patch_dto["weight"], 77);

    search_release.notify_one();
    let (test_st2, _, _) = test_task2.await.unwrap();
    assert_eq!(test_st2, StatusCode::OK);

    let (st, _, body) = send_json(
        &h.app,
        "GET",
        &format!("/sources/{id_a}/variable"),
        &h.admin_token,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let var_after2: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(var_after2["variable"], "patched_concurrent");

    let (st, _, body) = send_json(&h.app, "GET", "/sources", &h.admin_token, None).await;
    assert_eq!(st, StatusCode::OK);
    let list: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
    let found = list.iter().find(|s| s["id"] == id_a).unwrap();
    assert_eq!(found["weight"], 77);

    server.abort();
}

#[tokio::test]
async fn regression_explore_categories_preserve_js_pipelines() {
    use crate::source::MusicSearchRule;
    use crate::source::engine::parse_explore_categories;

    #[derive(Deserialize)]
    struct ExploreHitQuery {
        tag: Option<String>,
        p: Option<String>,
        l: Option<String>,
        extra: Option<String>,
    }

    let seen_hits: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let seen_hits_srv = seen_hits.clone();

    let mock_app = Router::new().route(
        "/cat-js",
        get(move |Query(q): Query<ExploreHitQuery>| {
            let hits = seen_hits_srv.clone();
            async move {
                let tag = q.tag.unwrap_or_default();
                let p = q.p.unwrap_or_default();
                let l = q.l.unwrap_or_default();
                let extra = q.extra.unwrap_or_default();
                hits.lock()
                    .unwrap()
                    .push(format!("{tag}:p={p}:l={l}:extra={extra}"));
                axum::Json(serde_json::json!([
                    { "id": format!("id-{tag}"), "name": format!("Track {tag}"), "artist": "JS Artist" }
                ]))
            }
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, mock_app).await.unwrap();
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

    let title_url_raw = format!(
        "AtJsFirst::@js:if (page > 0 && limit > 0) {{ java.toast('AT_JS_TITLE'); '{base}/cat-js?tag=atjs_title&p=' + page + '&l=' + limit + '&extra=' + encodeURIComponent('a::b'); }}\n\
         TagJsSecond::<JS>\njava.toast('TAG_JS_TITLE');\n'/cat-js?tag=tagjs_title&p=' + page + '&l=' + limit;\n</JS>&&PrefixTag::/cat-js?tag=prefix_tag&p={{{{page}}}}&l={{{{limit}}}}<js>result + '&extra=from_prefix'</js>"
    );
    let parsed_title_cats = parse_explore_categories(&title_url_raw, &base);
    assert_eq!(parsed_title_cats.len(), 3);
    assert!(parsed_title_cats[0].url.starts_with("@js:"));
    assert!(parsed_title_cats[1].url.starts_with("<JS>"));
    assert!(
        parsed_title_cats[2]
            .url
            .starts_with("/cat-js?tag=prefix_tag")
    );

    // Bare @js: containing '::' is also preserved as default category without splitting on '::'
    let bare_cats = parse_explore_categories(
        "@js:'http://example.test/?x=' + encodeURIComponent('a::b')",
        &base,
    );
    assert_eq!(bare_cats.len(), 1);
    assert!(bare_cats[0].url.starts_with("@js:"));

    let json_arr_atjs_raw = serde_json::json!([
        {
            "title": "JsonAtJs",
            "url": "@JS:java.toast('JSON_AT_JS'); '/cat-js?tag=json_atjs&p=' + page + '&l=' + limit"
        },
        {
            "title": "JsonTagJs",
            "url": "<js>java.toast('JSON_TAG_JS'); '/cat-js?tag=json_tagjs&p=' + page + '&l=' + limit</js>"
        },
        {
            "title": "JsonPrefixTag",
            "url": "/cat-js?tag=json_prefix&p={{page}}&l={{limit}}<js>result + '&extra=json_px'</js>"
        }
    ])
    .to_string();

    let json_arr_tagjs_first_raw = serde_json::json!([
        {
            "title": "JsonTagFirst",
            "url": "<js>java.toast('JSON_TAG_FIRST'); '/cat-js?tag=json_tag_first&p=' + page + '&l=' + limit</js>"
        }
    ])
    .to_string();

    let base_clone = base.clone();
    let title_url_raw_clone = title_url_raw.clone();
    let json_arr_atjs_clone = json_arr_atjs_raw.clone();
    let json_arr_tagjs_first_clone = json_arr_tagjs_first_raw.clone();
    tokio::task::spawn_blocking(move || {
        let search_rule = MusicSearchRule {
            track_list: Some("$".into()),
            track_id: Some("$.id".into()),
            title: Some("$.name".into()),
            artist: Some("$.artist".into()),
            ..Default::default()
        };

        // 1. Title::url syntax: explore_url = None (categories[0] = @js:) and Some(cat.url) for all 3
        let spec_title = MusicSourceSpec {
            source_name: "Title JS Explore".into(),
            source_url: base_clone.clone(),
            explore_url: Some(title_url_raw_clone),
            rule_search: Some(search_rule.clone()),
            ..Default::default()
        };

        let out_none = engine
            .explore_sync(&spec_title, SourceRuntimeState::default(), None, 2, 11)
            .unwrap();
        assert_eq!(out_none.data.tracks[0].title, "Track atjs_title");
        assert_eq!(out_none.diagnostics.toasts, vec!["AT_JS_TITLE"]);
        assert_eq!(out_none.data.categories.len(), 3);

        for (idx, expected_title, expected_toast) in [
            (0usize, "Track atjs_title", "AT_JS_TITLE"),
            (1usize, "Track tagjs_title", "TAG_JS_TITLE"),
        ] {
            let cat_url = out_none.data.categories[idx].url.clone();
            let out_cat = engine
                .explore_sync(
                    &spec_title,
                    SourceRuntimeState::default(),
                    Some(&cat_url),
                    3,
                    12,
                )
                .unwrap();
            assert_eq!(out_cat.data.tracks[0].title, expected_title);
            assert_eq!(out_cat.diagnostics.toasts, vec![expected_toast]);
        }
        let cat2_url = out_none.data.categories[2].url.clone();
        let out_cat2 = engine
            .explore_sync(
                &spec_title,
                SourceRuntimeState::default(),
                Some(&cat2_url),
                4,
                13,
            )
            .unwrap();
        assert_eq!(out_cat2.data.tracks[0].title, "Track prefix_tag");

        // 2. JSON array syntax with @JS: as categories[0]
        let spec_json_at = MusicSourceSpec {
            source_name: "JSON AtJS Explore".into(),
            source_url: base_clone.clone(),
            explore_url: Some(json_arr_atjs_clone),
            rule_search: Some(search_rule.clone()),
            ..Default::default()
        };
        let out_json_none = engine
            .explore_sync(&spec_json_at, SourceRuntimeState::default(), None, 5, 14)
            .unwrap();
        assert_eq!(out_json_none.data.tracks[0].title, "Track json_atjs");
        assert_eq!(out_json_none.diagnostics.toasts, vec!["JSON_AT_JS"]);
        assert_eq!(out_json_none.data.categories.len(), 3);

        let json_tag_url = out_json_none.data.categories[1].url.clone();
        let out_json_tag = engine
            .explore_sync(
                &spec_json_at,
                SourceRuntimeState::default(),
                Some(&json_tag_url),
                6,
                15,
            )
            .unwrap();
        assert_eq!(out_json_tag.data.tracks[0].title, "Track json_tagjs");
        assert_eq!(out_json_tag.diagnostics.toasts, vec!["JSON_TAG_JS"]);

        let json_px_url = out_json_none.data.categories[2].url.clone();
        let out_json_px = engine
            .explore_sync(
                &spec_json_at,
                SourceRuntimeState::default(),
                Some(&json_px_url),
                7,
                16,
            )
            .unwrap();
        assert_eq!(out_json_px.data.tracks[0].title, "Track json_prefix");

        // 3. JSON array syntax with <js>...</js> as categories[0] and explore_url = None
        let spec_json_tag = MusicSourceSpec {
            source_name: "JSON TagJS First".into(),
            source_url: base_clone,
            explore_url: Some(json_arr_tagjs_first_clone),
            rule_search: Some(search_rule),
            ..Default::default()
        };
        let out_tag_first = engine
            .explore_sync(&spec_json_tag, SourceRuntimeState::default(), None, 8, 17)
            .unwrap();
        assert_eq!(out_tag_first.data.tracks[0].title, "Track json_tag_first");
        assert_eq!(out_tag_first.diagnostics.toasts, vec!["JSON_TAG_FIRST"]);
    })
    .await
    .unwrap();

    // 4. HTTP POST /api/v1/music/online/explore with Title::url & JSON array JS categories + rejection guard
    let h = setup_online_test_harness(true).await;
    let source_json = serde_json::json!({
        "sourceName": "HTTP JS Explore Categories",
        "sourceUrl": base,
        "enabled": true,
        "exploreUrl": json_arr_atjs_raw,
        "loginUrl": "function login() { source.setVariable('HTTP_ADMIN_SECRET'); return 'ok'; }",
        "ruleSearch": {
            "trackList": "$",
            "trackId": "$.id",
            "title": "$.name",
            "artist": "$.artist"
        }
    });
    let (st, _, body) = send_json(
        &h.app,
        "POST",
        "/sources/import",
        &h.admin_token,
        Some(serde_json::json!({ "raw_json": source_json.to_string() })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let imp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let sid = imp["sources"][0]["id"].as_str().unwrap().to_string();

    let (st, _, _) = send_json(
        &h.app,
        "POST",
        &format!("/sources/{sid}/login-action"),
        &h.admin_token,
        Some(serde_json::json!({ "login_info": {}, "action": "login()" })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    // 4a. Non-admin explore_url = None executes categories[0] (@JS:) and returns raw JS category URLs
    let (st, _, body) = send_json(
        &h.app,
        "POST",
        "/online/explore",
        &h.member_token,
        Some(serde_json::json!({
            "source_id": sid,
            "page": 9,
            "limit": 18
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let resp_none: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(resp_none["results"][0]["title"], "Track json_atjs");
    let cats = resp_none["categories"].as_array().unwrap();
    assert_eq!(cats.len(), 3);
    assert!(cats[0]["url"].as_str().unwrap().starts_with("@JS:"));
    assert!(cats[1]["url"].as_str().unwrap().starts_with("<js>"));

    // 4b. Non-admin selects categories[1].url (<js>...</js>) and categories[2].url (prefix+<js>)
    for (cat_idx, expected_title) in [(1usize, "Track json_tagjs"), (2usize, "Track json_prefix")] {
        let chosen_url = cats[cat_idx]["url"].as_str().unwrap();
        let (st, _, body) = send_json(
            &h.app,
            "POST",
            "/online/explore",
            &h.member_token,
            Some(serde_json::json!({
                "source_id": sid,
                "explore_url": chosen_url,
                "page": 10,
                "limit": 19
            })),
        )
        .await;
        assert_eq!(st, StatusCode::OK);
        let resp_cat: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(resp_cat["results"][0]["title"], expected_title);
    }

    // 4c. Non-matching malicious @js: and <js>/<JS> overrides remain rejected with 400 Bad Request
    for malicious in [
        format!("@js:java.toast('PWN'); source.setVariable('PWNED'); '{base}/cat-js?tag=pwn'"),
        format!("<js>java.toast('PWN'); source.setVariable('PWNED'); '{base}/cat-js?tag=pwn'</js>"),
        format!("<JS>java.toast('PWN'); source.setVariable('PWNED'); '{base}/cat-js?tag=pwn'</JS>"),
    ] {
        let (st, _, _) = send_json(
            &h.app,
            "POST",
            "/online/explore",
            &h.member_token,
            Some(serde_json::json!({
                "source_id": sid,
                "explore_url": malicious
            })),
        )
        .await;
        assert_eq!(st, StatusCode::BAD_REQUEST);
    }

    // Admin variable must remain intact
    let (st, _, body) = send_json(
        &h.app,
        "GET",
        &format!("/sources/{sid}/variable"),
        &h.admin_token,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let var_resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(var_resp["variable"], "HTTP_ADMIN_SECRET");

    {
        let hits = seen_hits.lock().unwrap();
        assert_eq!(
            hits.as_slice(),
            &[
                "atjs_title:p=2:l=11:extra=a::b",
                "atjs_title:p=3:l=12:extra=a::b",
                "tagjs_title:p=3:l=12:extra=",
                "prefix_tag:p=4:l=13:extra=from_prefix",
                "json_atjs:p=5:l=14:extra=",
                "json_tagjs:p=6:l=15:extra=",
                "json_prefix:p=7:l=16:extra=json_px",
                "json_tag_first:p=8:l=17:extra=",
                "json_atjs:p=9:l=18:extra=",
                "json_tagjs:p=10:l=19:extra=",
                "json_prefix:p=10:l=19:extra=json_px",
            ]
        );
    }

    server.abort();
}

#[tokio::test]
async fn concurrent_import_sources_and_patch_variable_login_preserve_state() {
    let h = setup_online_test_harness(true).await;

    let src = serde_json::json!({
        "sourceName": "ConcurrentMusicPatchImport",
        "sourceUrl": "https://concurrent-music.test",
        "sourceGroup": "InitGroup",
        "weight": 1,
        "loginUi": "[{\"name\":\"user\",\"type\":\"text\"},{\"name\":\"apiToken\",\"type\":\"password\"}]",
        "loginUrl": "@js:'ok'"
    });
    let (st_imp, _, b_imp) = send_json(
        &h.app,
        "POST",
        "/sources/import",
        &h.admin_token,
        Some(serde_json::json!({ "raw_json": src.to_string() })),
    )
    .await;
    assert_eq!(st_imp, StatusCode::OK);
    let imp_val: serde_json::Value = serde_json::from_slice(&b_imp).unwrap();
    let sid = imp_val["sources"][0]["id"].as_str().unwrap().to_string();

    for round in 0..20i64 {
        let var_val = format!("var_round_{round}");
        let tok_val = format!("tok_round_{round}");
        let group_patch = format!("PatchGroup_{round}");
        let group_import = format!("ImportGroup_{round}");
        let weight_target = 100 + round;

        let action_js = format!(
            "source.setVariable('{var_val}');cookie.setCookie('https://concurrent-music.test','sid=s{round}');cache.put('k','c{round}');'ok'"
        );
        let import_payload = serde_json::json!({
            "sourceName": "ConcurrentMusicPatchImport",
            "sourceUrl": "https://concurrent-music.test",
            "sourceGroup": group_import,
            "weight": weight_target,
            "loginUi": "[{\"name\":\"user\",\"type\":\"text\"},{\"name\":\"apiToken\",\"type\":\"password\"}]",
            "loginUrl": "@js:'ok'",
            "ruleSearch": { "searchUrl": format!("https://concurrent-music.test/s?r={round}&q={{{{key}}}}") }
        })
        .to_string();

        let login_uri = format!("/sources/{sid}/login-action");
        let patch_uri = format!("/sources/{sid}");

        let (res_login, res_patch) = tokio::join!(
            send_json(
                &h.app,
                "POST",
                &login_uri,
                &h.admin_token,
                Some(serde_json::json!({
                    "login_info": { "user": format!("u{round}"), "apiToken": tok_val },
                    "action": action_js
                })),
            ),
            send_json(
                &h.app,
                "PATCH",
                &patch_uri,
                &h.admin_token,
                Some(serde_json::json!({
                    "weight": weight_target,
                    "group_name": group_patch
                })),
            ),
        );
        assert_eq!(res_login.0, StatusCode::OK);
        assert_eq!(res_patch.0, StatusCode::OK);

        // Race login-action against import upsert
        let var_val2 = format!("var_round_{round}_b");
        let tok_val2 = format!("tok_round_{round}_b");
        let action_js2 = format!(
            "source.setVariable('{var_val2}');cookie.setCookie('https://concurrent-music.test','sid=s{round}_b');cache.put('k','c{round}_b');'ok'"
        );
        let (res_login2, res_upsert) = tokio::join!(
            send_json(
                &h.app,
                "POST",
                &login_uri,
                &h.admin_token,
                Some(serde_json::json!({
                    "login_info": { "user": format!("u{round}_b"), "apiToken": tok_val2 },
                    "action": action_js2
                })),
            ),
            send_json(
                &h.app,
                "POST",
                "/sources/import",
                &h.admin_token,
                Some(serde_json::json!({ "raw_json": import_payload })),
            ),
        );
        assert_eq!(res_login2.0, StatusCode::OK);
        assert_eq!(res_upsert.0, StatusCode::OK);

        // Also race PATCH(variable) against import upsert
        let patched_var = format!("patched_var_{round}");
        let import_payload_b = serde_json::json!({
            "sourceName": "ConcurrentMusicPatchImport",
            "sourceUrl": "https://concurrent-music.test",
            "sourceGroup": group_import,
            "weight": weight_target + 1,
            "loginUi": "[{\"name\":\"user\",\"type\":\"text\"},{\"name\":\"apiToken\",\"type\":\"password\"}]",
            "loginUrl": "@js:'ok'",
            "ruleSearch": { "searchUrl": format!("https://concurrent-music.test/s2?r={round}&q={{{{key}}}}") }
        })
        .to_string();
        let (res_patch_var, res_upsert2) = tokio::join!(
            send_json(
                &h.app,
                "PATCH",
                &patch_uri,
                &h.admin_token,
                Some(serde_json::json!({ "variable": patched_var })),
            ),
            send_json(
                &h.app,
                "POST",
                "/sources/import",
                &h.admin_token,
                Some(serde_json::json!({ "raw_json": import_payload_b })),
            ),
        );
        assert_eq!(res_patch_var.0, StatusCode::OK);
        assert_eq!(res_upsert2.0, StatusCode::OK);

        // Verify neither login_info, variable, cookies, cache, nor import-updated metadata was lost
        let (st_var, _, b_var) = send_json(
            &h.app,
            "GET",
            &format!("/sources/{sid}/variable"),
            &h.admin_token,
            None,
        )
        .await;
        assert_eq!(st_var, StatusCode::OK);
        let var_json: serde_json::Value = serde_json::from_slice(&b_var).unwrap();
        assert_eq!(var_json["variable"], patched_var);

        let (st_chk, _, b_chk) = send_json(
            &h.app,
            "POST",
            &login_uri,
            &h.admin_token,
            Some(serde_json::json!({
                "login_info": { "user": format!("u{round}_b"), "apiToken": MASKED_SECRET_PLACEHOLDER },
                "action": "JSON.stringify({v:source.getVariable(),t:source.getLoginInfoMap().apiToken,c:cookie.getCookie('https://concurrent-music.test'),k:cache.get('k')})"
            })),
        )
        .await;
        assert_eq!(st_chk, StatusCode::OK);
        let chk_val: serde_json::Value = serde_json::from_slice(&b_chk).unwrap();
        let inner: serde_json::Value =
            serde_json::from_str(chk_val["result"].as_str().unwrap()).unwrap();
        assert_eq!(inner["v"], patched_var);
        assert_eq!(inner["t"], tok_val2);
        assert!(
            inner["c"]
                .as_str()
                .unwrap()
                .contains(&format!("sid=s{round}_b"))
        );
        assert_eq!(inner["k"], format!("c{round}_b"));
        assert_eq!(chk_val["source"]["weight"], weight_target + 1);
        assert_eq!(chk_val["source"]["group_name"], group_import);

        let (st_exp, _, b_exp) = send_json(
            &h.app,
            "GET",
            &format!("/sources/{sid}/export"),
            &h.admin_token,
            None,
        )
        .await;
        assert_eq!(st_exp, StatusCode::OK);
        let exp_json: serde_json::Value = serde_json::from_slice(&b_exp).unwrap();
        assert_eq!(
            exp_json["ruleSearch"]["searchUrl"],
            format!("https://concurrent-music.test/s2?r={round}&q={{{{key}}}}")
        );
    }
}

fn music_delayed_chunked_response(
    content_type: Option<&'static str>,
    chunks: Vec<(Vec<u8>, u64)>,
) -> Response {
    let (tx, rx) = tokio::sync::mpsc::channel::<xmedia_core::domain::error::DomainResult<Bytes>>(8);
    tokio::spawn(async move {
        for (data, delay_ms) in chunks {
            if delay_ms > 0 {
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            }
            if tx.send(Ok(Bytes::from(data))).await.is_err() {
                break;
            }
        }
    });
    let mut builder = Response::builder().status(StatusCode::OK);
    if let Some(ct) = content_type {
        builder = builder.header(header::CONTENT_TYPE, ct);
    }
    builder
        .body(Body::from_stream(
            xmedia_core::infra::stream::MpscByteStream::into_boxed(rx),
        ))
        .unwrap()
}

#[tokio::test]
async fn music_runtime_state_clear_concurrent_cookies_audio_sniffing_and_unmatched_apostrophe_e2e()
{
    let h = setup_online_test_harness(true).await;

    let origin = Router::new()
        .route(
            "/search",
            get(|| async {
                axum::Json(serde_json::json!({
                    "list": [{
                        "fallback": "Good Track",
                        "artist": "Artist",
                        "id": "t1"
                    }]
                }))
            }),
        )
        .route(
            "/audio/disguised-html.bin",
            get(|| async {
                music_delayed_chunked_response(
                    Some("application/octet-stream"),
                    vec![
                        (b"  <ht".to_vec(), 0),
                        (b"ml><script>alert(1)</script></html>".to_vec(), 5),
                    ],
                )
            }),
        )
        .route(
            "/audio/disguised-svg.bin",
            get(|| async {
                music_delayed_chunked_response(
                    None,
                    vec![
                        (b"\xEF\xBB\xBF <sv".to_vec(), 0),
                        (b"g xmlns=\"http://www.w3.org/2000/svg\"><script>alert(1)</script></svg>".to_vec(), 5),
                    ],
                )
            }),
        )
        .route(
            "/audio/disguised-xml-svg.bin",
            get(|| async {
                music_delayed_chunked_response(
                    Some("application/octet-stream"),
                    vec![
                        (b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>".to_vec(), 0),
                        (b"<svg xmlns=\"http://www.w3.org/2000/svg\"><script>alert(1)</script></svg>".to_vec(), 5),
                    ],
                )
            }),
        )
        .route(
            "/audio/disguised-utf16le-svg.bin",
            get(|| async {
                let mut payload = vec![0xFFu8, 0xFE];
                for u in "<svg xmlns=\"http://www.w3.org/2000/svg\"><script>alert(1)</script></svg>".encode_utf16() {
                    payload.extend_from_slice(&u.to_le_bytes());
                }
                let mid = payload.len() / 2;
                music_delayed_chunked_response(
                    Some("application/octet-stream"),
                    vec![(payload[..mid].to_vec(), 0), (payload[mid..].to_vec(), 5)],
                )
            }),
        )
        .route(
            "/audio/disguised-utf16be-svg.bin",
            get(|| async {
                let mut payload = vec![0xFEu8, 0xFF];
                for u in "<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>".encode_utf16() {
                    payload.extend_from_slice(&u.to_be_bytes());
                }
                music_delayed_chunked_response(
                    Some("application/octet-stream"),
                    vec![(payload, 0)],
                )
            }),
        )
        .route(
            "/audio/disguised-large-comment-svg.bin",
            get(|| async {
                let mut chunk1 = b"<!-- ".to_vec();
                chunk1.extend_from_slice(&vec![b'X'; 4600]);
                let chunk2 = b" --><svg xmlns=\"http://www.w3.org/2000/svg\"><script>alert(1)</script></svg>".to_vec();
                music_delayed_chunked_response(
                    Some("application/octet-stream"),
                    vec![(chunk1, 0), (chunk2, 5)],
                )
            }),
        )
        .route(
            "/audio/disguised-large-ws-svg.bin",
            get(|| async {
                let chunk1 = vec![b' '; 4600];
                let chunk2 = b"<svg xmlns=\"http://www.w3.org/2000/svg\"><script>alert(1)</script></svg>".to_vec();
                music_delayed_chunked_response(
                    Some("application/octet-stream"),
                    vec![(chunk1, 0), (chunk2, 5)],
                )
            }),
        )
        .route(
            "/audio/disguised-utf16le-ws-svg.bin",
            get(|| async {
                let mut payload = vec![0xFFu8, 0xFE];
                for _ in 0..2400 {
                    payload.extend_from_slice(&u16::from(b' ').to_le_bytes());
                }
                for u in "<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>".encode_utf16() {
                    payload.extend_from_slice(&u.to_le_bytes());
                }
                music_delayed_chunked_response(
                    Some("application/octet-stream"),
                    vec![(payload[..4200].to_vec(), 0), (payload[4200..].to_vec(), 5)],
                )
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let srv = tokio::spawn(async move {
        axum::serve(listener, origin).await.unwrap();
    });
    let base = format!("http://{addr}");

    let raw_src = serde_json::json!({
        "sourceName": "AdversarialMusic",
        "sourceUrl": base,
        "searchUrl": format!("{base}/search?q={{{{key}}}}"),
        "loginUrl": "@js:'ok'",
        "ruleSearch": {
            "trackList": "$.list",
            "title": "$.don't || $.fallback",
            "artist": "$.artist",
            "trackId": "$.id"
        }
    })
    .to_string();

    let (st_imp, _, b_imp) = send_json(
        &h.app,
        "POST",
        "/sources/import",
        &h.admin_token,
        Some(serde_json::json!({ "raw_json": raw_src })),
    )
    .await;
    assert_eq!(st_imp, StatusCode::OK);
    let imp_json: serde_json::Value = serde_json::from_slice(&b_imp).unwrap();
    let sid = imp_json["sources"][0]["id"].as_str().unwrap().to_string();

    // GAP-06: `$.don't || $.fallback` resolves to "Good Track"
    let (st_test, _, b_test) = send_json(
        &h.app,
        "POST",
        &format!("/sources/{sid}/test"),
        &h.admin_token,
        Some(serde_json::json!({ "mode": "search", "keyword": "q" })),
    )
    .await;
    assert_eq!(st_test, StatusCode::OK);
    let test_json: serde_json::Value = serde_json::from_slice(&b_test).unwrap();
    assert_eq!(test_json["sample_titles"][0], "Good Track");

    // GAP-01: set then clear variable, cache, and cookies via login-action
    let login_uri = format!("/sources/{sid}/login-action");
    let (st_set, _, _) = send_json(
        &h.app,
        "POST",
        &login_uri,
        &h.admin_token,
        Some(serde_json::json!({
            "login_info": {},
            "action": "source.setVariable('secret_var');cache.put('k1','v1');cookie.setCookie('https://music-probe.test','a=1; b=2');'ok'"
        })),
    )
    .await;
    assert_eq!(st_set, StatusCode::OK);

    let (st_clr, _, _) = send_json(
        &h.app,
        "POST",
        &login_uri,
        &h.admin_token,
        Some(serde_json::json!({
            "login_info": {},
            "action": "source.setVariable('');cache.delete('k1');cookie.removeCookie('https://music-probe.test');'ok'"
        })),
    )
    .await;
    assert_eq!(st_clr, StatusCode::OK);

    let (st_var, _, b_var) = send_json(
        &h.app,
        "GET",
        &format!("/sources/{sid}/variable"),
        &h.admin_token,
        None,
    )
    .await;
    assert_eq!(st_var, StatusCode::OK);
    let var_json: serde_json::Value = serde_json::from_slice(&b_var).unwrap();
    assert_eq!(var_json["variable"], "");

    let (st_chk, _, b_chk) = send_json(
        &h.app,
        "POST",
        &login_uri,
        &h.admin_token,
        Some(serde_json::json!({
            "login_info": {},
            "action": "JSON.stringify({v:source.getVariable(),k1:cache.get('k1'),ck:cookie.getCookie('https://music-probe.test')})"
        })),
    )
    .await;
    assert_eq!(st_chk, StatusCode::OK);
    let chk_json: serde_json::Value = serde_json::from_slice(&b_chk).unwrap();
    let inner: serde_json::Value =
        serde_json::from_str(chk_json["result"].as_str().unwrap()).unwrap();
    assert_eq!(inner["v"], "");
    assert!(inner["k1"].is_null());
    assert_eq!(inner["ck"], "");

    // GAP-01: concurrent writes to distinct cookie keys on the same host preserve both
    let (r_a, r_b) = tokio::join!(
        send_json(
            &h.app,
            "POST",
            &login_uri,
            &h.admin_token,
            Some(serde_json::json!({
                "login_info": {},
                "action": "cookie.setCookie('https://same-host.test','kA=valA');'a'"
            })),
        ),
        send_json(
            &h.app,
            "POST",
            &login_uri,
            &h.admin_token,
            Some(serde_json::json!({
                "login_info": {},
                "action": "cookie.setCookie('https://same-host.test','kB=valB');'b'"
            })),
        ),
    );
    assert_eq!(r_a.0, StatusCode::OK);
    assert_eq!(r_b.0, StatusCode::OK);

    let (st_ck, _, b_ck) = send_json(
        &h.app,
        "POST",
        &login_uri,
        &h.admin_token,
        Some(serde_json::json!({
            "login_info": {},
            "action": "cookie.getCookie('https://same-host.test')"
        })),
    )
    .await;
    assert_eq!(st_ck, StatusCode::OK);
    let ck_res = serde_json::from_slice::<serde_json::Value>(&b_ck).unwrap()["result"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(ck_res.contains("kA=valA"), "missing kA=valA in {ck_res}");
    assert!(ck_res.contains("kB=valB"), "missing kB=valB in {ck_res}");

    // GAP-03 & I2-GAP-05: audio-proxy rejects split-chunk HTML, SVG, XML+SVG, UTF-16LE/BE SVG,
    // and >4KB HTML comment + SVG disguised as octet-stream/empty CT
    for path in [
        "/audio/disguised-html.bin",
        "/audio/disguised-svg.bin",
        "/audio/disguised-xml-svg.bin",
        "/audio/disguised-utf16le-svg.bin",
        "/audio/disguised-utf16be-svg.bin",
        "/audio/disguised-large-comment-svg.bin",
        "/audio/disguised-large-ws-svg.bin",
        "/audio/disguised-utf16le-ws-svg.bin",
    ] {
        let (st_px, _, _) = send_json(
            &h.app,
            "GET",
            &format!(
                "/online/audio-proxy?url={}&source_id={sid}",
                url_encode_component(&format!("{base}{path}"))
            ),
            &h.member_token,
            None,
        )
        .await;
        assert_eq!(st_px, StatusCode::BAD_REQUEST, "expected 400 for {path}");
    }

    srv.abort();
}

#[tokio::test]
async fn online_music_endpoints_block_param_and_options_injection_and_support_relative_options() {
    use axum::routing::any;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let h = setup_online_test_harness(true).await;

    let admin_hits = Arc::new(AtomicUsize::new(0));
    let admin_hits_c = admin_hits.clone();
    let seen_reqs: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let seen_c = seen_reqs.clone();

    let origin_app = Router::new()
        .route(
            "/admin",
            any(move || {
                let hits = admin_hits_c.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    Response::builder()
                        .status(StatusCode::OK)
                        .body(Body::from(r#"{"url":"https://evil.test/pwn.mp3"}"#))
                        .unwrap()
                }
            }),
        )
        .route(
            "/api/{*rest}",
            any(move |req: AxumRequest| {
                let seen = seen_c.clone();
                async move {
                    let method = req.method().to_string();
                    let uri = req.uri().to_string();
                    let body = to_bytes(req.into_body(), 64 * 1024)
                        .await
                        .unwrap_or_default();
                    let body_str = String::from_utf8_lossy(&body).to_string();
                    seen.lock()
                        .unwrap()
                        .push((format!("{method} {uri}"), body_str));
                    let payload = if uri.contains("/search") {
                        serde_json::json!({
                            "items": [{ "id": "t1", "name": "Track One", "artist": "Artist" }]
                        })
                    } else if uri.contains("/url") {
                        serde_json::json!({ "url": "/audio/ok.mp3", "br": "320" })
                    } else if uri.contains("/lyric") {
                        serde_json::json!({ "lyric": "[00:01.00]Clean Line" })
                    } else {
                        serde_json::json!({ "id": "t1", "name": "Track One", "artist": "Artist" })
                    };
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .body(Body::from(payload.to_string()))
                        .unwrap()
                }
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base = format!("http://{addr}");
    let srv = tokio::spawn(async move {
        axum::serve(listener, origin_app).await.unwrap();
    });

    let src_json = serde_json::json!([
        {
            "sourceName": "Injection Guard Music",
            "sourceUrl": base,
            "searchUrl": "/api/search?q={{key}},{\"method\":\"POST\",\"body\":\"k={{key}}\"}",
            "supportedQualities": ["128", "320"],
            "ruleSearch": {
                "trackList": "$.items[*]",
                "trackId": "$.id",
                "title": "$.name",
                "artist": "$.artist"
            },
            "ruleTrackDetail": {
                "requestUrl": "/api/detail/{{trackId}}",
                "trackId": "$.id",
                "title": "$.name"
            },
            "ruleStream": {
                "requestUrl": "/api/url?id={{urlId}}&br={{quality}}",
                "streamUrl": "$.url"
            },
            "ruleLyric": {
                "requestUrl": "/api/lyric?id={{lyricId}}",
                "lyric": "$.lyric"
            }
        },
        {
            "sourceName": "Passthrough Guard Music",
            "sourceUrl": format!("{base}#pt"),
            "ruleStream": {
                "streamUrl": "{{playUrl}}"
            }
        }
    ]);

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
    let sid_tmpl = imp_val["sources"][0]["id"].as_str().unwrap().to_string();
    let sid_pt = imp_val["sources"][1]["id"].as_str().unwrap().to_string();

    // 1. Relative url,{options} search works cleanly
    let (st_search, _, b_search) = send_json(
        &h.app,
        "POST",
        "/online/search",
        &h.member_token,
        Some(serde_json::json!({
            "keyword": "a&extra=1",
            "source_ids": [&sid_tmpl],
            "page": 1
        })),
    )
    .await;
    assert_eq!(st_search, StatusCode::OK);
    let s_val: serde_json::Value = serde_json::from_slice(&b_search).unwrap();
    assert_eq!(s_val["results"][0]["title"], "Track One");

    // 2. Member POST /online/stream, /online/track-info, /online/lyrics neutralize ,{"method":"POST"}, &extra=1, ../../admin
    let evil_opts = r#"1,{"method":"POST","headers":{"X-Evil":"1"},"body":"evil=1"}"#;
    let (st_stream, _, b_stream) = send_json(
        &h.app,
        "POST",
        "/online/stream",
        &h.member_token,
        Some(serde_json::json!({
            "source_id": &sid_tmpl,
            "track_id": "../../admin",
            "url_id": evil_opts,
            "quality": evil_opts
        })),
    )
    .await;
    assert_eq!(st_stream, StatusCode::OK);
    let stream_val: serde_json::Value = serde_json::from_slice(&b_stream).unwrap();
    assert_eq!(stream_val["stream_url"], format!("{base}/audio/ok.mp3"));

    let (st_ti, _, _) = send_json(
        &h.app,
        "POST",
        "/online/track-info",
        &h.member_token,
        Some(serde_json::json!({
            "source_id": &sid_tmpl,
            "track_id": "../../admin",
            "url_id": "1&extra=1",
            "lyric_id": "../../admin",
            "quality": "320&extra=1"
        })),
    )
    .await;
    assert_eq!(st_ti, StatusCode::OK);

    let (st_lrc, _, b_lrc) = send_json(
        &h.app,
        "POST",
        "/online/lyrics",
        &h.member_token,
        Some(serde_json::json!({
            "source_id": &sid_tmpl,
            "track_id": "1",
            "lyric_id": "1&extra=1"
        })),
    )
    .await;
    assert_eq!(st_lrc, StatusCode::OK);
    let lrc: StructuredLyrics = serde_json::from_slice(&b_lrc).unwrap();
    assert_eq!(lrc.lines[0].text, "Clean Line");

    // 3. Passthrough play_url rejects options injection and data: URLs with 400
    for bad_play in [
        format!("{base}/audio/ok.mp3,{{\"method\":\"POST\"}}"),
        "data:text/html,<script>alert(1)</script>".to_string(),
    ] {
        let (st_bad, _, _) = send_json(
            &h.app,
            "POST",
            "/online/stream",
            &h.member_token,
            Some(serde_json::json!({
                "source_id": &sid_pt,
                "track_id": "1",
                "play_url": bad_play
            })),
        )
        .await;
        assert_eq!(st_bad, StatusCode::BAD_REQUEST);
    }

    assert_eq!(admin_hits.load(Ordering::SeqCst), 0);
    let logged = seen_reqs.lock().unwrap();
    assert_eq!(logged[0].0, "POST /api/search?q=a%26extra%3D1");
    assert_eq!(logged[0].1, "k=a%26extra%3D1");
    assert!(
        logged[1]
            .0
            .starts_with("GET /api/url?id=1%2C%7B%22method%22")
    );
    assert!(logged[1].0.contains("&br=1%2C%7B%22method%22"));
    assert_eq!(logged[2].0, "GET /api/detail/..%2F..%2Fadmin");
    assert_eq!(logged.last().unwrap().0, "GET /api/lyric?id=1%26extra%3D1");

    srv.abort();
}

#[tokio::test]
async fn music_online_search_enforces_source_concurrent_rate_throttling() {
    let h = setup_online_test_harness(true).await;

    let origin_app = Router::new().route(
        "/api/search",
        get(|| async {
            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"items":[{"id":"m1","name":"RatedTrack","artist":"Art"}]}"#,
                ))
                .unwrap()
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base = format!("http://{addr}");
    let srv = tokio::spawn(async move {
        axum::serve(listener, origin_app).await.unwrap();
    });

    let src = serde_json::json!({
        "sourceName": "RateLimitedMusic",
        "sourceUrl": base,
        "concurrentRate": "1/200",
        "searchUrl": format!("{base}/api/search?q={{{{key}}}}"),
        "ruleSearch": {
            "trackList": "$.items[*]",
            "trackId": "$.id",
            "title": "$.name",
            "artist": "$.artist"
        }
    });
    let (st_imp, _, b_imp) = send_json(
        &h.app,
        "POST",
        "/sources/import",
        &h.admin_token,
        Some(serde_json::json!({ "raw_json": src.to_string() })),
    )
    .await;
    assert_eq!(st_imp, StatusCode::OK);
    let imp_val: serde_json::Value = serde_json::from_slice(&b_imp).unwrap();
    let sid = imp_val["sources"][0]["id"].as_str().unwrap().to_string();

    let start = std::time::Instant::now();
    let (r1, r2) = tokio::join!(
        send_json(
            &h.app,
            "POST",
            "/online/search",
            &h.member_token,
            Some(serde_json::json!({ "keyword": "a", "source_ids": [&sid] })),
        ),
        send_json(
            &h.app,
            "POST",
            "/online/search",
            &h.member_token,
            Some(serde_json::json!({ "keyword": "b", "source_ids": [&sid] })),
        ),
    );
    let elapsed = start.elapsed();
    assert_eq!(r1.0, StatusCode::OK);
    assert_eq!(r2.0, StatusCode::OK);
    assert!(
        elapsed >= Duration::from_millis(160),
        "expected concurrentRate 1/200 to throttle 2 concurrent calls, took {:?}",
        elapsed
    );

    srv.abort();
}

#[tokio::test]
async fn music_online_browse_enforces_cancel_on_future_drop() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::Notify;

    let detail_entered = Arc::new(Notify::new());
    let detail_entered_c = detail_entered.clone();
    let after_hits = Arc::new(AtomicUsize::new(0));
    let after_hits_s = after_hits.clone();
    let after_hits_c = after_hits.clone();
    let after_hits_l = after_hits.clone();

    let origin_app = Router::new()
        .route(
            "/slow-explore",
            get(|| async {
                tokio::time::sleep(Duration::from_millis(400)).await;
                r#"{"items":[]}"#
            }),
        )
        .route(
            "/slow-detail",
            get(move || {
                let notify = detail_entered_c.clone();
                async move {
                    notify.notify_one();
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    r#"{"id":"t1","name":"SlowTrack","artist":"Art"}"#
                }
            }),
        )
        .route(
            "/after-detail-stream",
            get(move || {
                let hits = after_hits_s.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    r#"{"url":"https://cdn.example/a.mp3"}"#
                }
            }),
        )
        .route(
            "/after-detail-cover",
            get(move || {
                let hits = after_hits_c.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    r#"{"pic":"https://cdn.example/c.jpg"}"#
                }
            }),
        )
        .route(
            "/after-detail-lyric",
            get(move || {
                let hits = after_hits_l.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    r#"{"lrc":"[00:01.00]Hi"}"#
                }
            }),
        )
        .route(
            "/slow-stream",
            get(|| async {
                tokio::time::sleep(Duration::from_millis(400)).await;
                r#"{"url":"https://cdn.example/slow.mp3"}"#
            }),
        )
        .route(
            "/slow-lyric",
            get(|| async {
                tokio::time::sleep(Duration::from_millis(400)).await;
                r#"{"lrc":"[00:01.00]Slow"}"#
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base = format!("http://{addr}");
    let srv = tokio::spawn(async move {
        axum::serve(listener, origin_app).await.unwrap();
    });

    let h = setup_online_test_harness(true).await;

    let src_track_info = serde_json::json!({
        "sourceName": "DropTrackInfoMusic",
        "sourceUrl": base,
        "exploreUrl": "Slow::/slow-explore",
        "searchUrl": format!("{base}/slow-explore?q={{{{key}}}}"),
        "ruleSearch": { "trackList": "$.items[*]", "trackId": "$.id", "title": "$.name", "artist": "$.artist" },
        "ruleTrackDetail": {
            "requestUrl": format!("{base}/slow-detail"),
            "trackId": "$.id",
            "title": "$.name",
            "artist": "$.artist"
        },
        "ruleStream": {
            "requestUrl": format!("{base}/after-detail-stream"),
            "streamUrl": "$.url"
        },
        "ruleCover": {
            "requestUrl": format!("{base}/after-detail-cover"),
            "coverUrl": "$.pic"
        },
        "ruleLyric": {
            "requestUrl": format!("{base}/after-detail-lyric"),
            "lyric": "$.lrc"
        }
    });
    let (st1, _, b1) = send_json(
        &h.app,
        "POST",
        "/sources/import",
        &h.admin_token,
        Some(serde_json::json!({ "raw_json": src_track_info.to_string() })),
    )
    .await;
    assert_eq!(st1, StatusCode::OK);
    let v1: serde_json::Value = serde_json::from_slice(&b1).unwrap();
    let sid1 = v1["sources"][0]["id"].as_str().unwrap().to_string();

    let src_slow_single = serde_json::json!({
        "sourceName": "DropSingleMusic",
        "sourceUrl": base,
        "exploreUrl": "Slow::/slow-explore",
        "ruleStream": {
            "requestUrl": format!("{base}/slow-stream"),
            "streamUrl": "$.url"
        },
        "ruleLyric": {
            "requestUrl": format!("{base}/slow-lyric"),
            "lyric": "$.lrc"
        }
    });
    let (st2, _, b2) = send_json(
        &h.app,
        "POST",
        "/sources/import",
        &h.admin_token,
        Some(serde_json::json!({ "raw_json": src_slow_single.to_string() })),
    )
    .await;
    assert_eq!(st2, StatusCode::OK);
    let v2: serde_json::Value = serde_json::from_slice(&b2).unwrap();
    let sid2 = v2["sources"][0]["id"].as_str().unwrap().to_string();

    // 1. Drop explore, stream, and lyrics futures early
    for (uri, payload) in [
        (
            "/online/explore",
            serde_json::json!({ "source_id": sid1, "page": 1 }),
        ),
        (
            "/online/stream",
            serde_json::json!({ "source_id": sid2, "track_id": "t1" }),
        ),
        (
            "/online/lyrics",
            serde_json::json!({ "source_id": sid2, "track_id": "t1" }),
        ),
    ] {
        let res = tokio::time::timeout(
            Duration::from_millis(80),
            send_json(&h.app, "POST", uri, &h.member_token, Some(payload)),
        )
        .await;
        assert!(res.is_err(), "expected short client timeout on {uri}");
    }

    // 2. Drop track-info future after /slow-detail starts; CancelOnDrop must prevent stream/cover/lyric steps
    {
        let track_fut = send_json(
            &h.app,
            "POST",
            "/online/track-info",
            &h.member_token,
            Some(serde_json::json!({ "source_id": sid1, "track_id": "t1" })),
        );
        tokio::pin!(track_fut);
        tokio::select! {
            _ = &mut track_fut => panic!("track-info finished unexpectedly early"),
            _ = detail_entered.notified() => {
                // Drop track_fut immediately once detail HTTP request started
            }
        }
    }
    tokio::time::sleep(Duration::from_millis(260)).await;
    assert_eq!(
        after_hits.load(Ordering::SeqCst),
        0,
        "CancelOnDrop must abort track-info before stream/cover/lyric steps"
    );

    srv.abort();
}

#[tokio::test]
async fn music_online_browse_enforces_timeout_on_stalled_upstream() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use xmedia_core::domain::error::DomainError;

    use crate::application::online::MusicOnlineService;
    use crate::infra::persistence::repositories::ToastyMusicSourceRepository;

    let after_hits = Arc::new(AtomicUsize::new(0));
    let after_hits_c = after_hits.clone();

    let origin_app = Router::new()
        .route(
            "/stall",
            get(|| async {
                tokio::time::sleep(Duration::from_secs(2)).await;
                r#"{"items":[],"id":"t1","name":"Late","artist":"Art","url":"https://cdn.example/late.mp3","lrc":"[00:01.00]Late"}"#
            }),
        )
        .route(
            "/after",
            get(move || {
                let hits = after_hits_c.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    r#"{"url":"https://cdn.example/after.mp3"}"#
                }
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base = format!("http://{addr}");
    let srv = tokio::spawn(async move {
        axum::serve(listener, origin_app).await.unwrap();
    });

    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("xmedia_music_timeout.db");
    let db_url = format!("sqlite:{}", db_path.display());
    let mut models = ModelSet::new();
    register_core_models(&mut models);
    register_music_models(&mut models);
    let mut db = Db::builder().models(models).connect(&db_url).await.unwrap();
    ensure_core_schema(&mut db, &db_url).await.unwrap();
    ensure_music_schema(&mut db, &db_url).await.unwrap();

    let users_repo = Arc::new(ToastyUserRepository::new(db.clone()));
    let lifecycles = Arc::new(RwLock::new(HashMap::new()));
    let security = Arc::new(SecurityManager::new("music-timeout-secret"));
    let tokens = Arc::new(JwtTokenService::new("music-timeout-secret", 3600));
    let auth_svc = Arc::new(AuthService::new(users_repo, security, tokens, lifecycles));
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

    let sources_repo = Arc::new(ToastyMusicSourceRepository::new(db));
    let engine = MusicSourceEngine::new(
        HttpClientConfig {
            allow_private_ips: true,
            ..Default::default()
        },
        Duration::from_secs(5),
    );
    let svc = MusicOnlineService::with_engine(sources_repo, engine, true)
        .with_timeouts(Duration::from_millis(120), Duration::from_millis(160));

    let raw_src = serde_json::json!({
        "sourceName": "StalledMusicSource",
        "sourceUrl": base,
        "exploreUrl": format!("Stall::{base}/stall"),
        "ruleSearch": { "trackList": "$.items[*]", "trackId": "$.id", "title": "$.name", "artist": "$.artist" },
        "ruleTrackDetail": {
            "requestUrl": format!("{base}/stall"),
            "trackId": "$.id",
            "title": "$.name",
            "artist": "$.artist"
        },
        "ruleStream": {
            "requestUrl": format!("{base}/stall"),
            "streamUrl": "$.url"
        },
        "ruleCover": {
            "requestUrl": format!("{base}/after"),
            "coverUrl": "$.url"
        },
        "ruleLyric": {
            "requestUrl": format!("{base}/stall"),
            "lyric": "$.lrc"
        }
    });
    let imported = svc
        .import_sources(&admin, &raw_src.to_string())
        .await
        .unwrap();
    let sid = imported.sources[0].id;

    let ctx = MusicTrackContext {
        track_id: "t1".into(),
        title: "Track".into(),
        artist: "Art".into(),
        ..Default::default()
    };

    let t_start = std::time::Instant::now();
    let exp_err = svc.explore(&admin, sid, None, 1, 10).await.unwrap_err();
    assert!(matches!(exp_err, DomainError::Validation(ref m) if m.contains("超时")));

    let stream_err = svc
        .fetch_stream(&admin, sid, ctx.clone(), None)
        .await
        .unwrap_err();
    assert!(matches!(stream_err, DomainError::Validation(ref m) if m.contains("超时")));

    let lrc_err = svc
        .fetch_lyrics(&admin, sid, ctx.clone())
        .await
        .unwrap_err();
    assert!(matches!(lrc_err, DomainError::Validation(ref m) if m.contains("超时")));

    let info_err = svc
        .fetch_track_info(&admin, sid, ctx, None)
        .await
        .unwrap_err();
    assert!(matches!(info_err, DomainError::Validation(ref m) if m.contains("超时")));

    assert!(
        t_start.elapsed() < Duration::from_secs(1),
        "all 4 timed-out calls should finish well under 2s stall, took {:?}",
        t_start.elapsed()
    );
    assert_eq!(after_hits.load(Ordering::SeqCst), 0);

    srv.abort();
}

#[tokio::test]
async fn music_stream_sanitizes_dangerous_schemes_and_aligns_headers_applied_with_proxy() {
    use std::sync::atomic::AtomicBool;

    let origin_app = Router::new()
        .route(
            "/stream/js",
            get(|| async { r#"{"url":"javascript:alert(1)"}"# }),
        )
        .route(
            "/stream/data",
            get(|| async { r#"{"url":"data:text/plain,evil"}"# }),
        )
        .route(
            "/stream/file",
            get(|| async { r#"{"url":"file:///etc/passwd"}"# }),
        )
        .route(
            "/stream/vb",
            get(|| async { r#"{"url":"vbscript:msgbox(1)"}"# }),
        )
        .route(
            "/stream/upper",
            get(|| async { r#"{"url":"HTTPS://cdn.example/upper.mp3"}"# }),
        )
        .route(
            "/stream/ok",
            get(|| async { r#"{"url":"https://cdn.example/ok.mp3"}"# }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base = format!("http://{addr}");
    let srv = tokio::spawn(async move {
        axum::serve(listener, origin_app).await.unwrap();
    });

    // 1. parse_media_headers skips empty trimmed keys, Null values, and deduplicates case-insensitively
    let hdr_spec: MusicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "HeaderSanitizeMusic",
        "sourceUrl": base,
        "header": serde_json::json!({
            "": "blank",
            "   ": "spaces",
            "X-Null": null,
            "X-Test": "from-header",
            "x-test": "dup-lower"
        }).to_string(),
        "mediaHeader": serde_json::json!({
            " ": "ignored",
            "X-Null-Media": null,
            "X-TEST": "from-media"
        }).to_string()
    }))
    .unwrap();
    let parsed_hdrs = hdr_spec.parse_media_headers();
    assert_eq!(parsed_hdrs.len(), 1);
    assert_eq!(
        parsed_hdrs.get("X-TEST").map(String::as_str),
        Some("from-media")
    );

    // 2. Engine resolve_stream_sync_with_control rejects dangerous schemes in both requestUrl and direct modes
    let base_c = base.clone();
    let engine_out = tokio::task::spawn_blocking(move || {
        let engine = MusicSourceEngine::new(
            HttpClientConfig {
                allow_private_ips: true,
                ..Default::default()
            },
            Duration::from_secs(5),
        );
        let cancel = Arc::new(AtomicBool::new(false));
        let req_spec: MusicSourceSpec = serde_json::from_value(serde_json::json!({
            "sourceName": "ReqStreamSanitize",
            "sourceUrl": base_c,
            "ruleStream": {
                "requestUrl": "{{detailUrl}}",
                "streamUrl": "$.url"
            }
        }))
        .unwrap();
        for path in ["/stream/js", "/stream/data", "/stream/file", "/stream/vb"] {
            let ctx = MusicTrackContext {
                track_id: "t1".into(),
                detail_url: Some(format!("{base_c}{path}")),
                ..Default::default()
            };
            let err = engine
                .resolve_stream_sync_with_control(
                    &req_spec,
                    SourceRuntimeState::default(),
                    &ctx,
                    None,
                    Duration::from_secs(5),
                    cancel.clone(),
                )
                .expect_err("dangerous stream_url scheme must be rejected");
            assert!(
                err.message.contains("HTTP(S)"),
                "unexpected error for {path}: {}",
                err.message
            );
        }

        // Case-insensitive HTTPS:// must be accepted
        let upper_ctx = MusicTrackContext {
            track_id: "t1".into(),
            detail_url: Some(format!("{base_c}/stream/upper")),
            ..Default::default()
        };
        let upper_res = engine
            .resolve_stream_sync_with_control(
                &req_spec,
                SourceRuntimeState::default(),
                &upper_ctx,
                None,
                Duration::from_secs(5),
                cancel.clone(),
            )
            .unwrap()
            .data;
        assert_eq!(upper_res.stream_url, "https://cdn.example/upper.mp3");

        // Direct/template ruleStream without requestUrl must also reject dangerous schemes
        let tpl_spec: MusicSourceSpec = serde_json::from_value(serde_json::json!({
            "sourceName": "TplStreamSanitize",
            "sourceUrl": base_c,
            "ruleStream": {
                "streamUrl": "@js:track.urlId"
            }
        }))
        .unwrap();
        for bad in [
            "javascript:alert(1)",
            "data:text/plain,evil",
            "file:///etc/passwd",
        ] {
            let ctx = MusicTrackContext {
                track_id: "t1".into(),
                url_id: Some(bad.into()),
                ..Default::default()
            };
            let err = engine
                .resolve_stream_sync_with_control(
                    &tpl_spec,
                    SourceRuntimeState::default(),
                    &ctx,
                    None,
                    Duration::from_secs(5),
                    cancel.clone(),
                )
                .expect_err("direct/template non-HTTP(S) stream_url must be rejected");
            assert!(
                err.message.contains("HTTP(S)"),
                "unexpected error for {bad}: {}",
                err.message
            );
        }
    })
    .await;
    engine_out.unwrap();

    // 3. HTTP REST endpoints: track-info drops non-HTTP(S) stream_url, stream returns 400,
    //    and headers_applied accurately reflects audio-proxy custom headers.
    let h = setup_online_test_harness(true).await;

    let import_src = |raw: serde_json::Value| {
        let app = h.app.clone();
        let token = h.admin_token.clone();
        async move {
            let (st, _, b) = send_json(
                &app,
                "POST",
                "/sources/import",
                &token,
                Some(serde_json::json!({ "raw_json": raw.to_string() })),
            )
            .await;
            assert_eq!(st, StatusCode::OK);
            let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
            v["sources"][0]["id"].as_str().unwrap().to_string()
        }
    };

    // 3a. Source with only ruleStream.headers (not propagated by audio-proxy) -> headers_applied: false
    let sid_rule_hdr_only = import_src(serde_json::json!({
        "sourceName": "RuleHeaderOnlyMusic",
        "sourceUrl": base,
        "ruleStream": {
            "requestUrl": format!("{base}/stream/{{{{trackId}}}}"),
            "streamUrl": "$.url",
            "headers": r#"{"X-Rule-Only":"1"}"#
        }
    }))
    .await;

    for bad_id in ["js", "data", "file", "vb"] {
        let (st_info, _, b_info) = send_json(
            &h.app,
            "POST",
            "/online/track-info",
            &h.member_token,
            Some(serde_json::json!({
                "source_id": sid_rule_hdr_only,
                "track_id": bad_id,
                "title": "Bad",
                "artist": "Attacker"
            })),
        )
        .await;
        assert_eq!(st_info, StatusCode::OK);
        let info_v: serde_json::Value = serde_json::from_slice(&b_info).unwrap();
        assert!(
            info_v["stream_url"].is_null(),
            "track-info must drop non-HTTP(S) stream_url for {bad_id}, got: {}",
            info_v["stream_url"]
        );
        assert!(
            info_v["proxy_stream_url"].is_null(),
            "track-info must drop proxy_stream_url for {bad_id}, got: {}",
            info_v["proxy_stream_url"]
        );

        let (st_stream, _, _) = send_json(
            &h.app,
            "POST",
            "/online/stream",
            &h.member_token,
            Some(serde_json::json!({
                "source_id": sid_rule_hdr_only,
                "track_id": bad_id
            })),
        )
        .await;
        assert_eq!(st_stream, StatusCode::BAD_REQUEST);
    }

    let (st_ok, _, b_ok) = send_json(
        &h.app,
        "POST",
        "/online/stream",
        &h.member_token,
        Some(serde_json::json!({
            "source_id": sid_rule_hdr_only,
            "track_id": "ok"
        })),
    )
    .await;
    assert_eq!(st_ok, StatusCode::OK);
    let ok_v: serde_json::Value = serde_json::from_slice(&b_ok).unwrap();
    assert_eq!(ok_v["headers_applied"], false);

    let (st_dbg, _, b_dbg) = send_json(
        &h.app,
        "POST",
        &format!("/sources/{sid_rule_hdr_only}/test"),
        &h.admin_token,
        Some(serde_json::json!({ "mode": "stream", "track_id": "ok" })),
    )
    .await;
    assert_eq!(st_dbg, StatusCode::OK);
    let dbg_v: serde_json::Value = serde_json::from_slice(&b_dbg).unwrap();
    assert_eq!(dbg_v["stream_probe"]["headers_applied"], false);

    // 3b. Source with empty/null header entries -> headers_applied: false
    let sid_empty_hdr = import_src(serde_json::json!({
        "sourceName": "EmptyHeaderMusic",
        "sourceUrl": base,
        "header": serde_json::json!({ "": "blank", "X-Null": null }).to_string(),
        "ruleStream": {
            "requestUrl": format!("{base}/stream/ok"),
            "streamUrl": "$.url"
        }
    }))
    .await;
    let (st_emp, _, b_emp) = send_json(
        &h.app,
        "POST",
        "/online/stream",
        &h.member_token,
        Some(serde_json::json!({ "source_id": sid_empty_hdr, "track_id": "ok" })),
    )
    .await;
    assert_eq!(st_emp, StatusCode::OK);
    let emp_v: serde_json::Value = serde_json::from_slice(&b_emp).unwrap();
    assert_eq!(emp_v["headers_applied"], false);

    // 3c. Source with forbidden/range-only header entries -> headers_applied: false
    let sid_forbid_hdr = import_src(serde_json::json!({
        "sourceName": "ForbiddenOnlyHeaderMusic",
        "sourceUrl": base,
        "header": serde_json::json!({ "Host": "spoof.test", "Range": "bytes=0-1" }).to_string(),
        "ruleStream": {
            "requestUrl": format!("{base}/stream/ok"),
            "streamUrl": "$.url"
        }
    }))
    .await;
    let (st_forbid, _, b_forbid) = send_json(
        &h.app,
        "POST",
        "/online/stream",
        &h.member_token,
        Some(serde_json::json!({ "source_id": sid_forbid_hdr, "track_id": "ok" })),
    )
    .await;
    assert_eq!(st_forbid, StatusCode::OK);
    let forbid_v: serde_json::Value = serde_json::from_slice(&b_forbid).unwrap();
    assert_eq!(forbid_v["headers_applied"], false);

    // 3d. Source with valid proxy-forwarded mediaHeader -> headers_applied: true
    let sid_real_hdr = import_src(serde_json::json!({
        "sourceName": "RealMediaHeaderMusic",
        "sourceUrl": base,
        "mediaHeader": serde_json::json!({ "Referer": "https://music.example.test/" }).to_string(),
        "ruleStream": {
            "requestUrl": format!("{base}/stream/ok"),
            "streamUrl": "$.url"
        }
    }))
    .await;
    let (st_real, _, b_real) = send_json(
        &h.app,
        "POST",
        "/online/stream",
        &h.member_token,
        Some(serde_json::json!({ "source_id": sid_real_hdr, "track_id": "ok" })),
    )
    .await;
    assert_eq!(st_real, StatusCode::OK);
    let real_v: serde_json::Value = serde_json::from_slice(&b_real).unwrap();
    assert_eq!(real_v["headers_applied"], true);

    srv.abort();
}

#[tokio::test]
async fn online_music_items_favorites_progress_and_remote_e2e() {
    let h = setup_online_test_harness(true).await;

    let src_json = serde_json::json!({
        "sourceName": "Test Music Source",
        "sourceUrl": "https://music-item.test",
        "sourceGroup": "Test",
        "ruleSearch": { "list": "$.items[*]", "title": "$.t", "trackId": "$.id" }
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

    // 1. Upsert online music track with playback progress (seconds) and extra metadata
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

    // 2. Upsert a remote Subsonic track (source_kind = "remote", site-relative proxy cover)
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

    // 3. Lookup & view filtering
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

    // 4. Update progress -> clear -> delete
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
    let api_key = admin.api_key.clone();

    let state = MusicHttpState::new(auth_svc, library_svc, music_services);
    let app = Router::new()
        .nest("/api/v1/music", rest_router(state.clone()))
        .nest("/rest", crate::http::subsonic_router(state));

    // Mock upstream music server serving stream resolver, audio WAV, lyrics, and cover
    let audio_hits = Arc::new(AtomicUsize::new(0));
    let audio_hits_srv = Arc::clone(&audio_hits);
    let wav_bytes = Arc::new(super::make_wav(16_000));
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

    // 1. Import mock online music source
    let source_spec = serde_json::json!({
        "sourceName": "Mock Tracked Music Source",
        "sourceUrl": base_url,
        "enabled": true,
        "ruleStream": {
            "url": format!("{base_url}/stream/{{{{track.id}}}}"),
            "streamUrl": "$.url",
            "format": "$.format",
            "bitrate": "$.br"
        },
        "ruleLyric": {
            "url": format!("{base_url}/lyric/{{{{track.id}}}}"),
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

    // 2. Non-admin member cannot track into managed library
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
    let (st_forb, _, _) = send_json(
        &app,
        "POST",
        "/api/v1/music/online/track",
        &member_session.token,
        Some(track_payload.clone()),
    )
    .await;
    assert_eq!(st_forb, StatusCode::FORBIDDEN);

    // 3. Admin tracks the online song (lightweight registration, audio not yet downloaded)
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

    // Idempotent re-track returns the exact same TrackId
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

    // 4. OpenSubsonic search3 exposes the tracked song BEFORE audio is cached
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

    // 5. OpenSubsonic getLyricsBySongId resolves & caches lyrics WITHOUT downloading audio
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

    // 6. OpenSubsonic stream triggers transparent on-demand audio caching
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

    // Verify DB metadata updated to cached=true with real file size
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

    // 7. Second stream hits local managed cache (no second upstream download)
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

    // 8. Batch track + explicit offline cache endpoint
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

    server_handle.abort();
}

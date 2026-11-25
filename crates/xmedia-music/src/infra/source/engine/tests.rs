use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use xmedia_source::http::HttpClientConfig;
use xmedia_source::model::SourceRuntimeState;

use super::template::{resolve_literal_http_url, split_top_level_or};
use crate::domain::online::{MusicSourceSpec, MusicTrackContext};

#[test]
fn split_top_level_or_ignores_depth_zero_unmatched_apostrophes_and_preserves_scoped_quotes() {
    assert_eq!(
        split_top_level_or("$.don't || $.fallback"),
        vec!["$.don't", "$.fallback"]
    );
    assert_eq!(
        split_top_level_or("{{'a||b'}} || $.fallback"),
        vec!["{{'a||b'}}", "$.fallback"]
    );
    assert_eq!(
        split_top_level_or("$.items[?(@.a=='x||y')] || $.fallback"),
        vec!["$.items[?(@.a=='x||y')]", "$.fallback"]
    );
}

#[test]
fn resolve_literal_http_url_rejects_legado_options_and_data_urls() {
    assert_eq!(
        resolve_literal_http_url("/track/1", "https://music.example.com", "detailUrl").unwrap(),
        "https://music.example.com/track/1"
    );
    assert!(
        resolve_literal_http_url(
            "https://music.example.com/track/1,{\"method\":\"POST\",\"headers\":{\"X-Evil\":\"1\"}}",
            "https://music.example.com",
            "detailUrl",
        )
        .is_err()
    );
    assert!(
        resolve_literal_http_url(
            "data:text/plain,evil",
            "https://music.example.com",
            "detailUrl"
        )
        .is_err()
    );
}

#[tokio::test]
async fn music_engine_neutralizes_param_injection_and_preserves_relative_options() {
    use axum::Router;
    use axum::body::{Body, to_bytes};
    use axum::http::{HeaderMap, StatusCode};
    use axum::response::Response;
    use axum::routing::{any, get};
    use std::sync::atomic::{AtomicUsize, Ordering};

    let admin_hits = Arc::new(AtomicUsize::new(0));
    let admin_hits_c = admin_hits.clone();
    let captured_requests: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let cap_c = captured_requests.clone();

    let app = Router::new()
        .route(
            "/admin",
            any(move || {
                let hits = admin_hits_c.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    Response::builder()
                        .status(StatusCode::OK)
                        .body(Body::from(r#"{"url":"https://evil/pwn.mp3"}"#))
                        .unwrap()
                }
            }),
        )
        .route(
            "/api/{*rest}",
            any(move |req: axum::extract::Request| {
                let cap = cap_c.clone();
                async move {
                    let method = req.method().to_string();
                    let uri = req.uri().to_string();
                    let body_bytes = to_bytes(req.into_body(), 64 * 1024)
                        .await
                        .unwrap_or_default();
                    let body = String::from_utf8_lossy(&body_bytes).to_string();
                    cap.lock().unwrap().push((format!("{method} {uri}"), body));
                    let json = if uri.contains("/search") {
                        serde_json::json!({
                            "items": [{ "id": "1", "title": "Song1", "artist": "Art1" }]
                        })
                    } else if uri.contains("/stream") {
                        serde_json::json!({ "url": "/media/1.mp3", "br": "320" })
                    } else if uri.contains("/cover") {
                        serde_json::json!({ "url": "/cov/1.jpg" })
                    } else if uri.contains("/lyric") {
                        serde_json::json!({ "lyric": "[00:01.00]Safe Lyric" })
                    } else {
                        serde_json::json!({ "id": "1", "title": "Song1", "artist": "Art1" })
                    };
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .body(Body::from(json.to_string()))
                        .unwrap()
                }
            }),
        )
        .route(
            "/direct.mp3",
            get(|| async { Response::new(Body::from("ok")) }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let srv = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let base = format!("http://{addr}");

    let spec: MusicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "RelOptsMusic",
        "sourceUrl": base,
        "searchUrl": "/api/search?q={{key}},{\"method\":\"POST\",\"body\":\"k={{key}}&p={{page}}\"}",
        "ruleSearch": { "trackList": "$.items[*]", "trackId": "$.id", "title": "$.title", "artist": "$.artist" },
        "ruleTrackDetail": { "requestUrl": "/api/tracks/{{trackId}}", "trackId": "$.id", "title": "$.title" },
        "ruleStream": { "requestUrl": "/api/stream?id={{urlId}}&br={{quality}}", "streamUrl": "$.url" },
        "ruleCover": { "requestUrl": "/api/cover/{{coverId}}", "coverUrl": "$.url" },
        "ruleLyric": { "requestUrl": "/api/lyric?id={{lyricId}}", "lyric": "$.lyric" }
    }))
    .unwrap();

    let passthrough_spec: MusicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "PassthroughMusic",
        "sourceUrl": base,
        "ruleStream": { "streamUrl": "{{playUrl}}" }
    }))
    .unwrap();

    let engine = super::MusicSourceEngine::new(
        HttpClientConfig {
            allow_private_ips: true,
            ..Default::default()
        },
        Duration::from_secs(10),
    );

    let evil_opts = r#"1,{"method":"POST","headers":{"X-Pwned":"1"},"body":"evil=1"}"#;
    let evil_trav = "../../admin";
    let evil_query = "1&extra=1";

    let res = tokio::task::spawn_blocking(move || {
        let search_res = engine
            .search_sync(&spec, SourceRuntimeState::default(), "a&admin=1", 2, 10)
            .unwrap();
        assert_eq!(search_res.data[0].title, "Song1");

        let ctx_stream = MusicTrackContext {
            track_id: evil_trav.into(),
            url_id: Some(evil_opts.into()),
            cover_id: Some(evil_trav.into()),
            lyric_id: Some(evil_query.into()),
            ..Default::default()
        };
        let s_res = engine
            .resolve_stream_sync(
                &spec,
                SourceRuntimeState::default(),
                &ctx_stream,
                Some(evil_opts),
            )
            .unwrap();
        assert!(s_res.data.stream_url.ends_with("/media/1.mp3"));

        let d_res = engine
            .fetch_track_detail_sync(&spec, SourceRuntimeState::default(), &ctx_stream)
            .unwrap();
        assert_eq!(d_res.data.title, "Song1");

        let c_res = engine
            .resolve_cover_sync(&spec, SourceRuntimeState::default(), &ctx_stream)
            .unwrap();
        assert!(c_res.data.unwrap().ends_with("/cov/1.jpg"));

        let l_res = engine
            .resolve_lyrics_sync(&spec, SourceRuntimeState::default(), &ctx_stream)
            .unwrap();
        assert!(l_res.data.is_some());

        assert!(
            engine
                .resolve_stream_sync(
                    &passthrough_spec,
                    SourceRuntimeState::default(),
                    &MusicTrackContext {
                        play_url: Some(format!("{base}/direct.mp3,{{\"method\":\"POST\"}}")),
                        ..Default::default()
                    },
                    None,
                )
                .is_err()
        );
        assert!(
            engine
                .resolve_stream_sync(
                    &passthrough_spec,
                    SourceRuntimeState::default(),
                    &MusicTrackContext {
                        play_url: Some("data:text/html,<h1>x</h1>".into()),
                        ..Default::default()
                    },
                    None,
                )
                .is_err()
        );
    })
    .await;
    res.unwrap();

    assert_eq!(admin_hits.load(Ordering::SeqCst), 0);
    let reqs = captured_requests.lock().unwrap();
    assert_eq!(reqs.len(), 5);
    assert_eq!(reqs[0].0, "POST /api/search?q=a%26admin%3D1");
    assert_eq!(reqs[0].1, "k=a%26admin%3D1&p=2");
    assert!(
        reqs[1]
            .0
            .starts_with("GET /api/stream?id=1%2C%7B%22method%22")
    );
    assert!(reqs[1].0.contains("&br=1%2C%7B%22method%22"));
    assert_eq!(reqs[2].0, "GET /api/tracks/..%2F..%2Fadmin");
    assert_eq!(reqs[3].0, "GET /api/cover/..%2F..%2Fadmin");
    assert_eq!(reqs[4].0, "GET /api/lyric?id=1%26extra%3D1");

    let _ = HeaderMap::new();
    srv.abort();
}

#[tokio::test]
async fn music_eval_js_pipeline_probe_preserves_track_cover_url_and_duration() {
    use axum::Router;
    use axum::body::Body;
    use axum::http::StatusCode;
    use axum::response::Response;
    use axum::routing::get;

    let app = Router::new().route(
        "/api/stream",
        get(|| async {
            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .body(Body::from(r#"{"ok":true}"#))
                .unwrap()
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let srv = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let base = format!("http://{addr}");

    let spec: MusicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "ProbeRestoreMusic",
        "sourceUrl": base,
        "ruleStream": {
            "requestUrl": "@js:'/api/stream?t=' + encodeURIComponent(track.title) + ',{\"method\":\"GET\"}'",
            "streamUrl": "@js:'https://cdn.example.com/audio.mp3?dur=' + track.duration + '&cover=' + encodeURIComponent(track.coverUrl)"
        }
    }))
    .unwrap();

    let engine = super::MusicSourceEngine::new(
        HttpClientConfig {
            allow_private_ips: true,
            ..Default::default()
        },
        Duration::from_secs(5),
    );
    let ctx = MusicTrackContext {
        track_id: "t1".into(),
        title: "Song \"Remix\"".into(),
        artist: "Artist".into(),
        duration_secs: Some(245),
        cover_url: Some("https://cdn.example.com/cover.jpg".into()),
        ..Default::default()
    };

    let out = tokio::task::spawn_blocking(move || {
        engine
            .resolve_stream_sync(&spec, SourceRuntimeState::default(), &ctx, None)
            .unwrap()
    })
    .await
    .unwrap();

    assert_eq!(
        out.data.stream_url,
        "https://cdn.example.com/audio.mp3?dur=245&cover=https%3A%2F%2Fcdn.example.com%2Fcover.jpg"
    );
    srv.abort();
}

#[tokio::test]
async fn music_split_tag_js_url_and_options_evaluates_both_and_blocks_injection() {
    use axum::Router;
    use axum::body::{Body, to_bytes};
    use axum::http::StatusCode;
    use axum::response::Response;
    use axum::routing::any;

    let captured: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let cap_c = captured.clone();

    let app = Router::new().route(
        "/search",
        any(move |req: axum::extract::Request| {
            let cap = cap_c.clone();
            async move {
                let method = req.method().to_string();
                let uri = req.uri().to_string();
                let body_bytes = to_bytes(req.into_body(), 64 * 1024)
                    .await
                    .unwrap_or_default();
                let body = String::from_utf8_lossy(&body_bytes).to_string();
                cap.lock().unwrap().push((format!("{method} {uri}"), body));
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"items":[{"id":"1","title":"SplitSong","artist":"Art"}]}"#,
                    ))
                    .unwrap()
            }
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let srv = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let base = format!("http://{addr}");

    let split_ok_spec: MusicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "SplitTagJsMusic",
        "sourceUrl": base,
        "searchUrl": "<js>var sep = 0 || 1; '/search?q=' + encodeURIComponent(key) + '&s=' + sep</js>,{\"method\":\"POST\",\"body\":\"k={{key}}\"}",
        "ruleSearch": { "trackList": "$.items[*]", "trackId": "$.id", "title": "$.title", "artist": "$.artist" }
    }))
    .unwrap();

    let split_unsafe_url_spec: MusicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "SplitUnsafeUrlMusic",
        "sourceUrl": base,
        "searchUrl": "<js>'/search?q=' + key</js>,{\"method\":\"POST\",\"body\":\"k={{key}}\"}",
        "ruleSearch": { "trackList": "$.items[*]", "trackId": "$.id", "title": "$.title", "artist": "$.artist" }
    }))
    .unwrap();

    let split_or_fallback_spec: MusicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "SplitOrFallbackMusic",
        "sourceUrl": base,
        "searchUrl": "{{nonExistentVar}} || <js>'/search?q=' + encodeURIComponent(key)</js>,{\"method\":\"POST\",\"body\":\"k={{key}}\"}",
        "ruleSearch": { "trackList": "$.items[*]", "trackId": "$.id", "title": "$.title", "artist": "$.artist" }
    }))
    .unwrap();

    let engine = super::MusicSourceEngine::new(
        HttpClientConfig {
            allow_private_ips: true,
            ..Default::default()
        },
        Duration::from_secs(5),
    );

    tokio::task::spawn_blocking(move || {
        let res = engine
            .search_sync(
                &split_ok_spec,
                SourceRuntimeState::default(),
                "hello world",
                1,
                10,
            )
            .unwrap();
        assert_eq!(res.data[0].title, "SplitSong");

        let evil = r#"hello,{"method":"DELETE","headers":{"X-Evil":"1"}}"#;
        let url_err = engine
            .search_sync(
                &split_unsafe_url_spec,
                SourceRuntimeState::default(),
                evil,
                1,
                10,
            )
            .expect_err("option injection in <js> url_tpl must be rejected");
        assert!(
            url_err.message.contains("选项"),
            "unexpected error: {url_err}"
        );

        let breakout = r#"a","method":"DELETE"#;
        let res2 = engine
            .search_sync(
                &split_or_fallback_spec,
                SourceRuntimeState::default(),
                breakout,
                1,
                10,
            )
            .unwrap();
        assert_eq!(res2.data[0].title, "SplitSong");
    })
    .await
    .unwrap();

    let reqs = captured.lock().unwrap();
    assert_eq!(reqs.len(), 2);
    assert_eq!(reqs[0].0, "POST /search?q=hello%20world&s=1");
    assert_eq!(reqs[0].1, "k=hello%20world");
    assert!(reqs[1].0.starts_with("POST /search?q=a%22"));
    assert!(reqs[1].1.starts_with("k=a%22%2C%22method%22%3A%22DELETE"));
    srv.abort();
}

#[tokio::test]
async fn music_eval_js_pipeline_probe_preserves_empty_branching_and_blocks_injection() {
    use axum::Router;
    use axum::body::Body;
    use axum::http::StatusCode;
    use axum::response::Response;
    use axum::routing::{get, post};

    let app = Router::new()
        .route(
            "/api/resolve",
            post(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"url":"https://cdn.example.com/resolved.mp3"}"#,
                    ))
                    .unwrap()
            }),
        )
        .route(
            "/api/direct-meta",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"url":"https://cdn.example.com/direct.mp3"}"#,
                    ))
                    .unwrap()
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let srv = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let base = format!("http://{addr}");

    let branching_spec: MusicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "BranchingStreamMusic",
        "sourceUrl": base,
        "ruleStream": {
            "requestUrl": "@js: if (track.playUrl) { '/api/direct-meta?u=' + encodeURIComponent(track.playUrl); } else { '/api/resolve,' + JSON.stringify({ method: 'POST', body: JSON.stringify({ id: track.trackId, title: track.title }) }); }",
            "streamUrl": "$.url"
        }
    }))
    .unwrap();

    let unsafe_spec: MusicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "UnsafeBranchingMusic",
        "sourceUrl": base,
        "ruleStream": {
            "requestUrl": "@js: if (track.playUrl) { '/api/direct-meta'; } else { '/api/resolve?t=' + track.title + ',{\"method\":\"GET\"}'; }",
            "streamUrl": "$.url"
        }
    }))
    .unwrap();

    let engine = super::MusicSourceEngine::new(
        HttpClientConfig {
            allow_private_ips: true,
            ..Default::default()
        },
        Duration::from_secs(5),
    );

    tokio::task::spawn_blocking(move || {
        for title in ["Hello, World", "Song [Remix]", "Track \"Quoted\""] {
            let ctx = MusicTrackContext {
                track_id: "t1".into(),
                title: title.into(),
                artist: "Artist".into(),
                play_url: None,
                ..Default::default()
            };
            let res = engine
                .resolve_stream_sync(&branching_spec, SourceRuntimeState::default(), &ctx, None)
                .unwrap();
            assert_eq!(res.data.stream_url, "https://cdn.example.com/resolved.mp3");
        }

        let evil_ctx = MusicTrackContext {
            track_id: "t1".into(),
            title: r#"x,{"method":"DELETE","headers":{"X-Pwn":"1"}}"#.into(),
            artist: "Artist".into(),
            play_url: None,
            ..Default::default()
        };
        let err = engine
            .resolve_stream_sync(&unsafe_spec, SourceRuntimeState::default(), &evil_ctx, None)
            .expect_err("actual option injection must still be rejected");
        assert!(
            err.message.contains("选项") || err.message.contains("结构"),
            "unexpected error: {err}"
        );
    })
    .await
    .unwrap();

    srv.abort();
}

#[test]
fn music_run_login_action_strips_case_insensitive_js_wrappers_and_preserves_http_login_url() {
    let engine = super::MusicSourceEngine::default();
    let cases = [
        (
            "@JS: function login() { return 'upper-at:' + (result.user || ''); }",
            None,
            Some("login()"),
            "upper-at:bob",
        ),
        (
            "<js>function check() { return 'lower-tag:' + (result.user || ''); }</js>",
            Some("@JS: check()"),
            None,
            "lower-tag:bob",
        ),
        (
            "<JS>function check() { return 'upper-tag:' + (result.user || ''); }</JS>",
            Some("<JS>check()</JS>"),
            None,
            "upper-tag:bob",
        ),
        (
            "https://music.example.test/login",
            Some("<JS>'http-check:' + (result.user || '')</JS>"),
            None,
            "http-check:bob",
        ),
    ];

    for (login_url, login_check_js, action, expected) in cases {
        let spec: MusicSourceSpec = serde_json::from_value(serde_json::json!({
            "sourceName": "LoginWrapperMusic",
            "sourceUrl": "https://music.example.test",
            "loginUrl": login_url,
            "loginCheckJs": login_check_js,
        }))
        .unwrap();
        let out = engine
            .run_login_action_sync(
                &spec,
                SourceRuntimeState::default(),
                HashMap::from([("user".to_string(), "bob".to_string())]),
                action,
            )
            .unwrap();
        assert_eq!(out.data, expected);
    }

    let empty_action_spec: MusicSourceSpec = serde_json::from_value(serde_json::json!({
        "sourceName": "PlainHttpOnlyMusic",
        "sourceUrl": "https://music.example.test",
        "loginUrl": "https://music.example.test/login",
    }))
    .unwrap();
    assert!(
        engine
            .run_login_action_sync(
                &empty_action_spec,
                SourceRuntimeState::default(),
                HashMap::new(),
                None,
            )
            .is_err()
    );
}

#[test]
fn split_top_level_or_handles_consecutive_backslashes_before_closing_quotes() {
    assert_eq!(
        split_top_level_or(r#"{{java.get("missing\\")}} || fallback"#),
        vec![r#"{{java.get("missing\\")}}"#, "fallback"]
    );
    assert_eq!(
        split_top_level_or(r#"{{java.get('missing\\')}} || fallback"#),
        vec![r#"{{java.get('missing\\')}}"#, "fallback"]
    );
    assert_eq!(
        split_top_level_or(r#"{{java.get("a\\\"b || c")}} || fallback"#),
        vec![r#"{{java.get("a\\\"b || c")}}"#, "fallback"]
    );
    assert_eq!(
        split_top_level_or(r#"{{java.get('a\\\'b || c')}} || fallback"#),
        vec![r#"{{java.get('a\\\'b || c')}}"#, "fallback"]
    );
    assert_eq!(
        split_top_level_or(r#"$.don't || $.fallback"#),
        vec!["$.don't", "$.fallback"]
    );
}

#[tokio::test]
async fn music_resolve_stream_handles_backslash_fallback_and_uppercase_quality_in_js_pipelines() {
    use axum::{Router, routing::get};

    let app = Router::new()
        .route("/meta", get(|| async { r#"{"url":"/cdn/meta.mp3"}"# }))
        .route(
            "/flac.mp3",
            get(
                |axum::extract::Query(q): axum::extract::Query<HashMap<String, String>>| async move {
                    let v = q.get("v").cloned().unwrap_or_default();
                    serde_json::json!({ "url": format!("https://cdn.example/req-flac.mp3?v={v}") })
                        .to_string()
                },
            ),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base = format!("http://{addr}");
    let srv = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let base_c = base.clone();
    tokio::task::spawn_blocking(move || {
        let engine = super::MusicSourceEngine::new(
            HttpClientConfig {
                allow_private_ips: true,
                ..Default::default()
            },
            Duration::from_secs(5),
        );
        let ctx = MusicTrackContext {
            track_id: "t1".into(),
            title: "Song".into(),
            artist: "Artist".into(),
            ..Default::default()
        };

        for req_tpl in [
            format!(r#"{{{{java.get("missing\\")}}}} || {base_c}/meta"#),
            format!(r#"{{{{java.get('missing\\')}}}} || {base_c}/meta"#),
        ] {
            let spec: MusicSourceSpec = serde_json::from_value(serde_json::json!({
                "sourceName": "BackslashFallbackMusic",
                "sourceUrl": base_c,
                "ruleStream": {
                    "requestUrl": req_tpl,
                    "streamUrl": "$.url"
                }
            }))
            .unwrap();
            let out = engine
                .resolve_stream_sync(&spec, SourceRuntimeState::default(), &ctx, Some("flac"))
                .unwrap();
            assert_eq!(out.data.stream_url, format!("{base_c}/cdn/meta.mp3"));
        }

        let req_js_spec: MusicSourceSpec = serde_json::from_value(serde_json::json!({
            "sourceName": "QualityNoOptsReqJsMusic",
            "sourceUrl": base_c,
            "ruleStream": {
                "requestUrl": format!("{base_c}/{{{{QUALITY}}}}.mp3@js:result + '?v=1'"),
                "streamUrl": "$.url"
            }
        }))
        .unwrap();
        let req_out = engine
            .resolve_stream_sync(
                &req_js_spec,
                SourceRuntimeState::default(),
                &ctx,
                Some("flac"),
            )
            .unwrap();
        assert_eq!(
            req_out.data.stream_url,
            "https://cdn.example/req-flac.mp3?v=1"
        );

        let fetched_stream_js_spec: MusicSourceSpec = serde_json::from_value(serde_json::json!({
            "sourceName": "QualityStreamJsFetchedMusic",
            "sourceUrl": base_c,
            "ruleStream": {
                "requestUrl": format!("{base_c}/meta"),
                "streamUrl": "https://cdn.example/{{QUALITY}}.mp3@js:result + '?v=1'"
            }
        }))
        .unwrap();
        let fetched_out = engine
            .resolve_stream_sync(
                &fetched_stream_js_spec,
                SourceRuntimeState::default(),
                &ctx,
                Some("flac"),
            )
            .unwrap();
        assert_eq!(
            fetched_out.data.stream_url,
            "https://cdn.example/flac.mp3?v=1"
        );

        let direct_stream_js_spec: MusicSourceSpec = serde_json::from_value(serde_json::json!({
            "sourceName": "QualityStreamJsDirectMusic",
            "sourceUrl": base_c,
            "ruleStream": {
                "streamUrl": "https://cdn.example/{{QUALITY}}.mp3@js:result + '?v=1'"
            }
        }))
        .unwrap();
        let direct_out = engine
            .resolve_stream_sync(
                &direct_stream_js_spec,
                SourceRuntimeState::default(),
                &ctx,
                Some("flac"),
            )
            .unwrap();
        assert_eq!(
            direct_out.data.stream_url,
            "https://cdn.example/flac.mp3?v=1"
        );
    })
    .await
    .unwrap();

    srv.abort();
}

#[tokio::test]
async fn music_fetch_api_clamps_single_and_multistep_requests_to_remaining_budget() {
    use axum::{Router, routing::get};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Instant;

    let slow_stream_hits = Arc::new(AtomicUsize::new(0));
    let step1_hits = Arc::new(AtomicUsize::new(0));
    let step2_hits = Arc::new(AtomicUsize::new(0));

    let slow_stream_hits_c = slow_stream_hits.clone();
    let step1_hits_c = step1_hits.clone();
    let step2_hits_c = step2_hits.clone();

    let app = Router::new()
        .route(
            "/slow-stream",
            get(move || {
                let hits = slow_stream_hits_c.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(1200)).await;
                    r#"{"url":"https://cdn.example/late.mp3"}"#
                }
            }),
        )
        .route(
            "/step1",
            get(move || {
                let hits = step1_hits_c.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(150)).await;
                    r#"{"token":"ok"}"#
                }
            }),
        )
        .route(
            "/step2",
            get(move || {
                let hits = step2_hits_c.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(1200)).await;
                    r#"{"items":[{"id":"1","title":"Late","artist":"Art"}]}"#
                }
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base = format!("http://{addr}");
    let srv = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let base_c = base.clone();
    tokio::task::spawn_blocking(move || {
        let engine = super::MusicSourceEngine::new(
            HttpClientConfig {
                timeout_secs: 2,
                allow_private_ips: true,
                proxy_url: None,
            },
            Duration::from_secs(5),
        );
        let op_timeout = Duration::from_millis(220);
        let cancel = Arc::new(AtomicBool::new(false));

        let stream_spec: MusicSourceSpec = serde_json::from_value(serde_json::json!({
            "sourceName": "SlowStreamBudgetMusic",
            "sourceUrl": base_c,
            "ruleStream": {
                "requestUrl": format!("{base_c}/slow-stream"),
                "streamUrl": "$.url"
            }
        }))
        .unwrap();
        let ctx = MusicTrackContext {
            track_id: "t1".into(),
            title: "Song".into(),
            artist: "Artist".into(),
            ..Default::default()
        };

        let t_single = Instant::now();
        let single_err = engine
            .resolve_stream_sync_with_control(
                &stream_spec,
                SourceRuntimeState::default(),
                &ctx,
                None,
                op_timeout,
                cancel.clone(),
            )
            .expect_err("slow single fetch_api must abort within op_timeout");
        let single_elapsed = t_single.elapsed();
        assert!(
            single_elapsed < Duration::from_millis(500),
            "single fetch_api took {:?} (expected < 500ms), err={single_err}",
            single_elapsed
        );
        assert!(
            single_err.message.contains("超时") || single_err.message.contains("interrupted"),
            "unexpected single_err message: {}",
            single_err.message
        );

        let search_spec: MusicSourceSpec = serde_json::from_value(serde_json::json!({
            "sourceName": "MultiStepBudgetMusic",
            "sourceUrl": base_c,
            "searchUrl": format!("@js:java.ajax('{base_c}/step1'); '{base_c}/step2?q=' + encodeURIComponent(key)"),
            "ruleSearch": {
                "trackList": "$.items[*]",
                "trackId": "$.id",
                "title": "$.title",
                "artist": "$.artist"
            }
        }))
        .unwrap();

        let t_multi = Instant::now();
        let multi_err = engine
            .search_sync_with_control(
                &search_spec,
                SourceRuntimeState::default(),
                "test",
                1,
                10,
                op_timeout,
                cancel.clone(),
            )
            .expect_err("multi-step search fetch_api must abort within remaining budget");
        let multi_elapsed = t_multi.elapsed();
        assert!(
            multi_elapsed < Duration::from_millis(500),
            "multi-step search took {:?} (expected < 500ms), err={multi_err}",
            multi_elapsed
        );
        assert!(
            multi_err.message.contains("超时") || multi_err.message.contains("interrupted"),
            "unexpected multi_err message: {}",
            multi_err.message
        );

        let zero_err = engine
            .resolve_stream_sync_with_control(
                &stream_spec,
                SourceRuntimeState::default(),
                &ctx,
                None,
                Duration::ZERO,
                cancel,
            )
            .expect_err("zero op_timeout must reject before HTTP call");
        assert!(
            zero_err.message.contains("interrupted"),
            "unexpected zero_err message: {}",
            zero_err.message
        );
    })
    .await
    .unwrap();

    assert_eq!(slow_stream_hits.load(Ordering::SeqCst), 1);
    assert_eq!(step1_hits.load(Ordering::SeqCst), 1);
    assert_eq!(step2_hits.load(Ordering::SeqCst), 1);

    srv.abort();
}

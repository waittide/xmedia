use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::Request as AxumRequest;
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::get;
use tempfile::TempDir;
use toasty::Db;
use toasty::schema::ModelSet;
use xmedia_core::application::auth::AuthService;
use xmedia_core::application::dto::SetupAdminInput;
use xmedia_core::infra::persistence::repositories::ToastyUserRepository;
use xmedia_core::infra::security::{JwtTokenService, SecurityManager};
use xmedia_core::{ensure_core_schema, register_core_models};
use xmedia_source::http::HttpClientConfig;
use xmedia_source::model::SourceRuntimeState;

use super::{music_delayed_chunked_response, send_json, setup_online_test_harness};
use crate::domain::catalog::StructuredLyrics;
use crate::domain::online::{MusicSourceSpec, MusicTrackContext, url_encode_component};
use crate::infra::source::MusicSourceEngine;
use crate::{ensure_music_schema, register_music_models};

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
            _ = detail_entered.notified() => {}
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
        .authenticate(
            xmedia_core::application::dto::AuthCredential::Jwt(&admin_session.token),
            None,
        )
        .await
        .unwrap();

    let sources_repo: Arc<dyn xmedia_source::crud::SourceRepository> =
        Arc::new(ToastyMusicSourceRepository::new(db));
    let sources_crud =
        Arc::new(xmedia_source::crud::SourceCrudService::<MusicSourceSpec>::new(sources_repo));
    let engine = Arc::new(MusicSourceEngine::new(
        HttpClientConfig {
            allow_private_ips: true,
            ..Default::default()
        },
        Duration::from_secs(5),
    ));
    let media_proxy = Arc::new(crate::infra::source::HttpMusicMediaProxy::new(true));
    let svc = MusicOnlineService::with_ports(sources_crud.clone(), engine, media_proxy)
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
    let imported = sources_crud
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

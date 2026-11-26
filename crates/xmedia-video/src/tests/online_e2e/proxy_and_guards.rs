use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, StatusCode, header};
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

use crate::{ensure_video_schema, register_video_models};

use super::{
    delayed_chunked_response, fake_mp4_payload, get_bytes_with_auth, post_json, setup_test_app,
    urlencoding_param,
};

#[tokio::test]
async fn rule_play_subtitles_jsonpath_and_js_e2e() {
    let (_tmp, app, admin_tok, member_tok) = setup_test_app(true).await;

    let origin_app = Router::new().route(
        "/api/play",
        get(|| async {
            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "stream": "/media/ep1.mp4",
                        "subs": [
                            { "label": "中文", "lang": "zh", "url": "/subs/zh.vtt" },
                            { "label": "English", "lang": "en", "url": "/subs/en.srt" }
                        ]
                    })
                    .to_string(),
                ))
                .unwrap()
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let origin = format!("http://{addr}");
    let srv = tokio::spawn(async move {
        axum::serve(listener, origin_app).await.unwrap();
    });

    let src_json = serde_json::json!({
        "sourceName": "Rule Subtitles JSONPath",
        "sourceUrl": origin,
        "sourceFormat": "rule",
        "enabled": true,
        "rulePlay": {
            "requestUrl": "{{playUrl}}",
            "streamUrl": "$.stream",
            "subtitles": "$.subs"
        }
    });
    let (st, import_res) = post_json(
        &app,
        "/api/v1/video/sources/import",
        &admin_tok,
        serde_json::json!({ "raw_json": src_json.to_string() }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let sid = import_res["sources"][0]["id"].as_str().unwrap().to_string();

    let (st, play_res) = post_json(
        &app,
        "/api/v1/video/online/play",
        &member_tok,
        serde_json::json!({
            "source_id": sid,
            "play_url": format!("{origin}/api/play")
        }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(play_res["stream_url"], format!("{origin}/media/ep1.mp4"));
    let subs = play_res["subtitles"].as_array().unwrap();
    assert_eq!(subs.len(), 2);
    assert_eq!(subs[0]["label"], "中文");
    assert_eq!(subs[0]["lang"], "zh");
    assert_eq!(subs[0]["url"], format!("{origin}/subs/zh.vtt"));
    assert_eq!(subs[1]["label"], "English");
    assert_eq!(subs[1]["lang"], "en");
    assert_eq!(subs[1]["url"], format!("{origin}/subs/en.srt"));

    srv.abort();
}

#[tokio::test]
async fn hls_proxy_rejects_malformed_playlist_missing_extm3u_header() {
    let (_tmp, app, admin_tok, member_tok) = setup_test_app(true).await;
    let origin_app = Router::new()
        .route(
            "/bad.m3u8",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/vnd.apple.mpegurl")
                    .body(Body::from("not an m3u8 playlist"))
                    .unwrap()
            }),
        )
        .route(
            "/fake-tag.m3u8",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/vnd.apple.mpegurl")
                    .body(Body::from(
                        "#EXTM3U_GARBAGE_NOT_TAG\n#EXTINF:5.0,\nseg0.ts\n",
                    ))
                    .unwrap()
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let origin = format!("http://{addr}");
    let srv = tokio::spawn(async move {
        axum::serve(listener, origin_app).await.unwrap();
    });

    let (st_imp, imp_res) = post_json(
        &app,
        "/api/v1/video/sources/import",
        &admin_tok,
        serde_json::json!({
            "raw_json": serde_json::json!({
                "sourceName": "MalformedHlsSource",
                "sourceUrl": origin,
                "apiUrl": format!("{origin}/api"),
                "enabled": true
            }).to_string()
        }),
    )
    .await;
    assert_eq!(st_imp, StatusCode::OK);
    let sid = imp_res["sources"][0]["id"].as_str().unwrap();

    let encoded_url: String =
        form_urlencoded::byte_serialize(format!("{origin}/bad.m3u8").as_bytes()).collect();
    let (st, _, _) = get_bytes_with_auth(
        &app,
        &format!("/api/v1/video/online/hls-proxy?url={encoded_url}&source_id={sid}"),
        Some(&member_tok),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    let encoded_fake: String =
        form_urlencoded::byte_serialize(format!("{origin}/fake-tag.m3u8").as_bytes()).collect();
    let (st_fake, _, _) = get_bytes_with_auth(
        &app,
        &format!("/api/v1/video/online/hls-proxy?url={encoded_fake}&source_id={sid}"),
        Some(&member_tok),
        None,
    )
    .await;
    assert_eq!(st_fake, StatusCode::BAD_REQUEST);

    srv.abort();
}

#[tokio::test]
async fn hls_proxy_refetches_full_playlist_when_caller_sends_range_on_large_m3u8() {
    let (_tmp, app, admin_tok, member_tok) = setup_test_app(true).await;

    let mut playlist = String::from("#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:6\n");
    for i in 0..120 {
        playlist.push_str(&format!("#EXTINF:6.0,\nseg_{i:03}.ts\n"));
    }
    playlist.push_str("#EXT-X-ENDLIST\n");
    assert!(playlist.len() > 2048);

    let full_bytes = Arc::new(playlist.into_bytes());
    let seen_ranges: Arc<Mutex<Vec<Option<String>>>> = Arc::new(Mutex::new(Vec::new()));
    let seen_ranges_srv = seen_ranges.clone();
    let full_bytes_srv = full_bytes.clone();

    let origin_app = Router::new().route(
        "/large.m3u8",
        get(move |headers: HeaderMap| {
            let full = full_bytes_srv.clone();
            let log = seen_ranges_srv.clone();
            async move {
                let rng = headers
                    .get(header::RANGE)
                    .and_then(|v| v.to_str().ok())
                    .map(ToString::to_string);
                log.lock().unwrap().push(rng.clone());

                if let Some(r) = rng {
                    let spec = r.trim().strip_prefix("bytes=").unwrap_or("0-511");
                    let (s, e) = spec.split_once('-').unwrap_or(("0", "511"));
                    let start: usize = s.parse().unwrap_or(0);
                    let end: usize = e.parse().unwrap_or(511).min(full.len().saturating_sub(1));
                    let slice = full[start..=end].to_vec();
                    return Response::builder()
                        .status(StatusCode::PARTIAL_CONTENT)
                        .header("content-type", "application/vnd.apple.mpegurl")
                        .header(
                            "content-range",
                            format!("bytes {start}-{end}/{}", full.len()),
                        )
                        .body(Body::from(slice))
                        .unwrap();
                }

                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/vnd.apple.mpegurl")
                    .body(Body::from(full.as_ref().clone()))
                    .unwrap()
            }
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let origin = format!("http://{addr}");
    let srv = tokio::spawn(async move {
        axum::serve(listener, origin_app).await.unwrap();
    });

    let (st_imp, imp_res) = post_json(
        &app,
        "/api/v1/video/sources/import",
        &admin_tok,
        serde_json::json!({
            "raw_json": serde_json::json!({
                "sourceName": "LargeRangeHlsSource",
                "sourceUrl": origin,
                "apiUrl": format!("{origin}/api"),
                "enabled": true
            }).to_string()
        }),
    )
    .await;
    assert_eq!(st_imp, StatusCode::OK);
    let sid = imp_res["sources"][0]["id"].as_str().unwrap();

    let encoded_url: String =
        form_urlencoded::byte_serialize(format!("{origin}/large.m3u8").as_bytes()).collect();

    let (st1, _, body1) = get_bytes_with_auth(
        &app,
        &format!("/api/v1/video/online/hls-proxy?url={encoded_url}&source_id={sid}"),
        Some(&member_tok),
        Some("bytes=0-511"),
    )
    .await;
    assert_eq!(st1, StatusCode::OK);
    let text1 = String::from_utf8(body1).unwrap();
    assert!(text1.starts_with("#EXTM3U\n"));
    assert!(text1.contains("seg_000.ts"));
    assert!(text1.contains("seg_119.ts"));
    assert!(text1.contains("#EXT-X-ENDLIST"));
    assert!(text1.contains("/api/v1/video/online/hls-proxy?url="));

    let (st2, _, body2) = get_bytes_with_auth(
        &app,
        &format!("/api/v1/video/online/hls-proxy?url={encoded_url}&source_id={sid}"),
        Some(&member_tok),
        Some("bytes=200-711"),
    )
    .await;
    assert_eq!(st2, StatusCode::OK);
    let text2 = String::from_utf8(body2).unwrap();
    assert!(text2.starts_with("#EXTM3U\n"));
    assert!(text2.contains("seg_119.ts"));
    assert!(text2.contains("#EXT-X-ENDLIST"));

    let recorded = seen_ranges.lock().unwrap().clone();
    assert_eq!(
        recorded,
        vec![
            Some("bytes=0-511".to_string()),
            None,
            Some("bytes=200-711".to_string()),
            None,
        ]
    );

    srv.abort();
}

#[tokio::test]
async fn proxy_rejects_xml_declared_svg_and_refetches_extensionless_octet_stream_range_hls_e2e() {
    let (_tmp, app, admin_tok, member_tok) = setup_test_app(true).await;

    let mut manifest = String::from("#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:6\n");
    for i in 0..80 {
        manifest.push_str(&format!("#EXTINF:6.0,\nseg_{i:03}.ts\n"));
    }
    manifest.push_str("#EXT-X-ENDLIST\n");
    let manifest_bytes = Arc::new(manifest.into_bytes());
    let manifest_srv = manifest_bytes.clone();

    let origin_app = Router::new()
        .route(
            "/xml-svg.bin",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/octet-stream")
                    .body(Body::from(
                        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><svg xmlns=\"http://www.w3.org/2000/svg\"><script>alert(1)</script></svg>",
                    ))
                    .unwrap()
            }),
        )
        .route(
            "/xml-svg-split.bin",
            get(|| async {
                delayed_chunked_response(
                    "application/octet-stream",
                    vec![
                        (b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>".to_vec(), 0),
                        (
                            b"<svg xmlns=\"http://www.w3.org/2000/svg\"><script>alert(1)</script></svg>"
                                .to_vec(),
                            10,
                        ),
                    ],
                )
            }),
        )
        .route(
            "/manifest",
            get(move |headers: HeaderMap| {
                let full = manifest_srv.clone();
                async move {
                    if let Some(r) = headers.get(header::RANGE).and_then(|v| v.to_str().ok()) {
                        let spec = r.trim().strip_prefix("bytes=").unwrap_or("10-500");
                        let (s, e) = spec.split_once('-').unwrap_or(("10", "500"));
                        let start: usize = s.parse().unwrap_or(10);
                        let end: usize = e.parse().unwrap_or(500).min(full.len().saturating_sub(1));
                        let slice = full[start..=end].to_vec();
                        return Response::builder()
                            .status(StatusCode::PARTIAL_CONTENT)
                            .header("content-type", "application/octet-stream")
                            .header(
                                "content-range",
                                format!("bytes {start}-{end}/{}", full.len()),
                            )
                            .body(Body::from(slice))
                            .unwrap();
                    }
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/octet-stream")
                        .body(Body::from(full.as_ref().clone()))
                        .unwrap()
                }
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let origin = format!("http://{addr}");
    let srv = tokio::spawn(async move {
        axum::serve(listener, origin_app).await.unwrap();
    });

    let (st_imp, imp_res) = post_json(
        &app,
        "/api/v1/video/sources/import",
        &admin_tok,
        serde_json::json!({
            "raw_json": serde_json::json!({
                "sourceName": "XmlSvgGuardSource",
                "sourceUrl": origin,
                "apiUrl": format!("{origin}/api"),
                "enabled": true
            }).to_string()
        }),
    )
    .await;
    assert_eq!(st_imp, StatusCode::OK);
    let sid = imp_res["sources"][0]["id"].as_str().unwrap();

    for endpoint in ["stream-proxy", "hls-proxy"] {
        for path in ["/xml-svg.bin", "/xml-svg-split.bin"] {
            let encoded = urlencoding_param(&format!("{origin}{path}"));
            let (st, _, _) = get_bytes_with_auth(
                &app,
                &format!("/api/v1/video/online/{endpoint}?url={encoded}&source_id={sid}"),
                Some(&member_tok),
                None,
            )
            .await;
            assert_eq!(
                st,
                StatusCode::BAD_REQUEST,
                "expected 400 on {endpoint} for {path}"
            );
        }
    }

    let encoded_manifest = urlencoding_param(&format!("{origin}/manifest"));
    let (st_hls, hdrs_hls, body_hls) = get_bytes_with_auth(
        &app,
        &format!("/api/v1/video/online/hls-proxy?url={encoded_manifest}&source_id={sid}"),
        Some(&member_tok),
        Some("bytes=10-500"),
    )
    .await;
    assert_eq!(st_hls, StatusCode::OK);
    let ct = hdrs_hls
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        ct.contains("mpegurl"),
        "expected mpegurl content-type, got {ct}"
    );
    let rewritten = String::from_utf8(body_hls).unwrap();
    assert!(rewritten.starts_with("#EXTM3U\n"));
    assert!(rewritten.contains("seg_079.ts"));
    assert!(rewritten.contains("/api/v1/video/online/hls-proxy?url="));

    srv.abort();
}

#[tokio::test]
async fn video_caller_url_guards_chunked_partial_hls_and_utf16_comment_proxy_e2e() {
    let (_tmp, app, admin_tok, member_tok) = setup_test_app(true).await;

    let src_json = serde_json::json!([{
        "sourceName": "GuardVideoSource",
        "sourceUrl": "https://video-guard.example",
        "sourceFormat": "rule",
        "searchUrl": "/search?q={{key}}",
        "ruleSearch": { "list": "$.items", "title": "$.title", "detailUrl": "$.url" },
        "ruleDetail": { "title": "$.title" },
        "rulePlay": { "requestUrl": "{{playUrl}}", "streamUrl": "$.url" }
    }]);
    let (st_imp, imp_res) = post_json(
        &app,
        "/api/v1/video/sources/import",
        &admin_tok,
        serde_json::json!({ "raw_json": src_json.to_string() }),
    )
    .await;
    assert_eq!(st_imp, StatusCode::OK);
    let sid = imp_res["sources"][0]["id"].as_str().unwrap().to_string();

    for bad_url in [
        "https://video-guard.example/v/1,{\"method\":\"POST\",\"headers\":{\"X-Evil\":\"1\"}}",
        "data:text/html,<h1>evil</h1>",
    ] {
        let (st_d, _) = post_json(
            &app,
            "/api/v1/video/online/detail",
            &member_tok,
            serde_json::json!({ "source_id": sid, "detail_url": bad_url }),
        )
        .await;
        assert_eq!(
            st_d,
            StatusCode::BAD_REQUEST,
            "expected 400 on detail for {bad_url}"
        );

        let (st_p, _) = post_json(
            &app,
            "/api/v1/video/online/play",
            &member_tok,
            serde_json::json!({ "source_id": sid, "play_url": bad_url }),
        )
        .await;
        assert_eq!(
            st_p,
            StatusCode::BAD_REQUEST,
            "expected 400 on play for {bad_url}"
        );
    }

    let full_playlist = "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:5\n#EXTINF:5.0,\nsegment_0001_very_long_url.ts\n#EXTINF:5.0,\nsegment_0002.ts\n#EXT-X-ENDLIST\n";
    let origin_app = Router::new()
        .route(
            "/chunked-manifest",
            get(move |headers: HeaderMap| async move {
                if let Some(r) = headers.get(header::RANGE).and_then(|v| v.to_str().ok()) {
                    let _ = r;
                    let mut resp = delayed_chunked_response(
                        "application/octet-stream",
                        vec![
                            (b"ment_0001_very_long_url.ts\n".to_vec(), 0),
                            (
                                b"#EXTINF:5.0,\nsegment_0002.ts\n#EXT-X-ENDLIST\n".to_vec(),
                                5,
                            ),
                        ],
                    );
                    *resp.status_mut() = StatusCode::PARTIAL_CONTENT;
                    resp.headers_mut().insert(
                        header::CONTENT_RANGE,
                        header::HeaderValue::from_static("bytes 24-120/240"),
                    );
                    return resp;
                }
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/octet-stream")
                    .body(Body::from(full_playlist))
                    .unwrap()
            }),
        )
        .route(
            "/utf16le-svg.bin",
            get(|| async {
                let mut payload = vec![0xFFu8, 0xFE];
                for u in "<svg xmlns=\"http://www.w3.org/2000/svg\"><script>alert(1)</script></svg>"
                    .encode_utf16()
                {
                    payload.extend_from_slice(&u.to_le_bytes());
                }
                delayed_chunked_response(
                    "application/octet-stream",
                    vec![(payload[..4].to_vec(), 0), (payload[4..].to_vec(), 5)],
                )
            }),
        )
        .route(
            "/utf16le-1byte-svg.bin",
            get(|| async {
                let mut payload = vec![0xFFu8, 0xFE];
                for u in "<svg xmlns=\"http://www.w3.org/2000/svg\"><script>alert(1)</script></svg>"
                    .encode_utf16()
                {
                    payload.extend_from_slice(&u.to_le_bytes());
                }
                delayed_chunked_response(
                    "application/octet-stream",
                    vec![
                        (payload[..1].to_vec(), 0),
                        (payload[1..4].to_vec(), 5),
                        (payload[4..].to_vec(), 5),
                    ],
                )
            }),
        )
        .route(
            "/utf16be-svg.bin",
            get(|| async {
                let mut payload = vec![0xFEu8, 0xFF];
                for u in "<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>".encode_utf16() {
                    payload.extend_from_slice(&u.to_be_bytes());
                }
                delayed_chunked_response(
                    "application/octet-stream",
                    vec![(payload[..4].to_vec(), 0), (payload[4..].to_vec(), 5)],
                )
            }),
        )
        .route(
            "/large-comment-svg.bin",
            get(|| async {
                let mut chunk1 = b"<!-- ".to_vec();
                chunk1.extend_from_slice(&vec![b'X'; 4600]);
                let chunk2 =
                    b" --><svg xmlns=\"http://www.w3.org/2000/svg\"><script>alert(1)</script></svg>"
                        .to_vec();
                delayed_chunked_response("application/octet-stream", vec![(chunk1, 0), (chunk2, 5)])
            }),
        )
        .route(
            "/large-ws-svg.bin",
            get(|| async {
                let chunk1 = vec![b' '; 4600];
                let chunk2 =
                    b"<svg xmlns=\"http://www.w3.org/2000/svg\"><script>alert(1)</script></svg>"
                        .to_vec();
                delayed_chunked_response("application/octet-stream", vec![(chunk1, 0), (chunk2, 5)])
            }),
        )
        .route(
            "/utf16le-ws-svg.bin",
            get(|| async {
                let mut payload = vec![0xFFu8, 0xFE];
                for _ in 0..2400 {
                    payload.extend_from_slice(&u16::from(b' ').to_le_bytes());
                }
                for u in "<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>".encode_utf16() {
                    payload.extend_from_slice(&u.to_le_bytes());
                }
                delayed_chunked_response(
                    "application/octet-stream",
                    vec![(payload[..4200].to_vec(), 0), (payload[4200..].to_vec(), 5)],
                )
            }),
        )
        .route(
            "/live",
            get(|| async {
                delayed_chunked_response(
                    "text/plain",
                    vec![
                        (b"#EXT".to_vec(), 0),
                        (b"M3U\n#EXTINF:6.0,\nseg0.ts\n#EXT-X-ENDLIST\n".to_vec(), 5),
                    ],
                )
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let origin = format!("http://{addr}");
    let srv = tokio::spawn(async move {
        axum::serve(listener, origin_app).await.unwrap();
    });

    let encoded_manifest = urlencoding_param(&format!("{origin}/chunked-manifest"));
    let (st_hls, hdrs_hls, body_hls) = get_bytes_with_auth(
        &app,
        &format!("/api/v1/video/online/hls-proxy?url={encoded_manifest}&source_id={sid}"),
        Some(&member_tok),
        Some("bytes=24-120"),
    )
    .await;
    assert_eq!(st_hls, StatusCode::OK);
    assert!(
        hdrs_hls
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .contains("mpegurl")
    );
    let rewritten = String::from_utf8(body_hls).unwrap();
    assert!(rewritten.starts_with("#EXTM3U\n"));
    assert!(rewritten.contains("segment_0001_very_long_url.ts"));

    let (st_stream_hls, _, _) = get_bytes_with_auth(
        &app,
        &format!("/api/v1/video/online/stream-proxy?url={encoded_manifest}&source_id={sid}"),
        Some(&member_tok),
        Some("bytes=24-120"),
    )
    .await;
    assert_eq!(st_stream_hls, StatusCode::BAD_REQUEST);

    for endpoint in ["stream-proxy", "hls-proxy"] {
        for path in [
            "/utf16le-svg.bin",
            "/utf16le-1byte-svg.bin",
            "/utf16be-svg.bin",
            "/large-comment-svg.bin",
            "/large-ws-svg.bin",
            "/utf16le-ws-svg.bin",
        ] {
            let encoded = urlencoding_param(&format!("{origin}{path}"));
            let (st, _, _) = get_bytes_with_auth(
                &app,
                &format!("/api/v1/video/online/{endpoint}?url={encoded}&source_id={sid}"),
                Some(&member_tok),
                None,
            )
            .await;
            assert_eq!(
                st,
                StatusCode::BAD_REQUEST,
                "expected 400 on {endpoint} for {path}"
            );
        }
    }

    let encoded_live = urlencoding_param(&format!("{origin}/live"));
    let (st_live_hls, hdrs_live_hls, body_live_hls) = get_bytes_with_auth(
        &app,
        &format!("/api/v1/video/online/hls-proxy?url={encoded_live}&source_id={sid}"),
        Some(&member_tok),
        None,
    )
    .await;
    assert_eq!(st_live_hls, StatusCode::OK);
    assert!(
        hdrs_live_hls
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .contains("mpegurl")
    );
    let rewritten_live = String::from_utf8(body_live_hls).unwrap();
    assert!(rewritten_live.starts_with("#EXTM3U\n"));
    assert!(rewritten_live.contains("/api/v1/video/online/hls-proxy?url="));

    let (st_live_stream, _, _) = get_bytes_with_auth(
        &app,
        &format!("/api/v1/video/online/stream-proxy?url={encoded_live}&source_id={sid}"),
        Some(&member_tok),
        None,
    )
    .await;
    assert_eq!(st_live_stream, StatusCode::BAD_REQUEST);

    srv.abort();
}

#[tokio::test]
async fn video_online_search_enforces_source_concurrent_rate_throttling() {
    let (_tmp, app, admin_tok, member_tok) = setup_test_app(true).await;

    let origin_app = Router::new().route(
        "/api/search",
        get(|| async {
            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"items":[{"id":"v1","title":"RatedVideo","url":"/v/1"}]}"#,
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
        "sourceName": "RateLimitedVideo",
        "sourceUrl": base,
        "sourceFormat": "rule",
        "concurrentRate": "1/200",
        "searchUrl": format!("{base}/api/search?q={{{{key}}}}"),
        "ruleSearch": {
            "list": "$.items[*]",
            "vodId": "$.id",
            "name": "$.title",
            "detailUrl": "$.url"
        }
    });
    let (st_imp, imp_val) = post_json(
        &app,
        "/api/v1/video/sources/import",
        &admin_tok,
        serde_json::json!({ "raw_json": src.to_string() }),
    )
    .await;
    assert_eq!(st_imp, StatusCode::OK);
    let sid = imp_val["sources"][0]["id"].as_str().unwrap().to_string();

    let start = std::time::Instant::now();
    let (r1, r2) = tokio::join!(
        post_json(
            &app,
            "/api/v1/video/online/search",
            &member_tok,
            serde_json::json!({ "keyword": "a", "source_ids": [&sid] }),
        ),
        post_json(
            &app,
            "/api/v1/video/online/search",
            &member_tok,
            serde_json::json!({ "keyword": "b", "source_ids": [&sid] }),
        ),
    );
    let elapsed = start.elapsed();
    assert_eq!(r1.0, StatusCode::OK);
    assert_eq!(r2.0, StatusCode::OK);
    assert!(
        elapsed >= std::time::Duration::from_millis(160),
        "expected concurrentRate 1/200 to throttle 2 concurrent calls, took {:?}",
        elapsed
    );

    srv.abort();
}

#[tokio::test]
async fn subtitle_proxy_serves_vtt_converts_srt_injects_headers_and_enforces_security_e2e() {
    let (_tmp, app, admin_tok, member_tok) = setup_test_app(true).await;

    let origin_app = Router::new()
        .route(
            "/api/play",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "stream": "/media/ep1.mp4",
                            "subs": [
                                { "label": "English", "lang": "en", "url": "/subs/en.srt" },
                                { "label": "中文", "lang": "zh", "url": "/subs/zh.vtt" }
                            ]
                        })
                        .to_string(),
                    ))
                    .unwrap()
            }),
        )
        .route(
            "/subs/en.srt",
            get(|headers: HeaderMap| async move {
                if headers.get(header::REFERER).and_then(|v| v.to_str().ok())
                    != Some("https://sub.origin.test/")
                {
                    return Response::builder()
                        .status(StatusCode::FORBIDDEN)
                        .body(Body::from("missing referer"))
                        .unwrap();
                }
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/plain; charset=utf-8")
                    .body(Body::from(
                        "1\n00:00:01,000 --> 00:00:03,500\nHello, Subtitle!\n",
                    ))
                    .unwrap()
            }),
        )
        .route(
            "/subs/zh.vtt",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/vtt; charset=utf-8")
                    .body(Body::from(
                        "WEBVTT\n\n00:00:01.000 --> 00:00:03.500\n你好字幕\n",
                    ))
                    .unwrap()
            }),
        )
        .route(
            "/subs/evil.vtt",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/plain")
                    .body(Body::from(
                        "WEBVTT\n\n00:00:01.000 --> 00:00:02.000\n<svg onload=alert(1)>\n",
                    ))
                    .unwrap()
            }),
        )
        .route(
            "/subs/huge.vtt",
            get(|| async {
                let mut body = b"WEBVTT\n\n00:00:01.000 --> 00:00:02.000\n".to_vec();
                body.extend(vec![b'x'; 16 * 1024 * 1024 + 128]);
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/vtt")
                    .body(Body::from(body))
                    .unwrap()
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let origin = format!("http://{addr}");
    let srv = tokio::spawn(async move {
        axum::serve(listener, origin_app).await.unwrap();
    });

    let src_json = serde_json::json!({
        "sourceName": "Subtitle Proxy Source",
        "sourceUrl": origin,
        "sourceFormat": "rule",
        "mediaHeader": { "Referer": "https://sub.origin.test/" },
        "rulePlay": {
            "requestUrl": "{{playUrl}}",
            "streamUrl": "$.stream",
            "subtitles": "$.subs"
        }
    });
    let (st_imp, imp_res) = post_json(
        &app,
        "/api/v1/video/sources/import",
        &admin_tok,
        serde_json::json!({ "raw_json": src_json.to_string() }),
    )
    .await;
    assert_eq!(st_imp, StatusCode::OK);
    let sid = imp_res["sources"][0]["id"].as_str().unwrap().to_string();

    let (st_play, play_res) = post_json(
        &app,
        "/api/v1/video/online/play",
        &member_tok,
        serde_json::json!({
            "source_id": sid,
            "play_url": format!("{origin}/api/play")
        }),
    )
    .await;
    assert_eq!(st_play, StatusCode::OK);
    let subs = play_res["subtitles"].as_array().unwrap();
    assert_eq!(subs.len(), 2);
    let srt_proxy = subs[0]["proxy_url"].as_str().unwrap();
    assert!(srt_proxy.starts_with("/api/v1/video/online/subtitle-proxy?url="));

    let (st_srt, hdrs_srt, body_srt) =
        get_bytes_with_auth(&app, srt_proxy, Some(&member_tok), None).await;
    assert_eq!(st_srt, StatusCode::OK);
    assert_eq!(
        hdrs_srt
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("text/vtt; charset=utf-8")
    );
    assert_eq!(
        hdrs_srt
            .get(header::X_CONTENT_TYPE_OPTIONS)
            .and_then(|v| v.to_str().ok()),
        Some("nosniff")
    );
    assert_eq!(
        hdrs_srt
            .get(header::CONTENT_SECURITY_POLICY)
            .and_then(|v| v.to_str().ok()),
        Some("sandbox; default-src 'none'")
    );
    let srt_text = String::from_utf8(body_srt).unwrap();
    assert!(srt_text.starts_with("WEBVTT\n\n"));
    assert!(srt_text.contains("00:00:01.000 --> 00:00:03.500\nHello, Subtitle!"));

    let vtt_proxy = subs[1]["proxy_url"].as_str().unwrap();
    let (st_vtt, _, body_vtt) = get_bytes_with_auth(&app, vtt_proxy, Some(&member_tok), None).await;
    assert_eq!(st_vtt, StatusCode::OK);
    assert!(String::from_utf8(body_vtt).unwrap().contains("你好字幕"));

    let (st_unauth, _, _) = get_bytes_with_auth(&app, srt_proxy, None, None).await;
    assert_eq!(st_unauth, StatusCode::UNAUTHORIZED);

    for bad_path in ["/subs/evil.vtt", "/subs/huge.vtt"] {
        let encoded = urlencoding_param(&format!("{origin}{bad_path}"));
        let (st_bad, _, _) = get_bytes_with_auth(
            &app,
            &format!("/api/v1/video/online/subtitle-proxy?url={encoded}&source_id={sid}"),
            Some(&member_tok),
            None,
        )
        .await;
        assert_eq!(
            st_bad,
            StatusCode::BAD_REQUEST,
            "expected 400 for {bad_path}"
        );
    }

    let (_tmp_h, app_hardened, adm_h, mem_h) = setup_test_app(false).await;
    let (st_imp_h, imp_h) = post_json(
        &app_hardened,
        "/api/v1/video/sources/import",
        &adm_h,
        serde_json::json!({
            "raw_json": serde_json::json!({
                "sourceName": "HardenedSubSource",
                "sourceUrl": "https://sub.example.test",
                "apiUrl": "https://sub.example.test/api",
                "enabled": true
            }).to_string()
        }),
    )
    .await;
    assert_eq!(st_imp_h, StatusCode::OK);
    let sid_h = imp_h["sources"][0]["id"].as_str().unwrap();

    let encoded_loop = urlencoding_param(&format!("{origin}/subs/zh.vtt"));
    let (st_ssrf, _, _) = get_bytes_with_auth(
        &app_hardened,
        &format!("/api/v1/video/online/subtitle-proxy?url={encoded_loop}&source_id={sid_h}"),
        Some(&mem_h),
        None,
    )
    .await;
    assert_eq!(st_ssrf, StatusCode::BAD_REQUEST);

    srv.abort();
}

#[tokio::test]
async fn video_online_endpoints_enforce_cancel_on_future_drop() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    let step1_entered = Arc::new(tokio::sync::Notify::new());
    let step1_entered_c = step1_entered.clone();
    let step2_hits = Arc::new(AtomicUsize::new(0));
    let step2_hits_c = step2_hits.clone();

    let origin_app = Router::new()
        .route(
            "/step1",
            get(move || {
                let notify = step1_entered_c.clone();
                async move {
                    notify.notify_waiters();
                    tokio::time::sleep(Duration::from_millis(220)).await;
                    r#"{"items":[{"id":"v1","title":"Item"}],"cats":[{"id":"c1","name":"Cat"}],"title":"Video","stream":"https://cdn.example/v.mp4"}"#
                }
            }),
        )
        .route(
            "/step2",
            get(move || {
                let hits = step2_hits_c.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    r#"{"ok":true,"items":[{"id":"v1","title":"Step2"}],"cats":[{"id":"c1","name":"Cat2"}],"title":"Step2","stream":"https://cdn.example/v2.mp4"}"#
                }
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base = format!("http://{addr}");
    let srv = tokio::spawn(async move {
        axum::serve(listener, origin_app).await.unwrap();
    });

    let (_tmp, app, admin_tok, member_tok) = setup_test_app(true).await;

    let src_json = serde_json::json!({
        "sourceName": "DropVideoSource",
        "sourceUrl": base,
        "sourceFormat": "rule",
        "searchable": true,
        "enabledExplore": true,
        "categoriesUrl": format!("{base}/step1"),
        "ruleCategories": {
            "list": "$.cats[*]",
            "id": "$.id",
            "name": "@js:java.ajax(baseUrl.replace('/step1', '/step2')); 'Cat'"
        },
        "searchUrl": format!("{base}/step1?q={{{{key}}}}"),
        "ruleSearch": {
            "list": "$.items[*]",
            "title": "@js:java.ajax(baseUrl.replace('/step1', '/step2')); 'Title'",
            "videoId": "$.id"
        },
        "exploreUrl": format!("{base}/step1?p={{{{page}}}}"),
        "ruleExplore": {
            "list": "$.items[*]",
            "title": "@js:java.ajax(baseUrl.replace('/step1', '/step2')); 'Browse'",
            "videoId": "$.id"
        },
        "ruleDetail": {
            "requestUrl": format!("{base}/step1"),
            "name": "@js:java.ajax(baseUrl.replace('/step1', '/step2')); 'Detail'",
            "playUrl": "https://cdn.example/ep1.mp4"
        },
        "rulePlay": {
            "requestUrl": format!("{base}/step1"),
            "streamUrl": "@js:java.ajax(baseUrl.replace('/step1', '/step2')); 'https://cdn.example/v.mp4'"
        }
    });

    let (st_imp, imp_res) = post_json(
        &app,
        "/api/v1/video/sources/import",
        &admin_tok,
        serde_json::json!({ "raw_json": src_json.to_string() }),
    )
    .await;
    assert_eq!(st_imp, StatusCode::OK);
    let sid = imp_res["sources"][0]["id"].as_str().unwrap().to_string();

    for (uri, payload) in [
        (
            "/api/v1/video/online/categories",
            serde_json::json!({ "source_id": sid }),
        ),
        (
            "/api/v1/video/online/browse",
            serde_json::json!({ "source_id": sid, "page": 1 }),
        ),
        (
            "/api/v1/video/online/detail",
            serde_json::json!({ "source_id": sid, "video_id": "v1" }),
        ),
        (
            "/api/v1/video/online/play",
            serde_json::json!({ "source_id": sid, "play_url": format!("{base}/ep1") }),
        ),
        (
            "/api/v1/video/online/search",
            serde_json::json!({ "keyword": "drop", "source_ids": [sid] }),
        ),
    ] {
        let before_hits = step2_hits.load(Ordering::SeqCst);
        {
            let fut = post_json(&app, uri, &member_tok, payload);
            tokio::pin!(fut);
            tokio::select! {
                _ = &mut fut => panic!("{uri} completed before drop"),
                _ = step1_entered.notified() => {}
            }
        }
        tokio::time::sleep(Duration::from_millis(320)).await;
        assert_eq!(
            step2_hits.load(Ordering::SeqCst),
            before_hits,
            "dropping {uri} mid-flight must cancel/abort before step 2 java.ajax"
        );
    }

    srv.abort();
}

#[tokio::test]
async fn video_online_endpoints_enforce_timeout_on_stalled_upstream() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;
    use xmedia_core::domain::error::DomainError;
    use xmedia_source::http::HttpClientConfig;

    use crate::application::online::VideoOnlineService;
    use crate::http::online::RestVideoProxyUrlBuilder;
    use crate::infra::persistence::repositories::ToastyVideoSourceRepository;
    use crate::infra::source::{HttpVideoStreamTransport, VideoSourceEngine};

    let after_hits = Arc::new(AtomicUsize::new(0));
    let after_hits_c = after_hits.clone();

    let origin_app = Router::new()
        .route(
            "/hop1",
            get(|| async {
                tokio::time::sleep(Duration::from_millis(90)).await;
                r#"{"next":"/stall"}"#
            }),
        )
        .route(
            "/stall",
            get(|| async {
                tokio::time::sleep(Duration::from_secs(2)).await;
                r#"{"cats":[{"id":"1","name":"Cat"}],"items":[{"id":"1","title":"Late"}],"title":"Late","stream":"https://cdn.example/late.mp4"}"#
            }),
        )
        .route(
            "/after",
            get(move || {
                let hits = after_hits_c.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    r#"{"ok":true}"#
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
    let db_path = tmp.path().join("xmedia_video_timeout.db");
    let db_url = format!("sqlite:{}", db_path.display());
    let mut models = ModelSet::new();
    register_core_models(&mut models);
    register_video_models(&mut models);
    let mut db = Db::builder().models(models).connect(&db_url).await.unwrap();
    ensure_core_schema(&mut db, &db_url).await.unwrap();
    ensure_video_schema(&mut db, &db_url).await.unwrap();

    let users_repo = Arc::new(ToastyUserRepository::new(db.clone()));
    let lifecycles = Arc::new(RwLock::new(HashMap::new()));
    let security = Arc::new(SecurityManager::new("video-timeout-secret"));
    let tokens = Arc::new(JwtTokenService::new("video-timeout-secret", 3600));
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

    let sources_repo = Arc::new(ToastyVideoSourceRepository::new(db));
    let source_crud = Arc::new(xmedia_source::crud::SourceCrudService::new(sources_repo));
    let http_cfg = HttpClientConfig {
        allow_private_ips: true,
        ..Default::default()
    };
    let engine = Arc::new(VideoSourceEngine::new(
        http_cfg.clone(),
        Duration::from_secs(5),
    ));
    let transport = Arc::new(HttpVideoStreamTransport::new(http_cfg));
    let svc = VideoOnlineService::new(source_crud, engine, transport)
        .with_timeouts(Duration::from_millis(120), Duration::from_millis(160));

    let raw_src = serde_json::json!({
        "sourceName": "StalledMultiHopVideo",
        "sourceUrl": base,
        "sourceFormat": "rule",
        "enabledExplore": true,
        "categoriesUrl": format!("<js>java.ajax('{base}/hop1'); '{base}/stall'</js>"),
        "ruleCategories": {
            "list": "$.cats[*]",
            "id": "$.id",
            "name": "@js:java.ajax(baseUrl.replace('/stall', '/after')); 'Cat'"
        },
        "exploreUrl": format!("<js>java.ajax('{base}/hop1'); '{base}/stall'</js>"),
        "ruleExplore": {
            "list": "$.items[*]",
            "title": "@js:java.ajax(baseUrl.replace('/stall', '/after')); 'Item'",
            "videoId": "$.id"
        },
        "ruleDetail": {
            "requestUrl": format!("<js>java.ajax('{base}/hop1'); '{base}/stall'</js>"),
            "name": "@js:java.ajax(baseUrl.replace('/stall', '/after')); 'Detail'",
            "playUrl": "https://cdn.example/ep1.mp4"
        },
        "rulePlay": {
            "requestUrl": format!("<js>java.ajax('{base}/hop1'); '{base}/stall'</js>"),
            "streamUrl": "@js:java.ajax(baseUrl.replace('/stall', '/after')); 'https://cdn.example/v.mp4'"
        }
    });
    let imported = svc
        .import_sources(&admin, &raw_src.to_string())
        .await
        .unwrap();
    let sid = imported.sources[0].id;

    let t_start = std::time::Instant::now();
    let cat_err = svc.categories(&admin, sid).await.unwrap_err();
    assert!(matches!(cat_err, DomainError::Validation(ref m) if m.contains("超时")));

    let browse_err = svc
        .browse(&admin, sid, None, 1, &RestVideoProxyUrlBuilder)
        .await
        .unwrap_err();
    assert!(matches!(browse_err, DomainError::Validation(ref m) if m.contains("超时")));

    let detail_err = svc
        .detail(
            &admin,
            sid,
            Some("1"),
            None,
            Some(&RestVideoProxyUrlBuilder),
        )
        .await
        .unwrap_err();
    assert!(matches!(detail_err, DomainError::Validation(ref m) if m.contains("超时")));

    let play_err = svc
        .play(
            &admin,
            sid,
            &format!("{base}/ep1"),
            None,
            &RestVideoProxyUrlBuilder,
        )
        .await
        .unwrap_err();
    assert!(matches!(play_err, DomainError::Validation(ref m) if m.contains("超时")));

    assert!(
        t_start.elapsed() < Duration::from_secs(1),
        "all 4 timed-out video calls should finish well under 2s stall, took {:?}",
        t_start.elapsed()
    );
    assert_eq!(after_hits.load(Ordering::SeqCst), 0);

    srv.abort();
}

#[tokio::test]
async fn video_play_headers_applied_matches_build_sanitized_headers() {
    let (_tmp, app, admin_tok, member_tok) = setup_test_app(true).await;

    let origin_app = Router::new()
        .route(
            "/api/play",
            get(|| async { r#"{"stream":"/media/video.mp4"}"# }),
        )
        .route(
            "/media/video.mp4",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "video/mp4")
                    .body(Body::from(fake_mp4_payload()))
                    .unwrap()
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let origin = format!("http://{addr}");
    let srv = tokio::spawn(async move {
        axum::serve(listener, origin_app).await.unwrap();
    });

    let forbidden_applecms = serde_json::json!({
        "sourceName": "ForbiddenHeadersApplecms",
        "apiUrl": format!("{origin}/applecms/api.php/provide/vod/"),
        "header": r#"{"Host":"evil.com","Connection":"close"}"#,
        "mediaHeader": r#"{"Transfer-Encoding":"chunked","Bad\nName":"v","X-Newline":"a\nb"}"#
    });
    let forbidden_rule = serde_json::json!({
        "sourceName": "ForbiddenHeadersRule",
        "sourceUrl": origin,
        "sourceFormat": "rule",
        "header": r#"{"Host":"evil.com"}"#,
        "mediaHeader": r#"{"Connection":"close"}"#,
        "rulePlay": {
            "requestUrl": "{{playUrl}}",
            "streamUrl": "$.stream"
        }
    });
    let valid_rule = serde_json::json!({
        "sourceName": "ValidCustomHeaderRule",
        "sourceUrl": origin,
        "sourceFormat": "rule",
        "header": r#"{"Host":"evil.com"}"#,
        "mediaHeader": r#"{"Referer":"https://player.example.com/"}"#,
        "rulePlay": {
            "requestUrl": "{{playUrl}}",
            "streamUrl": "$.stream"
        }
    });

    let (st_imp, imp_res) = post_json(
        &app,
        "/api/v1/video/sources/import",
        &admin_tok,
        serde_json::json!({
            "raw_json": serde_json::Value::Array(vec![forbidden_applecms, forbidden_rule, valid_rule]).to_string()
        }),
    )
    .await;
    assert_eq!(st_imp, StatusCode::OK);
    let sources = imp_res["sources"].as_array().unwrap();
    let id_forbidden_applecms = sources[0]["id"].as_str().unwrap();
    let id_forbidden_rule = sources[1]["id"].as_str().unwrap();
    let id_valid_rule = sources[2]["id"].as_str().unwrap();

    let (st1, play1) = post_json(
        &app,
        "/api/v1/video/online/play",
        &member_tok,
        serde_json::json!({
            "source_id": id_forbidden_applecms,
            "play_url": format!("{origin}/media/video.mp4")
        }),
    )
    .await;
    assert_eq!(st1, StatusCode::OK);
    assert_eq!(play1["headers_applied"], false);

    let (st2, play2) = post_json(
        &app,
        "/api/v1/video/online/play",
        &member_tok,
        serde_json::json!({
            "source_id": id_forbidden_rule,
            "play_url": format!("{origin}/api/play")
        }),
    )
    .await;
    assert_eq!(st2, StatusCode::OK);
    assert_eq!(play2["headers_applied"], false);

    let (st3, play3) = post_json(
        &app,
        "/api/v1/video/online/play",
        &member_tok,
        serde_json::json!({
            "source_id": id_valid_rule,
            "play_url": format!("{origin}/api/play")
        }),
    )
    .await;
    assert_eq!(st3, StatusCode::OK);
    assert_eq!(play3["headers_applied"], true);

    srv.abort();
}

#[tokio::test]
async fn video_proxies_strip_static_range_from_source_media_headers() {
    let (_tmp, app, admin_tok, member_tok) = setup_test_app(true).await;

    let seen_hls_ranges: Arc<Mutex<Vec<Option<String>>>> = Arc::new(Mutex::new(Vec::new()));
    let seen_hls_referers: Arc<Mutex<Vec<Option<String>>>> = Arc::new(Mutex::new(Vec::new()));
    let seen_mp4_ranges: Arc<Mutex<Vec<Option<String>>>> = Arc::new(Mutex::new(Vec::new()));
    let seen_mp4_referers: Arc<Mutex<Vec<Option<String>>>> = Arc::new(Mutex::new(Vec::new()));

    let hls_ranges_c = seen_hls_ranges.clone();
    let hls_refs_c = seen_hls_referers.clone();
    let mp4_ranges_c = seen_mp4_ranges.clone();
    let mp4_refs_c = seen_mp4_referers.clone();

    let playlist_body = "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:10\n#EXTINF:10.0,\nseg0.ts\n#EXTINF:10.0,\nseg1.ts\n#EXT-X-ENDLIST\n";
    let mp4_bytes = fake_mp4_payload();

    let origin_app = Router::new()
        .route(
            "/hls/index.m3u8",
            get(move |headers: HeaderMap| {
                let ranges = hls_ranges_c.clone();
                let refs = hls_refs_c.clone();
                async move {
                    let rng = headers
                        .get(header::RANGE)
                        .and_then(|v| v.to_str().ok())
                        .map(ToString::to_string);
                    let ref_val = headers
                        .get(header::REFERER)
                        .and_then(|v| v.to_str().ok())
                        .map(ToString::to_string);
                    ranges.lock().unwrap().push(rng.clone());
                    refs.lock().unwrap().push(ref_val);
                    if rng.is_some() {
                        return Response::builder()
                            .status(StatusCode::PARTIAL_CONTENT)
                            .header("content-type", "application/vnd.apple.mpegurl")
                            .header(
                                "content-range",
                                format!("bytes 0-10/{}", playlist_body.len()),
                            )
                            .body(Body::from(&playlist_body[..11]))
                            .unwrap();
                    }
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/vnd.apple.mpegurl")
                        .body(Body::from(playlist_body))
                        .unwrap()
                }
            }),
        )
        .route(
            "/media/video.mp4",
            get(move |headers: HeaderMap| {
                let ranges = mp4_ranges_c.clone();
                let refs = mp4_refs_c.clone();
                let full = mp4_bytes.clone();
                async move {
                    let rng = headers
                        .get(header::RANGE)
                        .and_then(|v| v.to_str().ok())
                        .map(ToString::to_string);
                    let ref_val = headers
                        .get(header::REFERER)
                        .and_then(|v| v.to_str().ok())
                        .map(ToString::to_string);
                    ranges.lock().unwrap().push(rng.clone());
                    refs.lock().unwrap().push(ref_val);
                    if rng.as_deref() == Some("bytes=0-15") {
                        return Response::builder()
                            .status(StatusCode::PARTIAL_CONTENT)
                            .header("content-type", "video/mp4")
                            .header("content-range", format!("bytes 0-15/{}", full.len()))
                            .body(Body::from(full[..16].to_vec()))
                            .unwrap();
                    }
                    if rng.is_some() {
                        return Response::builder()
                            .status(StatusCode::PARTIAL_CONTENT)
                            .header("content-type", "video/mp4")
                            .header("content-range", format!("bytes 0-10/{}", full.len()))
                            .body(Body::from(full[..11].to_vec()))
                            .unwrap();
                    }
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "video/mp4")
                        .body(Body::from(full))
                        .unwrap()
                }
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let origin = format!("http://{addr}");
    let srv = tokio::spawn(async move {
        axum::serve(listener, origin_app).await.unwrap();
    });

    let src_json = serde_json::json!({
        "sourceName": "StaticRangeSource",
        "sourceUrl": origin,
        "sourceFormat": "rule",
        "header": r#"{"range":"bytes=0-5"}"#,
        "mediaHeader": {
            "Range": "bytes=0-10",
            "Referer": "https://ok.example/"
        },
        "rulePlay": {
            "requestUrl": "{{playUrl}}",
            "streamUrl": "$.stream"
        }
    });
    let (st_imp, imp_res) = post_json(
        &app,
        "/api/v1/video/sources/import",
        &admin_tok,
        serde_json::json!({ "raw_json": src_json.to_string() }),
    )
    .await;
    assert_eq!(st_imp, StatusCode::OK);
    let sid = imp_res["sources"][0]["id"].as_str().unwrap();

    let hls_url = urlencoding_param(&format!("{origin}/hls/index.m3u8"));
    let (st_hls, _, body_hls) = get_bytes_with_auth(
        &app,
        &format!("/api/v1/video/online/hls-proxy?url={hls_url}&source_id={sid}"),
        Some(&member_tok),
        None,
    )
    .await;
    assert_eq!(st_hls, StatusCode::OK);
    let hls_text = String::from_utf8(body_hls).unwrap();
    assert!(hls_text.contains("seg0.ts"), "missing seg0.ts: {hls_text}");
    assert!(hls_text.contains("seg1.ts"), "missing seg1.ts: {hls_text}");
    assert_eq!(*seen_hls_ranges.lock().unwrap(), vec![None]);
    assert_eq!(
        *seen_hls_referers.lock().unwrap(),
        vec![Some("https://ok.example/".to_string())]
    );

    let mp4_url = urlencoding_param(&format!("{origin}/media/video.mp4"));
    let (st_mp4_full, _, body_mp4_full) = get_bytes_with_auth(
        &app,
        &format!("/api/v1/video/online/stream-proxy?url={mp4_url}&source_id={sid}"),
        Some(&member_tok),
        None,
    )
    .await;
    assert_eq!(st_mp4_full, StatusCode::OK);
    assert_eq!(body_mp4_full.len(), fake_mp4_payload().len());

    let (st_mp4_rng, hdrs_rng, body_mp4_rng) = get_bytes_with_auth(
        &app,
        &format!("/api/v1/video/online/stream-proxy?url={mp4_url}&source_id={sid}"),
        Some(&member_tok),
        Some("bytes=0-15"),
    )
    .await;
    assert_eq!(st_mp4_rng, StatusCode::PARTIAL_CONTENT);
    assert_eq!(body_mp4_rng.len(), 16);
    assert_eq!(
        hdrs_rng
            .get(header::CONTENT_RANGE)
            .and_then(|v| v.to_str().ok()),
        Some("bytes 0-15/4096")
    );
    assert_eq!(
        *seen_mp4_ranges.lock().unwrap(),
        vec![None, Some("bytes=0-15".to_string())]
    );
    assert_eq!(
        *seen_mp4_referers.lock().unwrap(),
        vec![
            Some("https://ok.example/".to_string()),
            Some("https://ok.example/".to_string()),
        ]
    );

    srv.abort();
}

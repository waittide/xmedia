use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::{Query, Request as AxumRequest};
use axum::http::{HeaderMap, Request, StatusCode, header};
use axum::response::Response;
use axum::routing::get;
use bytes::Bytes;
use serde::Deserialize;
use tower::ServiceExt;
use uuid::Uuid;
use xmedia_source::crud::MASKED_SECRET_PLACEHOLDER;
use xmedia_source::http::HttpClientConfig;
use xmedia_source::model::SourceRuntimeState;
use xmedia_source::proxy::IMAGE_PROXY_CSP;

use super::{PNG_1X1, send_json, setup_online_test_harness};
use crate::domain::catalog::StructuredLyrics;
use crate::domain::online::{
    MusicSearchRule, MusicSourceSpec, OnlineMusicExploreCategory, url_encode_component,
};
use crate::infra::source::MusicSourceEngine;

#[tokio::test]
async fn music_online_crud_masking_rbac_and_proxy_streaming_e2e() {
    let captured_headers: Arc<Mutex<HashMap<String, String>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let cap_clone = captured_headers.clone();

    let audio_payload: Vec<u8> = (0u8..120).collect();
    let audio_for_range = audio_payload.clone();
    let audio_for_chunked = audio_payload;

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

    let (st, _, _) = send_json(
        &h.app,
        "POST",
        "/sources/import",
        &h.member_token,
        Some(serde_json::json!({ "raw_json": source_one.to_string() })),
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);

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
    let lazy_cover_url = search_dto["results"][0]["proxy_cover_url"]
        .as_str()
        .expect("search item with cover_id + ruleCover should expose lazy proxy_cover_url")
        .to_string();
    assert!(lazy_cover_url.starts_with("/api/v1/music/online/cover-proxy?source_id="));
    assert!(lazy_cover_url.contains("cover_id=pic-1"));
    let rel_lazy_cover = lazy_cover_url.strip_prefix("/api/v1/music").unwrap();
    let (st, hdrs, bytes) = send_json(&h.app, "GET", rel_lazy_cover, &h.member_token, None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(bytes, PNG_1X1);
    assert_eq!(hdrs.get(header::CONTENT_TYPE).unwrap(), "image/png");

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

    let download_uri = format!("{rel_audio}&download=1&filename=Artist%20-%20Song");
    let (st, dl_hdrs, dl_bytes) =
        send_json(&h.app, "GET", &download_uri, &h.member_token, None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(dl_bytes.len(), 120);
    let disp = dl_hdrs
        .get(header::CONTENT_DISPOSITION)
        .expect("download=1 should set Content-Disposition")
        .to_str()
        .unwrap();
    assert!(disp.starts_with("attachment;"));
    assert!(disp.contains("Artist%20-%20Song.mp3"));

    let (st, hdrs, full_bytes) = send_json(&h.app, "GET", rel_audio, &h.member_token, None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(hdrs.get(header::CONTENT_LENGTH).unwrap(), "120");
    assert_eq!(full_bytes, (0u8..120).collect::<Vec<u8>>());

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
    let parsed_title_cats = OnlineMusicExploreCategory::parse_list(&title_url_raw, &base);
    assert_eq!(parsed_title_cats.len(), 3);
    assert!(parsed_title_cats[0].url.starts_with("@js:"));
    assert!(parsed_title_cats[1].url.starts_with("<JS>"));
    assert!(
        parsed_title_cats[2]
            .url
            .starts_with("/cat-js?tag=prefix_tag")
    );

    let bare_cats = OnlineMusicExploreCategory::parse_list(
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
    let json_arr_tagjs_first_clone = json_arr_tagjs_first_raw;
    tokio::task::spawn_blocking(move || {
        let search_rule = MusicSearchRule {
            track_list: Some("$".into()),
            track_id: Some("$.id".into()),
            title: Some("$.name".into()),
            artist: Some("$.artist".into()),
            ..Default::default()
        };

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

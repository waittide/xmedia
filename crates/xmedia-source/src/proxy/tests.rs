use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::routing::get;
use bytes::Bytes;
use xmedia_core::domain::error::DomainError;
use xmedia_core::infra::stream::MpscByteStream;

use super::*;
use crate::http::{HttpClientConfig, MAX_REDIRECT_HOPS, ssrf_redirect_policy};

const PNG_BYTES: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0];

#[tokio::test]
async fn stream_byte_limiter_allows_within_cap_and_aborts_immediately_on_overflow() {
    use std::future::poll_fn;
    use std::pin::Pin;

    use futures_core::Stream;

    assert!(validate_proxy_content_length(Some(100), 100, "音频流").is_ok());
    assert!(validate_proxy_content_length(Some(101), 100, "音频流").is_err());

    // 1. Exact boundary passes cleanly
    let (tx_ok, rx_ok) = tokio::sync::mpsc::channel(4);
    tx_ok.send(Ok(Bytes::from_static(b"hello"))).await.unwrap();
    tx_ok.send(Ok(Bytes::from_static(b"world"))).await.unwrap();
    drop(tx_ok);

    let mut ok_limiter = StreamByteLimiter::new(MpscByteStream::new(rx_ok), 10);
    let mut collected = Vec::new();
    while let Some(item) = poll_fn(|cx| Pin::new(&mut ok_limiter).poll_next(cx)).await {
        collected.extend_from_slice(&item.unwrap());
    }
    assert_eq!(collected, b"helloworld");
    assert_eq!(ok_limiter.emitted_bytes(), 10);

    // 2. Overflow emits Err(Validation), drops upstream receiver immediately, and terminates with None
    let (tx_over, rx_over) = tokio::sync::mpsc::channel(4);
    tx_over
        .send(Ok(Bytes::from_static(b"123456")))
        .await
        .unwrap();
    tx_over
        .send(Ok(Bytes::from_static(b"78901")))
        .await
        .unwrap();

    let mut over_limiter = limit_byte_stream(MpscByteStream::new(rx_over), 10);
    let first = poll_fn(|cx| over_limiter.as_mut().poll_next(cx))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&first[..], b"123456");

    let second = poll_fn(|cx| over_limiter.as_mut().poll_next(cx))
        .await
        .unwrap();
    assert!(matches!(second, Err(DomainError::Validation(_))));
    assert!(
        tx_over.is_closed(),
        "upstream channel receiver must be dropped immediately upon byte cap breach"
    );
    assert!(
        poll_fn(|cx| over_limiter.as_mut().poll_next(cx))
            .await
            .is_none()
    );
}

#[test]
fn validate_safe_raster_image_accepts_all_7_raster_formats_and_rejects_svg_html() {
    let jpeg = [0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
    let mut webp = b"RIFF\x00\x00\x00\x00WEBP".to_vec();
    webp.extend_from_slice(&[0u8; 4]);
    let gif = b"GIF89a\x01\x00\x01\x00";
    let mut bmp = b"BM".to_vec();
    bmp.extend_from_slice(&[0u8; 12]);
    let ico = [0x00, 0x00, 0x01, 0x00, 0x01, 0x00];
    let avif = b"\x00\x00\x00\x18ftypavif\x00\x00\x00\x00";

    assert_eq!(
        validate_safe_raster_image(Some("image/png"), PNG_BYTES).unwrap(),
        "image/png"
    );
    assert_eq!(
        validate_safe_raster_image(Some("application/octet-stream"), &jpeg).unwrap(),
        "image/jpeg"
    );
    assert_eq!(
        validate_safe_raster_image(None, &webp).unwrap(),
        "image/webp"
    );
    assert_eq!(
        validate_safe_raster_image(Some("image/gif"), gif).unwrap(),
        "image/gif"
    );
    assert_eq!(
        validate_safe_raster_image(Some("image/bmp"), &bmp).unwrap(),
        "image/bmp"
    );
    assert_eq!(
        validate_safe_raster_image(Some("image/x-icon"), &ico).unwrap(),
        "image/x-icon"
    );
    assert_eq!(
        validate_safe_raster_image(Some("image/avif"), avif).unwrap(),
        "image/avif"
    );

    let svg = b"<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>";
    let html = b"<html><body>xss</body></html>";
    assert!(validate_safe_raster_image(Some("image/svg+xml"), svg).is_err());
    assert!(validate_safe_raster_image(Some("image/jpeg"), svg).is_err());
    assert!(validate_safe_raster_image(Some("image/png"), html).is_err());
    assert!(validate_safe_raster_image(Some("text/html"), PNG_BYTES).is_err());
    assert!(validate_safe_raster_image(Some("image/svg+xml"), PNG_BYTES).is_err());
    assert!(validate_safe_raster_image(None, b"").is_err());
}

#[tokio::test]
async fn fetch_validated_image_bytes_enforces_ssrf_size_cap_and_header_sanitization() {
    let captured: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));
    let captured_c = captured.clone();

    let app = Router::new()
        .route(
            "/ok.png",
            get(move |headers: HeaderMap| {
                let cap = captured_c.clone();
                async move {
                    let mut g = cap.lock().unwrap();
                    for (k, v) in &headers {
                        if let Ok(s) = v.to_str() {
                            g.insert(k.as_str().to_ascii_lowercase(), s.to_string());
                        }
                    }
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "image/png")
                        .body(Body::from(PNG_BYTES.to_vec()))
                        .unwrap()
                }
            }),
        )
        .route(
            "/big-header.png",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "image/png")
                    .header("content-length", "200")
                    .body(Body::from(vec![0u8; 200]))
                    .unwrap()
            }),
        )
        .route(
            "/big-stream.png",
            get(|| async {
                let mut payload = PNG_BYTES.to_vec();
                payload.extend_from_slice(&vec![1u8; 256]);
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "image/png")
                    .body(Body::from(payload))
                    .unwrap()
            }),
        )
        .route(
            "/evil.svg",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "image/svg+xml")
                    .body(Body::from("<svg></svg>"))
                    .unwrap()
            }),
        )
        .route(
            "/missing.png",
            get(|| async { (StatusCode::NOT_FOUND, "not found") }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let ssrf_cfg = HttpClientConfig {
        timeout_secs: 5,
        allow_private_ips: false,
        proxy_url: None,
    };
    assert!(
        fetch_image(&format!("http://{addr}/ok.png"), None, 1024, &ssrf_cfg)
            .await
            .is_err()
    );

    let ok_cfg = HttpClientConfig {
        timeout_secs: 5,
        allow_private_ips: true,
        proxy_url: None,
    };
    let custom_headers = HashMap::from([
        (
            "Referer".to_string(),
            "https://comic.example.com/".to_string(),
        ),
        ("Host".to_string(), "spoofed.example.com".to_string()),
        ("Connection".to_string(), "close".to_string()),
    ]);
    let (bytes, mime) = fetch_image(
        &format!("http://{addr}/ok.png"),
        Some(&custom_headers),
        1024,
        &ok_cfg,
    )
    .await
    .unwrap();
    assert_eq!(mime, "image/png");
    assert_eq!(bytes, PNG_BYTES);
    {
        let cap = captured.lock().unwrap();
        assert_eq!(
            cap.get("referer").map(String::as_str),
            Some("https://comic.example.com/")
        );
        assert_ne!(
            cap.get("host").map(String::as_str),
            Some("spoofed.example.com")
        );
    }

    for path in [
        "/big-header.png",
        "/big-stream.png",
        "/missing.png",
        "/evil.svg",
    ] {
        let cap = if path.starts_with("/big-") { 64 } else { 1024 };
        assert!(
            fetch_image(&format!("http://{addr}{path}"), None, cap, &ok_cfg)
                .await
                .is_err()
        );
    }

    server.abort();
}

#[test]
fn html_or_svg_sniffing_catches_xml_prologs_comments_and_utf16_bom() {
    assert!(looks_like_html_or_svg_bytes(
        b"<?xml version=\"1.0\" encoding=\"UTF-8\"?><svg xmlns=\"http://www.w3.org/2000/svg\"/>"
    ));
    assert!(looks_like_html_or_svg_bytes(
        b"\xEF\xBB\xBF \n<?xml version=\"1.0\"?>\n<!-- c --><!DOCTYPE html><html></html>"
    ));
    assert!(!has_enough_media_sniff_bytes(
        b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>",
        16
    ));
    assert!(has_enough_media_sniff_bytes(
        b"<?xml version=\"1.0\" encoding=\"UTF-8\"?><svg xmlns=\"http://www.w3.org/2000/svg\">",
        16
    ));

    let mut utf16_le = vec![0xFF, 0xFE];
    for unit in "<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>".encode_utf16() {
        utf16_le.extend_from_slice(&unit.to_le_bytes());
    }
    assert!(looks_like_html_or_svg_bytes(&utf16_le));

    let mut huge_comment = b"<!-- ".to_vec();
    huge_comment.extend_from_slice(&vec![b'X'; 4600]);
    assert!(has_unclosed_leading_xml_or_comment(&huge_comment));
    assert!(looks_like_html_or_svg_bytes(&huge_comment));

    assert!(!looks_like_html_or_svg_bytes(
        b"\xFF\xFB\x90\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00"
    ));
    assert!(!looks_like_html_or_svg_bytes(
        b"RIFF\x24\x00\x00\x00WAVEfmt "
    ));
}

#[tokio::test]
async fn fetch_validated_image_bytes_retries_transient_503_and_never_retries_ssrf_or_403() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let flaky_hits = Arc::new(AtomicUsize::new(0));
    let fh = flaky_hits.clone();
    let hits_403 = Arc::new(AtomicUsize::new(0));
    let h403 = hits_403.clone();
    let redir_hits = Arc::new(AtomicUsize::new(0));
    let rh = redir_hits.clone();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let target_url = format!("http://{addr}/flaky.png");

    let app = Router::new()
        .route(
            "/flaky.png",
            get(move || {
                let h = fh.clone();
                async move {
                    let n = h.fetch_add(1, Ordering::SeqCst) + 1;
                    if n == 1 {
                        Response::builder()
                            .status(StatusCode::SERVICE_UNAVAILABLE)
                            .body(Body::from("busy"))
                            .unwrap()
                    } else {
                        Response::builder()
                            .status(StatusCode::OK)
                            .header("content-type", "image/png")
                            .body(Body::from(PNG_BYTES.to_vec()))
                            .unwrap()
                    }
                }
            }),
        )
        .route(
            "/forbidden.png",
            get(move || {
                let c = h403.clone();
                async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    (StatusCode::FORBIDDEN, "forbidden")
                }
            }),
        )
        .route(
            "/redir",
            get(move || {
                let c = rh.clone();
                let loc = target_url.clone();
                async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    Response::builder()
                        .status(StatusCode::FOUND)
                        .header("location", loc)
                        .body(Body::empty())
                        .unwrap()
                }
            }),
        );

    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let ok_cfg = HttpClientConfig {
        timeout_secs: 5,
        allow_private_ips: true,
        proxy_url: None,
    };
    let (bytes, mime) = fetch_image(&format!("http://{addr}/flaky.png"), None, 1024, &ok_cfg)
        .await
        .unwrap();
    assert_eq!(mime, "image/png");
    assert_eq!(bytes, PNG_BYTES);
    assert_eq!(flaky_hits.load(Ordering::SeqCst), 2);

    assert!(
        fetch_image(&format!("http://{addr}/forbidden.png"), None, 1024, &ok_cfg)
            .await
            .is_err()
    );
    assert_eq!(hits_403.load(Ordering::SeqCst), 1);

    let redir_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .redirect(ssrf_redirect_policy(false, MAX_REDIRECT_HOPS))
        .build()
        .unwrap();
    assert!(
        image::fetch_client_image(
            &redir_client,
            None,
            &format!("http://{addr}/redir"),
            None,
            1024,
            true
        )
        .await
        .is_err()
    );
    assert_eq!(redir_hits.load(Ordering::SeqCst), 1);

    server.abort();
}

#[tokio::test]
async fn fetch_validated_image_bytes_for_source_strips_cross_origin_auth_headers() {
    let captured: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));
    let cap_c = captured.clone();

    let app = Router::new().route(
        "/cover.png",
        get(move |headers: HeaderMap| {
            let cap = cap_c.clone();
            async move {
                let mut guard = cap.lock().unwrap();
                guard.clear();
                for (k, v) in &headers {
                    if let Ok(s) = v.to_str() {
                        guard.insert(k.as_str().to_ascii_lowercase(), s.to_string());
                    }
                }
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "image/png")
                    .body(Body::from(PNG_BYTES.to_vec()))
                    .unwrap()
            }
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let cfg = HttpClientConfig {
        timeout_secs: 5,
        allow_private_ips: true,
        proxy_url: None,
    };
    let custom_headers = HashMap::from([
        ("Authorization".to_string(), "Bearer secret-jwt".to_string()),
        ("Cookie".to_string(), "session=secret-cookie".to_string()),
        ("X-Api-Key".to_string(), "xmk_secret_key".to_string()),
        (
            "Referer".to_string(),
            "https://comic.example.com/".to_string(),
        ),
    ]);

    fetch_source_image(
        Some("https://comic.example.com"),
        &format!("http://{addr}/cover.png"),
        Some(&custom_headers),
        1024,
        &cfg,
    )
    .await
    .unwrap();
    {
        let guard = captured.lock().unwrap();
        assert!(!guard.contains_key("authorization"));
        assert!(!guard.contains_key("cookie"));
        assert!(!guard.contains_key("x-api-key"));
        assert_eq!(
            guard.get("referer").map(String::as_str),
            Some("https://comic.example.com/")
        );
    }

    server.abort();
}

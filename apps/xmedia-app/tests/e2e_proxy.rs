use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Bytes;
use axum::http::header::{
    ACCEPT_RANGES, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, ETAG, RANGE,
};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use futures_util::StreamExt;
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use xmedia::{XMediaApp, XMediaConfig};
use xmedia_core::application::ports::{RealtimeEventPort, SystemEvent};
use xmedia_core::domain::library::MediaKind;
use xmedia_core::infra::events::BroadcastEventHub;

use xmedia_app_lib::config::AppServerStore;
use xmedia_app_lib::proxy::{
    APP_HEADER_NAME, APP_HEADER_VALUE, AppHealthResponse, MutatedServerResponse,
    ServersListResponse, TestServerResponse, start_gateway,
};

struct TestXMediaServer {
    url: String,
    events: Arc<BroadcastEventHub>,
    _tmp: TempDir,
    shutdown_tx: Option<oneshot::Sender<()>>,
}

impl Drop for TestXMediaServer {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

async fn range_media_handler(headers: HeaderMap) -> Response {
    let payload: Vec<u8> = (0u32..1024).map(|i| (i % 251) as u8).collect();
    let total = payload.len();

    if let Some(raw_range) = headers.get(RANGE).and_then(|v| v.to_str().ok())
        && let Some(spec) = raw_range.strip_prefix("bytes=")
        && let Some((start_s, end_s)) = spec.split_once('-')
        && let (Ok(start), Ok(end)) = (start_s.parse::<usize>(), end_s.parse::<usize>())
        && start <= end
        && end < total
    {
        let slice = payload[start..=end].to_vec();
        let mut res = (StatusCode::PARTIAL_CONTENT, slice).into_response();
        let h = res.headers_mut();
        h.insert(CONTENT_TYPE, HeaderValue::from_static("video/mp4"));
        h.insert(ACCEPT_RANGES, HeaderValue::from_static("bytes"));
        h.insert(
            CONTENT_RANGE,
            HeaderValue::from_str(&format!("bytes {start}-{end}/{total}")).unwrap(),
        );
        h.insert(
            CONTENT_LENGTH,
            HeaderValue::from_str(&(end - start + 1).to_string()).unwrap(),
        );
        h.insert(ETAG, HeaderValue::from_static("\"test-media-etag-1\""));
        h.insert(
            CONTENT_DISPOSITION,
            HeaderValue::from_static("inline; filename=\"clip.mp4\""),
        );
        return res;
    }

    let mut res = (StatusCode::OK, payload).into_response();
    let h = res.headers_mut();
    h.insert(CONTENT_TYPE, HeaderValue::from_static("video/mp4"));
    h.insert(ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    res
}

async fn echo_upload_handler(body: Bytes) -> Response {
    let len = body.len();
    let mut res = (StatusCode::OK, body).into_response();
    res.headers_mut().insert(
        "x-echo-bytes",
        HeaderValue::from_str(&len.to_string()).unwrap(),
    );
    res
}

async fn spawn_real_xmedia_server(secret: &str) -> TestXMediaServer {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("xmedia.db");
    let app = XMediaApp::connect_and_init(XMediaConfig {
        data_dir: tmp.path().to_path_buf(),
        database_url: format!("sqlite:{}", db_path.display()),
        secret: secret.to_string(),
        jwt_ttl_secs: 3600,
        ffmpeg_bin: "__missing_ffmpeg__".to_string(),
        external_web_dir: None,
    })
    .await
    .expect("real XMediaApp should initialize cleanly");

    let events = app.events.clone();
    let extra_routes = Router::new()
        .route("/api/v1/test-range", get(range_media_handler))
        .route("/api/v1/test-echo", post(echo_upload_handler));
    let router = extra_routes.merge(app.router);

    let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    tokio::spawn(async move {
        let _ = axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await;
    });

    TestXMediaServer {
        url: format!("http://127.0.0.1:{}", addr.port()),
        events,
        _tmp: tmp,
        shutdown_tx: Some(shutdown_tx),
    }
}

#[tokio::test]
async fn e2e_tauri_loopback_gateway_full_lifecycle() {
    let server_a = spawn_real_xmedia_server("secret-server-a-test-key").await;
    let server_b = spawn_real_xmedia_server("secret-server-b-test-key").await;

    let app_tmp = TempDir::new().unwrap();
    let servers_file = app_tmp.path().join("servers.json");
    let store = AppServerStore::open_shared(servers_file.clone()).unwrap();

    let gateway = start_gateway(
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
        store,
        None,
    )
    .await
    .expect("loopback gateway should start");

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();

    // 1. /_app/health initial state + X-XMedia-App header
    let health_resp = client
        .get(format!("{}/_app/health", gateway.url))
        .send()
        .await
        .unwrap();
    assert_eq!(health_resp.status(), StatusCode::OK);
    assert_eq!(
        health_resp
            .headers()
            .get(APP_HEADER_NAME)
            .and_then(|v| v.to_str().ok()),
        Some(APP_HEADER_VALUE)
    );
    let health: AppHealthResponse = health_resp.json().await.unwrap();
    assert_eq!(health.mode, "tauri");
    assert!(health.active_server.is_none());
    assert!(!health.has_servers);

    // 2. Unconfigured server -> /api/v1/* returns 503 no_server_configured
    let unconfigured_resp = client
        .get(format!("{}/api/v1/system/capabilities", gateway.url))
        .send()
        .await
        .unwrap();
    assert_eq!(unconfigured_resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    let unconfigured_json: Value = unconfigured_resp.json().await.unwrap();
    assert_eq!(
        unconfigured_json["error"]["code"],
        json!("no_server_configured")
    );

    // 3. /_app/servers/test connectivity probe: success + failure scenarios
    let probe_ok: TestServerResponse = client
        .post(format!("{}/_app/servers/test", gateway.url))
        .json(&json!({
            "url": format!("{}/", server_a.url),
            "allow_insecure_tls": false
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(probe_ok.ok, "probe should succeed: {:?}", probe_ok);
    assert_eq!(probe_ok.normalized_url, server_a.url);
    assert_eq!(probe_ok.needs_setup, Some(true));
    assert!(probe_ok.version.as_deref().is_some_and(|v| !v.is_empty()));

    let probe_invalid: TestServerResponse = client
        .post(format!("{}/_app/servers/test", gateway.url))
        .json(&json!({
            "url": "ftp://invalid.local",
            "allow_insecure_tls": false
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(!probe_invalid.ok);
    assert!(probe_invalid.error.is_some());

    // Bind and immediately drop a local port to get a guaranteed closed port.
    let closed_port = {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        l.local_addr().unwrap().port()
    };
    let probe_unreachable: TestServerResponse = client
        .post(format!("{}/_app/servers/test", gateway.url))
        .json(&json!({
            "url": format!("http://127.0.0.1:{closed_port}"),
            "allow_insecure_tls": false
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(!probe_unreachable.ok);
    assert!(probe_unreachable.error.is_some());

    // 4. Add Server A & activate + verify disk persistence
    let created_a: MutatedServerResponse = client
        .post(format!("{}/_app/servers", gateway.url))
        .json(&json!({
            "name": "Home A",
            "url": format!("{}/", server_a.url),
            "allow_insecure_tls": false,
            "activate": true
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        created_a.active_server_id,
        Some(created_a.server.id.clone())
    );
    assert_eq!(created_a.server.url, server_a.url);
    assert_eq!(created_a.servers.len(), 1);

    let persisted = AppServerStore::load(servers_file.clone()).unwrap();
    assert_eq!(persisted.active_server().unwrap().id, created_a.server.id);

    // 5. Real XMedia auth lifecycle through loopback proxy: setup -> login -> /auth/me (Bearer & Cookie)
    let setup_resp = client
        .post(format!("{}/api/v1/auth/setup", gateway.url))
        .json(&json!({
            "username": "admin",
            "display_name": "Admin User",
            "password": "StrongPassword123!"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(setup_resp.status(), StatusCode::CREATED);
    let set_cookie_header = setup_resp
        .headers()
        .get(reqwest::header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    assert!(
        set_cookie_header.contains("xmedia_token="),
        "setup response should forward Set-Cookie header, got: {set_cookie_header}"
    );
    let setup_json: Value = setup_resp.json().await.unwrap();
    let setup_token = setup_json["token"].as_str().unwrap().to_string();
    assert!(!setup_token.is_empty());

    let login_resp = client
        .post(format!("{}/api/v1/auth/login", gateway.url))
        .json(&json!({
            "username": "admin",
            "password": "StrongPassword123!"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(login_resp.status(), StatusCode::OK);
    let login_json: Value = login_resp.json().await.unwrap();
    let token = login_json["token"].as_str().unwrap().to_string();

    // Verify /api/v1/auth/me with Authorization Bearer
    let me_bearer: Value = client
        .get(format!("{}/api/v1/auth/me", gateway.url))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(me_bearer["username"], json!("admin"));

    // Verify /api/v1/auth/me with Cookie
    let me_cookie: Value = client
        .get(format!("{}/api/v1/auth/me", gateway.url))
        .header(reqwest::header::COOKIE, format!("xmedia_token={token}"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(me_cookie["username"], json!("admin"));

    // 6. Real-time SSE /api/v1/events/stream transparent passthrough
    let sse_client = reqwest::Client::new();
    let sse_resp = sse_client
        .get(format!("{}/api/v1/events/stream", gateway.url))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(sse_resp.status(), StatusCode::OK);
    let sse_ct = sse_resp
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        sse_ct.contains("text/event-stream"),
        "expected text/event-stream, got: {sse_ct}"
    );

    let mut sse_stream = sse_resp.bytes_stream();
    // Publish a live event on Server A's BroadcastEventHub and verify immediate delivery.
    tokio::time::sleep(Duration::from_millis(50)).await;
    server_a.events.publish(SystemEvent::MediaChanged {
        media_kind: MediaKind::Book,
    });

    let mut received_sse = String::new();
    tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(chunk) = sse_stream.next().await {
            let bytes = chunk.expect("SSE chunk should not error");
            received_sse.push_str(&String::from_utf8_lossy(&bytes));
            if received_sse.contains("book") {
                break;
            }
        }
    })
    .await
    .expect("SSE event must stream through loopback proxy without buffering");
    assert!(
        received_sse.contains("data:") && received_sse.contains("book"),
        "unexpected SSE payload: {received_sse}"
    );
    drop(sse_stream);

    // 7. Range 206 Partial Content streaming passthrough + header preservation
    let range_resp = client
        .get(format!("{}/api/v1/test-range", gateway.url))
        .header(RANGE, "bytes=100-199")
        .send()
        .await
        .unwrap();
    assert_eq!(range_resp.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        range_resp
            .headers()
            .get(CONTENT_RANGE)
            .and_then(|v| v.to_str().ok()),
        Some("bytes 100-199/1024")
    );
    assert_eq!(
        range_resp
            .headers()
            .get(ACCEPT_RANGES)
            .and_then(|v| v.to_str().ok()),
        Some("bytes")
    );
    assert_eq!(
        range_resp
            .headers()
            .get(CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok()),
        Some("100")
    );
    assert_eq!(
        range_resp.headers().get(ETAG).and_then(|v| v.to_str().ok()),
        Some("\"test-media-etag-1\"")
    );
    assert_eq!(
        range_resp
            .headers()
            .get(CONTENT_DISPOSITION)
            .and_then(|v| v.to_str().ok()),
        Some("inline; filename=\"clip.mp4\"")
    );
    let range_bytes = range_resp.bytes().await.unwrap();
    let expected_slice: Vec<u8> = (100u32..=199).map(|i| (i % 251) as u8).collect();
    assert_eq!(range_bytes.as_ref(), expected_slice.as_slice());

    // 8. Streaming upload body passthrough
    let upload_payload: Vec<u8> = (0..65536).map(|i| (i % 256) as u8).collect();
    let upload_resp = client
        .post(format!("{}/api/v1/test-echo", gateway.url))
        .body(upload_payload.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(upload_resp.status(), StatusCode::OK);
    assert_eq!(
        upload_resp
            .headers()
            .get("x-echo-bytes")
            .and_then(|v| v.to_str().ok()),
        Some("65536")
    );
    assert_eq!(
        upload_resp.bytes().await.unwrap().as_ref(),
        upload_payload.as_slice()
    );

    // 9. SPA frontend fallback returns index.html, while unknown /_app/* returns 404 JSON
    let spa_resp = client
        .get(format!("{}/connect", gateway.url))
        .send()
        .await
        .unwrap();
    assert_eq!(spa_resp.status(), StatusCode::OK);
    let spa_ct = spa_resp
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(spa_ct.contains("text/html"), "expected html, got: {spa_ct}");
    let spa_html = spa_resp.text().await.unwrap();
    assert!(
        spa_html.contains("<div id=\"app\">") || spa_html.contains("XMedia"),
        "expected SPA HTML document, got: {spa_html}"
    );

    let unknown_app_resp = client
        .get(format!("{}/_app/nonexistent", gateway.url))
        .send()
        .await
        .unwrap();
    assert_eq!(unknown_app_resp.status(), StatusCode::NOT_FOUND);

    // 10. Multi-server switch: Server A (setup complete -> needs_setup=false) -> Server B (fresh -> needs_setup=true)
    let status_a: Value = client
        .get(format!("{}/api/v1/auth/setup-status", gateway.url))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(status_a["needs_setup"], json!(false));

    let created_b: MutatedServerResponse = client
        .post(format!("{}/_app/servers", gateway.url))
        .json(&json!({
            "name": "Server B",
            "url": server_b.url,
            "allow_insecure_tls": false,
            "activate": false
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        created_b.active_server_id,
        Some(created_a.server.id.clone())
    );
    assert_eq!(created_b.servers.len(), 2);

    // Still routes to Server A before activating Server B
    let still_a: Value = client
        .get(format!("{}/api/v1/auth/setup-status", gateway.url))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(still_a["needs_setup"], json!(false));

    // Switch active server to Server B
    let activate_resp = client
        .put(format!(
            "{}/_app/servers/{}/activate",
            gateway.url, created_b.server.id
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(activate_resp.status(), StatusCode::OK);
    let clear_cookie = activate_resp
        .headers()
        .get(reqwest::header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    assert!(
        clear_cookie.contains("Max-Age=0"),
        "switching server should clear old HttpOnly session cookie, got: {clear_cookie}"
    );
    let activated_b: MutatedServerResponse = activate_resp.json().await.unwrap();
    assert_eq!(
        activated_b.active_server_id,
        Some(created_b.server.id.clone())
    );

    // Now routes to Server B (needs_setup == true)
    let status_b: Value = client
        .get(format!("{}/api/v1/auth/setup-status", gateway.url))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(status_b["needs_setup"], json!(true));

    // Verify GET /_app/servers lists both
    let list: ServersListResponse = client
        .get(format!("{}/_app/servers", gateway.url))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(list.servers.len(), 2);
    assert_eq!(list.active_server_id, Some(created_b.server.id.clone()));

    // Delete active Server B -> active_server_id becomes None -> /api/v1/* returns 503
    let deleted: ServersListResponse = client
        .delete(format!(
            "{}/_app/servers/{}",
            gateway.url, created_b.server.id
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(deleted.servers.len(), 1);
    assert!(deleted.active_server_id.is_none());

    let after_delete_resp = client
        .get(format!("{}/api/v1/system/capabilities", gateway.url))
        .send()
        .await
        .unwrap();
    assert_eq!(after_delete_resp.status(), StatusCode::SERVICE_UNAVAILABLE);

    gateway.shutdown().await;
}

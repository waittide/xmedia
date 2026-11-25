use super::*;
use axum::body::to_bytes;
use uuid::Uuid;

async fn decode_error_response(err: DomainError) -> (StatusCode, serde_json::Value) {
    let resp = ApiError(err).into_response();
    let status = resp.status();
    let bytes = to_bytes(resp.into_body(), 1024 * 64).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    (status, json)
}

#[tokio::test]
async fn api_error_returns_concise_inner_message_without_redundant_prefix() {
    let (status, body) =
        decode_error_response(DomainError::Validation("用户名不能为空".into())).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "validation_error");
    assert_eq!(body["error"]["message"], "用户名不能为空");

    let (status, body) = decode_error_response(DomainError::NotFound("书籍不存在".into())).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "not_found");
    assert_eq!(body["error"]["message"], "书籍不存在");
}

#[tokio::test]
async fn api_error_sanitizes_internal_storage_and_persistence_details() {
    let (status, body) = decode_error_response(DomainError::Storage(
        "failed opening /var/lib/xmedia/secret.db: Permission denied".into(),
    ))
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["error"]["code"], "storage_error");
    let msg = body["error"]["message"].as_str().unwrap();
    assert!(!msg.contains("/var/lib"));
    assert!(!msg.contains("secret.db"));
    assert_eq!(msg, "Internal storage error");

    let (status, body) = decode_error_response(DomainError::Persistence(
        "sqlite error: near \"SELECT * FROM users\": syntax error".into(),
    ))
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["error"]["code"], "persistence_error");
    let msg = body["error"]["message"].as_str().unwrap();
    assert!(!msg.contains("sqlite"));
    assert!(!msg.contains("SELECT"));
    assert_eq!(msg, "Internal database error");
}

#[test]
fn temp_upload_file_cleans_up_on_drop() {
    let path = std::env::temp_dir().join(format!("xmedia_drop_test_{}.tmp", Uuid::now_v7()));
    std::fs::write(&path, b"hello").unwrap();
    assert!(path.exists());
    {
        let guard = TempUploadFile {
            path: path.clone(),
            size: 5,
        };
        assert_eq!(guard.size(), 5);
        assert!(!guard.is_empty());
    }
    assert!(!path.exists());
}

#[test]
fn prepare_and_configure_temp_dir_cleans_stale_tmp_only() {
    let dir = tempfile::TempDir::new().unwrap();
    let tmp_sub = dir.path().join("tmp");
    std::fs::create_dir_all(&tmp_sub).unwrap();
    let stale = tmp_sub.join("leftover.tmp");
    let keep = tmp_sub.join("keep.txt");
    std::fs::write(&stale, b"stale").unwrap();
    std::fs::write(&keep, b"keep").unwrap();

    let prepared = configure_temp_dir(tmp_sub.clone()).unwrap();
    assert_eq!(prepared, tmp_sub);
    assert!(!stale.exists(), "stale .tmp files should be removed");
    assert!(keep.exists(), "non-.tmp files should be preserved");
    assert_eq!(resolve_temp_dir(), tmp_sub);
}

#[cfg(feature = "server")]
#[test]
fn extract_client_ip_honors_connect_info_and_loopback_forwarded_for_only() {
    use std::net::{IpAddr, SocketAddr};

    use axum::extract::ConnectInfo;
    use axum::http::Request;

    use crate::http::auth::extract_client_ip_from_headers_and_peer;

    // 1. Direct non-loopback peer ignores spoofed X-Forwarded-For unless trust_proxy is enabled
    let mut req = Request::builder()
        .uri("/api/v1/auth/login")
        .header("x-forwarded-for", "1.2.3.4")
        .body(Body::empty())
        .unwrap();
    req.extensions_mut().insert(ConnectInfo(
        "203.0.113.9:54321".parse::<SocketAddr>().unwrap(),
    ));
    let (parts, _) = req.into_parts();
    assert_eq!(
        extract_client_ip(&parts),
        Some("203.0.113.9".parse::<IpAddr>().unwrap())
    );

    // 2. Loopback reverse proxy / local gateway trusts first valid X-Forwarded-For IP
    let mut loop_req = Request::builder()
        .uri("/api/v1/auth/login")
        .header("x-forwarded-for", "198.51.100.7, 127.0.0.1")
        .body(Body::empty())
        .unwrap();
    loop_req.extensions_mut().insert(ConnectInfo(
        "127.0.0.1:41000".parse::<SocketAddr>().unwrap(),
    ));
    let (loop_parts, _) = loop_req.into_parts();
    assert_eq!(
        extract_client_ip(&loop_parts),
        Some("198.51.100.7".parse::<IpAddr>().unwrap())
    );

    // 3. IPv4-mapped IPv6 peer canonicalizes to IPv4
    let mut headers = HeaderMap::new();
    headers.insert("x-real-ip", HeaderValue::from_static("198.51.100.99"));
    assert_eq!(
        extract_client_ip_from_headers_and_peer(
            &headers,
            Some("::ffff:203.0.113.9".parse().unwrap()),
            false,
        ),
        Some("203.0.113.9".parse().unwrap())
    );
    assert_eq!(
        extract_client_ip_from_headers_and_peer(&headers, Some("10.0.0.2".parse().unwrap()), true,),
        Some("198.51.100.99".parse().unwrap())
    );
}

#[cfg(feature = "server")]
#[tokio::test]
async fn csrf_check_guards_cookie_mutations_while_preserving_bearer_api_key_basic_and_get() {
    use std::collections::HashMap;
    use std::sync::{Arc, RwLock};

    use axum::http::Request;
    use axum::http::header::{AUTHORIZATION, COOKIE, HOST, ORIGIN};
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
    use toasty::Db;
    use toasty::schema::ModelSet;

    use crate::application::auth::AuthService;
    use crate::application::dto::SetupAdminInput;
    use crate::infra::persistence::repositories::ToastyUserRepository;
    use crate::infra::security::{JwtTokenService, SecurityManager};
    use crate::{ensure_core_schema, register_core_models};

    let dir = tempfile::TempDir::new().unwrap();
    let db_url = format!("sqlite:{}", dir.path().join("csrf.db").display());
    let mut models = ModelSet::new();
    register_core_models(&mut models);
    let mut db = Db::builder().models(models).connect(&db_url).await.unwrap();
    ensure_core_schema(&mut db, &db_url).await.unwrap();

    let auth = AuthService::new(
        Arc::new(ToastyUserRepository::new(db)),
        Arc::new(SecurityManager::new("csrf-test-secret")),
        Arc::new(JwtTokenService::new("csrf-test-secret", 3600)),
        Arc::new(RwLock::new(HashMap::new())),
    );
    let session = auth
        .setup_initial_admin(SetupAdminInput {
            username: "admin".into(),
            display_name: None,
            password: "password123".into(),
        })
        .await
        .unwrap();
    let tok = &session.token;
    let api_key = &session.user.api_key;

    for (method, origin, sfs) in [
        ("POST", Some("https://evil.example"), None),
        ("PUT", Some("http://localhost:3000"), None),
        ("PATCH", Some("null"), None),
        ("DELETE", None, Some("cross-site")),
        ("POST", Some("http://localhost:8090"), Some("same-site")),
    ] {
        let mut b = Request::builder()
            .method(method)
            .uri("/api/v1/auth/preferences")
            .header(HOST, "localhost:8090")
            .header(COOKIE, format!("{AUTH_COOKIE_NAME}={tok}"));
        if let Some(o) = origin {
            b = b.header(ORIGIN, o);
        }
        if let Some(s) = sfs {
            b = b.header("sec-fetch-site", s);
        }
        let (parts, _) = b.body(Body::empty()).unwrap().into_parts();
        let err = authenticate_web_parts(&parts, &auth).await.unwrap_err();
        assert_eq!(err.into_response().status(), StatusCode::FORBIDDEN);
    }

    let (sess_parts, _) = Request::builder()
        .method("POST")
        .uri("/api/v1/users")
        .header(HOST, "localhost:8090")
        .header(COOKIE, format!("xmedia_session={tok}"))
        .header("sec-fetch-site", "cross-site")
        .body(Body::empty())
        .unwrap()
        .into_parts();
    assert_eq!(
        authenticate_web_parts(&sess_parts, &auth)
            .await
            .unwrap_err()
            .into_response()
            .status(),
        StatusCode::FORBIDDEN
    );

    let (same_origin_parts, _) = Request::builder()
        .method("PUT")
        .uri("/api/v1/auth/preferences")
        .header(HOST, "localhost:8090")
        .header(ORIGIN, "http://localhost:8090")
        .header("sec-fetch-site", "same-origin")
        .header(COOKIE, format!("{AUTH_COOKIE_NAME}={tok}"))
        .body(Body::empty())
        .unwrap()
        .into_parts();
    assert_eq!(
        authenticate_web_parts(&same_origin_parts, &auth)
            .await
            .unwrap()
            .username,
        "admin"
    );

    let (get_parts, _) = Request::builder()
        .method("GET")
        .uri("/api/v1/auth/me")
        .header(HOST, "localhost:8090")
        .header(ORIGIN, "https://evil.example")
        .header("sec-fetch-site", "cross-site")
        .header(COOKIE, format!("{AUTH_COOKIE_NAME}={tok}"))
        .body(Body::empty())
        .unwrap()
        .into_parts();
    assert!(authenticate_web_parts(&get_parts, &auth).await.is_ok());

    let (bearer_parts, _) = Request::builder()
        .method("POST")
        .uri("/api/v1/libraries")
        .header(ORIGIN, "https://evil.example")
        .header("sec-fetch-site", "cross-site")
        .header(AUTHORIZATION, format!("Bearer {tok}"))
        .header(COOKIE, format!("{AUTH_COOKIE_NAME}={tok}"))
        .body(Body::empty())
        .unwrap()
        .into_parts();
    assert!(authenticate_web_parts(&bearer_parts, &auth).await.is_ok());

    let (apikey_parts, _) = Request::builder()
        .method("POST")
        .uri("/api/v1/libraries")
        .header(ORIGIN, "https://evil.example")
        .header("sec-fetch-site", "cross-site")
        .header("x-api-key", api_key.as_str())
        .body(Body::empty())
        .unwrap()
        .into_parts();
    assert!(authenticate_web_parts(&apikey_parts, &auth).await.is_ok());

    let b64 = BASE64_STANDARD.encode("admin:password123");
    let (basic_parts, _) = Request::builder()
        .method("GET")
        .uri("/opds/book/v1.2")
        .header(ORIGIN, "https://evil.example")
        .header(AUTHORIZATION, format!("Basic {b64}"))
        .body(Body::empty())
        .unwrap()
        .into_parts();
    assert!(authenticate_opds_parts(&basic_parts, &auth).await.is_ok());
}

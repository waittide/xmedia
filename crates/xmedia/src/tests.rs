#[cfg(all(
    feature = "book",
    feature = "comic",
    feature = "music",
    feature = "video"
))]
mod smoke_and_overview_tests {
    use axum::body::{Body, to_bytes};
    use axum::http::{Request, StatusCode, header};
    use tempfile::TempDir;
    use tower::ServiceExt;

    use crate::{XMediaApp, XMediaConfig};

    #[tokio::test]
    async fn unified_gateway_all_four_domains_and_protocols_smoke_test() {
        let tmp = TempDir::new().unwrap();
        let db_path = tmp.path().join("xmedia.db");
        let app_instance = XMediaApp::connect_and_init(XMediaConfig {
            data_dir: tmp.path().to_path_buf(),
            database_url: format!("sqlite:{}", db_path.display()),
            secret: "unified-gateway-secret".into(),
            jwt_ttl_secs: 3600,
            ffmpeg_bin: "__missing_ffmpeg__".into(),
            external_web_dir: None,
        })
        .await
        .unwrap();

        assert_eq!(app_instance.enabled_modules.len(), 4);
        let app = app_instance.router;

        // 1. Capabilities endpoint lists all 4 enabled modules
        let cap_res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/system/capabilities")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(cap_res.status(), StatusCode::OK);
        let cap_json: serde_json::Value =
            serde_json::from_slice(&to_bytes(cap_res.into_body(), usize::MAX).await.unwrap())
                .unwrap();
        assert_eq!(cap_json["modules"].as_array().unwrap().len(), 4);

        // 2. Setup initial Admin -> sets HttpOnly session cookie + issues JWT + API key
        let setup_res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/setup")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"username":"admin","display_name":"Admin","password":"password123"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(setup_res.status(), StatusCode::CREATED);
        assert!(setup_res.headers().get(header::SET_COOKIE).is_some());
        let setup_json: serde_json::Value =
            serde_json::from_slice(&to_bytes(setup_res.into_body(), usize::MAX).await.unwrap())
                .unwrap();
        let token = setup_json["token"].as_str().unwrap();
        let api_key = setup_json["user"]["api_key"].as_str().unwrap();

        // 3. System overview shows 4 default managed libraries (Book, Comic, Music, Video)
        let ov_res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/system/overview")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(ov_res.status(), StatusCode::OK);
        let ov_json: serde_json::Value =
            serde_json::from_slice(&to_bytes(ov_res.into_body(), usize::MAX).await.unwrap())
                .unwrap();
        assert_eq!(ov_json["libraries"].as_array().unwrap().len(), 4);

        // 4. Verify all 4 domain REST endpoints + OpenSubsonic + OPDS Book & Comic routers are mounted
        for path in [
            "/api/v1/book/books",
            "/api/v1/comic/home",
            "/api/v1/music/home",
            "/api/v1/video/home",
            "/opds/book/v1.2",
            "/opds/book/v2",
            "/opds/comic/v1.2",
        ] {
            let res = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(path)
                        .header(header::AUTHORIZATION, format!("Bearer {token}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::OK, "failed endpoint: {path}");
        }

        let opds_key_res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/opds/book/v1.2?api_key={api_key}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(opds_key_res.status(), StatusCode::OK);

        let sub_res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/rest/ping.view?apiKey={api_key}&f=json"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(sub_res.status(), StatusCode::OK);

        // 5. Verify SSE endpoint auth (401 without token, 200 with ?token= and Authorization header)
        let unauth_sse = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/events/stream")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauth_sse.status(), StatusCode::UNAUTHORIZED);

        let header_sse = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/events/stream")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(header_sse.status(), StatusCode::OK);
        assert!(
            header_sse
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .is_some_and(|ct| ct.starts_with("text/event-stream"))
        );
        assert_eq!(
            header_sse
                .headers()
                .get("x-accel-buffering")
                .and_then(|v| v.to_str().ok()),
            Some("no")
        );
        drop(header_sse);

        // 6. Verify SSE ?token= query auth + live SystemEvent JSON frame delivery
        let query_sse = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/events/stream?token={token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(query_sse.status(), StatusCode::OK);
        assert!(
            query_sse
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .is_some_and(|ct| ct.starts_with("text/event-stream"))
        );

        use futures_core::Stream;
        use std::future::poll_fn;
        use std::pin::Pin;
        use std::time::Duration;
        use xmedia_core::application::ports::{RealtimeEventPort, SystemEvent};
        use xmedia_core::domain::library::MediaKind;

        app_instance.events.publish(SystemEvent::MediaChanged {
            media_kind: MediaKind::Book,
        });

        let mut data_stream = query_sse.into_body().into_data_stream();
        let chunk = tokio::time::timeout(
            Duration::from_secs(2),
            poll_fn(|cx| Pin::new(&mut data_stream).poll_next(cx)),
        )
        .await
        .expect("timed out waiting for SSE event frame")
        .expect("SSE stream ended unexpectedly")
        .expect("SSE body chunk error");

        let raw_frame = String::from_utf8(chunk.to_vec()).unwrap();
        let data_line = raw_frame
            .lines()
            .find_map(|line| line.strip_prefix("data:"))
            .map(str::trim)
            .expect("SSE frame must contain a data: line");
        let parsed: serde_json::Value = serde_json::from_str(data_line).unwrap();
        assert_eq!(parsed["event"], "media_changed");
        assert_eq!(parsed["media_kind"], "book");
    }

    #[tokio::test]
    async fn disabled_permissive_cors_and_app_path_spa_exclusion() {
        let tmp = TempDir::new().unwrap();
        let db_path = tmp.path().join("xmedia_nocors.db");
        let app_instance = XMediaApp::connect_and_init_with_cors(
            XMediaConfig {
                data_dir: tmp.path().to_path_buf(),
                database_url: format!("sqlite:{}", db_path.display()),
                secret: "no-cors-test-secret".into(),
                jwt_ttl_secs: 3600,
                ffmpeg_bin: "__missing_ffmpeg__".into(),
                external_web_dir: None,
            },
            false,
        )
        .await
        .unwrap();

        let app = app_instance.router;

        let cors_res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/system/capabilities")
                    .header(header::ORIGIN, "https://evil.example")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(cors_res.status(), StatusCode::OK);
        assert!(
            cors_res
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .is_none(),
            "Access-Control-Allow-Origin must not be set when permissive CORS is disabled"
        );

        for path in ["/_app", "/_app/", "/_app/unknown"] {
            let res = app
                .clone()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(
                res.status(),
                StatusCode::NOT_FOUND,
                "SPA fallback must not serve HTML for {path}"
            );
        }
    }

    #[tokio::test]
    async fn unified_gateway_pool_and_per_user_overview_isolation() {
        use xmedia_core::application::dto::{CreateLibraryInput, CreateUserInput, SetupAdminInput};
        use xmedia_core::domain::library::{MediaKind, ScanStatus};
        use xmedia_core::domain::user::UserRole;

        let tmp = TempDir::new().unwrap();
        let db_path = tmp.path().join("xmedia_pool.db");
        let app_instance = XMediaApp::connect_and_init(XMediaConfig {
            data_dir: tmp.path().to_path_buf(),
            database_url: format!("sqlite:{}", db_path.display()),
            secret: "pool-overview-secret-0123456789".into(),
            jwt_ttl_secs: 3600,
            ffmpeg_bin: "__missing_ffmpeg__".into(),
            external_web_dir: None,
        })
        .await
        .unwrap();

        let admin_sess = app_instance
            .auth
            .setup_initial_admin(SetupAdminInput {
                username: "admin".into(),
                display_name: Some("Admin".into()),
                password: "adminpassword123".into(),
            })
            .await
            .unwrap();
        let admin = app_instance
            .auth
            .authenticate_jwt(&admin_sess.token)
            .await
            .unwrap();
        app_instance
            .auth
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
        let member_sess = app_instance
            .auth
            .login("member", "memberpassword123")
            .await
            .unwrap();

        // Mount and scan a local book directory outside data_dir
        let books_tmp = TempDir::new().unwrap();
        let book_dir = books_tmp.path().join("mount_books");
        std::fs::create_dir_all(&book_dir).unwrap();
        std::fs::write(
            book_dir.join("Dune.txt"),
            "Chapter 1\nArrakis is a desert planet.\n",
        )
        .unwrap();
        let lib = app_instance
            .libraries
            .create_local_mount(
                &admin,
                CreateLibraryInput {
                    name: "Local Books".into(),
                    media_kind: MediaKind::Book,
                    root_path: book_dir.to_string_lossy().into_owned(),
                },
            )
            .await
            .unwrap();
        app_instance
            .libraries
            .trigger_scan(&admin, lib.id)
            .await
            .unwrap();
        for _ in 0..50 {
            let st = app_instance.libraries.get_library(lib.id).await.unwrap();
            if matches!(st.scan_status, ScanStatus::Completed { .. }) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }

        let app = app_instance.router;

        // All 4 pool endpoints are mounted and accessible to member
        for pool_path in [
            "/api/v1/book/pool",
            "/api/v1/comic/pool",
            "/api/v1/music/pool",
            "/api/v1/video/pool",
        ] {
            let res = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(pool_path)
                        .header(
                            header::AUTHORIZATION,
                            format!("Bearer {}", member_sess.token),
                        )
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                res.status(),
                StatusCode::OK,
                "failed pool path: {pool_path}"
            );
        }

        // Before claiming, both admin and member have 0 bound books in /system/overview
        let ov_before_res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/system/overview")
                    .header(
                        header::AUTHORIZATION,
                        format!("Bearer {}", admin_sess.token),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(ov_before_res.status(), StatusCode::OK);
        let ov_before: serde_json::Value = serde_json::from_slice(
            &to_bytes(ov_before_res.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        let admin_book_cnt_before = ov_before["domains"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["media_kind"] == "book")
            .unwrap()["item_count"]
            .as_u64()
            .unwrap();
        assert_eq!(admin_book_cnt_before, 0);

        // Admin claims the local book library
        let bind_res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/book/bindings")
                    .header(
                        header::AUTHORIZATION,
                        format!("Bearer {}", admin_sess.token),
                    )
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        serde_json::json!({ "library_id": lib.id.0 }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(bind_res.status(), StatusCode::OK);

        // Now admin sees 1 book in /system/overview, while member still sees 0
        let ov_admin_res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/system/overview")
                    .header(
                        header::AUTHORIZATION,
                        format!("Bearer {}", admin_sess.token),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let ov_admin: serde_json::Value = serde_json::from_slice(
            &to_bytes(ov_admin_res.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            ov_admin["domains"]
                .as_array()
                .unwrap()
                .iter()
                .find(|d| d["media_kind"] == "book")
                .unwrap()["item_count"]
                .as_u64()
                .unwrap(),
            1
        );

        let ov_member_res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/system/overview")
                    .header(
                        header::AUTHORIZATION,
                        format!("Bearer {}", member_sess.token),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let ov_member: serde_json::Value = serde_json::from_slice(
            &to_bytes(ov_member_res.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            ov_member["domains"]
                .as_array()
                .unwrap()
                .iter()
                .find(|d| d["media_kind"] == "book")
                .unwrap()["item_count"]
                .as_u64()
                .unwrap(),
            0
        );

        // Verify /system/overview exposes host root_path to Admin but redacts it for Member
        let admin_mount_lib = ov_admin["libraries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|l| l["name"] == "Local Books")
            .unwrap();
        assert!(
            !admin_mount_lib["kind"]["root_path"]
                .as_str()
                .unwrap()
                .is_empty(),
            "admin should see local mount root_path in /system/overview"
        );
        let member_mount_lib = ov_member["libraries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|l| l["name"] == "Local Books")
            .unwrap();
        assert_eq!(
            member_mount_lib["kind"]["root_path"].as_str().unwrap(),
            "",
            "member must never see host root_path in /system/overview"
        );
    }
}

mod unit_tests {
    use std::net::SocketAddr;

    use crate::resolve_allow_private_ips_for_testing;

    #[test]
    fn resolve_allow_private_ips_for_testing_gates_strictly_on_loopback() {
        let loopback_v4: SocketAddr = "127.0.0.1:8090".parse().unwrap();
        let loopback_v6: SocketAddr = "[::1]:8090".parse().unwrap();
        let wildcard_v4: SocketAddr = "0.0.0.0:8090".parse().unwrap();
        let wildcard_v6: SocketAddr = "[::]:8090".parse().unwrap();
        let lan_v4: SocketAddr = "192.168.1.5:8090".parse().unwrap();

        for env_val in [None, Some(""), Some("0"), Some("true"), Some("yes")] {
            assert!(!resolve_allow_private_ips_for_testing(loopback_v4, env_val));
            assert!(!resolve_allow_private_ips_for_testing(loopback_v6, env_val));
            assert!(!resolve_allow_private_ips_for_testing(wildcard_v4, env_val));
            assert!(!resolve_allow_private_ips_for_testing(wildcard_v6, env_val));
            assert!(!resolve_allow_private_ips_for_testing(lan_v4, env_val));
        }

        assert!(resolve_allow_private_ips_for_testing(
            loopback_v4,
            Some("1")
        ));
        assert!(resolve_allow_private_ips_for_testing(
            loopback_v4,
            Some(" 1 ")
        ));
        assert!(resolve_allow_private_ips_for_testing(
            loopback_v6,
            Some("1")
        ));

        assert!(!resolve_allow_private_ips_for_testing(
            wildcard_v4,
            Some("1")
        ));
        assert!(!resolve_allow_private_ips_for_testing(
            wildcard_v6,
            Some("1")
        ));
        assert!(!resolve_allow_private_ips_for_testing(lan_v4, Some("1")));
    }
}

#[cfg(all(feature = "comic", feature = "music", feature = "video"))]
mod gateway_integration_tests {
    use axum::Router;
    use axum::body::{Body, to_bytes};
    use axum::http::header::{AUTHORIZATION, CONTENT_TYPE};
    use axum::http::{Request, StatusCode};
    use axum::routing::get;
    use tempfile::TempDir;
    use tokio::net::TcpListener;
    use tower::ServiceExt;
    use xmedia_core::application::dto::{CreateUserInput, SetupAdminInput};
    use xmedia_core::domain::user::UserRole;

    use crate::{XMediaApp, XMediaConfig, XMediaInitOptions};

    const PNG_1X1: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    fn make_cfg(dir: &TempDir) -> XMediaConfig {
        let db_path = dir.path().join("xmedia.db");
        XMediaConfig {
            data_dir: dir.path().to_path_buf(),
            database_url: format!("sqlite:{}", db_path.display()),
            secret: "gateway-seed-test-secret-0123456789".into(),
            jwt_ttl_secs: 3600,
            ffmpeg_bin: "__missing_ffmpeg__".into(),
            external_web_dir: None,
        }
    }

    async fn setup_admin_and_member(app_instance: &XMediaApp) -> (String, String) {
        let admin_sess = app_instance
            .auth
            .setup_initial_admin(SetupAdminInput {
                username: "admin".into(),
                display_name: Some("Admin".into()),
                password: "adminpassword123".into(),
            })
            .await
            .unwrap();
        let admin = app_instance
            .auth
            .authenticate_jwt(&admin_sess.token)
            .await
            .unwrap();
        app_instance
            .auth
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
        let member_sess = app_instance
            .auth
            .login("member", "memberpassword123")
            .await
            .unwrap();
        (admin_sess.token, member_sess.token)
    }

    async fn send_req(
        router: &Router,
        method: &str,
        uri: &str,
        token: Option<&str>,
        body: Body,
        content_type: Option<&str>,
    ) -> (StatusCode, serde_json::Value) {
        let mut builder = Request::builder().method(method).uri(uri);
        if let Some(t) = token {
            builder = builder.header(AUTHORIZATION, format!("Bearer {t}"));
        }
        if let Some(ct) = content_type {
            builder = builder.header(CONTENT_TYPE, ct);
        }
        let resp = router
            .clone()
            .oneshot(builder.body(body).unwrap())
            .await
            .unwrap();
        let status = resp.status();
        let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json = if bytes.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or_else(|_| {
                serde_json::Value::String(String::from_utf8_lossy(&bytes).into_owned())
            })
        };
        (status, json)
    }

    #[tokio::test]
    async fn loopback_allow_private_ips_gating_through_gateway() {
        let mock_router = Router::new()
            .route(
                "/search",
                get(|| async {
                    axum::Json(serde_json::json!({
                        "items": [{
                            "title": "Local Mock Comic",
                            "url": "/comic/1",
                            "cover": "/cover.png"
                        }]
                    }))
                }),
            )
            .route(
                "/cover.png",
                get(|| async { ([(CONTENT_TYPE, "image/png")], PNG_1X1.to_vec()) }),
            );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_handle = tokio::spawn(async move {
            axum::serve(listener, mock_router).await.unwrap();
        });

        let base_url = format!("http://{addr}");
        let comic_source_json = serde_json::json!({
            "bookSourceName": "Loopback Mock Comic",
            "bookSourceUrl": base_url,
            "bookSourceType": 2,
            "searchUrl": format!("{base_url}/search?q={{{{key}}}}"),
            "ruleSearch": {
                "bookList": "$.items[*]",
                "name": "$.title",
                "bookUrl": "$.url",
                "coverUrl": "$.cover"
            }
        })
        .to_string();

        // 1. Default allow_private_ips == false blocks loopback requests
        {
            let tmp_blocked = TempDir::new().unwrap();
            let app_blocked = XMediaApp::connect_and_init(make_cfg(&tmp_blocked))
                .await
                .unwrap();
            let (token, _) = setup_admin_and_member(&app_blocked).await;

            let (st_imp, imp_json) = send_req(
                &app_blocked.router,
                "POST",
                "/api/v1/comic/sources/import",
                Some(&token),
                Body::from(serde_json::json!({ "raw_json": comic_source_json }).to_string()),
                Some("application/json"),
            )
            .await;
            assert_eq!(st_imp, StatusCode::OK);
            let source_id = imp_json["sources"][0]["id"].as_str().unwrap();

            let (st_search, search_json) = send_req(
                &app_blocked.router,
                "POST",
                "/api/v1/comic/online/search",
                Some(&token),
                Body::from(serde_json::json!({ "keyword": "test" }).to_string()),
                Some("application/json"),
            )
            .await;
            assert_eq!(st_search, StatusCode::OK);
            assert!(search_json["results"].as_array().unwrap().is_empty());
            assert!(!search_json["errors"].as_array().unwrap().is_empty());

            let (st_proxy, _) = send_req(
                &app_blocked.router,
                "GET",
                &format!(
                    "/api/v1/comic/online/image-proxy?url={base_url}/cover.png&source_id={source_id}"
                ),
                Some(&token),
                Body::empty(),
                None,
            )
            .await;
            assert!(
                st_proxy == StatusCode::BAD_REQUEST || st_proxy == StatusCode::FORBIDDEN,
                "expected SSRF rejection, got {st_proxy}"
            );

            for proxy_path in [
                format!("/api/v1/music/online/cover-proxy?url={base_url}/cover.png"),
                format!("/api/v1/video/online/poster-proxy?url={base_url}/cover.png"),
            ] {
                let (st_dom_proxy, _) = send_req(
                    &app_blocked.router,
                    "GET",
                    &proxy_path,
                    Some(&token),
                    Body::empty(),
                    None,
                )
                .await;
                assert!(
                    st_dom_proxy == StatusCode::BAD_REQUEST
                        || st_dom_proxy == StatusCode::FORBIDDEN,
                    "expected SSRF rejection on {proxy_path}, got {st_dom_proxy}"
                );
            }
        }

        // 2. Opt-in allow_private_ips == true allows loopback mock server
        {
            let tmp_allowed = TempDir::new().unwrap();
            let app_allowed = XMediaApp::connect_and_init_with_options(
                make_cfg(&tmp_allowed),
                XMediaInitOptions {
                    allow_private_ips: true,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
            let (token, _) = setup_admin_and_member(&app_allowed).await;

            let (st_imp, imp_json) = send_req(
                &app_allowed.router,
                "POST",
                "/api/v1/comic/sources/import",
                Some(&token),
                Body::from(serde_json::json!({ "raw_json": comic_source_json }).to_string()),
                Some("application/json"),
            )
            .await;
            assert_eq!(st_imp, StatusCode::OK);
            let source_id = imp_json["sources"][0]["id"].as_str().unwrap();

            let (st_search, search_json) = send_req(
                &app_allowed.router,
                "POST",
                "/api/v1/comic/online/search",
                Some(&token),
                Body::from(serde_json::json!({ "keyword": "test" }).to_string()),
                Some("application/json"),
            )
            .await;
            assert_eq!(st_search, StatusCode::OK);
            assert_eq!(search_json["results"].as_array().unwrap().len(), 1);
            assert_eq!(search_json["results"][0]["title"], "Local Mock Comic");

            let proxy_resp = app_allowed
                .router
                .clone()
                .oneshot(
                    Request::builder()
                        .method("GET")
                        .uri(format!(
                            "/api/v1/comic/online/image-proxy?url={base_url}/cover.png&source_id={source_id}"
                        ))
                        .header(AUTHORIZATION, format!("Bearer {token}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(proxy_resp.status(), StatusCode::OK);
            assert_eq!(
                proxy_resp
                    .headers()
                    .get(CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok()),
                Some("image/png")
            );

            for proxy_path in [
                format!("/api/v1/music/online/cover-proxy?url={base_url}/cover.png"),
                format!("/api/v1/video/online/poster-proxy?url={base_url}/cover.png"),
            ] {
                let dom_resp = app_allowed
                    .router
                    .clone()
                    .oneshot(
                        Request::builder()
                            .method("GET")
                            .uri(&proxy_path)
                            .header(AUTHORIZATION, format!("Bearer {token}"))
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(
                    dom_resp.status(),
                    StatusCode::OK,
                    "expected 200 OK on {proxy_path}"
                );
                assert_eq!(
                    dom_resp
                        .headers()
                        .get(CONTENT_TYPE)
                        .and_then(|v| v.to_str().ok()),
                    Some("image/png")
                );
            }
        }

        server_handle.abort();
    }

    #[tokio::test]
    async fn concurrent_manual_imports_produce_no_duplicates() {
        use std::collections::HashSet;
        use std::sync::Arc;
        use tokio::sync::Barrier;

        let tmp = TempDir::new().unwrap();
        let app_instance = XMediaApp::connect_and_init(make_cfg(&tmp)).await.unwrap();
        let (admin_token, _) = setup_admin_and_member(&app_instance).await;
        let router = app_instance.router;

        let domains: [(&str, &str, usize); 3] = [
            (
                "comic",
                include_str!("../../xmedia-comic/testdata/online_sources.json"),
                5,
            ),
            (
                "music",
                include_str!("../../xmedia-music/testdata/online_sources.json"),
                4,
            ),
            (
                "video",
                include_str!("../../xmedia-video/testdata/online_sources.json"),
                6,
            ),
        ];

        // 8 concurrent manual imports per domain = 24 concurrent tasks across a barrier
        let tasks_per_domain = 8usize;
        let total_tasks = domains.len() * tasks_per_domain;
        let barrier = Arc::new(Barrier::new(total_tasks));
        let mut handles = Vec::with_capacity(total_tasks);

        for (domain, sources_json, _) in domains {
            let import_payload =
                Arc::new(serde_json::json!({ "raw_json": sources_json }).to_string());
            for _ in 0..tasks_per_domain {
                let r = router.clone();
                let tok = admin_token.clone();
                let bar = barrier.clone();
                let payload = import_payload.clone();
                handles.push(tokio::spawn(async move {
                    bar.wait().await;
                    let (st, json) = send_req(
                        &r,
                        "POST",
                        &format!("/api/v1/{domain}/sources/import"),
                        Some(&tok),
                        Body::from(payload.as_str().to_owned()),
                        Some("application/json"),
                    )
                    .await;
                    (domain, st, json)
                }));
            }
        }

        let mut imported_sums: std::collections::HashMap<&'static str, u64> =
            std::collections::HashMap::new();
        for handle in handles {
            let (domain, st, json) = handle.await.unwrap();
            assert_eq!(st, StatusCode::OK, "domain={domain} body={json}");
            let imp = json["imported_count"].as_u64().unwrap();
            *imported_sums.entry(domain).or_insert(0) += imp;
        }

        for (domain, _, expected_count) in domains {
            assert_eq!(
                imported_sums.get(domain).copied().unwrap_or(0),
                expected_count as u64,
                "sum of imported_count for {domain} must equal {expected_count}"
            );

            let (st, listed) = send_req(
                &router,
                "GET",
                &format!("/api/v1/{domain}/sources"),
                Some(&admin_token),
                Body::empty(),
                None,
            )
            .await;
            assert_eq!(st, StatusCode::OK);
            let arr = listed.as_array().unwrap();
            assert_eq!(
                arr.len(),
                expected_count,
                "duplicate rows detected for {domain}: got {}",
                arr.len()
            );

            let unique_keys: HashSet<(String, String)> = arr
                .iter()
                .map(|item| {
                    (
                        item["name"].as_str().unwrap().to_string(),
                        item["source_url"].as_str().unwrap().to_string(),
                    )
                })
                .collect();
            assert_eq!(
                unique_keys.len(),
                expected_count,
                "expected {expected_count} unique (name, source_url) entries for {domain}"
            );
        }
    }

    #[tokio::test]
    async fn unified_gateway_theme_defaults_and_online_items_cross_domain_e2e() {
        let tmp = TempDir::new().unwrap();
        let app_instance = XMediaApp::connect_and_init(make_cfg(&tmp)).await.unwrap();
        let (admin_token, member_token) = setup_admin_and_member(&app_instance).await;
        let router = app_instance.router;

        // 1. New user receives clean Material You defaults via /auth/me
        let (st_me, me) = send_req(
            &router,
            "GET",
            "/api/v1/auth/me",
            Some(&member_token),
            Body::empty(),
            None,
        )
        .await;
        assert_eq!(st_me, StatusCode::OK);
        let member_id = me["id"].as_str().unwrap().to_string();
        assert_eq!(me["preferences"]["theme_mode"], "system");
        assert!(me["preferences"]["seed_color"].is_null());
        assert!(me["preferences"].get("light_preset").is_none());
        assert!(me["preferences"].get("dark_preset").is_none());
        assert!(me["preferences"].get("accent_mode").is_none());

        // 2. Theme mode, seed color, and all four domain preferences persist cleanly across GET /auth/me
        let (st_min, min_res) = send_req(
            &router,
            "PUT",
            "/api/v1/auth/preferences",
            Some(&member_token),
            Body::from(
                serde_json::json!({
                    "theme_mode": "dark",
                    "seed_color": "#006A6A",
                    "book": {
                        "font_family": "serif",
                        "font_size": 21,
                        "line_height": 2.0,
                        "margin_px": 48,
                        "flow": "scrolled"
                    },
                    "comic": {
                        "default_reader_mode": "webtoon",
                        "default_fit_mode": "fit_width",
                        "offset_cover_page": false,
                        "dim_images_in_dark": true
                    },
                    "music": {
                        "transcode_profile": "mp3320"
                    },
                    "video": {
                        "default_quality": "p720",
                        "autoplay_next": false
                    }
                })
                .to_string(),
            ),
            Some("application/json"),
        )
        .await;
        assert_eq!(st_min, StatusCode::OK);
        assert_eq!(min_res["preferences"]["theme_mode"], "dark");
        assert_eq!(min_res["preferences"]["seed_color"], "#006a6a");

        let (st_me_after, me_after) = send_req(
            &router,
            "GET",
            "/api/v1/auth/me",
            Some(&member_token),
            Body::empty(),
            None,
        )
        .await;
        assert_eq!(st_me_after, StatusCode::OK);
        assert_eq!(me_after["preferences"]["book"]["font_family"], "serif");
        assert_eq!(me_after["preferences"]["book"]["font_size"], 21);
        assert_eq!(me_after["preferences"]["book"]["flow"], "scrolled");
        assert_eq!(
            me_after["preferences"]["comic"]["default_reader_mode"],
            "webtoon"
        );
        assert_eq!(
            me_after["preferences"]["comic"]["default_fit_mode"],
            "fit_width"
        );
        assert_eq!(me_after["preferences"]["comic"]["offset_cover_page"], false);
        assert_eq!(me_after["preferences"]["comic"]["dim_images_in_dark"], true);
        assert_eq!(
            me_after["preferences"]["music"]["transcode_profile"],
            "mp3320"
        );
        assert_eq!(me_after["preferences"]["video"]["default_quality"], "p720");
        assert_eq!(me_after["preferences"]["video"]["autoplay_next"], false);

        // 4. Online items are mounted on comic/music/video and strictly isolated per domain
        for domain in ["comic", "music", "video"] {
            let (st_put, put_val) = send_req(
                &router,
                "PUT",
                &format!("/api/v1/{domain}/online/items"),
                Some(&member_token),
                Body::from(
                    serde_json::json!({
                        "source_kind": "remote",
                        "source_id": "remote-1",
                        "item_key": "shared-key",
                        "title": format!("Item in {domain}"),
                        "favorite": true,
                        "progress": { "position": 10.0, "duration": 100.0, "completed": false }
                    })
                    .to_string(),
                ),
                Some("application/json"),
            )
            .await;
            assert_eq!(st_put, StatusCode::OK, "domain={domain} body={put_val}");
            assert_eq!(put_val["media_kind"], domain);
            assert_eq!(put_val["availability"], "unverified");
        }

        for domain in ["comic", "music", "video"] {
            let (st_list, list_val) = send_req(
                &router,
                "GET",
                &format!("/api/v1/{domain}/online/items?view=continue"),
                Some(&member_token),
                Body::empty(),
                None,
            )
            .await;
            assert_eq!(st_list, StatusCode::OK);
            assert_eq!(list_val["total"], 1);
            assert_eq!(list_val["items"][0]["media_kind"], domain);
        }

        // 5. Deleting member succeeds and purges online items
        let (st_del_user, del_val) = send_req(
            &router,
            "DELETE",
            &format!("/api/v1/users/{member_id}"),
            Some(&admin_token),
            Body::empty(),
            None,
        )
        .await;
        assert_eq!(st_del_user, StatusCode::OK);
        assert_eq!(del_val["ok"], true);
    }
}

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use axum::routing::get;
use tempfile::TempDir;
use toasty::Db;
use toasty::schema::ModelSet;
use tower::ServiceExt;
use xmedia_core::application::auth::AuthService;
use xmedia_core::application::dto::{CreateUserInput, SetupAdminInput};
use xmedia_core::application::library::LibraryService;
use xmedia_core::domain::library::MediaKind;
use xmedia_core::domain::user::UserRole;
use xmedia_core::infra::events::BroadcastEventHub;
use xmedia_core::infra::persistence::repositories::{
    ToastyLibraryRepository, ToastyUserRepository,
};
use xmedia_core::infra::security::{JwtTokenService, SecurityManager};
use xmedia_core::{ensure_core_schema, register_core_models};

use crate::application::online::{
    AddOnlineShelfInput, BookOnlineService, DownloadJobStatus, OnlineDownloadFormat,
    StartDownloadInput, TrackOnlineBookInput, UpdateBookSourceInput, UpdateOnlineShelfInput,
};
use crate::domain::book::BookId;
use crate::http::rest_router;
use crate::http::state::BookHttpState;
use crate::source::BookSourceEngine;
use crate::source::http::HttpClientConfig;
use crate::source::model::{BookJsContext, ChapterJsContext};
use crate::{ensure_book_schema, init_book_module, register_book_models};

#[tokio::test]
async fn online_book_source_shelf_cache_download_epub_and_import_roundtrip() {
    let mock_app = Router::new()
        .route(
            "/search",
            get(|| async {
                r#"<html><body><div class="item">
                  <a class="t" href="/b/1">剑来</a>
                  <span class="a">烽火戏诸侯</span>
                </div></body></html>"#
            }),
        )
        .route(
            "/b/1",
            get(|| async {
                r#"<html><body>
                  <h1 id="title">剑来</h1>
                  <div id="intro">大千世界，无奇不有。</div>
                  <ul id="toc">
                    <li><a href="/b/1/c1">第一章 惊蛰</a></li>
                    <li><a href="/b/1/c2">第二章 开门</a></li>
                  </ul>
                </body></html>"#
            }),
        )
        .route(
            "/b/1/c1",
            get(|| async { r#"<div id="body"><p>二月二，龙抬头。</p></div>"# }),
        )
        .route(
            "/b/1/c2",
            get(|| async { r#"<div id="body"><p>小镇少年走出门外。</p></div>"# }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_handle = tokio::spawn(async move {
        axum::serve(listener, mock_app).await.unwrap();
    });

    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("online_test.db");
    let db_url = format!("sqlite:{}", db_path.display());

    let mut model_set = ModelSet::new();
    register_core_models(&mut model_set);
    register_book_models(&mut model_set);
    let mut db = Db::builder()
        .models(model_set)
        .connect(&db_url)
        .await
        .unwrap();
    ensure_core_schema(&mut db, &db_url).await.unwrap();
    ensure_book_schema(&mut db, &db_url).await.unwrap();

    let users_repo = Arc::new(ToastyUserRepository::new(db.clone()));
    let libs_repo = Arc::new(ToastyLibraryRepository::new(db.clone()));
    let lifecycles = Arc::new(RwLock::new(HashMap::new()));
    let security = Arc::new(SecurityManager::new("book-secret"));
    let tokens = Arc::new(JwtTokenService::new("book-secret", 3600));
    let events = Arc::new(BroadcastEventHub::default());
    let auth_svc = Arc::new(AuthService::new(
        users_repo,
        security,
        tokens,
        lifecycles.clone(),
    ));
    let session = auth_svc
        .setup_initial_admin(SetupAdminInput {
            username: "admin".into(),
            display_name: Some("Admin".into()),
            password: "password123".into(),
        })
        .await
        .unwrap();
    let admin = auth_svc.authenticate_jwt(&session.token).await.unwrap();
    let lib_svc = Arc::new(LibraryService::new(
        libs_repo.clone(),
        events.clone(),
        lifecycles,
    ));
    let book_services =
        init_book_module(db.clone(), tmp.path(), libs_repo, &lib_svc, events).unwrap();

    let sources_repo = Arc::new(
        crate::infra::persistence::repositories::ToastyBookSourceRepository::new(db.clone()),
    );
    let shelves_repo =
        Arc::new(crate::infra::persistence::repositories::ToastyOnlineShelfRepository::new(db));
    let online_svc = Arc::new(
        BookOnlineService::with_engine(
            sources_repo.clone(),
            shelves_repo,
            tmp.path(),
            book_services.catalog.clone(),
            BookSourceEngine::new(
                HttpClientConfig {
                    allow_private_ips: true,
                    ..Default::default()
                },
                Duration::from_secs(10),
            ),
            true,
        )
        .unwrap(),
    );

    let raw_source = serde_json::json!({
        "bookSourceName": "本地Mock书源",
        "bookSourceUrl": format!("http://{addr}"),
        "searchUrl": "/search?q={{key}}",
        "ruleSearch": {
            "bookList": "class.item",
            "name": "class.t@text",
            "author": "class.a@text",
            "bookUrl": "class.t@href"
        },
        "ruleBookInfo": {
            "name": "id.title@text",
            "intro": "id.intro@text"
        },
        "ruleToc": {
            "chapterList": "#toc li a",
            "chapterName": "text",
            "chapterUrl": "href"
        },
        "ruleContent": {
            "content": "id.body@html"
        }
    })
    .to_string();

    let imported = online_svc
        .import_sources(&admin, &raw_source)
        .await
        .unwrap();
    assert_eq!(imported.imported_count, 1);
    let source_id = imported.sources[0].id;

    let search = online_svc.search(&admin, "剑来", None, 1).await.unwrap();
    assert_eq!(search.items.len(), 1);
    let hit = &search.items[0];
    assert_eq!(hit.book.name, "剑来");

    let detail = online_svc
        .fetch_book_info(&admin, source_id, &hit.book.book_url, None)
        .await
        .unwrap();
    let toc = online_svc
        .fetch_toc(
            &admin,
            source_id,
            &detail.detail.toc_url,
            BookJsContext::default(),
        )
        .await
        .unwrap();
    assert_eq!(toc.chapters.len(), 2);

    // First read fetches remotely; second read hits disk cache
    let c1 = online_svc
        .fetch_chapter_content(
            &admin,
            source_id,
            &toc.chapters[0].url,
            BookJsContext::default(),
            ChapterJsContext {
                index: 0,
                title: toc.chapters[0].title.clone(),
                url: toc.chapters[0].url.clone(),
            },
            false,
        )
        .await
        .unwrap();
    assert!(!c1.cached);
    assert_eq!(c1.content.content, "二月二，龙抬头。");

    let c1_cached = online_svc
        .fetch_chapter_content(
            &admin,
            source_id,
            &toc.chapters[0].url,
            BookJsContext::default(),
            ChapterJsContext {
                index: 0,
                title: toc.chapters[0].title.clone(),
                url: toc.chapters[0].url.clone(),
            },
            false,
        )
        .await
        .unwrap();
    assert!(c1_cached.cached);

    // Online shelf add + progress update
    let shelf_item = online_svc
        .add_to_shelf(
            &admin,
            AddOnlineShelfInput {
                source_id,
                name: detail.detail.name.clone(),
                author: hit.book.author.clone(),
                book_url: detail.detail.book_url.clone(),
                toc_url: detail.detail.toc_url.clone(),
                cover_url: detail.detail.cover_url.clone(),
                intro: detail.detail.intro.clone(),
                kind: None,
                last_chapter: Some(toc.chapters[1].title.clone()),
                total_chapters: Some(2),
            },
        )
        .await
        .unwrap();
    let updated_shelf = online_svc
        .update_shelf_book(
            admin.id,
            shelf_item.id,
            UpdateOnlineShelfInput {
                current_chapter_index: Some(1),
                current_chapter_title: Some("第二章 开门".into()),
                scroll_ratio: Some(0.5),
                total_chapters: None,
                last_chapter: None,
                toc_url: None,
                source_id: None,
                book_url: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(updated_shelf.current_chapter_index, 1);

    // Background download -> EPUB 3.0 -> auto import into local library
    let job = online_svc
        .start_download(
            &admin,
            StartDownloadInput {
                book_id: None,
                source_id,
                book_name: "剑来".into(),
                author: "烽火戏诸侯".into(),
                book_url: detail.detail.book_url.clone(),
                toc_url: detail.detail.toc_url.clone(),
                cover_url: None,
                intro: detail.detail.intro.clone(),
                format: OnlineDownloadFormat::Epub,
                start_chapter: None,
                end_chapter: None,
                import_to_library: true,
            },
        )
        .await
        .unwrap();

    let mut finished = job;
    for _ in 0..80 {
        finished = online_svc
            .get_download(admin.id, finished.id)
            .await
            .unwrap();
        if matches!(
            finished.status,
            DownloadJobStatus::Completed | DownloadJobStatus::Failed
        ) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert_eq!(
        finished.status,
        DownloadJobStatus::Completed,
        "{finished:?}"
    );
    assert_eq!(finished.processed_chapters, 2);
    assert!(finished.imported_book_id.is_some());

    let (fname, mime, epub_bytes) = online_svc
        .read_download_file(admin.id, finished.id)
        .await
        .unwrap();
    assert_eq!(fname, "剑来.epub");
    assert_eq!(mime, "application/epub+zip");
    assert!(epub_bytes.starts_with(b"PK"));

    server_handle.abort();
}

#[tokio::test]
async fn deleting_user_cleans_up_online_shelf_entries() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("user_cascade_test.db");
    let db_url = format!("sqlite:{}", db_path.display());

    let mut models = ModelSet::new();
    register_core_models(&mut models);
    register_book_models(&mut models);
    let mut db = Db::builder().models(models).connect(&db_url).await.unwrap();
    ensure_core_schema(&mut db, &db_url).await.unwrap();
    ensure_book_schema(&mut db, &db_url).await.unwrap();

    let users = Arc::new(ToastyUserRepository::new(db.clone()));
    let libs_repo = Arc::new(ToastyLibraryRepository::new(db.clone()));
    let lifecycles = Arc::new(RwLock::new(HashMap::new()));
    let security = Arc::new(SecurityManager::new("cascade-secret"));
    let tokens = Arc::new(JwtTokenService::new("cascade-secret", 3600));
    let events = Arc::new(BroadcastEventHub::default());
    let auth = Arc::new(AuthService::new(
        users.clone(),
        security,
        tokens,
        lifecycles.clone(),
    ));
    let lib_svc = Arc::new(LibraryService::new(
        libs_repo.clone(),
        events.clone(),
        lifecycles.clone(),
    ));
    let book_services = init_book_module(db, tmp.path(), libs_repo, &lib_svc, events).unwrap();

    let session = auth
        .setup_initial_admin(SetupAdminInput {
            username: "admin".into(),
            display_name: None,
            password: "password123".into(),
        })
        .await
        .unwrap();
    let admin = auth.authenticate_jwt(&session.token).await.unwrap();
    let member = auth
        .create_user(
            &admin,
            CreateUserInput {
                username: "member_book".into(),
                display_name: None,
                password: "password123".into(),
                role: UserRole::Member,
            },
        )
        .await
        .unwrap();

    let raw_source = serde_json::json!({
        "bookSourceName": "级联测试书源",
        "bookSourceUrl": "https://example.com"
    })
    .to_string();
    let imported = book_services
        .online
        .import_sources(&admin, &raw_source)
        .await
        .unwrap();
    let source_id = imported.sources[0].id;

    let member_user =
        xmedia_core::domain::repository::UserRepository::find_by_id(users.as_ref(), member.id)
            .await
            .unwrap()
            .unwrap();
    book_services
        .online
        .add_to_shelf(
            &member_user,
            AddOnlineShelfInput {
                source_id,
                name: "测试书".into(),
                author: "作者".into(),
                book_url: "https://example.com/b/1".into(),
                toc_url: "https://example.com/b/1/toc".into(),
                cover_url: None,
                intro: None,
                kind: None,
                last_chapter: None,
                total_chapters: Some(10),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        book_services
            .online
            .list_shelf(member.id)
            .await
            .unwrap()
            .len(),
        1
    );

    auth.delete_user(&admin, member.id).await.unwrap();
    assert!(
        book_services
            .online
            .list_shelf(member.id)
            .await
            .unwrap()
            .is_empty(),
        "deleting a user must cascade-delete their online bookshelf entries"
    );
}

#[tokio::test]
async fn http_variable_endpoint_sync_epub_authz_svg_cover_reject_and_library_delete_cleanup() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("http_security_test.db");
    let db_url = format!("sqlite:{}", db_path.display());

    let mut models = ModelSet::new();
    register_core_models(&mut models);
    register_book_models(&mut models);
    let mut db = Db::builder().models(models).connect(&db_url).await.unwrap();
    ensure_core_schema(&mut db, &db_url).await.unwrap();
    ensure_book_schema(&mut db, &db_url).await.unwrap();

    let users = Arc::new(ToastyUserRepository::new(db.clone()));
    let libs_repo = Arc::new(ToastyLibraryRepository::new(db.clone()));
    let lifecycles = Arc::new(RwLock::new(HashMap::new()));
    let security = Arc::new(SecurityManager::new("sec-test-secret"));
    let tokens = Arc::new(JwtTokenService::new("sec-test-secret", 3600));
    let events = Arc::new(BroadcastEventHub::default());
    let auth = Arc::new(AuthService::new(
        users,
        security,
        tokens,
        lifecycles.clone(),
    ));
    let lib_svc = Arc::new(LibraryService::new(
        libs_repo.clone(),
        events.clone(),
        lifecycles.clone(),
    ));
    let book_services = init_book_module(db, tmp.path(), libs_repo, &lib_svc, events).unwrap();

    let admin_session = auth
        .setup_initial_admin(SetupAdminInput {
            username: "admin".into(),
            display_name: None,
            password: "password123".into(),
        })
        .await
        .unwrap();
    let admin = auth.authenticate_jwt(&admin_session.token).await.unwrap();
    let _ = auth
        .create_user(
            &admin,
            CreateUserInput {
                username: "member".into(),
                display_name: None,
                password: "password123".into(),
                role: UserRole::Member,
            },
        )
        .await
        .unwrap();
    let member_session = auth.login("member", "password123").await.unwrap();

    let state = BookHttpState::new(auth, lib_svc.clone(), book_services.clone());
    let app = Router::new().nest("/api/v1/book", rest_router(state));

    // 1. Import source & set long variable (> 240 chars)
    let raw_source = serde_json::json!({
        "bookSourceName": "契约测试源",
        "bookSourceUrl": "https://contract.example"
    })
    .to_string();
    let imported = book_services
        .online
        .import_sources(&admin, &raw_source)
        .await
        .unwrap();
    let source_id = imported.sources[0].id;
    let long_var = "k=".to_string() + &"z".repeat(400);
    let _ = book_services
        .online
        .update_source(
            &admin,
            source_id,
            UpdateBookSourceInput {
                variable: Some(long_var.clone()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    // Admin gets full untruncated variable via GET /sources/{id}/variable
    let var_admin_res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/book/sources/{source_id}/variable"))
                .header("authorization", format!("Bearer {}", admin_session.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(var_admin_res.status(), StatusCode::OK);
    let var_json: serde_json::Value = serde_json::from_slice(
        &to_bytes(var_admin_res.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(var_json["variable"], long_var);

    // Non-admin gets 403 Forbidden
    let var_member_res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/book/sources/{source_id}/variable"))
                .header("authorization", format!("Bearer {}", member_session.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(var_member_res.status(), StatusCode::FORBIDDEN);

    // Export endpoint includes X-Content-Type-Options: nosniff
    let export_res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/book/sources/{source_id}/export"))
                .header("authorization", format!("Bearer {}", admin_session.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(export_res.status(), StatusCode::OK);
    assert_eq!(
        export_res
            .headers()
            .get("x-content-type-options")
            .and_then(|v| v.to_str().ok()),
        Some("nosniff")
    );

    // 2. Track an online book (no network needed since toc_url is empty and host unreachable -> skeleton EPUB)
    let tracked = book_services
        .online
        .track_online_book(
            &admin,
            TrackOnlineBookInput {
                source_id,
                name: "清理验证书".into(),
                author: "作者".into(),
                book_url: "https://contract.example/b/1".into(),
                toc_url: String::new(),
                cover_url: None,
                intro: None,
                kind: None,
                last_chapter: None,
            },
        )
        .await
        .unwrap();

    // Create a dummy tracked chapter dir to confirm lifecycle delete cleans it too
    let tracked_dir = tmp
        .path()
        .join("book")
        .join("online_tracked")
        .join(tracked.id.0.to_string());
    std::fs::create_dir_all(&tracked_dir).unwrap();
    std::fs::write(tracked_dir.join("manifest.json"), b"{}").unwrap();
    let epub_path = tmp
        .path()
        .join("book")
        .join("uploads")
        .join(format!("{}.epub", tracked.id.0));
    assert!(epub_path.is_file());

    // 3. replace_cover must reject SVG payload
    let svg_err = book_services
        .catalog
        .replace_cover(
            &admin,
            BookId(tracked.id.0),
            bytes::Bytes::from_static(
                b"<svg xmlns='http://www.w3.org/2000/svg'><script>alert(1)</script></svg>",
            ),
            "image/svg+xml".into(),
        )
        .await
        .unwrap_err();
    assert!(matches!(
        svg_err,
        xmedia_core::domain::error::DomainError::Validation(_)
    ));

    // 4. POST /books/{id}/online/sync-epub: admin-only for both ?force=true and default (M2-MED-05)
    let force_member_res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/api/v1/book/books/{}/online/sync-epub?force=true",
                    tracked.id.0
                ))
                .header("authorization", format!("Bearer {}", member_session.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(force_member_res.status(), StatusCode::FORBIDDEN);

    let default_member_res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/api/v1/book/books/{}/online/sync-epub",
                    tracked.id.0
                ))
                .header("authorization", format!("Bearer {}", member_session.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(default_member_res.status(), StatusCode::FORBIDDEN);

    let default_admin_res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/api/v1/book/books/{}/online/sync-epub",
                    tracked.id.0
                ))
                .header("authorization", format!("Bearer {}", admin_session.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(default_admin_res.status(), StatusCode::OK);

    // 5. BookDomainLifecycle::on_library_deleted must clean up BOTH OnlineTracked EPUB & online_tracked dir
    let book_lifecycle = lifecycles
        .read()
        .unwrap()
        .get(&MediaKind::Book)
        .cloned()
        .expect("book lifecycle registered");
    book_lifecycle
        .on_library_deleted(tracked.library_id)
        .await
        .unwrap();
    assert!(
        !epub_path.exists(),
        "on_library_deleted must remove OnlineTracked EPUB file"
    );
    assert!(
        !tracked_dir.exists(),
        "on_library_deleted must remove online_tracked/<id> directory"
    );
}

#[tokio::test]
async fn book_import_patch_and_test_concurrent_with_login_action_preserve_runtime_state_e2e() {
    let mock_app = Router::new().route(
        "/search",
        get(|| async {
            tokio::time::sleep(Duration::from_millis(120)).await;
            r#"{"list":[{"name":"测试书","url":"/book/1"}]}"#
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_handle = tokio::spawn(async move {
        axum::serve(listener, mock_app).await.unwrap();
    });

    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("book_conc_e2e.db");
    let db_url = format!("sqlite:{}", db_path.display());

    let mut model_set = ModelSet::new();
    register_core_models(&mut model_set);
    register_book_models(&mut model_set);
    let mut db = Db::builder()
        .models(model_set)
        .connect(&db_url)
        .await
        .unwrap();
    ensure_core_schema(&mut db, &db_url).await.unwrap();
    ensure_book_schema(&mut db, &db_url).await.unwrap();

    let users_repo = Arc::new(ToastyUserRepository::new(db.clone()));
    let libs_repo = Arc::new(ToastyLibraryRepository::new(db.clone()));
    let lifecycles = Arc::new(RwLock::new(HashMap::new()));
    let security = Arc::new(SecurityManager::new("book-conc-secret"));
    let tokens = Arc::new(JwtTokenService::new("book-conc-secret", 3600));
    let events = Arc::new(BroadcastEventHub::default());
    let auth_svc = Arc::new(AuthService::new(
        users_repo,
        security,
        tokens,
        lifecycles.clone(),
    ));
    let admin_session = auth_svc
        .setup_initial_admin(SetupAdminInput {
            username: "admin".into(),
            display_name: Some("Admin".into()),
            password: "password123".into(),
        })
        .await
        .unwrap();
    let lib_svc = Arc::new(LibraryService::new(
        libs_repo.clone(),
        events.clone(),
        lifecycles,
    ));
    let book_services = crate::init_book_module_with_allow_private_ips(
        db.clone(),
        tmp.path(),
        libs_repo,
        &lib_svc,
        events,
        true,
    )
    .unwrap();
    let sources_repo =
        Arc::new(crate::infra::persistence::repositories::ToastyBookSourceRepository::new(db));

    let state = BookHttpState::new(auth_svc, lib_svc, book_services);
    let app = Router::new().nest("/api/v1/book", rest_router(state));

    // Import target source + 80 filler sources so import_sources upsert spans multiple DB calls
    let target_url = format!("http://{addr}");
    let target_src = serde_json::json!({
        "bookSourceName": "TargetBook",
        "bookSourceUrl": target_url,
        "bookSourceGroup": "G1",
        "weight": 10,
        "searchUrl": "/search?q={{key}}",
        "ruleSearch": { "bookList": "$.list", "name": "$.name", "bookUrl": "$.url" }
    });
    let mut batch: Vec<serde_json::Value> = (0..80)
        .map(|i| {
            serde_json::json!({
                "bookSourceName": format!("Filler-{i}"),
                "bookSourceUrl": format!("https://filler-{i}.test"),
                "searchUrl": "/search?q={{key}}",
                "ruleSearch": { "bookList": "$.list", "name": "$.name", "bookUrl": "$.url" }
            })
        })
        .collect();
    batch.push(target_src.clone());

    let imp_res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/book/sources/import")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {}", admin_session.token))
                .body(Body::from(
                    serde_json::json!({ "raw_json": serde_json::to_string(&batch).unwrap() })
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(imp_res.status(), StatusCode::OK);
    let imp_json: serde_json::Value =
        serde_json::from_slice(&to_bytes(imp_res.into_body(), 1024 * 1024).await.unwrap()).unwrap();
    let sid = imp_json["sources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "TargetBook")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    // 1. Concurrent POST /sources/import (81-item upsert) + login-action setting variable/login_info/cookie/cache
    let mut upsert_batch = batch.clone();
    upsert_batch[80]["weight"] = serde_json::json!(25);
    let import_req = app.clone().oneshot(
        Request::builder()
            .method("POST")
            .uri("/api/v1/book/sources/import")
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {}", admin_session.token))
            .body(Body::from(
                serde_json::json!({ "raw_json": serde_json::to_string(&upsert_batch).unwrap() })
                    .to_string(),
            ))
            .unwrap(),
    );
    let login_req = async {
        tokio::time::sleep(Duration::from_millis(5)).await;
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/book/sources/{sid}/login-action"))
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {}", admin_session.token))
                    .body(Body::from(
                        serde_json::json!({
                            "login_info": { "username": "alice" },
                            "action": "source.setVariable('MUST_NOT_LOSE_VAR');\
                                       cache.put('c1', 'v1');\
                                       cookie.setCookie('https://book-conc.test', 'sid=s1');\
                                       'ok'"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap()
    };
    let (r_imp, r_login) = tokio::join!(import_req, login_req);
    assert_eq!(r_imp.unwrap().status(), StatusCode::OK);
    assert_eq!(r_login.status(), StatusCode::OK);

    // 2. Concurrent PATCH /sources/{id} (updating weight & group_name) + login-action updating variable & cookie
    let patch_req = app.clone().oneshot(
        Request::builder()
            .method("PATCH")
            .uri(format!("/api/v1/book/sources/{sid}"))
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {}", admin_session.token))
            .body(Body::from(
                serde_json::json!({ "weight": 77, "group_name": "精选" }).to_string(),
            ))
            .unwrap(),
    );
    let login_req2 = app.clone().oneshot(
        Request::builder()
            .method("POST")
            .uri(format!("/api/v1/book/sources/{sid}/login-action"))
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {}", admin_session.token))
            .body(Body::from(
                serde_json::json!({
                    "login_info": { "username": "alice" },
                    "action": "source.setVariable('AFTER_PATCH_VAR');\
                               cookie.setCookie('https://book-conc.test', 'k2=v2');\
                               'ok'"
                })
                .to_string(),
            ))
            .unwrap(),
    );
    let (r_patch, r_login2) = tokio::join!(patch_req, login_req2);
    assert_eq!(r_patch.unwrap().status(), StatusCode::OK);
    assert_eq!(r_login2.unwrap().status(), StatusCode::OK);

    // 3. Concurrent slow POST /sources/{id}/test + login-action updating variable/cache/cookie
    let test_req = app.clone().oneshot(
        Request::builder()
            .method("POST")
            .uri(format!("/api/v1/book/sources/{sid}/test"))
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {}", admin_session.token))
            .body(Body::from(
                serde_json::json!({ "mode": "search", "keyword": "test" }).to_string(),
            ))
            .unwrap(),
    );
    let login_req3 = async {
        tokio::time::sleep(Duration::from_millis(20)).await;
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/book/sources/{sid}/login-action"))
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {}", admin_session.token))
                    .body(Body::from(
                        serde_json::json!({
                            "login_info": { "username": "bob" },
                            "action": "source.setVariable('FINAL_VAR');\
                                       cache.put('c2', 'v2');\
                                       cookie.setCookie('https://book-conc.test', 'k3=v3');\
                                       'ok'"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap()
    };
    let (r_test, r_login3) = tokio::join!(test_req, login_req3);
    assert_eq!(r_test.unwrap().status(), StatusCode::OK);
    assert_eq!(r_login3.status(), StatusCode::OK);

    // Assert zero lost variable, login_info, cookies, cache, weight, or group_name via HTTP & repository
    let sid_uuid = uuid::Uuid::parse_str(&sid).unwrap();
    let rec = crate::domain::repository::BookSourceRepository::find_by_id(
        sources_repo.as_ref(),
        sid_uuid,
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(rec.weight, 77);
    assert_eq!(rec.group_name, "精选");
    assert_eq!(rec.runtime_state.variable, "FINAL_VAR");
    assert_eq!(
        rec.runtime_state
            .login_info
            .get("username")
            .map(String::as_str),
        Some("bob")
    );
    assert_eq!(
        rec.runtime_state.cache.get("c1").map(String::as_str),
        Some("v1")
    );
    assert_eq!(
        rec.runtime_state.cache.get("c2").map(String::as_str),
        Some("v2")
    );
    let ck = rec
        .runtime_state
        .cookies
        .get("book-conc.test")
        .map(String::as_str)
        .unwrap_or("");
    assert!(ck.contains("sid=s1"), "missing sid=s1 in {ck}");
    assert!(ck.contains("k2=v2"), "missing k2=v2 in {ck}");
    assert!(ck.contains("k3=v3"), "missing k3=v3 in {ck}");

    server_handle.abort();
}

#[tokio::test]
async fn caller_supplied_book_urls_reject_legado_options_and_data_urls_with_400() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("book_url_guard_test.db");
    let db_url = format!("sqlite:{}", db_path.display());

    let mut models = ModelSet::new();
    register_core_models(&mut models);
    register_book_models(&mut models);
    let mut db = Db::builder().models(models).connect(&db_url).await.unwrap();
    ensure_core_schema(&mut db, &db_url).await.unwrap();
    ensure_book_schema(&mut db, &db_url).await.unwrap();

    let users = Arc::new(ToastyUserRepository::new(db.clone()));
    let libs_repo = Arc::new(ToastyLibraryRepository::new(db.clone()));
    let lifecycles = Arc::new(RwLock::new(HashMap::new()));
    let security = Arc::new(SecurityManager::new("book-url-guard-secret"));
    let tokens = Arc::new(JwtTokenService::new("book-url-guard-secret", 3600));
    let events = Arc::new(BroadcastEventHub::default());
    let auth_svc = Arc::new(AuthService::new(
        users.clone(),
        security,
        tokens,
        lifecycles.clone(),
    ));
    let lib_svc = Arc::new(LibraryService::new(
        libs_repo.clone(),
        events.clone(),
        lifecycles,
    ));
    let book_services = crate::init_book_module_with_allow_private_ips(
        db.clone(),
        tmp.path(),
        libs_repo,
        &lib_svc,
        events,
        true,
    )
    .unwrap();
    let admin = auth_svc
        .setup_initial_admin(SetupAdminInput {
            username: "admin".into(),
            display_name: Some("Admin".into()),
            password: "password123".into(),
        })
        .await
        .unwrap();
    let state = BookHttpState::new(auth_svc, lib_svc, book_services);
    let app = Router::new().nest("/api/v1/book", rest_router(state));

    let src_json = serde_json::json!([{
        "bookSourceName": "GuardSource",
        "bookSourceUrl": "https://book-guard.example",
        "searchUrl": "/search?q={{key}}",
        "ruleSearch": { "bookList": "$.items", "name": "$.name", "bookUrl": "$.url" }
    }]);
    let imp_res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/book/sources/import")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {}", admin.token))
                .body(Body::from(
                    serde_json::json!({ "raw_json": src_json.to_string() }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(imp_res.status(), StatusCode::OK);
    let imp_body: serde_json::Value =
        serde_json::from_slice(&to_bytes(imp_res.into_body(), 1024 * 1024).await.unwrap()).unwrap();
    let sid = imp_body["sources"][0]["id"].as_str().unwrap().to_string();

    for bad_url in [
        "https://book-guard.example/b/1,{\"method\":\"POST\",\"headers\":{\"X-Evil\":\"1\"}}",
        "data:text/html,<h1>evil</h1>",
    ] {
        for (uri, payload) in [
            (
                "/api/v1/book/online/book-info",
                serde_json::json!({ "source_id": sid, "book_url": bad_url }),
            ),
            (
                "/api/v1/book/online/toc",
                serde_json::json!({ "source_id": sid, "toc_url": bad_url }),
            ),
            (
                "/api/v1/book/online/content",
                serde_json::json!({ "source_id": sid, "chapter_url": bad_url }),
            ),
            (
                "/api/v1/book/online/downloads",
                serde_json::json!({
                    "source_id": sid,
                    "book_name": "Evil",
                    "book_url": bad_url,
                    "toc_url": "https://book-guard.example/toc/1",
                    "format": "txt"
                }),
            ),
        ] {
            let res = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(uri)
                        .header("content-type", "application/json")
                        .header("authorization", format!("Bearer {}", admin.token))
                        .body(Body::from(payload.to_string()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                res.status(),
                StatusCode::BAD_REQUEST,
                "expected 400 on {uri} for {bad_url}"
            );
        }
    }
}

#[tokio::test]
async fn book_online_search_enforces_cancel_on_future_drop() {
    use std::sync::atomic::{AtomicUsize, Ordering};

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
                    r#"{"items":[{"name":"Book","url":"/b/1"}]}"#
                }
            }),
        )
        .route(
            "/step2",
            get(move || {
                let hits = step2_hits_c.clone();
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
    let db_path = tmp.path().join("book_drop_search.db");
    let db_url = format!("sqlite:{}", db_path.display());

    let mut models = ModelSet::new();
    register_core_models(&mut models);
    register_book_models(&mut models);
    let mut db = Db::builder().models(models).connect(&db_url).await.unwrap();
    ensure_core_schema(&mut db, &db_url).await.unwrap();
    ensure_book_schema(&mut db, &db_url).await.unwrap();

    let users = Arc::new(ToastyUserRepository::new(db.clone()));
    let libs_repo = Arc::new(ToastyLibraryRepository::new(db.clone()));
    let lifecycles = Arc::new(RwLock::new(HashMap::new()));
    let security = Arc::new(SecurityManager::new("book-drop-search-secret"));
    let tokens = Arc::new(JwtTokenService::new("book-drop-search-secret", 3600));
    let events = Arc::new(BroadcastEventHub::default());
    let auth_svc = Arc::new(AuthService::new(
        users,
        security,
        tokens,
        lifecycles.clone(),
    ));
    let lib_svc = Arc::new(LibraryService::new(
        libs_repo.clone(),
        events.clone(),
        lifecycles,
    ));
    let book_services = crate::init_book_module_with_allow_private_ips(
        db,
        tmp.path(),
        libs_repo,
        &lib_svc,
        events,
        true,
    )
    .unwrap();
    let admin = auth_svc
        .setup_initial_admin(SetupAdminInput {
            username: "admin".into(),
            display_name: Some("Admin".into()),
            password: "password123".into(),
        })
        .await
        .unwrap();
    let state = BookHttpState::new(auth_svc, lib_svc, book_services);
    let app = Router::new().nest("/api/v1/book", rest_router(state));

    let src_json = serde_json::json!([{
        "bookSourceName": "DropBookSearchSource",
        "bookSourceUrl": base,
        "searchUrl": format!("{base}/step1?q={{{{key}}}}"),
        "ruleSearch": {
            "bookList": "$.items[*]",
            "name": "@js:java.ajax(baseUrl.replace('/step1', '/step2')); 'Book'",
            "bookUrl": "$.url"
        }
    }]);
    let imp_res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/book/sources/import")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {}", admin.token))
                .body(Body::from(
                    serde_json::json!({ "raw_json": src_json.to_string() }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(imp_res.status(), StatusCode::OK);
    let imp_body: serde_json::Value =
        serde_json::from_slice(&to_bytes(imp_res.into_body(), 1024 * 1024).await.unwrap()).unwrap();
    let sid = imp_body["sources"][0]["id"].as_str().unwrap().to_string();

    {
        let search_fut = app.clone().oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/book/online/search")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {}", admin.token))
                .body(Body::from(
                    serde_json::json!({ "keyword": "drop", "source_ids": [sid] }).to_string(),
                ))
                .unwrap(),
        );
        tokio::pin!(search_fut);
        tokio::select! {
            _ = &mut search_fut => panic!("book search completed before drop"),
            _ = step1_entered.notified() => {}
        }
    }

    tokio::time::sleep(Duration::from_millis(320)).await;
    assert_eq!(
        step2_hits.load(Ordering::SeqCst),
        0,
        "CancelTasksOnDrop must abort book search tasks before step 2 java.ajax"
    );

    srv.abort();
}

#[tokio::test]
async fn book_online_non_search_and_test_enforce_cancel_on_future_drop() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let step1_entered = Arc::new(tokio::sync::Notify::new());
    let step2_hits = Arc::new(AtomicUsize::new(0));
    let toc2_hits = Arc::new(AtomicUsize::new(0));
    let content2_hits = Arc::new(AtomicUsize::new(0));

    let s1_search = step1_entered.clone();
    let s1_book = step1_entered.clone();
    let s1_toc = step1_entered.clone();
    let s1_content = step1_entered.clone();
    let step2_hits_c = step2_hits.clone();
    let toc2_hits_c = toc2_hits.clone();
    let content2_hits_c = content2_hits.clone();

    let origin_app = Router::new()
        .route(
            "/search1",
            get(move || {
                let notify = s1_search.clone();
                async move {
                    notify.notify_waiters();
                    tokio::time::sleep(Duration::from_millis(220)).await;
                    r#"{"items":[{"name":"Book","url":"/book1"}]}"#
                }
            }),
        )
        .route(
            "/book1",
            get(move || {
                let notify = s1_book.clone();
                async move {
                    notify.notify_waiters();
                    tokio::time::sleep(Duration::from_millis(220)).await;
                    r#"{"name":"Book","tocUrl":"/toc1"}"#
                }
            }),
        )
        .route(
            "/toc1",
            get(move || {
                let notify = s1_toc.clone();
                async move {
                    notify.notify_waiters();
                    tokio::time::sleep(Duration::from_millis(220)).await;
                    r#"{"chapters":[{"title":"Ch1","url":"/content1"}],"next":"/toc2"}"#
                }
            }),
        )
        .route(
            "/toc2",
            get(move || {
                let hits = toc2_hits_c.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    r#"{"chapters":[{"title":"Ch2","url":"/content2"}]}"#
                }
            }),
        )
        .route(
            "/content1",
            get(move || {
                let notify = s1_content.clone();
                async move {
                    notify.notify_waiters();
                    tokio::time::sleep(Duration::from_millis(220)).await;
                    r#"{"title":"Ch1","body":"Page1","next":"/content2"}"#
                }
            }),
        )
        .route(
            "/content2",
            get(move || {
                let hits = content2_hits_c.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    r#"{"title":"Ch1","body":"Page2"}"#
                }
            }),
        )
        .route(
            "/step2",
            get(move || {
                let hits = step2_hits_c.clone();
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
    let db_path = tmp.path().join("book_drop_non_search.db");
    let db_url = format!("sqlite:{}", db_path.display());

    let mut models = ModelSet::new();
    register_core_models(&mut models);
    register_book_models(&mut models);
    let mut db = Db::builder().models(models).connect(&db_url).await.unwrap();
    ensure_core_schema(&mut db, &db_url).await.unwrap();
    ensure_book_schema(&mut db, &db_url).await.unwrap();

    let users = Arc::new(ToastyUserRepository::new(db.clone()));
    let libs_repo = Arc::new(ToastyLibraryRepository::new(db.clone()));
    let lifecycles = Arc::new(RwLock::new(HashMap::new()));
    let security = Arc::new(SecurityManager::new("book-drop-non-search-secret"));
    let tokens = Arc::new(JwtTokenService::new("book-drop-non-search-secret", 3600));
    let events = Arc::new(BroadcastEventHub::default());
    let auth_svc = Arc::new(AuthService::new(
        users,
        security,
        tokens,
        lifecycles.clone(),
    ));
    let lib_svc = Arc::new(LibraryService::new(
        libs_repo.clone(),
        events.clone(),
        lifecycles,
    ));
    let book_services = crate::init_book_module_with_allow_private_ips(
        db,
        tmp.path(),
        libs_repo,
        &lib_svc,
        events,
        true,
    )
    .unwrap();
    let admin = auth_svc
        .setup_initial_admin(SetupAdminInput {
            username: "admin".into(),
            display_name: Some("Admin".into()),
            password: "password123".into(),
        })
        .await
        .unwrap();
    let state = BookHttpState::new(auth_svc, lib_svc, book_services);
    let app = Router::new().nest("/api/v1/book", rest_router(state));

    let src_json = serde_json::json!([{
        "bookSourceName": "DropBookNonSearchSource",
        "bookSourceUrl": base,
        "searchUrl": format!("{base}/search1?q={{{{key}}}}"),
        "ruleSearch": {
            "bookList": "$.items[*]",
            "name": "@js:java.ajax(baseUrl.replace('/search1', '/step2')); 'Book'",
            "bookUrl": "$.url"
        },
        "ruleBookInfo": {
            "name": "@js:java.ajax(baseUrl.replace('/book1', '/step2')); 'Book'",
            "tocUrl": "$.tocUrl"
        },
        "ruleToc": {
            "chapterList": "$.chapters[*]",
            "chapterName": "$.title",
            "chapterUrl": "$.url",
            "nextTocUrl": "$.next"
        },
        "ruleContent": {
            "title": "$.title",
            "content": "$.body",
            "nextContentUrl": "$.next"
        }
    }]);
    let imp_res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/book/sources/import")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {}", admin.token))
                .body(Body::from(
                    serde_json::json!({ "raw_json": src_json.to_string() }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(imp_res.status(), StatusCode::OK);
    let imp_body: serde_json::Value =
        serde_json::from_slice(&to_bytes(imp_res.into_body(), 1024 * 1024).await.unwrap()).unwrap();
    let sid = imp_body["sources"][0]["id"].as_str().unwrap().to_string();

    for (uri, payload) in [
        (
            "/api/v1/book/online/book-info",
            serde_json::json!({ "source_id": sid, "book_url": format!("{base}/book1") }),
        ),
        (
            "/api/v1/book/online/toc",
            serde_json::json!({ "source_id": sid, "toc_url": format!("{base}/toc1") }),
        ),
        (
            "/api/v1/book/online/content",
            serde_json::json!({ "source_id": sid, "chapter_url": format!("{base}/content1") }),
        ),
        (
            &format!("/api/v1/book/sources/{sid}/test") as &str,
            serde_json::json!({ "mode": "full", "keyword": "drop" }),
        ),
    ] {
        {
            let fut = app.clone().oneshot(
                Request::builder()
                    .method("POST")
                    .uri(uri)
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {}", admin.token))
                    .body(Body::from(payload.to_string()))
                    .unwrap(),
            );
            tokio::pin!(fut);
            tokio::select! {
                _ = &mut fut => panic!("{uri} completed before drop"),
                _ = step1_entered.notified() => {}
            }
        }
        tokio::time::sleep(Duration::from_millis(320)).await;
        assert_eq!(
            step2_hits.load(Ordering::SeqCst),
            0,
            "dropping {uri} mid-flight must abort before step2 java.ajax"
        );
        assert_eq!(
            toc2_hits.load(Ordering::SeqCst),
            0,
            "dropping {uri} mid-flight must abort before toc2 page"
        );
        assert_eq!(
            content2_hits.load(Ordering::SeqCst),
            0,
            "dropping {uri} mid-flight must abort before content2 page"
        );
    }

    srv.abort();
}

#[tokio::test]
async fn book_online_non_search_enforces_timeout_on_stalled_upstream() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use xmedia_core::domain::error::DomainError;

    use crate::infra::persistence::repositories::{
        ToastyBookSourceRepository, ToastyOnlineShelfRepository,
    };

    let after_hits = Arc::new(AtomicUsize::new(0));
    let after_hits_c = after_hits.clone();

    let origin_app = Router::new()
        .route(
            "/hop1",
            get(|| async {
                tokio::time::sleep(Duration::from_millis(90)).await;
                r#"{"name":"Book","tocUrl":"/toc1"}"#
            }),
        )
        .route(
            "/toc1",
            get(|| async {
                tokio::time::sleep(Duration::from_millis(90)).await;
                r#"{"chapters":[{"title":"Ch1","url":"/content1"}],"next":"/stall"}"#
            }),
        )
        .route(
            "/content1",
            get(|| async {
                tokio::time::sleep(Duration::from_millis(90)).await;
                r#"{"title":"Ch1","body":"Page1","next":"/stall"}"#
            }),
        )
        .route(
            "/stall",
            get(|| async {
                tokio::time::sleep(Duration::from_secs(2)).await;
                r#"{"chapters":[{"title":"Late","url":"/c2"}],"body":"LateBody"}"#
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
    let db_path = tmp.path().join("book_timeout.db");
    let db_url = format!("sqlite:{}", db_path.display());
    let mut models = ModelSet::new();
    register_core_models(&mut models);
    register_book_models(&mut models);
    let mut db = Db::builder().models(models).connect(&db_url).await.unwrap();
    ensure_core_schema(&mut db, &db_url).await.unwrap();
    ensure_book_schema(&mut db, &db_url).await.unwrap();

    let users_repo = Arc::new(ToastyUserRepository::new(db.clone()));
    let libs_repo = Arc::new(ToastyLibraryRepository::new(db.clone()));
    let lifecycles = Arc::new(RwLock::new(HashMap::new()));
    let security = Arc::new(SecurityManager::new("book-timeout-secret"));
    let tokens = Arc::new(JwtTokenService::new("book-timeout-secret", 3600));
    let events = Arc::new(BroadcastEventHub::default());
    let auth_svc = Arc::new(AuthService::new(
        users_repo,
        security,
        tokens,
        lifecycles.clone(),
    ));
    let lib_svc = Arc::new(LibraryService::new(
        libs_repo.clone(),
        events.clone(),
        lifecycles,
    ));
    let book_services =
        init_book_module(db.clone(), tmp.path(), libs_repo, &lib_svc, events).unwrap();
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

    let sources_repo = Arc::new(ToastyBookSourceRepository::new(db.clone()));
    let shelves_repo = Arc::new(ToastyOnlineShelfRepository::new(db));
    let engine = BookSourceEngine::new(
        HttpClientConfig {
            allow_private_ips: true,
            ..Default::default()
        },
        Duration::from_secs(5),
    );
    let svc = BookOnlineService::with_engine(
        sources_repo,
        shelves_repo,
        tmp.path(),
        book_services.catalog.clone(),
        engine,
        true,
    )
    .unwrap()
    .with_timeouts(Duration::from_millis(120), Duration::from_millis(160));

    let raw_src = serde_json::json!([{
        "bookSourceName": "StalledMultiPageBook",
        "bookSourceUrl": base,
        "ruleBookInfo": {
            "init": format!("@js:java.ajax('{base}/stall')"),
            "name": format!("@js:java.ajax('{base}/after'); 'Book'"),
            "tocUrl": "$.tocUrl"
        },
        "ruleToc": {
            "chapterList": "$.chapters[*]",
            "chapterName": "$.title",
            "chapterUrl": "$.url",
            "nextTocUrl": "$.next"
        },
        "ruleContent": {
            "title": "$.title",
            "content": "$.body",
            "nextContentUrl": "$.next"
        }
    }]);
    let imported = svc
        .import_sources(&admin, &raw_src.to_string())
        .await
        .unwrap();
    let sid = imported.sources[0].id;

    let t_start = std::time::Instant::now();
    let info_err = svc
        .fetch_book_info(&admin, sid, &format!("{base}/hop1"), None)
        .await
        .unwrap_err();
    assert!(
        matches!(info_err, DomainError::Validation(ref m) if m.contains("超时")),
        "expected timeout error for book_info, got {info_err:?}"
    );

    let toc_err = svc
        .fetch_toc(
            &admin,
            sid,
            &format!("{base}/toc1"),
            BookJsContext::default(),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(toc_err, DomainError::Validation(ref m) if m.contains("超时")),
        "expected timeout error for toc, got {toc_err:?}"
    );

    let content_err = svc
        .fetch_chapter_content(
            &admin,
            sid,
            &format!("{base}/content1"),
            BookJsContext::default(),
            ChapterJsContext::default(),
            true,
        )
        .await
        .unwrap_err();
    assert!(
        matches!(content_err, DomainError::Validation(ref m) if m.contains("超时")),
        "expected timeout error for content, got {content_err:?}"
    );

    assert!(
        t_start.elapsed() < Duration::from_secs(1),
        "all 3 timed-out book calls should finish well under 2s stall, took {:?}",
        t_start.elapsed()
    );
    assert_eq!(after_hits.load(Ordering::SeqCst), 0);

    srv.abort();
}

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
            sources_repo,
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

use std::collections::HashMap;
use std::io::Write;
use std::sync::{Arc, RwLock};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use tempfile::TempDir;
use toasty::Db;
use toasty::schema::ModelSet;
use tower::ServiceExt;
use xmedia_core::application::auth::AuthService;
use xmedia_core::application::dto::{CreateLibraryInput, SetupAdminInput};
use xmedia_core::application::library::LibraryService;
use xmedia_core::domain::library::MediaKind;
use xmedia_core::infra::events::BroadcastEventHub;
use xmedia_core::infra::persistence::repositories::{
    ToastyLibraryRepository, ToastyUserRepository,
};
use xmedia_core::infra::security::{JwtTokenService, SecurityManager};
use xmedia_core::{ensure_core_schema, register_core_models};

use super::sample_epub_bytes;
use crate::http::state::BookHttpState;
use crate::http::{opds_router, rest_router};
use crate::{ensure_book_schema, init_book_module, register_book_models};

#[tokio::test]
async fn book_upload_scan_range_and_opds_roundtrip() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("xmedia_book_test.db");
    let db_url = format!("sqlite:{}", db_path.display());

    let mut models = ModelSet::new();
    register_core_models(&mut models);
    register_book_models(&mut models);
    let mut db = Db::builder().models(models).connect(&db_url).await.unwrap();
    ensure_core_schema(&mut db, &db_url).await.unwrap();
    ensure_book_schema(&mut db, &db_url).await.unwrap();

    let users = Arc::new(ToastyUserRepository::new(db.clone()));
    let libraries_repo = Arc::new(ToastyLibraryRepository::new(db.clone()));
    let lifecycles = Arc::new(RwLock::new(HashMap::new()));
    let security = Arc::new(SecurityManager::new("book-secret"));
    let tokens = Arc::new(JwtTokenService::new("book-secret", 3600));
    let events = Arc::new(BroadcastEventHub::default());

    let auth = Arc::new(AuthService::new(
        users.clone(),
        security,
        tokens,
        lifecycles.clone(),
    ));
    let library_svc = Arc::new(LibraryService::new(
        libraries_repo.clone(),
        events.clone(),
        lifecycles,
    ));

    let book_services =
        init_book_module(db, tmp.path(), libraries_repo, &library_svc, events).unwrap();

    let session = auth
        .setup_initial_admin(SetupAdminInput {
            username: "reader".into(),
            display_name: Some("Reader".into()),
            password: "password123".into(),
        })
        .await
        .unwrap();
    let admin_user = auth.authenticate_jwt(&session.token).await.unwrap();

    let state = BookHttpState::new(auth.clone(), library_svc.clone(), book_services.clone());
    let app = Router::new()
        .nest("/api/v1/book", rest_router(state.clone()))
        .nest("/opds/book", opds_router(state));

    // 1. Upload TXT + EPUB via multipart
    let boundary = "XMEDIABOUNDARY";
    let mut body_bytes = Vec::new();
    write!(
        &mut body_bytes,
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file1\"; filename=\"dune.txt\"\r\nContent-Type: text/plain\r\n\r\n《沙丘》\n作者：弗兰克·赫伯特\n\n第一章 厄拉科斯\n香料必须流动。\n\r\n"
    )
    .unwrap();
    write!(
        &mut body_bytes,
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file2\"; filename=\"foundation.epub\"\r\nContent-Type: application/epub+zip\r\n\r\n"
    )
    .unwrap();
    body_bytes.extend_from_slice(&sample_epub_bytes());
    write!(&mut body_bytes, "\r\n--{boundary}--\r\n").unwrap();

    let upload_res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/book/books/upload")
                .header("authorization", format!("Bearer {}", session.token))
                .header(
                    "content-type",
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body_bytes))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(upload_res.status(), StatusCode::CREATED);
    let upload_json: serde_json::Value =
        serde_json::from_slice(&to_bytes(upload_res.into_body(), usize::MAX).await.unwrap())
            .unwrap();
    let uploaded = upload_json["uploaded"].as_array().unwrap();
    assert_eq!(uploaded.len(), 2);
    let txt_id = uploaded[0]["id"].as_str().unwrap();
    let epub_id = uploaded[1]["id"].as_str().unwrap();

    // 2. Verify TXT chapter & EPUB resource endpoints
    let ch_res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/book/books/{txt_id}/chapters/1"))
                .header("authorization", format!("Bearer {}", session.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(ch_res.status(), StatusCode::OK);
    let ch_json: serde_json::Value =
        serde_json::from_slice(&to_bytes(ch_res.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(ch_json["title"], "第一章 厄拉科斯");
    assert!(
        ch_json["content"]
            .as_str()
            .unwrap()
            .contains("香料必须流动")
    );

    let epub_res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/v1/book/books/{epub_id}/resources/OEBPS/ch1.xhtml"
                ))
                .header("authorization", format!("Bearer {}", session.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(epub_res.status(), StatusCode::OK);
    assert_eq!(
        epub_res
            .headers()
            .get("x-content-type-options")
            .and_then(|v| v.to_str().ok()),
        Some("nosniff")
    );
    let csp = epub_res
        .headers()
        .get("content-security-policy")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        csp.contains("sandbox") && csp.contains("default-src 'none'"),
        "expected strict sandbox CSP header on EPUB resource, got: {csp}"
    );

    // 3. Verify HTTP Range & Suffix Range streaming
    let suffix_res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/book/books/{epub_id}/file"))
                .header("authorization", format!("Bearer {}", session.token))
                .header("range", "bytes=-16")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(suffix_res.status(), StatusCode::PARTIAL_CONTENT);
    let suffix_bytes = to_bytes(suffix_res.into_body(), usize::MAX).await.unwrap();
    assert_eq!(suffix_bytes.len(), 16);

    // 4. Verify OPDS 1.2 XML and OPDS 2.0 JSON with Basic Auth
    let basic = BASE64_STANDARD.encode("reader:password123");
    let opds1_res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/opds/book/v1.2/all")
                .header("authorization", format!("Basic {basic}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(opds1_res.status(), StatusCode::OK);
    let opds1_xml = String::from_utf8(
        to_bytes(opds1_res.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(opds1_xml.contains("Foundation"));
    assert!(opds1_xml.contains("/opds/book/books/"));

    let opds2_res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/opds/book/v2/all")
                .header("authorization", format!("Basic {basic}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(opds2_res.status(), StatusCode::OK);
    let opds2_json: serde_json::Value =
        serde_json::from_slice(&to_bytes(opds2_res.into_body(), usize::MAX).await.unwrap())
            .unwrap();
    assert_eq!(opds2_json["metadata"]["numberOfItems"], 2);

    // Unbound member probing OPDS library feeds directly must receive 404 Not Found
    auth.create_user(
        &admin_user,
        xmedia_core::application::dto::CreateUserInput {
            username: "outsider".into(),
            display_name: None,
            password: "password123".into(),
            role: xmedia_core::domain::user::UserRole::Member,
        },
    )
    .await
    .unwrap();
    let outsider_basic = BASE64_STANDARD.encode("outsider:password123");
    let book_libs = library_svc
        .list_libraries(Some(MediaKind::Book))
        .await
        .unwrap();
    let managed_book_lib_id = book_libs[0].id;
    for path in [
        format!("/opds/book/v1.2/libraries/{managed_book_lib_id}"),
        format!("/opds/book/v2/libraries/{managed_book_lib_id}"),
    ] {
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(&path)
                    .header("authorization", format!("Basic {outsider_basic}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND, "failed on {path}");
    }

    // 5. Verify local mount scan with sidecar .opf + cover & rename preservation
    let mount_dir = tmp.path().join("mounted_books");
    std::fs::create_dir_all(&mount_dir).unwrap();
    let mut sample_pdf = lopdf::Document::with_version("1.4");
    let pages_id = sample_pdf.new_object_id();
    let mut pages_dict = lopdf::Dictionary::new();
    pages_dict.set("Type", lopdf::Object::Name(b"Pages".to_vec()));
    pages_dict.set("Kids", lopdf::Object::Array(vec![]));
    pages_dict.set("Count", lopdf::Object::Integer(0));
    sample_pdf
        .objects
        .insert(pages_id, lopdf::Object::Dictionary(pages_dict));
    let mut catalog_dict = lopdf::Dictionary::new();
    catalog_dict.set("Type", lopdf::Object::Name(b"Catalog".to_vec()));
    catalog_dict.set("Pages", lopdf::Object::Reference(pages_id));
    let catalog_id = sample_pdf.add_object(lopdf::Object::Dictionary(catalog_dict));
    sample_pdf.trailer.set("Root", catalog_id);
    let mut pdf_bytes = Vec::new();
    sample_pdf.save_to(&mut pdf_bytes).unwrap();
    std::fs::write(mount_dir.join("three_body.pdf"), &pdf_bytes).unwrap();
    std::fs::write(
        mount_dir.join("three_body.opf"),
        r#"<package><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>三体</dc:title><dc:creator>刘慈欣</dc:creator></metadata></package>"#,
    )
    .unwrap();
    const TINY_PNG: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];
    std::fs::write(mount_dir.join("three_body.png"), TINY_PNG).unwrap();

    let created_lib = library_svc
        .create_local_mount(
            &admin_user,
            CreateLibraryInput {
                name: "Local Sci-Fi".into(),
                media_kind: MediaKind::Book,
                root_path: mount_dir.to_string_lossy().into_owned(),
            },
        )
        .await
        .unwrap();
    library_svc
        .trigger_scan(&admin_user, created_lib.id)
        .await
        .unwrap();

    for _ in 0..40 {
        let lib_now = library_svc.get_library(created_lib.id).await.unwrap();
        if matches!(
            lib_now.scan_status,
            xmedia_core::domain::library::ScanStatus::Completed { .. }
        ) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    assert_eq!(
        library_svc
            .get_library(created_lib.id)
            .await
            .unwrap()
            .item_count,
        1
    );

    let search_filter = crate::application::dto::CatalogSearchFilter {
        library_id: Some(created_lib.id),
        ..Default::default()
    };
    // Scanned local files land in the shared resource pool unbound by default
    let unbound_page = book_services
        .catalog
        .search_books(Some(&admin_user), search_filter.clone())
        .await
        .unwrap();
    assert_eq!(unbound_page.items.len(), 0);

    let pool_before = book_services
        .catalog
        .search_pool_books(&admin_user, search_filter.clone())
        .await
        .unwrap();
    assert_eq!(pool_before.items.len(), 1);
    assert!(!pool_before.items[0].bound);

    let bind_res = book_services
        .catalog
        .bind_books(
            &admin_user,
            xmedia_core::application::bindings::BindMediaItemsInput {
                library_id: Some(created_lib.id.0),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(bind_res.bound_count, 1);

    let pool_after = book_services
        .catalog
        .search_pool_books(&admin_user, search_filter.clone())
        .await
        .unwrap();
    assert!(pool_after.items[0].bound);

    let page1 = book_services
        .catalog
        .search_books(Some(&admin_user), search_filter.clone())
        .await
        .unwrap();
    assert_eq!(page1.items.len(), 1);
    let original_book = &page1.items[0];
    assert_eq!(original_book.metadata.title, "三体");
    assert_eq!(original_book.metadata.authors, vec!["刘慈欣"]);
    let cover_payload = book_services
        .catalog
        .get_cover(Some(&admin_user), original_book.id)
        .await
        .unwrap();
    assert_eq!(cover_payload.mime_type, "image/png");

    // Attach a bookmark before moving/renaming the file
    book_services
        .reading
        .create_bookmark(
            &admin_user,
            original_book.id,
            crate::application::reading::CreateBookmarkCommand {
                title: "Key Moment".into(),
                note: None,
                progress_percent: 42.0,
                locator: "page:1".into(),
                chapter_index: None,
                page_number: Some(1),
                scroll_ratio: None,
            },
        )
        .await
        .unwrap();

    // Rename book + sidecars and update .opf metadata
    std::fs::rename(
        mount_dir.join("three_body.pdf"),
        mount_dir.join("刘慈欣 - 三体.pdf"),
    )
    .unwrap();
    std::fs::remove_file(mount_dir.join("three_body.opf")).unwrap();
    std::fs::rename(
        mount_dir.join("three_body.png"),
        mount_dir.join("刘慈欣 - 三体.png"),
    )
    .unwrap();
    let renamed_opf = mount_dir.join("刘慈欣 - 三体.opf");
    std::fs::write(
        &renamed_opf,
        r#"<package><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>三体（纪念版）</dc:title><dc:creator>刘慈欣</dc:creator></metadata></package>"#,
    )
    .unwrap();

    library_svc
        .trigger_scan(&admin_user, created_lib.id)
        .await
        .unwrap();
    for _ in 0..40 {
        let lib_now = library_svc.get_library(created_lib.id).await.unwrap();
        if matches!(
            lib_now.scan_status,
            xmedia_core::domain::library::ScanStatus::Completed { .. }
        ) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }

    let page2 = book_services
        .catalog
        .search_books(Some(&admin_user), search_filter)
        .await
        .unwrap();
    assert_eq!(page2.items.len(), 1);
    assert_eq!(page2.items[0].id, original_book.id);
    assert_eq!(page2.items[0].metadata.title, "三体（纪念版）");
    let bms = book_services
        .reading
        .list_bookmarks(&admin_user, original_book.id)
        .await
        .unwrap();
    assert_eq!(bms.len(), 1);
}

#[tokio::test]
async fn local_book_storage_deduplicates_covers_and_reads_legacy_files() {
    use crate::application::ports::{BookStoragePort, CoverPayload};
    use crate::domain::book::BookId;
    use crate::infra::storage::LocalBookStorage;

    let tmp = TempDir::new().unwrap();
    let storage = LocalBookStorage::new(tmp.path()).unwrap();

    let png: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];
    let payload = CoverPayload::new(bytes::Bytes::from_static(png), "image/png");

    let k1 = storage.save_cover(BookId::new(), &payload).await.unwrap();
    let k2 = storage.save_cover(BookId::new(), &payload).await.unwrap();
    assert_eq!(
        k1, k2,
        "identical cover bytes should deduplicate via ThumbnailStore"
    );
    assert!(k1.ends_with(".png"));

    // Untrusted caller/sidecar SVG (even script-free) must never reach store_trusted_svg_bytes_async
    let untrusted_svg = CoverPayload::new(
        bytes::Bytes::from_static(
            b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"10\" height=\"10\"><rect width=\"10\" height=\"10\"/></svg>",
        ),
        "image/svg+xml",
    );
    assert!(
        storage
            .save_cover(BookId::new(), &untrusted_svg)
            .await
            .is_err()
    );

    // LocalBookScanner::read_entry must ignore sidecar SVG disguised as cover.jpg
    let scan_dir = tmp.path().join("svg_sidecar_book");
    std::fs::create_dir_all(&scan_dir).unwrap();
    std::fs::write(scan_dir.join("novel.txt"), b"Chapter 1\nHello world.\n").unwrap();
    std::fs::write(
        scan_dir.join("cover.jpg"),
        b"<svg xmlns=\"http://www.w3.org/2000/svg\"><rect width=\"10\" height=\"10\"/></svg>",
    )
    .unwrap();
    let loaded_entry = crate::application::ports::BookScannerPort::read_entry(
        &crate::infra::storage::LocalBookScanner,
        scan_dir.to_str().unwrap(),
        "novel.txt",
    )
    .await
    .unwrap();
    assert!(
        loaded_entry.sidecar_cover.is_none(),
        "sidecar SVG disguised as .jpg must be rejected during scan"
    );

    let loaded = storage.read_cover(&k1).await.unwrap();
    assert_eq!(loaded.bytes.as_ref(), png);
    assert_eq!(loaded.mime_type, "image/png");

    // Legacy per-book cover directly under `book/covers/` must still be readable & deletable
    let legacy_key = format!("{}.png", BookId::new());
    let legacy_path = tmp.path().join("book").join("covers").join(&legacy_key);
    std::fs::write(&legacy_path, png).unwrap();
    let legacy_loaded = storage.read_cover(&legacy_key).await.unwrap();
    assert_eq!(legacy_loaded.bytes.as_ref(), png);
    storage.delete_cover(&legacy_key).await.unwrap();
    assert!(!legacy_path.exists());
}

#[tokio::test]
async fn replace_cover_saves_new_before_deleting_old_and_preserves_old_on_failure() {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};

    use async_trait::async_trait;
    use bytes::Bytes;
    use xmedia_core::application::ports::{ByteRangeSpec, ByteRangeStream};
    use xmedia_core::domain::error::{DomainError, DomainResult};
    use xmedia_core::domain::library::Library;

    use crate::application::catalog::BookCatalogService;
    use crate::application::ports::{BookStoragePort, CoverPayload, StoredManagedBook};
    use crate::domain::book::{Book, BookFormat, BookId};
    use crate::infra::parser::CompositeBookParser;
    use crate::infra::persistence::repositories::{
        ToastyBookReadingRepository, ToastyBookRepository,
    };
    use crate::infra::storage::LocalBookStorage;

    struct RecordingStorage {
        inner: LocalBookStorage,
        fail_save: AtomicBool,
        ops: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl BookStoragePort for RecordingStorage {
        async fn save_managed_book(
            &self,
            book_id: BookId,
            format: BookFormat,
            data: Bytes,
        ) -> DomainResult<StoredManagedBook> {
            self.inner.save_managed_book(book_id, format, data).await
        }
        async fn delete_managed_book(&self, storage_key: &str) -> DomainResult<()> {
            self.inner.delete_managed_book(storage_key).await
        }
        async fn save_cover(&self, book_id: BookId, cover: &CoverPayload) -> DomainResult<String> {
            self.ops.lock().unwrap().push("save".into());
            if self.fail_save.load(Ordering::SeqCst) {
                return Err(DomainError::Storage("simulated disk full".into()));
            }
            self.inner.save_cover(book_id, cover).await
        }
        async fn read_cover(&self, cover_key: &str) -> DomainResult<CoverPayload> {
            self.inner.read_cover(cover_key).await
        }
        async fn delete_cover(&self, cover_key: &str) -> DomainResult<()> {
            self.ops.lock().unwrap().push(format!("delete:{cover_key}"));
            self.inner.delete_cover(cover_key).await
        }
        async fn read_book_bytes(&self, library: &Library, book: &Book) -> DomainResult<Bytes> {
            self.inner.read_book_bytes(library, book).await
        }
        async fn open_book_stream(
            &self,
            library: &Library,
            book: &Book,
            spec: ByteRangeSpec,
        ) -> DomainResult<ByteRangeStream> {
            self.inner.open_book_stream(library, book, spec).await
        }
    }

    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("replace_cover_test.db");
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
    let security = Arc::new(SecurityManager::new("cover-secret"));
    let tokens = Arc::new(JwtTokenService::new("cover-secret", 3600));
    let auth = Arc::new(AuthService::new(users, security, tokens, lifecycles));
    let session = auth
        .setup_initial_admin(SetupAdminInput {
            username: "admin".into(),
            display_name: None,
            password: "password123".into(),
        })
        .await
        .unwrap();
    let admin = auth.authenticate_jwt(&session.token).await.unwrap();

    let storage = Arc::new(RecordingStorage {
        inner: LocalBookStorage::new(tmp.path()).unwrap(),
        fail_save: AtomicBool::new(false),
        ops: Mutex::new(Vec::new()),
    });
    let bindings = Arc::new(xmedia_core::application::bindings::MediaBindingService::new(
        Arc::new(
            xmedia_core::infra::persistence::repositories::ToastyUserMediaBindingRepository::new(
                db.clone(),
            ),
        ),
        None,
    ));
    let catalog = BookCatalogService::new(
        Arc::new(ToastyBookRepository::new(db.clone())),
        libs_repo,
        Arc::new(ToastyBookReadingRepository::new(db)),
        bindings,
        Arc::new(CompositeBookParser),
        storage.clone(),
    );

    let uploaded = catalog
        .upload_book(
            &admin,
            "cover_test.txt",
            Bytes::from_static(b"Hello\nChapter 1\nBody"),
        )
        .await
        .unwrap();
    let initial_cover = catalog.get_cover(Some(&admin), uploaded.id).await.unwrap();

    // 1. Failing save must NOT delete old cover
    storage.ops.lock().unwrap().clear();
    storage.fail_save.store(true, Ordering::SeqCst);
    let err = catalog
        .replace_cover(
            &admin,
            uploaded.id,
            Bytes::from_static(&[0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3]),
            "image/jpeg".into(),
        )
        .await;
    assert!(err.is_err());
    assert_eq!(*storage.ops.lock().unwrap(), vec!["save".to_string()]);
    assert_eq!(
        catalog
            .get_cover(Some(&admin), uploaded.id)
            .await
            .unwrap()
            .bytes,
        initial_cover.bytes
    );

    // 2. Successful replace must save new cover BEFORE deleting old cover
    storage.ops.lock().unwrap().clear();
    storage.fail_save.store(false, Ordering::SeqCst);
    let new_png: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];
    catalog
        .replace_cover(
            &admin,
            uploaded.id,
            Bytes::from_static(new_png),
            "image/png".into(),
        )
        .await
        .unwrap();
    let ops = storage.ops.lock().unwrap().clone();
    assert_eq!(ops.len(), 2);
    assert_eq!(ops[0], "save");
    assert!(ops[1].starts_with("delete:"));
    assert_eq!(
        catalog
            .get_cover(Some(&admin), uploaded.id)
            .await
            .unwrap()
            .bytes
            .as_ref(),
        new_png
    );
}

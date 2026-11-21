pub mod application;
pub mod domain;
pub mod http;
pub mod infra;
pub mod source;

use std::path::Path;
use std::sync::Arc;

use toasty::Db;
use xmedia_core::application::library::LibraryService;
use xmedia_core::application::ports::RealtimeEventPort;
use xmedia_core::domain::error::DomainResult;
use xmedia_core::domain::repository::LibraryRepository;

use application::BookServices;
use application::catalog::BookCatalogService;
use application::lifecycle::BookDomainLifecycle;
use application::online::BookOnlineService;
use application::reading::BookReadingService;
use infra::parser::CompositeBookParser;
use infra::persistence::repositories::{
    ToastyBookReadingRepository, ToastyBookRepository, ToastyBookSourceRepository,
    ToastyOnlineShelfRepository,
};
use infra::storage::{LocalBookScanner, LocalBookStorage};

pub use infra::persistence::{ensure_book_schema, register_book_models};

pub fn init_book_module(
    db: Db,
    data_dir: &Path,
    libraries_repo: Arc<dyn LibraryRepository>,
    library_service: &LibraryService,
    events: Arc<dyn RealtimeEventPort>,
) -> DomainResult<BookServices> {
    let books = Arc::new(ToastyBookRepository::new(db.clone()));
    let readings = Arc::new(ToastyBookReadingRepository::new(db.clone()));
    let sources = Arc::new(ToastyBookSourceRepository::new(db.clone()));
    let online_shelves = Arc::new(ToastyOnlineShelfRepository::new(db));
    let parser = Arc::new(CompositeBookParser);
    let storage = Arc::new(LocalBookStorage::new(data_dir)?);
    let scanner = Arc::new(LocalBookScanner);

    let lifecycle = Arc::new(BookDomainLifecycle::new(
        libraries_repo.clone(),
        books.clone(),
        readings.clone(),
        online_shelves.clone(),
        scanner,
        parser.clone(),
        storage.clone(),
        events,
    ));
    library_service.register_lifecycle(lifecycle);

    let catalog = Arc::new(BookCatalogService::new(
        books.clone(),
        libraries_repo,
        readings.clone(),
        parser,
        storage,
    ));
    let reading = Arc::new(BookReadingService::new(readings, books));
    let online = Arc::new(BookOnlineService::new(
        sources,
        online_shelves,
        data_dir,
        catalog.clone(),
    )?);

    Ok(BookServices::new(catalog, reading, online))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::io::{Cursor, Write};
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
    use zip::ZipWriter;
    use zip::write::SimpleFileOptions;

    use crate::http::state::BookHttpState;
    use crate::http::{opds_router, rest_router};
    use crate::{ensure_book_schema, init_book_module, register_book_models};

    fn sample_epub_bytes() -> Vec<u8> {
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut zw = ZipWriter::new(&mut cursor);
            let opts = SimpleFileOptions::default();

            zw.start_file("META-INF/container.xml", opts).unwrap();
            zw.write_all(
                br#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#,
            )
            .unwrap();

            zw.start_file("OEBPS/content.opf", opts).unwrap();
            zw.write_all(
                br#"<?xml version="1.0" encoding="UTF-8"?>
<package version="3.0" xmlns="http://www.idpf.org/2007/opf" unique-identifier="bookid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Foundation</dc:title>
    <dc:creator>Isaac Asimov</dc:creator>
    <dc:language>en</dc:language>
    <dc:subject>Sci-Fi</dc:subject>
  </metadata>
  <manifest>
    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    <item id="ch1" href="ch1.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine>
    <itemref idref="ch1"/>
  </spine>
</package>"#,
            )
            .unwrap();

            zw.start_file("OEBPS/nav.xhtml", opts).unwrap();
            zw.write_all(
                br#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<body>
  <nav epub:type="toc">
    <ol>
      <li><a href="ch1.xhtml">The Psychohistorians</a></li>
    </ol>
  </nav>
</body>
</html>"#,
            )
            .unwrap();

            zw.start_file("OEBPS/ch1.xhtml", opts).unwrap();
            zw.write_all(br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><p>Hari Seldon</p></body></html>"#)
                .unwrap();

            zw.finish().unwrap();
        }
        cursor.into_inner()
    }

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
            serde_json::from_slice(&to_bytes(ch_res.into_body(), usize::MAX).await.unwrap())
                .unwrap();
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
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9C, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00,
            0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
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
            .get_cover(original_book.id)
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
    async fn online_book_source_shelf_cache_download_epub_and_import_roundtrip() {
        use std::time::Duration;

        use axum::routing::get;

        use crate::application::online::{
            AddOnlineShelfInput, BookOnlineService, DownloadJobStatus, OnlineDownloadFormat,
            StartDownloadInput, UpdateOnlineShelfInput,
        };
        use crate::source::BookSourceEngine;
        use crate::source::http::HttpClientConfig;
        use crate::source::model::{BookJsContext, ChapterJsContext};

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
    async fn local_book_storage_deduplicates_covers_and_reads_legacy_files() {
        use crate::application::ports::{BookStoragePort, CoverPayload};
        use crate::domain::book::BookId;
        use crate::infra::storage::LocalBookStorage;

        let tmp = TempDir::new().unwrap();
        let storage = LocalBookStorage::new(tmp.path()).unwrap();

        let png: &[u8] = &[
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9C, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00,
            0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ];
        let payload = CoverPayload {
            bytes: bytes::Bytes::from_static(png),
            mime_type: "image/png".to_string(),
        };

        let k1 = storage.save_cover(BookId::new(), &payload).await.unwrap();
        let k2 = storage.save_cover(BookId::new(), &payload).await.unwrap();
        assert_eq!(
            k1, k2,
            "identical cover bytes should deduplicate via ThumbnailStore"
        );
        assert!(k1.ends_with(".png"));

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
            async fn save_cover(
                &self,
                book_id: BookId,
                cover: &CoverPayload,
            ) -> DomainResult<String> {
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
        let catalog = BookCatalogService::new(
            Arc::new(ToastyBookRepository::new(db.clone())),
            libs_repo,
            Arc::new(ToastyBookReadingRepository::new(db)),
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
        let initial_cover = catalog.get_cover(uploaded.id).await.unwrap();

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
            catalog.get_cover(uploaded.id).await.unwrap().bytes,
            initial_cover.bytes
        );

        // 2. Successful replace must save new cover BEFORE deleting old cover
        storage.ops.lock().unwrap().clear();
        storage.fail_save.store(false, Ordering::SeqCst);
        let new_png: &[u8] = &[
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9C, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00,
            0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
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
            catalog.get_cover(uploaded.id).await.unwrap().bytes.as_ref(),
            new_png
        );
    }

    #[tokio::test]
    async fn deleting_user_cleans_up_online_shelf_entries() {
        use xmedia_core::application::dto::CreateUserInput;
        use xmedia_core::domain::user::UserRole;

        use crate::application::online::AddOnlineShelfInput;

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
        use xmedia_core::application::dto::CreateUserInput;
        use xmedia_core::domain::user::UserRole;

        use crate::application::online::{TrackOnlineBookInput, UpdateBookSourceInput};
        use crate::domain::book::BookId;

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
}

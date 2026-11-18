pub mod application;
pub mod domain;
pub mod http;
pub mod infra;

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
use application::reading::BookReadingService;
use infra::parser::CompositeBookParser;
use infra::persistence::repositories::{ToastyBookReadingRepository, ToastyBookRepository};
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
    let readings = Arc::new(ToastyBookReadingRepository::new(db));
    let parser = Arc::new(CompositeBookParser);
    let storage = Arc::new(LocalBookStorage::new(data_dir)?);
    let scanner = Arc::new(LocalBookScanner);

    let lifecycle = Arc::new(BookDomainLifecycle::new(
        libraries_repo.clone(),
        books.clone(),
        readings.clone(),
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

    Ok(BookServices::new(catalog, reading))
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
            users,
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
        std::fs::write(
            mount_dir.join("three_body.pdf"),
            b"%PDF-1.4\n1 0 obj << /Type /Catalog >> endobj\n%%EOF",
        )
        .unwrap();
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
}

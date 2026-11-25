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

const TINY_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

fn sample_fb2_bytes() -> Vec<u8> {
    let cover_b64 = BASE64_STANDARD.encode(TINY_PNG);
    format!(
        r##"<?xml version="1.0" encoding="UTF-8"?>
<FictionBook xmlns="http://www.gribuser.ru/xml/fictionbook/2.0" xmlns:l="http://www.w3.org/1999/xlink">
  <description>
    <title-info>
      <genre>sci_fi</genre>
      <author>
        <first-name>Stanislaw</first-name>
        <last-name>Lem</last-name>
      </author>
      <book-title>Solaris</book-title>
      <annotation><p>An sentient ocean planet.</p></annotation>
      <lang>en</lang>
      <sequence name="Solaris Cycle" number="2.5"/>
      <coverpage><image l:href="#cover.png"/></coverpage>
    </title-info>
    <publish-info>
      <publisher>MON</publisher>
      <year>1961</year>
    </publish-info>
  </description>
  <body>
    <section>
      <title><p>Chapter 1: The Arrival</p></title>
      <p>Kris Kelvin arrives aboard the Prometheus.</p>
      <p>The station seems eerily quiet.</p>
    </section>
    <section>
      <title><p>Chapter 2: The Visitors</p></title>
      <p>Strange phenomena manifest on the station.</p>
    </section>
  </body>
  <binary id="cover.png" content-type="image/png">{cover_b64}</binary>
</FictionBook>"##
    )
    .into_bytes()
}

fn push_exth_str(buf: &mut Vec<u8>, rec_type: u32, text: &str) {
    let payload = text.as_bytes();
    let rec_len = (8 + payload.len()) as u32;
    buf.extend_from_slice(&rec_type.to_be_bytes());
    buf.extend_from_slice(&rec_len.to_be_bytes());
    buf.extend_from_slice(payload);
}

fn build_synthetic_mobi_bytes(title: &str, author: &str, ch1: &str, ch2: &str) -> Vec<u8> {
    let html = format!(
        "<html><body><h1>Chapter One</h1><p>{ch1}</p><mbp:pagebreak/><h1>Chapter Two</h1><p>{ch2}</p></body></html>"
    );
    let text_record = html.as_bytes();

    // Build EXTH payload
    let mut exth_records = Vec::new();
    push_exth_str(&mut exth_records, 100, author);
    push_exth_str(&mut exth_records, 101, "Orbit Press");
    push_exth_str(&mut exth_records, 103, "Synthetic MOBI/AZW3 test book");
    // EXTH 201 = cover offset 0 relative to first_image_index (record 2)
    exth_records.extend_from_slice(&201u32.to_be_bytes());
    exth_records.extend_from_slice(&12u32.to_be_bytes());
    exth_records.extend_from_slice(&0u32.to_be_bytes());

    let exth_raw_len = 12 + exth_records.len();
    let exth_padded_len = (exth_raw_len + 3) & !3;
    let mut exth_block = Vec::with_capacity(exth_padded_len);
    exth_block.extend_from_slice(b"EXTH");
    exth_block.extend_from_slice(&(exth_raw_len as u32).to_be_bytes());
    exth_block.extend_from_slice(&4u32.to_be_bytes());
    exth_block.extend_from_slice(&exth_records);
    exth_block.resize(exth_padded_len, 0);

    // Record 0: 16-byte PalmDOC header + 232-byte MOBI header (total 248 bytes) + EXTH + Full Name
    let mobi_header_len: u32 = 232;
    let full_name_offset = (16 + mobi_header_len as usize + exth_block.len()) as u32;
    let full_name_bytes = title.as_bytes();
    let full_name_len = full_name_bytes.len() as u32;

    let mut rec0 = vec![0u8; 16 + mobi_header_len as usize];
    rec0[0..2].copy_from_slice(&1u16.to_be_bytes()); // compression = 1 (uncompressed)
    rec0[4..8].copy_from_slice(&(text_record.len() as u32).to_be_bytes());
    rec0[8..10].copy_from_slice(&1u16.to_be_bytes()); // 1 text record
    rec0[10..12].copy_from_slice(&4096u16.to_be_bytes());
    rec0[16..20].copy_from_slice(b"MOBI");
    rec0[20..24].copy_from_slice(&mobi_header_len.to_be_bytes());
    rec0[24..28].copy_from_slice(&2u32.to_be_bytes()); // mobi_type = 2 (book)
    rec0[28..32].copy_from_slice(&65001u32.to_be_bytes()); // UTF-8
    rec0[84..88].copy_from_slice(&full_name_offset.to_be_bytes());
    rec0[88..92].copy_from_slice(&full_name_len.to_be_bytes());
    rec0[108..112].copy_from_slice(&2u32.to_be_bytes()); // first_image_index = 2
    rec0[128..132].copy_from_slice(&0x40u32.to_be_bytes()); // exth_flags = has EXTH
    rec0.extend_from_slice(&exth_block);
    rec0.extend_from_slice(full_name_bytes);

    let records: [&[u8]; 3] = [&rec0, text_record, TINY_PNG];
    let num_records = records.len() as u16;
    let header_size = 78 + (records.len() * 8) + 2;

    let mut out = vec![0u8; header_size];
    let pdb_name = title.as_bytes();
    let copy_len = pdb_name.len().min(31);
    out[..copy_len].copy_from_slice(&pdb_name[..copy_len]);
    out[60..68].copy_from_slice(b"BOOKMOBI");
    out[76..78].copy_from_slice(&num_records.to_be_bytes());

    let mut cur_offset = header_size;
    for (i, rec) in records.iter().enumerate() {
        let entry_pos = 78 + i * 8;
        out[entry_pos..entry_pos + 4].copy_from_slice(&(cur_offset as u32).to_be_bytes());
        out[entry_pos + 7] = i as u8;
        cur_offset += rec.len();
    }
    for rec in records {
        out.extend_from_slice(rec);
    }
    out
}

async fn setup_test_book_app() -> (
    TempDir,
    String,
    xmedia_core::domain::user::User,
    Arc<LibraryService>,
    Router,
) {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("formats_meta_test.db");
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
    let security = Arc::new(SecurityManager::new("book-fmt-secret"));
    let tokens = Arc::new(JwtTokenService::new("book-fmt-secret", 3600));
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
            username: "admin".into(),
            display_name: Some("Admin".into()),
            password: "password123".into(),
        })
        .await
        .unwrap();
    let admin_user = auth.authenticate_jwt(&session.token).await.unwrap();

    let state = BookHttpState::new(auth, library_svc.clone(), book_services);
    let app = Router::new()
        .nest("/api/v1/book", rest_router(state.clone()))
        .nest("/opds/book", opds_router(state));

    (tmp, session.token, admin_user, library_svc, app)
}

#[tokio::test]
async fn mobi_azw3_fb2_upload_scan_toc_and_chapters_http_roundtrip() {
    let (tmp, token, admin_user, library_svc, app) = setup_test_book_app().await;

    let fb2_bytes = sample_fb2_bytes();
    let mobi_bytes = build_synthetic_mobi_bytes(
        "Hyperion",
        "Dan Simmons",
        "The Hegemony Consul sat on the balcony.",
        "Seven pilgrims boarded the treeship Yggdrasill.",
    );
    let azw3_bytes = build_synthetic_mobi_bytes(
        "Neuromancer",
        "William Gibson",
        "The sky above the port was the color of television.",
        "Case walked through Chiba City.",
    );

    // 1. Multipart upload of FB2 + MOBI + AZW3 + EPUB
    let boundary = "FMTBOUNDARY";
    let mut body = Vec::new();
    for (idx, (fname, mime, payload)) in [
        ("solaris.fb2", "application/x-fictionbook+xml", &fb2_bytes),
        (
            "hyperion.mobi",
            "application/x-mobipocket-ebook",
            &mobi_bytes,
        ),
        (
            "neuromancer.azw3",
            "application/vnd.amazon.ebook",
            &azw3_bytes,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        write!(
            &mut body,
            "--{boundary}\r\nContent-Disposition: form-data; name=\"f{idx}\"; filename=\"{fname}\"\r\nContent-Type: {mime}\r\n\r\n"
        )
        .unwrap();
        body.extend_from_slice(payload);
        write!(&mut body, "\r\n").unwrap();
    }
    let epub_bytes = sample_epub_bytes();
    write!(
        &mut body,
        "--{boundary}\r\nContent-Disposition: form-data; name=\"f3\"; filename=\"foundation.epub\"\r\nContent-Type: application/epub+zip\r\n\r\n"
    )
    .unwrap();
    body.extend_from_slice(&epub_bytes);
    write!(&mut body, "\r\n--{boundary}--\r\n").unwrap();

    let upload_res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/book/books/upload")
                .header("authorization", format!("Bearer {token}"))
                .header(
                    "content-type",
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(upload_res.status(), StatusCode::CREATED);
    let upload_json: serde_json::Value =
        serde_json::from_slice(&to_bytes(upload_res.into_body(), usize::MAX).await.unwrap())
            .unwrap();
    let uploaded = upload_json["uploaded"].as_array().unwrap();
    assert_eq!(uploaded.len(), 4);

    let fb2_dto = &uploaded[0];
    let mobi_dto = &uploaded[1];
    let azw3_dto = &uploaded[2];
    let epub_dto = &uploaded[3];

    assert_eq!(fb2_dto["format"], "fb2");
    assert_eq!(fb2_dto["metadata"]["title"], "Solaris");
    assert_eq!(fb2_dto["series"], "Solaris Cycle");
    assert_eq!(fb2_dto["series_index"], 2.5);
    assert_eq!(fb2_dto["has_cover"], true);

    assert_eq!(mobi_dto["format"], "mobi");
    assert_eq!(mobi_dto["metadata"]["title"], "Hyperion");
    assert_eq!(mobi_dto["metadata"]["authors"][0], "Dan Simmons");
    assert_eq!(mobi_dto["has_cover"], true);

    assert_eq!(azw3_dto["format"], "azw3");
    assert_eq!(azw3_dto["metadata"]["title"], "Neuromancer");
    assert_eq!(azw3_dto["metadata"]["authors"][0], "William Gibson");
    assert_eq!(azw3_dto["has_cover"], true);

    // 2. Verify TOC, Cover, and `/books/{id}/chapters/{index}` on FB2, MOBI, and AZW3
    for (id, expected_ch0, expected_ch1) in [
        (
            fb2_dto["id"].as_str().unwrap(),
            "Kris Kelvin arrives aboard the Prometheus.",
            "Strange phenomena manifest on the station.",
        ),
        (
            mobi_dto["id"].as_str().unwrap(),
            "The Hegemony Consul sat on the balcony.",
            "Seven pilgrims boarded the treeship Yggdrasill.",
        ),
        (
            azw3_dto["id"].as_str().unwrap(),
            "The sky above the port was the color of television.",
            "Case walked through Chiba City.",
        ),
    ] {
        let cover_res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/book/books/{id}/cover"))
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(cover_res.status(), StatusCode::OK);
        assert_eq!(
            cover_res
                .headers()
                .get("content-type")
                .unwrap()
                .to_str()
                .unwrap(),
            "image/png"
        );

        let toc_res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/book/books/{id}/toc"))
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(toc_res.status(), StatusCode::OK);
        let toc: serde_json::Value =
            serde_json::from_slice(&to_bytes(toc_res.into_body(), usize::MAX).await.unwrap())
                .unwrap();
        assert_eq!(toc.as_array().unwrap().len(), 2);

        for (idx, snippet) in [(0, expected_ch0), (1, expected_ch1)] {
            let ch_res = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(format!("/api/v1/book/books/{id}/chapters/{idx}"))
                        .header("authorization", format!("Bearer {token}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(ch_res.status(), StatusCode::OK);
            let ch_json: serde_json::Value =
                serde_json::from_slice(&to_bytes(ch_res.into_body(), usize::MAX).await.unwrap())
                    .unwrap();
            assert_eq!(ch_json["total_chapters"], 2);
            assert!(
                ch_json["content"].as_str().unwrap().contains(snippet),
                "expected snippet '{snippet}' in {ch_json:?}"
            );
        }

        // Out-of-range chapter -> 404
        let oob_res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/book/books/{id}/chapters/99"))
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(oob_res.status(), StatusCode::NOT_FOUND);
    }

    // EPUB must reject `/chapters/0` with 400 Bad Request
    let epub_id = epub_dto["id"].as_str().unwrap();
    let epub_ch_res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/book/books/{epub_id}/chapters/0"))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(epub_ch_res.status(), StatusCode::BAD_REQUEST);

    // 3. LocalMount library scan discovers `.prc` -> `mobi` and `.kf8` -> `azw3`
    let mount_dir = tmp.path().join("mounted_ebooks");
    std::fs::create_dir_all(&mount_dir).unwrap();
    std::fs::write(mount_dir.join("classic.prc"), &mobi_bytes).unwrap();
    std::fs::write(mount_dir.join("modern.kf8"), &azw3_bytes).unwrap();

    let lib = library_svc
        .create_local_mount(
            &admin_user,
            CreateLibraryInput {
                name: "Kindle Shelf".into(),
                media_kind: MediaKind::Book,
                root_path: mount_dir.to_string_lossy().into_owned(),
            },
        )
        .await
        .unwrap();
    library_svc.trigger_scan(&admin_user, lib.id).await.unwrap();
    for _ in 0..40 {
        let lib_now = library_svc.get_library(lib.id).await.unwrap();
        if matches!(
            lib_now.scan_status,
            xmedia_core::domain::library::ScanStatus::Completed { .. }
        ) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }

    let pool_res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/book/pool?library_id={}", lib.id))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(pool_res.status(), StatusCode::OK);
    let pool_json: serde_json::Value =
        serde_json::from_slice(&to_bytes(pool_res.into_body(), usize::MAX).await.unwrap()).unwrap();
    let pool_items = pool_json["items"].as_array().unwrap();
    assert_eq!(pool_items.len(), 2);
    let formats: Vec<&str> = pool_items
        .iter()
        .map(|b| b["format"].as_str().unwrap())
        .collect();
    assert!(formats.contains(&"mobi") && formats.contains(&"azw3"));
}

#[tokio::test]
async fn series_and_rating_roundtrip_across_rest_pool_opds_v1_opds_v2_and_schema_migration() {
    let (_tmp, token, _admin_user, _library_svc, app) = setup_test_book_app().await;

    let boundary = "METABOUNDARY";
    let mut body = Vec::new();
    write!(
        &mut body,
        "--{boundary}\r\nContent-Disposition: form-data; name=\"f\"; filename=\"foundation.epub\"\r\nContent-Type: application/epub+zip\r\n\r\n"
    )
    .unwrap();
    body.extend_from_slice(&sample_epub_bytes());
    write!(&mut body, "\r\n--{boundary}--\r\n").unwrap();

    let up_res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/book/books/upload")
                .header("authorization", format!("Bearer {token}"))
                .header(
                    "content-type",
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(up_res.status(), StatusCode::CREATED);
    let up_json: serde_json::Value =
        serde_json::from_slice(&to_bytes(up_res.into_body(), usize::MAX).await.unwrap()).unwrap();
    let book_id = up_json["uploaded"][0]["id"].as_str().unwrap().to_string();

    // Validation: negative series_index and rating > 10.0 must be rejected with 400
    for bad_patch in [
        serde_json::json!({ "series_index": -0.5 }),
        serde_json::json!({ "rating": 10.5 }),
    ] {
        let bad_res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/api/v1/book/books/{book_id}"))
                    .header("authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(bad_patch.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(bad_res.status(), StatusCode::BAD_REQUEST);
    }

    // Valid PATCH setting series, series_index, rating
    let patch_res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/api/v1/book/books/{book_id}"))
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "series": "  Foundation Saga  ",
                        "series_index": 1.5,
                        "rating": 9.2
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(patch_res.status(), StatusCode::OK);
    let patched: serde_json::Value =
        serde_json::from_slice(&to_bytes(patch_res.into_body(), usize::MAX).await.unwrap())
            .unwrap();
    assert_eq!(patched["series"], "Foundation Saga");
    assert_eq!(patched["series_index"], 1.5);
    assert_eq!(patched["rating"], 9.2);
    assert_eq!(patched["metadata"]["series"], "Foundation Saga");
    assert_eq!(patched["metadata"]["series_index"], 1.5);
    assert_eq!(patched["metadata"]["rating"], 9.2);

    // Verify PoolBookSummaryDto exposes series, series_index, rating
    let pool_res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/book/pool")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(pool_res.status(), StatusCode::OK);
    let pool_json: serde_json::Value =
        serde_json::from_slice(&to_bytes(pool_res.into_body(), usize::MAX).await.unwrap()).unwrap();
    let pool_book = &pool_json["items"][0];
    assert_eq!(pool_book["series"], "Foundation Saga");
    assert_eq!(pool_book["series_index"], 1.5);
    assert_eq!(pool_book["rating"], 9.2);
    assert_eq!(pool_book["bound"], true);

    // Verify OPDS v1.2 XML feed exposes calibre:series, calibre:series_index, calibre:rating
    let basic = BASE64_STANDARD.encode("admin:password123");
    let opds_v1_res = app
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
    assert_eq!(opds_v1_res.status(), StatusCode::OK);
    let v1_xml = String::from_utf8(
        to_bytes(opds_v1_res.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(v1_xml.contains("xmlns:calibre=\"http://calibre.kovidgoyal.net/2009/metadata\""));
    assert!(v1_xml.contains("<calibre:series>Foundation Saga</calibre:series>"));
    assert!(v1_xml.contains("<calibre:series_index>1.5</calibre:series_index>"));
    assert!(v1_xml.contains("<calibre:rating>9.2</calibre:rating>"));

    // Verify OPDS v2.0 JSON feed exposes belongsTo.series, series, series_index, rating
    let opds_v2_res = app
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
    assert_eq!(opds_v2_res.status(), StatusCode::OK);
    let v2_json: serde_json::Value =
        serde_json::from_slice(&to_bytes(opds_v2_res.into_body(), usize::MAX).await.unwrap())
            .unwrap();
    let pub_meta = &v2_json["publications"][0]["metadata"];
    assert_eq!(pub_meta["belongsTo"]["series"]["name"], "Foundation Saga");
    assert_eq!(pub_meta["belongsTo"]["series"]["position"], 1.5);
    assert_eq!(pub_meta["series"], "Foundation Saga");
    assert_eq!(pub_meta["series_index"], 1.5);
    assert_eq!(pub_meta["rating"], 9.2);
}

#[tokio::test]
async fn online_items_http_crud_lookup_and_progress_roundtrip() {
    let (_tmp, token, _admin_user, _library_svc, app) = setup_test_book_app().await;

    // 1. Import a book source via `/api/v1/book/sources/import`
    let imp_res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/book/sources/import")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "raw_json": serde_json::json!({
                            "bookSourceName": "在线书架源",
                            "bookSourceUrl": "https://shelf.example.com",
                            "enabled": true
                        }).to_string()
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(imp_res.status(), StatusCode::OK);
    let imp_json: serde_json::Value =
        serde_json::from_slice(&to_bytes(imp_res.into_body(), usize::MAX).await.unwrap()).unwrap();
    let source_id = imp_json["sources"][0]["id"].as_str().unwrap().to_string();

    // 2. PUT `/api/v1/book/online/items` (upsert favorite + reading progress)
    let put_res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/v1/book/online/items")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "source_kind": "online",
                        "source_id": source_id,
                        "item_key": "https://shelf.example.com/book/42",
                        "title": "深空彼岸",
                        "subtitle": "辰东",
                        "favorite": true,
                        "progress": {
                            "unit_index": 3,
                            "unit_title": "第四章 旧土",
                            "position": 0.5,
                            "duration": 1.0
                        }
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(put_res.status(), StatusCode::OK);
    let item_dto: serde_json::Value =
        serde_json::from_slice(&to_bytes(put_res.into_body(), usize::MAX).await.unwrap()).unwrap();
    let item_id = item_dto["id"].as_str().unwrap().to_string();
    assert_eq!(item_dto["media_kind"], "book");
    assert_eq!(item_dto["source_name"], "在线书架源");
    assert_eq!(item_dto["availability"], "ok");
    assert_eq!(item_dto["favorite"], true);
    assert_eq!(item_dto["progress"]["unit_index"], 3);

    // 3. GET `/api/v1/book/online/items?view=favorites`
    let list_res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/book/online/items?view=favorites")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(list_res.status(), StatusCode::OK);
    let list_json: serde_json::Value =
        serde_json::from_slice(&to_bytes(list_res.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(list_json["total"], 1);
    assert_eq!(list_json["items"][0]["id"], item_id);

    // 4. POST `/api/v1/book/online/items/lookup`
    let lookup_res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/book/online/items/lookup")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "items": [{
                            "source_kind": "online",
                            "source_id": source_id,
                            "item_key": "https://shelf.example.com/book/42"
                        }]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(lookup_res.status(), StatusCode::OK);
    let lookup_json: serde_json::Value =
        serde_json::from_slice(&to_bytes(lookup_res.into_body(), usize::MAX).await.unwrap())
            .unwrap();
    assert_eq!(lookup_json["items"].as_array().unwrap().len(), 1);

    // 5. PATCH `/api/v1/book/online/items/{id}` + PUT/DELETE `/progress`
    let patch_res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/api/v1/book/online/items/{item_id}"))
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "intro": "浩瀚宇宙" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(patch_res.status(), StatusCode::OK);

    let prog_res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/api/v1/book/online/items/{item_id}/progress"))
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "unit_index": 8,
                        "unit_title": "第九章 新篇",
                        "position": 0.8,
                        "duration": 1.0
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(prog_res.status(), StatusCode::OK);
    let prog_json: serde_json::Value =
        serde_json::from_slice(&to_bytes(prog_res.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(prog_json["progress"]["unit_index"], 8);

    let clr_res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/v1/book/online/items/{item_id}/progress"))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(clr_res.status(), StatusCode::OK);
    let clr_json: serde_json::Value =
        serde_json::from_slice(&to_bytes(clr_res.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert!(clr_json["progress"].is_null());

    // 6. Disabling source via PATCH `/api/v1/book/sources/{source_id}` updates availability to `source_disabled`
    let dis_res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/api/v1/book/sources/{source_id}"))
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "enabled": false }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(dis_res.status(), StatusCode::OK);

    let get_after_dis = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/book/online/items/{item_id}"))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(get_after_dis.status(), StatusCode::OK);
    let dis_item: serde_json::Value = serde_json::from_slice(
        &to_bytes(get_after_dis.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(dis_item["availability"], "source_disabled");

    // 7. DELETE `/api/v1/book/online/items/{id}`
    let del_res = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/v1/book/online/items/{item_id}"))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(del_res.status(), StatusCode::NO_CONTENT);
}

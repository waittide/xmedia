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

use application::ComicServices;
use application::catalog::ComicCatalogService;
use application::lifecycle::ComicDomainLifecycle;
use application::opds::ComicOpdsService;
use application::reading::ComicReadingService;
use infra::archive::PureRustArchiveReader;
use infra::persistence::repositories::{
    ToastyChapterRepository, ToastyComicReadingRepository, ToastySeriesRepository,
};
use infra::scanner::SmartComicScanner;
use infra::storage::LocalComicFileStorage;
use infra::thumbnail::WebpThumbnailGenerator;

pub use infra::persistence::{ensure_comic_schema, register_comic_models};

pub fn init_comic_module(
    db: Db,
    data_dir: &Path,
    libraries_repo: Arc<dyn LibraryRepository>,
    library_service: &LibraryService,
    events: Arc<dyn RealtimeEventPort>,
) -> DomainResult<ComicServices> {
    let series = Arc::new(ToastySeriesRepository::new(db.clone()));
    let chapters = Arc::new(ToastyChapterRepository::new(db.clone()));
    let reading_repo = Arc::new(ToastyComicReadingRepository::new(db));

    let archives = Arc::new(PureRustArchiveReader::new());
    let scanner = Arc::new(SmartComicScanner::new(archives.clone()));
    let thumbnails = Arc::new(WebpThumbnailGenerator::new(data_dir)?);
    let storage = Arc::new(LocalComicFileStorage::new(data_dir)?);

    let lifecycle = Arc::new(ComicDomainLifecycle::new(
        libraries_repo.clone(),
        series.clone(),
        chapters.clone(),
        reading_repo.clone(),
        scanner,
        archives.clone(),
        thumbnails.clone(),
        storage.clone(),
        events.clone(),
    ));
    library_service.register_lifecycle(lifecycle);

    let catalog = Arc::new(ComicCatalogService::new(
        libraries_repo.clone(),
        series.clone(),
        chapters.clone(),
        reading_repo.clone(),
        archives,
        thumbnails,
        storage,
        events,
    ));
    let reading = Arc::new(ComicReadingService::new(
        series.clone(),
        chapters.clone(),
        reading_repo.clone(),
    ));
    let opds = Arc::new(ComicOpdsService::new(
        libraries_repo,
        series,
        chapters,
        reading_repo,
    ));

    Ok(ComicServices::new(catalog, reading, opds))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::io::{Cursor, Write};
    use std::sync::{Arc, RwLock};

    use axum::Router;
    use axum::body::{Body, to_bytes};
    use axum::http::{Request, StatusCode, header};
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
    use image::{ImageBuffer, ImageFormat, Rgba};
    use tempfile::TempDir;
    use toasty::Db;
    use toasty::schema::ModelSet;
    use tower::ServiceExt;
    use xmedia_core::application::auth::AuthService;
    use xmedia_core::application::dto::{CreateLibraryInput, SetupAdminInput};
    use xmedia_core::application::library::LibraryService;
    use xmedia_core::domain::error::DomainError;
    use xmedia_core::domain::library::MediaKind;
    use xmedia_core::infra::events::BroadcastEventHub;
    use xmedia_core::infra::persistence::repositories::{
        ToastyLibraryRepository, ToastyUserRepository,
    };
    use xmedia_core::infra::security::{JwtTokenService, SecurityManager};
    use xmedia_core::{ensure_core_schema, register_core_models};
    use zip::ZipWriter;
    use zip::write::SimpleFileOptions;

    use crate::application::dto::{SeriesQueryFilter, UpdateProgressInput, UploadArchiveInput};
    use crate::domain::chapter::ChapterMetadataPatch;
    use crate::http::state::ComicHttpState;
    use crate::http::{opds_router, rest_router};
    use crate::{ensure_comic_schema, init_comic_module, register_comic_models};

    fn sample_cbz(series_name: &str, number: &str) -> Vec<u8> {
        let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_fn(120, 180, |_, _| Rgba([100, 140, 210, 255]));
        let mut png = Cursor::new(Vec::new());
        img.write_to(&mut png, ImageFormat::Png).unwrap();
        let png_bytes = png.into_inner();

        let mut cursor = Cursor::new(Vec::new());
        {
            let mut zw = ZipWriter::new(&mut cursor);
            let opts = SimpleFileOptions::default();
            zw.start_file("ComicInfo.xml", opts).unwrap();
            write!(
                &mut zw,
                r#"<?xml version="1.0" encoding="utf-8"?>
<ComicInfo>
  <Series>{series_name}</Series>
  <Number>{number}</Number>
  <Writer>Kamome Shirahama</Writer>
  <Genre>Fantasy</Genre>
  <Manga>YesAndRightToLeft</Manga>
</ComicInfo>"#
            )
            .unwrap();
            zw.start_file("001.png", opts).unwrap();
            zw.write_all(&png_bytes).unwrap();
            zw.start_file("002.png", opts).unwrap();
            zw.write_all(&png_bytes).unwrap();
            zw.finish().unwrap();
        }
        cursor.into_inner()
    }

    #[tokio::test]
    async fn comic_etag_pse_and_scan_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let db_path = tmp.path().join("xmedia_comic_test.db");
        let db_url = format!("sqlite:{}", db_path.display());

        let mut models = ModelSet::new();
        register_core_models(&mut models);
        register_comic_models(&mut models);
        let mut db = Db::builder().models(models).connect(&db_url).await.unwrap();
        ensure_core_schema(&mut db, &db_url).await.unwrap();
        ensure_comic_schema(&mut db, &db_url).await.unwrap();

        let users = Arc::new(ToastyUserRepository::new(db.clone()));
        let libraries_repo = Arc::new(ToastyLibraryRepository::new(db.clone()));
        let lifecycles = Arc::new(RwLock::new(HashMap::new()));
        let security = Arc::new(SecurityManager::new("comic-secret"));
        let tokens = Arc::new(JwtTokenService::new("comic-secret", 3600));
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

        let comic_services =
            init_comic_module(db, tmp.path(), libraries_repo, &library_svc, events).unwrap();

        let session = auth
            .setup_initial_admin(SetupAdminInput {
                username: "alice".into(),
                display_name: Some("Alice".into()),
                password: "secretpassword".into(),
            })
            .await
            .unwrap();
        let admin = auth.authenticate_jwt(&session.token).await.unwrap();

        let state = ComicHttpState::new(auth.clone(), library_svc.clone(), comic_services.clone());
        let app = Router::new()
            .nest("/api/v1/comic", rest_router(state.clone()))
            .nest("/opds/comic", opds_router(state));

        // 1. Upload Ch.1 and Ch.1.5 CBZs
        let up1 = comic_services
            .catalog
            .upload_archive(
                &admin,
                UploadArchiveInput {
                    filename: "Witch Hat Atelier - Ch.1.cbz".into(),
                    bytes: sample_cbz("Witch Hat Atelier", "1"),
                    target_series_id: None,
                    series_title_override: None,
                },
            )
            .await
            .unwrap();
        let _up2 = comic_services
            .catalog
            .upload_archive(
                &admin,
                UploadArchiveInput {
                    filename: "Witch Hat Atelier - Ch.1.5.cbz".into(),
                    bytes: sample_cbz("Witch Hat Atelier", "1.5"),
                    target_series_id: Some(up1.series_id),
                    series_title_override: None,
                },
            )
            .await
            .unwrap();

        // 2. Verify page streaming + ETag 304 Not Modified
        let page_uri = format!(
            "/api/v1/comic/chapters/{}/pages/1?token={}",
            up1.chapter_id, session.token
        );
        let p1_res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(&page_uri)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(p1_res.status(), StatusCode::OK);
        let etag = p1_res
            .headers()
            .get(header::ETAG)
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();

        let p1_304 = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(&page_uri)
                    .header(header::IF_NONE_MATCH, &etag)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(p1_304.status(), StatusCode::NOT_MODIFIED);

        // 2b. Verify Range 206 archive download on REST and OPDS with RFC 5987 Content-Disposition
        let rest_dl = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/comic/chapters/{}/download?token={}",
                        up1.chapter_id, session.token
                    ))
                    .header(header::RANGE, "bytes=0-15")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(rest_dl.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(
            rest_dl.headers().get(header::ACCEPT_RANGES).unwrap(),
            "bytes"
        );
        assert!(
            rest_dl
                .headers()
                .get(header::CONTENT_DISPOSITION)
                .unwrap()
                .to_str()
                .unwrap()
                .contains("filename*=UTF-8''")
        );
        let dl_chunk = to_bytes(rest_dl.into_body(), usize::MAX).await.unwrap();
        assert_eq!(dl_chunk.len(), 16);
        assert_eq!(&dl_chunk[0..2], b"PK");

        // 3. Verify OPDS 1.2 + PSE 1.1 page streaming & progress sync
        let basic = BASE64_STANDARD.encode("alice:secretpassword");
        let opds_dl = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/opds/comic/v1.2/chapters/{}/download",
                        up1.chapter_id
                    ))
                    .header(header::AUTHORIZATION, format!("Basic {basic}"))
                    .header(header::RANGE, "bytes=0-7")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(opds_dl.status(), StatusCode::PARTIAL_CONTENT);
        let opds_chunk = to_bytes(opds_dl.into_body(), usize::MAX).await.unwrap();
        assert_eq!(opds_chunk.len(), 8);
        assert_eq!(&opds_chunk[0..2], b"PK");
        let ch_feed_res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/opds/comic/v1.2/series/{}", up1.series_id))
                    .header(header::AUTHORIZATION, format!("Basic {basic}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(ch_feed_res.status(), StatusCode::OK);
        let ch_xml = String::from_utf8(
            to_bytes(ch_feed_res.into_body(), usize::MAX)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert!(ch_xml.contains("http://vaemendis.net/opds-pse/stream"));

        // Stream 0-based page 1 (which is 1-based page 2 of 2 -> completes Ch.1!)
        let pse_res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/opds/comic/v1.2/chapters/{}/pse/1",
                        up1.chapter_id
                    ))
                    .header(header::AUTHORIZATION, format!("Basic {basic}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(pse_res.status(), StatusCode::OK);

        // Continue reading should now point to Ch.1.5 page 1!
        let detail = comic_services
            .catalog
            .get_series_detail(&admin, up1.series_id)
            .await
            .unwrap();
        let ptr = detail.continue_reading.unwrap();
        assert_eq!(ptr.chapter_number, "1.5");
        assert_eq!(ptr.current_page, 1);

        // 4. Verify local mount scan via LibraryService
        let mount_dir = tmp.path().join("mounted_comics").join("Dungeon Meshi");
        std::fs::create_dir_all(&mount_dir).unwrap();
        std::fs::write(mount_dir.join("Ch.1.cbz"), sample_cbz("Dungeon Meshi", "1")).unwrap();

        let created_lib = library_svc
            .create_local_mount(
                &admin,
                CreateLibraryInput {
                    name: "Local Manga".into(),
                    media_kind: MediaKind::Comic,
                    root_path: tmp
                        .path()
                        .join("mounted_comics")
                        .to_string_lossy()
                        .into_owned(),
                },
            )
            .await
            .unwrap();
        library_svc
            .trigger_scan(&admin, created_lib.id)
            .await
            .unwrap();

        for _ in 0..40 {
            let lib_now = library_svc.get_library(created_lib.id).await.unwrap();
            if lib_now.item_count == 1 {
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

        // 5. Verify uploaded comic with same title as LocalMount series does NOT merge into local library
        let local_list = comic_services
            .catalog
            .list_series(
                &admin,
                SeriesQueryFilter {
                    library_id: Some(created_lib.id),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(local_list.len(), 1);
        let local_dm = &local_list[0];

        let cross_upload = comic_services
            .catalog
            .upload_archive(
                &admin,
                UploadArchiveInput {
                    filename: "Dungeon Meshi - Ch.2.cbz".into(),
                    bytes: sample_cbz("Dungeon Meshi", "2"),
                    target_series_id: Some(local_dm.id),
                    series_title_override: None,
                },
            )
            .await;
        assert!(matches!(cross_upload, Err(DomainError::Validation(_))));

        let managed_dm = comic_services
            .catalog
            .upload_archive(
                &admin,
                UploadArchiveInput {
                    filename: "Dungeon Meshi - Ch.2.cbz".into(),
                    bytes: sample_cbz("Dungeon Meshi", "2"),
                    target_series_id: None,
                    series_title_override: None,
                },
            )
            .await
            .unwrap();
        assert_ne!(managed_dm.series_id, local_dm.id);

        // 6. Verify DELETE /api/v1/comic/chapters/{id} decrements count and removes empty series
        let del_ch1 = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!(
                        "/api/v1/comic/chapters/{}?token={}",
                        up1.chapter_id, session.token
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(del_ch1.status(), StatusCode::NO_CONTENT);

        let after_ch1 = comic_services
            .catalog
            .get_series_detail(&admin, up1.series_id)
            .await
            .unwrap();
        assert_eq!(after_ch1.series.chapter_count, 1);

        let del_ch2 = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!(
                        "/api/v1/comic/chapters/{}?token={}",
                        _up2.chapter_id, session.token
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(del_ch2.status(), StatusCode::NO_CONTENT);

        assert!(matches!(
            comic_services
                .catalog
                .get_series_detail(&admin, up1.series_id)
                .await,
            Err(DomainError::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn update_chapter_metadata_preserves_user_reading_progress() {
        let tmp = TempDir::new().unwrap();
        let db_path = tmp.path().join("xmedia_comic_progress_test.db");
        let db_url = format!("sqlite:{}", db_path.display());

        let mut models = ModelSet::new();
        register_core_models(&mut models);
        register_comic_models(&mut models);
        let mut db = Db::builder().models(models).connect(&db_url).await.unwrap();
        ensure_core_schema(&mut db, &db_url).await.unwrap();
        ensure_comic_schema(&mut db, &db_url).await.unwrap();

        let users = Arc::new(ToastyUserRepository::new(db.clone()));
        let libraries_repo = Arc::new(ToastyLibraryRepository::new(db.clone()));
        let lifecycles = Arc::new(RwLock::new(HashMap::new()));
        let security = Arc::new(SecurityManager::new("comic-secret"));
        let tokens = Arc::new(JwtTokenService::new("comic-secret", 3600));
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
        let comic_services =
            init_comic_module(db, tmp.path(), libraries_repo, &library_svc, events).unwrap();

        let session = auth
            .setup_initial_admin(SetupAdminInput {
                username: "admin".into(),
                display_name: Some("Admin".into()),
                password: "secretpassword".into(),
            })
            .await
            .unwrap();
        let admin = auth.authenticate_jwt(&session.token).await.unwrap();

        let ch1 = comic_services
            .catalog
            .upload_archive(
                &admin,
                UploadArchiveInput {
                    filename: "Progress Series - Ch.1.cbz".into(),
                    bytes: sample_cbz("Progress Series", "1"),
                    target_series_id: None,
                    series_title_override: None,
                },
            )
            .await
            .unwrap();
        let ch2 = comic_services
            .catalog
            .upload_archive(
                &admin,
                UploadArchiveInput {
                    filename: "Progress Series - Ch.2.cbz".into(),
                    bytes: sample_cbz("Progress Series", "2"),
                    target_series_id: Some(ch1.series_id),
                    series_title_override: None,
                },
            )
            .await
            .unwrap();

        // Mark Ch.1 as partially read (page 1 of 2, not completed).
        comic_services
            .reading
            .update_progress(
                admin.id,
                ch1.chapter_id,
                UpdateProgressInput {
                    current_page: 1,
                    is_completed: Some(false),
                },
            )
            .await
            .unwrap();

        let updated_in_progress = comic_services
            .catalog
            .update_chapter_metadata(
                &admin,
                ch1.chapter_id,
                ChapterMetadataPatch {
                    title: Some("Renamed Ch.1".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(updated_in_progress.title, "Renamed Ch.1");
        assert_eq!(updated_in_progress.current_page, 1);
        assert!(!updated_in_progress.is_completed);

        // Complete Ch.1 (page 2 of 2) and update metadata again.
        comic_services
            .reading
            .update_progress(
                admin.id,
                ch1.chapter_id,
                UpdateProgressInput {
                    current_page: 2,
                    is_completed: Some(true),
                },
            )
            .await
            .unwrap();

        let updated_completed = comic_services
            .catalog
            .update_chapter_metadata(
                &admin,
                ch1.chapter_id,
                ChapterMetadataPatch {
                    summary: Some("Updated summary".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(updated_completed.summary, "Updated summary");
        assert_eq!(updated_completed.current_page, 2);
        assert!(updated_completed.is_completed);

        // Unread Ch.2 should still report (0, false).
        let updated_unread = comic_services
            .catalog
            .update_chapter_metadata(
                &admin,
                ch2.chapter_id,
                ChapterMetadataPatch {
                    title: Some("Renamed Ch.2".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(updated_unread.current_page, 0);
        assert!(!updated_unread.is_completed);
    }

    #[tokio::test]
    async fn opds_v12_and_v20_routes_render_consistent_feeds() {
        let tmp = TempDir::new().unwrap();
        let db_path = tmp.path().join("xmedia_comic_opds_test.db");
        let db_url = format!("sqlite:{}", db_path.display());

        let mut models = ModelSet::new();
        register_core_models(&mut models);
        register_comic_models(&mut models);
        let mut db = Db::builder().models(models).connect(&db_url).await.unwrap();
        ensure_core_schema(&mut db, &db_url).await.unwrap();
        ensure_comic_schema(&mut db, &db_url).await.unwrap();

        let users = Arc::new(ToastyUserRepository::new(db.clone()));
        let libraries_repo = Arc::new(ToastyLibraryRepository::new(db.clone()));
        let lifecycles = Arc::new(RwLock::new(HashMap::new()));
        let security = Arc::new(SecurityManager::new("comic-secret"));
        let tokens = Arc::new(JwtTokenService::new("comic-secret", 3600));
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
        let comic_services =
            init_comic_module(db, tmp.path(), libraries_repo, &library_svc, events).unwrap();

        let session = auth
            .setup_initial_admin(SetupAdminInput {
                username: "bob".into(),
                display_name: Some("Bob".into()),
                password: "secretpassword".into(),
            })
            .await
            .unwrap();
        let admin = auth.authenticate_jwt(&session.token).await.unwrap();

        let uploaded = comic_services
            .catalog
            .upload_archive(
                &admin,
                UploadArchiveInput {
                    filename: "Atelier & Magic - Ch.1.cbz".into(),
                    bytes: sample_cbz("Atelier & Magic", "1"),
                    target_series_id: None,
                    series_title_override: None,
                },
            )
            .await
            .unwrap();

        let state = ComicHttpState::new(auth, library_svc, comic_services);
        let app = Router::new().nest("/opds/comic", opds_router(state));
        let basic = BASE64_STANDARD.encode("bob:secretpassword");

        // OPDS v1.2 & v2 & v2.0 root routes
        for (uri, expected_ct_substr) in [
            ("/opds/comic/v1.2", "application/atom+xml"),
            ("/opds/comic/v2", "application/opds+json"),
            ("/opds/comic/v2.0", "application/opds+json"),
            ("/opds/comic/v1.2/libraries", "application/atom+xml"),
            ("/opds/comic/v2.0/libraries", "application/opds+json"),
            ("/opds/comic/v1.2/tags", "application/atom+xml"),
            ("/opds/comic/v2.0/tags", "application/opds+json"),
            ("/opds/comic/v1.2/shelves", "application/atom+xml"),
            ("/opds/comic/v2.0/shelves", "application/opds+json"),
            ("/opds/comic/v1.2/favorites", "application/atom+xml"),
            ("/opds/comic/v2.0/favorites", "application/opds+json"),
            ("/opds/comic/v1.2/latest", "application/atom+xml"),
            ("/opds/comic/v2.0/latest", "application/opds+json"),
            ("/opds/comic/v1.2/continue", "application/atom+xml"),
            ("/opds/comic/v2.0/continue", "application/opds+json"),
        ] {
            let res = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(uri)
                        .header(header::AUTHORIZATION, format!("Basic {basic}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::OK, "failed on {uri}");
            let ct = res
                .headers()
                .get(header::CONTENT_TYPE)
                .unwrap()
                .to_str()
                .unwrap();
            assert!(
                ct.contains(expected_ct_substr),
                "unexpected content-type {ct} for {uri}"
            );
        }

        // Verify query parameter search & XML escaping on v1.2 and JSON on v2.0
        let v1_search = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/opds/comic/v1.2/series?q=Atelier")
                    .header(header::AUTHORIZATION, format!("Basic {basic}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(v1_search.status(), StatusCode::OK);
        let v1_xml = String::from_utf8(
            to_bytes(v1_search.into_body(), usize::MAX)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert!(v1_xml.contains("Atelier &amp; Magic"));

        let v2_chapters = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/opds/comic/v2.0/series/{}", uploaded.series_id))
                    .header(header::AUTHORIZATION, format!("Basic {basic}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(v2_chapters.status(), StatusCode::OK);
        let v2_json: serde_json::Value =
            serde_json::from_slice(&to_bytes(v2_chapters.into_body(), usize::MAX).await.unwrap())
                .unwrap();
        assert_eq!(v2_json["metadata"]["title"], "Atelier & Magic");
        assert_eq!(v2_json["publications"].as_array().unwrap().len(), 1);
    }
}

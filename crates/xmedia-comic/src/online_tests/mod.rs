mod runtime_and_tracked;
mod sources_and_proxy;

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

pub(super) fn valid_png_bytes() -> Vec<u8> {
    let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
        ImageBuffer::from_fn(4, 4, |_, _| Rgba([12, 34, 56, 255]));
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, ImageFormat::Png).unwrap();
    out.into_inner()
}

pub(super) fn valid_jpeg_bytes() -> Vec<u8> {
    let img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::from_fn(4, 4, |_, _| Rgb([200, 100, 50]));
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, ImageFormat::Jpeg).unwrap();
    out.into_inner()
}

pub(super) struct TestHarness {
    pub(super) _tmp: TempDir,
    pub(super) app: Router,
    pub(super) admin_token: String,
    pub(super) member_token: String,
}

pub(super) async fn setup_test_harness(allow_private_ips: bool) -> TestHarness {
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
    let admin = auth
        .authenticate(
            xmedia_core::application::dto::AuthCredential::Jwt(&admin_sess.token),
            None,
        )
        .await
        .unwrap();

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
    let member_sess = auth
        .login("member", "memberpassword123", None)
        .await
        .unwrap();

    let state = ComicHttpState::new(auth, comic_services);
    let app = Router::new()
        .nest("/api/v1/plugins", crate::http::plugins_router(&state))
        .nest("/api/v1/comic", rest_router(state.clone()))
        .nest("/opds/comic", crate::http::opds_router(state));

    TestHarness {
        _tmp: tmp,
        app,
        admin_token: admin_sess.token,
        member_token: member_sess.token,
    }
}

pub(super) async fn json_request(
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

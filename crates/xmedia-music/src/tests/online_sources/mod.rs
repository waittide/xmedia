mod crud_and_explore;
mod proxy_and_security;
mod seeds_and_engine;
mod tracked_and_cache;

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Request, StatusCode, header};
use axum::response::Response;
use bytes::Bytes;
use tempfile::TempDir;
use toasty::Db;
use toasty::schema::ModelSet;
use tower::ServiceExt;
use xmedia_core::application::auth::AuthService;
use xmedia_core::application::dto::{CreateUserInput, SetupAdminInput};
use xmedia_core::application::library::LibraryService;
use xmedia_core::domain::user::UserRole;
use xmedia_core::infra::events::BroadcastEventHub;
use xmedia_core::infra::ffmpeg::FfmpegToolkit;
use xmedia_core::infra::persistence::repositories::{
    ToastyLibraryRepository, ToastyUserRepository,
};
use xmedia_core::infra::security::{JwtTokenService, SecurityManager};
use xmedia_core::{ensure_core_schema, register_core_models};

use crate::http::rest_router;
use crate::http::state::MusicHttpState;
use crate::{
    ensure_music_schema, init_music_module, init_music_module_with_allow_private_ips,
    register_music_models,
};

pub(super) const SEEDS_MUSIC_JSON: &str = include_str!("../../../testdata/online_sources.json");

pub(super) const PNG_1X1: &[u8] = &[
    0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, b'I', b'H', b'D', b'R',
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0A, b'I', b'D', b'A', b'T', 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, b'I', b'E', b'N', b'D', 0xAE,
    0x42, 0x60, 0x82,
];

pub(super) struct OnlineTestHarness {
    pub(super) _tmp: TempDir,
    pub(super) app: Router,
    pub(super) admin_token: String,
    pub(super) member_token: String,
}

pub(super) async fn setup_online_test_harness(allow_private_ips: bool) -> OnlineTestHarness {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("xmedia_music_online.db");
    let db_url = format!("sqlite:{}", db_path.display());

    let mut models = ModelSet::new();
    register_core_models(&mut models);
    register_music_models(&mut models);
    let mut db = Db::builder().models(models).connect(&db_url).await.unwrap();
    ensure_core_schema(&mut db, &db_url).await.unwrap();
    ensure_music_schema(&mut db, &db_url).await.unwrap();

    let users_repo = Arc::new(ToastyUserRepository::new(db.clone()));
    let libraries_repo = Arc::new(ToastyLibraryRepository::new(db.clone()));
    let lifecycles = Arc::new(RwLock::new(HashMap::new()));
    let events = Arc::new(BroadcastEventHub::default());
    let security = Arc::new(SecurityManager::new("music-online-secret"));
    let tokens = Arc::new(JwtTokenService::new("music-online-secret", 3600));

    let auth_svc = Arc::new(AuthService::new(
        users_repo,
        security,
        tokens,
        lifecycles.clone(),
    ));
    let library_svc = Arc::new(LibraryService::new(
        libraries_repo.clone(),
        events.clone(),
        lifecycles,
    ));

    let music_services = if allow_private_ips {
        init_music_module_with_allow_private_ips(
            db,
            tmp.path(),
            FfmpegToolkit::default(),
            libraries_repo,
            &library_svc,
            events,
            true,
        )
        .unwrap()
    } else {
        init_music_module(
            db,
            tmp.path(),
            FfmpegToolkit::default(),
            libraries_repo,
            &library_svc,
            events,
        )
        .unwrap()
    };

    let admin_session = auth_svc
        .setup_initial_admin(SetupAdminInput {
            username: "admin".into(),
            display_name: Some("Admin".into()),
            password: "adminpassword123".into(),
        })
        .await
        .unwrap();
    let admin = auth_svc
        .authenticate(
            xmedia_core::application::dto::AuthCredential::Jwt(&admin_session.token),
            None,
        )
        .await
        .unwrap();
    auth_svc
        .create_user(
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
    let member_session = auth_svc
        .login("member", "memberpassword123", None)
        .await
        .unwrap();

    let state = MusicHttpState::new(auth_svc, library_svc, music_services);
    let app = rest_router(state);

    OnlineTestHarness {
        _tmp: tmp,
        app,
        admin_token: admin_session.token,
        member_token: member_session.token,
    }
}

pub(super) async fn send_json(
    app: &Router,
    method: &str,
    uri: &str,
    token: &str,
    body: Option<serde_json::Value>,
) -> (StatusCode, HeaderMap, Vec<u8>) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"));
    let req_body = if let Some(val) = body {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
        Body::from(serde_json::to_vec(&val).unwrap())
    } else {
        Body::empty()
    };
    let resp = app
        .clone()
        .oneshot(builder.body(req_body).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let headers = resp.headers().clone();
    let bytes = to_bytes(resp.into_body(), 64 * 1024 * 1024).await.unwrap();
    (status, headers, bytes.to_vec())
}

pub(super) fn music_delayed_chunked_response(
    content_type: Option<&'static str>,
    chunks: Vec<(Vec<u8>, u64)>,
) -> Response {
    let (tx, rx) = tokio::sync::mpsc::channel::<xmedia_core::domain::error::DomainResult<Bytes>>(8);
    tokio::spawn(async move {
        for (data, delay_ms) in chunks {
            if delay_ms > 0 {
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            }
            if tx.send(Ok(Bytes::from(data))).await.is_err() {
                break;
            }
        }
    });
    let mut builder = Response::builder().status(StatusCode::OK);
    if let Some(ct) = content_type {
        builder = builder.header(header::CONTENT_TYPE, ct);
    }
    builder
        .body(Body::from_stream(
            xmedia_core::infra::stream::MpscByteStream::into_boxed(rx),
        ))
        .unwrap()
}

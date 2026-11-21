pub mod gateway;
pub mod static_files;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use anyhow::Context;
use axum::Router;
use axum::http::HeaderValue;
use toasty::Db;
use toasty::schema::ModelSet;
use tower_http::compression::CompressionLayer;
use tower_http::compression::predicate::{DefaultPredicate, NotForContentType, Predicate};
use tower_http::cors::{AllowOrigin, Any, CorsLayer};
use tower_http::trace::TraceLayer;
use xmedia_core::application::auth::AuthService;
use xmedia_core::application::library::LibraryService;
use xmedia_core::domain::library::MediaKind;
use xmedia_core::infra::events::BroadcastEventHub;
use xmedia_core::infra::ffmpeg::FfmpegToolkit;
use xmedia_core::infra::persistence::repositories::{
    ToastyLibraryRepository, ToastyUserRepository,
};
use xmedia_core::infra::security::{JwtTokenService, SecurityManager};
use xmedia_core::{
    ensure_core_schema, ensure_sqlite_parent_dir, is_core_schema_initialized, register_core_models,
};

use gateway::{FfmpegStatusCache, GatewayState, core_api_router};

pub const DEFAULT_DEV_SECRET: &str = "xmedia-change-this-secret-in-production";

#[derive(Debug, Clone)]
pub struct XMediaConfig {
    pub data_dir: PathBuf,
    pub database_url: String,
    pub secret: String,
    pub jwt_ttl_secs: u64,
    pub ffmpeg_bin: String,
    pub external_web_dir: Option<PathBuf>,
}

pub struct XMediaApp {
    pub auth: Arc<AuthService>,
    pub libraries: Arc<LibraryService>,
    pub events: Arc<BroadcastEventHub>,
    pub enabled_modules: Vec<MediaKind>,
    pub router: Router,
}

impl XMediaApp {
    pub async fn connect_and_init(cfg: XMediaConfig) -> anyhow::Result<Self> {
        Self::connect_and_init_with_cors_origins(cfg, false, &[]).await
    }

    pub async fn connect_and_init_with_cors(
        cfg: XMediaConfig,
        allow_permissive_cors: bool,
    ) -> anyhow::Result<Self> {
        Self::connect_and_init_with_cors_origins(cfg, allow_permissive_cors, &[]).await
    }

    pub async fn connect_and_init_with_cors_origins(
        cfg: XMediaConfig,
        allow_permissive_cors: bool,
        cors_allow_origins: &[String],
    ) -> anyhow::Result<Self> {
        if cfg.secret == DEFAULT_DEV_SECRET || cfg.secret.trim().is_empty() {
            tracing::warn!(
                "XMEDIA_SECRET is set to the default development value; please configure a strong unique XMEDIA_SECRET in production"
            );
        }

        std::fs::create_dir_all(&cfg.data_dir).with_context(|| {
            format!("failed to create data directory {}", cfg.data_dir.display())
        })?;
        if let Err(err) = xmedia_core::http::configure_temp_dir(cfg.data_dir.join("tmp")) {
            tracing::warn!(error = %err, "failed to prepare application temp directory");
        }
        ensure_sqlite_parent_dir(&cfg.database_url).map_err(|e| anyhow::anyhow!(e.to_string()))?;

        let mut models = ModelSet::new();
        register_core_models(&mut models);
        #[cfg(feature = "book")]
        xmedia_book::register_book_models(&mut models);
        #[cfg(feature = "comic")]
        xmedia_comic::register_comic_models(&mut models);
        #[cfg(feature = "music")]
        xmedia_music::register_music_models(&mut models);
        #[cfg(feature = "video")]
        xmedia_video::register_video_models(&mut models);

        let mut db = Db::builder()
            .models(models)
            .connect(&cfg.database_url)
            .await
            .map_err(|e| anyhow::anyhow!("database connection failed: {e}"))?;

        if !is_core_schema_initialized(&mut db).await {
            tracing::info!(
                "cold start detected: pushing unified database schema for core and all enabled modules"
            );
            db.push_schema()
                .await
                .map_err(|e| anyhow::anyhow!("failed to push unified database schema: {e}"))?;
        } else {
            ensure_core_schema(&mut db, &cfg.database_url)
                .await
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;
            #[cfg(feature = "book")]
            xmedia_book::ensure_book_schema(&mut db, &cfg.database_url)
                .await
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;
            #[cfg(feature = "comic")]
            xmedia_comic::ensure_comic_schema(&mut db, &cfg.database_url)
                .await
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;
            #[cfg(feature = "music")]
            xmedia_music::ensure_music_schema(&mut db, &cfg.database_url)
                .await
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;
            #[cfg(feature = "video")]
            xmedia_video::ensure_video_schema(&mut db, &cfg.database_url)
                .await
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        }

        let users_repo = Arc::new(ToastyUserRepository::new(db.clone()));
        let libraries_repo = Arc::new(ToastyLibraryRepository::new(db.clone()));
        let lifecycles = Arc::new(RwLock::new(HashMap::new()));
        let events = Arc::new(BroadcastEventHub::default());
        let security = Arc::new(SecurityManager::new(&cfg.secret));
        let tokens = Arc::new(JwtTokenService::new(&cfg.secret, cfg.jwt_ttl_secs));
        let ffmpeg = FfmpegToolkit::from_ffmpeg_bin(&cfg.ffmpeg_bin);

        let auth = Arc::new(AuthService::new(
            users_repo,
            security,
            tokens,
            lifecycles.clone(),
        ));
        let libraries = Arc::new(LibraryService::with_data_dir(
            libraries_repo.clone(),
            events.clone(),
            lifecycles,
            Some(cfg.data_dir.clone()),
        ));
        libraries
            .recover_interrupted_scans()
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;

        #[allow(unused_mut)]
        let mut enabled_modules = Vec::new();
        #[allow(unused_mut)]
        let mut api_v1 = Router::new();
        #[allow(unused_mut)]
        let mut root_protocols = Router::new();

        #[cfg(feature = "book")]
        {
            enabled_modules.push(MediaKind::Book);
            let book_services = xmedia_book::init_book_module(
                db.clone(),
                &cfg.data_dir,
                libraries_repo.clone(),
                &libraries,
                events.clone(),
            )
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
            libraries
                .ensure_managed_library(MediaKind::Book)
                .await
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;

            let book_state = xmedia_book::http::state::BookHttpState::new(
                auth.clone(),
                libraries.clone(),
                book_services,
            );
            api_v1 = api_v1.nest("/book", xmedia_book::http::rest_router(book_state.clone()));
            root_protocols =
                root_protocols.nest("/opds/book", xmedia_book::http::opds_router(book_state));
        }

        #[cfg(feature = "comic")]
        {
            enabled_modules.push(MediaKind::Comic);
            let comic_services = xmedia_comic::init_comic_module(
                db.clone(),
                &cfg.data_dir,
                libraries_repo.clone(),
                &libraries,
                events.clone(),
            )
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
            libraries
                .ensure_managed_library(MediaKind::Comic)
                .await
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;

            let comic_state = xmedia_comic::http::state::ComicHttpState::new(
                auth.clone(),
                libraries.clone(),
                comic_services,
            );
            api_v1 = api_v1.nest(
                "/comic",
                xmedia_comic::http::rest_router(comic_state.clone()),
            );
            root_protocols =
                root_protocols.nest("/opds/comic", xmedia_comic::http::opds_router(comic_state));
        }

        #[cfg(feature = "music")]
        {
            enabled_modules.push(MediaKind::Music);
            let music_services = xmedia_music::init_music_module(
                db.clone(),
                &cfg.data_dir,
                ffmpeg.clone(),
                libraries_repo.clone(),
                &libraries,
                events.clone(),
            )
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
            libraries
                .ensure_managed_library(MediaKind::Music)
                .await
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;

            let music_state = xmedia_music::http::state::MusicHttpState::new(
                auth.clone(),
                libraries.clone(),
                music_services,
            );
            api_v1 = api_v1.nest(
                "/music",
                xmedia_music::http::rest_router(music_state.clone()),
            );
            root_protocols =
                root_protocols.nest("/rest", xmedia_music::http::subsonic_router(music_state));
        }

        #[cfg(feature = "video")]
        {
            enabled_modules.push(MediaKind::Video);
            let video_services = xmedia_video::init_video_module(
                db.clone(),
                &cfg.data_dir,
                ffmpeg.clone(),
                libraries_repo.clone(),
                &libraries,
                events.clone(),
            )
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
            libraries
                .ensure_managed_library(MediaKind::Video)
                .await
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;

            let video_state = xmedia_video::http::state::VideoHttpState::new(
                auth.clone(),
                libraries.clone(),
                video_services,
            );
            api_v1 = api_v1.nest("/video", xmedia_video::http::rest_router(video_state));
        }

        let _ = db;

        let gateway_state = GatewayState {
            auth: auth.clone(),
            libraries: libraries.clone(),
            libraries_repo,
            events: events.clone(),
            ffmpeg,
            ffmpeg_status_cache: FfmpegStatusCache::new(),
            enabled_modules: Arc::new(enabled_modules.clone()),
            external_web_dir: cfg.external_web_dir,
        };

        let merged_api_v1 = api_v1.merge(core_api_router().with_state(gateway_state.clone()));

        let compression_predicate = DefaultPredicate::new()
            .and(NotForContentType::const_new("audio/"))
            .and(NotForContentType::const_new("video/"))
            .and(NotForContentType::const_new("image/"))
            .and(NotForContentType::const_new("application/octet-stream"))
            .and(NotForContentType::const_new("application/epub+zip"))
            .and(NotForContentType::const_new("application/pdf"))
            .and(NotForContentType::const_new("text/event-stream"));

        let spa_service = axum::routing::any(static_files::serve_spa).with_state(gateway_state);

        let mut router = Router::new()
            .nest("/api/v1", merged_api_v1)
            .merge(root_protocols)
            .fallback_service(spa_service);
        if allow_permissive_cors {
            router = router.layer(CorsLayer::permissive());
        } else if !cors_allow_origins.is_empty() {
            let parsed_origins: Vec<HeaderValue> = cors_allow_origins
                .iter()
                .map(|o| o.trim())
                .filter(|o| !o.is_empty())
                .map(|o| {
                    HeaderValue::from_str(o)
                        .with_context(|| format!("invalid CORS allow-origin header value: '{o}'"))
                })
                .collect::<anyhow::Result<Vec<_>>>()?;
            if !parsed_origins.is_empty() {
                router = router.layer(
                    CorsLayer::new()
                        .allow_origin(AllowOrigin::list(parsed_origins))
                        .allow_methods(Any)
                        .allow_headers(Any),
                );
            }
        }
        let router = router
            .layer(CompressionLayer::new().compress_when(compression_predicate))
            .layer(TraceLayer::new_for_http());

        Ok(Self {
            auth,
            libraries,
            events,
            enabled_modules,
            router,
        })
    }
}

#[cfg(all(
    test,
    feature = "book",
    feature = "comic",
    feature = "music",
    feature = "video"
))]
mod tests {
    use axum::body::{Body, to_bytes};
    use axum::http::{Request, StatusCode, header};
    use tempfile::TempDir;
    use tower::ServiceExt;

    use crate::{XMediaApp, XMediaConfig};

    #[tokio::test]
    async fn unified_gateway_all_four_domains_and_protocols_smoke_test() {
        let tmp = TempDir::new().unwrap();
        let db_path = tmp.path().join("xmedia.db");
        let app_instance = XMediaApp::connect_and_init(XMediaConfig {
            data_dir: tmp.path().to_path_buf(),
            database_url: format!("sqlite:{}", db_path.display()),
            secret: "unified-gateway-secret".into(),
            jwt_ttl_secs: 3600,
            ffmpeg_bin: "__missing_ffmpeg__".into(),
            external_web_dir: None,
        })
        .await
        .unwrap();

        assert_eq!(app_instance.enabled_modules.len(), 4);
        let app = app_instance.router;

        // 1. Capabilities endpoint lists all 4 enabled modules
        let cap_res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/system/capabilities")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(cap_res.status(), StatusCode::OK);
        let cap_json: serde_json::Value =
            serde_json::from_slice(&to_bytes(cap_res.into_body(), usize::MAX).await.unwrap())
                .unwrap();
        assert_eq!(cap_json["modules"].as_array().unwrap().len(), 4);

        // 2. Setup initial Admin -> sets HttpOnly session cookie + issues JWT + API key
        let setup_res = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/setup")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"username":"admin","display_name":"Admin","password":"password123"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(setup_res.status(), StatusCode::CREATED);
        assert!(setup_res.headers().get(header::SET_COOKIE).is_some());
        let setup_json: serde_json::Value =
            serde_json::from_slice(&to_bytes(setup_res.into_body(), usize::MAX).await.unwrap())
                .unwrap();
        let token = setup_json["token"].as_str().unwrap();
        let api_key = setup_json["user"]["api_key"].as_str().unwrap();

        // 3. System overview shows 4 default managed libraries (Book, Comic, Music, Video)
        let ov_res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/system/overview")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(ov_res.status(), StatusCode::OK);
        let ov_json: serde_json::Value =
            serde_json::from_slice(&to_bytes(ov_res.into_body(), usize::MAX).await.unwrap())
                .unwrap();
        assert_eq!(ov_json["libraries"].as_array().unwrap().len(), 4);

        // 4. Verify all 4 domain REST endpoints + OpenSubsonic + OPDS Book & Comic routers are mounted
        for path in [
            "/api/v1/book/books",
            "/api/v1/comic/home",
            "/api/v1/music/home",
            "/api/v1/video/home",
            "/opds/book/v1.2",
            "/opds/book/v2",
            "/opds/comic/v1.2",
        ] {
            let res = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(path)
                        .header(header::AUTHORIZATION, format!("Bearer {token}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::OK, "failed endpoint: {path}");
        }

        let opds_key_res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/opds/book/v1.2?api_key={api_key}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(opds_key_res.status(), StatusCode::OK);

        let sub_res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/rest/ping.view?apiKey={api_key}&f=json"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(sub_res.status(), StatusCode::OK);

        // 5. Verify SSE endpoint auth (401 without token, 200 with ?token= and Authorization header)
        let unauth_sse = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/events/stream")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauth_sse.status(), StatusCode::UNAUTHORIZED);

        let header_sse = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/events/stream")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(header_sse.status(), StatusCode::OK);
        assert!(
            header_sse
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .is_some_and(|ct| ct.starts_with("text/event-stream"))
        );
        assert_eq!(
            header_sse
                .headers()
                .get("x-accel-buffering")
                .and_then(|v| v.to_str().ok()),
            Some("no")
        );
        drop(header_sse);

        // 6. Verify SSE ?token= query auth + live SystemEvent JSON frame delivery
        let query_sse = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/events/stream?token={token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(query_sse.status(), StatusCode::OK);
        assert!(
            query_sse
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .is_some_and(|ct| ct.starts_with("text/event-stream"))
        );

        use futures_core::Stream;
        use std::future::poll_fn;
        use std::pin::Pin;
        use std::time::Duration;
        use xmedia_core::application::ports::{RealtimeEventPort, SystemEvent};
        use xmedia_core::domain::library::MediaKind;

        app_instance.events.publish(SystemEvent::MediaChanged {
            media_kind: MediaKind::Book,
        });

        let mut data_stream = query_sse.into_body().into_data_stream();
        let chunk = tokio::time::timeout(
            Duration::from_secs(2),
            poll_fn(|cx| Pin::new(&mut data_stream).poll_next(cx)),
        )
        .await
        .expect("timed out waiting for SSE event frame")
        .expect("SSE stream ended unexpectedly")
        .expect("SSE body chunk error");

        let raw_frame = String::from_utf8(chunk.to_vec()).unwrap();
        let data_line = raw_frame
            .lines()
            .find_map(|line| line.strip_prefix("data:"))
            .map(str::trim)
            .expect("SSE frame must contain a data: line");
        let parsed: serde_json::Value = serde_json::from_str(data_line).unwrap();
        assert_eq!(parsed["event"], "media_changed");
        assert_eq!(parsed["media_kind"], "book");
    }

    #[tokio::test]
    async fn disabled_permissive_cors_and_app_path_spa_exclusion() {
        let tmp = TempDir::new().unwrap();
        let db_path = tmp.path().join("xmedia_nocors.db");
        let app_instance = XMediaApp::connect_and_init_with_cors(
            XMediaConfig {
                data_dir: tmp.path().to_path_buf(),
                database_url: format!("sqlite:{}", db_path.display()),
                secret: "no-cors-test-secret".into(),
                jwt_ttl_secs: 3600,
                ffmpeg_bin: "__missing_ffmpeg__".into(),
                external_web_dir: None,
            },
            false,
        )
        .await
        .unwrap();

        let app = app_instance.router;

        let cors_res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/system/capabilities")
                    .header(header::ORIGIN, "https://evil.example")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(cors_res.status(), StatusCode::OK);
        assert!(
            cors_res
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .is_none(),
            "Access-Control-Allow-Origin must not be set when permissive CORS is disabled"
        );

        for path in ["/_app", "/_app/", "/_app/unknown"] {
            let res = app
                .clone()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(
                res.status(),
                StatusCode::NOT_FOUND,
                "SPA fallback must not serve HTML for {path}"
            );
        }
    }
}

pub mod gateway;
pub mod seeds;
pub mod static_files;

use std::collections::HashMap;
use std::net::SocketAddr;
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

use gateway::{FfmpegStatusCache, GatewayState, SseConnectionLimiter, core_api_router};

pub const DEFAULT_DEV_SECRET: &str = "xmedia-change-this-secret-in-production";

pub fn resolve_allow_private_ips_for_testing(bind: SocketAddr, env_val: Option<&str>) -> bool {
    let requested = env_val.map(str::trim) == Some("1");
    if !requested {
        return false;
    }
    if !bind.ip().is_loopback() {
        tracing::warn!(
            %bind,
            "ignoring XMEDIA_ALLOW_PRIVATE_IPS_FOR_TESTING=1 because bind address is not loopback"
        );
        return false;
    }
    tracing::warn!(
        %bind,
        "XMEDIA_ALLOW_PRIVATE_IPS_FOR_TESTING=1 enabled on loopback bind; SSRF private-IP checks are relaxed for testing"
    );
    true
}

#[derive(Debug, Clone)]
pub struct XMediaConfig {
    pub data_dir: PathBuf,
    pub database_url: String,
    pub secret: String,
    pub jwt_ttl_secs: u64,
    pub ffmpeg_bin: String,
    pub external_web_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Default)]
pub struct XMediaInitOptions {
    pub allow_permissive_cors: bool,
    pub cors_allow_origins: Vec<String>,
    pub allow_private_ips: bool,
    pub seed_sources: bool,
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
        Self::connect_and_init_with_options(cfg, XMediaInitOptions::default()).await
    }

    pub async fn connect_and_init_with_cors(
        cfg: XMediaConfig,
        allow_permissive_cors: bool,
    ) -> anyhow::Result<Self> {
        Self::connect_and_init_with_options(
            cfg,
            XMediaInitOptions {
                allow_permissive_cors,
                ..Default::default()
            },
        )
        .await
    }

    pub async fn connect_and_init_with_cors_origins(
        cfg: XMediaConfig,
        allow_permissive_cors: bool,
        cors_allow_origins: &[String],
    ) -> anyhow::Result<Self> {
        Self::connect_and_init_with_options(
            cfg,
            XMediaInitOptions {
                allow_permissive_cors,
                cors_allow_origins: cors_allow_origins.to_vec(),
                ..Default::default()
            },
        )
        .await
    }

    pub async fn connect_and_init_with_options(
        cfg: XMediaConfig,
        opts: XMediaInitOptions,
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
        let tokens = Arc::new(
            JwtTokenService::try_new(&cfg.secret, cfg.jwt_ttl_secs)
                .map_err(|e| anyhow::anyhow!(e.to_string()))?,
        );
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
            let book_services = xmedia_book::init_book_module_with_allow_private_ips(
                db.clone(),
                &cfg.data_dir,
                libraries_repo.clone(),
                &libraries,
                events.clone(),
                opts.allow_private_ips,
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
            let comic_services = xmedia_comic::init_comic_module_with_allow_private_ips(
                db.clone(),
                &cfg.data_dir,
                libraries_repo.clone(),
                &libraries,
                events.clone(),
                opts.allow_private_ips,
            )
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
            if opts.seed_sources {
                let report = seeds::seed_comic_sources(&comic_services)
                    .await
                    .map_err(|e| anyhow::anyhow!(e.to_string()))?;
                tracing::info!(
                    imported = report.imported_count,
                    total = report.sources.len(),
                    "seeded curated comic sources"
                );
            }
            libraries
                .ensure_managed_library(MediaKind::Comic)
                .await
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;

            let comic_state = xmedia_comic::http::state::ComicHttpState::new(
                auth.clone(),
                libraries.clone(),
                comic_services,
            );
            let comic_router = xmedia_comic::http::rest_router(comic_state.clone())
                .merge(seeds::comic_seed_router(comic_state.clone()));
            api_v1 = api_v1.nest("/comic", comic_router);
            root_protocols =
                root_protocols.nest("/opds/comic", xmedia_comic::http::opds_router(comic_state));
        }

        #[cfg(feature = "music")]
        {
            enabled_modules.push(MediaKind::Music);
            let music_services = xmedia_music::init_music_module_with_allow_private_ips(
                db.clone(),
                &cfg.data_dir,
                ffmpeg.clone(),
                libraries_repo.clone(),
                &libraries,
                events.clone(),
                opts.allow_private_ips,
            )
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
            if opts.seed_sources {
                let report = seeds::seed_music_sources(&music_services)
                    .await
                    .map_err(|e| anyhow::anyhow!(e.to_string()))?;
                tracing::info!(
                    imported = report.imported_count,
                    total = report.sources.len(),
                    "seeded curated music sources"
                );
            }
            libraries
                .ensure_managed_library(MediaKind::Music)
                .await
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;

            let music_state = xmedia_music::http::state::MusicHttpState::new(
                auth.clone(),
                libraries.clone(),
                music_services,
            );
            let music_router = xmedia_music::http::rest_router(music_state.clone())
                .merge(seeds::music_seed_router(music_state.clone()));
            api_v1 = api_v1.nest("/music", music_router);
            root_protocols =
                root_protocols.nest("/rest", xmedia_music::http::subsonic_router(music_state));
        }

        #[cfg(feature = "video")]
        {
            enabled_modules.push(MediaKind::Video);
            let video_services = xmedia_video::init_video_module_with_allow_private_ips(
                db.clone(),
                &cfg.data_dir,
                ffmpeg.clone(),
                libraries_repo.clone(),
                &libraries,
                events.clone(),
                opts.allow_private_ips,
            )
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
            if opts.seed_sources {
                let report = seeds::seed_video_sources(&video_services)
                    .await
                    .map_err(|e| anyhow::anyhow!(e.to_string()))?;
                tracing::info!(
                    imported = report.imported_count,
                    total = report.sources.len(),
                    "seeded curated video sources"
                );
            }
            libraries
                .ensure_managed_library(MediaKind::Video)
                .await
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;

            let video_state = xmedia_video::http::state::VideoHttpState::new(
                auth.clone(),
                libraries.clone(),
                video_services,
            );
            let video_router = xmedia_video::http::rest_router(video_state.clone())
                .merge(seeds::video_seed_router(video_state));
            api_v1 = api_v1.nest("/video", video_router);
        }

        let _ = (db, opts.allow_private_ips, opts.seed_sources);

        let gateway_state = GatewayState {
            auth: auth.clone(),
            libraries: libraries.clone(),
            libraries_repo,
            events: events.clone(),
            sse_limiter: SseConnectionLimiter::new(),
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
        if opts.allow_permissive_cors {
            router = router.layer(CorsLayer::permissive());
        } else if !opts.cors_allow_origins.is_empty() {
            let parsed_origins: Vec<HeaderValue> = opts
                .cors_allow_origins
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

#[cfg(test)]
mod unit_tests {
    use std::net::SocketAddr;

    use crate::resolve_allow_private_ips_for_testing;

    #[test]
    fn resolve_allow_private_ips_for_testing_gates_strictly_on_loopback() {
        let loopback_v4: SocketAddr = "127.0.0.1:8090".parse().unwrap();
        let loopback_v6: SocketAddr = "[::1]:8090".parse().unwrap();
        let wildcard_v4: SocketAddr = "0.0.0.0:8090".parse().unwrap();
        let wildcard_v6: SocketAddr = "[::]:8090".parse().unwrap();
        let lan_v4: SocketAddr = "192.168.1.5:8090".parse().unwrap();

        for env_val in [None, Some(""), Some("0"), Some("true"), Some("yes")] {
            assert!(!resolve_allow_private_ips_for_testing(loopback_v4, env_val));
            assert!(!resolve_allow_private_ips_for_testing(loopback_v6, env_val));
            assert!(!resolve_allow_private_ips_for_testing(wildcard_v4, env_val));
            assert!(!resolve_allow_private_ips_for_testing(wildcard_v6, env_val));
            assert!(!resolve_allow_private_ips_for_testing(lan_v4, env_val));
        }

        assert!(resolve_allow_private_ips_for_testing(
            loopback_v4,
            Some("1")
        ));
        assert!(resolve_allow_private_ips_for_testing(
            loopback_v4,
            Some(" 1 ")
        ));
        assert!(resolve_allow_private_ips_for_testing(
            loopback_v6,
            Some("1")
        ));

        assert!(!resolve_allow_private_ips_for_testing(
            wildcard_v4,
            Some("1")
        ));
        assert!(!resolve_allow_private_ips_for_testing(
            wildcard_v6,
            Some("1")
        ));
        assert!(!resolve_allow_private_ips_for_testing(lan_v4, Some("1")));
    }
}

#[cfg(all(test, feature = "comic", feature = "music", feature = "video"))]
mod gateway_seed_tests {
    use axum::Router;
    use axum::body::{Body, to_bytes};
    use axum::http::header::{AUTHORIZATION, CONTENT_TYPE};
    use axum::http::{Request, StatusCode};
    use axum::routing::get;
    use tempfile::TempDir;
    use tokio::net::TcpListener;
    use tower::ServiceExt;
    use xmedia_core::application::dto::{CreateUserInput, SetupAdminInput};
    use xmedia_core::domain::user::UserRole;

    use crate::{XMediaApp, XMediaConfig, XMediaInitOptions};

    const PNG_1X1: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    fn make_cfg(dir: &TempDir) -> XMediaConfig {
        let db_path = dir.path().join("xmedia.db");
        XMediaConfig {
            data_dir: dir.path().to_path_buf(),
            database_url: format!("sqlite:{}", db_path.display()),
            secret: "gateway-seed-test-secret-0123456789".into(),
            jwt_ttl_secs: 3600,
            ffmpeg_bin: "__missing_ffmpeg__".into(),
            external_web_dir: None,
        }
    }

    async fn setup_admin_and_member(app_instance: &XMediaApp) -> (String, String) {
        let admin_sess = app_instance
            .auth
            .setup_initial_admin(SetupAdminInput {
                username: "admin".into(),
                display_name: Some("Admin".into()),
                password: "adminpassword123".into(),
            })
            .await
            .unwrap();
        let admin = app_instance
            .auth
            .authenticate_jwt(&admin_sess.token)
            .await
            .unwrap();
        app_instance
            .auth
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
        let member_sess = app_instance
            .auth
            .login("member", "memberpassword123")
            .await
            .unwrap();
        (admin_sess.token, member_sess.token)
    }

    async fn send_req(
        router: &Router,
        method: &str,
        uri: &str,
        token: Option<&str>,
        body: Body,
        content_type: Option<&str>,
    ) -> (StatusCode, serde_json::Value) {
        let mut builder = Request::builder().method(method).uri(uri);
        if let Some(t) = token {
            builder = builder.header(AUTHORIZATION, format!("Bearer {t}"));
        }
        if let Some(ct) = content_type {
            builder = builder.header(CONTENT_TYPE, ct);
        }
        let resp = router
            .clone()
            .oneshot(builder.body(body).unwrap())
            .await
            .unwrap();
        let status = resp.status();
        let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json = if bytes.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or_else(|_| {
                serde_json::Value::String(String::from_utf8_lossy(&bytes).into_owned())
            })
        };
        (status, json)
    }

    #[tokio::test]
    async fn import_seeds_authz_bodies_counts_idempotency_and_concurrency() {
        let tmp = TempDir::new().unwrap();
        let app_instance = XMediaApp::connect_and_init(make_cfg(&tmp)).await.unwrap();
        let (admin_token, member_token) = setup_admin_and_member(&app_instance).await;
        let app = &app_instance.router;

        // Fresh startup must have 0 online sources
        for domain in ["comic", "music", "video"] {
            let (st, json) = send_req(
                app,
                "GET",
                &format!("/api/v1/{domain}/sources"),
                Some(&admin_token),
                Body::empty(),
                None,
            )
            .await;
            assert_eq!(st, StatusCode::OK);
            assert_eq!(json.as_array().unwrap().len(), 0);
        }

        // Unauthenticated -> 401; non-admin -> 403 forbidden
        for domain in ["comic", "music", "video"] {
            let uri = format!("/api/v1/{domain}/sources/import-seeds");
            let (st_unauth, _) = send_req(app, "POST", &uri, None, Body::empty(), None).await;
            assert_eq!(st_unauth, StatusCode::UNAUTHORIZED);

            let (st_member, json_member) = send_req(
                app,
                "POST",
                &uri,
                Some(&member_token),
                Body::from("{}"),
                Some("application/json"),
            )
            .await;
            assert_eq!(st_member, StatusCode::FORBIDDEN);
            assert_eq!(json_member["error"]["code"], "forbidden");
        }

        // Malformed non-empty bodies -> 400 Bad Request
        for bad_body in ["not json", "[1,2]", "123", "null", "\"text\""] {
            let (st_bad, json_bad) = send_req(
                app,
                "POST",
                "/api/v1/comic/sources/import-seeds",
                Some(&admin_token),
                Body::from(bad_body),
                Some("application/json"),
            )
            .await;
            assert_eq!(st_bad, StatusCode::BAD_REQUEST, "body={bad_body}");
            assert_eq!(json_bad["error"]["code"], "validation_error");
        }

        // Empty body (no Content-Type) on comic -> 200 OK, 5 seeds
        let (st_comic, json_comic) = send_req(
            app,
            "POST",
            "/api/v1/comic/sources/import-seeds",
            Some(&admin_token),
            Body::empty(),
            None,
        )
        .await;
        assert_eq!(st_comic, StatusCode::OK);
        assert_eq!(json_comic["imported_count"], 5);
        assert_eq!(json_comic["updated_count"], 0);
        assert_eq!(json_comic["sources"].as_array().unwrap().len(), 5);

        // JSON {} body on music (4 seeds) and video (6 seeds)
        for (domain, expected_count) in [("music", 4usize), ("video", 6usize)] {
            let (st, json) = send_req(
                app,
                "POST",
                &format!("/api/v1/{domain}/sources/import-seeds"),
                Some(&admin_token),
                Body::from("{}"),
                Some("application/json"),
            )
            .await;
            assert_eq!(st, StatusCode::OK);
            assert_eq!(json["imported_count"], expected_count);
            assert_eq!(json["updated_count"], 0);
            assert_eq!(json["sources"].as_array().unwrap().len(), expected_count);
        }

        // Second call is idempotent (imported_count == 0, UUIDs preserved)
        for (domain, expected_count) in [("comic", 5usize), ("music", 4usize), ("video", 6usize)] {
            let (_, before) = send_req(
                app,
                "GET",
                &format!("/api/v1/{domain}/sources"),
                Some(&admin_token),
                Body::empty(),
                None,
            )
            .await;
            let before_ids: Vec<&str> = before
                .as_array()
                .unwrap()
                .iter()
                .map(|s| s["id"].as_str().unwrap())
                .collect();

            let (st2, rep2) = send_req(
                app,
                "POST",
                &format!("/api/v1/{domain}/sources/import-seeds"),
                Some(&admin_token),
                Body::from("{}"),
                Some("application/json"),
            )
            .await;
            assert_eq!(st2, StatusCode::OK);
            assert_eq!(rep2["imported_count"], 0);
            assert_eq!(rep2["updated_count"], 0);
            assert_eq!(rep2["sources"].as_array().unwrap().len(), expected_count);

            let (_, after) = send_req(
                app,
                "GET",
                &format!("/api/v1/{domain}/sources"),
                Some(&admin_token),
                Body::empty(),
                None,
            )
            .await;
            let after_ids: Vec<&str> = after
                .as_array()
                .unwrap()
                .iter()
                .map(|s| s["id"].as_str().unwrap())
                .collect();
            assert_eq!(after_ids, before_ids);
        }

        // Concurrent import-seeds on a fresh DB must not insert duplicate rows
        let tmp_conc = TempDir::new().unwrap();
        let conc_app = XMediaApp::connect_and_init(make_cfg(&tmp_conc))
            .await
            .unwrap();
        let (conc_admin, _) = setup_admin_and_member(&conc_app).await;
        let r = &conc_app.router;
        let (r1, r2, r3, r4) = tokio::join!(
            send_req(
                r,
                "POST",
                "/api/v1/comic/sources/import-seeds",
                Some(&conc_admin),
                Body::empty(),
                None,
            ),
            send_req(
                r,
                "POST",
                "/api/v1/comic/sources/import-seeds",
                Some(&conc_admin),
                Body::from("{}"),
                Some("application/json"),
            ),
            send_req(
                r,
                "POST",
                "/api/v1/comic/sources/import-seeds",
                Some(&conc_admin),
                Body::empty(),
                None,
            ),
            send_req(
                r,
                "POST",
                "/api/v1/comic/sources/import-seeds",
                Some(&conc_admin),
                Body::from("{}"),
                Some("application/json"),
            ),
        );
        for (st, _) in [&r1, &r2, &r3, &r4] {
            assert_eq!(*st, StatusCode::OK);
        }
        let total_imported: u64 = [&r1, &r2, &r3, &r4]
            .iter()
            .map(|(_, j)| j["imported_count"].as_u64().unwrap())
            .sum();
        assert_eq!(total_imported, 5);
        let (_, listed) = send_req(
            r,
            "GET",
            "/api/v1/comic/sources",
            Some(&conc_admin),
            Body::empty(),
            None,
        )
        .await;
        assert_eq!(listed.as_array().unwrap().len(), 5);
    }

    #[tokio::test]
    async fn import_seeds_t2_f11_02_non_overwrite_and_db_reconnect_persistence() {
        let tmp = TempDir::new().unwrap();
        let cfg = make_cfg(&tmp);
        let expected_module_count: usize;
        let disabled_comic_id: String;
        let deleted_music_id: String;
        let video_var_id: String;
        let large_var = "V".repeat(64 * 1024);

        {
            let app_instance = XMediaApp::connect_and_init(cfg.clone()).await.unwrap();
            expected_module_count = app_instance.enabled_modules.len();
            let (admin_token, _) = setup_admin_and_member(&app_instance).await;
            let app = &app_instance.router;

            for domain in ["comic", "music", "video"] {
                let (st, _) = send_req(
                    app,
                    "POST",
                    &format!("/api/v1/{domain}/sources/import-seeds"),
                    Some(&admin_token),
                    Body::from("{}"),
                    Some("application/json"),
                )
                .await;
                assert_eq!(st, StatusCode::OK);
            }

            let (_, comic_list) = send_req(
                app,
                "GET",
                "/api/v1/comic/sources",
                Some(&admin_token),
                Body::empty(),
                None,
            )
            .await;
            let comic_arr = comic_list.as_array().unwrap();
            let deleted_comic_id = comic_arr[0]["id"].as_str().unwrap().to_string();
            disabled_comic_id = comic_arr[1]["id"].as_str().unwrap().to_string();

            // T2-F11-02: Delete 1 seed and disable/mutate 1 seed
            let (st_del, _) = send_req(
                app,
                "DELETE",
                &format!("/api/v1/comic/sources/{deleted_comic_id}"),
                Some(&admin_token),
                Body::empty(),
                None,
            )
            .await;
            assert!(st_del.is_success());

            let (st_patch, _) = send_req(
                app,
                "PATCH",
                &format!("/api/v1/comic/sources/{disabled_comic_id}"),
                Some(&admin_token),
                Body::from(
                    serde_json::json!({
                        "enabled": false,
                        "weight": 99,
                        "group_name": "Persisted"
                    })
                    .to_string(),
                ),
                Some("application/json"),
            )
            .await;
            assert_eq!(st_patch, StatusCode::OK);

            // Re-call import-seeds -> restores deleted seed (1), preserves disabled seed untouched
            let (st_reimp, reimp) = send_req(
                app,
                "POST",
                "/api/v1/comic/sources/import-seeds",
                Some(&admin_token),
                Body::from("{}"),
                Some("application/json"),
            )
            .await;
            assert_eq!(st_reimp, StatusCode::OK);
            assert_eq!(reimp["imported_count"], 1);
            assert_eq!(reimp["updated_count"], 0);
            assert_eq!(reimp["sources"].as_array().unwrap().len(), 5);

            // Delete 1 music source & set 64 KiB variable on 1 video source
            let (_, music_list) = send_req(
                app,
                "GET",
                "/api/v1/music/sources",
                Some(&admin_token),
                Body::empty(),
                None,
            )
            .await;
            deleted_music_id = music_list[0]["id"].as_str().unwrap().to_string();
            let (st_mdel, _) = send_req(
                app,
                "DELETE",
                &format!("/api/v1/music/sources/{deleted_music_id}"),
                Some(&admin_token),
                Body::empty(),
                None,
            )
            .await;
            assert!(st_mdel.is_success());

            let (_, video_list) = send_req(
                app,
                "GET",
                "/api/v1/video/sources",
                Some(&admin_token),
                Body::empty(),
                None,
            )
            .await;
            video_var_id = video_list[0]["id"].as_str().unwrap().to_string();
            let (st_vpatch, _) = send_req(
                app,
                "PATCH",
                &format!("/api/v1/video/sources/{video_var_id}"),
                Some(&admin_token),
                Body::from(serde_json::json!({ "variable": large_var }).to_string()),
                Some("application/json"),
            )
            .await;
            assert_eq!(st_vpatch, StatusCode::OK);
        }

        // Reconnect fresh XMediaApp to the exact same SQLite DB & data_dir
        let reopened = XMediaApp::connect_and_init(cfg).await.unwrap();
        let app2 = &reopened.router;
        let (st_login, login_json) = send_req(
            app2,
            "POST",
            "/api/v1/auth/login",
            None,
            Body::from(
                serde_json::json!({ "username": "admin", "password": "adminpassword123" })
                    .to_string(),
            ),
            Some("application/json"),
        )
        .await;
        assert_eq!(st_login, StatusCode::OK);
        let token2 = login_json["token"].as_str().unwrap();

        // Default managed libraries intact
        let (st_ov, ov_json) = send_req(
            app2,
            "GET",
            "/api/v1/system/overview",
            Some(token2),
            Body::empty(),
            None,
        )
        .await;
        assert_eq!(st_ov, StatusCode::OK);
        assert_eq!(
            ov_json["libraries"].as_array().unwrap().len(),
            expected_module_count
        );

        // Comic mutations & restored seed persisted
        let (_, comic_after) = send_req(
            app2,
            "GET",
            "/api/v1/comic/sources",
            Some(token2),
            Body::empty(),
            None,
        )
        .await;
        let comic_arr2 = comic_after.as_array().unwrap();
        assert_eq!(comic_arr2.len(), 5);
        let disabled_entry = comic_arr2
            .iter()
            .find(|s| s["id"].as_str() == Some(&disabled_comic_id))
            .expect("disabled comic seed must persist");
        assert_eq!(disabled_entry["enabled"], false);
        assert_eq!(disabled_entry["weight"], 99);
        assert_eq!(disabled_entry["group_name"], "Persisted");

        // Deleted music source remains deleted
        let (_, music_after) = send_req(
            app2,
            "GET",
            "/api/v1/music/sources",
            Some(token2),
            Body::empty(),
            None,
        )
        .await;
        let music_arr2 = music_after.as_array().unwrap();
        assert_eq!(music_arr2.len(), 3);
        assert!(
            music_arr2
                .iter()
                .all(|s| s["id"].as_str() != Some(&deleted_music_id))
        );

        // 64 KiB variable persisted
        let (st_var, var_json) = send_req(
            app2,
            "GET",
            &format!("/api/v1/video/sources/{video_var_id}/variable"),
            Some(token2),
            Body::empty(),
            None,
        )
        .await;
        assert_eq!(st_var, StatusCode::OK);
        assert_eq!(var_json["variable"].as_str().unwrap(), large_var);
    }

    #[tokio::test]
    async fn loopback_allow_private_ips_gating_through_gateway() {
        let mock_router = Router::new()
            .route(
                "/search",
                get(|| async {
                    axum::Json(serde_json::json!({
                        "items": [{
                            "title": "Local Mock Comic",
                            "url": "/comic/1",
                            "cover": "/cover.png"
                        }]
                    }))
                }),
            )
            .route(
                "/cover.png",
                get(|| async { ([(CONTENT_TYPE, "image/png")], PNG_1X1.to_vec()) }),
            );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_handle = tokio::spawn(async move {
            axum::serve(listener, mock_router).await.unwrap();
        });

        let base_url = format!("http://{addr}");
        let comic_source_json = serde_json::json!({
            "bookSourceName": "Loopback Mock Comic",
            "bookSourceUrl": base_url,
            "bookSourceType": 2,
            "searchUrl": format!("{base_url}/search?q={{{{key}}}}"),
            "ruleSearch": {
                "bookList": "$.items[*]",
                "name": "$.title",
                "bookUrl": "$.url",
                "coverUrl": "$.cover"
            }
        })
        .to_string();

        // 1. Default allow_private_ips == false blocks loopback requests
        {
            let tmp_blocked = TempDir::new().unwrap();
            let app_blocked = XMediaApp::connect_and_init(make_cfg(&tmp_blocked))
                .await
                .unwrap();
            let (token, _) = setup_admin_and_member(&app_blocked).await;

            let (st_imp, imp_json) = send_req(
                &app_blocked.router,
                "POST",
                "/api/v1/comic/sources/import",
                Some(&token),
                Body::from(serde_json::json!({ "raw_json": comic_source_json }).to_string()),
                Some("application/json"),
            )
            .await;
            assert_eq!(st_imp, StatusCode::OK);
            let source_id = imp_json["sources"][0]["id"].as_str().unwrap();

            let (st_search, search_json) = send_req(
                &app_blocked.router,
                "POST",
                "/api/v1/comic/online/search",
                Some(&token),
                Body::from(serde_json::json!({ "keyword": "test" }).to_string()),
                Some("application/json"),
            )
            .await;
            assert_eq!(st_search, StatusCode::OK);
            assert!(search_json["results"].as_array().unwrap().is_empty());
            assert!(!search_json["errors"].as_array().unwrap().is_empty());

            let (st_proxy, _) = send_req(
                &app_blocked.router,
                "GET",
                &format!(
                    "/api/v1/comic/online/image-proxy?url={base_url}/cover.png&source_id={source_id}"
                ),
                Some(&token),
                Body::empty(),
                None,
            )
            .await;
            assert!(
                st_proxy == StatusCode::BAD_REQUEST || st_proxy == StatusCode::FORBIDDEN,
                "expected SSRF rejection, got {st_proxy}"
            );

            for proxy_path in [
                format!("/api/v1/music/online/cover-proxy?url={base_url}/cover.png"),
                format!("/api/v1/video/online/poster-proxy?url={base_url}/cover.png"),
            ] {
                let (st_dom_proxy, _) = send_req(
                    &app_blocked.router,
                    "GET",
                    &proxy_path,
                    Some(&token),
                    Body::empty(),
                    None,
                )
                .await;
                assert!(
                    st_dom_proxy == StatusCode::BAD_REQUEST
                        || st_dom_proxy == StatusCode::FORBIDDEN,
                    "expected SSRF rejection on {proxy_path}, got {st_dom_proxy}"
                );
            }
        }

        // 2. Opt-in allow_private_ips == true allows loopback mock server
        {
            let tmp_allowed = TempDir::new().unwrap();
            let app_allowed = XMediaApp::connect_and_init_with_options(
                make_cfg(&tmp_allowed),
                XMediaInitOptions {
                    allow_private_ips: true,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
            let (token, _) = setup_admin_and_member(&app_allowed).await;

            let (st_imp, imp_json) = send_req(
                &app_allowed.router,
                "POST",
                "/api/v1/comic/sources/import",
                Some(&token),
                Body::from(serde_json::json!({ "raw_json": comic_source_json }).to_string()),
                Some("application/json"),
            )
            .await;
            assert_eq!(st_imp, StatusCode::OK);
            let source_id = imp_json["sources"][0]["id"].as_str().unwrap();

            let (st_search, search_json) = send_req(
                &app_allowed.router,
                "POST",
                "/api/v1/comic/online/search",
                Some(&token),
                Body::from(serde_json::json!({ "keyword": "test" }).to_string()),
                Some("application/json"),
            )
            .await;
            assert_eq!(st_search, StatusCode::OK);
            assert_eq!(search_json["results"].as_array().unwrap().len(), 1);
            assert_eq!(search_json["results"][0]["title"], "Local Mock Comic");

            let proxy_resp = app_allowed
                .router
                .clone()
                .oneshot(
                    Request::builder()
                        .method("GET")
                        .uri(format!(
                            "/api/v1/comic/online/image-proxy?url={base_url}/cover.png&source_id={source_id}"
                        ))
                        .header(AUTHORIZATION, format!("Bearer {token}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(proxy_resp.status(), StatusCode::OK);
            assert_eq!(
                proxy_resp
                    .headers()
                    .get(CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok()),
                Some("image/png")
            );

            for proxy_path in [
                format!("/api/v1/music/online/cover-proxy?url={base_url}/cover.png"),
                format!("/api/v1/video/online/poster-proxy?url={base_url}/cover.png"),
            ] {
                let dom_resp = app_allowed
                    .router
                    .clone()
                    .oneshot(
                        Request::builder()
                            .method("GET")
                            .uri(&proxy_path)
                            .header(AUTHORIZATION, format!("Bearer {token}"))
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(
                    dom_resp.status(),
                    StatusCode::OK,
                    "expected 200 OK on {proxy_path}"
                );
                assert_eq!(
                    dom_resp
                        .headers()
                        .get(CONTENT_TYPE)
                        .and_then(|v| v.to_str().ok()),
                    Some("image/png")
                );
            }
        }

        server_handle.abort();
    }

    #[tokio::test]
    async fn startup_seed_sources_option_is_opt_in_and_idempotent() {
        let tmp = TempDir::new().unwrap();
        let cfg = make_cfg(&tmp);

        // Default startup: seed_sources == false -> 0 sources
        {
            let app_default = XMediaApp::connect_and_init(cfg.clone()).await.unwrap();
            let (token, _) = setup_admin_and_member(&app_default).await;
            for domain in ["comic", "music", "video"] {
                let (st, list) = send_req(
                    &app_default.router,
                    "GET",
                    &format!("/api/v1/{domain}/sources"),
                    Some(&token),
                    Body::empty(),
                    None,
                )
                .await;
                assert_eq!(st, StatusCode::OK);
                assert_eq!(list.as_array().unwrap().len(), 0);
            }
        }

        // Reconnect with seed_sources: true -> populates all domain seeds
        let disabled_id: String;
        {
            let app_seeded = XMediaApp::connect_and_init_with_options(
                cfg.clone(),
                XMediaInitOptions {
                    seed_sources: true,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
            let (_, login_json) = send_req(
                &app_seeded.router,
                "POST",
                "/api/v1/auth/login",
                None,
                Body::from(
                    serde_json::json!({ "username": "admin", "password": "adminpassword123" })
                        .to_string(),
                ),
                Some("application/json"),
            )
            .await;
            let token = login_json["token"].as_str().unwrap();

            for (domain, expected) in [("comic", 5usize), ("music", 4usize), ("video", 6usize)] {
                let (st, list) = send_req(
                    &app_seeded.router,
                    "GET",
                    &format!("/api/v1/{domain}/sources"),
                    Some(token),
                    Body::empty(),
                    None,
                )
                .await;
                assert_eq!(st, StatusCode::OK);
                assert_eq!(list.as_array().unwrap().len(), expected);
            }

            let (_, comic_list) = send_req(
                &app_seeded.router,
                "GET",
                "/api/v1/comic/sources",
                Some(token),
                Body::empty(),
                None,
            )
            .await;
            let del_id = comic_list[0]["id"].as_str().unwrap();
            disabled_id = comic_list[1]["id"].as_str().unwrap().to_string();

            send_req(
                &app_seeded.router,
                "DELETE",
                &format!("/api/v1/comic/sources/{del_id}"),
                Some(token),
                Body::empty(),
                None,
            )
            .await;
            send_req(
                &app_seeded.router,
                "PATCH",
                &format!("/api/v1/comic/sources/{disabled_id}"),
                Some(token),
                Body::from(serde_json::json!({ "enabled": false }).to_string()),
                Some("application/json"),
            )
            .await;
        }

        // Second startup with seed_sources: true -> restores missing, preserves disabled state
        let app_seeded_again = XMediaApp::connect_and_init_with_options(
            cfg,
            XMediaInitOptions {
                seed_sources: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let (_, login_json2) = send_req(
            &app_seeded_again.router,
            "POST",
            "/api/v1/auth/login",
            None,
            Body::from(
                serde_json::json!({ "username": "admin", "password": "adminpassword123" })
                    .to_string(),
            ),
            Some("application/json"),
        )
        .await;
        let token2 = login_json2["token"].as_str().unwrap();

        let (_, comic_after) = send_req(
            &app_seeded_again.router,
            "GET",
            "/api/v1/comic/sources",
            Some(token2),
            Body::empty(),
            None,
        )
        .await;
        let arr = comic_after.as_array().unwrap();
        assert_eq!(arr.len(), 5);
        let disabled_entry = arr
            .iter()
            .find(|s| s["id"].as_str() == Some(&disabled_id))
            .unwrap();
        assert_eq!(disabled_entry["enabled"], false);
    }

    #[tokio::test]
    async fn concurrent_import_seeds_and_manual_import_produce_no_duplicates() {
        use std::collections::HashSet;
        use std::sync::Arc;
        use tokio::sync::Barrier;

        let tmp = TempDir::new().unwrap();
        let app_instance = XMediaApp::connect_and_init(make_cfg(&tmp)).await.unwrap();
        let (admin_token, _) = setup_admin_and_member(&app_instance).await;
        let router = app_instance.router;

        let domains: [(&str, &str, usize); 3] = [
            ("comic", crate::seeds::COMIC_SEEDS_JSON, 5),
            ("music", crate::seeds::MUSIC_SEEDS_JSON, 4),
            ("video", crate::seeds::VIDEO_SEEDS_JSON, 6),
        ];

        // 4 import-seeds + 4 manual import per domain = 24 concurrent tasks across a barrier
        let tasks_per_kind = 4usize;
        let total_tasks = domains.len() * tasks_per_kind * 2;
        let barrier = Arc::new(Barrier::new(total_tasks));
        let mut handles = Vec::with_capacity(total_tasks);

        for (domain, seeds_json, _) in domains {
            let import_payload =
                Arc::new(serde_json::json!({ "raw_json": seeds_json }).to_string());
            for i in 0..(tasks_per_kind * 2) {
                let is_seed = i % 2 == 0;
                let r = router.clone();
                let tok = admin_token.clone();
                let bar = barrier.clone();
                let payload = import_payload.clone();
                handles.push(tokio::spawn(async move {
                    bar.wait().await;
                    let (uri, body, ct) = if is_seed {
                        (
                            format!("/api/v1/{domain}/sources/import-seeds"),
                            Body::from("{}"),
                            Some("application/json"),
                        )
                    } else {
                        (
                            format!("/api/v1/{domain}/sources/import"),
                            Body::from(payload.as_str().to_owned()),
                            Some("application/json"),
                        )
                    };
                    let (st, json) = send_req(&r, "POST", &uri, Some(&tok), body, ct).await;
                    (domain, st, json)
                }));
            }
        }

        let mut imported_sums: std::collections::HashMap<&'static str, u64> =
            std::collections::HashMap::new();
        for handle in handles {
            let (domain, st, json) = handle.await.unwrap();
            assert_eq!(st, StatusCode::OK, "domain={domain} body={json}");
            let imp = json["imported_count"].as_u64().unwrap();
            *imported_sums.entry(domain).or_insert(0) += imp;
        }

        for (domain, _, expected_count) in domains {
            assert_eq!(
                imported_sums.get(domain).copied().unwrap_or(0),
                expected_count as u64,
                "sum of imported_count for {domain} must equal {expected_count}"
            );

            let (st, listed) = send_req(
                &router,
                "GET",
                &format!("/api/v1/{domain}/sources"),
                Some(&admin_token),
                Body::empty(),
                None,
            )
            .await;
            assert_eq!(st, StatusCode::OK);
            let arr = listed.as_array().unwrap();
            assert_eq!(
                arr.len(),
                expected_count,
                "duplicate rows detected for {domain}: got {}",
                arr.len()
            );

            let unique_keys: HashSet<(String, String)> = arr
                .iter()
                .map(|item| {
                    (
                        item["name"].as_str().unwrap().to_string(),
                        item["source_url"].as_str().unwrap().to_string(),
                    )
                })
                .collect();
            assert_eq!(
                unique_keys.len(),
                expected_count,
                "expected {expected_count} unique (name, source_url) entries for {domain}"
            );
        }
    }

    #[tokio::test]
    async fn unified_gateway_theme_defaults_and_online_items_cross_domain_e2e() {
        let tmp = TempDir::new().unwrap();
        let app_instance = XMediaApp::connect_and_init(make_cfg(&tmp)).await.unwrap();
        let (admin_token, member_token) = setup_admin_and_member(&app_instance).await;
        let router = app_instance.router;

        // 1. New user receives Material You seed defaults via /auth/me
        let (st_me, me) = send_req(
            &router,
            "GET",
            "/api/v1/auth/me",
            Some(&member_token),
            Body::empty(),
            None,
        )
        .await;
        assert_eq!(st_me, StatusCode::OK);
        let member_id = me["id"].as_str().unwrap().to_string();
        assert_eq!(me["preferences"]["theme_mode"], "system");
        assert_eq!(me["preferences"]["light_preset"], "material-you-light");
        assert_eq!(me["preferences"]["dark_preset"], "material-you-dark");
        assert_eq!(me["preferences"]["accent_mode"], "seed");
        assert!(me["preferences"]["seed_color"].is_null());

        // 2. Legacy Catppuccin preferences with custom accent migrate cleanly to seed_color
        let (st_legacy, legacy_res) = send_req(
            &router,
            "PUT",
            "/api/v1/auth/preferences",
            Some(&member_token),
            Body::from(
                serde_json::json!({
                    "theme_mode": "light",
                    "light_preset": "catppuccin-latte",
                    "dark_preset": "catppuccin-mocha",
                    "accent_mode": "custom",
                    "accent": "mauve",
                    "custom_accent": "#1A2B3C"
                })
                .to_string(),
            ),
            Some("application/json"),
        )
        .await;
        assert_eq!(st_legacy, StatusCode::OK);
        assert_eq!(
            legacy_res["preferences"]["light_preset"],
            "material-you-light"
        );
        assert_eq!(
            legacy_res["preferences"]["dark_preset"],
            "material-you-dark"
        );
        assert_eq!(legacy_res["preferences"]["accent_mode"], "seed");
        assert_eq!(legacy_res["preferences"]["seed_color"], "#1a2b3c");

        // 3. Minimal payload (omitting deprecated fields) persists cleanly
        let (st_min, min_res) = send_req(
            &router,
            "PUT",
            "/api/v1/auth/preferences",
            Some(&member_token),
            Body::from(
                serde_json::json!({
                    "theme_mode": "dark",
                    "seed_color": "#006A6A"
                })
                .to_string(),
            ),
            Some("application/json"),
        )
        .await;
        assert_eq!(st_min, StatusCode::OK);
        assert_eq!(min_res["preferences"]["theme_mode"], "dark");
        assert_eq!(min_res["preferences"]["seed_color"], "#006a6a");
        assert_eq!(min_res["preferences"]["accent_mode"], "seed");

        // 4. Online items are mounted on comic/music/video and strictly isolated per domain
        for domain in ["comic", "music", "video"] {
            let (st_put, put_val) = send_req(
                &router,
                "PUT",
                &format!("/api/v1/{domain}/online/items"),
                Some(&member_token),
                Body::from(
                    serde_json::json!({
                        "source_kind": "remote",
                        "source_id": "remote-1",
                        "item_key": "shared-key",
                        "title": format!("Item in {domain}"),
                        "favorite": true,
                        "progress": { "position": 10.0, "duration": 100.0, "completed": false }
                    })
                    .to_string(),
                ),
                Some("application/json"),
            )
            .await;
            assert_eq!(st_put, StatusCode::OK, "domain={domain} body={put_val}");
            assert_eq!(put_val["media_kind"], domain);
            assert_eq!(put_val["availability"], "unverified");
        }

        for domain in ["comic", "music", "video"] {
            let (st_list, list_val) = send_req(
                &router,
                "GET",
                &format!("/api/v1/{domain}/online/items?view=continue"),
                Some(&member_token),
                Body::empty(),
                None,
            )
            .await;
            assert_eq!(st_list, StatusCode::OK);
            assert_eq!(list_val["total"], 1);
            assert_eq!(list_val["items"][0]["media_kind"], domain);
        }

        // 5. Deleting member succeeds and purges online items
        let (st_del_user, del_val) = send_req(
            &router,
            "DELETE",
            &format!("/api/v1/users/{member_id}"),
            Some(&admin_token),
            Body::empty(),
            None,
        )
        .await;
        assert_eq!(st_del_user, StatusCode::OK);
        assert_eq!(del_val["ok"], true);
    }
}

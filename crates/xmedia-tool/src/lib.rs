pub mod application;
pub mod domain;
pub mod http;
pub mod infra;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use axum::Router;
use axum::http::HeaderValue;
use axum::middleware;
use tower_http::compression::CompressionLayer;
use tower_http::compression::predicate::{DefaultPredicate, NotForContentType, Predicate};
use tower_http::cors::{AllowOrigin, Any, CorsLayer};
use tower_http::trace::TraceLayer;
use xmedia_core::application::ports::FontRepository;
use xmedia_core::infra::ffmpeg::FfmpegToolkit;
use xmedia_core::infra::fonts::FsFontStore;

use crate::application::auth::ToolAuthService;
use crate::application::scraping::ScrapingOrchestratorService;
use crate::application::settings::ToolSettingsService;
use crate::application::workbench::MediaWorkbenchService;
use crate::domain::ports::{
    HttpWebhookPort, MediaToolboxPort, ScrapeCachePort, ScraperProviderPort, ScraperTransportPort,
    WorkspaceFsPort,
};
use crate::http::auth::guard_tool_http_request;
use crate::http::routes::{tool_api_router, tool_fonts_api_router};
use crate::http::static_files::serve_tool_spa;
use crate::http::{ToolHttpState, ToolRequestSecurityPolicy};
use crate::infra::config_store::JsonFileConfigRepository;
use crate::infra::inspectors::build_default_inspectors;
use crate::infra::inspectors::toolbox::DefaultMediaToolbox;
use crate::infra::scrape_cache::ScrapeMetadataCache;
use crate::infra::scrapers::artwork_downloader::ReqwestScraperTransport;
use crate::infra::scrapers::build_default_scraper_providers;
use crate::infra::webhook_client::ReqwestWebhookClient;
use crate::infra::workspace_fs::LocalWorkspaceFs;

#[derive(Debug, Clone)]
pub struct XMediaToolConfig {
    pub data_dir: PathBuf,
    pub initial_roots: Vec<PathBuf>,
    pub password: Option<String>,
    pub ffmpeg_bin: String,
    pub external_web_dir: Option<PathBuf>,
}

pub struct XMediaToolApp {
    pub settings: ToolSettingsService,
    pub workbench: MediaWorkbenchService,
    pub scraping: ScrapingOrchestratorService,
    pub router: Router,
}

impl XMediaToolApp {
    pub async fn init(cfg: XMediaToolConfig) -> anyhow::Result<Self> {
        Self::init_with_providers_bind_and_cors(cfg, build_default_scraper_providers(), None, &[])
            .await
    }

    pub async fn init_with_cors_origins(
        cfg: XMediaToolConfig,
        cors_allow_origins: &[String],
    ) -> anyhow::Result<Self> {
        Self::init_with_providers_bind_and_cors(
            cfg,
            build_default_scraper_providers(),
            None,
            cors_allow_origins,
        )
        .await
    }

    pub async fn init_with_bind_and_cors(
        cfg: XMediaToolConfig,
        bind_addr: Option<SocketAddr>,
        cors_allow_origins: &[String],
    ) -> anyhow::Result<Self> {
        Self::init_with_providers_bind_and_cors(
            cfg,
            build_default_scraper_providers(),
            bind_addr,
            cors_allow_origins,
        )
        .await
    }

    pub async fn init_with_bind_cors_and_lock(
        cfg: XMediaToolConfig,
        bind_addr: Option<SocketAddr>,
        cors_allow_origins: &[String],
        lock_roots: bool,
    ) -> anyhow::Result<Self> {
        Self::init_full(
            cfg,
            build_default_scraper_providers(),
            bind_addr,
            cors_allow_origins,
            lock_roots,
        )
        .await
    }

    pub async fn init_with_providers_and_cors(
        cfg: XMediaToolConfig,
        providers: Vec<Arc<dyn ScraperProviderPort>>,
        cors_allow_origins: &[String],
    ) -> anyhow::Result<Self> {
        Self::init_full(cfg, providers, None, cors_allow_origins, false).await
    }

    pub async fn init_with_providers_bind_and_cors(
        cfg: XMediaToolConfig,
        providers: Vec<Arc<dyn ScraperProviderPort>>,
        bind_addr: Option<SocketAddr>,
        cors_allow_origins: &[String],
    ) -> anyhow::Result<Self> {
        Self::init_full(cfg, providers, bind_addr, cors_allow_origins, false).await
    }

    async fn init_full(
        cfg: XMediaToolConfig,
        providers: Vec<Arc<dyn ScraperProviderPort>>,
        bind_addr: Option<SocketAddr>,
        cors_allow_origins: &[String],
        lock_roots: bool,
    ) -> anyhow::Result<Self> {
        std::fs::create_dir_all(&cfg.data_dir).with_context(|| {
            format!(
                "failed to create xmedia-tool data directory {}",
                cfg.data_dir.display()
            )
        })?;

        let workspace_fs: Arc<dyn WorkspaceFsPort> = Arc::new(LocalWorkspaceFs);
        let toolbox: Arc<dyn MediaToolboxPort> = Arc::new(DefaultMediaToolbox);
        let webhook: Arc<dyn HttpWebhookPort> = Arc::new(ReqwestWebhookClient);
        let scrape_cache: Arc<dyn ScrapeCachePort> = Arc::new(ScrapeMetadataCache::new(Some(
            cfg.data_dir.join("scrape-cache"),
        )));
        let scraper_transport: Arc<dyn ScraperTransportPort> = Arc::new(ReqwestScraperTransport);

        let config_file = cfg.data_dir.join("tool-config.json");
        let config_repo = Arc::new(
            JsonFileConfigRepository::load_or_init(config_file, &cfg.initial_roots)
                .map_err(|e| anyhow::anyhow!(e.to_string()))?,
        );
        let settings = ToolSettingsService::new(config_repo, Arc::clone(&workspace_fs))
            .with_sandbox_policy(Some(cfg.data_dir.clone()), &cfg.initial_roots, lock_roots);

        let ffmpeg = FfmpegToolkit::from_ffmpeg_bin(&cfg.ffmpeg_bin);
        let inspectors = build_default_inspectors(ffmpeg.clone());
        let workbench = MediaWorkbenchService::new(
            settings.clone(),
            inspectors,
            workspace_fs,
            toolbox,
            Arc::clone(&webhook),
            ffmpeg.clone(),
        );
        let scraping = ScrapingOrchestratorService::new(
            settings.clone(),
            workbench.clone(),
            providers,
            scrape_cache,
            scraper_transport,
        );
        let auth = ToolAuthService::new(cfg.password);
        let security = ToolRequestSecurityPolicy::new(bind_addr, cors_allow_origins);
        let fonts: Arc<dyn FontRepository> = Arc::new(FsFontStore::new(cfg.data_dir.join("fonts")));

        let state = ToolHttpState {
            auth,
            settings: settings.clone(),
            workbench: workbench.clone(),
            scraping: scraping.clone(),
            webhook,
            fonts,
            ffmpeg,
            external_web_dir: cfg.external_web_dir,
            security,
        };

        let compression_predicate = DefaultPredicate::new()
            .and(NotForContentType::const_new("image/"))
            .and(NotForContentType::const_new("font/"))
            .and(NotForContentType::const_new("application/octet-stream"));

        let spa_service = axum::routing::any(serve_tool_spa).with_state(state.clone());
        let api_router = Router::new()
            .merge(tool_fonts_api_router(state.clone()))
            .nest("/v1/tool", tool_api_router(state.clone()))
            .with_state(state.clone());

        let mut router = Router::new()
            .nest("/api", api_router)
            .fallback_service(spa_service)
            .layer(middleware::from_fn_with_state(
                state,
                guard_tool_http_request,
            ));
        if !cors_allow_origins.is_empty() {
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
            settings,
            workbench,
            scraping,
            router,
        })
    }
}

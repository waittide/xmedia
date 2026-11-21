pub mod application;
pub mod domain;
pub mod http;
pub mod infra;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use axum::Router;
use tower_http::compression::CompressionLayer;
use tower_http::compression::predicate::{DefaultPredicate, NotForContentType, Predicate};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use xmedia_core::infra::ffmpeg::FfmpegToolkit;

use crate::application::auth::ToolAuthService;
use crate::application::scraping::ScrapingOrchestratorService;
use crate::application::settings::ToolSettingsService;
use crate::application::workbench::MediaWorkbenchService;
use crate::domain::ports::ScraperProviderPort;
use crate::http::ToolHttpState;
use crate::http::routes::tool_api_router;
use crate::http::static_files::serve_tool_spa;
use crate::infra::config_store::JsonFileConfigRepository;
use crate::infra::inspectors::build_default_inspectors;
use crate::infra::scrapers::build_default_scraper_providers;

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
        Self::init_with_providers(cfg, build_default_scraper_providers()).await
    }

    pub async fn init_with_providers(
        cfg: XMediaToolConfig,
        providers: Vec<Arc<dyn ScraperProviderPort>>,
    ) -> anyhow::Result<Self> {
        std::fs::create_dir_all(&cfg.data_dir).with_context(|| {
            format!(
                "failed to create xmedia-tool data directory {}",
                cfg.data_dir.display()
            )
        })?;

        let config_file = cfg.data_dir.join("tool-config.json");
        let config_repo = Arc::new(
            JsonFileConfigRepository::load_or_init(config_file, &cfg.initial_roots)
                .map_err(|e| anyhow::anyhow!(e.to_string()))?,
        );
        let settings = ToolSettingsService::new(config_repo);

        let ffmpeg = FfmpegToolkit::from_ffmpeg_bin(&cfg.ffmpeg_bin);
        let inspectors = build_default_inspectors(ffmpeg.clone());
        let workbench = MediaWorkbenchService::new(settings.clone(), inspectors, ffmpeg.clone());
        let scraping = ScrapingOrchestratorService::with_cache_dir(
            settings.clone(),
            workbench.clone(),
            providers,
            Some(cfg.data_dir.join("scrape-cache")),
        );
        let auth = ToolAuthService::new(cfg.password);

        let state = ToolHttpState {
            auth,
            settings: settings.clone(),
            workbench: workbench.clone(),
            scraping: scraping.clone(),
            ffmpeg,
            external_web_dir: cfg.external_web_dir,
        };

        let compression_predicate = DefaultPredicate::new()
            .and(NotForContentType::const_new("image/"))
            .and(NotForContentType::const_new("application/octet-stream"));

        let spa_service = axum::routing::get(serve_tool_spa).with_state(state.clone());

        let router = Router::new()
            .nest(
                "/api/v1/tool",
                tool_api_router(state.clone()).with_state(state),
            )
            .fallback_service(spa_service)
            .layer(CorsLayer::permissive())
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

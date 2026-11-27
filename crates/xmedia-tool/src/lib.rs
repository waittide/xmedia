pub mod application;
pub mod domain;
pub mod http;
pub mod infra;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use axum::Router;
use axum::http::{HeaderValue, Method};
use axum::middleware;
use tower_http::compression::CompressionLayer;
use tower_http::compression::predicate::{DefaultPredicate, NotForContentType, Predicate};
use tower_http::cors::{AllowHeaders, AllowOrigin, CorsLayer};
use tower_http::trace::TraceLayer;
use xmedia_core::application::ports::FontRepository;
use xmedia_core::infra::ffmpeg::FfmpegToolkit;
use xmedia_core::infra::fonts::FsFontStore;

use crate::application::auth::ToolAuthService;
use crate::application::ports::{
    HttpWebhookPort, MediaToolboxPort, ScrapeCachePort, ScraperProviderPort, ScraperTransportPort,
    WorkspaceFsPort,
};
use crate::application::scraping::{ScrapeApplyDeps, ScrapeApplyService, ScraperCatalogService};
use crate::application::settings::{SettingsSandboxPolicy, ToolSettingsService};
use crate::application::workbench::{
    FormatToolsService, MetadataService, RenameService, RootLockRegistry, WorkspaceBrowserService,
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

#[derive(Default)]
pub struct ToolInitOptions {
    pub providers: Option<Vec<Arc<dyn ScraperProviderPort>>>,
    pub bind_addr: Option<SocketAddr>,
    pub cors_allow_origins: Vec<String>,
    pub lock_roots: bool,
}

pub struct XMediaToolApp {
    pub settings: ToolSettingsService,
    pub workspace: WorkspaceBrowserService,
    pub metadata: MetadataService,
    pub rename: RenameService,
    pub tools: FormatToolsService,
    pub scrapers: ScraperCatalogService,
    pub scrape_apply: ScrapeApplyService,
    pub router: Router,
}

impl XMediaToolApp {
    pub async fn init(cfg: XMediaToolConfig) -> anyhow::Result<Self> {
        Self::init_with_options(cfg, ToolInitOptions::default()).await
    }

    pub async fn init_with_options(
        cfg: XMediaToolConfig,
        options: ToolInitOptions,
    ) -> anyhow::Result<Self> {
        ensure_data_dir(&cfg.data_dir)?;

        let providers = options
            .providers
            .unwrap_or_else(build_default_scraper_providers);
        let workspace_fs: Arc<dyn WorkspaceFsPort> = Arc::new(LocalWorkspaceFs);
        let toolbox: Arc<dyn MediaToolboxPort> = Arc::new(DefaultMediaToolbox);
        let webhook: Arc<dyn HttpWebhookPort> = Arc::new(ReqwestWebhookClient);
        let scrape_cache: Arc<dyn ScrapeCachePort> = Arc::new(ScrapeMetadataCache::new(Some(
            cfg.data_dir.join("scrape-cache"),
        )));
        let scraper_transport: Arc<dyn ScraperTransportPort> = Arc::new(ReqwestScraperTransport);

        let settings = init_settings_service(&cfg, Arc::clone(&workspace_fs), options.lock_roots)?;
        let ffmpeg = FfmpegToolkit::from_ffmpeg_bin(&cfg.ffmpeg_bin);
        let inspectors = build_default_inspectors(ffmpeg.clone());
        let root_locks = RootLockRegistry::default();

        let workspace =
            WorkspaceBrowserService::new(settings.clone(), inspectors, Arc::clone(&workspace_fs));
        let metadata = MetadataService::new(
            settings.clone(),
            workspace.clone(),
            Arc::clone(&workspace_fs),
            Arc::clone(&toolbox),
        );
        let rename = RenameService::new(
            settings.clone(),
            workspace.clone(),
            metadata.clone(),
            Arc::clone(&workspace_fs),
            Arc::clone(&webhook),
            root_locks.clone(),
        );
        let tools = FormatToolsService::new(
            settings.clone(),
            workspace.clone(),
            Arc::clone(&workspace_fs),
            Arc::clone(&toolbox),
            ffmpeg.clone(),
        );
        let scrapers = ScraperCatalogService::new(
            settings.clone(),
            providers,
            scrape_cache,
            Arc::clone(&scraper_transport),
        );
        let scrape_apply = ScrapeApplyService::new(ScrapeApplyDeps {
            settings: settings.clone(),
            catalog: scrapers.clone(),
            workspace: workspace.clone(),
            metadata: metadata.clone(),
            rename: rename.clone(),
            fs: Arc::clone(&workspace_fs),
            toolbox,
            transport: scraper_transport,
            webhook: Arc::clone(&webhook),
            root_locks,
        });

        let auth = ToolAuthService::new(cfg.password);
        let security =
            ToolRequestSecurityPolicy::new(options.bind_addr, &options.cors_allow_origins);
        let fonts: Arc<dyn FontRepository> = Arc::new(FsFontStore::new(cfg.data_dir.join("fonts")));

        let state = ToolHttpState {
            auth,
            settings: settings.clone(),
            workspace: workspace.clone(),
            metadata: metadata.clone(),
            rename: rename.clone(),
            tools: tools.clone(),
            scrapers: scrapers.clone(),
            scrape_apply: scrape_apply.clone(),
            fs: workspace_fs,
            webhook,
            fonts,
            ffmpeg,
            external_web_dir: cfg.external_web_dir,
            security,
        };

        let router = build_tool_router(state, &options.cors_allow_origins)?;

        Ok(Self {
            settings,
            workspace,
            metadata,
            rename,
            tools,
            scrapers,
            scrape_apply,
            router,
        })
    }
}

fn ensure_data_dir(data_dir: &std::path::Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(data_dir).with_context(|| {
        format!(
            "failed to create xmedia-tool data directory {}",
            data_dir.display()
        )
    })
}

fn init_settings_service(
    cfg: &XMediaToolConfig,
    workspace_fs: Arc<dyn WorkspaceFsPort>,
    lock_roots: bool,
) -> anyhow::Result<ToolSettingsService> {
    let config_file = cfg.data_dir.join("tool-config.json");
    let config_repo = Arc::new(
        JsonFileConfigRepository::load_or_init(config_file, &cfg.initial_roots)
            .map_err(|e| anyhow::anyhow!(e.to_string()))?,
    );
    Ok(ToolSettingsService::new(
        config_repo,
        workspace_fs,
        SettingsSandboxPolicy {
            data_dir: Some(cfg.data_dir.clone()),
            cli_roots: cfg.initial_roots.clone(),
            roots_locked: lock_roots,
        },
    ))
}

fn build_cors_layer(cors_allow_origins: &[String]) -> anyhow::Result<Option<CorsLayer>> {
    if cors_allow_origins.is_empty() {
        return Ok(None);
    }
    let parsed_origins: Vec<HeaderValue> = cors_allow_origins
        .iter()
        .map(|o| o.trim())
        .filter(|o| !o.is_empty())
        .map(|o| {
            HeaderValue::from_str(o)
                .with_context(|| format!("invalid CORS allow-origin header value: '{o}'"))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    if parsed_origins.is_empty() {
        return Ok(None);
    }
    Ok(Some(
        CorsLayer::new()
            .allow_origin(AllowOrigin::list(parsed_origins))
            .allow_methods([
                Method::GET,
                Method::POST,
                Method::PUT,
                Method::PATCH,
                Method::DELETE,
                Method::HEAD,
                Method::OPTIONS,
            ])
            .allow_headers(AllowHeaders::mirror_request())
            .allow_credentials(true),
    ))
}

fn build_tool_router(
    state: ToolHttpState,
    cors_allow_origins: &[String],
) -> anyhow::Result<Router> {
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
    if let Some(cors) = build_cors_layer(cors_allow_origins)? {
        router = router.layer(cors);
    }
    Ok(router
        .layer(CompressionLayer::new().compress_when(compression_predicate))
        .layer(
            TraceLayer::new_for_http().make_span_with(|req: &axum::http::Request<_>| {
                let uri = xmedia_core::http::redact_uri_for_log(req.uri());
                tracing::debug_span!(
                    "request",
                    method = %req.method(),
                    uri = %uri,
                    version = ?req.version(),
                )
            }),
        ))
}

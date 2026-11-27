mod gateway;
mod static_files;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use anyhow::Context;
use axum::Router;
use axum::http::{HeaderValue, Method};
use toasty::Db;
use toasty::schema::ModelSet;
use tower_http::compression::CompressionLayer;
use tower_http::compression::predicate::{DefaultPredicate, NotForContentType, Predicate};
use tower_http::cors::{AllowHeaders, AllowOrigin, CorsLayer};
use tower_http::trace::TraceLayer;
use xmedia_core::application::activity::ActivityService;
use xmedia_core::application::auth::AuthService;
use xmedia_core::application::bindings::MediaBindingService;
use xmedia_core::application::library::LibraryService;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;
use xmedia_core::domain::repository::{UserPluginRepository, UserRepository};
use xmedia_core::domain::user::User;
use xmedia_core::infra::events::BroadcastEventHub;
use xmedia_core::infra::ffmpeg::FfmpegToolkit;
use xmedia_core::infra::fonts::FsFontStore;
use xmedia_core::infra::persistence::repositories::{
    ToastyLibraryRepository, ToastyOnlineLibraryItemRepository, ToastyUserActivityRepository,
    ToastyUserMediaBindingRepository, ToastyUserPluginRepository, ToastyUserRepository,
};
use xmedia_core::infra::security::{JwtTokenService, SecurityManager};
use xmedia_core::{
    ensure_core_schema, ensure_sqlite_parent_dir, is_core_schema_initialized, register_core_models,
};
use xmedia_source::crud::{PluginDomainHandler, PluginRegistry, plugins_router};

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
}

pub struct XMediaApp {
    pub auth: Arc<AuthService>,
    pub libraries: Arc<LibraryService>,
    pub events: Arc<BroadcastEventHub>,
    enabled_modules: Vec<MediaKind>,
    router: Router,
    temp_dir: PathBuf,
    #[cfg(feature = "book")]
    book_catalog: Arc<xmedia_book::application::catalog::BookCatalogService>,
    #[cfg(feature = "comic")]
    comic_catalog: Arc<xmedia_comic::application::catalog::ComicCatalogService>,
    #[cfg(feature = "music")]
    music_ingest: Arc<xmedia_music::application::ingest::MusicIngestService>,
}

#[cfg(any(feature = "book", feature = "comic", feature = "music"))]
struct ScopedImportTempFile {
    path: PathBuf,
}

#[cfg(any(feature = "book", feature = "comic", feature = "music"))]
impl ScopedImportTempFile {
    async fn write_in(dir: &std::path::Path, filename: &str, data: &[u8]) -> DomainResult<Self> {
        if data.is_empty() {
            return Err(DomainError::Validation(
                "imported file payload cannot be empty".into(),
            ));
        }
        tokio::fs::create_dir_all(dir).await.map_err(|e| {
            DomainError::Storage(format!("failed to prepare import temp directory: {e}"))
        })?;
        let clean_ext = std::path::Path::new(filename)
            .extension()
            .and_then(|s| s.to_str())
            .map(|e| {
                e.trim()
                    .trim_start_matches('.')
                    .chars()
                    .filter(|c| c.is_ascii_alphanumeric())
                    .collect::<String>()
            })
            .filter(|e| !e.is_empty());
        let temp_name = match clean_ext {
            Some(ext) => format!("xmedia_import_{}.{ext}", uuid::Uuid::now_v7()),
            None => format!("xmedia_import_{}.tmp", uuid::Uuid::now_v7()),
        };
        let path = dir.join(temp_name);
        let guard = Self { path: path.clone() };
        tokio::fs::write(&path, data)
            .await
            .map_err(|e| DomainError::Storage(format!("failed to write import temp file: {e}")))?;
        Ok(guard)
    }

    fn path(&self) -> &std::path::Path {
        &self.path
    }
}

#[cfg(any(feature = "book", feature = "comic", feature = "music"))]
impl Drop for ScopedImportTempFile {
    fn drop(&mut self) {
        let path = std::mem::take(&mut self.path);
        if path.as_os_str().is_empty() {
            return;
        }
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn_blocking(move || {
                let _ = std::fs::remove_file(&path);
            });
        } else {
            let _ = std::fs::remove_file(&path);
        }
    }
}

impl XMediaApp {
    pub async fn init(cfg: XMediaConfig) -> anyhow::Result<Self> {
        Self::init_with_options(cfg, XMediaInitOptions::default()).await
    }

    pub async fn init_with_options(
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
        let temp_dir = cfg.data_dir.join("tmp");
        if let Err(err) = xmedia_core::http::configure_temp_dir(temp_dir.clone()) {
            tracing::warn!(error = %err, "failed to prepare application temp directory");
        }
        ensure_sqlite_parent_dir(&cfg.database_url).map_err(|e| anyhow::anyhow!(e.to_string()))?;

        let (db, bindings_table_created) = connect_and_migrate_db(&cfg.database_url).await?;

        let users_repo = Arc::new(ToastyUserRepository::new(db.clone()));
        let libraries_repo = Arc::new(ToastyLibraryRepository::new(db.clone()));
        let binding_repo = Arc::new(ToastyUserMediaBindingRepository::new(db.clone()));
        let online_items_repo = Arc::new(ToastyOnlineLibraryItemRepository::new(db.clone()));
        let plugins_repo: Arc<dyn UserPluginRepository> =
            Arc::new(ToastyUserPluginRepository::with_book_cache_dir(
                db.clone(),
                cfg.data_dir.join("online_book_cache"),
            ));
        let activity = Arc::new(ActivityService::new(Arc::new(
            ToastyUserActivityRepository::new(db.clone()),
        )));
        let lifecycles = Arc::new(RwLock::new(HashMap::new()));
        let events = Arc::new(BroadcastEventHub::default());
        let bindings = Arc::new(MediaBindingService::new(binding_repo, Some(events.clone())));
        let security = Arc::new(SecurityManager::new(&cfg.secret));
        let tokens = Arc::new(
            JwtTokenService::try_new(&cfg.secret, cfg.jwt_ttl_secs)
                .map_err(|e| anyhow::anyhow!(e.to_string()))?,
        );
        let ffmpeg = FfmpegToolkit::from_ffmpeg_bin(&cfg.ffmpeg_bin);

        let auth = Arc::new(
            AuthService::new(users_repo.clone(), security, tokens, lifecycles.clone())
                .with_cleanup(
                    Some(activity.clone()),
                    Some(online_items_repo),
                    Some(bindings.clone()),
                )
                .with_plugins_cleanup(plugins_repo.clone()),
        );
        let libraries = Arc::new(
            LibraryService::new(libraries_repo.clone(), events.clone(), lifecycles)
                .with_mount_policy(Some(cfg.data_dir.clone()), None),
        );
        libraries
            .recover_interrupted_scans()
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;

        #[allow(unused_mut)]
        let mut enabled_modules = Vec::new();
        #[allow(unused_mut)]
        let mut plugin_handlers: Vec<Arc<dyn PluginDomainHandler>> = Vec::new();
        #[allow(unused_mut)]
        let mut api_v1 = Router::new();
        #[allow(unused_mut)]
        let mut root_protocols = Router::new();
        #[allow(unused_mut)]
        let mut backfill_batches: Vec<(MediaKind, Vec<uuid::Uuid>)> = Vec::new();
        #[cfg(feature = "book")]
        let book_catalog;
        #[cfg(feature = "comic")]
        let comic_catalog;
        #[cfg(feature = "music")]
        let music_ingest;

        #[cfg(feature = "book")]
        {
            enabled_modules.push(MediaKind::Book);
            let book_services = xmedia_book::init_book_module_with_bindings(
                db.clone(),
                &cfg.data_dir,
                libraries_repo.clone(),
                &libraries,
                bindings.clone(),
                events.clone(),
                opts.allow_private_ips,
            )
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
            book_catalog = book_services.catalog.clone();
            libraries
                .ensure_managed_library(MediaKind::Book)
                .await
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;
            if bindings_table_created {
                let ids = book_services
                    .catalog
                    .list_all_book_ids_for_backfill()
                    .await
                    .map_err(|e| anyhow::anyhow!(e.to_string()))?;
                backfill_batches.push((MediaKind::Book, ids));
            }

            plugin_handlers.push(xmedia_book::http::plugin_handler(
                book_services.online.clone(),
            ));
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
            let comic_services = xmedia_comic::init_comic_module_with_bindings(
                db.clone(),
                &cfg.data_dir,
                libraries_repo.clone(),
                &libraries,
                events.clone(),
                bindings.clone(),
                opts.allow_private_ips,
            )
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
            comic_catalog = comic_services.catalog.clone();
            libraries
                .ensure_managed_library(MediaKind::Comic)
                .await
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;
            if bindings_table_created {
                let ids = comic_services
                    .catalog
                    .list_all_series_ids_for_backfill()
                    .await
                    .map_err(|e| anyhow::anyhow!(e.to_string()))?;
                backfill_batches.push((MediaKind::Comic, ids));
            }

            plugin_handlers.push(xmedia_comic::http::plugin_handler(
                comic_services.online.clone(),
            ));
            let comic_state =
                xmedia_comic::http::state::ComicHttpState::new(auth.clone(), comic_services);
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
            let music_services = xmedia_music::init_music_module_with_bindings(
                db.clone(),
                &cfg.data_dir,
                ffmpeg.clone(),
                libraries_repo.clone(),
                &libraries,
                events.clone(),
                bindings.clone(),
                opts.allow_private_ips,
            )
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
            music_ingest = music_services.ingest.clone();
            libraries
                .ensure_managed_library(MediaKind::Music)
                .await
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;
            if bindings_table_created {
                let ids = music_services
                    .catalog
                    .list_all_track_ids_for_backfill()
                    .await
                    .map_err(|e| anyhow::anyhow!(e.to_string()))?;
                backfill_batches.push((MediaKind::Music, ids));
            }

            plugin_handlers.push(xmedia_music::http::plugin_handler(
                music_services.online.clone(),
            ));
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
            let video_services = xmedia_video::init_video_module_with_bindings(
                db.clone(),
                &cfg.data_dir,
                ffmpeg.clone(),
                libraries_repo.clone(),
                &libraries,
                bindings.clone(),
                events.clone(),
                opts.allow_private_ips,
            )
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
            libraries
                .ensure_managed_library(MediaKind::Video)
                .await
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;
            if bindings_table_created {
                let ids = video_services
                    .catalog
                    .list_all_binding_ids_for_backfill()
                    .await
                    .map_err(|e| anyhow::anyhow!(e.to_string()))?;
                backfill_batches.push((MediaKind::Video, ids));
            }

            plugin_handlers.push(xmedia_video::http::plugin_handler(
                video_services.online.clone(),
            ));
            let video_state = xmedia_video::http::state::VideoHttpState::new(
                auth.clone(),
                libraries.clone(),
                video_services,
            );
            api_v1 = api_v1.nest(
                "/video",
                xmedia_video::http::rest_router(video_state.clone()),
            );
            root_protocols = root_protocols.nest(
                "/jellyfin",
                xmedia_video::http::jellyfin_router(video_state),
            );
        }

        let plugin_registry = Arc::new(PluginRegistry::new(plugin_handlers));
        api_v1 = api_v1.nest("/plugins", plugins_router(auth.clone(), plugin_registry));

        if bindings_table_created {
            backfill_admin_bindings(users_repo.as_ref(), &bindings, &backfill_batches).await?;
        }
        migrate_legacy_sources_to_first_admin(&db, users_repo.as_ref(), plugins_repo.as_ref())
            .await?;

        let _ = opts.allow_private_ips;

        let gateway_state = GatewayState {
            db,
            plugins_repo,
            auth: auth.clone(),
            libraries: libraries.clone(),
            libraries_repo,
            bindings,
            activity,
            fonts: Arc::new(FsFontStore::new(cfg.data_dir.join("fonts"))),
            events: events.clone(),
            sse_limiter: SseConnectionLimiter::new(),
            ffmpeg,
            ffmpeg_status_cache: FfmpegStatusCache::new(),
            enabled_modules: Arc::new(enabled_modules.clone()),
            external_web_dir: cfg.external_web_dir,
        };

        let router = build_gateway_router(gateway_state, api_v1, root_protocols, &opts)?;

        Ok(Self {
            auth,
            libraries,
            events,
            enabled_modules,
            router,
            temp_dir,
            #[cfg(feature = "book")]
            book_catalog,
            #[cfg(feature = "comic")]
            comic_catalog,
            #[cfg(feature = "music")]
            music_ingest,
        })
    }

    pub fn into_embedded_host(self) -> (Arc<Self>, Router, Vec<MediaKind>) {
        let router = self.router.clone();
        let enabled_modules = self.enabled_modules.clone();
        (Arc::new(self), router, enabled_modules)
    }

    pub async fn serve(
        self,
        bind: SocketAddr,
        listener: tokio::net::TcpListener,
        shutdown: impl std::future::Future<Output = ()> + Send + 'static,
    ) -> anyhow::Result<()> {
        tracing::info!("xmedia listening on http://{bind}");
        if self.enabled_modules.contains(&MediaKind::Book) {
            tracing::info!(
                "Book OPDS 1.2 / 2.0: http://{bind}/opds/book/v1.2 | http://{bind}/opds/book/v2"
            );
        }
        if self.enabled_modules.contains(&MediaKind::Comic) {
            tracing::info!("Comic OPDS 1.2 + PSE: http://{bind}/opds/comic/v1.2");
        }
        if self.enabled_modules.contains(&MediaKind::Music) {
            tracing::info!("Music OpenSubsonic API: http://{bind}/rest");
        }
        axum::serve(
            listener,
            self.router
                .into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(shutdown)
        .await
        .context("http server error")
    }

    pub async fn import_local_media(
        &self,
        actor: &User,
        kind: MediaKind,
        filename: String,
        data: bytes::Bytes,
    ) -> DomainResult<(String, String)> {
        match kind {
            MediaKind::Book => {
                #[cfg(feature = "book")]
                {
                    let temp =
                        ScopedImportTempFile::write_in(&self.temp_dir, &filename, &data).await?;
                    let book = self
                        .book_catalog
                        .upload_book_from_path(actor, &filename, temp.path())
                        .await?;
                    Ok((book.id.to_string(), book.metadata.title))
                }
                #[cfg(not(feature = "book"))]
                {
                    let _ = (&self.temp_dir, actor, filename, data);
                    Err(DomainError::UnsupportedFormat(
                        "book module is not enabled".into(),
                    ))
                }
            }
            MediaKind::Comic => {
                #[cfg(feature = "comic")]
                {
                    let temp =
                        ScopedImportTempFile::write_in(&self.temp_dir, &filename, &data).await?;
                    let uploaded = self
                        .comic_catalog
                        .upload_archive_from_path(
                            actor,
                            xmedia_comic::application::dto::UploadArchivePathInput {
                                filename,
                                temp_path: temp.path().to_path_buf(),
                                target_series_id: None,
                                series_title_override: None,
                            },
                        )
                        .await?;
                    Ok((uploaded.series_id.to_string(), uploaded.series_title))
                }
                #[cfg(not(feature = "comic"))]
                {
                    let _ = (&self.temp_dir, actor, filename, data);
                    Err(DomainError::UnsupportedFormat(
                        "comic module is not enabled".into(),
                    ))
                }
            }
            MediaKind::Music => {
                #[cfg(feature = "music")]
                {
                    let temp =
                        ScopedImportTempFile::write_in(&self.temp_dir, &filename, &data).await?;
                    let batch = self
                        .music_ingest
                        .upload_files_from_paths(
                            actor,
                            vec![xmedia_music::application::dto::UploadFilePathItem {
                                filename,
                                temp_path: temp.path().to_path_buf(),
                            }],
                        )
                        .await?;
                    let track = batch.uploaded_tracks.into_iter().next().ok_or_else(|| {
                        DomainError::Validation(
                            "no audio track produced from imported payload".into(),
                        )
                    })?;
                    Ok((track.id.to_string(), track.title))
                }
                #[cfg(not(feature = "music"))]
                {
                    let _ = (&self.temp_dir, actor, filename, data);
                    Err(DomainError::UnsupportedFormat(
                        "music module is not enabled".into(),
                    ))
                }
            }
            MediaKind::Video => {
                let _ = (&self.temp_dir, actor, filename, data);
                Err(DomainError::UnsupportedFormat(
                    "direct video payload import is not supported".into(),
                ))
            }
        }
    }
}

async fn connect_and_migrate_db(database_url: &str) -> anyhow::Result<(Db, bool)> {
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
        .connect(database_url)
        .await
        .map_err(|e| anyhow::anyhow!("database connection failed: {e}"))?;

    let bindings_table_created = if !is_core_schema_initialized(&mut db).await {
        tracing::info!(
            "cold start detected: pushing unified database schema for core and all enabled modules"
        );
        db.push_schema()
            .await
            .map_err(|e| anyhow::anyhow!("failed to push unified database schema: {e}"))?;
        #[cfg(feature = "comic")]
        xmedia_comic::ensure_comic_indexes(&mut db)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        #[cfg(feature = "music")]
        xmedia_music::ensure_music_indexes(&mut db)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        #[cfg(feature = "video")]
        xmedia_video::ensure_video_indexes(&mut db)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        false
    } else {
        let core_report = ensure_core_schema(&mut db, database_url)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        #[cfg(feature = "book")]
        xmedia_book::ensure_book_schema(&mut db, database_url)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        #[cfg(feature = "comic")]
        xmedia_comic::ensure_comic_schema(&mut db, database_url)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        #[cfg(feature = "music")]
        xmedia_music::ensure_music_schema(&mut db, database_url)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        #[cfg(feature = "video")]
        xmedia_video::ensure_video_schema(&mut db, database_url)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        core_report.bindings_table_created
    };

    Ok((db, bindings_table_created))
}

async fn backfill_admin_bindings(
    users_repo: &dyn UserRepository,
    bindings: &MediaBindingService,
    batches: &[(MediaKind, Vec<uuid::Uuid>)],
) -> anyhow::Result<()> {
    let admin_ids: Vec<_> = users_repo
        .list_all()
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?
        .into_iter()
        .filter(|u| u.role.is_admin())
        .map(|u| u.id)
        .collect();
    if admin_ids.is_empty() {
        return Ok(());
    }
    let mut total_bound = 0usize;
    for (kind, ids) in batches {
        if ids.is_empty() {
            continue;
        }
        for &admin_id in &admin_ids {
            total_bound += bindings
                .bind_batch(admin_id, *kind, ids)
                .await
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        }
    }
    tracing::info!(
        admins = admin_ids.len(),
        total_bound,
        "completed one-time upgrade backfill for user_media_bindings"
    );
    Ok(())
}

pub(crate) async fn migrate_all_legacy_sources_to_user(
    db: &Db,
    plugins_repo: &dyn UserPluginRepository,
    owner_user_id: uuid::Uuid,
) -> DomainResult<usize> {
    #[allow(unused_mut)]
    let mut migrated = 0usize;
    #[cfg(feature = "book")]
    {
        let mut db_clone = db.clone();
        migrated +=
            xmedia_book::migrate_legacy_book_sources(&mut db_clone, owner_user_id, plugins_repo)
                .await?;
    }
    #[cfg(feature = "comic")]
    {
        let mut db_clone = db.clone();
        migrated +=
            xmedia_comic::migrate_legacy_comic_sources(&mut db_clone, owner_user_id, plugins_repo)
                .await?;
    }
    #[cfg(feature = "music")]
    {
        let mut db_clone = db.clone();
        migrated +=
            xmedia_music::migrate_legacy_music_sources(&mut db_clone, owner_user_id, plugins_repo)
                .await?;
    }
    #[cfg(feature = "video")]
    {
        let mut db_clone = db.clone();
        migrated +=
            xmedia_video::migrate_legacy_video_sources(&mut db_clone, owner_user_id, plugins_repo)
                .await?;
    }
    let _ = (db, plugins_repo, owner_user_id);
    if migrated > 0 {
        tracing::info!(
            owner_user_id = %owner_user_id,
            migrated,
            "migrated legacy global sources into isolated user_plugins"
        );
    }
    Ok(migrated)
}

async fn migrate_legacy_sources_to_first_admin(
    db: &Db,
    users_repo: &dyn UserRepository,
    plugins_repo: &dyn UserPluginRepository,
) -> anyhow::Result<()> {
    let first_admin = users_repo
        .list_all()
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?
        .into_iter()
        .find(|u| u.role.is_admin());
    if let Some(admin) = first_admin {
        migrate_all_legacy_sources_to_user(db, plugins_repo, admin.id.0)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    }
    Ok(())
}

fn build_gateway_router(
    gateway_state: GatewayState,
    api_v1: Router,
    root_protocols: Router,
    opts: &XMediaInitOptions,
) -> anyhow::Result<Router> {
    let merged_api_v1 = api_v1.merge(core_api_router().with_state(gateway_state.clone()));

    let compression_predicate = DefaultPredicate::new()
        .and(NotForContentType::const_new("audio/"))
        .and(NotForContentType::const_new("video/"))
        .and(NotForContentType::const_new("image/"))
        .and(NotForContentType::const_new("font/"))
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
            );
        }
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

#[cfg(test)]
mod tests;

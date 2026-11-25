pub mod gateway;
pub mod static_files;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
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
use xmedia_core::application::activity::ActivityService;
use xmedia_core::application::auth::AuthService;
use xmedia_core::application::bindings::MediaBindingService;
use xmedia_core::application::library::LibraryService;
use xmedia_core::domain::library::MediaKind;
use xmedia_core::domain::repository::UserRepository;
use xmedia_core::infra::events::BroadcastEventHub;
use xmedia_core::infra::ffmpeg::FfmpegToolkit;
use xmedia_core::infra::persistence::repositories::{
    ToastyLibraryRepository, ToastyOnlineLibraryItemRepository, ToastyUserActivityRepository,
    ToastyUserMediaBindingRepository, ToastyUserRepository,
};
use xmedia_core::infra::security::{JwtTokenService, SecurityManager};
use xmedia_core::{
    ensure_core_schema_with_report, ensure_sqlite_parent_dir, is_core_schema_initialized,
    register_core_models,
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
}

pub struct XMediaApp {
    pub auth: Arc<AuthService>,
    pub libraries: Arc<LibraryService>,
    pub events: Arc<BroadcastEventHub>,
    pub enabled_modules: Vec<MediaKind>,
    pub router: Router,
    temp_dir: PathBuf,
    #[cfg(feature = "book")]
    book_catalog: Arc<xmedia_book::application::catalog::BookCatalogService>,
    #[cfg(feature = "comic")]
    comic_catalog: Arc<xmedia_comic::application::catalog::ComicCatalogService>,
    #[cfg(feature = "music")]
    music_ingest: Arc<xmedia_music::application::ingest::MusicIngestService>,
}

struct ScopedImportTempFile {
    path: PathBuf,
}

impl ScopedImportTempFile {
    async fn write_in(
        dir: &Path,
        filename: &str,
        data: &[u8],
    ) -> xmedia_core::domain::error::DomainResult<Self> {
        use xmedia_core::domain::error::DomainError;

        if data.is_empty() {
            return Err(DomainError::Validation(
                "imported file payload cannot be empty".into(),
            ));
        }
        tokio::fs::create_dir_all(dir).await.map_err(|e| {
            DomainError::Storage(format!("failed to prepare import temp directory: {e}"))
        })?;
        let clean_ext = Path::new(filename)
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

    fn path(&self) -> &Path {
        &self.path
    }
}

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
        let temp_dir = cfg.data_dir.join("tmp");
        if let Err(err) = xmedia_core::http::configure_temp_dir(temp_dir.clone()) {
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

        let bindings_table_created = if !is_core_schema_initialized(&mut db).await {
            tracing::info!(
                "cold start detected: pushing unified database schema for core and all enabled modules"
            );
            db.push_schema()
                .await
                .map_err(|e| anyhow::anyhow!("failed to push unified database schema: {e}"))?;
            false
        } else {
            let core_report = ensure_core_schema_with_report(&mut db, &cfg.database_url)
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
            core_report.bindings_table_created
        };

        let users_repo = Arc::new(ToastyUserRepository::new(db.clone()));
        let libraries_repo = Arc::new(ToastyLibraryRepository::new(db.clone()));
        let binding_repo = Arc::new(ToastyUserMediaBindingRepository::new(db.clone()));
        let online_items_repo = Arc::new(ToastyOnlineLibraryItemRepository::new(db.clone()));
        let activity_repo = Arc::new(ToastyUserActivityRepository::new(db.clone()));
        let activity = Arc::new(ActivityService::new(activity_repo));
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
                .with_activity(activity.clone())
                .with_online_items(online_items_repo)
                .with_bindings(bindings.clone()),
        );
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

            let comic_state = xmedia_comic::http::state::ComicHttpState::new(
                auth.clone(),
                libraries.clone(),
                comic_services,
            );
            let comic_router = xmedia_comic::http::rest_router(comic_state.clone());
            api_v1 = api_v1.nest("/comic", comic_router);
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

            let music_state = xmedia_music::http::state::MusicHttpState::new(
                auth.clone(),
                libraries.clone(),
                music_services,
            );
            let music_router = xmedia_music::http::rest_router(music_state.clone());
            api_v1 = api_v1.nest("/music", music_router);
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

            let video_state = xmedia_video::http::state::VideoHttpState::new(
                auth.clone(),
                libraries.clone(),
                video_services,
            );
            let video_router = xmedia_video::http::rest_router(video_state.clone());
            api_v1 = api_v1.nest("/video", video_router);
            root_protocols = root_protocols.nest(
                "/jellyfin",
                xmedia_video::http::jellyfin_router(video_state),
            );
        }

        if bindings_table_created {
            let admin_ids: Vec<_> = users_repo
                .list_all()
                .await
                .map_err(|e| anyhow::anyhow!(e.to_string()))?
                .into_iter()
                .filter(|u| u.role.is_admin())
                .map(|u| u.id)
                .collect();
            if !admin_ids.is_empty() {
                let mut total_bound = 0usize;
                for (kind, ids) in &backfill_batches {
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
            }
        }

        let _ = (db, opts.allow_private_ips);

        let gateway_state = GatewayState {
            auth: auth.clone(),
            libraries: libraries.clone(),
            libraries_repo,
            bindings,
            activity,
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
            temp_dir,
            #[cfg(feature = "book")]
            book_catalog,
            #[cfg(feature = "comic")]
            comic_catalog,
            #[cfg(feature = "music")]
            music_ingest,
        })
    }

    pub async fn import_local_book(
        &self,
        actor: &xmedia_core::domain::user::User,
        filename: String,
        data: bytes::Bytes,
    ) -> xmedia_core::domain::error::DomainResult<(String, String)> {
        #[cfg(feature = "book")]
        {
            let temp = ScopedImportTempFile::write_in(&self.temp_dir, &filename, &data).await?;
            let book = self
                .book_catalog
                .upload_book_from_path(actor, &filename, temp.path())
                .await?;
            Ok((book.id.to_string(), book.metadata.title))
        }
        #[cfg(not(feature = "book"))]
        {
            let _ = (&self.temp_dir, actor, filename, data);
            Err(xmedia_core::domain::error::DomainError::UnsupportedFormat(
                "book module is not enabled".into(),
            ))
        }
    }

    pub async fn import_local_comic(
        &self,
        actor: &xmedia_core::domain::user::User,
        filename: String,
        data: bytes::Bytes,
    ) -> xmedia_core::domain::error::DomainResult<(String, String)> {
        #[cfg(feature = "comic")]
        {
            let temp = ScopedImportTempFile::write_in(&self.temp_dir, &filename, &data).await?;
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
            Err(xmedia_core::domain::error::DomainError::UnsupportedFormat(
                "comic module is not enabled".into(),
            ))
        }
    }

    pub async fn import_local_track(
        &self,
        actor: &xmedia_core::domain::user::User,
        filename: String,
        data: bytes::Bytes,
    ) -> xmedia_core::domain::error::DomainResult<(String, String)> {
        #[cfg(feature = "music")]
        {
            let temp = ScopedImportTempFile::write_in(&self.temp_dir, &filename, &data).await?;
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
                xmedia_core::domain::error::DomainError::Validation(
                    "no audio track produced from imported payload".into(),
                )
            })?;
            Ok((track.id.to_string(), track.title))
        }
        #[cfg(not(feature = "music"))]
        {
            let _ = (&self.temp_dir, actor, filename, data);
            Err(xmedia_core::domain::error::DomainError::UnsupportedFormat(
                "music module is not enabled".into(),
            ))
        }
    }
}

#[cfg(test)]
mod tests;

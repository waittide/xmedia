mod dto;
mod handlers_media;
mod handlers_sources;
mod security;

#[cfg(test)]
mod tests;

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::middleware;
use axum::routing::{any, get, post, put};
use tokio::net::TcpListener;
use tokio::sync::{Mutex, Semaphore, oneshot};
use tokio::task::JoinHandle;

use crate::local_engine::LocalEngine;
use crate::opds_client::OpdsClient;
use crate::remote_sources::{RemoteSourceStore, SharedRemoteSourceStore};
use crate::subsonic_client::SubsonicClient;

pub use dto::{
    AppHealthResponse, MutatedRemoteSourceResponse, RemoteSourcesListResponse,
    SubsonicAlbumsResponse, TestRemoteSourceRequest, TestRemoteSourceResponse,
};

pub const APP_HEADER_NAME: &str = "x-xmedia-app";
pub const APP_HEADER_VALUE: &str = "standalone";

const MAX_CONCURRENT_IMPORTS: usize = 4;

#[derive(Clone)]
pub struct StandaloneGatewayState {
    pub local_engine: LocalEngine,
    pub sources: SharedRemoteSourceStore,
    pub sources_write_lock: Arc<Mutex<()>>,
    pub opds: OpdsClient,
    pub subsonic: SubsonicClient,
    pub local_gateway_url: String,
    pub expected_port: u16,
}

impl StandaloneGatewayState {
    pub fn new(
        local_engine: LocalEngine,
        sources: SharedRemoteSourceStore,
        local_gateway_url: String,
        expected_port: u16,
    ) -> anyhow::Result<Self> {
        let secure = security::build_remote_http_client(false)?;
        let insecure = security::build_remote_http_client(true)?;
        let local_upload = security::build_local_upload_http_client()?;
        let import_semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_IMPORTS));
        let opds = OpdsClient::new(
            secure.clone(),
            insecure.clone(),
            local_upload.clone(),
            import_semaphore.clone(),
            local_engine.temp_dir.clone(),
        );
        let subsonic = SubsonicClient::new(
            secure,
            insecure,
            local_upload,
            import_semaphore,
            local_engine.temp_dir.clone(),
        );
        Ok(Self {
            local_engine,
            sources,
            sources_write_lock: Arc::new(Mutex::new(())),
            opds,
            subsonic,
            local_gateway_url,
            expected_port,
        })
    }
}

pub fn format_gateway_url(addr: SocketAddr) -> String {
    let port = addr.port();
    match addr.ip() {
        IpAddr::V4(ip) => {
            let host = if ip.is_unspecified() {
                Ipv4Addr::LOCALHOST
            } else {
                ip
            };
            format!("http://{host}:{port}")
        }
        IpAddr::V6(ip) => {
            let host = if ip.is_unspecified() {
                Ipv6Addr::LOCALHOST
            } else {
                ip
            };
            format!("http://[{host}]:{port}")
        }
    }
}

pub struct RunningGateway {
    pub addr: SocketAddr,
    pub url: String,
    pub local_engine: LocalEngine,
    shutdown_tx: Option<oneshot::Sender<()>>,
    handle: Option<JoinHandle<()>>,
}

impl RunningGateway {
    pub async fn shutdown(mut self) {
        self.shutdown_gracefully(Duration::from_secs(5)).await;
    }

    pub async fn shutdown_gracefully(&mut self, timeout: Duration) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.handle.take()
            && tokio::time::timeout(timeout, handle).await.is_err()
        {
            tracing::warn!("timed out waiting for standalone local gateway shutdown");
        }
    }
}

impl Drop for RunningGateway {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

pub fn build_gateway_router(state: StandaloneGatewayState, xmedia_router: Router) -> Router {
    let app_router = Router::new()
        .route("/_app", any(dto::app_not_found))
        .route("/_app/", any(dto::app_not_found))
        .route("/_app/health", get(handlers_sources::app_health))
        .route(
            "/_app/auto-session",
            post(handlers_sources::app_auto_session),
        )
        .route(
            "/_app/remote-sources",
            get(handlers_sources::list_remote_sources).post(handlers_sources::create_remote_source),
        )
        .route(
            "/_app/remote-sources/test",
            post(handlers_sources::test_remote_source),
        )
        .route(
            "/_app/remote-sources/{id}",
            put(handlers_sources::update_remote_source)
                .delete(handlers_sources::delete_remote_source),
        )
        .route("/_app/opds/{id}/browse", get(handlers_media::opds_browse))
        .route(
            "/_app/opds/{id}/proxy-image",
            get(handlers_media::opds_proxy_image),
        )
        .route("/_app/opds/{id}/import", post(handlers_media::opds_import))
        .route(
            "/_app/subsonic/{id}/albums",
            get(handlers_media::subsonic_list_albums),
        )
        .route(
            "/_app/subsonic/{id}/albums/{album_id}",
            get(handlers_media::subsonic_get_album),
        )
        .route(
            "/_app/subsonic/{id}/search",
            get(handlers_media::subsonic_search),
        )
        .route(
            "/_app/subsonic/{id}/cover/{cover_id}",
            get(handlers_media::subsonic_proxy_cover),
        )
        .route(
            "/_app/subsonic/{id}/stream/{song_id}",
            get(handlers_media::subsonic_proxy_stream).head(handlers_media::subsonic_proxy_stream),
        )
        .route(
            "/_app/subsonic/{id}/import/{song_id}",
            post(handlers_media::subsonic_import_song),
        )
        .route("/_app/{*path}", any(dto::app_not_found))
        .with_state(state.clone());

    app_router
        .merge(xmedia_router)
        .layer(middleware::from_fn_with_state(
            state,
            security::guard_app_loopback_request,
        ))
}

pub async fn start_gateway(
    bind_addr: SocketAddr,
    data_dir: PathBuf,
    external_web_dir: Option<PathBuf>,
) -> anyhow::Result<RunningGateway> {
    let sources = RemoteSourceStore::open_shared(data_dir.join("remote_sources.json"))?;
    let (local_engine, xmedia_router) = LocalEngine::start(data_dir, external_web_dir).await?;

    let listener = TcpListener::bind(bind_addr).await?;
    let addr = listener.local_addr()?;
    let url = format_gateway_url(addr);

    let state =
        StandaloneGatewayState::new(local_engine.clone(), sources, url.clone(), addr.port())?;
    let router = build_gateway_router(state, xmedia_router);

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let handle = tokio::spawn(async move {
        let server = axum::serve(listener, router).with_graceful_shutdown(async move {
            let _ = shutdown_rx.await;
        });
        if let Err(err) = server.await {
            tracing::error!(error = %err, "standalone local loopback gateway exited with error");
        }
    });

    Ok(RunningGateway {
        addr,
        url,
        local_engine,
        shutdown_tx: Some(shutdown_tx),
        handle: Some(handle),
    })
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut sig) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            sig.recv().await;
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
}

pub async fn run_gateway_forever(
    bind_addr: SocketAddr,
    data_dir: PathBuf,
    external_web_dir: Option<PathBuf>,
) -> anyhow::Result<()> {
    let sources = RemoteSourceStore::open_shared(data_dir.join("remote_sources.json"))?;
    let (local_engine, xmedia_router) = LocalEngine::start(data_dir, external_web_dir).await?;

    let listener = TcpListener::bind(bind_addr).await?;
    let addr = listener.local_addr()?;
    let url = format_gateway_url(addr);

    let state = StandaloneGatewayState::new(local_engine, sources, url.clone(), addr.port())?;
    let router = build_gateway_router(state, xmedia_router);

    tracing::info!(%url, "XMedia standalone local loopback gateway listening");
    println!("XMEDIA_GATEWAY_URL={url}");
    let _ = std::io::Write::flush(&mut std::io::stdout());

    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::header::{CACHE_CONTROL, SET_COOKIE};
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post, put};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use xmedia_core::domain::library::MediaKind;

use crate::local_engine::LocalEngine;
use crate::opds_client::{OpdsClient, OpdsError};
use crate::remote_sources::{
    CreateRemoteSourceInput, RemoteProtocol, RemoteSource, RemoteSourceStore,
    RemoteSourceSummaryDto, SharedRemoteSourceStore, SourceStoreError, UpdateRemoteSourceInput,
};
use crate::subsonic_client::{SubsonicAlbumDto, SubsonicClient, SubsonicError};

pub const APP_HEADER_NAME: &str = "x-xmedia-app";
pub const APP_HEADER_VALUE: &str = "standalone";

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone)]
pub struct StandaloneGatewayState {
    pub local_engine: LocalEngine,
    pub sources: SharedRemoteSourceStore,
    pub opds: OpdsClient,
    pub subsonic: SubsonicClient,
    pub local_gateway_url: String,
}

impl StandaloneGatewayState {
    pub fn new(
        local_engine: LocalEngine,
        sources: SharedRemoteSourceStore,
        local_gateway_url: String,
    ) -> anyhow::Result<Self> {
        let secure = build_remote_http_client(false)?;
        let insecure = build_remote_http_client(true)?;
        let opds = OpdsClient::new(secure.clone(), insecure.clone());
        let subsonic = SubsonicClient::new(secure, insecure);
        Ok(Self {
            local_engine,
            sources,
            opds,
            subsonic,
            local_gateway_url,
        })
    }
}

fn build_remote_http_client(allow_insecure_tls: bool) -> anyhow::Result<reqwest::Client> {
    let client = reqwest::Client::builder()
        .use_rustls_tls()
        .connect_timeout(CONNECT_TIMEOUT)
        .redirect(reqwest::redirect::Policy::limited(10))
        .danger_accept_invalid_certs(allow_insecure_tls)
        .build()?;
    Ok(client)
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
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.await;
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
        .route("/_app/health", get(app_health))
        .route("/_app/auto-session", get(app_auto_session))
        .route(
            "/_app/remote-sources",
            get(list_remote_sources).post(create_remote_source),
        )
        .route("/_app/remote-sources/test", post(test_remote_source))
        .route(
            "/_app/remote-sources/{id}",
            put(update_remote_source).delete(delete_remote_source),
        )
        .route("/_app/opds/{id}/browse", get(opds_browse))
        .route("/_app/opds/{id}/proxy-image", get(opds_proxy_image))
        .route("/_app/opds/{id}/import", post(opds_import))
        .route("/_app/subsonic/{id}/albums", get(subsonic_list_albums))
        .route(
            "/_app/subsonic/{id}/albums/{album_id}",
            get(subsonic_get_album),
        )
        .route("/_app/subsonic/{id}/search", get(subsonic_search))
        .route(
            "/_app/subsonic/{id}/cover/{cover_id}",
            get(subsonic_proxy_cover),
        )
        .route(
            "/_app/subsonic/{id}/stream/{song_id}",
            get(subsonic_proxy_stream),
        )
        .route(
            "/_app/subsonic/{id}/import/{song_id}",
            post(subsonic_import_song),
        )
        .route("/_app/{*path}", any(app_not_found))
        .with_state(state);

    app_router.merge(xmedia_router)
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
    let url = format!("http://127.0.0.1:{}", addr.port());

    let state = StandaloneGatewayState::new(local_engine.clone(), sources, url.clone())?;
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

pub async fn run_gateway_forever(
    bind_addr: SocketAddr,
    data_dir: PathBuf,
    external_web_dir: Option<PathBuf>,
) -> anyhow::Result<()> {
    let sources = RemoteSourceStore::open_shared(data_dir.join("remote_sources.json"))?;
    let (local_engine, xmedia_router) = LocalEngine::start(data_dir, external_web_dir).await?;

    let listener = TcpListener::bind(bind_addr).await?;
    let addr = listener.local_addr()?;
    let url = format!("http://127.0.0.1:{}", addr.port());

    let state = StandaloneGatewayState::new(local_engine, sources, url.clone())?;
    let router = build_gateway_router(state, xmedia_router);

    tracing::info!(%url, "XMedia standalone local loopback gateway listening");
    println!("XMEDIA_GATEWAY_URL={url}");
    let _ = std::io::Write::flush(&mut std::io::stdout());

    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
struct ErrorBody {
    code: String,
    message: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ErrorEnvelope {
    error: ErrorBody,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AppHealthResponse {
    pub mode: String,
    pub version: String,
    pub remote_source_count: usize,
    pub enabled_modules: Vec<MediaKind>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RemoteSourcesListResponse {
    pub sources: Vec<RemoteSourceSummaryDto>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MutatedRemoteSourceResponse {
    pub source: RemoteSourceSummaryDto,
    pub sources: Vec<RemoteSourceSummaryDto>,
}

#[derive(Debug, Deserialize)]
pub struct TestRemoteSourceRequest {
    pub protocol: RemoteProtocol,
    pub url: String,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub allow_insecure_tls: bool,
    #[serde(default)]
    pub source_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TestRemoteSourceResponse {
    pub ok: bool,
    pub latency_ms: u64,
    pub protocol: RemoteProtocol,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct OpdsBrowseQuery {
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    q: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpdsProxyImageQuery {
    url: String,
}

#[derive(Debug, Deserialize)]
struct OpdsImportRequest {
    url: String,
    #[serde(default)]
    filename: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct SubsonicAlbumsQuery {
    #[serde(rename = "type", default)]
    list_type: Option<String>,
    #[serde(default)]
    size: Option<u32>,
    #[serde(default)]
    offset: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SubsonicAlbumsResponse {
    pub albums: Vec<SubsonicAlbumDto>,
}

#[derive(Debug, Deserialize, Default)]
struct SubsonicSearchQuery {
    #[serde(default)]
    q: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct SubsonicImportRequest {
    #[serde(default)]
    filename: Option<String>,
}

fn with_app_headers(mut res: Response) -> Response {
    res.headers_mut().insert(
        HeaderName::from_static(APP_HEADER_NAME),
        HeaderValue::from_static(APP_HEADER_VALUE),
    );
    res.headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    res
}

fn app_error(status: StatusCode, code: &str, message: impl Into<String>) -> Response {
    let body = ErrorEnvelope {
        error: ErrorBody {
            code: code.to_string(),
            message: message.into(),
        },
    };
    with_app_headers((status, Json(body)).into_response())
}

fn store_error_response(err: SourceStoreError) -> Response {
    match err {
        SourceStoreError::InvalidUrl(msg) => app_error(StatusCode::BAD_REQUEST, "invalid_url", msg),
        SourceStoreError::NotFound(msg) => app_error(StatusCode::NOT_FOUND, "not_found", msg),
        SourceStoreError::Io(msg) => {
            app_error(StatusCode::INTERNAL_SERVER_ERROR, "storage_error", msg)
        }
    }
}

fn opds_error_response(err: OpdsError) -> Response {
    app_error(err.status_code(), err.code(), err.to_string())
}

fn subsonic_error_response(err: SubsonicError) -> Response {
    app_error(err.status_code(), err.code(), err.to_string())
}

fn load_source_with_protocol(
    state: &StandaloneGatewayState,
    id: &str,
    expected: RemoteProtocol,
) -> Result<RemoteSource, Response> {
    let guard = state.sources.read().map_err(|_| {
        app_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "store_poisoned",
            "Remote source store lock poisoned",
        )
    })?;
    let Some(src) = guard.get(id) else {
        return Err(app_error(
            StatusCode::NOT_FOUND,
            "not_found",
            format!("Remote source '{id}' not found"),
        ));
    };
    if src.protocol != expected {
        return Err(app_error(
            StatusCode::BAD_REQUEST,
            "protocol_mismatch",
            format!(
                "Remote source '{id}' uses protocol '{}', expected '{}'",
                src.protocol.as_str(),
                expected.as_str()
            ),
        ));
    }
    Ok(src)
}

fn mark_source_ok(state: &StandaloneGatewayState, id: &str) {
    if let Ok(mut guard) = state.sources.write() {
        guard.mark_ok(id);
    }
}

async fn app_health(State(state): State<StandaloneGatewayState>) -> Response {
    let remote_source_count = match state.sources.read() {
        Ok(guard) => guard.count(),
        Err(_) => {
            return app_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "store_poisoned",
                "Remote source store lock poisoned",
            );
        }
    };
    with_app_headers(
        Json(AppHealthResponse {
            mode: "standalone".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            remote_source_count,
            enabled_modules: state.local_engine.enabled_modules.clone(),
        })
        .into_response(),
    )
}

async fn app_auto_session(State(state): State<StandaloneGatewayState>) -> Response {
    match state.local_engine.issue_auto_session().await {
        Ok(session) => {
            let cookie = state.local_engine.session_cookie_header(&session.token);
            let mut res = with_app_headers((StatusCode::OK, Json(session)).into_response());
            res.headers_mut().append(SET_COOKIE, cookie);
            res
        }
        Err(err) => app_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "auto_session_failed",
            err.to_string(),
        ),
    }
}

async fn list_remote_sources(State(state): State<StandaloneGatewayState>) -> Response {
    let sources = match state.sources.read() {
        Ok(guard) => guard.list_summaries(),
        Err(_) => {
            return app_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "store_poisoned",
                "Remote source store lock poisoned",
            );
        }
    };
    with_app_headers(Json(RemoteSourcesListResponse { sources }).into_response())
}

async fn create_remote_source(
    State(state): State<StandaloneGatewayState>,
    Json(input): Json<CreateRemoteSourceInput>,
) -> Response {
    let outcome = match state.sources.write() {
        Ok(mut guard) => guard.create(input),
        Err(_) => {
            return app_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "store_poisoned",
                "Remote source store lock poisoned",
            );
        }
    };
    match outcome {
        Ok((source, sources)) => with_app_headers(
            (
                StatusCode::CREATED,
                Json(MutatedRemoteSourceResponse { source, sources }),
            )
                .into_response(),
        ),
        Err(err) => store_error_response(err),
    }
}

async fn update_remote_source(
    State(state): State<StandaloneGatewayState>,
    AxumPath(id): AxumPath<String>,
    Json(input): Json<UpdateRemoteSourceInput>,
) -> Response {
    let outcome = match state.sources.write() {
        Ok(mut guard) => guard.update(&id, input),
        Err(_) => {
            return app_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "store_poisoned",
                "Remote source store lock poisoned",
            );
        }
    };
    match outcome {
        Ok((source, sources)) => {
            with_app_headers(Json(MutatedRemoteSourceResponse { source, sources }).into_response())
        }
        Err(err) => store_error_response(err),
    }
}

async fn delete_remote_source(
    State(state): State<StandaloneGatewayState>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let outcome = match state.sources.write() {
        Ok(mut guard) => guard.delete(&id),
        Err(_) => {
            return app_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "store_poisoned",
                "Remote source store lock poisoned",
            );
        }
    };
    match outcome {
        Ok(sources) => {
            with_app_headers(Json(RemoteSourcesListResponse { sources }).into_response())
        }
        Err(err) => store_error_response(err),
    }
}

async fn test_remote_source(
    State(state): State<StandaloneGatewayState>,
    Json(input): Json<TestRemoteSourceRequest>,
) -> Response {
    let username = input.username.unwrap_or_default();
    let mut password = input.password.unwrap_or_default();
    if password.is_empty()
        && let Some(ref source_id) = input.source_id
        && let Ok(guard) = state.sources.read()
        && let Some(existing) = guard.get(source_id)
    {
        password = existing.password;
    }

    let response = match input.protocol {
        RemoteProtocol::Opds => {
            let res = state
                .opds
                .test_opds(&input.url, &username, &password, input.allow_insecure_tls)
                .await;
            let detail = if res.ok {
                let ver = res.opds_version.as_deref().unwrap_or("1.2");
                let title = res.title.as_deref().unwrap_or("OPDS Catalog");
                let count = res.entry_count.unwrap_or(0);
                Some(format!("OPDS {ver} · {title} ({count})"))
            } else {
                None
            };
            TestRemoteSourceResponse {
                ok: res.ok,
                latency_ms: res.latency_ms,
                protocol: RemoteProtocol::Opds,
                detail,
                error: res.error,
            }
        }
        RemoteProtocol::Subsonic => {
            let res = state
                .subsonic
                .test_subsonic(&input.url, &username, &password, input.allow_insecure_tls)
                .await;
            let detail = if res.ok {
                let label = if res.open_subsonic.unwrap_or(false) {
                    "OpenSubsonic"
                } else {
                    "Subsonic"
                };
                match res.server_version.as_deref() {
                    Some(v) if !v.is_empty() => Some(format!("{label} {v}")),
                    _ => Some(label.to_string()),
                }
            } else {
                None
            };
            TestRemoteSourceResponse {
                ok: res.ok,
                latency_ms: res.latency_ms,
                protocol: RemoteProtocol::Subsonic,
                detail,
                error: res.error,
            }
        }
    };

    if response.ok
        && let Some(ref source_id) = input.source_id
    {
        mark_source_ok(&state, source_id);
    }

    with_app_headers(Json(response).into_response())
}

async fn opds_browse(
    State(state): State<StandaloneGatewayState>,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<OpdsBrowseQuery>,
) -> Response {
    let source = match load_source_with_protocol(&state, &id, RemoteProtocol::Opds) {
        Ok(s) => s,
        Err(resp) => return resp,
    };

    match state
        .opds
        .browse_opds(&source, query.url.as_deref(), query.q.as_deref())
        .await
    {
        Ok(feed) => {
            mark_source_ok(&state, &id);
            with_app_headers(Json(feed).into_response())
        }
        Err(err) => opds_error_response(err),
    }
}

async fn opds_proxy_image(
    State(state): State<StandaloneGatewayState>,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<OpdsProxyImageQuery>,
) -> Response {
    let source = match load_source_with_protocol(&state, &id, RemoteProtocol::Opds) {
        Ok(s) => s,
        Err(resp) => return resp,
    };

    match state.opds.proxy_image(&source, &query.url).await {
        Ok(res) => res,
        Err(err) => opds_error_response(err),
    }
}

async fn opds_import(
    State(state): State<StandaloneGatewayState>,
    AxumPath(id): AxumPath<String>,
    Json(input): Json<OpdsImportRequest>,
) -> Response {
    let source = match load_source_with_protocol(&state, &id, RemoteProtocol::Opds) {
        Ok(s) => s,
        Err(resp) => return resp,
    };

    let session = match state.local_engine.issue_auto_session().await {
        Ok(s) => s,
        Err(err) => {
            return app_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "auto_session_failed",
                err.to_string(),
            );
        }
    };

    match state
        .opds
        .import_to_local(
            &source,
            &input.url,
            input.filename.as_deref(),
            &state.local_gateway_url,
            &session.token,
        )
        .await
    {
        Ok(result) => with_app_headers((StatusCode::CREATED, Json(result)).into_response()),
        Err(err) => opds_error_response(err),
    }
}

async fn subsonic_list_albums(
    State(state): State<StandaloneGatewayState>,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<SubsonicAlbumsQuery>,
) -> Response {
    let source = match load_source_with_protocol(&state, &id, RemoteProtocol::Subsonic) {
        Ok(s) => s,
        Err(resp) => return resp,
    };

    let list_type = query.list_type.as_deref().unwrap_or("newest");
    let size = query.size.unwrap_or(30);
    let offset = query.offset.unwrap_or(0);

    match state
        .subsonic
        .list_albums(&source, list_type, size, offset)
        .await
    {
        Ok(albums) => {
            mark_source_ok(&state, &id);
            with_app_headers(Json(SubsonicAlbumsResponse { albums }).into_response())
        }
        Err(err) => subsonic_error_response(err),
    }
}

async fn subsonic_get_album(
    State(state): State<StandaloneGatewayState>,
    AxumPath((id, album_id)): AxumPath<(String, String)>,
) -> Response {
    let source = match load_source_with_protocol(&state, &id, RemoteProtocol::Subsonic) {
        Ok(s) => s,
        Err(resp) => return resp,
    };

    match state.subsonic.get_album(&source, &album_id).await {
        Ok(detail) => {
            mark_source_ok(&state, &id);
            with_app_headers(Json(detail).into_response())
        }
        Err(err) => subsonic_error_response(err),
    }
}

async fn subsonic_search(
    State(state): State<StandaloneGatewayState>,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<SubsonicSearchQuery>,
) -> Response {
    let source = match load_source_with_protocol(&state, &id, RemoteProtocol::Subsonic) {
        Ok(s) => s,
        Err(resp) => return resp,
    };

    match state
        .subsonic
        .search(&source, query.q.as_deref().unwrap_or(""))
        .await
    {
        Ok(res) => {
            mark_source_ok(&state, &id);
            with_app_headers(Json(res).into_response())
        }
        Err(err) => subsonic_error_response(err),
    }
}

async fn subsonic_proxy_cover(
    State(state): State<StandaloneGatewayState>,
    AxumPath((id, cover_id)): AxumPath<(String, String)>,
) -> Response {
    let source = match load_source_with_protocol(&state, &id, RemoteProtocol::Subsonic) {
        Ok(s) => s,
        Err(resp) => return resp,
    };

    match state.subsonic.proxy_cover(&source, &cover_id).await {
        Ok(res) => res,
        Err(err) => subsonic_error_response(err),
    }
}

async fn subsonic_proxy_stream(
    State(state): State<StandaloneGatewayState>,
    AxumPath((id, song_id)): AxumPath<(String, String)>,
    headers: HeaderMap,
) -> Response {
    let source = match load_source_with_protocol(&state, &id, RemoteProtocol::Subsonic) {
        Ok(s) => s,
        Err(resp) => return resp,
    };

    match state
        .subsonic
        .proxy_stream(&source, &song_id, &headers)
        .await
    {
        Ok(res) => res,
        Err(err) => subsonic_error_response(err),
    }
}

async fn subsonic_import_song(
    State(state): State<StandaloneGatewayState>,
    AxumPath((id, song_id)): AxumPath<(String, String)>,
    body: Bytes,
) -> Response {
    let source = match load_source_with_protocol(&state, &id, RemoteProtocol::Subsonic) {
        Ok(s) => s,
        Err(resp) => return resp,
    };

    let input: SubsonicImportRequest = if body.is_empty() {
        SubsonicImportRequest::default()
    } else {
        match serde_json::from_slice(&body) {
            Ok(v) => v,
            Err(err) => {
                return app_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_json",
                    format!("Invalid import request body: {err}"),
                );
            }
        }
    };

    let session = match state.local_engine.issue_auto_session().await {
        Ok(s) => s,
        Err(err) => {
            return app_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "auto_session_failed",
                err.to_string(),
            );
        }
    };

    match state
        .subsonic
        .import_song_to_local(
            &source,
            &song_id,
            input.filename.as_deref(),
            &state.local_gateway_url,
            &session.token,
        )
        .await
    {
        Ok(result) => with_app_headers((StatusCode::CREATED, Json(result)).into_response()),
        Err(err) => subsonic_error_response(err),
    }
}

async fn app_not_found() -> Response {
    app_error(
        StatusCode::NOT_FOUND,
        "not_found",
        "Unknown client API route",
    )
}

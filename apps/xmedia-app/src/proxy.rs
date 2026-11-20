use std::collections::HashSet;
use std::net::SocketAddr;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

use axum::Json;
use axum::Router;
use axum::body::Body;
use axum::extract::{Path as AxumPath, State};
use axum::http::header::{
    CACHE_CONTROL, CONNECTION, CONTENT_LENGTH, CONTENT_TYPE, HOST, SET_COOKIE, TRANSFER_ENCODING,
};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, Request, StatusCode, Uri};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{any, delete, get, post, put};
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use crate::config::{ConfigError, ServerProfile, SharedServerStore, normalize_server_url};

pub const APP_HEADER_NAME: &str = "x-xmedia-app";
pub const APP_HEADER_VALUE: &str = "tauri-client";

const CLEAR_AUTH_COOKIE: &str = "xmedia_token=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const TEST_PROBE_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(RustEmbed)]
#[folder = "../../web/dist"]
#[allow_missing = true]
struct EmbeddedWebAssets;

#[derive(Clone)]
pub struct ProxyState {
    pub store: SharedServerStore,
    pub secure_client: reqwest::Client,
    pub insecure_client: reqwest::Client,
    pub external_dist_dir: Option<PathBuf>,
}

impl ProxyState {
    pub fn new(
        store: SharedServerStore,
        external_dist_dir: Option<PathBuf>,
    ) -> anyhow::Result<Self> {
        let secure_client = build_proxy_client(false)?;
        let insecure_client = build_proxy_client(true)?;
        Ok(Self {
            store,
            secure_client,
            insecure_client,
            external_dist_dir,
        })
    }

    fn client_for(&self, allow_insecure_tls: bool) -> &reqwest::Client {
        if allow_insecure_tls {
            &self.insecure_client
        } else {
            &self.secure_client
        }
    }
}

fn build_proxy_client(allow_insecure_tls: bool) -> anyhow::Result<reqwest::Client> {
    // Keep connection pooling enabled and avoid global request timeout so SSE
    // event streams and large media range/upload streams are never interrupted.
    let client = reqwest::Client::builder()
        .use_rustls_tls()
        .connect_timeout(CONNECT_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .danger_accept_invalid_certs(allow_insecure_tls)
        .build()?;
    Ok(client)
}

pub struct RunningGateway {
    pub addr: SocketAddr,
    pub url: String,
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

pub fn build_gateway_router(
    store: SharedServerStore,
    external_dist_dir: Option<PathBuf>,
) -> anyhow::Result<Router> {
    let state = ProxyState::new(store, external_dist_dir)?;
    let router = Router::new()
        .route("/_app/health", get(app_health))
        .route("/_app/servers", get(list_servers).post(upsert_server))
        .route("/_app/servers/test", post(test_server))
        .route("/_app/servers/{id}/activate", put(activate_server))
        .route("/_app/servers/{id}", delete(delete_server))
        .route("/_app/{*path}", any(app_not_found))
        .route("/api", any(proxy_to_active_server))
        .route("/api/{*path}", any(proxy_to_active_server))
        .route("/opds", any(proxy_to_active_server))
        .route("/opds/{*path}", any(proxy_to_active_server))
        .route("/rest", any(proxy_to_active_server))
        .route("/rest/{*path}", any(proxy_to_active_server))
        .fallback(get(serve_spa))
        .with_state(state);
    Ok(router)
}

pub async fn start_gateway(
    bind_addr: SocketAddr,
    store: SharedServerStore,
    external_dist_dir: Option<PathBuf>,
) -> anyhow::Result<RunningGateway> {
    let router = build_gateway_router(store, external_dist_dir)?;
    let listener = TcpListener::bind(bind_addr).await?;
    let addr = listener.local_addr()?;
    let url = format!("http://127.0.0.1:{}", addr.port());

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let handle = tokio::spawn(async move {
        let server = axum::serve(listener, router).with_graceful_shutdown(async move {
            let _ = shutdown_rx.await;
        });
        if let Err(err) = server.await {
            tracing::error!(error = %err, "local loopback gateway exited with error");
        }
    });

    Ok(RunningGateway {
        addr,
        url,
        shutdown_tx: Some(shutdown_tx),
        handle: Some(handle),
    })
}

pub async fn run_gateway_forever(
    bind_addr: SocketAddr,
    store: SharedServerStore,
    external_dist_dir: Option<PathBuf>,
) -> anyhow::Result<()> {
    let router = build_gateway_router(store, external_dist_dir)?;
    let listener = TcpListener::bind(bind_addr).await?;
    let addr = listener.local_addr()?;
    let url = format!("http://127.0.0.1:{}", addr.port());
    tracing::info!(%url, "XMedia local loopback gateway listening");
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
    pub active_server: Option<ServerProfile>,
    pub has_servers: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ServersListResponse {
    pub active_server_id: Option<String>,
    pub servers: Vec<ServerProfile>,
}

#[derive(Debug, Deserialize)]
pub struct UpsertServerRequest {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub allow_insecure_tls: bool,
    #[serde(default)]
    pub activate: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MutatedServerResponse {
    pub active_server_id: Option<String>,
    pub server: ServerProfile,
    pub servers: Vec<ServerProfile>,
}

#[derive(Debug, Deserialize)]
pub struct TestServerRequest {
    pub url: String,
    #[serde(default)]
    pub allow_insecure_tls: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TestServerResponse {
    pub ok: bool,
    pub latency_ms: u64,
    pub normalized_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub needs_setup: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CapabilitiesProbe {
    version: String,
}

#[derive(Debug, Deserialize)]
struct SetupStatusProbe {
    needs_setup: bool,
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

fn attach_clear_auth_cookie(mut res: Response) -> Response {
    res.headers_mut()
        .append(SET_COOKIE, HeaderValue::from_static(CLEAR_AUTH_COOKIE));
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

fn config_error_response(err: ConfigError) -> Response {
    match err {
        ConfigError::InvalidUrl(msg) => app_error(StatusCode::BAD_REQUEST, "invalid_url", msg),
        ConfigError::NotFound(msg) => app_error(StatusCode::NOT_FOUND, "not_found", msg),
        ConfigError::Io(msg) => app_error(StatusCode::INTERNAL_SERVER_ERROR, "storage_error", msg),
    }
}

async fn app_health(State(state): State<ProxyState>) -> Response {
    let (active_server, has_servers) = match state.store.read() {
        Ok(guard) => (guard.active_server(), guard.has_servers()),
        Err(_) => {
            return app_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "store_poisoned",
                "Server configuration store lock poisoned",
            );
        }
    };
    with_app_headers(
        Json(AppHealthResponse {
            mode: "tauri".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            active_server,
            has_servers,
        })
        .into_response(),
    )
}

async fn list_servers(State(state): State<ProxyState>) -> Response {
    let snapshot = match state.store.read() {
        Ok(guard) => guard.snapshot(),
        Err(_) => {
            return app_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "store_poisoned",
                "Server configuration store lock poisoned",
            );
        }
    };
    with_app_headers(
        Json(ServersListResponse {
            active_server_id: snapshot.active_server_id,
            servers: snapshot.servers,
        })
        .into_response(),
    )
}

async fn upsert_server(
    State(state): State<ProxyState>,
    Json(input): Json<UpsertServerRequest>,
) -> Response {
    let outcome = match state.store.write() {
        Ok(mut guard) => guard.upsert_server(
            input.id,
            input.name,
            input.url,
            input.allow_insecure_tls,
            input.activate,
        ),
        Err(_) => {
            return app_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "store_poisoned",
                "Server configuration store lock poisoned",
            );
        }
    };

    match outcome {
        Ok(out) => {
            let mut res = with_app_headers(
                Json(MutatedServerResponse {
                    active_server_id: out.snapshot.active_server_id,
                    server: out.server,
                    servers: out.snapshot.servers,
                })
                .into_response(),
            );
            if out.active_changed {
                res = attach_clear_auth_cookie(res);
            }
            res
        }
        Err(err) => config_error_response(err),
    }
}

async fn activate_server(
    State(state): State<ProxyState>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let outcome = match state.store.write() {
        Ok(mut guard) => guard.activate_server(&id),
        Err(_) => {
            return app_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "store_poisoned",
                "Server configuration store lock poisoned",
            );
        }
    };

    match outcome {
        Ok(out) => {
            let mut res = with_app_headers(
                Json(MutatedServerResponse {
                    active_server_id: out.snapshot.active_server_id,
                    server: out.server,
                    servers: out.snapshot.servers,
                })
                .into_response(),
            );
            if out.active_changed {
                res = attach_clear_auth_cookie(res);
            }
            res
        }
        Err(err) => config_error_response(err),
    }
}

async fn delete_server(
    State(state): State<ProxyState>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let outcome = match state.store.write() {
        Ok(mut guard) => guard.delete_server(&id),
        Err(_) => {
            return app_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "store_poisoned",
                "Server configuration store lock poisoned",
            );
        }
    };

    match outcome {
        Ok(out) => {
            let mut res = with_app_headers(
                Json(ServersListResponse {
                    active_server_id: out.snapshot.active_server_id,
                    servers: out.snapshot.servers,
                })
                .into_response(),
            );
            if out.cleared_active {
                res = attach_clear_auth_cookie(res);
            }
            res
        }
        Err(err) => config_error_response(err),
    }
}

async fn test_server(
    State(state): State<ProxyState>,
    Json(input): Json<TestServerRequest>,
) -> Response {
    let normalized_url = match normalize_server_url(&input.url) {
        Ok(u) => u,
        Err(err) => {
            return with_app_headers(
                Json(TestServerResponse {
                    ok: false,
                    latency_ms: 0,
                    normalized_url: String::new(),
                    needs_setup: None,
                    version: None,
                    error: Some(err.to_string()),
                })
                .into_response(),
            );
        }
    };

    let client = state.client_for(input.allow_insecure_tls);
    let caps_url = format!("{normalized_url}/api/v1/system/capabilities");
    let setup_url = format!("{normalized_url}/api/v1/auth/setup-status");

    let started = Instant::now();
    let (caps_res, setup_res) = tokio::join!(
        client.get(&caps_url).timeout(TEST_PROBE_TIMEOUT).send(),
        client.get(&setup_url).timeout(TEST_PROBE_TIMEOUT).send(),
    );
    let latency_ms = started.elapsed().as_millis() as u64;

    let caps_resp = match caps_res {
        Ok(r) => r,
        Err(err) => {
            return with_app_headers(
                Json(TestServerResponse {
                    ok: false,
                    latency_ms,
                    normalized_url,
                    needs_setup: None,
                    version: None,
                    error: Some(format!("Connection failed: {err}")),
                })
                .into_response(),
            );
        }
    };

    if !caps_resp.status().is_success() {
        return with_app_headers(
            Json(TestServerResponse {
                ok: false,
                latency_ms,
                normalized_url,
                needs_setup: None,
                version: None,
                error: Some(format!(
                    "Server capabilities endpoint returned HTTP {}",
                    caps_resp.status().as_u16()
                )),
            })
            .into_response(),
        );
    }

    let caps: CapabilitiesProbe = match caps_resp.json().await {
        Ok(v) => v,
        Err(err) => {
            return with_app_headers(
                Json(TestServerResponse {
                    ok: false,
                    latency_ms,
                    normalized_url,
                    needs_setup: None,
                    version: None,
                    error: Some(format!("Not a valid XMedia server response: {err}")),
                })
                .into_response(),
            );
        }
    };

    let setup_resp = match setup_res {
        Ok(r) => r,
        Err(err) => {
            return with_app_headers(
                Json(TestServerResponse {
                    ok: false,
                    latency_ms,
                    normalized_url,
                    needs_setup: None,
                    version: Some(caps.version),
                    error: Some(format!("Failed to check setup status: {err}")),
                })
                .into_response(),
            );
        }
    };

    if !setup_resp.status().is_success() {
        return with_app_headers(
            Json(TestServerResponse {
                ok: false,
                latency_ms,
                normalized_url,
                needs_setup: None,
                version: Some(caps.version),
                error: Some(format!(
                    "Setup status endpoint returned HTTP {}",
                    setup_resp.status().as_u16()
                )),
            })
            .into_response(),
        );
    }

    let setup: SetupStatusProbe = match setup_resp.json().await {
        Ok(v) => v,
        Err(err) => {
            return with_app_headers(
                Json(TestServerResponse {
                    ok: false,
                    latency_ms,
                    normalized_url,
                    needs_setup: None,
                    version: Some(caps.version),
                    error: Some(format!("Invalid setup status response: {err}")),
                })
                .into_response(),
            );
        }
    };

    with_app_headers(
        Json(TestServerResponse {
            ok: true,
            latency_ms,
            normalized_url,
            needs_setup: Some(setup.needs_setup),
            version: Some(caps.version),
            error: None,
        })
        .into_response(),
    )
}

async fn app_not_found() -> Response {
    app_error(
        StatusCode::NOT_FOUND,
        "not_found",
        "Unknown client API route",
    )
}

fn collect_connection_header_tokens(headers: &HeaderMap) -> HashSet<String> {
    let mut set = HashSet::new();
    for val in headers.get_all(CONNECTION) {
        if let Ok(s) = val.to_str() {
            for token in s.split(',') {
                let t = token.trim().to_ascii_lowercase();
                if !t.is_empty() {
                    set.insert(t);
                }
            }
        }
    }
    set
}

fn is_hop_by_hop_header(name: &HeaderName, extra_connection_tokens: &HashSet<String>) -> bool {
    let lower = name.as_str();
    matches!(
        lower,
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailers"
            | "transfer-encoding"
            | "upgrade"
            | "proxy-connection"
    ) || extra_connection_tokens.contains(lower)
}

/// Rewrites upstream `Set-Cookie` headers so cookies issued by remote HTTPS or
/// domain-bound servers are accepted by the local `http://127.0.0.1:<port>` WebView.
fn sanitize_set_cookie_for_loopback(raw: &str) -> String {
    let mut parts = raw.split(';');
    let Some(first) = parts.next() else {
        return raw.to_string();
    };
    let mut kept = vec![first.trim().to_string()];
    for attr in parts {
        let trimmed = attr.trim();
        if trimmed.is_empty() {
            continue;
        }
        let attr_name = trimmed
            .split_once('=')
            .map(|(k, _)| k.trim())
            .unwrap_or(trimmed);
        if attr_name.eq_ignore_ascii_case("secure") || attr_name.eq_ignore_ascii_case("domain") {
            continue;
        }
        kept.push(trimmed.to_string());
    }
    kept.join("; ")
}

async fn proxy_to_active_server(State(state): State<ProxyState>, req: Request<Body>) -> Response {
    let active_server = match state.store.read() {
        Ok(guard) => guard.active_server(),
        Err(_) => {
            return app_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "store_poisoned",
                "Server configuration store lock poisoned",
            );
        }
    };

    let Some(server) = active_server else {
        return app_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "no_server_configured",
            "No server configured",
        );
    };

    let (parts, body) = req.into_parts();
    let path_and_query = parts
        .uri
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or("/");
    let target_url = format!("{}{}", server.url, path_and_query);

    let reqwest_method = match reqwest::Method::from_bytes(parts.method.as_str().as_bytes()) {
        Ok(m) => m,
        Err(_) => {
            return app_error(
                StatusCode::BAD_REQUEST,
                "invalid_method",
                "Unsupported HTTP method",
            );
        }
    };

    let connection_tokens = collect_connection_header_tokens(&parts.headers);
    let mut upstream_headers = reqwest::header::HeaderMap::new();
    for (name, value) in &parts.headers {
        if name == HOST || is_hop_by_hop_header(name, &connection_tokens) {
            continue;
        }
        if let (Ok(r_name), Ok(r_val)) = (
            reqwest::header::HeaderName::from_bytes(name.as_str().as_bytes()),
            reqwest::header::HeaderValue::from_bytes(value.as_bytes()),
        ) {
            upstream_headers.append(r_name, r_val);
        }
    }

    let content_length = parts
        .headers
        .get(CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok());
    let is_chunked = parts.headers.contains_key(TRANSFER_ENCODING);
    let should_stream_body = match content_length {
        Some(0) => false,
        Some(_) => true,
        None => is_chunked || matches!(parts.method, Method::POST | Method::PUT | Method::PATCH),
    };

    let client = state.client_for(server.allow_insecure_tls);
    let mut upstream_req = client
        .request(reqwest_method, &target_url)
        .headers(upstream_headers);

    if should_stream_body {
        upstream_req = upstream_req.body(reqwest::Body::wrap_stream(body.into_data_stream()));
    }

    let upstream_res = match upstream_req.send().await {
        Ok(r) => r,
        Err(err) => {
            tracing::warn!(target = %target_url, error = %err, "upstream proxy request failed");
            return app_error(
                StatusCode::BAD_GATEWAY,
                "upstream_unreachable",
                format!("Failed to reach server {}: {err}", server.url),
            );
        }
    };

    let status =
        StatusCode::from_u16(upstream_res.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);

    // Collect upstream connection header tokens before copying response headers.
    let mut resp_connection_tokens = HashSet::new();
    for val in upstream_res.headers().get_all(reqwest::header::CONNECTION) {
        if let Ok(s) = val.to_str() {
            for token in s.split(',') {
                let t = token.trim().to_ascii_lowercase();
                if !t.is_empty() {
                    resp_connection_tokens.insert(t);
                }
            }
        }
    }

    let mut out_headers = HeaderMap::new();
    for (name, value) in upstream_res.headers() {
        let Ok(axum_name) = HeaderName::from_bytes(name.as_str().as_bytes()) else {
            continue;
        };
        if is_hop_by_hop_header(&axum_name, &resp_connection_tokens) {
            continue;
        }
        if axum_name == SET_COOKIE {
            if let Ok(raw_cookie) = value.to_str() {
                let sanitized = sanitize_set_cookie_for_loopback(raw_cookie);
                if let Ok(axum_val) = HeaderValue::from_str(&sanitized) {
                    out_headers.append(SET_COOKIE, axum_val);
                }
            }
            continue;
        }
        if let Ok(axum_val) = HeaderValue::from_bytes(value.as_bytes()) {
            out_headers.append(axum_name, axum_val);
        }
    }

    let body = Body::from_stream(upstream_res.bytes_stream());
    let mut response = Response::new(body);
    *response.status_mut() = status;
    *response.headers_mut() = out_headers;
    response
}

fn normalize_asset_path(uri_path: &str) -> Option<PathBuf> {
    let trimmed = uri_path.trim_start_matches('/');
    let path = Path::new(trimmed);
    let mut clean = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::Normal(part) => clean.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(clean)
}

fn asset_response(path_hint: &str, body: impl Into<Body>, immutable: bool) -> Response {
    let mime = mime_guess::from_path(path_hint).first_or_octet_stream();
    let mut res = (StatusCode::OK, body.into()).into_response();
    if let Ok(ct) = HeaderValue::from_str(mime.as_ref()) {
        res.headers_mut().insert(CONTENT_TYPE, ct);
    }
    let cache = if immutable {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    res.headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static(cache));
    res
}

async fn serve_spa(State(state): State<ProxyState>, uri: Uri) -> Response {
    let req_path = uri.path();
    if req_path == "/_app"
        || req_path.starts_with("/_app/")
        || req_path == "/api"
        || req_path.starts_with("/api/")
        || req_path == "/opds"
        || req_path.starts_with("/opds/")
        || req_path == "/rest"
        || req_path.starts_with("/rest/")
    {
        return app_error(StatusCode::NOT_FOUND, "not_found", "Not Found");
    }

    let rel_opt = normalize_asset_path(req_path);
    let rel_str = rel_opt
        .as_ref()
        .map(|p| {
            p.components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/")
        })
        .unwrap_or_default();

    let is_hashed_asset = rel_str.starts_with("assets/");

    if let Some(ref ext_dir) = state.external_dist_dir {
        if !rel_str.is_empty() {
            let candidate = ext_dir.join(&rel_str);
            if candidate.is_file()
                && let Ok(bytes) = tokio::fs::read(&candidate).await
            {
                return asset_response(&rel_str, bytes, is_hashed_asset);
            }
        }
        let index_file = ext_dir.join("index.html");
        if index_file.is_file()
            && let Ok(bytes) = tokio::fs::read(&index_file).await
        {
            return asset_response("index.html", bytes, false);
        }
    }

    if !rel_str.is_empty()
        && let Some(embedded) = EmbeddedWebAssets::get(&rel_str)
    {
        return asset_response(&rel_str, embedded.data, is_hashed_asset);
    }

    if let Some(index) = EmbeddedWebAssets::get("index.html") {
        return asset_response("index.html", index.data, false);
    }

    Html(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>XMedia</title></head>\
         <body><h1>XMedia Client</h1><p>Frontend assets are not built yet.</p></body></html>",
    )
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_set_cookie_strips_secure_and_domain() {
        let raw = "xmedia_token=abc123; Path=/; HttpOnly; SameSite=Lax; Domain=media.example.com; Max-Age=3600; Secure";
        assert_eq!(
            sanitize_set_cookie_for_loopback(raw),
            "xmedia_token=abc123; Path=/; HttpOnly; SameSite=Lax; Max-Age=3600"
        );
    }
}

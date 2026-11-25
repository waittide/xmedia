use axum::Json;
use axum::extract::{Path as AxumPath, State};
use axum::http::header::SET_COOKIE;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use url::Url;

use super::dto::{
    AppHealthResponse, AppIconResponse, MutatedRemoteSourceResponse, RemoteSourcesListResponse,
    SetAppIconRequest, TestRemoteSourceRequest, TestRemoteSourceResponse, app_error,
    mark_source_ok, store_error_response, with_app_headers,
};
use super::{APP_HEADER_NAME, APP_HEADER_VALUE, StandaloneGatewayState};
use crate::app_icon::AppIconVariant;
use crate::remote_sources::{
    CreateRemoteSourceInput, RemoteProtocol, UpdateRemoteSourceInput, is_same_origin,
    normalize_remote_url,
};

pub(super) async fn app_health(State(state): State<StandaloneGatewayState>) -> Response {
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

pub(super) async fn get_app_icon(State(state): State<StandaloneGatewayState>) -> Response {
    with_app_headers(
        Json(AppIconResponse {
            icon: state.app_icon.current().as_str().to_string(),
        })
        .into_response(),
    )
}

pub(super) async fn set_app_icon(
    State(state): State<StandaloneGatewayState>,
    Json(input): Json<SetAppIconRequest>,
) -> Response {
    let Some(variant) = AppIconVariant::parse(&input.icon) else {
        return app_error(
            StatusCode::BAD_REQUEST,
            "invalid_icon",
            "Icon must be one of: cobalt, porcelain, obsidian, monolith",
        );
    };
    match state.app_icon.set_async(variant).await {
        Ok(saved) => with_app_headers(
            Json(AppIconResponse {
                icon: saved.as_str().to_string(),
            })
            .into_response(),
        ),
        Err(err) => {
            tracing::error!(error = %err, "failed to persist standalone app icon");
            app_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "storage_error",
                "Failed to persist app icon preference",
            )
        }
    }
}

pub(super) async fn app_auto_session(
    State(state): State<StandaloneGatewayState>,
    headers: HeaderMap,
) -> Response {
    let valid_app_header = headers
        .get(APP_HEADER_NAME)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.trim().eq_ignore_ascii_case(APP_HEADER_VALUE));
    if !valid_app_header {
        return app_error(
            StatusCode::FORBIDDEN,
            "missing_app_header",
            "Auto-session requires the X-XMedia-App: standalone header",
        );
    }
    match state.local_engine.issue_auto_session().await {
        Ok(session) => {
            let cookie = state.local_engine.session_cookie_header(&session.token);
            let mut res = with_app_headers((StatusCode::OK, Json(session)).into_response());
            res.headers_mut().append(SET_COOKIE, cookie);
            res
        }
        Err(err) => {
            tracing::error!(error = %err, "failed to issue standalone auto-session");
            app_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "auto_session_failed",
                "Failed to issue local session",
            )
        }
    }
}

pub(super) async fn list_remote_sources(State(state): State<StandaloneGatewayState>) -> Response {
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

pub(super) async fn create_remote_source(
    State(state): State<StandaloneGatewayState>,
    Json(input): Json<CreateRemoteSourceInput>,
) -> Response {
    let _write_guard = state.sources_write_lock.lock().await;
    let pending = match state.sources.read() {
        Ok(guard) => match guard.prepare_create(input) {
            Ok(p) => p,
            Err(err) => return store_error_response(err),
        },
        Err(_) => {
            return app_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "store_poisoned",
                "Remote source store lock poisoned",
            );
        }
    };
    let persisted = match tokio::task::spawn_blocking(move || pending.persist()).await {
        Ok(Ok(p)) => p,
        Ok(Err(err)) => return store_error_response(err),
        Err(err) => {
            tracing::error!(error = %err, "remote source create persist task failed");
            return app_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "storage_error",
                "Failed to persist remote sources",
            );
        }
    };
    let (source, sources) = match state.sources.write() {
        Ok(mut guard) => guard.commit_mutation(persisted),
        Err(_) => {
            return app_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "store_poisoned",
                "Remote source store lock poisoned",
            );
        }
    };
    let Some(source) = source else {
        return app_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "storage_error",
            "Failed to persist remote sources",
        );
    };
    with_app_headers(
        (
            StatusCode::CREATED,
            Json(MutatedRemoteSourceResponse { source, sources }),
        )
            .into_response(),
    )
}

pub(super) async fn update_remote_source(
    State(state): State<StandaloneGatewayState>,
    AxumPath(id): AxumPath<String>,
    Json(input): Json<UpdateRemoteSourceInput>,
) -> Response {
    let _write_guard = state.sources_write_lock.lock().await;
    let pending = match state.sources.read() {
        Ok(guard) => match guard.prepare_update(&id, input) {
            Ok(p) => p,
            Err(err) => return store_error_response(err),
        },
        Err(_) => {
            return app_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "store_poisoned",
                "Remote source store lock poisoned",
            );
        }
    };
    let persisted = match tokio::task::spawn_blocking(move || pending.persist()).await {
        Ok(Ok(p)) => p,
        Ok(Err(err)) => return store_error_response(err),
        Err(err) => {
            tracing::error!(error = %err, "remote source update persist task failed");
            return app_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "storage_error",
                "Failed to persist remote sources",
            );
        }
    };
    let (source, sources) = match state.sources.write() {
        Ok(mut guard) => guard.commit_mutation(persisted),
        Err(_) => {
            return app_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "store_poisoned",
                "Remote source store lock poisoned",
            );
        }
    };
    let Some(source) = source else {
        return app_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "storage_error",
            "Failed to persist remote sources",
        );
    };
    with_app_headers(Json(MutatedRemoteSourceResponse { source, sources }).into_response())
}

pub(super) async fn delete_remote_source(
    State(state): State<StandaloneGatewayState>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let _write_guard = state.sources_write_lock.lock().await;
    let pending = match state.sources.read() {
        Ok(guard) => match guard.prepare_delete(&id) {
            Ok(p) => p,
            Err(err) => return store_error_response(err),
        },
        Err(_) => {
            return app_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "store_poisoned",
                "Remote source store lock poisoned",
            );
        }
    };
    let persisted = match tokio::task::spawn_blocking(move || pending.persist()).await {
        Ok(Ok(p)) => p,
        Ok(Err(err)) => return store_error_response(err),
        Err(err) => {
            tracing::error!(error = %err, "remote source delete persist task failed");
            return app_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "storage_error",
                "Failed to persist remote sources",
            );
        }
    };
    let (_, sources) = match state.sources.write() {
        Ok(mut guard) => guard.commit_mutation(persisted),
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

pub(super) async fn test_remote_source(
    State(state): State<StandaloneGatewayState>,
    Json(input): Json<TestRemoteSourceRequest>,
) -> Response {
    let username = input.username.unwrap_or_default();
    let mut password = input.password.unwrap_or_default();
    if password.is_empty()
        && let Some(ref source_id) = input.source_id
        && let Ok(guard) = state.sources.read()
        && let Some(existing) = guard.get(source_id)
        && !existing.password.is_empty()
    {
        let input_norm = match normalize_remote_url(&input.url) {
            Ok(u) => u,
            Err(err) => return store_error_response(err),
        };
        let (Ok(input_parsed), Ok(existing_parsed)) =
            (Url::parse(&input_norm), Url::parse(&existing.url))
        else {
            return app_error(
                StatusCode::BAD_REQUEST,
                "invalid_url",
                "Invalid remote source URL",
            );
        };
        if !is_same_origin(&input_parsed, &existing_parsed) {
            return app_error(
                StatusCode::BAD_REQUEST,
                "origin_mismatch",
                "Cannot reuse stored credentials when testing a different origin",
            );
        }
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

use axum::Json;
use axum::body::Body;
use axum::extract::{Path as AxumPath, State};
use axum::http::header::{
    CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE, SET_COOKIE, X_CONTENT_TYPE_OPTIONS,
};
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use url::Url;

use super::dto::{
    AppAppearanceResponse, AppHealthResponse, AppIconResponse, MutatedRemoteSourceResponse,
    RemoteSourcesListResponse, SetAppAppearanceRequest, SetAppIconRequest, TestRemoteSourceRequest,
    TestRemoteSourceResponse, app_error, mark_source_ok, store_error_response, with_app_headers,
};
use super::{APP_HEADER_NAME, APP_HEADER_VALUE, StandaloneGatewayState};
use crate::app_icon::AppIconVariant;
use crate::mobile_ui::SystemFontSource;
use crate::remote_sources::{
    CreateRemoteSourceInput, PendingSourceMutation, RemoteProtocol, RemoteSourceStore,
    RemoteSourceSummaryDto, SourceStoreError, UpdateRemoteSourceInput, is_same_origin,
    normalize_remote_url,
};

fn has_valid_standalone_header(headers: &HeaderMap) -> bool {
    headers
        .get(APP_HEADER_NAME)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.trim().eq_ignore_ascii_case(APP_HEADER_VALUE))
}

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
    let safe_area = state
        .mobile_ui
        .query_safe_area()
        .await
        .filter(|insets| insets.is_non_zero());
    let system_fonts = state.mobile_ui.system_font_dtos();
    with_app_headers(
        Json(AppHealthResponse {
            mode: "standalone".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            remote_source_count,
            enabled_modules: state.local_engine.enabled_modules.clone(),
            safe_area,
            system_fonts,
        })
        .into_response(),
    )
}

pub(super) async fn set_app_appearance(
    State(state): State<StandaloneGatewayState>,
    headers: HeaderMap,
    Json(input): Json<SetAppAppearanceRequest>,
) -> Response {
    if !has_valid_standalone_header(&headers) {
        return app_error(
            StatusCode::FORBIDDEN,
            "missing_app_header",
            "Appearance sync requires the X-XMedia-App: standalone header",
        );
    }
    let safe_area = state
        .mobile_ui
        .sync_appearance(input.dark)
        .await
        .filter(|insets| insets.is_non_zero());
    let system_fonts = state.mobile_ui.system_font_dtos();
    with_app_headers(
        Json(AppAppearanceResponse {
            dark: input.dark,
            safe_area,
            system_fonts,
        })
        .into_response(),
    )
}

pub(super) async fn get_app_system_font(
    State(state): State<StandaloneGatewayState>,
    AxumPath(slot): AxumPath<usize>,
) -> Response {
    let Some(entry) = state.mobile_ui.get_system_font(slot) else {
        return app_error(
            StatusCode::NOT_FOUND,
            "not_found",
            "Host system font slot not available",
        );
    };
    let (body, content_len) = match entry.source {
        SystemFontSource::File(path) => match tokio::fs::read(&path).await {
            Ok(bytes) => {
                let len = bytes.len();
                (Body::from(bytes), len)
            }
            Err(_) => {
                return app_error(
                    StatusCode::NOT_FOUND,
                    "not_found",
                    "Host system font file could not be read",
                );
            }
        },
        SystemFontSource::Memory(bytes) => {
            let len = bytes.len();
            (Body::from(bytes.to_vec()), len)
        }
    };

    let mut res = (StatusCode::OK, body).into_response();
    let headers = res.headers_mut();
    headers.insert(
        HeaderName::from_static(APP_HEADER_NAME),
        HeaderValue::from_static(APP_HEADER_VALUE),
    );
    if let Ok(ct) = HeaderValue::from_str(entry.mime_type) {
        headers.insert(CONTENT_TYPE, ct);
    }
    if let Ok(cl) = HeaderValue::from_str(&content_len.to_string()) {
        headers.insert(CONTENT_LENGTH, cl);
    }
    headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    headers.insert(
        CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=300"),
    );
    res
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
    headers: HeaderMap,
    Json(input): Json<SetAppIconRequest>,
) -> Response {
    if let Some(err_res) = super::security::check_app_session_error(&state, &headers).await {
        return err_res;
    }
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
    if !has_valid_standalone_header(&headers) {
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

async fn execute_source_mutation<F>(
    state: &StandaloneGatewayState,
    op_label: &'static str,
    prepare: F,
) -> Result<(Option<RemoteSourceSummaryDto>, Vec<RemoteSourceSummaryDto>), Box<Response>>
where
    F: FnOnce(&RemoteSourceStore) -> Result<PendingSourceMutation, SourceStoreError>,
{
    let _write_guard = state.sources_write_lock.lock().await;
    let pending = match state.sources.read() {
        Ok(guard) => prepare(&guard).map_err(|err| Box::new(store_error_response(err)))?,
        Err(_) => {
            return Err(Box::new(app_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "store_poisoned",
                "Remote source store lock poisoned",
            )));
        }
    };
    let persisted = match tokio::task::spawn_blocking(move || pending.persist()).await {
        Ok(Ok(p)) => p,
        Ok(Err(err)) => return Err(Box::new(store_error_response(err))),
        Err(err) => {
            tracing::error!(op = op_label, error = %err, "remote source persist task failed");
            return Err(Box::new(app_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "storage_error",
                "Failed to persist remote sources",
            )));
        }
    };
    match state.sources.write() {
        Ok(mut guard) => Ok(guard.commit_mutation(persisted)),
        Err(_) => Err(Box::new(app_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "store_poisoned",
            "Remote source store lock poisoned",
        ))),
    }
}

pub(super) async fn create_remote_source(
    State(state): State<StandaloneGatewayState>,
    Json(input): Json<CreateRemoteSourceInput>,
) -> Response {
    let (source, sources) = match execute_source_mutation(&state, "create", |store| {
        store.prepare_create(input)
    })
    .await
    {
        Ok(res) => res,
        Err(err_res) => return *err_res,
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
    let (source, sources) =
        match execute_source_mutation(&state, "update", |store| store.prepare_update(&id, input))
            .await
        {
            Ok(res) => res,
            Err(err_res) => return *err_res,
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
    let (_, sources) =
        match execute_source_mutation(&state, "delete", |store| store.prepare_delete(&id)).await {
            Ok(res) => res,
            Err(err_res) => return *err_res,
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

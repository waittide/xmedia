use axum::Json;
use axum::http::header::CACHE_CONTROL;
use axum::http::{HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use xmedia_core::domain::library::MediaKind;

use super::{APP_HEADER_NAME, APP_HEADER_VALUE, StandaloneGatewayState};
use crate::opds_client::OpdsError;
use crate::remote_sources::{
    RemoteProtocol, RemoteSource, RemoteSourceSummaryDto, SourceStoreError,
};
use crate::subsonic_client::{SubsonicAlbumDto, SubsonicError};

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
pub struct AppIconResponse {
    pub icon: String,
}

#[derive(Debug, Deserialize)]
pub struct SetAppIconRequest {
    pub icon: String,
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
pub(super) struct OpdsBrowseQuery {
    #[serde(default)]
    pub(super) url: Option<String>,
    #[serde(default)]
    pub(super) q: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct OpdsProxyImageQuery {
    pub(super) url: String,
    #[serde(default)]
    pub(super) page: Option<u64>,
    #[serde(default, rename = "maxWidth")]
    pub(super) max_width: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub(super) struct OpdsImportRequest {
    pub(super) url: String,
    #[serde(default)]
    pub(super) filename: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct SubsonicAlbumsQuery {
    #[serde(rename = "type", default)]
    pub(super) list_type: Option<String>,
    #[serde(default)]
    pub(super) size: Option<u32>,
    #[serde(default)]
    pub(super) offset: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SubsonicAlbumsResponse {
    pub albums: Vec<SubsonicAlbumDto>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct SubsonicSearchQuery {
    #[serde(default)]
    pub(super) q: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct SubsonicImportRequest {
    #[serde(default)]
    pub(super) filename: Option<String>,
}

pub(super) fn with_app_headers(mut res: Response) -> Response {
    res.headers_mut().insert(
        HeaderName::from_static(APP_HEADER_NAME),
        HeaderValue::from_static(APP_HEADER_VALUE),
    );
    res.headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    res
}

pub(super) fn app_error(status: StatusCode, code: &str, message: impl Into<String>) -> Response {
    let body = ErrorEnvelope {
        error: ErrorBody {
            code: code.to_string(),
            message: message.into(),
        },
    };
    with_app_headers((status, Json(body)).into_response())
}

pub(super) fn store_error_response(err: SourceStoreError) -> Response {
    match err {
        SourceStoreError::InvalidUrl(msg) => app_error(StatusCode::BAD_REQUEST, "invalid_url", msg),
        SourceStoreError::NotFound(msg) => app_error(StatusCode::NOT_FOUND, "not_found", msg),
        SourceStoreError::Io(msg) => {
            tracing::error!(error = %msg, "remote source store I/O error");
            app_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "storage_error",
                "Failed to persist remote sources",
            )
        }
    }
}

pub(super) fn opds_error_response(err: OpdsError) -> Response {
    match err {
        OpdsError::Internal(msg) => {
            tracing::error!(error = %msg, "internal OPDS gateway error");
            app_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Internal OPDS processing error",
            )
        }
        other => app_error(other.status_code(), other.code(), other.to_string()),
    }
}

pub(super) fn subsonic_error_response(err: SubsonicError) -> Response {
    match err {
        SubsonicError::Internal(msg) => {
            tracing::error!(error = %msg, "internal Subsonic gateway error");
            app_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Internal Subsonic processing error",
            )
        }
        other => app_error(other.status_code(), other.code(), other.to_string()),
    }
}

pub(super) struct AppApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl AppApiError {
    pub(super) fn into_response(self) -> Response {
        app_error(self.status, self.code, self.message)
    }
}

pub(super) fn load_source_with_protocol(
    state: &StandaloneGatewayState,
    id: &str,
    expected: RemoteProtocol,
) -> Result<RemoteSource, AppApiError> {
    let guard = state.sources.read().map_err(|_| AppApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        code: "store_poisoned",
        message: "Remote source store lock poisoned".to_string(),
    })?;
    let Some(src) = guard.get(id) else {
        return Err(AppApiError {
            status: StatusCode::NOT_FOUND,
            code: "not_found",
            message: format!("Remote source '{id}' not found"),
        });
    };
    if src.protocol != expected {
        return Err(AppApiError {
            status: StatusCode::BAD_REQUEST,
            code: "protocol_mismatch",
            message: format!(
                "Remote source '{id}' uses protocol '{}', expected '{}'",
                src.protocol.as_str(),
                expected.as_str()
            ),
        });
    }
    Ok(src)
}

pub(super) fn mark_source_ok(state: &StandaloneGatewayState, id: &str) {
    let pending = if let Ok(mut guard) = state.sources.write() {
        guard.mark_ok_throttled(id)
    } else {
        None
    };
    if let Some(task) = pending {
        tokio::task::spawn_blocking(move || {
            if let Err(err) = task.persist() {
                tracing::warn!(error = %err, "failed to persist remote source last_ok_at");
            }
        });
    }
}

pub(super) async fn app_not_found() -> Response {
    app_error(
        StatusCode::NOT_FOUND,
        "not_found",
        "Unknown client API route",
    )
}

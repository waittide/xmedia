#[cfg(feature = "server")]
pub mod auth;
pub mod fonts;
#[cfg(feature = "server")]
pub mod online_items;
#[cfg(test)]
mod tests;
pub mod upload;

#[cfg(feature = "server")]
pub use auth::*;
pub use fonts::*;
pub use upload::*;

use axum::Json;
use axum::body::Body;
use axum::http::header::{
    ACCEPT_RANGES, CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_RANGE,
    CONTENT_SECURITY_POLICY, CONTENT_TYPE, X_CONTENT_TYPE_OPTIONS,
};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::Serialize;

use crate::application::ports::{BinaryPayload, ByteChunkStream, ByteRangeSpec, ByteRangeStream};
use crate::domain::error::DomainError;

#[derive(Debug)]
pub struct ApiError(pub DomainError);

impl From<DomainError> for ApiError {
    fn from(err: DomainError) -> Self {
        Self(err)
    }
}

#[derive(Serialize)]
struct ErrorEnvelope<'a> {
    error: ErrorBody<'a>,
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    code: &'static str,
    message: &'a str,
}

pub fn error_json_response(status: StatusCode, code: &'static str, message: &str) -> Response {
    (
        status,
        Json(ErrorEnvelope {
            error: ErrorBody { code, message },
        }),
    )
        .into_response()
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        match self.0 {
            DomainError::RangeNotSatisfiable { total_size } => {
                let mut res =
                    (StatusCode::RANGE_NOT_SATISFIABLE, "Range Not Satisfiable").into_response();
                if let Ok(hv) = HeaderValue::from_str(&format!("bytes */{total_size}")) {
                    res.headers_mut().insert(CONTENT_RANGE, hv);
                }
                res
            }
            DomainError::Validation(msg) => {
                error_json_response(StatusCode::BAD_REQUEST, "validation_error", &msg)
            }
            DomainError::Unauthorized(msg) => {
                error_json_response(StatusCode::UNAUTHORIZED, "unauthorized", &msg)
            }
            DomainError::Forbidden(msg) => {
                error_json_response(StatusCode::FORBIDDEN, "forbidden", &msg)
            }
            DomainError::NotFound(msg) => {
                error_json_response(StatusCode::NOT_FOUND, "not_found", &msg)
            }
            DomainError::Conflict(msg) => {
                error_json_response(StatusCode::CONFLICT, "conflict", &msg)
            }
            DomainError::TooManyRequests(msg) => {
                error_json_response(StatusCode::TOO_MANY_REQUESTS, "rate_limited", &msg)
            }
            DomainError::UnsupportedFormat(msg) => error_json_response(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "unsupported_format",
                &msg,
            ),
            DomainError::Storage(detail) => {
                tracing::error!(
                    error_code = "storage_error",
                    detail = %detail,
                    "internal storage error"
                );
                error_json_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "storage_error",
                    "Internal storage error",
                )
            }
            DomainError::Persistence(detail) => {
                tracing::error!(
                    error_code = "persistence_error",
                    detail = %detail,
                    "internal persistence error"
                );
                error_json_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "persistence_error",
                    "Internal database error",
                )
            }
        }
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

pub fn parse_http_range_spec(headers: &HeaderMap) -> ByteRangeSpec {
    let Some(header_val) = headers.get(axum::http::header::RANGE) else {
        return ByteRangeSpec::Full;
    };
    let Ok(raw) = header_val.to_str() else {
        return ByteRangeSpec::Malformed;
    };
    let trimmed = raw.trim();
    if trimmed.len() < 6 || !trimmed[..6].eq_ignore_ascii_case("bytes=") {
        return ByteRangeSpec::Malformed;
    }
    let spec = &trimmed[6..];
    let Some(first) = spec.split(',').next() else {
        return ByteRangeSpec::Malformed;
    };
    let Some((start_str, end_str)) = first.trim().split_once('-') else {
        return ByteRangeSpec::Malformed;
    };
    let start_trimmed = start_str.trim();
    let end_trimmed = end_str.trim();

    if start_trimmed.is_empty() {
        let Ok(last_bytes) = end_trimmed.parse::<u64>() else {
            return ByteRangeSpec::Malformed;
        };
        if last_bytes == 0 {
            return ByteRangeSpec::Malformed;
        }
        return ByteRangeSpec::Suffix { last_bytes };
    }

    let Ok(start) = start_trimmed.parse::<u64>() else {
        return ByteRangeSpec::Malformed;
    };
    let end_inclusive = if end_trimmed.is_empty() {
        None
    } else {
        let Ok(end_val) = end_trimmed.parse::<u64>() else {
            return ByteRangeSpec::Malformed;
        };
        if end_val < start {
            return ByteRangeSpec::Malformed;
        }
        Some(end_val)
    };
    ByteRangeSpec::FromTo {
        start,
        end_inclusive,
    }
}

pub fn sanitize_download_filename(title: &str, ext: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    let base = cleaned.trim();
    let base = if base.is_empty() { "media" } else { base };
    format!("{base}.{ext}")
}

fn encode_disposition(disposition_kind: &str, filename: &str) -> Option<HeaderValue> {
    let encoded_name: String = filename
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.') {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    HeaderValue::from_str(&format!(
        "{disposition_kind}; filename*=UTF-8''{encoded_name}"
    ))
    .ok()
}

pub fn into_range_http_response(
    range_stream: ByteRangeStream,
    is_partial: bool,
    mime_type: &str,
    filename: Option<&str>,
    as_attachment: bool,
    duration_secs: Option<u32>,
) -> Response {
    let effective_partial = is_partial && range_stream.total_size > 0;
    let status = if effective_partial {
        StatusCode::PARTIAL_CONTENT
    } else {
        StatusCode::OK
    };
    let content_len = if range_stream.total_size == 0 {
        0
    } else {
        range_stream
            .end_inclusive
            .saturating_sub(range_stream.start)
            .saturating_add(1)
    };

    let body = Body::from_stream(range_stream.stream);
    let mut res = (status, body).into_response();
    let headers = res.headers_mut();
    if let Ok(ct) = HeaderValue::from_str(mime_type) {
        headers.insert(CONTENT_TYPE, ct);
    }
    headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    if !as_attachment {
        headers.insert(
            CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(
                "sandbox; default-src 'none'; img-src 'self' data:; media-src 'self'",
            ),
        );
    }
    headers.insert(ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    headers.insert(
        CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=3600"),
    );
    if let Ok(len_val) = HeaderValue::from_str(&content_len.to_string()) {
        headers.insert(CONTENT_LENGTH, len_val);
    }
    if effective_partial
        && let Ok(range_val) = HeaderValue::from_str(&format!(
            "bytes {}-{}/{}",
            range_stream.start, range_stream.end_inclusive, range_stream.total_size
        ))
    {
        headers.insert(CONTENT_RANGE, range_val);
    }
    if let Some(name) = filename {
        let disp_kind = if as_attachment {
            "attachment"
        } else {
            "inline"
        };
        if let Some(disp) = encode_disposition(disp_kind, name) {
            headers.insert(CONTENT_DISPOSITION, disp);
        }
    }
    if let Some(dur) = duration_secs
        && let Ok(hv) = HeaderValue::from_str(&dur.to_string())
    {
        headers.insert("x-content-duration", hv);
    }
    res
}

pub fn into_stream_http_response(
    stream: ByteChunkStream,
    mime_type: &str,
    filename: Option<&str>,
    as_attachment: bool,
    duration_secs: Option<u32>,
    estimated_len: Option<u64>,
) -> Response {
    let body = Body::from_stream(stream);
    let mut res = (StatusCode::OK, body).into_response();
    let headers = res.headers_mut();
    if let Ok(ct) = HeaderValue::from_str(mime_type) {
        headers.insert(CONTENT_TYPE, ct);
    }
    headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    if !as_attachment {
        headers.insert(
            CONTENT_SECURITY_POLICY,
            HeaderValue::from_static("sandbox; default-src 'none'; media-src 'self'"),
        );
    }
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if let Some(name) = filename {
        let disp_kind = if as_attachment {
            "attachment"
        } else {
            "inline"
        };
        if let Some(disp) = encode_disposition(disp_kind, name) {
            headers.insert(CONTENT_DISPOSITION, disp);
        }
    }
    if let Some(dur) = duration_secs
        && let Ok(hv) = HeaderValue::from_str(&dur.to_string())
    {
        headers.insert("x-content-duration", hv);
    }
    if let Some(est) = estimated_len
        && let Ok(hv) = HeaderValue::from_str(&est.to_string())
    {
        headers.insert("x-estimated-content-length", hv);
    }
    res
}

pub fn into_binary_cache_response(payload: BinaryPayload) -> Response {
    let mut res = (StatusCode::OK, Body::from(payload.bytes)).into_response();
    let headers = res.headers_mut();
    if let Ok(ct) = HeaderValue::from_str(&payload.mime_type) {
        headers.insert(CONTENT_TYPE, ct);
    }
    headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    headers.insert(
        CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("sandbox; default-src 'none'; img-src 'self' data:"),
    );
    headers.insert(
        CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=604800"),
    );
    res
}

/// Redacts sensitive authentication query parameters (`token`, `api_key`, `p`, `t`, `s`, etc.)
/// before recording HTTP request URIs in tracing spans.
pub fn redact_uri_for_log(uri: &axum::http::Uri) -> String {
    let path = uri.path();
    let Some(query) = uri.query() else {
        return path.to_string();
    };
    let mut parts = Vec::new();
    for seg in query.split('&') {
        if let Some((k, _)) = seg.split_once('=')
            && matches!(
                k,
                "token" | "api_key" | "apiKey" | "apikey" | "p" | "t" | "s" | "password"
            )
        {
            parts.push(format!("{k}=[REDACTED]"));
            continue;
        }
        parts.push(seg.to_string());
    }
    format!("{path}?{}", parts.join("&"))
}

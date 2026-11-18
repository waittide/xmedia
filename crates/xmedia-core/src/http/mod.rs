use axum::Json;
use axum::body::Body;
use axum::http::header::{
    ACCEPT_RANGES, AUTHORIZATION, CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_LENGTH,
    CONTENT_RANGE, CONTENT_TYPE, COOKIE, SET_COOKIE, WWW_AUTHENTICATE,
};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use serde::Serialize;

use crate::application::auth::AuthService;
use crate::application::ports::{BinaryPayload, ByteChunkStream, ByteRangeSpec, ByteRangeStream};
use crate::domain::error::DomainError;
use crate::domain::user::User;

pub const AUTH_COOKIE_NAME: &str = "xmedia_token";

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
            other => {
                let (status, code) = match &other {
                    DomainError::Validation(_) => (StatusCode::BAD_REQUEST, "validation_error"),
                    DomainError::Unauthorized(_) => (StatusCode::UNAUTHORIZED, "unauthorized"),
                    DomainError::Forbidden(_) => (StatusCode::FORBIDDEN, "forbidden"),
                    DomainError::NotFound(_) => (StatusCode::NOT_FOUND, "not_found"),
                    DomainError::Conflict(_) => (StatusCode::CONFLICT, "conflict"),
                    DomainError::UnsupportedFormat(_) => {
                        (StatusCode::UNSUPPORTED_MEDIA_TYPE, "unsupported_format")
                    }
                    DomainError::Storage(_) => (StatusCode::INTERNAL_SERVER_ERROR, "storage_error"),
                    DomainError::Persistence(_) => {
                        (StatusCode::INTERNAL_SERVER_ERROR, "persistence_error")
                    }
                    DomainError::RangeNotSatisfiable { .. } => unreachable!(),
                };
                let msg = other.to_string();
                (
                    status,
                    Json(ErrorEnvelope {
                        error: ErrorBody {
                            code,
                            message: &msg,
                        },
                    }),
                )
                    .into_response()
            }
        }
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

fn is_https_request(headers: &HeaderMap) -> bool {
    headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|p| {
            p.split(',')
                .next()
                .unwrap_or("")
                .trim()
                .eq_ignore_ascii_case("https")
        })
}

pub fn build_session_cookie(
    token: &str,
    max_age_secs: u64,
    req_headers: &HeaderMap,
) -> HeaderValue {
    let secure_attr = if is_https_request(req_headers) {
        "; Secure"
    } else {
        ""
    };
    let raw = format!(
        "{AUTH_COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age_secs}{secure_attr}"
    );
    HeaderValue::from_str(&raw).unwrap_or_else(|_| HeaderValue::from_static(""))
}

pub fn build_clear_cookie(req_headers: &HeaderMap) -> HeaderValue {
    if is_https_request(req_headers) {
        HeaderValue::from_static("xmedia_token=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0; Secure")
    } else {
        HeaderValue::from_static("xmedia_token=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0")
    }
}

pub fn attach_cookie(mut res: Response, cookie: HeaderValue) -> Response {
    res.headers_mut().insert(SET_COOKIE, cookie);
    res
}

pub fn extract_request_token(parts: &Parts) -> Option<String> {
    if let Some(raw_auth) = parts
        .headers
        .get(AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        && let Some(bearer) = raw_auth.strip_prefix("Bearer ")
    {
        let trimmed = bearer.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    for cookie_header in parts.headers.get_all(COOKIE) {
        if let Ok(cookie_str) = cookie_header.to_str() {
            for pair in cookie_str.split(';') {
                if let Some((k, v)) = pair.trim().split_once('=')
                    && k.trim() == AUTH_COOKIE_NAME
                {
                    let val = v.trim();
                    if !val.is_empty() {
                        return Some(val.to_string());
                    }
                }
            }
        }
    }

    if let Some(query) = parts.uri.query() {
        for (k, v) in form_urlencoded::parse(query.as_bytes()) {
            if k == "token" && !v.trim().is_empty() {
                return Some(v.trim().to_string());
            }
        }
    }

    None
}

pub async fn authenticate_web_parts(parts: &Parts, auth: &AuthService) -> ApiResult<User> {
    if let Some(token) = extract_request_token(parts) {
        return Ok(auth.authenticate_jwt(&token).await?);
    }
    if let Some(api_key) = parts.headers.get("x-api-key").and_then(|v| v.to_str().ok()) {
        return Ok(auth.authenticate_api_key(api_key).await?);
    }
    Err(ApiError(DomainError::Unauthorized(
        "missing authentication token".into(),
    )))
}

pub struct OpdsAuthChallenge;

impl IntoResponse for OpdsAuthChallenge {
    fn into_response(self) -> Response {
        let mut res = (StatusCode::UNAUTHORIZED, "OPDS authentication required").into_response();
        res.headers_mut().insert(
            WWW_AUTHENTICATE,
            HeaderValue::from_static(r#"Basic realm="xmedia OPDS Catalog", charset="UTF-8""#),
        );
        res
    }
}

pub async fn authenticate_opds_parts(
    parts: &Parts,
    auth: &AuthService,
) -> Result<User, OpdsAuthChallenge> {
    if let Some(auth_val) = parts.headers.get(AUTHORIZATION)
        && let Ok(auth_str) = auth_val.to_str()
        && let Some(b64) = auth_str
            .strip_prefix("Basic ")
            .or_else(|| auth_str.strip_prefix("basic "))
        && let Ok(decoded_bytes) = BASE64_STANDARD.decode(b64.trim())
        && let Ok(decoded_str) = String::from_utf8(decoded_bytes)
        && let Some((username, password)) = decoded_str.split_once(':')
        && let Ok(user) = auth.verify_basic_credentials(username, password).await
    {
        return Ok(user);
    }

    if let Some(api_key) = parts.headers.get("x-api-key").and_then(|v| v.to_str().ok())
        && let Ok(user) = auth.authenticate_api_key(api_key).await
    {
        return Ok(user);
    }

    if let Some(query) = parts.uri.query() {
        for (k, v) in form_urlencoded::parse(query.as_bytes()) {
            if (k == "api_key" || k == "apiKey")
                && !v.trim().is_empty()
                && let Ok(user) = auth.authenticate_api_key(&v).await
            {
                return Ok(user);
            }
        }
    }

    if let Some(token) = extract_request_token(parts) {
        if let Ok(user) = auth.authenticate_jwt(&token).await {
            return Ok(user);
        }
        if let Ok(user) = auth.authenticate_api_key(&token).await {
            return Ok(user);
        }
    }

    Err(OpdsAuthChallenge)
}

pub fn parse_http_range(headers: &HeaderMap) -> Option<(u64, Option<u64>)> {
    match parse_http_range_spec(headers) {
        ByteRangeSpec::FromTo {
            start,
            end_inclusive,
        } => Some((start, end_inclusive)),
        _ => None,
    }
}

pub fn parse_http_range_spec(headers: &HeaderMap) -> ByteRangeSpec {
    let Some(header_val) = headers.get(axum::http::header::RANGE) else {
        return ByteRangeSpec::Full;
    };
    let Ok(raw) = header_val.to_str() else {
        return ByteRangeSpec::Malformed;
    };
    let Some(spec) = raw.trim().strip_prefix("bytes=") else {
        return ByteRangeSpec::Malformed;
    };
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
    let status = if is_partial {
        StatusCode::PARTIAL_CONTENT
    } else {
        StatusCode::OK
    };
    let content_len = range_stream
        .end_inclusive
        .saturating_sub(range_stream.start)
        .saturating_add(1);

    let body = Body::from_stream(range_stream.stream);
    let mut res = (status, body).into_response();
    let headers = res.headers_mut();
    if let Ok(ct) = HeaderValue::from_str(mime_type) {
        headers.insert(CONTENT_TYPE, ct);
    }
    headers.insert(ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    headers.insert(
        CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=3600"),
    );
    if let Ok(len_val) = HeaderValue::from_str(&content_len.to_string()) {
        headers.insert(CONTENT_LENGTH, len_val);
    }
    if is_partial
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
    if let Ok(ct) = HeaderValue::from_str(&payload.mime_type) {
        res.headers_mut().insert(CONTENT_TYPE, ct);
    }
    res.headers_mut().insert(
        CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=604800"),
    );
    res
}

use std::path::{Path, PathBuf};

use axum::Json;
use axum::body::Body;
use axum::extract::multipart::Field;
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
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

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

fn error_json_response(status: StatusCode, code: &'static str, message: &str) -> Response {
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

/// RAII temporary upload file created while streaming multipart uploads to disk.
/// Automatically removes the underlying file on drop unless moved elsewhere.
#[derive(Debug)]
pub struct TempUploadFile {
    path: PathBuf,
    size: u64,
}

impl TempUploadFile {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn is_empty(&self) -> bool {
        self.size == 0
    }
}

impl Drop for TempUploadFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Streams an Axum multipart `Field` directly into a temporary file chunk-by-chunk,
/// enforcing `max_bytes` incrementally so large uploads never buffer in RAM.
pub async fn stream_multipart_field_to_temp_file(
    field: &mut Field<'_>,
    max_bytes: u64,
    ext_hint: Option<&str>,
) -> ApiResult<TempUploadFile> {
    let clean_ext = ext_hint
        .map(|e| {
            e.trim()
                .trim_start_matches('.')
                .chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .collect::<String>()
        })
        .filter(|e| !e.is_empty());

    let file_name = match clean_ext {
        Some(ext) => format!("xmedia_upload_{}.{ext}.tmp", Uuid::now_v7()),
        None => format!("xmedia_upload_{}.tmp", Uuid::now_v7()),
    };
    let path = std::env::temp_dir().join(file_name);
    let mut file = tokio::fs::File::create(&path).await.map_err(|e| {
        ApiError(DomainError::Storage(format!(
            "failed to create temp upload file: {e}"
        )))
    })?;
    let mut guard = TempUploadFile { path, size: 0 };

    while let Some(chunk) = field
        .chunk()
        .await
        .map_err(|e| ApiError(DomainError::Validation(format!("upload read error: {e}"))))?
    {
        if chunk.is_empty() {
            continue;
        }
        guard.size = guard.size.saturating_add(chunk.len() as u64);
        if guard.size > max_bytes {
            return Err(ApiError(DomainError::Validation(format!(
                "uploaded file exceeds maximum allowed size ({max_bytes} bytes)"
            ))));
        }
        file.write_all(&chunk).await.map_err(|e| {
            ApiError(DomainError::Storage(format!(
                "failed writing temp upload chunk: {e}"
            )))
        })?;
    }

    file.flush().await.map_err(|e| {
        ApiError(DomainError::Storage(format!(
            "failed flushing temp upload file: {e}"
        )))
    })?;

    Ok(guard)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    async fn decode_error_response(err: DomainError) -> (StatusCode, serde_json::Value) {
        let resp = ApiError(err).into_response();
        let status = resp.status();
        let bytes = to_bytes(resp.into_body(), 1024 * 64).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        (status, json)
    }

    #[tokio::test]
    async fn api_error_returns_concise_inner_message_without_redundant_prefix() {
        let (status, body) =
            decode_error_response(DomainError::Validation("用户名不能为空".into())).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "validation_error");
        assert_eq!(body["error"]["message"], "用户名不能为空");

        let (status, body) =
            decode_error_response(DomainError::NotFound("书籍不存在".into())).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "not_found");
        assert_eq!(body["error"]["message"], "书籍不存在");
    }

    #[tokio::test]
    async fn api_error_sanitizes_internal_storage_and_persistence_details() {
        let (status, body) = decode_error_response(DomainError::Storage(
            "failed opening /var/lib/xmedia/secret.db: Permission denied".into(),
        ))
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body["error"]["code"], "storage_error");
        let msg = body["error"]["message"].as_str().unwrap();
        assert!(!msg.contains("/var/lib"));
        assert!(!msg.contains("secret.db"));
        assert_eq!(msg, "Internal storage error");

        let (status, body) = decode_error_response(DomainError::Persistence(
            "sqlite error: near \"SELECT * FROM users\": syntax error".into(),
        ))
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body["error"]["code"], "persistence_error");
        let msg = body["error"]["message"].as_str().unwrap();
        assert!(!msg.contains("sqlite"));
        assert!(!msg.contains("SELECT"));
        assert_eq!(msg, "Internal database error");
    }

    #[test]
    fn temp_upload_file_cleans_up_on_drop() {
        let path = std::env::temp_dir().join(format!("xmedia_drop_test_{}.tmp", Uuid::now_v7()));
        std::fs::write(&path, b"hello").unwrap();
        assert!(path.exists());
        {
            let guard = TempUploadFile {
                path: path.clone(),
                size: 5,
            };
            assert_eq!(guard.size(), 5);
            assert!(!guard.is_empty());
        }
        assert!(!path.exists());
    }
}

use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use axum::extract::{ConnectInfo, FromRef, FromRequestParts};
use axum::http::header::{
    AUTHORIZATION, COOKIE, HOST, ORIGIN, REFERER, SET_COOKIE, WWW_AUTHENTICATE,
};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;

use super::{ApiError, ApiResult};
use crate::application::auth::AuthService;
use crate::domain::error::{DomainError, DomainResult};
use crate::domain::user::User;

pub const AUTH_COOKIE_NAME: &str = "xmedia_token";
const ALT_SESSION_COOKIE_NAME: &str = "xmedia_session";

/// Extracted client IP address for per-IP rate limiting and audit logging.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ClientIp(pub Option<IpAddr>);

impl<S> FromRequestParts<S> for ClientIp
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(Self(extract_client_ip(parts)))
    }
}

fn parse_forwarded_ip_token(raw: &str) -> Option<IpAddr> {
    let trimmed = raw.trim().trim_matches('"');
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(ip) = trimmed.parse::<IpAddr>() {
        return Some(ip.to_canonical());
    }
    if let Ok(sock) = trimmed.parse::<SocketAddr>() {
        return Some(sock.ip().to_canonical());
    }
    None
}

pub(crate) fn extract_client_ip_from_headers_and_peer(
    headers: &HeaderMap,
    peer_ip: Option<IpAddr>,
    trust_proxy: bool,
) -> Option<IpAddr> {
    let canonical_peer = peer_ip.map(|ip| ip.to_canonical());
    let may_trust_forwarded = trust_proxy || canonical_peer.is_some_and(|ip| ip.is_loopback());

    if may_trust_forwarded {
        if let Some(xff) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
            for candidate in xff.split(',') {
                if let Some(ip) = parse_forwarded_ip_token(candidate) {
                    return Some(ip);
                }
            }
        }
        if let Some(xri) = headers.get("x-real-ip").and_then(|v| v.to_str().ok())
            && let Some(ip) = parse_forwarded_ip_token(xri)
        {
            return Some(ip);
        }
    }

    canonical_peer
}

/// Extracts the canonical client IP from `ConnectInfo<SocketAddr>` (or `ClientIp` extension),
/// honoring `X-Forwarded-For` / `X-Real-IP` only when the peer is loopback or `XMEDIA_TRUST_PROXY=true`.
pub fn extract_client_ip(parts: &Parts) -> Option<IpAddr> {
    if let Some(explicit) = parts.extensions.get::<ClientIp>().and_then(|c| c.0) {
        return Some(explicit.to_canonical());
    }
    let peer_ip = parts
        .extensions
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ci| ci.0.ip())
        .or_else(|| parts.extensions.get::<SocketAddr>().map(SocketAddr::ip));
    extract_client_ip_from_headers_and_peer(&parts.headers, peer_ip, cached_trust_proxy())
}

fn cached_cookie_secure_override() -> Option<bool> {
    static OVERRIDE: std::sync::OnceLock<Option<bool>> = std::sync::OnceLock::new();
    *OVERRIDE.get_or_init(|| {
        let explicit = std::env::var("XMEDIA_COOKIE_SECURE").ok()?;
        let trimmed = explicit.trim();
        if trimmed.eq_ignore_ascii_case("true") || trimmed == "1" {
            Some(true)
        } else if trimmed.eq_ignore_ascii_case("false") || trimmed == "0" {
            Some(false)
        } else {
            None
        }
    })
}

fn cached_trust_proxy() -> bool {
    static TRUST_PROXY: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *TRUST_PROXY.get_or_init(|| {
        std::env::var("XMEDIA_TRUST_PROXY").ok().is_some_and(|v| {
            let t = v.trim();
            t.eq_ignore_ascii_case("true") || t == "1"
        })
    })
}

fn is_https_request(headers: &HeaderMap) -> bool {
    if let Some(explicit) = cached_cookie_secure_override() {
        return explicit;
    }
    if !cached_trust_proxy() {
        return false;
    }
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

fn extract_bearer_token(headers: &HeaderMap) -> Option<String> {
    let raw_auth = headers.get(AUTHORIZATION)?.to_str().ok()?;
    let trimmed_hdr = raw_auth.trim();
    if trimmed_hdr.len() >= 7 && trimmed_hdr[..7].eq_ignore_ascii_case("bearer ") {
        let trimmed = trimmed_hdr[7..].trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    None
}

fn extract_session_cookie_token(headers: &HeaderMap) -> Option<String> {
    for cookie_header in headers.get_all(COOKIE) {
        if let Ok(cookie_str) = cookie_header.to_str() {
            for pair in cookie_str.split(';') {
                if let Some((k, v)) = pair.trim().split_once('=') {
                    let key = k.trim();
                    if key == AUTH_COOKIE_NAME || key == ALT_SESSION_COOKIE_NAME {
                        let val = v.trim();
                        if !val.is_empty() {
                            return Some(val.to_string());
                        }
                    }
                }
            }
        }
    }
    None
}

fn extract_query_token(parts: &Parts) -> Option<String> {
    if matches!(
        parts.method,
        axum::http::Method::GET | axum::http::Method::HEAD
    ) && let Some(query) = parts.uri.query()
    {
        for (k, v) in form_urlencoded::parse(query.as_bytes()) {
            if k == "token" && !v.trim().is_empty() {
                return Some(v.trim().to_string());
            }
        }
    }
    None
}

fn is_state_mutating_method(method: &axum::http::Method) -> bool {
    matches!(
        *method,
        axum::http::Method::POST
            | axum::http::Method::PUT
            | axum::http::Method::PATCH
            | axum::http::Method::DELETE
    )
}

fn parse_origin_tuple(
    raw: &str,
    allow_path_and_query: bool,
) -> Option<(String, String, Option<u16>)> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("null") {
        return None;
    }
    let uri: axum::http::Uri = trimmed.parse().ok()?;
    let scheme = uri.scheme_str()?.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let auth = uri.authority()?;
    if auth.as_str().contains('@') {
        return None;
    }
    if !allow_path_and_query && (!matches!(uri.path(), "" | "/") || uri.query().is_some()) {
        return None;
    }
    let host = auth.host().trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() {
        return None;
    }
    Some((scheme, host, auth.port_u16()))
}

fn resolve_target_host_and_port(parts: &Parts) -> Option<(String, Option<u16>)> {
    if cached_trust_proxy()
        && let Some(fwd_host) = parts
            .headers
            .get("x-forwarded-host")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.split(',').next())
            .map(str::trim)
            .filter(|s| !s.is_empty())
    {
        let auth: axum::http::uri::Authority = fwd_host.parse().ok()?;
        if auth.as_str().contains('@') {
            return None;
        }
        let host = auth.host().trim_end_matches('.').to_ascii_lowercase();
        if !host.is_empty() {
            return Some((host, auth.port_u16()));
        }
    }

    if let Some(raw_host) = parts.headers.get(HOST).and_then(|v| v.to_str().ok()) {
        let trimmed = raw_host.trim();
        if trimmed.is_empty() {
            return None;
        }
        let auth: axum::http::uri::Authority = trimmed.parse().ok()?;
        if auth.as_str().contains('@') {
            return None;
        }
        let host = auth.host().trim_end_matches('.').to_ascii_lowercase();
        if host.is_empty() {
            return None;
        }
        return Some((host, auth.port_u16()));
    }

    let auth = parts.uri.authority()?;
    if auth.as_str().contains('@') {
        return None;
    }
    let host = auth.host().trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() {
        return None;
    }
    Some((host, auth.port_u16()))
}

fn origin_matches_target(
    origin: &(String, String, Option<u16>),
    target: &(String, Option<u16>),
    headers: &HeaderMap,
) -> bool {
    let (origin_scheme, origin_host, origin_port) = origin;
    let (target_host, target_port) = target;
    if origin_host != target_host {
        return false;
    }
    let origin_effective_port =
        origin_port.unwrap_or(if origin_scheme == "https" { 443 } else { 80 });
    if let Some(tp) = *target_port {
        return origin_effective_port == tp;
    }
    if is_https_request(headers) {
        return origin_scheme == "https" && origin_effective_port == 443;
    }
    origin_port.is_none()
}

/// Validates `Sec-Fetch-Site` and `Origin` (with `Referer` fallback) against CSRF on
/// state-mutating (`POST`/`PUT`/`PATCH`/`DELETE`) requests authenticated via session cookie.
pub fn verify_cookie_csrf(parts: &Parts) -> DomainResult<()> {
    if !is_state_mutating_method(&parts.method) {
        return Ok(());
    }

    for sfs_val in parts.headers.get_all("sec-fetch-site") {
        let Ok(raw_sfs) = sfs_val.to_str() else {
            return Err(DomainError::Forbidden(
                "invalid Sec-Fetch-Site header on cookie-authenticated mutation".into(),
            ));
        };
        let sfs = raw_sfs.trim();
        if !sfs.eq_ignore_ascii_case("same-origin") && !sfs.eq_ignore_ascii_case("none") {
            return Err(DomainError::Forbidden(
                "cross-site session cookie request blocked (CSRF)".into(),
            ));
        }
    }

    let origins: Vec<&HeaderValue> = parts.headers.get_all(ORIGIN).iter().collect();
    if !origins.is_empty() {
        if origins.len() > 1 {
            return Err(DomainError::Forbidden(
                "multiple Origin headers rejected on cookie-authenticated mutation".into(),
            ));
        }
        let Some(raw_origin) = origins[0].to_str().ok() else {
            return Err(DomainError::Forbidden(
                "invalid Origin header on cookie-authenticated mutation".into(),
            ));
        };
        let Some(parsed_origin) = parse_origin_tuple(raw_origin, false) else {
            return Err(DomainError::Forbidden(
                "cross-origin session cookie request blocked (CSRF)".into(),
            ));
        };
        let Some(target) = resolve_target_host_and_port(parts) else {
            return Err(DomainError::Forbidden(
                "missing Host header for Origin verification on cookie-authenticated mutation"
                    .into(),
            ));
        };
        if !origin_matches_target(&parsed_origin, &target, &parts.headers) {
            return Err(DomainError::Forbidden(
                "cross-origin session cookie request blocked (CSRF)".into(),
            ));
        }
        return Ok(());
    }

    if let Some(ref_val) = parts.headers.get(REFERER) {
        let Some(raw_ref) = ref_val.to_str().ok() else {
            return Err(DomainError::Forbidden(
                "invalid Referer header on cookie-authenticated mutation".into(),
            ));
        };
        let Some(parsed_ref) = parse_origin_tuple(raw_ref, true) else {
            return Err(DomainError::Forbidden(
                "cross-origin session cookie request blocked (CSRF)".into(),
            ));
        };
        let Some(target) = resolve_target_host_and_port(parts) else {
            return Err(DomainError::Forbidden(
                "missing Host header for Referer verification on cookie-authenticated mutation"
                    .into(),
            ));
        };
        if !origin_matches_target(&parsed_ref, &target, &parts.headers) {
            return Err(DomainError::Forbidden(
                "cross-origin session cookie request blocked (CSRF)".into(),
            ));
        }
    }

    Ok(())
}

pub fn extract_request_token(parts: &Parts) -> Option<String> {
    extract_bearer_token(&parts.headers)
        .or_else(|| extract_session_cookie_token(&parts.headers))
        .or_else(|| extract_query_token(parts))
}

pub async fn authenticate_web_parts(parts: &Parts, auth: &AuthService) -> ApiResult<User> {
    let client_ip = extract_client_ip(parts);
    if let Some(token) = extract_bearer_token(&parts.headers) {
        return Ok(auth.authenticate_jwt(&token).await?);
    }
    if let Some(api_key) = parts.headers.get("x-api-key").and_then(|v| v.to_str().ok()) {
        return Ok(auth
            .authenticate_api_key_with_ip(api_key, client_ip)
            .await?);
    }
    if let Some(cookie_token) = extract_session_cookie_token(&parts.headers) {
        verify_cookie_csrf(parts)?;
        return Ok(auth.authenticate_jwt(&cookie_token).await?);
    }
    if let Some(query_token) = extract_query_token(parts) {
        return Ok(auth.authenticate_jwt(&query_token).await?);
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
    let client_ip = extract_client_ip(parts);
    if let Some(auth_val) = parts.headers.get(AUTHORIZATION)
        && let Ok(auth_str) = auth_val.to_str()
        && let Some(b64) = auth_str
            .strip_prefix("Basic ")
            .or_else(|| auth_str.strip_prefix("basic "))
        && let Ok(decoded_bytes) = BASE64_STANDARD.decode(b64.trim())
        && let Ok(decoded_str) = String::from_utf8(decoded_bytes)
        && let Some((username, password)) = decoded_str.split_once(':')
        && let Ok(user) = auth
            .verify_basic_credentials_with_ip(username, password, client_ip)
            .await
    {
        return Ok(user);
    }

    if let Some(api_key) = parts.headers.get("x-api-key").and_then(|v| v.to_str().ok())
        && let Ok(user) = auth.authenticate_api_key_with_ip(api_key, client_ip).await
    {
        return Ok(user);
    }

    if let Some(query) = parts.uri.query() {
        for (k, v) in form_urlencoded::parse(query.as_bytes()) {
            if (k == "api_key" || k == "apiKey")
                && !v.trim().is_empty()
                && let Ok(user) = auth.authenticate_api_key_with_ip(&v, client_ip).await
            {
                return Ok(user);
            }
        }
    }

    if let Some(token) = extract_request_token(parts) {
        if let Ok(user) = auth.authenticate_jwt(&token).await {
            return Ok(user);
        }
        if let Ok(user) = auth.authenticate_api_key_with_ip(&token, client_ip).await {
            return Ok(user);
        }
    }

    Err(OpdsAuthChallenge)
}

/// Generic Axum Web authentication extractor usable by any domain state implementing
/// `FromRef<S> for Arc<AuthService>`.
pub struct WebAuth(pub User);

impl<S> FromRequestParts<S> for WebAuth
where
    S: Send + Sync,
    Arc<AuthService>: FromRef<S>,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let auth = Arc::<AuthService>::from_ref(state);
        let user = authenticate_web_parts(parts, &auth).await?;
        Ok(Self(user))
    }
}

/// Generic Axum OPDS authentication extractor usable by any domain state implementing
/// `FromRef<S> for Arc<AuthService>`.
pub struct OpdsAuth(pub User);

impl<S> FromRequestParts<S> for OpdsAuth
where
    S: Send + Sync,
    Arc<AuthService>: FromRef<S>,
{
    type Rejection = OpdsAuthChallenge;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let auth = Arc::<AuthService>::from_ref(state);
        let user = authenticate_opds_parts(parts, &auth).await?;
        Ok(Self(user))
    }
}

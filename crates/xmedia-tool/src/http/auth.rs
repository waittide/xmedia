use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use axum::extract::{ConnectInfo, FromRequestParts, Request, State};
use axum::http::header::{AUTHORIZATION, COOKIE, HOST, ORIGIN};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, Uri};
use axum::middleware::Next;
use axum::response::Response;
use reqwest::Url;
use xmedia_core::http::{ApiError, error_json_response};

use crate::http::ToolHttpState;

pub const TOOL_COOKIE_NAME: &str = "xmedia_tool_token";

#[derive(Clone, Debug, Default)]
pub struct ToolRequestSecurityPolicy {
    bind_addr: Option<SocketAddr>,
    cors_allow_origins: Arc<Vec<String>>,
}

impl ToolRequestSecurityPolicy {
    pub fn new(bind_addr: Option<SocketAddr>, cors_allow_origins: &[String]) -> Self {
        let normalized = cors_allow_origins
            .iter()
            .map(|o| o.trim().trim_end_matches('/').to_string())
            .filter(|o| !o.is_empty())
            .collect();
        Self {
            bind_addr,
            cors_allow_origins: Arc::new(normalized),
        }
    }

    pub fn is_explicit_cors_origin(&self, raw_origin: &str) -> bool {
        let trimmed = raw_origin.trim().trim_end_matches('/');
        if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("null") {
            return false;
        }
        self.cors_allow_origins
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(trimmed))
    }

    #[allow(clippy::result_large_err)]
    pub fn validate_request_headers(&self, headers: &HeaderMap) -> Result<(), Response> {
        let loopback_mode = self
            .bind_addr
            .map(|addr| addr.ip().is_loopback())
            .unwrap_or(true);

        let (canonical_host, expected_port) = match self.bind_addr {
            Some(addr) if addr.ip().is_loopback() => {
                let Some(raw_host) = headers.get(HOST).and_then(|v| v.to_str().ok()) else {
                    return Err(error_json_response(
                        StatusCode::FORBIDDEN,
                        "invalid_host",
                        "Missing or invalid Host header on loopback tool endpoint",
                    ));
                };
                let Some((host, port)) = parse_loopback_host_header(raw_host, Some(addr.port()))
                else {
                    return Err(error_json_response(
                        StatusCode::FORBIDDEN,
                        "invalid_host",
                        "Loopback tool endpoints only accept matching loopback Host headers",
                    ));
                };
                (host.to_string(), port)
            }
            Some(_addr) => {
                let Some(raw_host) = headers.get(HOST).and_then(|v| v.to_str().ok()) else {
                    return Err(error_json_response(
                        StatusCode::FORBIDDEN,
                        "invalid_host",
                        "Missing or invalid Host header",
                    ));
                };
                let Some((host, port)) = parse_general_host_header(raw_host) else {
                    return Err(error_json_response(
                        StatusCode::FORBIDDEN,
                        "invalid_host",
                        "Invalid Host header",
                    ));
                };
                (host, port)
            }
            None => {
                if let Some(raw_host_val) = headers.get(HOST) {
                    let Some(raw_host) = raw_host_val.to_str().ok() else {
                        return Err(error_json_response(
                            StatusCode::FORBIDDEN,
                            "invalid_host",
                            "Invalid Host header",
                        ));
                    };
                    let Some((host, port)) = parse_loopback_host_header(raw_host, None) else {
                        return Err(error_json_response(
                            StatusCode::FORBIDDEN,
                            "invalid_host",
                            "Loopback tool endpoints only accept loopback Host headers",
                        ));
                    };
                    (host.to_string(), port)
                } else {
                    ("localhost".to_string(), Some(80))
                }
            }
        };

        let raw_origin_str = headers.get(ORIGIN).and_then(|v| v.to_str().ok());
        let origin_explicitly_allowed =
            raw_origin_str.is_some_and(|raw| self.is_explicit_cors_origin(raw));

        if !origin_explicitly_allowed
            && let Some(sec_fetch_site) =
                headers.get("sec-fetch-site").and_then(|v| v.to_str().ok())
        {
            let sfs = sec_fetch_site.trim();
            if !sfs.eq_ignore_ascii_case("same-origin") && !sfs.eq_ignore_ascii_case("none") {
                return Err(error_json_response(
                    StatusCode::FORBIDDEN,
                    "cross_site_blocked",
                    "Cross-site or cross-port requests to xmedia-tool are forbidden",
                ));
            }
        }

        if !origin_explicitly_allowed && let Some(origin_val) = headers.get(ORIGIN) {
            let valid = origin_val.to_str().ok().is_some_and(|raw| {
                is_allowed_same_origin_header(raw, &canonical_host, expected_port, loopback_mode)
            });
            if !valid {
                return Err(error_json_response(
                    StatusCode::FORBIDDEN,
                    "cross_origin_blocked",
                    "Cross-origin requests to xmedia-tool are forbidden",
                ));
            }
        }

        Ok(())
    }
}

fn extract_bearer_token(headers: &HeaderMap) -> Option<String> {
    let raw_auth = headers.get(AUTHORIZATION).and_then(|h| h.to_str().ok())?;
    let trimmed_hdr = raw_auth.trim();
    if trimmed_hdr.len() >= 7 && trimmed_hdr[..7].eq_ignore_ascii_case("bearer ") {
        let trimmed = trimmed_hdr[7..].trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    None
}

fn extract_cookie_tokens(headers: &HeaderMap) -> Vec<String> {
    let mut out = Vec::new();
    for cookie_header in headers.get_all(COOKIE) {
        if let Ok(cookie_str) = cookie_header.to_str() {
            for pair in cookie_str.split(';') {
                if let Some((k, v)) = pair.trim().split_once('=')
                    && k.trim() == TOOL_COOKIE_NAME
                {
                    let val = v.trim();
                    if !val.is_empty() {
                        out.push(val.to_string());
                    }
                }
            }
        }
    }
    out
}

fn extract_tool_token_from_request(
    method: &Method,
    headers: &HeaderMap,
    uri: &Uri,
) -> Option<String> {
    if let Some(bearer) = extract_bearer_token(headers) {
        return Some(bearer);
    }

    if let Some(cookie_tok) = extract_cookie_tokens(headers).into_iter().next() {
        return Some(cookie_tok);
    }

    // Only accept ?token= query authentication on read-only GET/HEAD asset requests.
    if (*method == Method::GET || *method == Method::HEAD)
        && let Some(query) = uri.query()
    {
        for (k, v) in form_urlencoded::parse(query.as_bytes()) {
            if k == "token" && !v.trim().is_empty() {
                return Some(v.trim().to_string());
            }
        }
    }

    None
}

pub fn extract_tool_token(parts: &Parts) -> Option<String> {
    extract_tool_token_from_request(&parts.method, &parts.headers, &parts.uri)
}

pub fn extract_all_tool_tokens(parts: &Parts) -> Vec<String> {
    let mut tokens = Vec::new();
    if let Some(bearer) = extract_bearer_token(&parts.headers) {
        tokens.push(bearer);
    }
    tokens.extend(extract_cookie_tokens(&parts.headers));
    tokens
}

pub fn build_tool_session_cookie(token: &str, max_age_secs: u64) -> HeaderValue {
    let raw = format!(
        "{TOOL_COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={max_age_secs}"
    );
    HeaderValue::from_str(&raw).unwrap_or_else(|_| HeaderValue::from_static(""))
}

pub fn build_tool_clear_cookie() -> HeaderValue {
    HeaderValue::from_static("xmedia_tool_token=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0")
}

pub fn parse_loopback_host_header(
    raw_host: &str,
    expected_port: Option<u16>,
) -> Option<(&'static str, Option<u16>)> {
    let trimmed = raw_host.trim().to_ascii_lowercase();
    match expected_port {
        Some(port) => {
            if port == 80 {
                match trimmed.as_str() {
                    "127.0.0.1" => return Some(("127.0.0.1", Some(80))),
                    "localhost" => return Some(("localhost", Some(80))),
                    "[::1]" => return Some(("[::1]", Some(80))),
                    _ => {}
                }
            }
            let expected_suffix = format!(":{port}");
            let host_part = trimmed.strip_suffix(&expected_suffix)?;
            match host_part {
                "127.0.0.1" => Some(("127.0.0.1", Some(port))),
                "localhost" => Some(("localhost", Some(port))),
                "[::1]" => Some(("[::1]", Some(port))),
                _ => None,
            }
        }
        None => match trimmed.as_str() {
            "127.0.0.1" => Some(("127.0.0.1", Some(80))),
            "localhost" => Some(("localhost", Some(80))),
            "[::1]" => Some(("[::1]", Some(80))),
            _ => {
                let (host_part, port_str) = trimmed.rsplit_once(':')?;
                let port = port_str.parse::<u16>().ok().filter(|&p| p > 0)?;
                match host_part {
                    "127.0.0.1" => Some(("127.0.0.1", Some(port))),
                    "localhost" => Some(("localhost", Some(port))),
                    "[::1]" => Some(("[::1]", Some(port))),
                    _ => None,
                }
            }
        },
    }
}

fn parse_general_host_header(raw_host: &str) -> Option<(String, Option<u16>)> {
    let trimmed = raw_host.trim();
    if trimmed.is_empty()
        || trimmed.contains('/')
        || trimmed.contains('@')
        || trimmed.contains('?')
        || trimmed.contains('#')
        || trimmed.bytes().any(|b| b.is_ascii_whitespace())
    {
        return None;
    }
    let parsed = Url::parse(&format!("http://{trimmed}")).ok()?;
    if parsed.path() != "/" || !parsed.username().is_empty() || parsed.password().is_some() {
        return None;
    }
    let host = parsed.host_str()?.to_ascii_lowercase();
    Some((host, parsed.port()))
}

pub fn is_allowed_same_origin_header(
    raw_origin: &str,
    canonical_host: &str,
    expected_port: Option<u16>,
    loopback_only_http: bool,
) -> bool {
    let Ok(parsed) = Url::parse(raw_origin.trim()) else {
        return false;
    };
    if loopback_only_http {
        if parsed.scheme() != "http" {
            return false;
        }
    } else if !matches!(parsed.scheme(), "http" | "https") {
        return false;
    }
    if !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return false;
    }
    if let Some(port) = expected_port
        && parsed.port_or_known_default() != Some(port)
    {
        return false;
    }
    if expected_port.is_none() && parsed.port().is_some() {
        return false;
    }
    parsed
        .host_str()
        .is_some_and(|h| h.eq_ignore_ascii_case(canonical_host))
}

/// Anti-DNS-rebinding and anti-CSRF middleware enforcing `Host`, `Sec-Fetch-Site`, and `Origin`
/// checks across `xmedia-tool` HTTP endpoints (matching `apps/xmedia-app/src/gateway/security.rs`).
pub async fn guard_tool_http_request(
    State(state): State<ToolHttpState>,
    req: Request,
    next: Next,
) -> Response {
    if let Err(resp) = state.security.validate_request_headers(req.headers()) {
        return resp;
    }
    next.run(req).await
}

/// Axum middleware enforcing `ToolAuthService` across all protected workbench API sub-routes.
pub async fn require_tool_auth(
    State(state): State<ToolHttpState>,
    req: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let provided = extract_tool_token_from_request(req.method(), req.headers(), req.uri());
    state.auth.verify_token(provided.as_deref())?;
    Ok(next.run(req).await)
}

pub struct ToolClientIp(pub Option<IpAddr>);

impl<S> FromRequestParts<S> for ToolClientIp
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let ip = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|ci| ci.0.ip().to_canonical());
        Ok(Self(ip))
    }
}

pub struct ToolAuthGuard;

impl FromRequestParts<ToolHttpState> for ToolAuthGuard {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &ToolHttpState,
    ) -> Result<Self, Self::Rejection> {
        let provided = extract_tool_token(parts);
        state.auth.verify_token(provided.as_deref())?;
        Ok(Self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_host_and_origin_guards_reject_rebinding_and_cross_port_origins() {
        assert_eq!(
            parse_loopback_host_header("127.0.0.1:8091", Some(8091)),
            Some(("127.0.0.1", Some(8091)))
        );
        assert_eq!(
            parse_loopback_host_header("localhost:8091", Some(8091)),
            Some(("localhost", Some(8091)))
        );
        assert_eq!(
            parse_loopback_host_header("[::1]:8091", Some(8091)),
            Some(("[::1]", Some(8091)))
        );
        assert_eq!(
            parse_loopback_host_header("127.0.0.1:9999", Some(8091)),
            None
        );
        assert_eq!(
            parse_loopback_host_header("evil.rebind.test:8091", Some(8091)),
            None
        );

        assert!(is_allowed_same_origin_header(
            "http://127.0.0.1:8091",
            "127.0.0.1",
            Some(8091),
            true
        ));
        assert!(!is_allowed_same_origin_header(
            "http://127.0.0.1:3000",
            "127.0.0.1",
            Some(8091),
            true
        ));
        assert!(!is_allowed_same_origin_header(
            "http://localhost:8091",
            "127.0.0.1",
            Some(8091),
            true
        ));
        assert!(!is_allowed_same_origin_header(
            "https://evil.example",
            "127.0.0.1",
            Some(8091),
            true
        ));
        assert!(!is_allowed_same_origin_header(
            "null",
            "127.0.0.1",
            Some(8091),
            true
        ));
    }
}

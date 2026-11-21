use axum::extract::{FromRequestParts, Request, State};
use axum::http::header::{AUTHORIZATION, COOKIE};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, Method, Uri};
use axum::middleware::Next;
use axum::response::Response;
use xmedia_core::http::ApiError;

use crate::http::ToolHttpState;

pub const TOOL_COOKIE_NAME: &str = "xmedia_tool_token";

fn extract_tool_token_from_request(
    method: &Method,
    headers: &HeaderMap,
    uri: &Uri,
) -> Option<String> {
    if let Some(raw_auth) = headers.get(AUTHORIZATION).and_then(|h| h.to_str().ok())
        && let Some(bearer) = raw_auth.strip_prefix("Bearer ")
    {
        let trimmed = bearer.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    for cookie_header in headers.get_all(COOKIE) {
        if let Ok(cookie_str) = cookie_header.to_str() {
            for pair in cookie_str.split(';') {
                if let Some((k, v)) = pair.trim().split_once('=')
                    && k.trim() == TOOL_COOKIE_NAME
                {
                    let val = v.trim();
                    if !val.is_empty() {
                        return Some(val.to_string());
                    }
                }
            }
        }
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

pub fn build_tool_session_cookie(token: &str) -> HeaderValue {
    let raw =
        format!("{TOOL_COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age=604800");
    HeaderValue::from_str(&raw).unwrap_or_else(|_| HeaderValue::from_static(""))
}

pub fn build_tool_clear_cookie() -> HeaderValue {
    HeaderValue::from_static("xmedia_tool_token=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0")
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

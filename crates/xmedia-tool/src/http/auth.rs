use axum::extract::FromRequestParts;
use axum::http::HeaderValue;
use axum::http::header::{AUTHORIZATION, COOKIE};
use axum::http::request::Parts;
use xmedia_core::http::ApiError;

use crate::http::ToolHttpState;

pub const TOOL_COOKIE_NAME: &str = "xmedia_tool_token";

pub fn extract_tool_token(parts: &Parts) -> Option<String> {
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

    if let Some(query) = parts.uri.query() {
        for (k, v) in form_urlencoded::parse(query.as_bytes()) {
            if k == "token" && !v.trim().is_empty() {
                return Some(v.trim().to_string());
            }
        }
    }

    None
}

pub fn build_tool_session_cookie(token: &str) -> HeaderValue {
    let raw = format!("{TOOL_COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age=604800");
    HeaderValue::from_str(&raw).unwrap_or_else(|_| HeaderValue::from_static(""))
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

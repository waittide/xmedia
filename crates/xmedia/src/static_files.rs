use std::path::{Component, Path, PathBuf};

use axum::body::Body;
use axum::extract::State;
use axum::http::header::{
    CACHE_CONTROL, CONTENT_SECURITY_POLICY, CONTENT_TYPE, REFERRER_POLICY, X_CONTENT_TYPE_OPTIONS,
    X_FRAME_OPTIONS,
};
use axum::http::{HeaderValue, Method, StatusCode, Uri};
use axum::response::{Html, IntoResponse, Response};
use rust_embed::RustEmbed;

use crate::gateway::GatewayState;

const SPA_HTML_CSP: &str = "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; worker-src 'self' blob:; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob: https:; media-src 'self' blob: data:; font-src 'self' data: blob:; connect-src 'self'; frame-src 'self' blob:; object-src 'none'; base-uri 'self'; frame-ancestors 'self'";

#[derive(RustEmbed)]
#[folder = "../../web/dist"]
#[allow_missing = true]
struct EmbeddedWebAssets;

fn normalize_asset_path(uri_path: &str) -> Option<PathBuf> {
    let trimmed = uri_path.trim_start_matches('/');
    let path = Path::new(trimmed);
    let mut clean = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::Normal(part) => clean.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(clean)
}

fn is_vite_hashed_asset(rel_str: &str) -> bool {
    let Some(file_name) = rel_str.strip_prefix("assets/") else {
        return false;
    };
    let stem = file_name
        .rsplit_once('.')
        .map(|(s, _)| s)
        .unwrap_or(file_name);
    let bytes = stem.as_bytes();
    bytes.len() >= 9
        && bytes[bytes.len() - 9] == b'-'
        && bytes[bytes.len() - 8..]
            .iter()
            .all(|&b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn asset_response(path_hint: &str, body: impl Into<Body>, immutable: bool) -> Response {
    let mime = mime_guess::from_path(path_hint).first_or_octet_stream();
    let mut res = (StatusCode::OK, body.into()).into_response();
    let headers = res.headers_mut();
    if let Ok(ct) = HeaderValue::from_str(mime.as_ref()) {
        headers.insert(CONTENT_TYPE, ct);
    }
    let cache = if immutable {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    headers.insert(CACHE_CONTROL, HeaderValue::from_static(cache));
    headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    headers.insert(REFERRER_POLICY, HeaderValue::from_static("same-origin"));
    headers.insert(X_FRAME_OPTIONS, HeaderValue::from_static("SAMEORIGIN"));
    if mime.type_() == "text" && mime.subtype() == "html" {
        headers.insert(
            CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(SPA_HTML_CSP),
        );
    }
    res
}

async fn read_contained_external_file(ext_dir: &Path, rel_path: &Path) -> Option<Vec<u8>> {
    let ext_dir = ext_dir.to_path_buf();
    let candidate = ext_dir.join(rel_path);
    tokio::task::spawn_blocking(move || {
        let canon_root = std::fs::canonicalize(&ext_dir).ok()?;
        let canon_file = std::fs::canonicalize(&candidate).ok()?;
        if !canon_file.starts_with(&canon_root) || !canon_file.is_file() {
            return None;
        }
        std::fs::read(&canon_file).ok()
    })
    .await
    .ok()
    .flatten()
}

pub async fn serve_spa(State(state): State<GatewayState>, method: Method, uri: Uri) -> Response {
    let req_path = uri.path();
    if req_path == "/api"
        || req_path.starts_with("/api/")
        || req_path == "/opds"
        || req_path.starts_with("/opds/")
        || req_path.starts_with("/rest/")
        || req_path == "/rest"
        || req_path == "/jellyfin"
        || req_path.starts_with("/jellyfin/")
        || req_path.starts_with("/_app/")
        || req_path == "/_app"
    {
        return (StatusCode::NOT_FOUND, "Not Found").into_response();
    }

    if method != Method::GET && method != Method::HEAD {
        return (StatusCode::METHOD_NOT_ALLOWED, "Method Not Allowed").into_response();
    }

    let rel_opt = normalize_asset_path(req_path);
    let rel_str = rel_opt
        .as_ref()
        .map(|p| {
            p.components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/")
        })
        .unwrap_or_default();

    let is_hashed_asset = is_vite_hashed_asset(&rel_str);

    if let Some(ref ext_dir) = state.external_web_dir {
        if let Some(ref rel_path) = rel_opt
            && !rel_str.is_empty()
            && let Some(bytes) = read_contained_external_file(ext_dir, rel_path).await
        {
            return asset_response(&rel_str, bytes, is_hashed_asset);
        }
        if rel_str.starts_with("assets/") {
            return (StatusCode::NOT_FOUND, "Not Found").into_response();
        }
        if let Some(bytes) = read_contained_external_file(ext_dir, Path::new("index.html")).await {
            return asset_response("index.html", bytes, false);
        }
    }

    if !rel_str.is_empty()
        && let Some(embedded) = EmbeddedWebAssets::get(&rel_str)
    {
        return asset_response(&rel_str, embedded.data, is_hashed_asset);
    }

    if rel_str.starts_with("assets/") {
        return (StatusCode::NOT_FOUND, "Not Found").into_response();
    }

    if let Some(index) = EmbeddedWebAssets::get("index.html") {
        return asset_response("index.html", index.data, false);
    }

    let mut res = Html(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>xmedia</title></head>\
         <body><h1>xmedia server is running</h1><p>Unified Web SPA assets are not built yet.</p></body></html>",
    )
    .into_response();
    let headers = res.headers_mut();
    headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    headers.insert(REFERRER_POLICY, HeaderValue::from_static("same-origin"));
    headers.insert(X_FRAME_OPTIONS, HeaderValue::from_static("SAMEORIGIN"));
    headers.insert(
        CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(SPA_HTML_CSP),
    );
    res
}

#[cfg(test)]
mod tests {
    use super::is_vite_hashed_asset;

    #[test]
    fn vite_hashed_asset_detects_hashes_with_hyphens_and_multidot_names() {
        assert!(is_vite_hashed_asset("assets/index-CdG13ci-.js"));
        assert!(is_vite_hashed_asset("assets/BookDetailView-Ck-3nOyq.js"));
        assert!(is_vite_hashed_asset("assets/en-US-DNWpP-0k.js"));
        assert!(is_vite_hashed_asset("assets/vue-vendor-CQq8P3gb.js"));
        assert!(is_vite_hashed_asset("assets/pdf.worker.min-CIqJB0yW.mjs"));
        assert!(!is_vite_hashed_asset("assets/my-very-long-asset.js"));
        assert!(!is_vite_hashed_asset("assets/my-component.js"));
        assert!(!is_vite_hashed_asset("assets/apple-touch-icon.png"));
        assert!(!is_vite_hashed_asset("assets/custom-stylesheet.css"));
        assert!(!is_vite_hashed_asset("assets/plain.js"));
        assert!(!is_vite_hashed_asset("assets/foo-bar.js"));
        assert!(!is_vite_hashed_asset("index.html"));
    }
}
